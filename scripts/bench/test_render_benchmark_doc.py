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
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

import render_benchmark_doc as render

#: A real harness scorecard, trimmed to the fields the renderer reads.
#: See fixtures/make_real_scorecard_fixture.py for provenance and re-derivation.
REAL_SCORECARD = Path(__file__).resolve().parent / "fixtures" / "real-scorecard-v4.json"

#: The module as CI invokes it: a subprocess, so the check-door tests key on
#: the exit code the workflow reads, not an in-process return value.
MODULE = Path(__file__).resolve().parent / "render_benchmark_doc.py"

#: Distinguishes "this key is absent" from "this key holds None" in the tamper
#: matrices below; both must be refused, for different reasons.
_MISSING = object()


def _label(path) -> str:
    """Readable subTest label for a path that may index into a list."""
    return ".".join(str(key) for key in path)


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
            # Shape is production's, verified against REAL_SCORECARD below: a
            # top-level `sha256` plus a per-component map. Hand-inventing this
            # block is what published the corpus digest as `n/a`.
            "integrity": {
                "sha256": "f" * 64,
                "component_sha256": {"dataiku": "a" * 64, "negative_corpus": "b" * 64},
            },
            "evaluated_population": {"documents": 2910, "entities": 14719},
        },
        "parameters": {"profile": "full", "sampling_seed": 20260710, "ner_threshold": 0.3},
        "runs": runs,
        "runner_provenance": {
            "entry_point": "scripts/bench/run_no_opf_benchmark.py",
            "model_bundles": [
                {"model_id": "kiji-distilbert", "expected_sha256": "c" * 64},
                {"model_id": "davlan-mbert-ner-hrl-onnx", "expected_sha256": "d" * 64},
            ],
        },
    }


def entry(version: str = "v0.14.0", **kwargs) -> dict:
    return render.history_entry_from_scorecard(
        scorecard(**kwargs),
        version=version,
        machine="Test host, 1 core, 1 GB",
        scorecard_filename=f"scorecard-{version}.json",
        scorecard_sha256="0" * 64,
    )


def history_of(item: dict) -> dict:
    """Wrap one already-extracted entry in an otherwise empty history."""
    value = render.empty_history()
    value["releases"].append(item)
    return value


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

    def test_nested_provenance_fields_move_the_document(self):
        """The blocks that live under `dataset` / `provenance` in the entry.

        These are the ones that regressed: the corpus digest rendered from keys
        no scorecard emits, so the cell was constant `n/a` and no assertion on
        the flat fields above could see it.
        """
        baseline = render.apply_blocks(DOC, history())
        for path, value in (
            (("dataset", "integrity", "sha256"), "9" * 64),
            (("dataset", "integrity", "component_sha256", "dataiku"), "9" * 64),
            (("dataset", "integrity", "component_sha256", "negative_corpus"), "9" * 64),
            (("dataset", "evaluated_population", "documents"), 4242),
            (("dataset", "evaluated_population", "entities"), 4242),
            (("provenance", "model_bundles", 0, "expected_sha256"), "9" * 64),
            (("provenance", "model_bundles", 1, "model_id"), "some-other-model"),
        ):
            with self.subTest(path=".".join(str(part) for part in path)):
                mutated = copy.deepcopy(history())
                node = mutated["releases"][-1]
                for key in path[:-1]:
                    node = node[key]
                node[path[-1]] = value
                self.assertNotEqual(
                    baseline,
                    render.apply_blocks(DOC, mutated),
                    f"{path} is not reflected in the rendered document",
                )

    def test_no_required_provenance_cell_renders_as_na(self):
        """`n/a` in the provenance table means a field silently went missing."""
        rendered = render.render_current_release(history())
        for line in rendered.splitlines():
            if line.startswith("|") and "n/a" in line:
                self.fail(f"provenance table renders a missing field as n/a: {line}")


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


