"""Prove the typed-refusal completeness guard is required by its own suite.

Each mutant edits one site of `test_typed_refusal_producer.py` in a private module
copy and reruns the guard suite. A mutant counts as killed only when exactly its
intended tests fail with assertion failures (or, for the skip mutants, exactly its
intended tests are skipped) and no test errors. Any other outcome, including an
unexpected exception, a different failing test or a changed test count, fails the
probe. The unmutated baseline must run every test with no failure, error or skip.
"""
import inspect
import io
import json
import sys
import types
import unittest

import test_typed_refusal_producer as guard

SUITE = "TypedRefusalProducerGuardTests"
LIVE = "test_identity_rows_name_every_refused_range_and_round_trip"
ROUTING = "test_live_fixture_test_checks_each_rows_own_response"
FABRICATED = "test_guard_rejects_fabricated_incomplete_emitters"
LIVE_DEF = f"    def {LIVE}(self):\n"
LIVE_CALL = 'check_row(self, row, row["response"])'
# (name, anchor, replacement, expected failing tests, expected skipped tests)
MUTANTS = (
    ("live_check_disconnected", LIVE_CALL, "pass", {ROUTING}, set()),
    ("live_response_discarded", LIVE_CALL,
     'row["response"] = "completed"\n            ' + LIVE_CALL, {ROUTING}, set()),
    ("first_span_only_checker",
     'test.assertEqual(spans(response), row["expected_refused_ranges"], row["id"])',
     'test.assertEqual(spans(response)[:1], row["expected_refused_ranges"][:1], row["id"])',
     {FABRICATED}, set()),
    ("completeness_assertion_removed",
     'test.assertIs(response.get("evidence_complete"), True, row["id"])', "pass",
     {FABRICATED}, set()),
    ("live_test_skipped", LIVE_DEF, '    @unittest.skip("mutant")\n' + LIVE_DEF,
     set(), {LIVE, ROUTING}),
    ("live_test_skipped_if_fixture_missing", LIVE_DEF,
     '    @unittest.skipUnless(FIXTURE.with_name("absent.json").exists(), "fixture missing")\n'
     + LIVE_DEF, set(), {LIVE, ROUTING}),
)


def run(source: str, name: str) -> unittest.TestResult:
    # mock.patch in the routing oracle resolves the module by name, so register it.
    module = types.ModuleType(name)
    module.__file__ = guard.__file__
    sys.modules[name] = module
    try:
        exec(compile(source, f"<{name}>", "exec"), module.__dict__)
        suite = unittest.defaultTestLoader.loadTestsFromTestCase(getattr(module, SUITE))
        return unittest.TextTestRunner(stream=io.StringIO()).run(suite)
    finally:
        del sys.modules[name]


def outcome(result: unittest.TestResult) -> dict:
    method = lambda test: test.id().rsplit(".", 1)[-1]
    return {"run": result.testsRun,
            "failed": {method(test) for test, _ in result.failures},
            "errored": {method(test) for test, _ in result.errors},
            "skipped": {method(test) for test, _ in result.skipped}}


def check():
    baseline_tests, mutant_count = 6, 6
    if len(MUTANTS) != mutant_count:
        raise RuntimeError("mutant roster changed")
    source = inspect.getsource(guard)
    baseline = outcome(run(source, "typed_refusal_guard_baseline"))
    if baseline != {"run": baseline_tests, "failed": set(), "errored": set(), "skipped": set()}:
        raise RuntimeError(f"baseline typed-refusal guard did not run cleanly: {baseline}")
    results = {"baseline": "pass"}
    for name, anchor, replacement, failing, skipped in MUTANTS:
        if source.count(anchor) != 1:
            raise RuntimeError(f"{name}: mutation anchor changed")
        got = outcome(run(source.replace(anchor, replacement), f"typed_refusal_guard_{name}"))
        want = {"run": baseline_tests, "failed": failing, "errored": set(), "skipped": skipped}
        if got != want:
            raise RuntimeError(f"{name}: expected {want}, got {got}")
        results[name] = "killed"
    if len(results) != mutant_count + 1:
        raise RuntimeError("not every mutant was killed")
    return results


if __name__ == "__main__":
    print(json.dumps(check(), sort_keys=True))
