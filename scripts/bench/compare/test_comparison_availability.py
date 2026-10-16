"""Unavailable rows require receipts and remain visible without numeric imputation."""

import copy
import unittest

from comparison_availability import REASON, digest, unavailable_tools
from finalize_report import finalize
from test_finalize_report import report as complete_report
import benchmark_charts as charts
import compare
import render


def report():
    result = complete_report()
    result.update(harness_revision="a" * 40, compare_sha256="b" * 64, corpus={"layers": {"C": {"documents": 1}}},
                  contracts={"v4": "c" * 64}, heldout_split={"layers": {"C": ["validation", "test"]}})
    del result["tools"]["opf"]
    del result["contention_samples"]["opf"]
    receipt = dict(schema_version=1, tool="opf", status="unavailable", reason=REASON,
                   measurement_revision=result["harness_revision"], comparison_adapter_sha256=result["compare_sha256"],
                   corpus_sha256=digest(result["corpus"]), contracts_sha256=digest(result["contracts"]),
                   checkpoint_sha256="d" * 64, runtime_source_revision="e" * 40)
    configuration = digest(receipt)
    receipt["attempts"] = [dict(attempt=n, timeout_seconds=600, outcome="socket_timeout",
                                log_sha256=str(n) * 64, configuration_sha256=configuration) for n in (1, 2)]
    result["unavailable"] = {"opf": receipt}
    return result


class AvailabilityTest(unittest.TestCase):
    def test_two_unchanged_attempts_can_finalize_without_fake_samples(self):
        result = finalize(report())
        self.assertEqual(unavailable_tools(result), {"opf"})
        self.assertNotIn("opf", result["tools"])
        self.assertNotIn("opf", result["contention_samples"])

    def test_missing_corrupt_or_numeric_receipt_fails_closed(self):
        for mutation in (
            lambda r: r["unavailable"]["opf"].update(leaked_bytes=0),
            lambda r: r["unavailable"]["opf"].update(corpus_sha256="0" * 64),
            lambda r: r["unavailable"]["opf"]["attempts"].pop(),
            lambda r: r["unavailable"]["opf"]["attempts"][1].update(configuration_sha256="0" * 64),
            lambda r: r["unavailable"].update(gliner=r["unavailable"]["opf"]),
            lambda r: r["tools"].update(opf={"leaked_bytes": 0}),
            lambda r: r["tools"].pop("gliner"),
        ):
            with self.subTest(mutation=mutation):
                bad = copy.deepcopy(report())
                mutation(bad)
                with self.assertRaises(ValueError):
                    finalize(bad)

    def test_chart_keeps_opf_na_and_no_zero_scores(self):
        panel = charts.own_panel([], report(), {"opf": "opf"}, "synthetic corpus")
        self.assertEqual(len(panel.bars), 1)
        self.assertEqual(panel.bars[0].name, "OPF")
        self.assertIsNone(panel.bars[0].f2)
        self.assertIsNone(panel.bars[0].leaked)
        self.assertIsNone(panel.bars[0].fp_per_1k)
        self.assertIn("N/A", panel.note)

    def test_table_preserves_na_and_names_skipped_language_count(self):
        row = render.unavailable_summary_row("v4", "C", "opf", False)
        self.assertIn("| opf | N/A | N/A | N/A |", row)
        self.assertNotIn("| 0 |", row)
        tools = {"scrubadub-base": {"contracts": {"v4": {"A": {"skipped_languages": {"es": 7}}}}}}
        note = render.skipped_language_footnote(tools)
        self.assertIn("scrubadub-base/", note)
        self.assertIn("es=7", note)
        self.assertIn("fully missed", note)

    def test_scrubadub_unsupported_spanish_is_fully_missed(self):
        text = "alice@example.invalid"
        docs = [compare.score.Document("synthetic", text, "es", "ES", "synthetic",
                (compare.score.Span(0, len(text), "EMAIL"),))]
        def must_not_run(_document):
            raise AssertionError("unsupported input reached backend")
        result = compare.measure("scrubadub-base", must_not_run, {"C": docs},
                                 compare.load_mapping()["scrubadub"], ("en", "de", "nl", "fr", "pt"))
        for contract in result["contracts"].values():
            row = contract["C"]
            self.assertEqual(row["leaked_bytes"], len(text))
            self.assertEqual(row["skipped_gold_bytes"], len(text))
            self.assertEqual(row["skipped_languages"], {"es": 1})
            self.assertEqual(row["latency"]["samples"], 0)
            self.assertEqual(row["metrics"]["product_coverage"]["full"]["leaked_bytes"], len(text))


if __name__ == "__main__":
    unittest.main()
