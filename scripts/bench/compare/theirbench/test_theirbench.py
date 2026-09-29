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


def release_provenance() -> dict:
    """A complete tagged-row provenance block, as theirbench.py --gaze-release-tag writes it."""
    from tagged_gaze import RELEASE_PINS

    return {
        "tag": "v0.15.1", "commit": "e" * 40, "crates_tree": "c" * 40,
        **RELEASE_PINS["v0.15.1"], "policy_sha256": "f" * 64,
        "build": {"command": "cargo build --locked -q -p gaze-recognizers --example clean_for_bench "
                             "--features safety-net-nym", "profile": "debug", "rustc": "r", "cargo": "c",
                  "cargo_lock_sha256": "1" * 64, "rust_toolchain_sha256": "2" * 64, "binary_sha256": "b" * 64},
        "prediction_sha256": "9" * 64,
        "measured_with": {"harness_revision": "d" * 40, "harness_dirty": False},
        "reproduces": {"prediction_sha256": "9" * 64, "binary_sha256": "a" * 64,
                       "harness_revision": "d" * 40, "harness_dirty": False},
    }


def own_result(system: str, **extra) -> dict:
    """A vendor-evaluator result whose published score follows from its counts:
    60 of 80 predicted, 100 annotated -> precision 0.75, recall 0.6, F2 0.625 (rounded to 3)."""
    return {"system": system, **extra,
            "scored": {"documents": 1500, "precision": 0.75, "recall": 0.6, "f2": 0.625},
            "input": {"prediction_sha256": "", "dataset_sha256": "d" * 64, "documents": 1500, "beta": 2,
                      "digits": 3, "counts": {"true_positives": 60, "predicted": 80, "annotated": 100}}}


def synthetic() -> dict:
    rows = {"gaze-full": row(10), "gaze-rules-only": row(40), "gaze-v0.15.1": row(12), "presidio-en": row(5),
            "presidio-strong": row(30), "opf": row(20)}
    quiet = {"cpu": {"contended": False, "valid": True}}
    return {"not_run": {"x": "licence"}, "benchmarks": {"presidio-research": {
        "rows": rows, "chart_rows": ["gaze-full", "presidio-strong", "opf"], "typed_hold": ["opf"],
        "comparison_revision": "154f3da6", "gaze_crates_tree": "97e45cfe07d1",
        "rescored_with": {"comparison_revision": "b1446215", "harness_revision": "abcdef0123", "harness_dirty": False},
        "provenance": {tool: (quiet if tool != "gaze-v0.15.1" else {**quiet, "release": release_provenance()})
                       for tool in rows},
        "tagged_measurements": {"gaze-v0.15.1": {"harness_revision": "d" * 40}},
        "common_intersection_labels": ["EMAIL_ADDRESS"], "hardware": "hw",
        "harness_revision": "0123456789", "own_metric": {tool: {"f2": 0.5} for tool in rows},
        "reproduction": {"published": {"vanilla": {"f2": 0.661, "source": "nb4"}},
                         "reproduced": {"vanilla": {"f2": 0.59}},
                         "historical": {"vanilla": {"f2": 0.664, "evaluator_commit": "ac490f9b9611"}}}}}}


