"""Model-free tests for the competitors'-own-benchmark tooling."""

from __future__ import annotations

import json
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import loaders  # noqa: E402

MAPS = {
    "extension_labels": ["IPADDRESS", "DATE"],
    "gold": {"bench": {"PERSON": ["FIRSTNAME", "SURNAME"], "IP": ["IPADDRESS"],
                       "DATE_TIME": ["DATE", "DATEOFBIRTH"], "ADDR": ["STREET", "ZIP"]}},
    "tool_extension": {"tool": {"ip": ["IPADDRESS"]}},
}


class ComposeMappingTest(unittest.TestCase):
    def setUp(self) -> None:
        self._main = loaders._main_corpus_labels
        loaders._main_corpus_labels = lambda: {"FIRSTNAME", "SURNAME", "DATEOFBIRTH", "STREET", "ZIP"}

    def tearDown(self) -> None:
        loaders._main_corpus_labels = self._main

    def test_overlap_and_extension(self) -> None:
        tool = {"name": ["FIRSTNAME"], "ip": [], "dob": ["DATEOFBIRTH"], "zip": ["ZIP"], "secret": []}
        composed = loaders.compose_mapping("tool", tool, "bench", MAPS)
        self.assertEqual(composed, {"name": ("PERSON",), "ip": ("IP",), "dob": ("DATE_TIME",),
                                    "zip": ("ADDR",), "secret": ()})

    def test_without_extension_ip_is_uncovered(self) -> None:
        maps = {**MAPS, "tool_extension": {}}
        self.assertEqual(loaders.compose_mapping("tool", {"ip": []}, "bench", maps), {"ip": ()})

    def test_extension_for_unknown_tool_label_fails(self) -> None:
        with self.assertRaisesRegex(ValueError, "never emits"):
            loaders.compose_mapping("tool", {"name": ["FIRSTNAME"]}, "bench", MAPS)

    def test_unknown_gold_target_fails(self) -> None:
        maps = json.loads(json.dumps(MAPS))
        maps["gold"]["bench"]["X"] = ["NOT_A_LABEL"]
        with self.assertRaisesRegex(ValueError, "unknown canonical"):
            loaders.compose_mapping("tool", {"ip": []}, "bench", maps)

    def test_committed_maps_compose_for_every_tool(self) -> None:
        maps = loaders.load_label_maps()
        raw = json.loads((loaders.BENCH / "compare/label-map.json").read_text(encoding="utf-8"))
        loaders._main_corpus_labels = self._main
        for benchmark in maps["gold"]:
            for tool, table in raw.items():
                composed = loaders.compose_mapping(tool, table, benchmark, maps)
                self.assertEqual(set(composed), set(table))


class BioSpansTest(unittest.TestCase):
    def test_chunks(self) -> None:
        spans = loaders.bio_spans(["Hi", "John", "Smith", "in", "New", "York"],
                                  ["O", "B-PERSON", "I-PERSON", "O", "B-LOC", "I-LOC"])
        self.assertEqual(spans, [(3, 13, "PERSON"), (17, 25, "LOC")])

    def test_orphan_and_type_switch_start_new_chunks(self) -> None:
        self.assertEqual(loaders.bio_spans(["a", "b", "c"], ["I-X", "I-Y", "I-Y"]),
                         [(0, 1, "X"), (2, 5, "Y")])

    def test_adjacent_b_tags_split(self) -> None:
        self.assertEqual(loaders.bio_spans(["a", "b"], ["B-X", "B-X"]), [(0, 1, "X"), (2, 3, "X")])

    def test_rejects_other_schemes(self) -> None:
        with self.assertRaises(ValueError):
            loaders.bio_spans(["a"], ["S-X"])


