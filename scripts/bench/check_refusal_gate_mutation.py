"""Prove benign and concealed refusal guards are required by behavioral oracles."""
import inspect
import io
import json
import unittest
from unittest import mock

import refusal_gate
from test_refusal_contract import PairedGateTests


def check():
    def run(test):
        return unittest.TextTestRunner(stream=io.StringIO()).run(
            unittest.TestSuite([PairedGateTests(test)]))
    source = inspect.getsource(refusal_gate.compare)
    results = {"baseline": "pass"}
    for name, anchor, test in (
        ("disabled_benign_refusal_guard", "if bad:",
         "test_benign_refusals_fail_even_when_unchanged_and_another_layer_improves"),
        ("disabled_concealed_refusal_guard", "if concealed:",
         "test_base_ordinary_refusal_cannot_inherit_candidate_block"),
    ):
        if not run(test).wasSuccessful():
            raise RuntimeError("baseline refusal oracle failed")
        if source.count(anchor) != 1:
            raise RuntimeError("refusal guard mutation anchor changed")
        namespace = dict(refusal_gate.__dict__)
        exec(compile(source.replace(anchor, "if False:"), "<refusal-mutant>", "exec"), namespace)
        with mock.patch.object(refusal_gate, "compare", namespace["compare"]):
            result = run(test)
        if len(result.failures) != 1 or result.errors:
            raise RuntimeError("refusal guard mutant survived or oracle errored")
        results[name] = "killed"
    return results


if __name__ == "__main__":
    print(json.dumps(check(), sort_keys=True))
