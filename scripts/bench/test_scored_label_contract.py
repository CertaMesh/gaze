#!/usr/bin/env python3
"""Scored-label contracts: which corpus labels count as gold PII."""

import copy
import json
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parent))

import gaze_bench_score as score
import render_benchmark_doc as render
import run_no_opf_benchmark as runner
import test_render_benchmark_doc as render_tests
import test_run_no_opf_benchmark as runner_tests


REPO_ROOT = Path(__file__).resolve().parents[2]
V2_PATH = REPO_ROOT / "docs/reference/benchmarks/scored-labels-v2.json"

# The 29 labels observed in the pinned Dataiku EN/DE selection.
CORPUS_LABELS = frozenset(
    {
        "AGE", "BUILDINGNUM", "CITY", "COMPANYNAME", "COUNTRY",
        "CREDITCARDNUMBER", "DATEOFBIRTH", "DRIVERLICENSENUM", "EMAIL",
        "FIRSTNAME", "IBAN", "IDCARDNUM", "LICENSEPLATENUM", "NATIONALID",
        "ORGANIZATION", "PASSPORTID", "PASSWORD", "PHONENUMBER", "REGION",
        "SECURITYTOKEN", "SSN", "STATE", "STREET", "SURNAME", "TAXNUM",
        "TITLE", "URL", "USERNAME", "ZIP",
    }
)

TEXT = "mail a@b.io pw hunter22 end"
EMAIL = score.Span(5, 11, "EMAIL")
PASSWORD = score.Span(15, 23, "PASSWORD")


def document(*spans: score.Span, uid: str = "synthetic-contract-1") -> score.Document:
    return score.Document(
        uid=uid,
        text=TEXT,
        language="en",
        region="US",
        source_dataset="synthetic",
        spans=tuple(spans),
    )


def v2() -> score.ScoredLabelContract:
    return score.load_scored_label_contract(V2_PATH)


def metrics(doc: score.Document, *predictions: score.Span) -> dict:
    accumulator = score.MetricAccumulator()
    accumulator.add(doc, list(predictions))
    return accumulator.result()


def write_contract(directory: str, labels: list[dict]) -> Path:
    path = Path(directory) / "contract.json"
    path.write_text(
        json.dumps(
            {
                "schema_version": 1,
                "contract": "scored-labels-test",
                "contract_version": 2,
                "labels": labels,
            }
        ),
        encoding="utf-8",
    )
    return path


class CommittedContractTests(unittest.TestCase):
    def test_v2_rules_on_every_corpus_label_and_excludes_only_credentials(self) -> None:
        contract = v2()
        self.assertEqual(contract.contract_id, "scored-labels-v2")
        self.assertEqual(contract.version, 2)
        self.assertEqual(contract.excluded_labels, frozenset({"PASSWORD", "SECURITYTOKEN"}))
        self.assertEqual(
            contract.neutral_prediction_classes,
            frozenset({"custom:password", "custom:security_token", "custom:secret"}),
        )
        self.assertEqual(contract.scored_labels | contract.excluded_labels, CORPUS_LABELS)
        self.assertRegex(contract.sha256, r"^[0-9a-f]{64}$")

    def test_v2_marks_the_open_rulings_pending(self) -> None:
        entries = json.loads(V2_PATH.read_text(encoding="utf-8"))["labels"]
        pending = {entry["label"] for entry in entries if entry["ruling"] == "pending"}
        self.assertEqual(
            pending, {"USERNAME", "URL", "COMPANYNAME", "COUNTRY", "STATE"}
        )
        self.assertTrue(all(entry["scored"] for entry in entries if entry["label"] in pending))

    def test_default_is_implicit_v1_scoring_everything(self) -> None:
        contract = runner.load_scored_label_contract(REPO_ROOT, None)
        self.assertIs(contract, score.SCORED_LABEL_CONTRACT_V1)
        doc = document(EMAIL, PASSWORD)
        self.assertEqual(score.apply_scored_label_contract([doc], contract), [doc])

    def test_runner_accepts_the_flag(self) -> None:
        args = runner.parse_args(["quick", "--scored-labels", str(V2_PATH)])
        self.assertEqual(args.scored_labels, V2_PATH)
        self.assertIsNone(runner.parse_args(["quick"]).scored_labels)
        contract = runner.load_scored_label_contract(REPO_ROOT, args.scored_labels)
        self.assertEqual(contract.path, "docs/reference/benchmarks/scored-labels-v2.json")