class VersionOrderTest(unittest.TestCase):
    """`releases[-1]` drives the Current release section, so ties are unsafe."""

    def test_prerelease_sorts_before_its_release(self):
        ordered = sorted(
            ["v0.14.0", "v0.14.0-rc.2", "v0.13.0", "v0.14.0-rc.1", "v0.14.1"],
            key=render.version_sort_key,
        )
        self.assertEqual(
            ordered,
            ["v0.13.0", "v0.14.0-rc.1", "v0.14.0-rc.2", "v0.14.0", "v0.14.1"],
        )

    def test_a_release_and_its_prerelease_never_compare_equal(self):
        self.assertNotEqual(
            render.version_sort_key("v0.14.0"),
            render.version_sort_key("v0.14.0-rc.1"),
        )

    def test_alphanumeric_and_numeric_identifiers_are_comparable(self):
        ordered = sorted(
            ["v1.0.0-alpha", "v1.0.0-1", "v1.0.0-alpha.1"],
            key=render.version_sort_key,
        )
        self.assertEqual(ordered, ["v1.0.0-1", "v1.0.0-alpha", "v1.0.0-alpha.1"])


class RealScorecardShapeTest(unittest.TestCase):
    """The hand-written fixture must agree with a real harness scorecard.

    `scorecard()` above is convenient but invented, and an invented shape is
    exactly how `integrity.algorithm` / `integrity.value` — keys the harness has
    never emitted — passed 28 green tests while publishing `Corpus sha256|n/a`
    on every real release row. These tests bind the fixture to real bytes.
    """

    @classmethod
    def setUpClass(cls):
        cls.real = json.loads(REAL_SCORECARD.read_text(encoding="utf-8"))

    def test_fixture_file_is_present_and_is_a_v4_scorecard(self):
        self.assertEqual(self.real["schema_version"], render.SCORECARD_SCHEMA_VERSION)

    def _shape(self, value):
        """Key structure only; values are irrelevant to a shape comparison."""
        if isinstance(value, dict):
            return {key: self._shape(item) for key, item in sorted(value.items())}
        if isinstance(value, list):
            return [self._shape(value[0])] if value else []
        return type(value).__name__

    def test_hand_written_integrity_block_matches_the_real_one(self):
        self.assertEqual(
            self._shape(scorecard()["dataset"]["integrity"]),
            self._shape(self.real["dataset"]["integrity"]),
            "the hand-written integrity block has drifted from the harness shape",
        )

    def test_hand_written_provenance_blocks_are_a_subset_of_the_real_ones(self):
        for block, keys in (
            (("dataset", "evaluated_population"), None),
            (("runner_provenance",), ("entry_point", "model_bundles")),
        ):
            node_fixture = scorecard()
            node_real = self.real
            for key in block:
                node_fixture = node_fixture[key]
                node_real = node_real[key]
            for key in keys or node_fixture:
                with self.subTest(block=".".join(block), key=key):
                    self.assertIn(key, node_real)

    def test_real_scorecard_renders_its_corpus_digest_and_components(self):
        integrity = self.real["dataset"]["integrity"]
        rendered = render.render_current_release(
            history_of(
                render.history_entry_from_scorecard(
                    self.real,
                    version="v0.14.0",
                    machine="Test host, 1 core, 1 GB",
                    scorecard_filename="scorecard-v0.14.0.json",
                    scorecard_sha256="0" * 64,
                )
            )
        )
        self.assertIn(f"| Corpus sha256 | `{integrity['sha256']}` |", rendered)
        for component, digest in integrity["component_sha256"].items():
            self.assertIn(f"`{component}`", rendered)
            self.assertIn(digest, rendered)
        self.assertNotIn("n/a", rendered)

    def test_real_scorecard_renders_population_and_model_bundles(self):
        rendered = render.render_current_release(
            history_of(
                render.history_entry_from_scorecard(
                    self.real,
                    version="v0.14.0",
                    machine="m",
                    scorecard_filename="scorecard-v0.14.0.json",
                    scorecard_sha256="0" * 64,
                )
            )
        )
        population = self.real["dataset"]["evaluated_population"]
        self.assertIn(f"{population['documents']:,} documents", rendered)
        self.assertIn(f"{population['entities']:,} entities", rendered)
        for bundle in self.real["runner_provenance"]["model_bundles"]:
            self.assertIn(f"`{bundle['model_id']}`", rendered)
            self.assertIn(bundle["expected_sha256"], rendered)

    def test_real_scorecard_round_trips_through_the_cli(self):
        """End to end: append the real scorecard, render, then `--check`."""
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            scorecard_path = root / "scorecard-v0.14.0.json"
            scorecard_path.write_text(json.dumps(self.real), encoding="utf-8")
            history_path = root / "release-history.json"
            history_path.write_text(
                json.dumps(render.empty_history()), encoding="utf-8"
            )
            doc_path = root / "README.md"
            doc_path.write_text(DOC, encoding="utf-8")
            argv = ["--doc", str(doc_path), "--history", str(history_path)]
            self.assertEqual(
                render.main(
                    argv
                    + [
                        "--append-history",
                        "--scorecard",
                        str(scorecard_path),
                        "--version",
                        "v0.14.0",
                        "--machine",
                        "Test host, 1 core, 1 GB",
                    ]
                ),
                0,
            )
            written = doc_path.read_text(encoding="utf-8")
            self.assertIn(self.real["dataset"]["integrity"]["sha256"], written)
            self.assertEqual(render.main(argv + ["--check"]), 0)


