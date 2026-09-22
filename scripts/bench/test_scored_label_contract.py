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
V3_PATH = REPO_ROOT / "docs/reference/benchmarks/scored-labels-v3.json"

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

    def test_trend_line_never_joins_rows_from_different_contracts(self) -> None:
        v2_block = {
            "id": "scored-labels-v2",
            "version": 2,
            "file_sha256": "e" * 64,
            "excluded_labels": ["PASSWORD"],
        }
        rows = [render_tests.entry(version) for version in ("v0.13.0", "v0.14.0", "v0.15.0")]
        for row, surviving in zip(rows, (30000, 25000, 20000)):
            row["arms"][render.SHIPPED_DEFAULT_ARM]["surviving_pii_utf8_bytes"] = surviving
        rows[1]["scored_label_contract"] = dict(v2_block)
        rows[2]["scored_label_contract"] = dict(v2_block)
        history = {**render.empty_history(), "releases": rows}

        charts = render.render_charts(history)
        self.assertIn("line [25000, 20000]", charts)
        self.assertIn("1 row(s) under another contract", charts)

        # A different v2 file is a different contract too.
        rows[1]["scored_label_contract"]["file_sha256"] = "d" * 64
        charts = render.render_charts(history)
        self.assertNotIn("line [", charts)
        self.assertIn("2 row(s) under another contract", charts)

        # History measured under one contract renders exactly as before.
        for row in rows:
            row.pop("scored_label_contract", None)
        self.assertIn("line [30000, 25000, 20000]", render.render_charts(history))
        self.assertNotIn("another contract", render.render_charts(history))


def v3() -> score.ScoredLabelContract:
    return score.load_scored_label_contract(V3_PATH)


def gap_document(text: str, *spans: score.Span, uid: str = "synthetic-gap-1") -> score.Document:
    """A document with byte-offset gold spans located by value."""
    return score.Document(
        uid=uid,
        text=text,
        language="en",
        region="US",
        source_dataset="synthetic",
        spans=tuple(spans),
    )


def at(text: str, value: str, label: str, occurrence: int = 0) -> score.Span:
    """Span over the `occurrence`-th byte-level occurrence of `value` in `text`."""
    raw = text.encode("utf-8")
    needle = value.encode("utf-8")
    start = -1
    for _ in range(occurrence + 1):
        start = raw.index(needle, start + 1)
    return score.Span(start, start + len(needle), label)


def under(contract: score.ScoredLabelContract, doc: score.Document) -> score.Document:
    (applied,) = score.apply_scored_label_contract([doc], contract)
    return applied


def gap(result: dict) -> dict:
    return result["gold_gap"]


EMMA_TEXT = "My name is Emma Clarke. Emma likes tea."


def emma_document() -> score.Document:
    return gap_document(
        EMMA_TEXT,
        at(EMMA_TEXT, "Emma", "FIRSTNAME"),
        at(EMMA_TEXT, "Clarke", "SURNAME"),
    )


def emma_predictions(repeat_class: str = "name") -> list[score.Span]:
    first = at(EMMA_TEXT, "Emma Clarke", "name")
    repeat = at(EMMA_TEXT, "Emma", repeat_class, occurrence=1)
    return [first, repeat]


