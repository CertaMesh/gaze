"""Published per-label and layer aggregates stay tied to their scorecards."""

import gzip
import hashlib
import json
import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import render_benchmark_doc as render


ROOT = Path(__file__).resolve().parents[2] / "docs/reference/benchmarks"


class ReleaseHistoryAggregateTests(unittest.TestCase):
    def test_each_contract_and_layer_matches_its_pinned_evidence(self):
        history = json.loads((ROOT / "release-history.json").read_text())
        for release in history["releases"]:
            for row in (release, *release.get("contract_results", [])):
                card = json.loads((ROOT / row["scorecard"]).read_text())
                for config, arm in row["arms"].items():
                    run = next(run for run in card["runs"] if run["config"] == config)
                    self.assertEqual(arm["per_label_recall"], run["per_label_recall"])
                layers = row["agentic_layers"]
                artifact_path = ROOT / layers["source_file"]
                self.assertEqual(
                    hashlib.sha256(artifact_path.read_bytes()).hexdigest(),
                    layers["source_sha256"],
                )
                with gzip.open(artifact_path, "rt", encoding="utf-8") as stream:
                    artifact = json.load(stream)
                for key, value in render.layer_history_aggregates(artifact).items():
                    self.assertEqual(layers[key], value)


if __name__ == "__main__":
    unittest.main()
