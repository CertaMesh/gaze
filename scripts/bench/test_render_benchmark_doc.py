#!/usr/bin/env python3
"""Contract tests for scripts/bench/render_benchmark_doc.py.

The load-bearing tests are the two mutation sweeps. A renderer that silently
drops a column would still produce a plausible document, so presence checks
prove nothing: instead every mapped scorecard path is mutated in turn and the
extracted entry must move, and every rendered field is mutated in turn and the
document must move. Adding a column without wiring it fails here.
"""

from __future__ import annotations

import copy
import json
import tempfile
import unittest
from pathlib import Path

import render_benchmark_doc as render


def _arm(seed: int) -> dict:
    return {
        "config": "placeholder",
        "metrics": {
            "zero_leak_document_rate": 0.35 + seed / 100,
            "utf8_bytes": {
                "pii": 130282,
                "leaked": 93850 - seed * 1000,
                "leak_rate": 0.7204 - seed / 100,
                "false_positive": 5423 + seed,
                "precision": 0.8704336399474376,
            },
        },
        "pipeline_contract": {
            "restore_exact_rate": 1.0,
            "manifest_valid_document_rate": 1.0,
        },
        "pipeline_availability": {
            "completion_rate": 1.0,
            "failed_closed_documents": 0,
        },
        "latency_ms": {"clean_ms": {"p95": 4.07 + seed}},
    }


def scorecard(revision: str = "a" * 40, dirty: bool = False) -> dict:
    runs = []
    for index, name in enumerate(
        ("rule-floor-extended", "pass2-ner", "full-stack-kiji-resolve")
    ):
        run = _arm(index)
        run["config"] = name
        runs.append(run)
    return {
        "schema_version": 4,
        "generated_at": "2026-09-12T04:26:35.500796+00:00",
        "gaze": {"revision": revision, "dirty": dirty},
        "dataset": {
            "repository": "DataikuNLP/kiji-pii-training-data+gaze",
            "revision": "0275550+a4-negative-v1",
            "integrity": {"algorithm": "sha256", "value": "f" * 64},
        },
        "parameters": {"profile": "full", "sampling_seed": 20260710, "ner_threshold": 0.3},
        "runs": runs,
        "runner_provenance": {"entry_point": "scripts/bench/run_no_opf_benchmark.py"},
    }


def entry(version: str = "v0.14.0", **kwargs) -> dict:
    return render.history_entry_from_scorecard(
        scorecard(**kwargs),
        version=version,
        machine="Test host, 1 core, 1 GB",
        scorecard_filename=f"scorecard-{version}.json",
        scorecard_sha256="0" * 64,
    )


def history(*versions: str) -> dict:
    value = render.empty_history()
    for index, version in enumerate(versions or ("v0.14.0",)):
        item = entry(version)
        for block in item["arms"].values():
            block["surviving_pii_utf8_bytes"] -= index * 500
        value["releases"].append(item)
    return value


DOC = """# Gaze Benchmarks

Prose that must survive untouched.

<!-- BEGIN GENERATED: current-release -->
stale
<!-- END GENERATED: current-release -->

More prose.

<!-- BEGIN GENERATED: charts -->
stale
<!-- END GENERATED: charts -->

<!-- BEGIN GENERATED: history -->
stale
<!-- END GENERATED: history -->

Trailing prose.
"""


def _mutate(value):
    if isinstance(value, bool):
        return not value
    if isinstance(value, int):
        return value + 1
    if isinstance(value, float):
        return value + 0.25
    raise AssertionError(f"no mutation defined for {value!r}")


class ScorecardMappingTest(unittest.TestCase):
    """Every scorecard path in ARM_FIELD_SOURCES must reach the history entry."""

    def test_every_mapped_source_path_moves_the_extracted_value(self):
        baseline = entry()
        for field, path in render.ARM_FIELD_SOURCES.items():
            with self.subTest(field=field):
                mutated = scorecard()
                node = mutated["runs"][0]
                for key in path[:-1]:
                    node = node[key]
                node[path[-1]] = _mutate(node[path[-1]])
                produced = render.history_entry_from_scorecard(
                    mutated,
                    version="v0.14.0",
                    machine="Test host, 1 core, 1 GB",
                    scorecard_filename="scorecard-v0.14.0.json",
                    scorecard_sha256="0" * 64,
                )
                arm = "rule-floor-extended"
                self.assertNotEqual(
                    baseline["arms"][arm][field],
                    produced["arms"][arm][field],
                    f"{field} did not follow its scorecard source {'.'.join(path)}",
                )

    def test_missing_source_path_is_rejected(self):
        broken = scorecard()
        del broken["runs"][0]["metrics"]["utf8_bytes"]["leaked"]
        with self.assertRaises(render.RenderError):
            render.history_entry_from_scorecard(
                broken,
                version="v0.14.0",
                machine="m",
                scorecard_filename="scorecard-v0.14.0.json",
                scorecard_sha256="0" * 64,
            )

    def test_dirty_tree_scorecard_is_refused(self):
        with self.assertRaises(render.RenderError):
            entry(dirty=True)

    def test_wrong_schema_version_is_refused(self):
        stale = scorecard()
        stale["schema_version"] = 3
        with self.assertRaises(render.RenderError):
            render.history_entry_from_scorecard(
                stale,
                version="v0.14.0",
                machine="m",
                scorecard_filename="scorecard-v0.14.0.json",
                scorecard_sha256="0" * 64,
            )

    def test_every_extracted_field_is_rendered(self):
        self.assertEqual(
            set(render.ARM_FIELD_SOURCES),
            {field for _, field, _ in render.ARM_COLUMNS},
            "a field is extracted but never shown, or shown but never extracted",
        )


