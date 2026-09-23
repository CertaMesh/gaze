"""Unit tests for the single-pass Stage A bench scripts (solo todo 3738).

Covers the pure parts: the development-split selection rule, the byte-set intersections and
acceptance checks of the paired scorer, and the stage attribution of the exclusion
enumeration. The model runs themselves need the pinned bundles and are not exercised here.
"""

from __future__ import annotations

import unittest

import gaze_bench_score as score
import nym_exclusion_stages as stages
import nym_recognizer_dev_sweep as sweep
import single_pass_nym_paired as paired


def row(actions: int, touching: int, negative_flags: int) -> dict[str, object]:
    return {"actions": actions, "gold_touching": touching, "negative_flags": negative_flags}


class SelectionRuleTest(unittest.TestCase):
    def test_the_lowest_threshold_passing_both_bars_wins(self) -> None:
        table = {
            "USERNAME": {
                "0.30": row(100, 60, 0),  # precision 0.60: fails
                "0.35": row(100, 80, 0),  # passes
                "0.40": row(90, 80, 0),
            }
        }
        self.assertEqual(sweep.choose(table, 1_000), {"USERNAME": 0.35})

    def test_the_acceptance_bar_binds_where_op_b_alone_would_pass(self) -> None:
        # 42 flags on 978 negatives is under op-B's 0.05 per document but over 1 per 1,024.
        table = {"DATE_OF_BIRTH": {"0.75": row(342, 322, 42), "0.95": row(333, 318, 0)}}
        self.assertEqual(sweep.choose(table, 978), {"DATE_OF_BIRTH": 0.95})

    def test_one_flag_per_1024_is_the_bar(self) -> None:
        self.assertTrue(sweep.passes(row(10, 10, 1), 1_024))
        self.assertFalse(sweep.passes(row(10, 10, 1), 978))
        self.assertFalse(sweep.passes(row(0, 0, 0), 1_024), "no actions never qualifies")

    def test_a_label_no_threshold_qualifies_is_left_out(self) -> None:
        table = {"BUILDING_NUMBER": {"0.30": row(100, 10, 0), "0.99": row(50, 20, 0)}}
        self.assertEqual(sweep.choose(table, 1_000), {})


class PairedScorerTest(unittest.TestCase):
    def test_intersect_merges_and_clips(self) -> None:
        self.assertEqual(paired.intersect([(0, 10), (20, 30)], [(5, 25)]), [(5, 10), (20, 25)])
        self.assertEqual(paired.intersect([(0, 5)], [(5, 9)]), [])

    def test_nym_flags_are_recognizer_sources_or_net_actions(self) -> None:
        def item(stage: str, sources: list[str]) -> dict[str, object]:
            return {"provenance": {"stage": stage, "decision": "policy", "source_ids": sources}}

        self.assertTrue(paired.nym_flag(item("primary_pipeline", ["nym/username"])))
        self.assertTrue(paired.nym_flag(item("safety_net", ["nym-small-int8"])))
        self.assertFalse(paired.nym_flag(item("primary_pipeline", ["postal.de"])))

    def test_acceptance_compares_against_pass2_on_the_paired_population(self) -> None:
        def summary(leaked: int, false_positive: int, flags: int, completed: int = 10) -> dict:
            return {
                "all": {
                    "attempted": 10,
                    "completed": completed,
                    "exact_restores": completed,
                    "paired": {"leaked": leaked, "false_positive": false_positive},
                },
                "negative_nym_flags": {"trace_items": flags},
            }

        checks = paired.acceptance(
            {
                paired.BASELINE: summary(10_000, 1_000, 0),
                paired.RESOLVE: summary(4_000, 1_500, 1),
                paired.SINGLE_PASS: summary(3_900, 1_400, 0),
            }
        )[paired.SINGLE_PASS]
        self.assertEqual(checks["bought_bytes"], 6_100)
        self.assertTrue(checks["matches_or_beats_resolve"])
        self.assertFalse(checks["at_least_reference"])
        self.assertEqual(checks["added_false_positive_bytes"], 400)
        self.assertTrue(checks["added_false_positive_within_budget"])
        self.assertTrue(checks["completed_every_request"])
        self.assertTrue(checks["negative_flags_within_budget"])


class ExclusionStageTest(unittest.TestCase):
    def document(self) -> score.Document:
        return score.Document(
            uid="doc", text="Kennzeichen M-AB 1234 in 10115 Berlin", language="de",
            region="DE", source_dataset="unit", spans=(),
        )

    @staticmethod
    def trace_item(start: int, end: int, cls: str, sources: list[str]) -> dict[str, object]:
        return {
            "raw_start": start, "raw_end": end, "class": cls, "action": "tokenize",
            "provenance": {"stage": "primary_pipeline", "decision": "policy", "source_ids": sources},
        }

    @staticmethod
    def nym_row(loser: bool, tier: str = "None", stage: str | None = None) -> dict[str, object]:
        return {
            "recognizer_id": "nym/license_plate", "class": "custom:license_plate",
            "action": "Tokenize", "conflict_loser": loser, "decided_by": tier,
            "provenance_stage": stage,
        }

    def classify(self, rows: list[dict[str, object]], items: list[dict[str, object]]) -> dict:
        traced = {
            "candidates": [{"id": "nym/license_plate", "raw": [12, 21], "normalized": [12, 21]}],
            "rows": rows,
        }
        (fate,) = stages.classify(self.document(), traced, {"final_protection_trace": items})
        return fate

    def test_a_whole_nym_token_is_published(self) -> None:
        fate = self.classify(
            [self.nym_row(False)],
            [self.trace_item(12, 21, "custom:license_plate", ["nym/license_plate"])],
        )
        self.assertEqual((fate["decided_at"], fate["raw_bytes"]), ("publication", 0))

    def test_a_container_win_is_the_containment_rung(self) -> None:
        fate = self.classify(
            [self.nym_row(True, "ContainmentPrecedence")],
            [self.trace_item(0, 30, "custom:iban", ["iban.structural", "nym/license_plate"])],
        )
        self.assertEqual(fate["decided_at"], "containment_rung")
        self.assertEqual((fate["own_class_bytes"], fate["raw_bytes"]), (0, 0))

    def test_uncovered_bytes_are_lost_and_attributed(self) -> None:
        fate = self.classify(
            [self.nym_row(True, "RulePriority")],
            [self.trace_item(12, 16, "custom:phone", ["phone.national.de"])],
        )
        self.assertEqual(fate["decided_at"], "arbitration")
        self.assertEqual(fate["raw_bytes"], 5)

    def test_no_audit_row_is_pool_admission(self) -> None:
        self.assertEqual(self.classify([], [])["decided_at"], "pool_admission")

    def test_a_preserved_winner_is_the_action_policy(self) -> None:
        row = self.nym_row(False)
        row["action"] = "Preserve"
        fate = self.classify([row], [])
        self.assertEqual((fate["decided_at"], fate["raw_bytes"]), ("action_policy", 9))


if __name__ == "__main__":
    unittest.main()