class GoldGapContractTests(unittest.TestCase):
    """Contract v3 loader: the gold_gap block is closed and fails closed."""

    def v3_value(self) -> dict:
        return json.loads(V3_PATH.read_text(encoding="utf-8"))

    def load(self, value: dict) -> score.ScoredLabelContract:
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "contract.json"
            path.write_text(json.dumps(value), encoding="utf-8")
            return score.load_scored_label_contract(path)

    def test_v3_labels_are_v2_labels(self) -> None:
        contract = v3()
        self.assertEqual(contract.contract_id, "scored-labels-v3")
        self.assertEqual(contract.version, 3)
        self.assertEqual(contract.scored_labels, v2().scored_labels)
        self.assertEqual(contract.excluded_labels, v2().excluded_labels)
        self.assertEqual(
            contract.neutral_prediction_classes, v2().neutral_prediction_classes
        )
        self.assertIsNone(v2().gold_gap)
        self.assertEqual(
            self.v3_value()["labels"],
            json.loads(V2_PATH.read_text(encoding="utf-8"))["labels"],
        )

    def test_every_scored_label_is_ruled_on_once(self) -> None:
        block = self.v3_value()["gold_gap"]
        creditable = {
            label for labels in block["compatible_labels"].values() for label in labels
        }
        not_creditable = {entry["label"] for entry in block["not_creditable"]}
        self.assertFalse(creditable & not_creditable)
        self.assertEqual(creditable | not_creditable, v3().scored_labels)

    def test_malformed_gold_gap_fails_closed(self) -> None:
        def with_block(mutate) -> dict:
            value = self.v3_value()
            mutate(value)
            return value

        cases = {
            "needs a gold_gap block": lambda v: v.pop("gold_gap"),
            "must be an object": lambda v: v.update(gold_gap=[]),
            "unknown keys": lambda v: v["gold_gap"].update(case_fold=True),
            "missing keys": lambda v: v["gold_gap"].pop("boundary"),
            "same_document must be True": lambda v: v["gold_gap"].update(
                same_document=False
            ),
            "match must be": lambda v: v["gold_gap"].update(match="case_insensitive"),
            "trim must be": lambda v: v["gold_gap"].update(trim="unicode_whitespace"),
            "status must be": lambda v: v["gold_gap"].update(status="headline"),
            "does not rule on scored labels": lambda v: v["gold_gap"].update(
                not_creditable=[]
            ),
            "not a scored label": lambda v: v["gold_gap"]["compatible_labels"].update(
                {"custom:password": ["PASSWORD"]}
            ),
            "rules on AGE twice": lambda v: v["gold_gap"]["compatible_labels"].update(
                {"custom:age": ["AGE"]}
            ),
            "lists a label twice": lambda v: v["gold_gap"]["compatible_labels"].update(
                {"name": ["FIRSTNAME", "FIRSTNAME", "SURNAME"]}
            ),
            "needs contract_version 3": lambda v: v.update(contract_version=2),
            "not supported by this scorer": lambda v: v.update(contract_version=4),
        }
        for expected, mutate in cases.items():
            with self.subTest(expected):
                with self.assertRaisesRegex(score.ScoredLabelContractError, expected):
                    self.load(with_block(mutate))

    def test_report_names_the_diagnostic_only_under_v3(self) -> None:
        doc = emma_document()
        v3_report = score.scored_label_contract_report(v3(), [under(v3(), doc)])
        v2_report = score.scored_label_contract_report(v2(), [under(v2(), doc)])
        self.assertEqual(v3_report["gold_gap"], {"status": "diagnostic"})
        self.assertNotIn("gold_gap", v2_report)
        self.assertEqual(v3_report["version"], 3)


