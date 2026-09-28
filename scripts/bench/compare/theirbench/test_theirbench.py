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
    quiet = {"cpu": {"contended": False}}
    return {"not_run": {"x": "licence"}, "benchmarks": {"presidio-research": {
        "rows": rows, "chart_rows": ["gaze-full", "presidio-strong", "opf"],
        "provenance": {tool: quiet for tool in rows},
        "common_intersection_labels": ["EMAIL_ADDRESS"], "hardware": "hw",
        "harness_revision": "0123456789", "own_metric": {tool: {"f2": 0.5} for tool in rows},
        "reproduction": {"published": {"vanilla": {"f2": 0.661, "source": "nb4"}},
                         "reproduced": {"vanilla": {"f2": 0.66}}}}}}


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
                                          **{k: None for k in ("identity", "harness_revision", "gaze_crates_tree",
                                                               "label_maps_sha256", "mapping_sha256", "hardware",
                                                               "common_intersection_labels", "splits", "provenance")}}),
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

if __name__ == "__main__":
    unittest.main()