class RenderTest(unittest.TestCase):
    def test_declared_rows_are_validated_and_no_chart_is_drawn(self) -> None:
        import render_theirbench as render

        entry = synthetic()["benchmarks"]["presidio-research"]
        # presidio-en leaks least but is not declared; declared order is kept.
        self.assertEqual(render.chart_rows(entry), ["gaze-full", "presidio-strong", "opf"])
        self.assertNotIn("mermaid", render.render(synthetic()))

    def test_untagged_gaze_rows_are_never_published(self) -> None:
        import render_theirbench as render
        from tagged_gaze import UntaggedGazeError, check_public

        body = render.render(synthetic())
        self.assertNotIn("gaze-full", body)
        self.assertNotIn("gaze-rules-only", body)
        self.assertNotIn("Gaze ran on crates tree", body)
        self.assertIn("| gaze-v0.15.1 |", body)
        self.assertLess(body.index("| gaze-v0.15.1 |"), body.index("| opf |"))
        # Changing the untagged rows cannot change the published block.
        moved = synthetic()
        moved["benchmarks"]["presidio-research"]["rows"]["gaze-full"] = row(1)
        self.assertEqual(render.render(moved), body)
        # A Gaze row that is neither main-tree evidence nor a release tag is refused.
        odd = synthetic()
        odd["benchmarks"]["presidio-research"]["rows"]["gaze-candidate"] = row(3)
        with self.assertRaisesRegex(ValueError, "gaze-vX.Y.Z"):
            render.render(odd)
        with self.assertRaises(UntaggedGazeError):
            check_public("| gaze-full | 10 |", "block")

    def test_without_a_tagged_row_the_page_says_not_yet_measured(self) -> None:
        import render_theirbench as render

        data = synthetic()
        del data["benchmarks"]["presidio-research"]["rows"]["gaze-v0.15.1"]
        self.assertRegex(render.render(data), r"Gaze v\d+\.\d+\.\d+: not yet measured on this set")

    def test_declared_row_must_be_measured(self) -> None:
        import render_theirbench as render

        entry = synthetic()["benchmarks"]["presidio-research"]
        entry["chart_rows"].append("scrubadub-spacy")
        with self.assertRaisesRegex(ValueError, "not measured"):
            render.chart_rows(entry)

    def test_no_latency_is_published(self) -> None:
        import render_theirbench as render

        body = render.render(synthetic())
        self.assertNotIn("p50", body)
        self.assertNotIn(" ms", body)

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
        gaze = next(line for line in body.splitlines() if line.startswith("| gaze-v0.15.1 |"))
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

    def test_rescore_adds_char_level_to_a_report_measured_without_it(self) -> None:
        import theirbench
        from loaders import score

        # "Müller" is 7 bytes / 6 chars; stored predictions are in character offsets.
        document = score.Document("d0", "Hi Müller", "en", "", "t", (score.Span(3, 10, "PERSON"),))
        mapping = {"name": ("PERSON",)}
        with tempfile.TemporaryDirectory() as root:
            pred = Path(root)
            (pred / "x.test.jsonl").write_text(json.dumps({"index": 0, "spans": [[3, 9, "name"]]}) + "\n",
                                               encoding="utf-8")
            cells = theirbench.Cells(mapping, frozenset({"PERSON"}))
            cells.add(document, [score.Span(3, 10, "name")])
            result = cells.result()
            for view in result.values():
                del view["char_level"]
            report = {"rows": {"x": {"test": result}}}
            original = theirbench.tool_family
            theirbench.tool_family = lambda name: "fam"
            try:
                theirbench.rescore(report, {"test": [document]}, {"fam": mapping}, frozenset({"PERSON"}), pred)
            finally:
                theirbench.tool_family = original
            char = report["rows"]["x"]["test"]["product_coverage"]["char_level"]
            self.assertEqual((char["tp"], char["fp"], char["fn"]), (6, 0, 0))
            self.assertEqual(char["f2"], 1.0)

    def test_measured_backend_code_is_unchanged(self) -> None:
        import backends

        self.assertEqual(backends.backend_code_sha256(), backends.BACKEND_CODE_SHA256)


class GuardTest(unittest.TestCase):
    def test_piibench_typed_seqeval_is_held_for_held_tools(self) -> None:
        import render_theirbench as render

        entry = synthetic()["benchmarks"]["presidio-research"]
        own_pb, own_pr = render.metrics("piibench-commercial")[-1], render.metrics("presidio-research")[-1]
        entry["own_metric"] = {tool: {"f1": 0.5, "f2": 0.5} for tool in entry["rows"]}
        self.assertEqual(render.cell(own_pb, "piibench-commercial", entry, "opf"), render.HELD)
        self.assertEqual(render.cell(own_pb, "piibench-commercial", entry, "gaze-full"), "0.500")
        self.assertEqual(render.cell(own_pr, "presidio-research", entry, "opf"), "0.500")

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


RESOLVE = lambda tag: ("e" * 40, "c" * 40)  # noqa: E731  (commit, crates tree) of the tag


