"""Outcome counters: every path counted, existing scorecard fields untouched."""

import json
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parent))

import bench_json
import gaze_bench_score as score
import outcome_counters as oc
import scorecard_record as record
import verify_record_scorecards as proof


ROOT = Path(__file__).resolve().parents[2]
BENCH = ROOT / "docs/reference/benchmarks"
TEXT = "Call 4111 at Berlin office now"  # ASCII, so byte and char offsets agree.
GOLD = score.Span(5, 9, "ACCOUNTNUM")  # "4111"


def document(uid="d", spans=(GOLD,), **extra):
    return score.Document(uid=uid, text=TEXT, language="en", region="US",
                          source_dataset="synthetic", spans=tuple(spans), **extra)


def refused(code):
    return {"fixture_id": "d", "pipeline_error_stage": "clean",
            "pipeline_error_code": code, "timing": {"total_ms": 1.0}}


class RefusalTests(unittest.TestCase):
    def test_each_untransformable_reason_is_counted_by_reason(self):
        for reason in oc.UNTRANSFORMABLE_REASONS:
            with self.subTest(reason=reason):
                outcomes = oc.OutcomeAccumulator()
                outcomes.add_refused(document(), "untransformable_" + reason)
                result = outcomes.result()
                self.assertEqual(result["untransformable_refused"]["documents"], 1)
                self.assertEqual(
                    result["untransformable_refused"]["by_reason"],
                    {other: int(other == reason) for other in oc.UNTRANSFORMABLE_REASONS},
                )
                self.assertEqual(result["blocked_operations"], {
                    "documents": 1,
                    "by_kind": {"untransformable": 1, "possibly_sensitive": 0, "fail_closed": 0},
                })

    def test_refusal_kinds_are_separate(self):
        outcomes = oc.OutcomeAccumulator()
        outcomes.add_refused(document("a"), oc.POSSIBLY_SENSITIVE_REFUSED_CODE)
        outcomes.add_refused(document("b"), "sqlite")
        result = outcomes.result()
        self.assertEqual(result["blocked_operations"]["by_kind"],
                         {"untransformable": 0, "possibly_sensitive": 1, "fail_closed": 1})
        self.assertEqual(result["untransformable_refused"]["documents"], 0)
        self.assertEqual(
            result["protection"]["possibly_sensitive"]["dispositions"], {"protect": 0, "refuse": 1}
        )

    def test_only_refusals_of_documents_without_scored_gold_are_fp_refusals(self):
        outcomes = oc.OutcomeAccumulator()
        outcomes.add_refused(document("gold"), "untransformable_encoded_carrier")
        outcomes.add_refused(document("benign", spans=()), "untransformable_encoded_carrier")
        # An out-of-contract label is not gold either.
        outcomes.add_refused(
            document("excluded", spans=(), excluded_spans=(GOLD,)), "sqlite"
        )
        self.assertEqual(outcomes.result()["fp_refusals"], {"documents": 2})

    def test_refusal_codes_are_closed_pipeline_failure_reasons(self):
        doc = document()
        for code in sorted(oc.UNTRANSFORMABLE_CODES | {oc.POSSIBLY_SENSITIVE_REFUSED_CODE}):
            with self.subTest(code=code):
                score.validate_response(doc, refused(code))
        with self.assertRaises(score.ResponseValidationError):
            score.validate_response(doc, refused("untransformable_unknown_reason"))