class ExclusionScoringTests(unittest.TestCase):
    def test_excluded_label_contributes_nothing_to_leaked_or_gold_bytes(self) -> None:
        doc = document(EMAIL, PASSWORD)
        v1_result = metrics(doc, EMAIL)
        self.assertEqual(v1_result["utf8_bytes"]["pii"], 14)
        self.assertEqual(v1_result["utf8_bytes"]["leaked"], 8)
        self.assertEqual(v1_result["entities"]["gold"], 2)

        (applied,) = score.apply_scored_label_contract([doc], v2())
        v2_result = metrics(applied, EMAIL)
        self.assertEqual(v2_result["utf8_bytes"]["pii"], 6)
        self.assertEqual(v2_result["utf8_bytes"]["leaked"], 0)
        self.assertEqual(v2_result["entities"]["gold"], 1)
        self.assertEqual(v2_result["documents_without_leaks"], 1)

    def test_protecting_an_excluded_span_is_not_a_false_positive(self) -> None:
        (applied,) = score.apply_scored_label_contract([document(EMAIL, PASSWORD)], v2())
        result = metrics(applied, EMAIL, score.Span(15, 23, "name"))
        self.assertEqual(result["utf8_bytes"]["false_positive"], 0)
        self.assertEqual(result["utf8_bytes"]["predicted"], 6)
        self.assertEqual(result["prediction_spans"]["total"], 1)
        self.assertEqual(result["documents_with_false_positives"], 0)

    def test_bytes_beyond_the_excluded_span_stay_false_positive(self) -> None:
        (applied,) = score.apply_scored_label_contract([document(EMAIL, PASSWORD)], v2())
        result = metrics(applied, EMAIL, score.Span(12, 23, "name"))
        self.assertEqual(result["utf8_bytes"]["false_positive"], 3)
        self.assertEqual(result["prediction_spans"]["total"], 2)
        # The ignored bytes leave the non-PII denominator: 27 - 6 gold - 8 ignored.
        self.assertAlmostEqual(result["utf8_bytes"]["false_positive_rate"], 3 / 13)

    def test_scored_gold_overlapping_an_excluded_span_is_still_scored(self) -> None:
        overlapping_email = score.Span(15, 20, "EMAIL")
        (applied,) = score.apply_scored_label_contract(
            [document(overlapping_email, PASSWORD)], v2()
        )
        result = metrics(applied)
        self.assertEqual(result["utf8_bytes"]["pii"], 5)
        self.assertEqual(result["utf8_bytes"]["leaked"], 5)

    def test_neutral_prediction_class_is_not_a_false_positive(self) -> None:
        (applied,) = score.apply_scored_label_contract([document(EMAIL)], v2())
        v1_result = metrics(document(EMAIL), EMAIL, score.Span(15, 23, "custom:security_token"))
        self.assertEqual(v1_result["utf8_bytes"]["false_positive"], 8)
        result = metrics(applied, EMAIL, score.Span(15, 23, "custom:security_token"))
        self.assertEqual(result["utf8_bytes"]["false_positive"], 0)
        self.assertEqual(result["prediction_spans"]["total"], 1)
        # Any other class on the same bytes stays a false positive.
        other = metrics(applied, EMAIL, score.Span(15, 23, "custom:url"))
        self.assertEqual(other["utf8_bytes"]["false_positive"], 8)

    def test_neutral_prediction_class_still_protects_scored_gold(self) -> None:
        (applied,) = score.apply_scored_label_contract([document(EMAIL)], v2())
        result = metrics(applied, score.Span(5, 11, "custom:password"))
        self.assertEqual(result["utf8_bytes"]["leaked"], 0)
        self.assertEqual(result["utf8_bytes"]["true_positive"], 6)

    def test_unruled_corpus_label_fails_closed(self) -> None:
        with self.assertRaisesRegex(score.ScoredLabelContractError, "NEWLABEL"):
            score.apply_scored_label_contract(
                [document(score.Span(0, 4, "NEWLABEL"))], v2()
            )

    def test_malformed_contract_entries_fail_closed(self) -> None:
        good = {"label": "EMAIL", "scored": True, "ruling": "settled", "reason": "x"}
        cases = {
            "duplicates": [good, good],
            "boolean": [{**good, "scored": "yes"}],
            "reason": [{**good, "reason": " "}],
            "ruling": [{**good, "ruling": "maybe"}],
        }
        for expected, labels in cases.items():
            with self.subTest(expected), tempfile.TemporaryDirectory() as directory:
                with self.assertRaisesRegex(score.ScoredLabelContractError, expected):
                    score.load_scored_label_contract(write_contract(directory, labels))

    def test_subtract_intervals(self) -> None:
        self.assertEqual(
            score.subtract_intervals([(0, 10), (20, 30)], [(2, 4), (8, 22), (29, 40)]),
            [(0, 2), (4, 8), (22, 29)],
        )
        self.assertEqual(score.subtract_intervals([(0, 5)], []), [(0, 5)])
        self.assertEqual(score.subtract_intervals([(3, 5)], [(0, 9)]), [])