class GoldGapScoringTests(unittest.TestCase):
    """Contract v3 scoring: which unlabelled repeats count as gold-gap protection."""

    def credited(self, doc: score.Document, predictions: list[score.Span]) -> dict:
        return gap(metrics(under(v3(), doc), *predictions))

    def assert_not_credited(self, doc: score.Document, predictions: list[score.Span]) -> None:
        result = metrics(under(v3(), doc), *predictions)
        self.assertEqual(gap(result)["gold_gap_protected_bytes"], 0)
        self.assertEqual(
            gap(result)["false_positive_bytes_after_gold_gap"],
            result["utf8_bytes"]["false_positive"],
        )

    def test_exact_repeat_is_credited(self) -> None:
        result = metrics(under(v3(), emma_document()), *emma_predictions())
        self.assertEqual(result["utf8_bytes"]["false_positive"], 5)  # " " + "Emma"
        self.assertEqual(gap(result)["gold_gap_protected_bytes"], 4)
        self.assertEqual(gap(result)["gold_gap_protected_bytes_by_label"], {"FIRSTNAME": 4})
        self.assertEqual(gap(result)["false_positive_bytes_after_gold_gap"], 1)
        self.assertEqual(gap(result)["status"], "diagnostic")

    def test_case_variant_is_not_credited(self) -> None:
        text = "My name is Emma Clarke. EMMA likes tea."
        doc = gap_document(text, at(text, "Emma", "FIRSTNAME"), at(text, "Clarke", "SURNAME"))
        self.assert_not_credited(doc, [at(text, "EMMA", "name")])

    def test_substring_of_a_gold_value_is_not_credited(self) -> None:
        text = "Her name is Annabel. Anna likes tea."
        doc = gap_document(text, at(text, "Annabel", "FIRSTNAME"))
        self.assert_not_credited(doc, [at(text, "Anna", "name", occurrence=1)])

    def test_superstring_of_a_gold_value_is_not_credited(self) -> None:
        text = "My name is Emma Clarke. Emma Clarke likes tea."
        doc = gap_document(text, at(text, "Emma", "FIRSTNAME"), at(text, "Clarke", "SURNAME"))
        self.assert_not_credited(doc, [at(text, "Emma Clarke", "name", occurrence=1)])

    def test_cross_document_repeat_is_not_credited(self) -> None:
        other_text = "Emma likes tea."
        other = gap_document(other_text, uid="synthetic-gap-2")
        accumulator = score.MetricAccumulator()
        accumulator.add(under(v3(), emma_document()), emma_predictions()[:1])
        accumulator.add(under(v3(), other), [at(other_text, "Emma", "name")])
        result = accumulator.result()
        self.assertEqual(result["utf8_bytes"]["false_positive"], 5)  # " " + "Emma"
        self.assertEqual(gap(result)["gold_gap_protected_bytes"], 0)

    def test_class_mismatch_is_not_credited(self) -> None:
        self.assert_not_credited(emma_document(), emma_predictions("location"))
        self.assert_not_credited(emma_document(), emma_predictions("custom:phone"))

    def test_padded_span_credits_only_the_trimmed_bytes(self) -> None:
        text = "Alice met Bob.  Alice  went home."
        doc = gap_document(text, at(text, "Alice", "FIRSTNAME"), at(text, "Bob", "FIRSTNAME"))
        padded = at(text, "  Alice  ", "name")
        result = metrics(under(v3(), doc), padded)
        self.assertEqual(result["utf8_bytes"]["false_positive"], 9)
        self.assertEqual(gap(result)["gold_gap_protected_bytes"], 5)
        self.assertEqual(gap(result)["false_positive_bytes_after_gold_gap"], 4)

    def test_padding_with_newlines_and_tabs_is_trimmed(self) -> None:
        text = "City: Berlin\n\tBerlin\n"
        doc = gap_document(text, at(text, "Berlin", "CITY"))
        result = metrics(under(v3(), doc), at(text, "\n\tBerlin\n", "location"))
        self.assertEqual(gap(result)["gold_gap_protected_bytes"], 6)
        self.assertEqual(gap(result)["false_positive_bytes_after_gold_gap"], 3)

    def test_word_boundary_violations_are_not_credited(self) -> None:
        cases = {
            "Berliner": ("City: Berlin. Ein Berliner kam.", "Berlin", "CITY", "location"),
            "Berlinbesuch": ("City: Berlin. Ein Berlinbesuch.", "Berlin", "CITY", "location"),
            "Meiers": ("Herr Meier. Das Haus des Meiers.", "Meier", "SURNAME", "name"),
            "Annas": ("Name: Anna. Annas Hund bellt.", "Anna", "FIRSTNAME", "name"),
            "digit": ("Name: Anna. Anna2 ist ein Login.", "Anna", "FIRSTNAME", "name"),
            "leading letter": ("Name: Anna. MaryAnna kam.", "Anna", "FIRSTNAME", "name"),
            "NFD mark": ("Herr Meier. Meieŕ kam.", "Meier", "SURNAME", "name"),
            # Rust calls these alphanumeric, so is_inside_word would too.
            "circled letter": ("Name: Anna. x AnnaⒶ y", "Anna", "FIRSTNAME", "name"),
            "newer Unicode letter": (
                "Name: Anna. x Anna\U000323b0 y", "Anna", "FIRSTNAME", "name"
            ),
        }
        for name, (text, value, label, predicted_class) in cases.items():
            with self.subTest(name):
                doc = gap_document(text, at(text, value, label))
                repeat = at(text, value, predicted_class, occurrence=1)
                self.assert_not_credited(doc, [repeat])

    def test_word_character_covers_every_rust_alphanumeric(self) -> None:
        # The boundary must reject every edge Gaze's `is_inside_word` would:
        # each code point Rust's `char::is_alphanumeric` accepts under the
        # pinned toolchain is a word character here too.
        table = json.loads(
            (Path(__file__).resolve().parent / "fixtures/rust-char-is-alphanumeric.json")
            .read_text(encoding="utf-8")
        )
        missing = [
            code_point
            for first, last in table["ranges"]
            for code_point in range(first, last + 1)
            if not score._is_word_character(chr(code_point))
        ]
        self.assertEqual(
            sum(last - first + 1 for first, last in table["ranges"]), table["code_points"]
        )
        self.assertEqual(missing, [])

    def test_punctuation_neighbours_are_boundaries(self) -> None:
        cases = {
            "Berlin-Reise": ("City: Berlin. Die Berlin-Reise.", "Berlin", "CITY", "location"),
            "apostrophe": ("Name: Anna. Anna's dog.", "Anna", "FIRSTNAME", "name"),
            "underscore": ("Name: Anna. Anna_x wrote.", "Anna", "FIRSTNAME", "name"),
            "end of text": ("Name: Anna. Bye Anna", "Anna", "FIRSTNAME", "name"),
            "non-ascii neighbour value": ("Stadt: Köln. Nach Köln.", "Köln", "CITY", "location"),
        }
        for name, (text, value, label, predicted_class) in cases.items():
            with self.subTest(name):
                doc = gap_document(text, at(text, value, label))
                repeat = at(text, value, predicted_class, occurrence=1)
                result = self.credited(doc, [repeat])
                self.assertEqual(
                    result["gold_gap_protected_bytes"], len(value.encode("utf-8"))
                )

    def test_same_document_homonym_is_credited_by_the_rule(self) -> None:
        # The blind spot the human audit exists for: byte equality is not
        # identity. The rule credits it; the audit decides whether it may.
        text = "May Example submitted the form. Delivery is scheduled for May."
        doc = gap_document(text, at(text, "May", "FIRSTNAME"), at(text, "Example", "SURNAME"))
        result = self.credited(doc, [at(text, "May", "name", occurrence=1)])
        self.assertEqual(result["gold_gap_protected_bytes"], 3)

    def test_overlap_with_gold_or_ignored_bytes_is_not_credited(self) -> None:
        text = "Emma pw Emma end Emma"
        doc = gap_document(
            text,
            at(text, "Emma", "FIRSTNAME"),
            at(text, "Emma", "PASSWORD", occurrence=1),
        )
        applied = under(v3(), doc)
        # Touching gold: the prediction spills one byte into the gold span.
        spill = score.Span(3, 4, "name")
        self.assertEqual(gap(metrics(applied, spill))["gold_gap_protected_bytes"], 0)
        # Touching an excluded-label span (v2 ignores those bytes).
        on_excluded = at(text, "Emma", "name", occurrence=1)
        self.assertEqual(gap(metrics(applied, on_excluded))["gold_gap_protected_bytes"], 0)
        # Touching a neutral prediction class's bytes.
        neutral = score.Span(at(text, "end", "x").start, at(text, "end", "x").end, "custom:secret")
        wide = score.Span(neutral.start, at(text, "Emma", "x", occurrence=2).end, "name")
        self.assertEqual(
            gap(metrics(applied, neutral, wide))["gold_gap_protected_bytes"], 0
        )
        # The clean repeat is credited, so the fixture itself can credit.
        clean = at(text, "Emma", "name", occurrence=2)
        self.assertEqual(gap(metrics(applied, clean))["gold_gap_protected_bytes"], 4)

    def test_duplicate_and_overlapping_predictions_are_credited_once(self) -> None:
        text = "City: Nelson. Name: Nelson Park. Nelson again."
        doc = gap_document(
            text,
            at(text, "Nelson", "CITY"),
            at(text, "Nelson", "FIRSTNAME", occurrence=1),
            at(text, "Park", "SURNAME"),
        )
        repeat_name = at(text, "Nelson", "name", occurrence=2)
        repeat_location = at(text, "Nelson", "location", occurrence=2)
        result = self.credited(doc, [repeat_name, repeat_name, repeat_location])
        self.assertEqual(result["gold_gap_protected_bytes"], 6)
        # First compatible gold span in document order: the CITY.
        self.assertEqual(result["gold_gap_protected_bytes_by_label"], {"CITY": 6})
        self.assertEqual(result["gold_gap_protected_ranges"], 1)

    def test_conservation_holds_per_document(self) -> None:
        text = "Alice met Bob.  Alice  went. Bob, Berliner, Alice."
        doc = gap_document(text, at(text, "Alice", "FIRSTNAME"), at(text, "Bob", "FIRSTNAME"))
        predictions = [
            at(text, "Alice", "name"),
            at(text, "  Alice  ", "name"),
            at(text, "Bob", "name", occurrence=1),
            at(text, "Berliner", "location"),
            at(text, "Alice", "name", occurrence=2),
            at(text, "went", "organization"),
        ]
        result = metrics(under(v3(), doc), *predictions)
        utf8 = result["utf8_bytes"]
        self.assertEqual(
            utf8["predicted"],
            utf8["true_positive"]
            + gap(result)["false_positive_bytes_after_gold_gap"]
            + gap(result)["gold_gap_protected_bytes"],
        )
        self.assertEqual(gap(result)["gold_gap_protected_bytes"], 5 + 3 + 5)
        self.assertAlmostEqual(
            gap(result)["adjusted_precision"],
            utf8["true_positive"]
            / (utf8["predicted"] - gap(result)["gold_gap_protected_bytes"]),
        )

    def test_v2_numbers_are_unchanged_under_v3(self) -> None:
        text = "Alice met Bob.  Alice  went. Bob, Berliner, Alice. pw hunter22"
        doc = gap_document(
            text,
            at(text, "Alice", "FIRSTNAME"),
            at(text, "Bob", "FIRSTNAME"),
            at(text, "hunter22", "PASSWORD"),
        )
        predictions = [
            at(text, "Alice", "name"),
            at(text, "  Alice  ", "name"),
            at(text, "Bob", "name", occurrence=1),
            at(text, "Berliner", "location"),
            at(text, "hunter22", "custom:password"),
        ]
        v2_result = metrics(under(v2(), doc), *predictions)
        v3_result = metrics(under(v3(), doc), *predictions)
        self.assertNotIn("gold_gap", v2_result)
        self.assertGreater(gap(v3_result)["gold_gap_protected_bytes"], 0)
        v3_result.pop("gold_gap")
        self.assertEqual(v3_result, v2_result)
        self.assertEqual(
            json.dumps(v3_result, sort_keys=True), json.dumps(v2_result, sort_keys=True)
        )

    def test_gold_gap_never_runs_under_v1_or_v2(self) -> None:
        with mock.patch.object(
            score, "gold_gap_credits", side_effect=AssertionError("called")
        ):
            metrics(emma_document(), *emma_predictions())
            metrics(under(v2(), emma_document()), *emma_predictions())
            with self.assertRaisesRegex(AssertionError, "called"):
                metrics(under(v3(), emma_document()), *emma_predictions())



