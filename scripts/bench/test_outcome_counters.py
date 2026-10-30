"""Outcome counters: every path counted, existing scorecard fields untouched."""

import gzip
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


ROOT = Path(__file__).resolve().parents[2]
BENCH = ROOT / "docs/reference/benchmarks"
RECORD = BENCH / "observations-v0.17.0.jsonl.gz"
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
            result["protection"]["possibly_sensitive"]["dispositions"], {"protect_values": 0, "refuse_documents": 1}
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

    def test_refusal_codes_do_not_collide_with_existing_pipeline_failures(self):
        self.assertTrue(
            (oc.UNTRANSFORMABLE_CODES | {oc.POSSIBLY_SENSITIVE_REFUSED_CODE})
            <= score.PIPELINE_FAILURE_REASONS
        )
        for reason in score.PIPELINE_FAILURE_REASONS - oc.UNTRANSFORMABLE_CODES - {oc.POSSIBLY_SENSITIVE_REFUSED_CODE}:
            self.assertEqual(oc.refusal_kind(reason), "fail_closed")


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
            "dispositions": {"protect_values": 2, "refuse_documents": 0},
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

    def test_an_over_extended_value_is_an_affected_false_positive_value(self):
        outcomes = oc.OutcomeAccumulator()
        outcomes.add_scored(document(), [score.Span(3, 12, "custom:a")])  # covers gold 5-9
        result = outcomes.result()
        self.assertEqual(result["protection"]["confirmed"],
                         {"values": 1, "gold_values": 1, "false_positive_values": 1})
        self.assertEqual(result["false_positives"], {
            "documents": 1,
            "documents_from_possibly_sensitive_only": 0,
            "values": 1,
            "possibly_sensitive_values": 0,
        })
        self.assertEqual(result["usable_content"]["lost_to_false_positive_bytes"], 5)

    def test_confirmed_over_extension_keeps_a_document_out_of_ambiguous_only(self):
        outcomes = oc.OutcomeAccumulator()
        outcomes.add_scored(document(), [
            score.Span(3, 12, "custom:a"),  # 5 non-gold bytes around gold 5-9
            score.Span(13, 19, "custom:family:f"),  # "Berlin", 6 non-gold bytes
        ])
        result = outcomes.result()
        self.assertEqual(result["false_positives"], {
            "documents": 1,
            "documents_from_possibly_sensitive_only": 0,
            "values": 2,
            "possibly_sensitive_values": 1,
        })
        self.assertEqual(result["usable_content"]["lost_to_false_positive_bytes"], 11)

    def test_exact_confirmed_value_beside_an_ambiguous_fp_is_ambiguous_only(self):
        outcomes = oc.OutcomeAccumulator()
        outcomes.add_scored(document(), [
            score.Span(5, 9, "custom:a"), score.Span(13, 19, "custom:family:f"),
        ])
        self.assertEqual(
            outcomes.result()["false_positives"]["documents_from_possibly_sensitive_only"], 1
        )

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


def refused_record_copy(target):
    """The v0.17.0 record with one gold and one benign document refused."""
    with gzip.open(RECORD, "rt", encoding="utf-8") as stream:
        rows = [json.loads(line) for line in stream]
    spans = {row["id"]: row["gold"] for row in rows[0]["documents"]}
    chosen = {}
    for row in rows[1:]:
        kind = "gold" if spans[row["document_id"]] else "benign"
        if row["layer"] == "C" and kind not in chosen:
            chosen[kind] = row["document_id"]
            row["response"] = {
                "fixture_id": row["document_id"], "pipeline_error_stage": "clean",
                "pipeline_error_code": (
                    "untransformable_encoded_carrier" if kind == "gold"
                    else oc.POSSIBLY_SENSITIVE_REFUSED_CODE
                ),
                "refused": True, "fallback_redact": False,
            }
    with gzip.open(target, "wt", encoding="utf-8") as stream:
        for row in rows:
            stream.write(json.dumps(row) + "\n")
    return chosen


def assert_partition(test, counters, run):
    """The counters partition the same predictions the byte score uses."""
    protection = counters["protection"]
    metrics = run["metrics"]
    test.assertEqual(
        protection["confirmed"]["values"] + protection["possibly_sensitive"]["values"],
        metrics["prediction_spans"]["total"],
    )
    test.assertEqual(
        protection["confirmed"]["gold_values"] + protection["possibly_sensitive"]["gold_values"],
        metrics["prediction_spans"]["overlapped_gold"],
    )
    test.assertEqual(counters["false_positives"]["documents"],
                     metrics["documents_with_false_positives"])
    test.assertEqual(counters["usable_content"]["lost_to_false_positive_bytes"],
                     metrics["utf8_bytes"]["false_positive"])
    test.assertEqual(counters["attempted_documents"],
                     run["pipeline_availability"]["attempted_documents"])
    test.assertEqual(counters["blocked_operations"]["documents"],
                     run["pipeline_availability"]["failed_closed_documents"])


