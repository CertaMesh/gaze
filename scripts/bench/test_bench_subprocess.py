"""Synthetic process falsifiers; run --mutation-proof for the targeted kill set."""
from __future__ import annotations

import contextlib
import dataclasses
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import tempfile
import time
import traceback
import tracemalloc
import unittest
from unittest import mock

import bench_subprocess as transport
import gaze_bench_score as score

CANARY = "synthetic-private-canary"
HERE = Path(__file__).resolve()


def limits(**changes):
    return dataclasses.replace(transport.TransportLimits(
        request_bytes=512 * 1024, stdout_bytes=512 * 1024, stderr_bytes=512 * 1024,
        chunk_bytes=1024, handshake_seconds=0.6, exchange_seconds=0.6,
        invocation_seconds=4, finish_seconds=0.25, terminate_seconds=0.1,
        reap_seconds=0.3,
    ), **changes)


def command(mode):
    return [sys.executable, str(HERE), "--child", mode]


def child(mode):
    def emit(value):
        os.write(1, json.dumps(value, ensure_ascii=False).encode() + b"\n")

    if mode == "early":
        os.write(2, CANARY.encode())
        return 7
    if mode == "bad-handshake":
        emit({CANARY: CANARY})
        return 0
    if mode.startswith("probe"):
        emit({"schema_version": score.VALIDATOR_PROBE_PROTOCOL_SCHEMA_VERSION,
              "validator_kinds_by_class": {"email": ["synthetic"]},
              "validator_recognizers": [{"id": "synthetic", "class": "email",
                                         "validator_kind": "synthetic"}]})
    if mode in ("ignore", "finish-hang"):
        signal.signal(signal.SIGTERM, signal.SIG_IGN)
    if mode in ("blocked-write", "trickle", "silent", "no-newline"):
        emit({"ready": True})
    if mode == "blocked-write":
        for _ in range(1024):
            os.write(2, b"x" * 1024)
        time.sleep(2)
        return 0
    if mode == "bad-json-handshake":
        os.write(1, b"{bad}\n")
        return 0
    if mode == "prefix-idle":
        os.write(1, b'{"x":')
        time.sleep(1.5)
        return 0
    if mode == "prefix":
        # Unsolicited stdout only after the request write has started, never finished.
        os.read(0, 4096)
        os.write(1, b'{"x":')
        while not os.read(0, 65536).endswith(b"\n"):
            pass
        os.write(1, b"1}\n")
        return 0
    for line in sys.stdin.buffer:
        request = json.loads(line)
        if mode in ("flood", "flood-excess"):
            for _ in range(128):
                os.write(2, (CANARY * 100).encode())
        if mode == "early-response":
            os.write(2, request.get("text", CANARY).encode())
            return 8
        if mode == "silent":
            time.sleep(1.2)
            emit(request)
        elif mode == "trickle":
            for byte in b'{"x":1}\n':
                time.sleep(0.12)
                os.write(1, bytes([byte]))
        elif mode in ("ignore", "finish-hang"):
            emit(request)
            time.sleep(1.5)
            return 0
        elif mode == "inherit":
            if os.fork() == 0:
                time.sleep(0.7)
                os._exit(0)
            emit(request)
            return 0
        elif mode == "split":
            for byte in json.dumps(request, ensure_ascii=False).encode() + b"\n":
                os.write(1, bytes([byte]))
        elif mode == "raw":
            os.write(1, bytes.fromhex(request["wire"]))
            return 0
        elif mode == "no-newline":
            os.write(1, b'{"x":1}')
            time.sleep(1.2)
        elif mode.startswith("probe"):
            emit({"fixture_id": request["fixture_id"] if mode == "probe" else CANARY,
                  "gold_validation": [],
                  "predictions": {"validator_backed": [], "shape_only": []}})
        elif mode.startswith("score"):
            from test_openpii_gaze_bench import ResponseValidationTests
            response = ResponseValidationTests().success_response()
            response["fixture_id"] = request["fixture_id"]
            response["clean_text"] = request["text"]
            if mode == "score-bad":
                response[CANARY] = CANARY
            elif mode == "score-reason":
                response = {"fixture_id": request["fixture_id"],
                            "pipeline_error_code": CANARY, "pipeline_error_stage": "clean",
                            "timing": {"total_ms": 1.0}}
            elif mode == "score-refusal":
                response = {"fixture_id": request["fixture_id"],
                            "pipeline_error_code": "safety_net_fallback_residual_suspect",
                            "pipeline_error_stage": "clean", "timing": {"total_ms": 1.0}}
            os.write(2, request["text"].encode())
            emit(response)
        else:
            emit(request)
        if mode == "nonzero":
            return 9
    if mode == "exit-flood":
        for _ in range(16):
            os.write(2, CANARY.encode() * 512)
    if mode == "exit-extra":
        os.write(1, b"{}\n")
    return 0