class TaggedRowTest(unittest.TestCase):
    """A tagged Gaze release joins the aggregate only from a clean checkout of its tag."""

    def entry_and_report(self):
        data = synthetic()
        entry = data["benchmarks"]["presidio-research"]
        entry.update(identity={"documents": 1500, "sha256": "d" * 64}, splits={"test": {"documents": 1500}},
                     label_maps_sha256="a" * 64, mapping_sha256="b" * 64, typed_hold=["gaze", "opf"])
        entry["rescored_with"]["comparison_sha256"] = {"compare.py": "c" * 64}
        del entry["rows"]["gaze-v0.15.1"]
        report = {
            "schema_version": 1, "benchmark": "presidio-research", "preflight": None,
            "harness_dirty": False, "harness_revision": "d" * 40, "hardware": "hw",
            "generated_at": "2026-09-29T00:00:00+00:00",
            "identity": entry["identity"], "splits": entry["splits"],
            "common_intersection_labels": entry["common_intersection_labels"],
            "label_maps_sha256": entry["label_maps_sha256"], "mapping_sha256": entry["mapping_sha256"],
            "typed_hold": entry["typed_hold"], "comparison_sha256": {"compare.py": "c" * 64},
            "rows": {"gaze-v0.15.1": {"test": row(12)}},
            "provenance": {"gaze-v0.15.1": {"release": release_provenance()}},
        }
        own = own_result("gaze-v0.15.1")
        own["input"]["prediction_sha256"] = "9" * 64
        return data, entry, report, own

    def test_a_valid_report_adds_one_row_the_page_shows_first(self) -> None:
        import render_theirbench as render

        data, entry, report, own = self.entry_and_report()
        self.assertEqual(render.add_tagged(data, report, own, RESOLVE), "gaze-v0.15.1")
        self.assertEqual(entry["own_metric"]["gaze-v0.15.1"], own["scored"])
        self.assertEqual(entry["tagged_measurements"]["gaze-v0.15.1"]["harness_revision"], "d" * 40)
        body = render.render(data)
        self.assertLess(body.index("| gaze-v0.15.1 |"), body.index("| opf |"))
        self.assertNotIn("not yet measured", body)
        self.assertIn("Row gaze-v0.15.1: a clean checkout of tag `v0.15.1` (crates tree `cccccccc`", body)
        self.assertIn("harness `dddddddd`", body)

    def test_each_mismatch_refuses_the_row(self) -> None:
        import render_theirbench as render

        def refused(mutate, match: str) -> None:
            data, _entry, report, own = self.entry_and_report()
            mutate(report, own)
            with self.assertRaisesRegex(ValueError, match):
                render.add_tagged(data, report, own, RESOLVE)

        refused(lambda r, o: r.update(harness_dirty=True), "clean, full")
        refused(lambda r, o: r.update(preflight=5), "clean, full")
        refused(lambda r, o: r.update(identity={"documents": 1}), "identity differs")
        refused(lambda r, o: r.update(label_maps_sha256="z"), "label_maps_sha256 differs")
        refused(lambda r, o: r.update(comparison_sha256={"compare.py": "0"}), "different pinned comparison")
        refused(lambda r, o: r["rows"].update({"presidio-en": {"test": row(5)}}), "exactly one")
        refused(lambda r, o: r["provenance"]["gaze-v0.15.1"].pop("release"), "does not name the release")
        # Tagged-row identity: every link from the numbers to the tag must hold.
        release = lambda r: r["provenance"]["gaze-v0.15.1"]["release"]  # noqa: E731
        refused(lambda r, o: release(r).update(crates_tree="0" * 40), "the tag is")
        refused(lambda r, o: release(r).update(commit="0" * 40), "the tag is")
        refused(lambda r, o: release(r).update(policy_home_normalized_sha256="0" * 64), "pinned ones")
        refused(lambda r, o: release(r).update(ner_model_tree_sha256="0" * 64), "pinned ones")
        refused(lambda r, o: release(r).update(nym_model_tree_sha256="0" * 64), "pinned ones")
        refused(lambda r, o: release(r).pop("build"), "no build record")
        refused(lambda r, o: release(r)["build"].update(command="cargo build"), "no build record")
        refused(lambda r, o: release(r)["measured_with"].update(harness_dirty=True), "dirty or unrecorded")
        refused(lambda r, o: release(r).pop("reproduces"), "no clean earlier run")
        refused(lambda r, o: release(r)["reproduces"].update(prediction_sha256="0" * 64), "no clean earlier run")
        refused(lambda r, o: release(r)["reproduces"].update(harness_dirty=True), "no clean earlier run")
        refused(lambda r, o: o.update(system="gaze-full"), "own-scorer result is for")
        # The vendor evaluator's score must be of THESE predictions on the pinned dataset.
        refused(lambda r, o: o["input"].update(prediction_sha256="0" * 64), "own scorer read predictions")
        refused(lambda r, o: o["input"].update(dataset_sha256="0" * 64), "own scorer used dataset")
        refused(lambda r, o: o.pop("input"), "no input receipt")
        # The published score must be what the receipt's counts give (recomputed at merge).
        refused(lambda r, o: o["scored"].update(f2=0.01), "published f2 0.01 is not what its counts give")
        refused(lambda r, o: o["scored"].update(precision=0.99), "published precision")
        refused(lambda r, o: o["scored"].update(recall=0.1), "published recall")
        refused(lambda r, o: o["input"]["counts"].update(true_positives=61), "published precision")
        refused(lambda r, o: o["input"]["counts"].update(true_positives=90, predicted=80), "counts are inconsistent")
        refused(lambda r, o: o["input"].pop("counts"), "no counts")
        refused(lambda r, o: o["input"].update(documents=1499), "scored 1499 documents")
        refused(lambda r, o: o["scored"].update(documents=1499), "published documents count")
        refused(lambda r, o: (r["rows"].update({"gaze-main": r["rows"].pop("gaze-v0.15.1")}),
                              r["provenance"].update({"gaze-main": {}})), "exactly one")

    def test_a_row_already_present_is_refused(self) -> None:
        import render_theirbench as render

        data, entry, report, own = self.entry_and_report()
        entry["rows"]["gaze-v0.15.1"] = row(1)
        with self.assertRaisesRegex(ValueError, "already in"):
            render.add_tagged(data, report, own, RESOLVE)

    def _temp_repo(self, tmp: str):
        import subprocess

        root = Path(tmp)

        def git(*args: str) -> str:
            return subprocess.run(["git", "-c", "user.email=t@t", "-c", "user.name=t", "-c",
                                   "commit.gpgsign=false", "-c", "tag.gpgsign=false", *args],
                                  cwd=root, check=True, capture_output=True, text=True).stdout.strip()

        git("init", "-q", "-b", "main")
        (root / "crates").mkdir()
        (root / "crates" / "f").write_text("x")
        (root / ".gitignore").write_text("target\n")
        git("add", ".gitignore", "crates")
        git("commit", "-q", "-m", "c")
        return root, git

    def test_release_checkout_must_be_the_clean_tag(self) -> None:
        import theirbench
        from tagged_gaze import UntaggedGazeError

        with tempfile.TemporaryDirectory() as tmp:
            root, git = self._temp_repo(tmp)
            git("branch", "v1.2.3")  # a branch of that name is not a release
            original = theirbench.compare.REPO
            theirbench.compare.REPO = root
            try:
                with self.assertRaises(UntaggedGazeError):
                    theirbench.verify_release_checkout("v1.2.3", root)
                git("tag", "-a", "-m", "release", "v1.2.4")
                got = theirbench.verify_release_checkout("v1.2.4", root)
                self.assertEqual(got["tag"], "v1.2.4")
                self.assertEqual(got["crates_tree"], git("rev-parse", "HEAD:crates"))
                (root / "crates" / "f").write_text("changed")
                with self.assertRaises(SystemExit):  # local changes
                    theirbench.verify_release_checkout("v1.2.4", root)
                git("checkout", "-q", "--", "crates")
                (root / "later").write_text("y")
                git("add", "later")
                git("commit", "-q", "-m", "later")
                with self.assertRaises(SystemExit):  # HEAD is no longer the tag's commit
                    theirbench.verify_release_checkout("v1.2.4", root)
            finally:
                theirbench.compare.REPO = original

    def test_a_stale_binary_can_never_be_attributed_to_the_tag(self) -> None:
        """The harness builds the binary itself in a fresh directory; anything already there is refused."""
        import argparse
        from unittest import mock

        import theirbench

        with tempfile.TemporaryDirectory() as tmp:
            root, git = self._temp_repo(tmp)
            (root / "Cargo.lock").write_text("lock")
            (root / "rust-toolchain.toml").write_text("tc")
            stale = Path(tmp).parent / "another-checkout-target"
            (stale / "debug" / "examples").mkdir(parents=True, exist_ok=True)
            (stale / "debug" / "examples" / "clean_for_bench").write_bytes(b"stale")
            with self.assertRaisesRegex(SystemExit, "already exists"):
                theirbench.build_tagged_binary("v1.2.4", root, stale)
            # A build that produces no binary is refused too (nothing is borrowed from elsewhere).
            fresh = Path(tmp).parent / "fresh-target"
            with mock.patch.object(theirbench.subprocess, "run", return_value=None):
                with self.assertRaisesRegex(SystemExit, "produced no"):
                    theirbench.build_tagged_binary("v1.2.4", root, fresh)
            # --gaze-binary is refused outright in tagged mode.
            args = argparse.Namespace(tool=None, gaze_binary=Path("x"), gaze_release_root=root,
                                      gaze_policy=Path("p"), output=Path(tmp) / "new.json")
            with self.assertRaisesRegex(SystemExit, "builds its own binary"):
                theirbench.validate_tagged_args(args)

    def test_a_tagged_row_is_measured_fresh_never_resumed(self) -> None:
        import argparse

        import theirbench

        with tempfile.TemporaryDirectory() as tmp:
            existing = Path(tmp) / "report.json"
            existing.write_text("{}")
            args = argparse.Namespace(tool=None, gaze_binary=None, gaze_release_root=Path(tmp),
                                      gaze_policy=Path("p"), output=existing)
            with self.assertRaisesRegex(SystemExit, "fresh report"):
                theirbench.validate_tagged_args(args)
        base = {"harness_revision": "a" * 40, "harness_dirty": False, "rows": {"opf": {}}}
        theirbench.check_resume(base, "a" * 40, False)  # same clean commit resumes
        for report, head, dirty, match in (
            (base, "b" * 40, False, "recorded clean commit"),
            (base, "a" * 40, True, "local changes"),
            ({**base, "harness_dirty": True}, "a" * 40, False, "recorded clean commit"),
            ({**base, "rows": {"gaze-v0.15.1": {}}}, "a" * 40, False, "never carried"),
        ):
            with self.assertRaisesRegex(SystemExit, match):
                theirbench.check_resume(report, head, dirty)

    def test_policy_and_model_digests_must_equal_the_pins(self) -> None:
        import theirbench
        from tagged_gaze import RELEASE_PINS

        with tempfile.TemporaryDirectory() as tmp:
            ner, nym = Path(tmp) / "ner", Path(tmp) / "nym"
            for directory in (ner, nym):
                directory.mkdir()
                (directory / "model.bin").write_bytes(directory.name.encode())
            policy = Path(tmp) / "policy.toml"
            policy.write_text(f'[safety_net.nym]\nmodel_dir = "{nym}"\n')
            pins = {"policy_home_normalized_sha256": theirbench.compare.normalized_policy_sha256(
                        policy, theirbench.hashlib.sha256(policy.read_bytes()).hexdigest()),
                    "ner_model_tree_sha256": theirbench.compare.digest_tree(ner),
                    "nym_model_tree_sha256": theirbench.compare.digest_tree(nym)}
            original = dict(RELEASE_PINS)
            RELEASE_PINS["v9.9.9"] = pins
            try:
                found = theirbench.verify_tagged_pins("v9.9.9", policy, ner)
                self.assertEqual({k: found[k] for k in pins}, pins)
                bad = Path(tmp) / "nym-tampered"  # digest_tree caches by path: use another directory
                bad.mkdir()
                (bad / "model.bin").write_bytes(b"nym")
                (bad / "extra.bin").write_bytes(b"tampered")
                tampered = Path(tmp) / "tampered.toml"
                tampered.write_text(f'[safety_net.nym]\nmodel_dir = "{bad}"\n')
                with self.assertRaisesRegex(SystemExit, "policy_home_normalized_sha256"):
                    theirbench.verify_tagged_pins("v9.9.9", tampered, ner)  # its policy bytes differ too
                pins["policy_home_normalized_sha256"] = theirbench.compare.normalized_policy_sha256(
                    tampered, theirbench.hashlib.sha256(tampered.read_bytes()).hexdigest())
                with self.assertRaisesRegex(SystemExit, "nym_model_tree_sha256"):
                    theirbench.verify_tagged_pins("v9.9.9", tampered, ner)
                pins["policy_home_normalized_sha256"] = theirbench.compare.normalized_policy_sha256(
                    policy, theirbench.hashlib.sha256(policy.read_bytes()).hexdigest())
                policy.write_text(f'[safety_net.nym]\nmodel_dir = "{nym}"\n# edited\n')
                with self.assertRaisesRegex(SystemExit, "policy_home_normalized_sha256"):
                    theirbench.verify_tagged_pins("v9.9.9", policy, ner)
                with self.assertRaisesRegex(SystemExit, "no pinned"):
                    theirbench.verify_tagged_pins("v8.8.8", policy, ner)
            finally:
                RELEASE_PINS.clear()
                RELEASE_PINS.update(original)

    def test_a_second_run_must_reproduce_the_predictions(self) -> None:
        import theirbench

        with tempfile.TemporaryDirectory() as tmp:
            first = Path(tmp) / "first.json"
            release = {"tag": "v0.15.1", "commit": "e" * 40, "crates_tree": "c" * 40,
                       "policy_home_normalized_sha256": "1", "ner_model_tree_sha256": "2",
                       "nym_model_tree_sha256": "3", "prediction_sha256": "9" * 64,
                       "build": {"binary_sha256": "b" * 64},
                       "measured_with": {"harness_revision": "d" * 40, "harness_dirty": False}}
            first.write_text(json.dumps({"provenance": {"gaze-v0.15.1": {"release": release}}}))
            got = theirbench.reproduction_record(first, "gaze-v0.15.1", dict(release))
            self.assertEqual(got["prediction_sha256"], "9" * 64)
            self.assertEqual(got["binary_sha256"], "b" * 64)
            with self.assertRaisesRegex(SystemExit, "prediction_sha256 differs"):
                theirbench.reproduction_record(first, "gaze-v0.15.1", {**release, "prediction_sha256": "0" * 64})
            with self.assertRaisesRegex(SystemExit, "crates_tree differs"):
                theirbench.reproduction_record(first, "gaze-v0.15.1", {**release, "crates_tree": "0" * 40})