class RequiredProvenanceGuardTest(unittest.TestCase):
    """A missing provenance field must fail loudly, never render as `n/a`.

    This is the durable half of the fix: with these guards the *next* schema
    move turns the suite red instead of publishing a blank cell, so the test
    fixture's fidelity stops being the only thing standing between a schema
    change and a released document that claims a digest it does not have.
    """

    def _reject(self, mutate):
        broken = scorecard()
        mutate(broken)
        with self.assertRaises(render.RenderError):
            render.history_entry_from_scorecard(
                broken,
                version="v0.14.0",
                machine="m",
                scorecard_filename="scorecard-v0.14.0.json",
                scorecard_sha256="0" * 64,
            )

    def test_missing_integrity_block_is_refused(self):
        self._reject(lambda card: card["dataset"].pop("integrity"))

    def test_missing_corpus_digest_is_refused(self):
        self._reject(lambda card: card["dataset"]["integrity"].pop("sha256"))

    def test_non_hex_corpus_digest_is_refused(self):
        self._reject(
            lambda card: card["dataset"]["integrity"].update({"sha256": "not-a-digest"})
        )

    def test_legacy_algorithm_value_shape_is_refused(self):
        """The shape the renderer used to read is not a valid scorecard."""
        self._reject(
            lambda card: card["dataset"].update(
                {"integrity": {"algorithm": "sha256", "value": "f" * 64}}
            )
        )

    def test_missing_evaluated_population_is_refused(self):
        self._reject(lambda card: card["dataset"].pop("evaluated_population"))

    def test_missing_model_bundles_is_refused(self):
        self._reject(lambda card: card["runner_provenance"].pop("model_bundles"))

    def test_model_bundle_without_a_digest_is_refused(self):
        self._reject(
            lambda card: card["runner_provenance"]["model_bundles"][0].pop(
                "expected_sha256"
            )
        )

    def test_history_file_missing_provenance_is_refused(self):
        """The `--check` door: CI renders from this file, not the scorecard.

        A guard only at extraction would let a hand-edited history file publish
        a blank, because `--check` never reads a scorecard.
        """
        for path in (
            ("dataset", "integrity", "sha256"),
            ("dataset", "evaluated_population", "documents"),
            ("dataset", "evaluated_population", "entities"),
            ("provenance", "model_bundles"),
        ):
            with self.subTest(path=".".join(path)):
                broken = copy.deepcopy(history())
                node = broken["releases"][-1]
                for key in path[:-1]:
                    node = node[key]
                del node[path[-1]]
                with self.assertRaises(render.RenderError):
                    render.validate_history(broken)

    def test_a_well_formed_history_still_validates(self):
        render.validate_history(history("v0.13.0", "v0.14.0"))

    def test_empty_model_bundle_list_is_allowed(self):
        """A rule-only run pins no model; that is a fact, not a missing field."""
        card = scorecard()
        card["runner_provenance"]["model_bundles"] = []
        rendered = render.render_current_release(
            history_of(
                render.history_entry_from_scorecard(
                    card,
                    version="v0.14.0",
                    machine="m",
                    scorecard_filename="scorecard-v0.14.0.json",
                    scorecard_sha256="0" * 64,
                )
            )
        )
        self.assertIn("no neural backend", rendered)
        self.assertNotIn("n/a", rendered)