def invoke(mode, root, *, late=False, post=False, owners=None):
    created = owners if owners is not None else []
    def factory(*args, **kwargs):
        owner = transport.BenchSubprocess(command(mode), cwd=root, limits=limits())
        created.append(owner)
        return owner
    from test_openpii_gaze_bench import ResponseValidationTests
    document = dataclasses.replace(ResponseValidationTests().document(), text=CANARY)
    with mock.patch.object(score, "BenchSubprocess", side_effect=factory):
        if mode.startswith("probe") or mode in ("bad-handshake", "early"):
            result = score.collect_validator_measurements(Path("synthetic"), [document], [document.uid])
        else:
            calls = 0
            original_add = score.MetricAccumulator.add
            def bad_add(accumulator, *args, **kwargs):
                nonlocal calls
                calls += 1
                if calls == 1:
                    return original_add(accumulator, *args, **kwargs)
                raise ValueError(CANARY)
            def bad(*args, **kwargs):
                raise ValueError(CANARY)
            with contextlib.ExitStack() as stack:
                if late:
                    stack.enter_context(mock.patch.object(score.MetricAccumulator, "add", new=bad_add))
                if post:
                    stack.enter_context(mock.patch.object(score, "identified_document_population", side_effect=bad))
                result = score.run_config(
                    root, Path("synthetic"), "rule-floor-extended", [document], root, root,
                    None, None, None, 0.3, root / "diagnostics", warmup_count=1,
                )
    return result, created


SUCCESS_MODES = ("score", "score-refusal", "probe")


def assert_success_counts(mode, result):
    """A tolerated failure is not a pass: pin what each success actually produced."""
    from test_openpii_gaze_bench import ResponseValidationTests
    uid = ResponseValidationTests().document().uid
    if mode == "probe":
        assert result["schema_version"] == score.VALIDATOR_PROBE_PROTOCOL_SCHEMA_VERSION, "success-required"
        assert list(result["documents"]) == [uid], "success-required"
        return
    process_meta, availability = result["process"], result["pipeline_availability"]
    refused = mode == "score-refusal"
    assert process_meta["warmup_count"] == 1, "success-required"
    assert len(process_meta["discarded_warmup_samples"]) == 1, "success-required"
    assert availability["attempted_documents"] == 1, "success-required"
    assert availability["completed_documents"] == (0 if refused else 1), "success-required"
    assert availability["failed_closed_documents"] == (1 if refused else 0), "success-required"
    assert result["scored_population"]["documents"] == (0 if refused else 1), "success-required"
    assert result["failed_closed_population"]["documents"] == (1 if refused else 0), "success-required"
    assert availability["errors"] == (
        {"safety_net_fallback_residual_suspect": 1} if refused else {}
    ), "success-required"


def boundary_scenario(mode, root):
    owners = []
    try:
        _boundary_scenario(mode, root, owners)
    finally:
        # Assert first; this independent emergency reaper must not hide a mutant.
        for owner in owners:
            if owner.process is not None:
                if owner.process.poll() is None:
                    owner.process.kill()
                owner.process.wait(timeout=1)
                for stream in (owner.process.stdin, owner.process.stdout, owner.process.stderr):
                    stream.close()
            if owner.selector is not None:
                owner.selector.close()


def _boundary_scenario(mode, root, owners):
    os.chdir(root)
    late = mode == "late"
    post = mode == "post"
    actual_mode = "score" if late or post else mode
    error = None
    result = None
    try:
        result, _ = invoke(actual_mode, root, late=late, post=post, owners=owners)
        process_meta = result.get("process", {})
        assert "stderr_log" not in process_meta and "stderr_bytes" not in process_meta, "metadata-boundary"
        assert CANARY not in repr(result), "result-boundary"
    except transport.ProducerFailure as caught:
        assert caught.__context__ is None and caught.__cause__ is None, "exception-context"
        assert CANARY not in str(caught) and CANARY not in repr(caught), "exception-boundary"
        rendered = "".join(traceback.format_exception(caught))
        assert CANARY not in rendered, "traceback-boundary"
        error = (caught.code, caught.phase)
    assert owners, "caller-reaping"
    for owner in owners:
        assert owner.process.returncode is not None, "caller-reaping"
        assert all(stream.closed for stream in
                   (owner.process.stdin, owner.process.stdout, owner.process.stderr)), "caller-fd-close"
        try:
            os.waitpid(owner.process.pid, os.WNOHANG)
        except ChildProcessError:
            pass
        else:
            raise AssertionError("caller-reaping")
    assert not list(root.iterdir()), "file-boundary"
    if mode in SUCCESS_MODES:
        assert error is None, "success-required"
        assert_success_counts(mode, result)
    else:
        assert error is not None, "failure-required"
    if mode == "uncaught":
        invoke("score-bad", root, owners=owners)


