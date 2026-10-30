"""Prove the benign-refusal gate branch is required by the behavioral oracle."""
import inspect
import io
import json
import unittest
from unittest import mock

import refusal_gate
from test_refusal_contract import PairedGateTests


def check():
    test = "test_benign_refusals_fail_even_when_unchanged_and_another_layer_improves"
    def run():
        return unittest.TextTestRunner(stream=io.StringIO()).run(
            unittest.TestSuite([PairedGateTests(test)]))
    if not run().wasSuccessful():
        raise RuntimeError("baseline refusal oracle failed")
    source = inspect.getsource(refusal_gate.compare)
    if source.count("if bad:") != 1:
        raise RuntimeError("refusal guard mutation anchor changed")
    namespace = dict(refusal_gate.__dict__)
    exec(compile(source.replace("if bad:", "if False:"), "<benign-refusal-mutant>", "exec"), namespace)
    with mock.patch.object(refusal_gate, "compare", namespace["compare"]):
        result = run()
    if len(result.failures) != 1 or result.errors:
        raise RuntimeError("benign refusal guard mutant survived or oracle errored")
    return {"baseline": "pass", "disabled_benign_refusal_guard": "killed"}


if __name__ == "__main__":
    print(json.dumps(check(), sort_keys=True))