class RunnerWiringTests(unittest.TestCase):
    """run() must score the contract it records, on the documents it measures."""

    class Stop(Exception):
        pass

    def run_until_scorecard(self, *flags: str) -> tuple[list, dict]:
        measured: list = []
        recorded: dict = {}

        def execute_measurements(**kwargs: object) -> tuple[list, list]:
            measured.extend(kwargs["documents"])
            return [], []

        def assemble_scorecard(**kwargs: object) -> None:
            recorded.update(kwargs)
            raise self.Stop

        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for name in ("dataset.parquet", "clean_for_bench", "probe"):
                (root / name).write_bytes(b"")
            args = runner.parse_args(
                ["full", "--no-download", "--dataset", str(root / "dataset.parquet"),
                 "--output-dir", str(root / "out"), *flags]
            )
            positive = [document(EMAIL, PASSWORD)]
            with mock.patch.object(runner, "validate_required_models", return_value={}), \
                    mock.patch.object(runner.dataiku, "verify_dataset"), \
                    mock.patch.object(runner.dataiku, "load_documents", return_value=(positive, {})), \
                    mock.patch.object(runner, "load_negative_documents", return_value=([], {})), \
                    mock.patch.object(runner.dataiku, "build_binary", return_value=root / "clean_for_bench"), \
                    mock.patch.object(score, "build_validator_probe", return_value=root / "probe"), \
                    mock.patch.object(score, "collect_validator_measurements", return_value={}), \
                    mock.patch.object(runner, "execute_measurements", side_effect=execute_measurements), \
                    mock.patch.object(runner, "composite_dataset_report", return_value=({}, {})), \
                    mock.patch.object(score, "validator_gold_census", return_value={}), \
                    mock.patch.object(score, "assemble_scorecard", side_effect=assemble_scorecard):
                with self.assertRaises(self.Stop):
                    runner.run(args)
        return measured, recorded["scored_label_contract"]

    def test_v2_flag_scores_and_records_v2(self) -> None:
        measured, contract = self.run_until_scorecard("--scored-labels", str(V2_PATH))
        self.assertEqual([span.label for span in measured[0].spans], ["EMAIL"])
        self.assertEqual(contract["id"], "scored-labels-v2")
        self.assertEqual(contract["file_sha256"], v2().sha256)
        self.assertEqual(contract["excluded_gold_utf8_bytes"], 8)

    def test_no_flag_scores_and_records_v1(self) -> None:
        measured, contract = self.run_until_scorecard()
        self.assertEqual([span.label for span in measured[0].spans], ["EMAIL", "PASSWORD"])
        self.assertEqual((contract["id"], contract["version"]), ("scored-labels-v1", 1))


