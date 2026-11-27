"""Model-free checks of the F8 budget gate and workload generator."""
import importlib.util
import json
from pathlib import Path

SPEC = importlib.util.spec_from_file_location("f8_envelope", Path(__file__).with_name("f8_envelope.py"))
f8 = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(f8)
BUDGETS = json.loads(f8.BUDGETS.read_text())


QUIET = {"load": [0.5, 0.5, 0.5], "foreign": [], "quiet": True}


def receipt(turn, quiet=True, concurrency=1, **overrides):
    probes = [QUIET, QUIET if quiet else {"load": [6.0, 3.0, 2.0], "foreign": [], "quiet": False}, QUIET]
    base = {
        "schema": f8.SCHEMA, "envelope": "laptop", "quiet": True,
        "budgets_sha256": f8.sha256_file(f8.BUDGETS),
        "policy_source": "gaze setup --non-interactive",
        "host": {"os": "Darwin", "machine": "arm64", "probes": probes},
        "workload": {"concurrent_sessions": concurrency, "daemon_args": []},
        "samples_ms": {"turn": turn, "reclean": [100.0] * 20, "restore": [5.0] * 20, "cold_start": [900.0] * 5},
        "rss_bytes": {"steady": 800_000_000, "peak": 1_000_000_000},
    }
    base.update(overrides)
    return base


def test_passing_receipt_has_no_failures():
    assert f8.check(receipt([100.0] * 100), BUDGETS) == []


def test_p99_needs_one_hundred_turns():
    # 99 fast turns cannot name a p99, so the gate must not pass them.
    failures = f8.check(receipt([10.0] * 99), BUDGETS)
    assert failures == ["turn_p99_ms: missing measurement (not a pass)"]


def test_p99_allows_one_slow_turn_in_one_hundred_but_not_two():
    assert f8.check(receipt([100.0] * 99 + [1_200.0]), BUDGETS) == []
    turns = [100.0] * 98 + [1_200.0] * 2
    assert f8.check(receipt(turns), BUDGETS) == ["turn_p99_ms: 1200.0 exceeds budget 1000"]


def test_load_in_the_middle_of_a_run_fails_even_if_the_stored_flag_says_quiet():
    failures = f8.check(receipt([100.0] * 100, quiet=False, rss_bytes={"steady": None, "peak": 1}), BUDGETS)
    assert any("not quiet" in f for f in failures)
    assert "steady_rss_bytes: missing measurement (not a pass)" in failures


def test_receipt_must_match_the_decided_configuration():
    cases = {
        "daemon flags": {"workload": {"concurrent_sessions": 1, "daemon_args": ["--safety-net", "none"]}},
        "budgets file": {"budgets_sha256": "0" * 64},
        "gaze setup": {"policy_source": "custom"},
        "platform": {"host": {"os": "Linux", "machine": "x86_64", "probes": [QUIET, QUIET]}},
    }
    for needle, override in cases.items():
        failures = f8.check(receipt([100.0] * 100, **override), BUDGETS)
        assert any(needle in f for f in failures), (needle, failures)


def test_cold_start_needs_five_runs():
    r = receipt([100.0] * 100)
    r["samples_ms"]["cold_start"] = [900.0] * 4
    assert any("fewer than 5" in f for f in f8.check(r, BUDGETS))


def test_wrong_concurrency_fails():
    assert any("concurrent" in f for f in f8.check(receipt([100.0] * 100, concurrency=4), BUDGETS))


def test_workload_shape_is_deterministic_and_in_bounds():
    history, new = f8.conversation(7, 3)
    assert (history, new) == f8.conversation(7, 3)
    size = sum(len(h.encode()) for h in history)
    assert len(history) == 12 and 28 * 1024 <= size <= 32 * 1024
    assert all(len(n.encode()) <= 4 * 1024 for n in new)
    assert all("@" in n or "DE89" in n or "straße" in n.lower() or "Germany" in n or "Anschrift" in n for n in new)


def test_token_prefixes_normalize_per_sample():
    value = {"t": "<0a1b2c3d:EMAIL_1> and <ffffffff:IBAN_1> <0a1b2c3d:EMAIL_2>",
             "u": ["email1.0a1b2c3d@gaze-fake.invalid", "ffffffff:name_1", "deadbeef0a1b2c3d:x"]}
    assert f8.normalize(value, {}) == {"t": "<S0:EMAIL_1> and <S1:IBAN_1> <S0:EMAIL_2>",
                                       "u": ["email1.S0@gaze-fake.invalid", "S1:name_1", "deadbeef0a1b2c3d:x"]}


def test_exchange_reads_a_burst_of_replies_written_at_once(tmp_path, monkeypatch):
    # A server-mode burst: the daemon answers four frames in one write. The
    # harness must not strand buffered replies behind an empty descriptor.
    import stat
    import sys
    fake = tmp_path / "fake-gaze"
    fake.write_text(
        f"#!{sys.executable}\n"
        "import sys\n"
        "lines = [sys.stdin.readline() for _ in range(4)]\n"
        "sys.stdout.write(''.join(lines)); sys.stdout.flush()\n"
        "sys.stdin.read()\n"
    )
    fake.chmod(fake.stat().st_mode | stat.S_IEXEC)
    monkeypatch.setattr(f8, "EXCHANGE_TIMEOUT", 5.0)
    daemon = f8.Daemon(fake, tmp_path / "policy.toml", tmp_path / "stderr.log")
    try:
        replies = daemon.exchange([{"n": i} for i in range(4)])
        assert [r for r, _ in replies] == [{"n": i} for i in range(4)]
    finally:
        daemon.close()
    assert daemon.proc.returncode == 0 and daemon.peak > 0


def test_the_daemons_own_cores_do_not_count_as_foreign_load():
    busy_self = {"load": [3.5, 2.0, 1.0], "own_cores": 2.0, "foreign": [], "quiet": True}
    busy_other = {"load": [3.5, 2.0, 1.0], "own_cores": 1.0, "foreign": [], "quiet": True}
    r = receipt([100.0] * 100)
    r["host"]["probes"] = [QUIET, busy_self, QUIET]
    assert f8.check(r, BUDGETS) == []
    r["host"]["probes"] = [QUIET, busy_other, QUIET]
    assert any("not quiet" in f for f in f8.check(r, BUDGETS))


def test_cpu_seconds_reads_this_process():
    import os
    import time
    start = f8.cpu_seconds(os.getpid())
    deadline = time.process_time() + 0.3
    while time.process_time() < deadline:
        pass
    assert f8.cpu_seconds(os.getpid()) - start >= 0.2