class ProtectionTests(unittest.TestCase):
    def test_confirmed_and_ambiguous_protection_are_counted_apart(self):
        outcomes = oc.OutcomeAccumulator()
        outcomes.add_scored(document("confirmed"), [
            score.Span(5, 9, "custom:account_number"),  # gold
            score.Span(13, 19, "Location"),  # "Berlin", false positive
        ])
        outcomes.add_scored(document("ambiguous"), [
            score.Span(5, 9, "custom:family:payment-card-or-iban"),  # gold
            score.Span(0, 4, "custom:family:payment-card-or-iban"),  # "Call", FP
        ])
        protection = outcomes.result()["protection"]
        self.assertEqual(protection["confirmed"],
                         {"values": 2, "gold_values": 1, "false_positive_values": 1})
        self.assertEqual(protection["possibly_sensitive"], {
            "values": 2, "gold_values": 1, "false_positive_values": 1,
            "dispositions": {"protect": 2, "refuse": 0},
        })

    def test_false_positives_by_document_and_by_value(self):
        outcomes = oc.OutcomeAccumulator()
        outcomes.add_scored(document("clean"), [score.Span(5, 9, "custom:a")])
        outcomes.add_scored(document("two-fp"), [
            score.Span(0, 4, "custom:a"), score.Span(13, 19, "custom:a"),
        ])
        outcomes.add_scored(document("ambiguous-fp"), [
            score.Span(13, 19, "custom:family:f"),
        ])
        outcomes.add_scored(document("mixed-fp"), [
            score.Span(0, 4, "custom:a"), score.Span(13, 19, "custom:family:f"),
        ])
        false_positives = outcomes.result()["false_positives"]
        self.assertEqual(false_positives, {
            "documents": 3,
            "documents_from_possibly_sensitive_only": 1,
            "values": 5,
            "possibly_sensitive_values": 2,
        })

    def test_out_of_contract_and_neutral_bytes_are_not_counted(self):
        doc = document(spans=(), excluded_spans=(GOLD,),
                       neutral_prediction_classes=frozenset({"Location"}))
        outcomes = oc.OutcomeAccumulator()
        outcomes.add_scored(doc, [score.Span(5, 9, "custom:a"), score.Span(13, 19, "Location")])
        result = outcomes.result()
        self.assertEqual(result["protection"]["confirmed"]["values"], 0)
        self.assertEqual(result["false_positives"]["documents"], 0)
        # Only bytes outside the excluded label and the neutral prediction count.
        self.assertEqual(result["usable_content"]["non_pii_bytes"], len(TEXT) - 4 - 6)


class UsableContentTests(unittest.TestCase):
    """Protection must leave the rest of the document usable."""

    def test_exact_ambiguous_protection_keeps_all_other_content(self):
        outcomes = oc.OutcomeAccumulator()
        outcomes.add_scored(document(), [score.Span(5, 9, "custom:family:f")])
        self.assertEqual(outcomes.result()["usable_content"], {
            "non_pii_bytes": len(TEXT) - 4,
            "retained_non_pii_bytes": len(TEXT) - 4,
            "lost_to_false_positive_bytes": 0,
            "lost_to_refusal_bytes": 0,
            "retained_ratio": 1.0,
        })

    def test_over_protection_loses_only_the_extra_bytes(self):
        outcomes = oc.OutcomeAccumulator()
        outcomes.add_scored(document(), [score.Span(5, 19, "custom:family:f")])
        usable = outcomes.result()["usable_content"]
        self.assertEqual(usable["lost_to_false_positive_bytes"], 10)
        self.assertEqual(usable["retained_non_pii_bytes"], len(TEXT) - 14)

    def test_refusal_is_not_a_gain(self):
        # Refusing leaks nothing but leaves no usable content at all.
        outcomes = oc.OutcomeAccumulator()
        outcomes.add_scored(document("kept"), [GOLD])
        outcomes.add_refused(document("refused"), "untransformable_no_safe_rendering")
        usable = outcomes.result()["usable_content"]
        self.assertEqual(usable["lost_to_refusal_bytes"], len(TEXT) - 4)
        self.assertEqual(usable["retained_ratio"], 0.5)


class RunConfigTests(unittest.TestCase):
    def test_replayed_run_carries_counters_beside_unchanged_metrics(self):
        # One real recorded success plus two synthetic refusals of other documents.
        header, observations = record._read(BENCH / "observations-v0.17.0.jsonl.gz")
        rows = {row["id"]: row for row in header["documents"]}
        observed = next(row for row in observations
                        if row["layer"] == "C" and row["response"]["final_protection_trace"])
        kept = record._document_from_row(rows[observed["document_id"]])
        responses = {kept.uid: {
            **observed["response"],
            "timing": {"clean_ms": 0.0, "restore_ms": 0.0, "post_policy_scan_ms": None},
        }}
        refused_documents = [document("refused-benign", spans=()), document("refused-gold")]
        responses["refused-benign"] = {
            **refused("untransformable_json_number_no_text_only_path"), "fixture_id": "refused-benign",
        }
        responses["refused-gold"] = {
            **refused(oc.POSSIBLY_SENSITIVE_REFUSED_CODE), "fixture_id": "refused-gold",
        }
        run = score.run_config(
            Path("."), Path("."), "replay", [kept, *refused_documents], Path("."),
            None, None, None, 0.5, Path("."), replay_responses=responses,
        )
        counters = run[oc.OUTCOME_COUNTERS_FIELD]
        self.assertEqual(list(run)[-1], oc.OUTCOME_COUNTERS_FIELD)
        self.assertEqual(counters["attempted_documents"], 3)
        self.assertEqual(counters["blocked_operations"]["by_kind"],
                         {"untransformable": 1, "possibly_sensitive": 1, "fail_closed": 0})
        self.assertEqual(counters["fp_refusals"], {"documents": 1})
        self.assertEqual(counters["protection"]["possibly_sensitive"]["dispositions"]["refuse"], 1)
        protection = counters["protection"]
        self.assertEqual(
            protection["confirmed"]["values"] + protection["possibly_sensitive"]["values"],
            run["metrics"]["prediction_spans"]["total"],
        )
        self.assertEqual(run["failed_closed_population"]["reason_counts"], {
            "possibly_sensitive_refused": 1,
            "untransformable_json_number_no_text_only_path": 1,
        })
        self.assertEqual(counters["usable_content"]["lost_to_refusal_bytes"], 2 * len(TEXT) - 4)