class RecordReportTests(unittest.TestCase):
    def test_recorded_refusals_reach_the_counters(self):
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / RECORD.name
            chosen = refused_record_copy(path)
            self.assertEqual(set(chosen), {"gold", "benign"})
            counted = oc.report(path, score.SCORED_LABEL_CONTRACT_V1)
            card = record.rescore(path, score.SCORED_LABEL_CONTRACT_V1, max_workers=1)
        (counters,) = counted["runs"]
        (run,) = card["runs"]
        self.assertEqual(counters["config"], run["config"])
        self.assertEqual(counters["blocked_operations"], {
            "documents": 2,
            "by_kind": {"untransformable": 1, "possibly_sensitive": 1, "fail_closed": 0},
        })
        self.assertEqual(counters["untransformable_refused"]["by_reason"]["encoded_carrier"], 1)
        self.assertEqual(counters["fp_refusals"], {"documents": 1})
        self.assertEqual(counters["protection"]["possibly_sensitive"]["dispositions"]["refuse_documents"], 1)
        self.assertGreater(counters["usable_content"]["lost_to_refusal_bytes"], 0)
        assert_partition(self, counters, run)

    def test_generated_layers_are_counted_under_the_layer_contract(self):
        source = BENCH / "observations-v0.15.1-agentic.jsonl.gz"
        contract = score.load_scored_label_contract(BENCH / "scored-labels-v2.json")
        counted = oc.report(source, contract)
        card = record.rescore(source, contract, max_workers=1)
        self.assertEqual(set(counted["layers"]), {"A", "D", "R"})
        for layer in ("A", "D", "R"):
            for counters, run in zip(counted["layers"][layer], card["layers"][layer]["runs"], strict=True):
                with self.subTest(layer=layer, config=run["config"]):
                    self.assertEqual(counters["config"], run["config"])
                    assert_partition(self, counters, run)

    def test_report_cli_writes_the_counters(self):
        with tempfile.TemporaryDirectory() as temporary:
            output = Path(temporary) / "counters.json"
            self.assertEqual(oc.main(["report", str(RECORD), "--output", str(output)]), 0)
            written = json.loads(output.read_text(encoding="utf-8"))
        self.assertEqual(written, oc.report(RECORD, score.SCORED_LABEL_CONTRACT_V1))
        self.assertEqual(written["schema"], oc.REPORT_SCHEMA)


class CommittedRecordProofTests(unittest.TestCase):
    def test_v0170_scorecards_are_unchanged_and_counters_pinned(self):
        evidence_path = BENCH / "outcome-counters-v0.17.0.json"
        evidence = oc.prove(RECORD, "v0.17.0")
        self.assertEqual(json.dumps(evidence, indent=2) + "\n",
                         evidence_path.read_text(encoding="utf-8"))
        self.assertEqual(set(evidence["contracts"]), {"v1", "v2", "v3", "v4"})
        for name, item in evidence["contracts"].items():
            with self.subTest(contract=name):
                self.assertTrue(item["scorecard_byte_identical"])
                committed = bench_json.load(BENCH / item["committed_scorecard"])
                (run,) = committed["runs"]
                (counters,) = item["outcome_counters"]
                self.assertEqual(counters["config"], run["config"])
                assert_partition(self, counters, run)
                self.assertEqual(counters["blocked_operations"]["documents"], 0)
                self.assertEqual(set(evidence["layers"]), {"A", "D", "R"})
                for layer in ("A", "D", "R"):
                    runs = committed["layers"][layer]["runs"]
                    for counters, run in zip(evidence["layers"][layer], runs, strict=True):
                        with self.subTest(layer=layer, config=run["config"]):
                            self.assertEqual(counters["config"], run["config"])
                            assert_partition(self, counters, run)

    def test_proof_detects_a_rescore_that_moves_an_existing_number(self):
        original = record.rescore

        def drifting(*args, **kwargs):
            card = original(*args, **kwargs)
            card["runs"][0]["metrics"]["utf8_bytes"]["leaked"] += 1
            return card

        with mock.patch.object(record, "rescore", drifting), tempfile.TemporaryDirectory() as temporary:
            evidence = oc.prove(RECORD, "v0.17.0")
            self.assertFalse(any(
                item["scorecard_byte_identical"] for item in evidence["contracts"].values()
            ))
            target = Path(temporary) / "evidence.json"
            self.assertEqual(oc.main([
                "prove", str(RECORD), "--release", "v0.17.0", "--write", str(target),
            ]), 1)
            self.assertFalse(target.exists())

    def test_check_mode_fails_on_stale_evidence(self):
        with tempfile.TemporaryDirectory() as temporary:
            stale = Path(temporary) / "evidence.json"
            stale.write_text("{}\n", encoding="utf-8")
            self.assertEqual(oc.main([
                "prove", str(RECORD), "--release", "v0.17.0", "--check", str(stale),
            ]), 1)


if __name__ == "__main__":
    unittest.main()
