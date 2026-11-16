"""Completeness guard for the pinned pipeline refusal producer.

The fixture is written by the bench example's real `core` rule floor
(`typed_refusal_producer_guard_requires_every_refused_span`), which fails when
its output drifts. A row's response is either "completed", an unqualified
legacy refusal, or a refusal carrying the pinned producer identity. Identity
rows must name exactly the expected refused ranges and, through the shipped
contract without patching the pin, block when all-gold and stay ordinary when a
benign sibling is refused.
"""
import json
from pathlib import Path
import unittest

import gaze_bench_score as score
import refusal_contract as contract
import refusal_gate

FIXTURE = (Path(__file__).resolve().parents[2]
           / "crates/gaze-recognizers/tests/fixtures/typed_refusal_producer.json")
PIPELINE = "gaze-pipeline-typed-refusal-v1"
REASON = "json_number_no_text_only_path"


def load():
    rows = json.loads(FIXTURE.read_text())
    for row in rows:
        row["document"] = score.Document(
            row["id"], row["text"], "en", "US", "synthetic",
            tuple(score.Span(a, b, label) for a, b, label in row["gold"]))
    return rows


def spans(response):
    return [[s["raw_start"], s["raw_end"]] for s in response.get("refusal_evidence") or ()]


def complete_response(row, ranges):
    return {"fixture_id": row["id"], "pipeline_error_stage": "clean",
            "pipeline_error_code": "untransformable_" + REASON, "timing": {"total_ms": 1.0},
            "evidence_complete": True, "evidence_producer": PIPELINE,
            "refusal_evidence": [{"raw_start": a, "raw_end": b,
                                  "outcome": "confirmed_untransformable", "reason": REASON}
                                 for a, b in ranges]}


def classify(row, response):
    result = refusal_gate.inventory([row["document"]], {row["id"]: response})
    return "blocked" if result["blocked"] else "ordinary"


class TypedRefusalProducerGuardTests(unittest.TestCase):
    def test_fixture_covers_multi_field_gold_and_mixed_siblings(self):
        rows = load()
        multi_gold = [r for r in rows if r["expected_verdict"] == "blocked"
                      and len(r["expected_refused_ranges"]) > 1]
        mixed = [r for r in rows if r["expected_verdict"] == "ordinary"]
        self.assertTrue(multi_gold and len(mixed) >= 2)
        for row in rows:
            gold = sorted([a, b] for a, b, _ in row["gold"])
            expected = row["expected_refused_ranges"]
            self.assertEqual(expected, sorted(expected), row["id"])
            # All-gold rows refuse exactly their gold; mixed rows add a benign range.
            if row["expected_verdict"] == "blocked":
                self.assertEqual(expected, gold, row["id"])
            else:
                self.assertTrue(set(map(tuple, gold)) < set(map(tuple, expected)), row["id"])

    def test_expected_ranges_classify_as_declared_under_the_shipped_pin(self):
        for row in load():
            response = complete_response(row, row["expected_refused_ranges"])
            contract.validate_evidence(response, len(row["text"].encode("utf-8")))
            self.assertEqual(classify(row, response), row["expected_verdict"], row["id"])

    def test_a_first_path_only_producer_would_misclassify_mixed_rows(self):
        # Why the guard requires every span: one refused path makes a mixed row blocked.
        misclassified = 0
        for row in load():
            first = row["expected_refused_ranges"][:1]
            if row["expected_verdict"] == "ordinary" and first[0] in [[a, b] for a, b, _ in row["gold"]]:
                self.assertEqual(classify(row, complete_response(row, first)), "blocked", row["id"])
                misclassified += 1
        self.assertGreater(misclassified, 0)

    def test_identity_rows_name_every_refused_range_and_round_trip(self):
        # Zero identity rows is the pre-emitter state; any identity row is fully checked.
        rows = load()
        self.assertEqual(len(rows), 4)
        for row in rows:
            check_row(self, row, row["response"])

    def test_guard_rejects_fabricated_incomplete_emitters(self):
        # On main no fixture row refuses, so feed the same check_row the live
        # fixture uses with fabricated pinned-identity outputs.
        checked = first_path_failures = mixed_ordinary = 0
        for row in load():
            expected = row["expected_refused_ranges"]
            self.assertEqual(check_row(self, row, complete_response(row, expected)),
                             row["expected_verdict"], row["id"])
            checked += 1
            if row["expected_verdict"] == "ordinary":
                mixed_ordinary += 1
            if len(expected) > 1:
                with self.assertRaises(AssertionError, msg=row["id"]):
                    check_row(self, row, complete_response(row, expected[:1]))
                first_path_failures += 1
        self.assertEqual((checked, first_path_failures, mixed_ordinary), (4, 3, 2))


def check_row(test, row, response):
    """The guard's single checking rule; returns the verdict it classified."""
    if response == "completed":
        return "completed"
    test.assertIsInstance(response, dict, row["id"])
    if "evidence_producer" not in response:
        # Legacy unqualified refusal: never complete, never blocked.
        test.assertNotIn("evidence_complete", response, row["id"])
        test.assertEqual(classify(row, response), "ordinary", row["id"])
        return "ordinary"
    test.assertEqual(response["evidence_producer"], PIPELINE, row["id"])
    test.assertIn(response["evidence_producer"], contract.COMPLETE_EVIDENCE_PRODUCERS)
    test.assertIs(response["evidence_complete"], True, row["id"])
    test.assertEqual(spans(response), row["expected_refused_ranges"], row["id"])
    contract.validate_evidence(response, len(row["text"].encode("utf-8")))
    verdict = classify(row, response)
    test.assertEqual(verdict, row["expected_verdict"], row["id"])
    test.assertEqual(contract.eligible(row["document"], response),
                     row["expected_verdict"] == "blocked", row["id"])
    return verdict

if __name__ == "__main__":
    unittest.main()