class CommittedRecordProofTests(unittest.TestCase):
    def test_v0170_record_rescores_with_only_the_counters_added(self):
        evidence_path = BENCH / "outcome-counters-v0.17.0.json"
        evidence = oc.prove(BENCH / "observations-v0.17.0.jsonl.gz", "v0.17.0")
        self.assertEqual(json.dumps(evidence, indent=2) + "\n",
                         evidence_path.read_text(encoding="utf-8"))
        self.assertEqual(set(evidence["contracts"]), {"v1", "v2", "v3", "v4"})
        for name, item in evidence["contracts"].items():
            with self.subTest(contract=name):
                self.assertTrue(item["existing_fields_byte_identical"])
                committed = bench_json.load(BENCH / item["committed_scorecard"])
                (run,) = committed["runs"]
                counters = item["outcome_counters"][run["config"]]
                metrics = run["metrics"]
                # The counters partition the same predictions the byte score uses.
                protection = counters["protection"]
                self.assertEqual(
                    protection["confirmed"]["values"] + protection["possibly_sensitive"]["values"],
                    metrics["prediction_spans"]["total"],
                )
                self.assertEqual(
                    protection["confirmed"]["gold_values"]
                    + protection["possibly_sensitive"]["gold_values"],
                    metrics["prediction_spans"]["overlapped_gold"],
                )
                self.assertEqual(counters["false_positives"]["documents"],
                                 metrics["documents_with_false_positives"])
                self.assertEqual(counters["usable_content"]["lost_to_false_positive_bytes"],
                                 metrics["utf8_bytes"]["false_positive"])
                self.assertEqual(counters["blocked_operations"]["documents"], 0)

    def test_proof_refuses_a_changed_existing_field(self):
        card = {"runs": [{"config": "c", "metrics": {"x": 1}, oc.OUTCOME_COUNTERS_FIELD: {}}]}
        expected = {"runs": [{"config": "c", "metrics": {"x": 2}}]}
        self.assertFalse(proof.exact_except_timing(card, expected))
        expected["runs"][0]["metrics"]["x"] = 1
        self.assertTrue(proof.exact_except_timing(card, expected))
        # A scorecard that already has counters must match them too.
        expected["runs"][0][oc.OUTCOME_COUNTERS_FIELD] = {"changed": True}
        self.assertFalse(proof.exact_except_timing(card, expected))

    def test_proof_detects_a_rescore_that_moves_an_existing_number(self):
        original = record.rescore

        def drifting(*args, **kwargs):
            card = original(*args, **kwargs)
            card["runs"][0]["metrics"]["utf8_bytes"]["leaked"] += 1
            return card

        with mock.patch.object(record, "rescore", drifting), tempfile.TemporaryDirectory() as temporary:
            evidence = oc.prove(BENCH / "observations-v0.17.0.jsonl.gz", "v0.17.0")
            self.assertFalse(any(
                item["existing_fields_byte_identical"] for item in evidence["contracts"].values()
            ))
            target = Path(temporary) / "evidence.json"
            self.assertEqual(oc.main([
                str(BENCH / "observations-v0.17.0.jsonl.gz"),
                "--release", "v0.17.0", "--write", str(target),
            ]), 1)
            self.assertFalse(target.exists())

    def test_check_mode_fails_on_stale_evidence(self):
        with tempfile.TemporaryDirectory() as temporary:
            stale = Path(temporary) / "evidence.json"
            stale.write_text("{}\n", encoding="utf-8")
            self.assertEqual(oc.main([
                str(BENCH / "observations-v0.17.0.jsonl.gz"),
                "--release", "v0.17.0", "--check", str(stale),
            ]), 1)


if __name__ == "__main__":
    unittest.main()
