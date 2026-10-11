"""Supplemental quality evidence remains value-free and replayable."""

import json
import argparse
import contextlib
import importlib.util
import io
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import patch

import safety_net_bench_lib as lib


class SafetyNetEvidenceTest(unittest.TestCase):
    def test_both_producers_retain_replayable_evidence_even_without_snapshot_updates(self):
        spec = importlib.util.spec_from_file_location("ner_bench_evidence_test", Path(__file__).with_name("ner-bench-scorer.py"))
        ner = importlib.util.module_from_spec(spec)
        sys.modules[spec.name] = ner
        spec.loader.exec_module(ner)
        span = lib.Span(0, 5, "Name")
        fixtures = [lib.Fixture("a", "Global", [], "synthetic private input", [span])]
        clean = {"a": lib.CleanFixture("private clean input", [], [span])}
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            coverage = root / "coverage.json"
            coverage.write_text('{"totals": {"fixture_count": 1}}')
            args = argparse.Namespace(mode="all", measure_latency=False, no_update=True, device="cpu",
                                      python=sys.executable, snapshot=root / "snapshot.json")
            stdout = io.StringIO()
            with patch.object(lib, "validate_inputs", return_value=(root, coverage, root, "b" * 64)), \
                 patch.object(lib, "load_fixtures", return_value=fixtures), \
                 patch.object(lib, "clean_fixtures", return_value=clean), \
                 patch.object(lib, "run_backend", return_value=[span]), contextlib.redirect_stdout(stdout):
                self.assertEqual(lib.scorer_main(args, "opf"), 0)
            opf = json.loads(stdout.getvalue())
            with patch.object(ner, "run_model", return_value=[span]):
                result = ner.run_one_model(root, args, SimpleNamespace(id="synthetic-model"), root,
                                           fixtures, clean, lib.sha256_file(coverage), "b" * 64)
            for run in (opf, result):
                self.assertEqual(set(run["observations_by_mode"]), {"direct_detector", "observer_residual"})
                for mode, ref in run["observations_by_mode"].items():
                    path = root / ref["file"]
                    self.assertEqual(lib.sha256_file(path), ref["sha256"])
                    self.assertEqual(lib.rescore_observations(json.loads(path.read_text())), run["metrics_by_mode"][mode])
                    proc = subprocess.run([sys.executable, str(Path(__file__).with_name("rescore_safety_net.py")), str(path)],
                                          capture_output=True, text=True)
                    self.assertEqual(proc.returncode, 0, proc.stderr)
                    self.assertEqual(json.loads(proc.stdout), run["metrics_by_mode"][mode])
            self.assertFalse(args.snapshot.exists())

    def test_historical_quality_data_and_docs_carry_the_same_warning(self):
        root = Path(__file__).resolve().parents[2]
        docs = (root / "docs/reference/benchmarks/README.md").read_text()
        self.assertEqual(docs.count(lib.POOLED_SCORER_WARNING), 3)
        for name in ("safety_net_matrix_snapshot.json", "ner_models_snapshot.json"):
            data = json.loads((root / "crates/gaze-recognizers/benches" / name).read_text())
            self.assertGreater(len(data["cells"]), 0)
            for cell in data["cells"]:
                self.assertEqual(cell["scoring_provenance"], lib.POOLED_SCORER_PROVENANCE)
            for provenance in data.get("strict_span_leak_rate_provenance", {}).values():
                self.assertEqual(provenance, lib.POOLED_SCORER_PROVENANCE)

    def test_direct_and_observer_evidence_replays_without_input_values(self):
        span = lib.Span(0, 5, "Name")
        fixtures = [lib.Fixture("a", "Global", [], "synthetic private text", [span]),
                    lib.Fixture("b", "Global", [], "other private text", [])]
        predictions = {"a": [], "b": [span]}
        clean = {"a": lib.CleanFixture("private clean text", [], [span]),
                 "b": lib.CleanFixture("other clean text", [], [])}
        with tempfile.TemporaryDirectory() as directory:
            for mode in ("direct_detector", "observer_residual"):
                ref = lib.retain_observations(Path(directory) / "snapshot.json", "synthetic-backend",
                                              mode, fixtures, predictions, clean, "a" * 64, "b" * 64)
                path = Path(directory) / ref["file"]
                raw = path.read_text()
                self.assertNotIn("private", raw)
                self.assertNotIn("clean_text", raw)
                self.assertNotIn("fixture_id", raw)
                self.assertEqual(lib.sha256_file(path), ref["sha256"])
                expected = (lib.score_direct(fixtures, predictions) if mode == "direct_detector"
                            else lib.score_observer(fixtures, clean, predictions))
                self.assertEqual(lib.rescore_observations(json.loads(raw)), expected)
                repeated = lib.retain_observations(Path(directory) / "snapshot.json", "synthetic-backend",
                                                   mode, fixtures, predictions, clean, "a" * 64, "b" * 64)
                self.assertEqual(repeated, ref)
            old_files = {p.name: p.read_bytes() for p in Path(directory).glob("*.json")}
            lib.retain_observations(Path(directory) / "snapshot.json", "synthetic-backend",
                                    "direct_detector", fixtures, {"a": [span], "b": []}, None, "a" * 64, "b" * 64)
            self.assertEqual(len(list(Path(directory).glob("*.json"))), 3)
            for filename, data in old_files.items():
                self.assertEqual((Path(directory) / filename).read_bytes(), data)

    def test_unmarked_legacy_snapshot_cell_is_marked_when_other_locale_is_updated(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "snapshot.json"
            path.write_text(json.dumps({"corpus": {}, "cells": [
                {"backend": "synthetic-backend", "locale": "DeDe", "mode": "direct_detector", "metrics": {"recall": 0.5}}
            ], "strict_span_leak_rate": {"synthetic-backend.DeDe": 0.5}}))
            lib.update_matrix_snapshot(path, "synthetic-backend", "direct_detector",
                                       {"Global": {"recall": 0.0}}, "a" * 64, "b" * 64, 2, {},
                                       scoring_provenance={"contract": "fixture-span-v2", "observations": {"file": "evidence.json", "sha256": "c" * 64}})
            snapshot = json.loads(path.read_text())
            self.assertEqual(snapshot["cells"][0]["scoring_provenance"]["warning"], lib.POOLED_SCORER_WARNING)
            self.assertEqual(snapshot["cells"][0]["metrics"], {"recall": 0.5})
            self.assertEqual(snapshot["cells"][1]["scoring_provenance"]["contract"], "fixture-span-v2")


if __name__ == "__main__":
    unittest.main()