class RenderMutationTest(unittest.TestCase):
    """Every rendered field must actually change the rendered document."""

    def test_every_arm_field_moves_the_document(self):
        baseline = render.apply_blocks(DOC, history())
        for _, field, _ in render.ARM_COLUMNS:
            with self.subTest(field=field):
                mutated = copy.deepcopy(history())
                arms = mutated["releases"][-1]["arms"]
                block = arms["rule-floor-extended"]
                block[field] = _mutate(block[field])
                self.assertNotEqual(
                    baseline,
                    render.apply_blocks(DOC, mutated),
                    f"{field} is not reflected in the rendered document",
                )

    def test_trend_chart_follows_the_shipped_default(self):
        baseline = render.apply_blocks(DOC, history("v0.12.0", "v0.13.0", "v0.14.0"))
        mutated = copy.deepcopy(history("v0.12.0", "v0.13.0", "v0.14.0"))
        arm = mutated["releases"][0]["arms"][render.SHIPPED_DEFAULT_ARM]
        arm["surviving_pii_utf8_bytes"] += 4321
        self.assertNotEqual(baseline, render.apply_blocks(DOC, mutated))

    def test_provenance_fields_move_the_document(self):
        baseline = render.apply_blocks(DOC, history())
        for field, value in (
            ("commit", "b" * 40),
            ("machine", "Another host"),
            ("date", "2099-01-01"),
            ("scorecard_sha256", "1" * 64),
        ):
            with self.subTest(field=field):
                mutated = copy.deepcopy(history())
                mutated["releases"][-1][field] = value
                self.assertNotEqual(
                    baseline,
                    render.apply_blocks(DOC, mutated),
                    f"{field} is not reflected in the rendered document",
                )


class RowCountTest(unittest.TestCase):
    """The document has to read sensibly at 0, 1, 2 and 3 release rows."""

    def test_zero_rows_renders_a_placeholder_and_no_chart(self):
        rendered = render.apply_blocks(DOC, render.empty_history())
        self.assertIn("No release has been measured yet", rendered)
        self.assertIn("*none yet*", rendered)
        self.assertNotIn("xychart-beta", rendered)

    def test_one_row_renders_the_bar_chart_but_not_the_trend(self):
        rendered = render.apply_blocks(DOC, history("v0.14.0"))
        self.assertEqual(rendered.count("xychart-beta"), 1)
        self.assertIn("The trend chart renders from two releases onward", rendered)

    def test_two_and_three_rows_render_both_charts(self):
        for versions in (("v0.13.0", "v0.14.0"), ("v0.12.0", "v0.13.0", "v0.14.0")):
            with self.subTest(rows=len(versions)):
                rendered = render.apply_blocks(DOC, history(*versions))
                self.assertEqual(rendered.count("xychart-beta"), 2)
                for version in versions:
                    self.assertIn(version, rendered)

    def test_prose_outside_the_markers_is_preserved(self):
        rendered = render.apply_blocks(DOC, history())
        self.assertIn("Prose that must survive untouched.", rendered)
        self.assertIn("More prose.", rendered)
        self.assertIn("Trailing prose.", rendered)
        self.assertNotIn("stale", rendered)

    def test_rendering_is_idempotent(self):
        once = render.apply_blocks(DOC, history("v0.13.0", "v0.14.0"))
        self.assertEqual(once, render.apply_blocks(once, history("v0.13.0", "v0.14.0")))

    def test_missing_marker_is_refused(self):
        with self.assertRaises(render.RenderError):
            render.apply_blocks("# no markers here\n", history())

    def test_provisional_row_does_not_claim_the_released_tree(self):
        plain = history("v0.14.0")
        self.assertIn("— measured on the released tree", render.apply_blocks(DOC, plain))
        marked = copy.deepcopy(plain)
        marked["releases"][-1]["provisional"] = True
        marked["releases"][-1]["note"] = "harness absent at the tag; built from main"
        rendered = render.apply_blocks(DOC, marked)
        self.assertIn("*not* measured on the released tree", rendered)
        self.assertNotIn("— measured on the released tree", rendered)
        self.assertIn("harness absent at the tag", rendered)