class HistoryProvenanceValueGuardTest(unittest.TestCase):
    """The `--check` door must validate digest *values*, not key presence.

    `history_entry_from_scorecard` has always run every digest through
    `_require_hex64`; `validate_history` only checked that the key existed. CI
    renders from `release-history.json` alone, so a hand-edited file carrying
    `"n/a"` published that exact string with the gate green -- the failure this
    change exists to eliminate, reached through the door meant to stop it.
    """

    #: Values that occupy a digest slot without being one. `"n/a"` is the exact
    #: published string being kept out; the rest are the near-misses a hand-edit
    #: or a half-finished schema migration produces.
    NON_DIGESTS = (
        "n/a",
        "N/A",
        "",
        None,
        "f" * 63,
        "f" * 65,
        "F" * 64,
        "g" * 64,
        0,
        ["f" * 64],
    )

    #: A population count is printed as `{value:,}`; anything that is not a
    #: non-negative int either publishes a lie or crashes at the format site.
    NON_COUNTS = (-1, -2910, "lots", "2910", 2910.0, None, True, [2910])

    def _history_with(self, path, value):
        broken = copy.deepcopy(history())
        node = broken["releases"][-1]
        for key in path[:-1]:
            node = node[key]
        node[path[-1]] = value
        return broken

    def _assert_all_refused(self, path, values):
        for value in values:
            with self.subTest(path=_label(path), value=value):
                with self.assertRaises(render.RenderError):
                    render.validate_history(self._history_with(path, value))

    def test_corpus_digest_value_is_validated_on_the_history_path(self):
        self._assert_all_refused(("dataset", "integrity", "sha256"), self.NON_DIGESTS)

    def test_every_component_digest_value_is_validated(self):
        components = history()["releases"][-1]["dataset"]["integrity"][
            "component_sha256"
        ]
        self.assertTrue(components, "fixture must carry component digests")
        for component in components:
            self._assert_all_refused(
                ("dataset", "integrity", "component_sha256", component),
                self.NON_DIGESTS,
            )

    def test_a_broken_component_digest_map_is_refused(self):
        for value in (None, "n/a", [], 0):
            with self.subTest(value=value):
                with self.assertRaises(render.RenderError):
                    render.validate_history(
                        self._history_with(
                            ("dataset", "integrity", "component_sha256"), value
                        )
                    )

    def test_every_model_bundle_digest_value_is_validated(self):
        bundles = history()["releases"][-1]["provenance"]["model_bundles"]
        self.assertTrue(bundles, "fixture must carry model bundles")
        for index in range(len(bundles)):
            self._assert_all_refused(
                ("provenance", "model_bundles", index, "expected_sha256"),
                self.NON_DIGESTS,
            )

    def test_every_model_bundle_needs_a_non_empty_model_id(self):
        for value in ("", None, 0, ["kiji"]):
            with self.subTest(value=value):
                with self.assertRaises(render.RenderError):
                    render.validate_history(
                        self._history_with(
                            ("provenance", "model_bundles", 0, "model_id"), value
                        )
                    )

    def test_a_model_bundle_entry_must_be_an_object(self):
        broken = copy.deepcopy(history())
        broken["releases"][-1]["provenance"]["model_bundles"][0] = "kiji-distilbert"
        with self.assertRaises(render.RenderError):
            render.validate_history(broken)

    def test_scorecard_digest_value_is_validated(self):
        self._assert_all_refused(("scorecard_sha256",), self.NON_DIGESTS)

    def test_population_counts_are_validated(self):
        for key in ("documents", "entities"):
            self._assert_all_refused(
                ("dataset", "evaluated_population", key), self.NON_COUNTS
            )

    def test_the_guard_refuses_bad_values_without_refusing_good_ones(self):
        render.validate_history(history("v0.13.0", "v0.14.0"))


