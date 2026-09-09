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


def invoke(mode, root, *, late=False, post=False):
    created = []
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


def boundary_scenario(mode, root):
    os.chdir(root)
    late = mode == "late"
    post = mode == "post"
    actual_mode = "score" if late or post else mode
    error = None
    try:
        result, owners = invoke(actual_mode, root, late=late, post=post)
        process_meta = result.get("process", {})
        assert "stderr_log" not in process_meta and "stderr_bytes" not in process_meta, "metadata-boundary"
        assert CANARY not in repr(result), "result-boundary"
        assert all(owner.process.returncode is not None for owner in owners), "caller-reaping"
    except transport.ProducerFailure as caught:
        assert caught.__context__ is None and caught.__cause__ is None, "exception-context"
        assert CANARY not in str(caught) and CANARY not in repr(caught), "exception-boundary"
        rendered = "".join(traceback.format_exception(caught))
        assert CANARY not in rendered, "traceback-boundary"
        error = (caught.code, caught.phase)
    assert not list(root.iterdir()), "file-boundary"
    if mode not in ("score", "score-refusal", "probe"):
        assert error is not None, "failure-required"
    if mode == "uncaught":
        invoke("score-bad", root)


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
                 {"chunk_bytes": 600000}, {"handshake_seconds": 5}, {"chunk_bytes": True})
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
                else:
                    self.assertTrue(result.returncode == 0 and not result.stdout and not result.stderr, "boundary-result")


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
    "frame-truncation": ("if newline != len(frame) - 1 or (request is not None and offset != len(request)):\n                        raise ProducerFailure(\"protocol\", self.phase)",
                         "if request is not None and offset != len(request):\n                        raise ProducerFailure(\"protocol\", self.phase)\n                    frame = frame[:newline + 1]",
                         "test_malformed_extra_partial_frames", "frame-refusal"),
    "deadline": ("if time.monotonic() >= deadline:", "if False:",
                 "test_write_blocking_and_absolute_deadlines", "absolute-deadline"),
    "cleanup": ("        if self.process is None:\n            return clean",
                "        return clean\n        if self.process is None:\n            return clean",
                "test_cleanup_on_processing_exception", "child-reaping"),
    "context": ("        except Exception:\n            pass",
                "        except Exception:\n            raise ProducerFailure('payload_processing') from None",
                "test_boundary_no_exception_context_in_process", "exception-context"),
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
                                env=environment, capture_output=True, timeout=20)
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
