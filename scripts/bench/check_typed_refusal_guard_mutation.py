"""Prove the typed-refusal completeness guard is required by its own suite.

Each mutant edits one site of `test_typed_refusal_producer.py` in a private module
copy and reruns the guard suite. A mutant is killed only when the suite fails,
errors, skips a test or runs a different number of tests; the unmutated baseline
must run every test with no skip, so a skipped live guard also fails here.
"""
import inspect
import io
import json
import sys
import types
import unittest

import test_typed_refusal_producer as guard

SUITE = "TypedRefusalProducerGuardTests"
LIVE_TEST = "    def test_identity_rows_name_every_refused_range_and_round_trip(self):\n"
MUTANTS = (
    ("live_check_disconnected",
     'check_row(self, row, row["response"])', "pass"),
    ("first_span_only_checker",
     'test.assertEqual(spans(response), row["expected_refused_ranges"], row["id"])',
     'test.assertEqual(spans(response)[:1], row["expected_refused_ranges"][:1], row["id"])'),
    ("completeness_assertion_removed",
     'test.assertIs(response.get("evidence_complete"), True, row["id"])', "pass"),
    ("live_test_skipped", LIVE_TEST, '    @unittest.skip("mutant")\n' + LIVE_TEST),
    ("live_test_skipped_if_fixture_missing", LIVE_TEST,
     '    @unittest.skipUnless(FIXTURE.with_name("absent.json").exists(), "fixture missing")\n'
     + LIVE_TEST),
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


def check():
    source = inspect.getsource(guard)
    expected = unittest.defaultTestLoader.loadTestsFromTestCase(
        getattr(guard, SUITE)).countTestCases()
    baseline = run(source, "typed_refusal_guard_baseline")
    if (not baseline.wasSuccessful() or baseline.skipped or baseline.testsRun != expected
            or expected == 0):
        raise RuntimeError("baseline typed-refusal guard did not run cleanly")
    results = {"baseline": "pass"}
    for name, anchor, replacement in MUTANTS:
        if source.count(anchor) != 1:
            raise RuntimeError(f"{name}: mutation anchor changed")
        result = run(source.replace(anchor, replacement), f"typed_refusal_guard_{name}")
        survived = (result.wasSuccessful() and not result.skipped
                    and result.testsRun == expected)
        if survived:
            raise RuntimeError(f"{name}: mutant survived")
        results[name] = "killed"
    return results


if __name__ == "__main__":
    print(json.dumps(check(), sort_keys=True))