class GoldGapRenderTests(unittest.TestCase):
    """A v3 row prints the diagnostic beside the unchanged v2 columns."""

    def card(self, version: int, gold_gap: bool) -> dict:
        card = render_tests.scorecard()
        card["scoring"] = {
            "scored_label_contract": {
                "id": f"scored-labels-v{version}",
                "version": version,
                "file_sha256": "e" * 64,
                "excluded_labels": ["PASSWORD"],
            }
        }
        if gold_gap:
            for index, run in enumerate(card["runs"]):
                run["metrics"]["gold_gap"] = {
                    "status": "diagnostic",
                    "gold_gap_protected_bytes": 1000 + index,
                    "gold_gap_protected_bytes_by_label": {
                        "CITY": 400,
                        "FIRSTNAME": 600 + index,
                    },
                    "gold_gap_protected_ranges": 10,
                    "gold_gap_protected_ranges_by_label": {"CITY": 4, "FIRSTNAME": 6},
                    "false_positive_bytes_after_gold_gap": 2000,
                    "adjusted_precision": 0.5,
                }
        return card

    def entry(self, card: dict) -> dict:
        return render.history_entry_from_scorecard(
            card,
            version="v0.16.0",
            machine="Test host, 1 core, 1 GB",
            scorecard_filename="scorecard-v0.16.0.json",
            scorecard_sha256="0" * 64,
        )

    def test_v3_row_renders_the_diagnostic_beside_the_headline(self) -> None:
        v3_entry = self.entry(self.card(3, gold_gap=True))
        v2_entry = self.entry(self.card(2, gold_gap=False))
        arm = v3_entry["arms"][render.SHIPPED_DEFAULT_ARM]
        self.assertEqual(arm["gold_gap"]["gold_gap_protected_bytes"], 1001)
        # The headline columns are the v2 numbers, identical in both rows.
        for name, block in v2_entry["arms"].items():
            headline = {k: v for k, v in v3_entry["arms"][name].items() if k != "gold_gap"}
            self.assertEqual(headline, block)
        history = {**render.empty_history(), "releases": [v3_entry]}
        render.validate_history(history)
        text = render.render_current_release(history)
        self.assertIn("diagnostic; v2 headline unchanged", text)
        self.assertIn("| `pass2-ner` | 1,001 | 2,000 | 0.500000 |", text)
        self.assertIn("False-positive bytes ↔", text)
        v2_text = render.render_current_release(
            {**render.empty_history(), "releases": [v2_entry]}
        )
        self.assertNotIn("Gold-gap", v2_text)

    def test_gold_gap_outside_v3_or_malformed_fails_closed(self) -> None:
        with self.assertRaisesRegex(render.RenderError, "only there"):
            self.entry(self.card(2, gold_gap=True))
        with self.assertRaisesRegex(render.RenderError, "only there"):
            self.entry(self.card(3, gold_gap=False))
        card = self.card(3, gold_gap=True)
        card["runs"][0]["metrics"]["gold_gap"]["gold_gap_protected_bytes"] += 1
        with self.assertRaisesRegex(render.RenderError, "do not sum"):
            self.entry(card)
        card = self.card(3, gold_gap=True)
        card["runs"][0]["metrics"]["gold_gap"]["status"] = "headline"
        with self.assertRaisesRegex(render.RenderError, "diagnostic"):
            self.entry(card)
        v3_entry = self.entry(self.card(3, gold_gap=True))
        del v3_entry["arms"][render.SHIPPED_DEFAULT_ARM]["gold_gap"]["adjusted_precision"]
        with self.assertRaisesRegex(render.RenderError, "malformed gold_gap"):
            render.validate_history({**render.empty_history(), "releases": [v3_entry]})


if __name__ == "__main__":
    unittest.main()