class PiibenchLoaderTest(unittest.TestCase):
    def test_hash_pin_and_byte_offsets(self) -> None:
        with tempfile.TemporaryDirectory() as root:
            data = Path(root)
            (data / "data").mkdir()
            test = data / "data/test_5k.jsonl"
            test.write_text(json.dumps({"tokens": ["Grüße", "Anna"], "labels": ["O", "B-PERSON"],
                                        "source": "few_nerd"}) + "\n", encoding="utf-8")
            manifest = {"piibench": {}, "sources": [], "excluded": {}, "published_full_mix": {},
                        "files": {"test_5k.jsonl": loaders.sha256(test)}}
            (data / "manifest.json").write_text(json.dumps(manifest), encoding="utf-8")
            splits, _ = loaders.load_piibench_commercial(data)
            document = splits["test"][0]
            span = document.spans[0]
            self.assertEqual(document.text.encode()[span.start:span.end], b"Anna")
            test.write_text(test.read_text(encoding="utf-8") + "\n", encoding="utf-8")
            with self.assertRaisesRegex(ValueError, "differs"):
                loaders.load_piibench_commercial(data)


def cell(leaked: int, fp: int = 10) -> dict:
    return {"leaked_bytes": leaked, "false_positive_bytes": fp, "true_positive_bytes": 100 - leaked,
            "document_leak_rate": 0.5, "typed_entities": {"f1": 0.4, "f2": 0.5}}