class RenderSiteRefusesNonDigestsTest(unittest.TestCase):
    """The printer reads provenance through the same predicate that guards it.

    Guarding only upstream would make a reintroduced `n/a` fallback at the
    render site unreachable, and therefore untestable: "renderer falls back to
    n/a" has to stay a mutation this suite can kill.
    """

    def _render_with(self, path, value):
        item = copy.deepcopy(entry())
        node = item
        for key in path[:-1]:
            node = node[key]
        if value is _MISSING:
            del node[path[-1]]
        else:
            node[path[-1]] = value
        return render.render_current_release(history_of(item))

    def test_missing_or_non_digest_provenance_never_renders(self):
        for path in (
            ("dataset", "integrity", "sha256"),
            ("provenance", "model_bundles", 0, "expected_sha256"),
            ("scorecard_sha256",),
        ):
            for value in (_MISSING, "n/a", None, ""):
                with self.subTest(path=_label(path), value=value):
                    with self.assertRaises(render.RenderError):
                        self._render_with(path, value)

    def test_a_component_digest_may_be_absent_but_never_a_non_digest(self):
        """The boundary between omitted evidence and a false claim.

        Nothing records which components an entry *should* carry, so a dropped
        key can only ever print one row fewer -- it cannot publish something
        untrue. A key that is present must hold a real digest.
        """
        path = ("dataset", "integrity", "component_sha256", "dataiku")
        rendered = self._render_with(path, _MISSING)
        self.assertNotIn("`dataiku`", rendered)
        self.assertNotIn("n/a", rendered)
        for value in ("n/a", None, ""):
            with self.subTest(value=value):
                with self.assertRaises(render.RenderError):
                    self._render_with(path, value)

    def test_missing_or_non_integer_population_never_renders(self):
        for key in ("documents", "entities"):
            for value in (_MISSING, "n/a", None, -1):
                with self.subTest(key=key, value=value):
                    with self.assertRaises(render.RenderError):
                        self._render_with(
                            ("dataset", "evaluated_population", key), value
                        )

    def test_a_well_formed_entry_still_renders_every_provenance_row(self):
        rendered = render.render_current_release(history_of(entry()))
        self.assertIn(f"| Corpus sha256 | `{'f' * 64}` |", rendered)
        self.assertIn(f"| Scorecard sha256 | `{'0' * 64}` |", rendered)
        self.assertIn("2,910 documents / 14,719 entities", rendered)
        self.assertNotIn("n/a", rendered)


class CheckDoorTest(unittest.TestCase):
    """The reviewer's reproduction, driven through the real CLI entry point.

    `--check` is what CI runs, and it never reads a scorecard. A subprocess is
    the honest shape here: it exercises the process exit code the workflow
    actually keys on, not an in-process return value.
    """

    def _run_check(self, root):
        return subprocess.run(
            [
                sys.executable,
                str(MODULE),
                "--doc",
                str(root / "README.md"),
                "--history",
                str(root / "release-history.json"),
                "--check",
            ],
            capture_output=True,
            text=True,
        )

    def test_check_refuses_a_history_that_would_publish_na(self):
        for path in (
            ("dataset", "integrity", "sha256"),
            ("dataset", "integrity", "component_sha256", "dataiku"),
            ("provenance", "model_bundles", 0, "expected_sha256"),
            ("scorecard_sha256",),
        ):
            with self.subTest(path=_label(path)):
                with tempfile.TemporaryDirectory() as tmp:
                    root = Path(tmp)
                    (root / "scorecard-v0.14.0.json").write_text("{}", encoding="utf-8")
                    doc_path = root / "README.md"
                    doc_path.write_text(DOC, encoding="utf-8")
                    broken = copy.deepcopy(history())
                    node = broken["releases"][-1]
                    for key in path[:-1]:
                        node = node[key]
                    node[path[-1]] = "n/a"
                    (root / "release-history.json").write_text(
                        json.dumps(broken), encoding="utf-8"
                    )
                    result = self._run_check(root)
                    # Exit 2 is the RenderError path; exit 1 only means the
                    # document drifted. Asserting "non-zero" would pass on a
                    # stale README without the guard ever running.
                    self.assertEqual(
                        result.returncode,
                        2,
                        "--check must refuse the non-digest itself, not merely "
                        f"report drift: rc={result.returncode} {result.stdout}",
                    )
                    self.assertNotIn("n/a", result.stdout)
                    self.assertNotIn("n/a", doc_path.read_text(encoding="utf-8"))

    def test_check_still_passes_on_a_well_formed_history(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / "scorecard-v0.14.0.json").write_text("{}", encoding="utf-8")
            (root / "release-history.json").write_text(
                json.dumps(history()), encoding="utf-8"
            )
            (root / "README.md").write_text(
                render.apply_blocks(DOC, history()), encoding="utf-8"
            )
            result = self._run_check(root)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertIn("is in sync", result.stdout)


if __name__ == "__main__":
    unittest.main()