class HistoryValidationTest(unittest.TestCase):
    def test_empty_history_is_valid(self):
        render.validate_history(render.empty_history())

    def test_duplicate_version_is_refused(self):
        value = history("v0.14.0")
        value["releases"].append(copy.deepcopy(value["releases"][0]))
        with self.assertRaises(render.RenderError):
            render.validate_history(value)

    def test_bad_version_string_is_refused(self):
        value = history("v0.14.0")
        value["releases"][0]["version"] = "0.14"
        with self.assertRaises(render.RenderError):
            render.validate_history(value)

    def test_missing_arm_field_is_refused(self):
        value = history("v0.14.0")
        del value["releases"][0]["arms"]["pass2-ner"]["leak_rate"]
        with self.assertRaises(render.RenderError):
            render.validate_history(value)

    def test_wrong_schema_version_is_refused(self):
        value = history("v0.14.0")
        value["schema_version"] = 99
        with self.assertRaises(render.RenderError):
            render.validate_history(value)


class CliTest(unittest.TestCase):
    def setUp(self):
        self._tmp = tempfile.TemporaryDirectory()
        self.root = Path(self._tmp.name)
        self.doc = self.root / "README.md"
        self.history = self.root / "release-history.json"
        self.doc.write_text(DOC, encoding="utf-8")
        self.addCleanup(self._tmp.cleanup)

    def _write_history(self, value):
        render.write_history(self.history, value)

    def _argv(self, *extra):
        return ["--doc", str(self.doc), "--history", str(self.history), *extra]

    def test_check_passes_on_a_freshly_rendered_document(self):
        self._write_history(render.empty_history())
        self.assertEqual(render.main(self._argv()), 0)
        self.assertEqual(render.main(self._argv("--check")), 0)

    def test_check_fails_when_the_document_drifts(self):
        self._write_history(render.empty_history())
        self.assertEqual(render.main(self._argv()), 0)
        value = history("v0.14.0")
        (self.root / "scorecard-v0.14.0.json").write_text("{}", encoding="utf-8")
        self._write_history(value)
        self.assertEqual(render.main(self._argv("--check")), 1)

    def test_check_fails_when_the_scorecard_evidence_is_deleted(self):
        self._write_history(history("v0.14.0"))
        self.assertEqual(render.main(self._argv("--check")), 2)

    def test_append_history_rejects_a_mismatched_filename(self):
        self._write_history(render.empty_history())
        path = self.root / "scorecard-v9.9.9.json"
        path.write_text(json.dumps(scorecard()), encoding="utf-8")
        self.assertEqual(
            render.main(
                self._argv(
                    "--append-history",
                    "--scorecard",
                    str(path),
                    "--version",
                    "v0.14.0",
                    "--machine",
                    "m",
                )
            ),
            2,
        )

    def test_append_history_requires_machine(self):
        self._write_history(render.empty_history())
        path = self.root / "scorecard-v0.14.0.json"
        path.write_text(json.dumps(scorecard()), encoding="utf-8")
        self.assertEqual(
            render.main(
                self._argv(
                    "--append-history",
                    "--scorecard",
                    str(path),
                    "--version",
                    "v0.14.0",
                )
            ),
            2,
        )

    def test_append_history_writes_a_row_and_renders(self):
        self._write_history(render.empty_history())
        path = self.root / "scorecard-v0.14.0.json"
        path.write_text(json.dumps(scorecard()), encoding="utf-8")
        self.assertEqual(
            render.main(
                self._argv(
                    "--append-history",
                    "--scorecard",
                    str(path),
                    "--version",
                    "v0.14.0",
                    "--machine",
                    "Test host, 1 core, 1 GB",
                )
            ),
            0,
        )
        stored = json.loads(self.history.read_text(encoding="utf-8"))
        self.assertEqual([r["version"] for r in stored["releases"]], ["v0.14.0"])
        self.assertIn("v0.14.0", self.doc.read_text(encoding="utf-8"))
        self.assertEqual(render.main(self._argv("--check")), 0)

    def test_appended_rows_sort_by_version_not_insertion_order(self):
        self._write_history(render.empty_history())
        for version in ("v0.14.0", "v0.12.0", "v0.13.0"):
            path = self.root / f"scorecard-{version}.json"
            path.write_text(json.dumps(scorecard()), encoding="utf-8")
            self.assertEqual(
                render.main(
                    self._argv(
                        "--append-history",
                        "--scorecard",
                        str(path),
                        "--version",
                        version,
                        "--machine",
                        "m",
                    )
                ),
                0,
            )
        stored = json.loads(self.history.read_text(encoding="utf-8"))
        self.assertEqual(
            [r["version"] for r in stored["releases"]],
            ["v0.12.0", "v0.13.0", "v0.14.0"],
        )


class CommittedDocumentTest(unittest.TestCase):
    """The document committed in this repo must already be in sync."""

    def test_committed_document_is_in_sync(self):
        self.assertEqual(render.main(["--check"]), 0)


if __name__ == "__main__":
    unittest.main()