def row(leaked: int) -> dict:
    return {"product_coverage": cell(leaked), "common_intersection": cell(leaked // 2),
            "latency": {"p50_ms": 1.0}}


def synthetic() -> dict:
    rows = {"gaze-full": row(10), "gaze-rules-only": row(40), "presidio-en": row(5),
            "presidio-strong": row(30), "opf": row(20)}
    quiet = {"cpu": {"contended": False, "valid": True}}
    return {"not_run": {"x": "licence"}, "benchmarks": {"presidio-research": {
        "rows": rows, "chart_rows": ["gaze-full", "presidio-strong", "opf"], "typed_hold": ["opf"],
        "comparison_revision": "154f3da6",
        "rescored_with": {"comparison_revision": "b1446215", "harness_revision": "abcdef0123", "harness_dirty": False},
        "provenance": {tool: quiet for tool in rows},
        "common_intersection_labels": ["EMAIL_ADDRESS"], "hardware": "hw",
        "harness_revision": "0123456789", "own_metric": {tool: {"f2": 0.5} for tool in rows},
        "reproduction": {"published": {"vanilla": {"f2": 0.661, "source": "nb4"}},
                         "reproduced": {"vanilla": {"f2": 0.59}},
                         "historical": {"vanilla": {"f2": 0.664, "evaluator_commit": "ac490f9b9611"}}}}}}


class RenderTest(unittest.TestCase):
    def test_chart_uses_declared_rows_not_lowest_leak(self) -> None:
        import render_theirbench as render

        entry = synthetic()["benchmarks"]["presidio-research"]
        # presidio-en leaks least but is not declared; it must not be charted.
        self.assertEqual(render.chart_rows(entry), ["gaze-full", "presidio-strong", "opf"])
        body = render.render(synthetic())
        chart = body[body.index("x-axis"):body.index("bar [")]
        self.assertNotIn("presidio-en", chart)

    def test_declared_row_must_be_measured(self) -> None:
        import render_theirbench as render

        entry = synthetic()["benchmarks"]["presidio-research"]
        entry["chart_rows"].append("scrubadub-spacy")
        with self.assertRaisesRegex(ValueError, "not measured"):
            render.chart_rows(entry)

    def test_contended_or_unrecorded_latency_is_not_published(self) -> None:
        import render_theirbench as render

        entry = synthetic()["benchmarks"]["presidio-research"]
        self.assertEqual(render.latency_cell(entry, "opf"), "1.0")
        entry["provenance"]["opf"] = {"cpu": {"contended": True}}
        self.assertEqual(render.latency_cell(entry, "opf"), render.QUIET)
        entry["provenance"]["opf"] = {"cpu": {"contended": False, "valid": False}}
        self.assertEqual(render.latency_cell(entry, "opf"), render.QUIET)
        del entry["provenance"]["gaze-full"]
        self.assertEqual(render.latency_cell(entry, "gaze-full"), render.QUIET)

    def test_check_detects_drift(self) -> None:
        import render_theirbench as render

        with tempfile.TemporaryDirectory() as root:
            data, doc = Path(root) / "data.json", Path(root) / "README.md"
            data.write_text(json.dumps(synthetic()), encoding="utf-8")
            doc.write_text("x\n<!-- BEGIN GENERATED: their-benchmarks -->\n<!-- END GENERATED: their-benchmarks -->\n",
                           encoding="utf-8")
            args = ["render", "--data", str(data), "--doc", str(doc)]
            self.assertEqual(render.main([*args, "--check"]), 1)
            self.assertEqual(render.main(args), 0)
            self.assertEqual(render.main([*args, "--check"]), 0)
            self.assertIn("presidio-strong", doc.read_text(encoding="utf-8"))
            self.assertIn("reproduced 0.664 with the evaluator at `ac490f9b`", doc.read_text(encoding="utf-8"))
            changed = synthetic()
            changed["benchmarks"]["presidio-research"]["rows"]["opf"] = row(25)
            self.assertIn("presidio-strong", doc.read_text(encoding="utf-8"))
            data.write_text(json.dumps(changed), encoding="utf-8")
            self.assertEqual(render.main([*args, "--check"]), 1)

    def test_smoke_results_are_refused(self) -> None:
        import render_theirbench as render

        with tempfile.TemporaryDirectory() as root:
            smoke = Path(root) / "smoke.json"
            smoke.write_text(json.dumps({"smoke_limit": 30, "system": "x", "scored": {}}), encoding="utf-8")
            report = Path(root) / "report.json"
            report.write_text(json.dumps({"harness_dirty": False, "benchmark": "presidio-research", "rows": {},
                                          "chart_configs": {}, "rescored_with": {"harness_dirty": False},
                                          **{k: None for k in ("identity", "harness_revision", "gaze_crates_tree",
                                                               "label_maps_sha256", "mapping_sha256", "hardware",
                                                               "common_intersection_labels", "splits", "provenance",
                                                               "comparison_revision", "comparison_sha256",
                                                               "typed_hold")}}),
                              encoding="utf-8")
            with self.assertRaisesRegex(ValueError, "smoke"):
                render.assemble([report], [f"presidio-research={smoke}"], [])


class ForeignCpuTest(unittest.TestCase):
    def test_own_tree_is_excluded_and_one_core_marks_contended(self) -> None:
        import cpu_watch

        rows = [(1, 0, 90.0, "launchd"), (100, 1, 570.0, "python"), (101, 100, 300.0, "opf"),
                (200, 1, 60.0, "cargo"), (201, 200, 50.0, "rustc"), (300, 1, 3.0, "idle")]
        total, top = cpu_watch.foreign_cpu(rows, root=100)
        self.assertEqual(total, 200.0)  # launchd + cargo + rustc; own tree and <5 % noise excluded
        self.assertEqual(top[0], ["launchd", 90.0])
        watch = cpu_watch.ForeignCpuWatch(root=100)
        for peak, contended in ((100.0, False), (100.1, True)):
            watch.samples = [{"foreign_cpu_percent": 10.0}, {"foreign_cpu_percent": peak}]
            self.assertEqual(watch.result()["contended"], contended)


class PinnedComparisonTest(unittest.TestCase):
    def test_comparison_files_match_the_pinned_revision(self) -> None:
        sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
        import backends

        self.assertEqual(backends.verify_pinned_comparison(), backends.PINNED_SHA256)
        original = backends.PINNED_SHA256["compare.py"]
        try:
            backends.PINNED_SHA256["compare.py"] = "0" * 64
            with self.assertRaises(SystemExit):
                backends.verify_pinned_comparison()
        finally:
            backends.PINNED_SHA256["compare.py"] = original

    def test_chart_configs_are_the_comparison_declared_rows(self) -> None:
        import backends

        configs = backends.chart_configs()
        self.assertEqual(configs["presidio"], "presidio-strong")
        self.assertTrue(set(configs.values()) <= set(backends.compare.TOOLS))


class HoldAndRescoreTest(unittest.TestCase):
    def test_held_typed_cells_and_no_bare_layer_codes(self) -> None:
        import render_theirbench as render

        body = render.render(synthetic())
        opf = next(line for line in body.splitlines() if line.startswith("| opf |"))
        gaze = next(line for line in body.splitlines() if line.startswith("| gaze-full |"))
        self.assertEqual(opf.count(render.HELD), 2)
        self.assertNotIn(render.HELD, gaze)
        self.assertIn("0.500", opf)  # Presidio Research F2 is type-agnostic, never held
        import re
        self.assertIsNone(re.search(r"(?<![A-Za-z0-9_-])[CADR](?![A-Za-z0-9_-])", body.replace("| ", "")))

    def test_hold_rule_covers_families_and_secrets_only(self) -> None:
        sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
        import theirbench

        maps = {"gaze": {"custom:family:x": [], "email": []}, "opf": {"secret": []},
                "scrubadub": {"credential": []}, "presidio": {"PERSON": []}, "gliner": {"passport number": []}}
        self.assertEqual(theirbench.typed_hold(maps), ["gaze", "opf", "scrubadub"])

    def test_rescore_refuses_moved_bytes(self) -> None:
        import theirbench
        from loaders import score

        document = score.Document("d0", "Mail anna@example.invalid now", "en", "", "t",
                                  (score.Span(5, 25, "EMAIL"),))
        mapping = {"email": ("EMAIL",)}
        with tempfile.TemporaryDirectory() as root:
            pred = Path(root)
            (pred / "x.test.jsonl").write_text(json.dumps({"index": 0, "spans": [[5, 25, "email"]]}) + "\n",
                                               encoding="utf-8")
            cells = theirbench.Cells(mapping, frozenset({"EMAIL"}))
            cells.add(document, [score.Span(5, 25, "email")])
            report = {"rows": {"x": {"test": cells.result()}}}
            original = theirbench.tool_family
            theirbench.tool_family = lambda name: "fam"
            try:
                theirbench.rescore(report, {"test": [document]}, {"fam": mapping}, frozenset({"EMAIL"}), pred)
                report["rows"]["x"]["test"]["product_coverage"]["leaked_bytes"] = 7
                with self.assertRaisesRegex(SystemExit, "leaked_bytes"):
                    theirbench.rescore(report, {"test": [document]}, {"fam": mapping}, frozenset({"EMAIL"}), pred)
            finally:
                theirbench.tool_family = original

    def test_measured_backend_code_is_unchanged(self) -> None:
        import backends

        self.assertEqual(backends.backend_code_sha256(), backends.BACKEND_CODE_SHA256)


class GuardTest(unittest.TestCase):
    def test_piibench_typed_seqeval_is_held_for_held_tools(self) -> None:
        import render_theirbench as render

        entry = synthetic()["benchmarks"]["presidio-research"]
        self.assertEqual(render.own_metric_cell("piibench-commercial", entry, "opf", "f2"), render.HELD)
        self.assertEqual(render.own_metric_cell("piibench-commercial", entry, "gaze-full", "f2"), "0.500")
        self.assertEqual(render.own_metric_cell("presidio-research", entry, "opf", "f2"), "0.500")

    def test_backend_code_drift_refuses_to_run(self) -> None:
        import backends

        original = backends.BACKEND_CODE_SHA256
        try:
            backends.BACKEND_CODE_SHA256 = "0" * 64
            with self.assertRaisesRegex(SystemExit, "backend construction"):
                backends.verify_pinned_comparison()
        finally:
            backends.BACKEND_CODE_SHA256 = original

    def test_unrescored_or_dirty_rescore_is_never_published(self) -> None:
        import render_theirbench as render

        base = {"harness_dirty": False, "benchmark": "presidio-research", "rows": {}, "chart_configs": {}}
        with tempfile.TemporaryDirectory() as root:
            for extra in ({}, {"rescored_with": {"harness_dirty": True}}):
                report = Path(root) / "report.json"
                report.write_text(json.dumps({**base, **extra}), encoding="utf-8")
                with self.assertRaisesRegex(ValueError, "rescore"):
                    render.assemble([report], [], [])

if __name__ == "__main__":
    unittest.main()