TUNED = "presidio-tuned-presidio-research"


class VendorTunedRowTest(unittest.TestCase):
    """A vendor's own benchmark is charted against the vendor's published tuned setup."""

    def declaration(self) -> dict:
        return json.loads((Path(__file__).with_name("vendor-tuned.json")).read_text(encoding="utf-8"))["presidio-research"]

    def entry_and_report(self):
        data = synthetic()
        entry = data["benchmarks"]["presidio-research"]
        entry.update(identity={"documents": 1500, "sha256": "d" * 64}, splits={"test": {"documents": 1500}},
                     label_maps_sha256="a" * 64, mapping_sha256="b" * 64, typed_hold=["gaze", "opf"])
        entry["rescored_with"]["comparison_sha256"] = {"compare.py": "c" * 64}
        entry["reproduction"]["reproduced"]["custom"] = {"f2": 0.9}
        decl = self.declaration()
        raw = "7" * 64
        report = {
            "schema_version": 1, "benchmark": "presidio-research", "preflight": None,
            "harness_dirty": False, "harness_revision": "d" * 40, "hardware": "hw",
            "generated_at": "2026-09-29T00:00:00+00:00",
            "identity": entry["identity"], "splits": entry["splits"],
            "common_intersection_labels": entry["common_intersection_labels"],
            "label_maps_sha256": entry["label_maps_sha256"], "mapping_sha256": entry["mapping_sha256"],
            "typed_hold": entry["typed_hold"], "comparison_sha256": {"compare.py": "c" * 64},
            "rows": {TUNED: {"test": row(7)}},
            "provenance": {TUNED: {"vendor_tuned": {
                **{key: decl[key] for key in ("setup", "source", "commit", "caption")},
                "producer": {"raw_sha256": raw, "smoke_limit": None, "openmed": dict(decl["model"]),
                             "dataset_sha256": "d" * 64}, "raw_sha256": raw,
                "prediction_sha256": "8" * 64}}},
        }
        own = own_result(TUNED, tuned_replay=True, evaluator_commit="6db3769a")
        own["input"]["prediction_sha256"] = "8" * 64
        return data, entry, report, own

    def test_the_tuned_row_replaces_the_declared_presidio_bar_only(self) -> None:
        import benchmark_charts as charts
        import render_theirbench as render

        data, entry, report, own = self.entry_and_report()
        self.assertEqual(render.add_tuned(data, report, own, self.declaration()), TUNED)
        self.assertEqual(entry["chart_rows"], ["gaze-full", TUNED, "opf"])
        self.assertIn(TUNED, entry["rows"])
        entry["splits"] = {"test": {"documents": 1500}}
        for block in entry["rows"].values():
            block["product_coverage"].update(char_level={"f2": 0.5}, total_bytes=1000)
        panel = charts.third_party_panel(entry, "Presidio Research", "F2",
                                         {"presidio": "presidio-strong", "opf": "opf"}, "v0.15.1")
        bars = {bar.name: bar for bar in panel.bars}
        self.assertNotIn("Presidio", bars)  # default and strong are not on the panel
        self.assertEqual(bars["Presidio (tuned)"].leaked, 7)
        self.assertEqual(bars["Presidio (tuned)"].column, "Presidio")
        self.assertIn("Presidio tuned for this dataset by its authors (their published custom setup)", panel.caption)
        table = charts.model_card_tables([panel])
        self.assertIn("(tuned)", table)
        self.assertIn(panel.caption, table)
        body = render.render(data)
        self.assertIn(f"Row {TUNED}: Presidio tuned for this dataset by its authors", body)
        self.assertIn("| presidio-strong |", body)  # still in the page table

    def test_the_page_states_the_source_as_text_not_a_deep_link(self) -> None:
        """The published-docs scrub tokenizes a bare deep link (CI `test`, xtask scrub_public_text)."""
        import render_theirbench as render

        data, entry, report, own = self.entry_and_report()
        render.add_tuned(data, report, own, self.declaration())
        body = render.render(data)
        self.assertNotIn("https://", body)
        self.assertIn("(source `microsoft/presidio-research` `notebooks/5_Evaluate_Custom_Presidio_Analyzer.ipynb`, "
                      "commit `6db3769a`)", body)
        self.assertEqual(render.source_text("https://github.com/o/r/blob/abc/a/b.ipynb"), "`o/r` `a/b.ipynb`")

    def test_tuned_can_win_and_the_panel_shows_it(self) -> None:
        import benchmark_charts as charts

        data, entry, report, own = self.entry_and_report()
        import render_theirbench as render

        report["rows"][TUNED]["test"]["product_coverage"]["char_level"] = {"f2": 0.95}
        render.add_tuned(data, report, own, self.declaration())
        entry["rows"]["gaze-v0.15.1"]["product_coverage"]["char_level"] = {"f2": 0.7}
        for name, block in entry["rows"].items():
            block["product_coverage"].setdefault("char_level", {"f2": 0.5})
            block["product_coverage"].setdefault("total_bytes", 1000)
        panel = charts.third_party_panel(entry, "Presidio Research", "F2",
                                         {"presidio": "presidio-strong", "opf": "opf"}, "v0.15.1")
        f2 = {bar.name: bar.f2 for bar in panel.bars}
        self.assertGreater(f2["Presidio (tuned)"], f2["Gaze 0.15"])

    def test_each_mismatch_refuses_the_row(self) -> None:
        import render_theirbench as render

        def refused(mutate, match: str) -> None:
            data, _entry, report, own = self.entry_and_report()
            mutate(report, own, data)
            with self.assertRaisesRegex(ValueError, match):
                render.add_tuned(data, report, own, self.declaration())

        tuned = lambda r: r["provenance"][TUNED]["vendor_tuned"]  # noqa: E731
        refused(lambda r, o, d: r.update(harness_dirty=True), "clean, full")
        refused(lambda r, o, d: r.update(identity={"documents": 1}), "identity differs")
        refused(lambda r, o, d: r.update(comparison_sha256={"compare.py": "0"}), "different pinned")
        refused(lambda r, o, d: r["rows"].update({"presidio-en": {"test": row(5)}}), "exactly the vendor-tuned")
        refused(lambda r, o, d: tuned(r).update(commit="0" * 40), "does not match the declared")
        refused(lambda r, o, d: tuned(r).update(source="https://example.invalid"), "does not match the declared")
        refused(lambda r, o, d: tuned(r)["producer"].update(smoke_limit=10), "smoke run")
        refused(lambda r, o, d: tuned(r)["producer"].update(raw_sha256="0" * 64), "does not describe")
        # P1: the model the producer ran must be the pinned one, at merge as well as at run time.
        model = lambda r: tuned(r)["producer"]["openmed"]  # noqa: E731
        refused(lambda r, o, d: model(r).update(revision="wrong"), "model revision")
        refused(lambda r, o, d: model(r).update(tree_sha256="0" * 64), "model tree_sha256")
        refused(lambda r, o, d: model(r).update(model="someone/else"), "model model")
        refused(lambda r, o, d: tuned(r)["producer"].pop("openmed"), "model model")
        refused(lambda r, o, d: tuned(r)["producer"].update(dataset_sha256="0" * 64), "different dataset")
        # P2: the replay's score must be of the measured row's predictions on the pinned dataset.
        refused(lambda r, o, d: o["input"].update(prediction_sha256="0" * 64), "own scorer read predictions")
        refused(lambda r, o, d: o["input"].update(dataset_sha256="0" * 64), "own scorer used dataset")
        refused(lambda r, o, d: o.pop("input"), "no input receipt")
        # The published score must be what the receipt's counts give (recomputed at merge).
        refused(lambda r, o, d: o["scored"].update(f2=0.01), "published f2 0.01 is not what its counts give")
        refused(lambda r, o, d: o["scored"].update(precision=0.99), "published precision")
        refused(lambda r, o, d: o["scored"].update(recall=0.1), "published recall")
        refused(lambda r, o, d: o["input"]["counts"].update(annotated=101), "published recall")
        refused(lambda r, o, d: o["input"]["counts"].update(true_positives=90, predicted=80), "counts are inconsistent")
        refused(lambda r, o, d: o["input"].pop("counts"), "no counts")
        refused(lambda r, o, d: o["input"].update(documents=1499), "scored 1499 documents")
        refused(lambda r, o, d: o.update(tuned_replay=False), "notebook-5 replay")
        refused(lambda r, o, d: o.update(system="presidio-strong"), "notebook-5 replay")
        refused(lambda r, o, d: d["benchmarks"]["presidio-research"]["reproduction"]["reproduced"].clear(),
                "reproduced first")
        refused(lambda r, o, d: d["benchmarks"]["presidio-research"]["rows"].update({TUNED: row(1)}),
                "already has")

    def test_piibench_has_no_vendor_tuned_setup_and_says_so(self) -> None:
        import benchmark_charts as charts

        self.assertIsNone(json.loads(Path(__file__).with_name("vendor-tuned.json").read_text())["piibench-commercial"])
        entry = synthetic()["benchmarks"]["presidio-research"]
        entry["splits"] = {"test": {"documents": 5000}}
        for block in entry["rows"].values():
            block["product_coverage"].update(char_level={"f2": 0.5}, total_bytes=1000)
        panel = charts.third_party_panel(entry, "PIIBench-commercial", "F1",
                                         {"presidio": "presidio-strong", "opf": "opf"}, "v0.15.1")
        self.assertIn("declared best configuration", panel.caption)
        self.assertEqual({bar.name for bar in panel.bars}, {"Gaze 0.15", "Presidio", "OPF"})

    def test_tuned_labels_compose_and_the_tuned_scorer_leaves_common_labels_alone(self) -> None:
        import theirbench

        mappings = theirbench.compare.load_mapping()
        decl = self.declaration()
        composed = theirbench.tuned_mapping(decl, mappings, "presidio-research")
        self.assertEqual(composed["FIRST_NAME"], ("PERSON",))
        self.assertEqual(composed["TITLE"], ("TITLE",))
        self.assertEqual(composed["GENDER"], ())
        self.assertEqual(composed["EMAIL_ADDRESS"], ("EMAIL_ADDRESS",))
        # Every entity the notebook's OpenMed mapping can emit has a table entry.
        import presidio_research_repro as repro

        emitted = set(repro.OPENMED_MAPPING.values()) | {"TITLE"}
        self.assertEqual(sorted(emitted - set(composed)), [])

    def test_a_tuned_run_needs_its_findings_and_a_fresh_report(self) -> None:
        import argparse
        import theirbench

        with tempfile.TemporaryDirectory() as tmp:
            out = Path(tmp) / "r.json"
            ok = dict(tool=None, gaze_release_tag=None, tuned_raw=Path("raw"), tuned_meta=Path("meta"), output=out)
            theirbench.validate_vendor_tuned_args(argparse.Namespace(**ok))
            for bad in ({"tuned_raw": None}, {"tool": ["opf"]}, {"gaze_release_tag": "v0.15.1"}):
                with self.assertRaisesRegex(SystemExit, "needs --tuned-raw"):
                    theirbench.validate_vendor_tuned_args(argparse.Namespace(**{**ok, **bad}))
            out.write_text("{}")
            with self.assertRaisesRegex(SystemExit, "fresh report"):
                theirbench.validate_vendor_tuned_args(argparse.Namespace(**ok))

    def test_the_recomputation_accepts_the_committed_own_scores_and_the_piibench_shape(self) -> None:
        """One rule for both merge paths, exercised on every committed own-metric shape."""
        import tagged_gaze

        tagged_gaze.check_own_score(own_result("x"), 1500, "fixture")
        # PIIBench: seqeval span F1, four digits, `overall` instead of `scored`.
        tp, predicted, annotated = 1300, 4472, 4993
        precision, recall = tp / predicted, tp / annotated
        f1 = 2 * precision * recall / (precision + recall)
        own = {"overall": {"precision": round(precision, 4), "recall": round(recall, 4), "f1": round(f1, 4)},
               "input": {"documents": 5000, "beta": 1, "digits": 4,
                         "counts": {"true_positives": tp, "predicted": predicted, "annotated": annotated}}}
        tagged_gaze.check_own_score(own, 5000, "piibench")
        own["overall"]["f1"] = 0.01
        with self.assertRaisesRegex(ValueError, "published f1 0.01"):
            tagged_gaze.check_own_score(own, 5000, "piibench")
        # Zero predictions score 0, never a division error.
        zero = own_result("z")
        zero["input"]["counts"] = {"true_positives": 0, "predicted": 0, "annotated": 100}
        zero["scored"].update(precision=0.0, recall=0.0, f2=0.0)
        tagged_gaze.check_own_score(zero, 1500, "zero")

    def test_the_pin_is_in_the_declaration_and_the_shared_checks_refuse_a_mismatch(self) -> None:
        import tagged_gaze

        model = self.declaration()["model"]
        self.assertEqual(set(model), {"model", "revision", "tree_sha256"})
        self.assertEqual(len(model["revision"]), 40)
        self.assertEqual(len(model["tree_sha256"]), 64)
        tagged_gaze.check_model_receipt(dict(model), model, "ok")
        for key in model:
            with self.assertRaisesRegex(ValueError, f"model {key}"):
                tagged_gaze.check_model_receipt({**model, key: "x"}, model, "receipt")

    def test_the_producer_refuses_a_model_directory_that_is_not_the_pin_before_inference(self) -> None:
        import subprocess

        with tempfile.TemporaryDirectory() as tmp:
            fake = Path(tmp) / "model"
            fake.mkdir()
            (fake / "config.json").write_text("{}")
            # Runs under the plain interpreter: the pin check comes before any vendor import.
            done = subprocess.run(
                [sys.executable, str(Path(__file__).with_name("tuned_presidio.py")), "--checkout", tmp,
                 "--openmed-model", str(fake), "--output", str(Path(tmp) / "o"), "--meta", str(Path(tmp) / "m")],
                capture_output=True, text=True)
        self.assertNotEqual(done.returncode, 0)
        self.assertIn("--openmed-model: model tree_sha256", done.stderr)
        self.assertNotIn("Traceback", done.stderr)

    def test_benchmark_without_a_tuned_setup_is_refused(self) -> None:
        import theirbench

        with self.assertRaisesRegex(SystemExit, "no vendor-tuned setup"):
            theirbench.vendor_tuned_entry("piibench-commercial")


class HarnessTagTest(unittest.TestCase):
    def test_tag_must_point_at_the_recorded_commit(self) -> None:
        import render_theirbench as render

        entry = {"harness_revision": "aaa", "rescored_with": {"harness_revision": "bbb"}}
        tags = {"measured": "bench/m", "rescored": "bench/r"}
        good = {"bench/m": "aaa", "bench/r": "bbb"}
        self.assertEqual(render.checked_harness_tags(entry, tags, good.get), tags)
        with self.assertRaisesRegex(ValueError, "rescored harness commit"):
            render.checked_harness_tags(entry, tags, {"bench/m": "aaa", "bench/r": "aaa"}.get)
        with self.assertRaises(ValueError):
            render.checked_harness_tags(entry, {"other": "bench/m"}, good.get)

if __name__ == "__main__":
    unittest.main()
