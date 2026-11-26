"""Model-free checks of the F8 budget gate and workload generator."""
import importlib.util
import json
from pathlib import Path

SPEC = importlib.util.spec_from_file_location("f8_envelope", Path(__file__).with_name("f8_envelope.py"))
f8 = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(f8)
BUDGETS = json.loads(f8.BUDGETS.read_text())


def receipt(turn, quiet=True, concurrency=1, **overrides):
    base = {
        "schema": f8.SCHEMA, "envelope": "laptop", "quiet": quiet,
        "workload": {"concurrent_sessions": concurrency},
        "samples_ms": {"turn": turn, "reclean": [100.0] * 20, "restore": [5.0] * 20, "cold_start": [900.0]},
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


def test_noisy_host_and_missing_rss_fail():
    failures = f8.check(receipt([100.0] * 100, quiet=False, rss_bytes={"steady": None, "peak": 1}), BUDGETS)
    assert any("not quiet" in f for f in failures)
    assert "steady_rss_bytes: missing measurement (not a pass)" in failures


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
    value = {"t": "<0a1b2c3d:EMAIL_1> and <ffffffff:IBAN_1> <0a1b2c3d:EMAIL_2>"}
    assert f8.normalize(value, {}) == {"t": "<S0:EMAIL_1> and <S1:IBAN_1> <S0:EMAIL_2>"}