class ContractProvenanceTests(unittest.TestCase):
    def test_report_digest_changes_with_the_contract_and_is_recorded(self) -> None:
        docs = [document(EMAIL, PASSWORD)]
        v1_report = score.scored_label_contract_report(score.SCORED_LABEL_CONTRACT_V1, docs)
        v2_report = score.scored_label_contract_report(
            v2(), score.apply_scored_label_contract(docs, v2())
        )
        self.assertNotEqual(v1_report["scored_gold_digest"], v2_report["scored_gold_digest"])
        self.assertEqual(
            (v1_report["scored_gold_utf8_bytes"], v1_report["excluded_gold_utf8_bytes"]),
            (14, 0),
        )
        self.assertEqual(
            (v2_report["scored_gold_utf8_bytes"], v2_report["excluded_gold_utf8_bytes"]),
            (6, 8),
        )
        self.assertEqual(v2_report["excluded_labels"], ["PASSWORD", "SECURITYTOKEN"])
        self.assertEqual(
            v2_report["neutral_prediction_classes"],
            ["custom:password", "custom:secret", "custom:security_token"],
        )
        self.assertEqual(v2_report["file_sha256"], v2().sha256)

        card = score.assemble_scorecard(
            repo_root=REPO_ROOT,
            dataset_metadata={},
            dataset_report={},
            sampling_report={"available_population": {}, "evaluated_population": {}},
            parameters={},
            runs=[],
            scored_label_contract=v2_report,
        )
        self.assertEqual(card["scoring"]["scored_label_contract"], v2_report)
        self.assertEqual(
            score.scorecard_scored_label_contract_identity(card),
            ("scored-labels-v2", 2, v2().sha256),
        )

    def test_comparator_refuses_to_compare_across_contracts(self) -> None:
        baseline = runner_tests.scorecard()
        candidate = copy.deepcopy(baseline)
        self.assertTrue(score.compare_scorecards(candidate, baseline)["regression"]["passed"])

        candidate["scoring"] = {
            "scored_label_contract": {"id": "scored-labels-v1", "version": 1, "file_sha256": None}
        }
        self.assertTrue(score.compare_scorecards(candidate, baseline)["regression"]["passed"])

        candidate["scoring"]["scored_label_contract"] = {
            "id": "scored-labels-v2", "version": 2, "file_sha256": "a" * 64
        }
        regression = score.compare_scorecards(candidate, baseline)["regression"]
        self.assertFalse(regression["passed"])
        self.assertIn(
            "scored_label_contract_match",
            {failure["gate"] for failure in regression["failures"]},
        )


class RendererContractTests(unittest.TestCase):
    def test_v1_rows_carry_no_contract_and_render_unlabeled(self) -> None:
        entry = render_tests.entry()
        self.assertNotIn("scored_label_contract", entry)
        history = {**render.empty_history(), "releases": [entry]}
        self.assertNotIn("scored labels", render.render_history(history))

    def test_v2_rows_are_labeled_with_their_contract(self) -> None:
        card = render_tests.scorecard()
        card["scoring"] = {
            "scored_label_contract": {
                "id": "scored-labels-v2",
                "version": 2,
                "file_sha256": "e" * 64,
                "excluded_labels": ["PASSWORD"],
            }
        }
        entry = render.history_entry_from_scorecard(
            card,
            version="v0.16.0",
            machine="Test host, 1 core, 1 GB",
            scorecard_filename="scorecard-v0.16.0.json",
            scorecard_sha256="0" * 64,
        )
        self.assertEqual(entry["scored_label_contract"]["version"], 2)
        history = {**render.empty_history(), "releases": [entry]}
        self.assertIn("v0.16.0 · scored labels v2", render.render_history(history))
        self.assertIn("out of contract: PASSWORD", render.render_current_release(history))


if __name__ == "__main__":
    unittest.main()
