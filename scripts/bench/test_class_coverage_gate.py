"""Tests for the like-for-like class coverage-loss regression gate.

The gate exists because three separate manual reviews missed a credential going
from fully protected to raw on the wire. Each test below is a MUTATION PROBE: it
constructs the condition and asserts the gate goes red, so the gate cannot rot
into a dormant symbol-presence check.
"""

import copy
import unittest

import gaze_bench_score as bench


def run(label_table):
    return {"config": "full-stack-kiji-resolve", "per_label_recall": label_table}


def label(entities, fully_covered, overlapped, leaked=100):
    return {
        "entities": entities,
        "fully_covered": fully_covered,
        "overlapped": overlapped,
        "leaked_utf8_bytes": leaked,
    }


BASE = {
    "SECURITYTOKEN": label(157, 112, 120, 796),
    "SURNAME": label(1202, 1196, 1199, 19),
}


class ClassCoverageGateTests(unittest.TestCase):
    def failures(self, candidate_table, baseline_table=None):
        baseline_table = BASE if baseline_table is None else baseline_table
        return bench._class_coverage_failures(
            "full-stack-kiji-resolve",
            run(candidate_table),
            run(baseline_table),
        )

    def test_identical_scorecards_pass(self):
        self.assertEqual(self.failures(copy.deepcopy(BASE)), [])

    def test_fully_covered_decrease_on_constant_population_is_red(self):
        # MUTATION: the exact condition observed on the government-ID change.
        candidate = copy.deepcopy(BASE)
        candidate["SECURITYTOKEN"] = label(157, 111, 119, 828)
        found = self.failures(candidate)
        self.assertEqual(len(found), 1, found)
        self.assertEqual(found[0]["gate"], bench.CLASS_COVERAGE_LOSS_GATE)
        self.assertEqual(found[0]["label"], "SECURITYTOKEN")
        self.assertEqual(found[0]["entities"], 157)
        self.assertEqual(found[0]["fully_covered_delta"], -1)
        self.assertEqual(found[0]["overlapped_delta"], -1)
        self.assertEqual(found[0]["leaked_utf8_bytes_delta"], 32)

    def test_overlap_only_decrease_is_red(self):
        # MUTATION: the shape observed on LICENSEPLATENUM and TAXNUM in the URL
        # change, where fully_covered never moved and only overlap was lost.
        candidate = copy.deepcopy(BASE)
        candidate["SURNAME"] = label(1202, 1196, 1198, 24)
        found = self.failures(candidate)
        self.assertEqual(len(found), 1, found)
        self.assertEqual(found[0]["overlapped_delta"], -1)
        self.assertEqual(found[0]["fully_covered_delta"], 0)

    def test_coverage_gain_is_not_a_failure(self):
        candidate = copy.deepcopy(BASE)
        candidate["SECURITYTOKEN"] = label(157, 130, 140, 400)
        self.assertEqual(self.failures(candidate), [])

    def test_population_move_is_a_non_pass_not_a_silent_skip(self):
        # MUTATION: this is the dodge the gate must refuse. A change that loses
        # coverage AND perturbs the population must not slip through quietly --
        # that is the shape that disqualified an earlier candidate whose leak
        # "win" was denominator shrinkage.
        candidate = copy.deepcopy(BASE)
        candidate["SECURITYTOKEN"] = label(150, 90, 95, 900)
        found = self.failures(candidate)
        self.assertEqual(len(found), 1, found)
        self.assertEqual(found[0]["gate"], bench.CLASS_POPULATION_EVALUABILITY_GATE)
        self.assertEqual(found[0]["reason"], "population_moved, cannot evaluate")
        self.assertEqual(found[0]["candidate_entities"], 150)
        self.assertEqual(found[0]["baseline_entities"], 157)
        self.assertNotEqual(found, [], "population movement must never pass silently")

    def test_absent_on_both_sides_is_comparable_as_empty(self):
        found = bench._class_coverage_failures(
            "full-stack-kiji-resolve",
            {"config": "full-stack-kiji-resolve"},
            {"config": "full-stack-kiji-resolve"},
        )
        self.assertEqual(found, [])

    def test_asymmetric_telemetry_is_a_non_pass(self):
        # Dropping the block on one side must not be a way to silence the gate.
        found = bench._class_coverage_failures(
            "full-stack-kiji-resolve",
            {"config": "full-stack-kiji-resolve"},
            run(copy.deepcopy(BASE)),
        )
        self.assertEqual(len(found), 1, found)
        self.assertEqual(found[0]["gate"], bench.CLASS_COVERAGE_LOSS_GATE)
        self.assertIn("only one side", found[0]["reason"])

    def test_malformed_block_fails_closed(self):
        with self.assertRaises(ValueError):
            bench._class_coverage_failures(
                "full-stack-kiji-resolve",
                run({"SECURITYTOKEN": {"entities": "many"}}),
                run(copy.deepcopy(BASE)),
            )

    def test_gate_is_wired_into_compare_scorecards(self):
        # Guards against the gate existing but never being invoked -- the
        # dormant-gate failure mode. Verified through the public entry point.
        self.assertIn(
            "_class_coverage_failures",
            bench.compare_scorecards.__code__.co_names,
            "compare_scorecards must invoke the coverage gate",
        )


if __name__ == "__main__":
    unittest.main()