@unittest.skipUnless(os.name == "posix" and sys.platform in ("darwin", "linux"), "POSIX transport")
class TransportTests(unittest.TestCase):
    def owner(self, mode="echo", **changes):
        owner = transport.BenchSubprocess(command(mode), limits=limits(**changes))
        def fixture_cleanup():
            # Mutation probes must not disable the test harness's own reaper.
            if owner.process is not None:
                if owner.process.poll() is None:
                    owner.process.kill()
                owner.process.wait(timeout=1)
                for stream in (owner.process.stdin, owner.process.stdout, owner.process.stderr):
                    stream.close()
            if owner.selector is not None:
                owner.selector.close()
        self.addCleanup(fixture_cleanup)
        return owner

    def reaped(self, owner):
        self.assertTrue(owner.process.returncode is not None, "child-reaping")
        self.assertTrue(all(stream.closed for stream in
                            (owner.process.stdin, owner.process.stdout, owner.process.stderr)), "fd-close")
        with self.assertRaises(ChildProcessError, msg="child-reaping"):
            os.waitpid(owner.process.pid, os.WNOHANG)

    def test_normal_multiple_split_unicode(self):
        owner = self.owner("split")
        with owner:
            for index in range(3):
                value = {"text": CANARY + "ä", "nested": [index, True, None, {"q": "\\\""}]}
                self.assertTrue(owner.exchange(value) == value, "normal-exchange")
        self.reaped(owner)

    def test_limits_and_platform_reject_before_spawn(self):
        cases = ({"request_bytes": 0}, {"exchange_seconds": float("nan")},
                 {"finish_seconds": float("inf")}, {"nesting": 65},
                 {"chunk_bytes": 600000}, {"handshake_seconds": 5}, {"chunk_bytes": True},
                 {"terminate_seconds": 0}, {"reap_seconds": float("nan")})
        for changes in cases:
            with self.subTest(changes=tuple(changes)), mock.patch.object(transport.subprocess, "Popen") as spawn:
                with self.assertRaises(transport.ProducerFailure):
                    with self.owner(**changes):
                        pass
                spawn.assert_not_called()
        with mock.patch.object(transport.sys, "platform", "unsupported"), mock.patch.object(transport.subprocess, "Popen") as spawn:
            with self.assertRaises(transport.ProducerFailure):
                with self.owner():
                    pass
            spawn.assert_not_called()

    def test_request_cap_and_preallocation(self):
        owner = self.owner(request_bytes=1024)
        with owner:
            self.assertTrue(owner.exchange({"x": "a" * 1015}) == {"x": "a" * 1015}, "request-cap")
            tracemalloc.start()
            try:
                huge = "x" * 2_000_000
                before = tracemalloc.get_traced_memory()[0]
                with self.assertRaises(transport.ProducerFailure) as failure:
                    owner.exchange({"x": huge})
                peak = tracemalloc.get_traced_memory()[1] - before
                self.assertTrue(peak < 200_000, "request-preallocation")
                self.assertEqual(failure.exception.code, "input_limit")
            finally:
                tracemalloc.stop()
        self.reaped(owner)

    def test_frame_cap_and_truncated_prefix(self):
        wire = b'{"x":"' + b'a' * 1015 + b'"}\n'
        for payload, cap, succeeds in ((wire, len(wire), True), (wire, len(wire)-1, False),
                                       (b'{"x":1}\n' + b'a' * 1024, 1024, False)):
            owner = self.owner("raw", stdout_bytes=cap, chunk_bytes=128)
            if succeeds:
                with owner:
                    self.assertTrue(owner.exchange({"wire": payload.hex()}) == {"x": "a" * 1015}, "frame-cap")
            else:
                with self.assertRaises(transport.ProducerFailure, msg="no-truncated-prefix"):
                    with owner:
                        owner.exchange({"wire": payload.hex()})
            self.reaped(owner)

    def test_malformed_extra_partial_frames(self):
        frames = [b'{"x":1}', b'{bad}\n', b'{"x":"\xff"}\n', b'{"x":1,"x":2}\n',
                  b'{"x":NaN}\n', b'{"x":1e999}\n', b'[]\n', b'{}\n{}\n',
                  b'{"x":' + b'[' * 65 + b'0' + b']' * 65 + b'}\n']
        for index, wire in enumerate(frames):
            with self.subTest(case=index):
                owner = self.owner("raw")
                with self.assertRaises(transport.ProducerFailure, msg="frame-refusal"):
                    with owner:
                        owner.exchange({"wire": wire.hex()})
                self.reaped(owner)

    def test_stderr_flood_success_bounded_memory(self):
        owner = self.owner("flood")
        tracemalloc.start()
        try:
            with owner:
                self.assertTrue(owner.exchange({"x": 1}) == {"x": 1}, "flood-exchange")
            self.assertTrue(tracemalloc.get_traced_memory()[1] < 2_000_000, "stderr-memory")
        finally:
            tracemalloc.stop()
        self.reaped(owner)

    def test_stderr_budget(self):
        owner = self.owner("flood-excess", stderr_bytes=4096)
        with self.assertRaises(transport.ProducerFailure, msg="stderr-budget") as failure:
            with owner:
                owner.exchange({"x": 1})
        self.assertEqual(failure.exception.code, "stderr_limit", "stderr-budget")
        self.reaped(owner)

    def test_write_blocking_and_absolute_deadlines(self):
        for mode in ("blocked-write", "trickle", "silent", "no-newline"):
            owner = self.owner(mode, exchange_seconds=0.25, stderr_bytes=2 * 1024 * 1024)
            started = time.monotonic()
            with self.assertRaises(transport.ProducerFailure, msg="absolute-deadline") as failure:
                with owner:
                    self.assertTrue(owner.receive_handshake() == {"ready": True}, "child-ready")
                    started = time.monotonic()
                    owner.exchange({"text": "x" * 400_000})
            self.assertEqual(failure.exception.code, "deadline", "absolute-deadline")
            self.assertTrue(time.monotonic() - started < 1.1, "absolute-deadline")
            self.reaped(owner)

    def test_handshake_early_failure(self):
        owner = self.owner("early")
        with self.assertRaises(transport.ProducerFailure):
            with owner:
                owner.receive_handshake()
        self.reaped(owner)

    def test_finish_nonzero_and_ignored_termination(self):
        for mode in ("nonzero", "ignore"):
            owner = self.owner(mode)
            start = time.monotonic()
            with self.assertRaises(transport.ProducerFailure, msg="finish-failure"):
                with owner:
                    owner.exchange({"x": 1})
            self.assertTrue(time.monotonic() - start < 1, "cleanup-deadline")
            self.reaped(owner)
            if mode == "ignore":
                self.assertEqual(owner.process.returncode, -signal.SIGKILL, "kill-escalation")

    def test_inherited_writer_and_unrelated_sentinel(self):
        sentinel = subprocess.Popen([sys.executable, "-c", "import time; time.sleep(3)"],
                                    stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        try:
            owner = self.owner("inherit")
            started = time.monotonic()
            with owner:
                owner.exchange({"x": 1})
            self.assertTrue(time.monotonic() - started < 0.6, "inherited-writer-deadline")
            self.reaped(owner)
            self.assertTrue(sentinel.poll() is None, "unrelated-process")
        finally:
            sentinel.terminate()
            sentinel.wait(timeout=1)

    def test_cleanup_on_processing_exception(self):
        owner = self.owner("ignore")
        with self.assertRaises(ValueError):
            with owner:
                owner.exchange({"x": 1})
                raise ValueError("synthetic processing failure")
        self.reaped(owner)

    def test_shutdown_output_and_stderr_budget(self):
        for mode, expected in (("exit-flood", "stderr_limit"), ("exit-extra", "protocol")):
            owner = self.owner(mode, stderr_bytes=4096)
            with self.assertRaises(transport.ProducerFailure) as failure:
                with owner:
                    owner.exchange({"x": 1})
            self.assertEqual(failure.exception.code, expected, "shutdown-drain")
            self.reaped(owner)

    def test_invocation_and_parse_deadline(self):
        owner = self.owner(invocation_seconds=0.6)
        with self.assertRaises(transport.ProducerFailure) as failure:
            with owner:
                owner.exchange({"x": 1})
                time.sleep(0.65)
                owner.exchange({"x": 2})
        self.assertEqual(failure.exception.code, "deadline", "invocation-deadline")
        self.reaped(owner)
        owner = self.owner()
        with owner:
            owner.exchange({"x": 1})
            original = transport.json.loads
            def slow(*args, **kwargs):
                time.sleep(0.65)
                return original(*args, **kwargs)
            with mock.patch.object(transport.json, "loads", side_effect=slow):
                with self.assertRaises(transport.ProducerFailure) as failure:
                    owner.exchange({"x": 2})
            self.assertEqual(failure.exception.code, "deadline", "decode-deadline")
        self.reaped(owner)

    def test_cleanup_drain_failure_still_reaps(self):
        owner = self.owner("ignore")
        with self.assertRaises(transport.ProducerFailure) as failure:
            with owner:
                owner.exchange({"x": 1})
                with mock.patch.object(owner, "_wait_exit", side_effect=OSError(CANARY)):
                    self.assertFalse(owner._cleanup(), "cleanup-report")
                    raise transport.ProducerFailure("cleanup", "cleanup")
        self.assertEqual(failure.exception.code, "cleanup")
        self.reaped(owner)

    def test_boundary_no_exception_context_in_process(self):
        with tempfile.TemporaryDirectory() as root:
            for mode in ("score-bad", "score-reason", "probe-bad", "bad-handshake"):
                with self.assertRaises(transport.ProducerFailure) as failure:
                    invoke(mode, Path(root))
                error = failure.exception
                self.assertTrue(error.__context__ is None and error.__cause__ is None, "exception-context")
                self.assertTrue(CANARY not in repr(error), "exception-boundary")

    def test_cancelled_payload_and_unknown_population_are_closed(self):
        owners = []
        def factory(*args, **kwargs):
            owner = transport.BenchSubprocess(command("probe"), limits=limits())
            owners.append(owner)
            return owner
        with tempfile.TemporaryDirectory() as root:
            with mock.patch.object(score, "BenchSubprocess") as spawn:
                with self.assertRaises(transport.ProducerFailure) as failure:
                    score.collect_validator_measurements(Path(root), [], [CANARY])
                spawn.assert_not_called()
                self.assertTrue(failure.exception.__context__ is None, "exception-context")
            with mock.patch.object(score, "BenchSubprocess", side_effect=factory), mock.patch.object(
                score, "_validate_validator_probe_handshake", side_effect=KeyboardInterrupt(CANARY)
            ):
                with self.assertRaises(transport.ProducerFailure) as failure:
                    score.collect_validator_measurements(Path(root), [], [])
            self.assertEqual(failure.exception.code, "cancelled", "cancelled-boundary")
            self.assertTrue(failure.exception.__context__ is None and failure.exception.__cause__ is None, "exception-context")
            self.reaped(owners[0])

    def test_boundary_success_and_failure_sinks(self):
        for mode in ("score", "score-refusal", "probe", "early", "bad-handshake",
                     "probe-bad", "score-bad", "score-reason", "late", "post", "uncaught"):
            with self.subTest(mode=mode), tempfile.TemporaryDirectory() as root:
                result = subprocess.run([sys.executable, str(HERE), "--boundary", mode, root],
                                        capture_output=True, timeout=5)
                self.assertTrue(CANARY.encode() not in result.stdout + result.stderr, "output-canary")
                self.assertTrue(not list(Path(root).iterdir()), "file-boundary")
                if mode == "uncaught":
                    self.assertTrue(result.returncode != 0 and b"ProducerFailure" in result.stderr, "uncaught-error")
                elif mode in SUCCESS_MODES:
                    # An always-failing transport must die here, not merely elsewhere.
                    self.assertTrue(result.returncode == 0 and not result.stdout and not result.stderr, "success-required")
                else:
                    self.assertTrue(result.returncode == 0 and not result.stdout and not result.stderr, "boundary-result")

    # ---- review round r1 regressions ------------------------------------------

    def test_phase_accessor_cannot_escape_boundary(self):
        class BadPhase:
            @property
            def phase(self):
                raise RuntimeError(CANARY)

        @transport.producer_boundary
        def fail(owner):
            raise ValueError(CANARY)

        try:
            error = self.ambient(lambda: fail(BadPhase()))
        except RuntimeError:
            self.fail("phase-boundary")
        self.assertEqual(error.phase, "payload", "phase-boundary")

    def test_caller_failures_reap_and_close(self):
        previous = Path.cwd()
        try:
            for mode in ("bad-handshake", "probe-bad", "score-bad", "late", "post"):
                with self.subTest(mode=mode), tempfile.TemporaryDirectory() as root:
                    try:
                        boundary_scenario(mode, Path(root))
                    finally:
                        os.chdir(previous)
        finally:
            os.chdir(previous)

    def ambient(self, call):
        """Enter the boundary from inside live, payload-bearing caller handlers."""
        try:
            raise ValueError(CANARY)
        except ValueError:
            try:
                raise KeyError(CANARY)
            except KeyError:
                with self.assertRaises(transport.ProducerFailure) as failure:
                    call()
        error = failure.exception
        self.assertIsNone(error.__context__, "exception-context")
        self.assertIsNone(error.__cause__, "exception-context")
        self.assertNotIn(CANARY, "".join(traceback.format_exception(error)), "exception-context")
        self.assertNotIn(CANARY, repr(error), "exception-context")
        return error

    def test_ambient_caller_context_never_crosses_high_level_calls(self):
        # `raise ... from None` only hides the caller's live exception; it stays attached.
        with tempfile.TemporaryDirectory() as root:
            for mode in ("early", "bad-handshake", "score-bad", "score-reason", "probe-bad"):
                with self.subTest(mode=mode):
                    self.ambient(lambda mode=mode: invoke(mode, Path(root)))

    def test_ambient_context_cleared_for_validation_and_cancellation(self):
        with tempfile.TemporaryDirectory() as root:
            with mock.patch.object(score, "BenchSubprocess") as spawn:
                error = self.ambient(
                    lambda: score.collect_validator_measurements(Path(root), [], [CANARY]))
                spawn.assert_not_called()
            self.assertEqual(error.code, "payload_processing", "exception-context")
            owners = []
            def factory(*args, **kwargs):
                owner = transport.BenchSubprocess(command("probe"), limits=limits())
                owners.append(owner)
                return owner
            with mock.patch.object(score, "BenchSubprocess", side_effect=factory), mock.patch.object(
                score, "_validate_validator_probe_handshake", side_effect=KeyboardInterrupt(CANARY)
            ):
                error = self.ambient(
                    lambda: score.collect_validator_measurements(Path(root), [], []))
            self.assertEqual(error.code, "cancelled", "cancelled-boundary")
            self.reaped(owners[0])

    def test_ambient_context_cleared_on_context_manager_exit(self):
        # __exit__ raises outside the decorated boundary and needs the same scrub.
        owner = self.owner("nonzero")
        def call():
            with owner:
                owner.exchange({"x": 1})
        error = self.ambient(call)
        self.assertEqual((error.code, error.phase), ("producer_exit", "finish"), "exception-context")
        self.reaped(owner)

    def test_ambient_context_cleared_on_cleanup_failure(self):
        owner = self.owner("ignore")
        def call():
            with owner:
                owner.exchange({"x": 1})
                owner._wait_exit = mock.Mock(side_effect=OSError(CANARY))
        error = self.ambient(call)
        self.assertEqual(error.code, "cleanup", "exception-context")
        self.reaped(owner)

    def test_early_stdout_prefix_rejected_before_request_completes(self):
        # A response cannot predate its request, in either selector report order.
        for reverse in (False, True):
            with self.subTest(reverse=reverse):
                owner = self.owner("prefix", chunk_bytes=8192, exchange_seconds=2)
                with self.assertRaises(transport.ProducerFailure, msg="early-prefix") as failure:
                    with owner:
                        real_select = owner.selector.select
                        def ordered(timeout, _real=real_select, _reverse=reverse):
                            events = _real(timeout)
                            return list(reversed(events)) if _reverse else events
                        with mock.patch.object(owner.selector, "select", side_effect=ordered):
                            owner.exchange({"text": "x" * 200_000})
                self.assertEqual(failure.exception.code, "protocol", "early-prefix")
                self.reaped(owner)

    def test_idle_stdout_before_request_rejected(self):
        owner = self.owner("prefix-idle")
        with self.assertRaises(transport.ProducerFailure, msg="idle-prefix") as failure:
            with owner:
                time.sleep(0.25)
                owner.exchange({"x": 1})
        self.assertEqual(failure.exception.code, "protocol", "idle-prefix")
        self.reaped(owner)

    def test_transport_failures_carry_the_owner_phase(self):
        missing = HERE.parent / "definitely-absent-producer-binary"
        owner = transport.BenchSubprocess([str(missing)], limits=limits())
        self.addCleanup(lambda: owner.selector.close() if owner.selector is not None else None)
        with self.assertRaises(transport.ProducerFailure, msg="phase-accuracy") as failure:
            with owner:
                pass
        self.assertEqual(failure.exception.phase, "start", "phase-accuracy")
        self.assertNotIn(str(missing), str(failure.exception), "phase-accuracy")
        owner = self.owner("bad-json-handshake")
        with self.assertRaises(transport.ProducerFailure, msg="phase-accuracy") as failure:
            with owner:
                owner.receive_handshake()
        self.assertEqual(failure.exception.phase, "handshake", "phase-accuracy")
        self.reaped(owner)
        for wire in (b"{bad}\n", b'{"x":"\xff"}\n'):
            with self.subTest(wire=wire):
                owner = self.owner("raw")
                with self.assertRaises(transport.ProducerFailure, msg="phase-accuracy") as failure:
                    with owner:
                        owner.exchange({"wire": wire.hex()})
                self.assertEqual(failure.exception.phase, "exchange", "phase-accuracy")
                self.reaped(owner)

    def test_stdin_registration_released_when_receive_aborts(self):
        owner = self.owner("blocked-write", stderr_bytes=4096, exchange_seconds=2)
        with self.assertRaises(transport.ProducerFailure, msg="stdin-registration"):
            with owner:
                self.assertTrue(owner.receive_handshake() == {"ready": True}, "child-ready")
                try:
                    owner.exchange({"text": "x" * 400_000})
                except transport.ProducerFailure as first:
                    self.assertEqual(first.code, "stderr_limit", "stdin-registration")
                    self.assertNotIn(owner.process.stdin.fileno(),
                                     {key.fd for key in owner.selector.get_map().values()},
                                     "stdin-registration")
                    raise
        self.reaped(owner)

    def test_cleanup_budget_outlives_the_invocation_budget(self):
        # Deliberate: termination and reaping stay available after the invocation expires.
        slow_cleanup = dict(handshake_seconds=0.3, exchange_seconds=0.3, finish_seconds=0.25,
                            invocation_seconds=0.4, terminate_seconds=0.5, reap_seconds=1.0)
        try:
            limits(**slow_cleanup).validate()
        except transport.ProducerFailure:
            self.fail("cleanup-budget")
        owner = self.owner("ignore", **slow_cleanup)
        with self.assertRaises(transport.ProducerFailure, msg="cleanup-budget") as failure:
            with owner:
                owner.exchange({"x": 1})
                time.sleep(0.45)
                owner.check_deadline()
        self.assertEqual(failure.exception.code, "deadline", "cleanup-budget")
        self.assertTrue(time.monotonic() > owner.invocation_deadline, "cleanup-budget")
        self.reaped(owner)
        self.assertEqual(owner.process.returncode, -signal.SIGKILL, "cleanup-budget")

    def test_mutation_roster_sites_are_unique_and_applicable(self):
        source = Path(transport.__file__).read_text()
        seen = {}
        for name, (old, new, target, marker) in MUTATIONS.items():
            with self.subTest(mutation=name):
                self.assertEqual(source.count(old), 1, "mutation-roster")
                self.assertNotIn((old, new), seen, "mutation-roster")
                seen[(old, new)] = name
                self.assertNotEqual(old, new, "mutation-roster")
                self.assertTrue(callable(getattr(TransportTests, target, None)), "mutation-roster")
                compile(source.replace(old, new, 1), "<mutant>", "exec")


MUTATIONS = {
    "stderr-file": ("        self.stderr_seen += len(chunk)",
                    "        open('synthetic-output', 'ab').write(chunk)\n        self.stderr_seen += len(chunk)",
                    "test_boundary_success_and_failure_sinks", "file-boundary"),
    "stderr-stdout": ("        self.stderr_seen += len(chunk)",
                      "        print(chunk.decode('utf-8', errors='replace'))\n        self.stderr_seen += len(chunk)",
                      "test_boundary_success_and_failure_sinks", "output-canary"),
    "stderr-budget": ("if self.stderr_seen > self.limits.stderr_bytes:", "if False:",
                      "test_stderr_budget", "stderr-budget"),
    "request-cap": ("if len(output) + len(data) > self.limits.request_bytes:", "if False:",
                    "test_request_cap_and_preallocation", "request-preallocation"),
    "frame-truncation": ("if newline >= 0:\n                        if newline != len(chunk) - 1:\n                            raise ProducerFailure(\"protocol\", self.phase)\n                        complete = True",
                         "if newline >= 0:\n                        frame = frame[:len(frame) - len(chunk) + newline + 1]\n                        complete = True",
                         "test_malformed_extra_partial_frames", "frame-refusal"),
    "deadline": ("    def _check(self, deadline):\n        if time.monotonic() >= deadline:",
                 "    def _check(self, deadline):\n        if False:",
                 "test_write_blocking_and_absolute_deadlines", "absolute-deadline"),
    "cleanup": ("        if self.process is None:\n            return clean",
                "        return clean\n        if self.process is None:\n            return clean",
                "test_cleanup_on_processing_exception", "child-reaping"),
    "context": ("        except Exception:\n            phase = _owner_phase(args, phase)",
                "        except Exception:\n            raise ProducerFailure('payload_processing') from None",
                "test_boundary_no_exception_context_in_process", "exception-context"),
    "ambient-context": ("        closed.__cause__ = None\n        closed.__context__ = None",
                        "        closed.__cause__ = None",
                        "test_ambient_caller_context_never_crosses_high_level_calls", "exception-context"),
    "owner-phase": ("        phase = getattr(args[0], \"phase\", None) if args else None", "        phase = None",
                    "test_transport_failures_carry_the_owner_phase", "phase-accuracy"),
    "early-prefix": ("                pending = request is not None and offset != len(request)",
                     "                pending = False",
                     "test_early_stdout_prefix_rejected_before_request_completes", "early-prefix"),
    "stdin-registration": ("            if request is not None:\n                # Never strand a write registration, and never mask the first failure.\n                self._unregister(self.process.stdin)",
                           "            if False:\n                self._unregister(self.process.stdin)",
                           "test_stdin_registration_released_when_receive_aborts", "stdin-registration"),
    "always-fail": ("    def exchange(self, request):\n        if self.finished",
                    "    def exchange(self, request):\n        raise ProducerFailure(\"protocol\", \"exchange\")\n        if self.finished",
                    "test_boundary_success_and_failure_sinks", "success-required"),
    "cleanup-budget": ("        if max(self.handshake_seconds, self.exchange_seconds, self.finish_seconds) > self.invocation_seconds:",
                       "        if max(self.handshake_seconds, self.exchange_seconds, self.finish_seconds,\n               self.terminate_seconds, self.reap_seconds) > self.invocation_seconds:",
                       "test_cleanup_budget_outlives_the_invocation_budget", "cleanup-budget"),
    "caller-cleanup": ("    def _cleanup(self):\n        clean = True",
                       "    def _cleanup(self):\n        return True\n        clean = True",
                       "test_caller_failures_reap_and_close", "caller-reaping"),
    "phase-accessor": ("    except BaseException:\n        # Error reporting must not expose a second diagnostic from an accessor.\n        return default",
                       "    except BaseException:\n        # Error reporting must not expose a second diagnostic from an accessor.\n        raise",
                       "test_phase_accessor_cannot_escape_boundary", "phase-boundary"),
}


def apply_mutation(name):
    old, new, _, _ = MUTATIONS[name]
    source = Path(transport.__file__).read_text()
    assert old in source, "mutation-site"
    exec(compile(source.replace(old, new, 1), transport.__file__, "exec"), transport.__dict__)
    score.run_config = transport.producer_boundary(score.run_config.__wrapped__)
    score.collect_validator_measurements = transport.producer_boundary(score.collect_validator_measurements.__wrapped__)


def mutation_worker(name):
    apply_mutation(name)
    target = MUTATIONS[name][2]
    result = unittest.TestResult()
    unittest.TestSuite([TransportTests(target)]).run(result)
    # Only the named assertion is a kill. Errors/watchdog/compile failure aren't.
    killed = any(MUTATIONS[name][3] in detail for _, detail in result.failures)
    if not killed:
        print("survived-or-wrong-failure:" + name)
        return 1
    print("killed:" + name + ":" + target)
    return 0


def mutation_proof():
    for name in MUTATIONS:
        environment = {**os.environ, "GAZE_BENCH_TEST_MUTANT": name}
        result = subprocess.run([sys.executable, str(HERE), "--mutant", name],
                                env=environment, capture_output=True, timeout=120)
        if result.returncode != 0 or CANARY.encode() in result.stdout + result.stderr:
            print("mutation-proof-failed:" + name)
            return 1
        print(result.stdout.decode().strip())
    return 0


if __name__ == "__main__":
    if len(sys.argv) > 1 and sys.argv[1] == "--child":
        raise SystemExit(child(sys.argv[2]))
    if len(sys.argv) > 1 and sys.argv[1] == "--mutation-proof":
        raise SystemExit(mutation_proof())
    if len(sys.argv) > 1 and sys.argv[1] == "--mutant":
        raise SystemExit(mutation_worker(sys.argv[2]))
    if len(sys.argv) > 1 and sys.argv[1] == "--boundary":
        if os.environ.get("GAZE_BENCH_TEST_MUTANT"):
            apply_mutation(os.environ["GAZE_BENCH_TEST_MUTANT"])
        boundary_scenario(sys.argv[2], Path(sys.argv[3]))
    else:
        unittest.main()
