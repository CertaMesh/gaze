"""Supplemental quality evidence remains value-free and replayable."""

import json
import tempfile
import unittest
from pathlib import Path

import safety_net_bench_lib as lib


class SafetyNetEvidenceTest(unittest.TestCase):
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
