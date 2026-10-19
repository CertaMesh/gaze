"""Published per-label and layer aggregates stay tied to their scorecards."""

import gzip
import hashlib
import json
import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import bench_json
import render_benchmark_doc as render
import render_agentic_adjacency_doc as adjacency


ROOT = Path(__file__).resolve().parents[2] / "docs/reference/benchmarks"


class ReleaseHistoryAggregateTests(unittest.TestCase):
    def test_full_layer_history_keeps_its_replay_archive_receipt(self):
        card = bench_json.load(ROOT / 'scorecard-v0.17.0.json.gz')
        aggregate = render.layer_history_aggregates(card)
        self.assertEqual(aggregate['observation_record'], card['observation_record'])
        aggregate['observation_record']['sha256'] = '0' * 64
        self.assertNotEqual(aggregate['observation_record'], card['observation_record'])

    def test_each_contract_and_layer_matches_its_pinned_evidence(self):
        history = json.loads((ROOT / "release-history.json").read_text())
        for release in history["releases"]:
            rows = (release, *release.get("contract_results", []))
            # Generated layers are scored under their own layer contract, not the
            # corpus contract, so a release records them once or more; a v3
            # result re-scored from a C-only record carries none.
            if not any("agentic_layers" in row for row in rows):
                measured = adjacency.load_history(adjacency.HISTORY)
                unavailable = [row for row in measured['rows'] if row['version'] == release['version']]
                self.assertTrue(unavailable, release['version'])
                self.assertTrue(all(row.get('unavailable') is True for row in unavailable))
                self.assertTrue(all(row['binary_revision'] == release['commit'] for row in unavailable))
            for row in rows:
                card_path = ROOT / row["scorecard"]
                self.assertEqual(bench_json.content_sha256(card_path), row['scorecard_sha256'])
                card = bench_json.load(card_path)
                for config, arm in row["arms"].items():
                    run = next(run for run in card["runs"] if run["config"] == config)
                    self.assertEqual(arm["per_label_recall"], run["per_label_recall"])
                if "agentic_layers" not in row:
                    continue
                layers = row["agentic_layers"]
                if "source_file" not in layers:
                    if all(layer in card.get('layers', {}) for layer in ('A', 'D', 'R')):
                        self.assertEqual(layers, render.layer_history_aggregates(card))
                        continue
                    source_backed = [
                        candidate["agentic_layers"]
                        for candidate in rows
                        if "source_file" in candidate.get("agentic_layers", {})
                    ]
                    self.assertTrue(
                        any(
                            all(layers[key] == candidate[key] for key in layers)
                            for candidate in source_backed
                        ),
                        release["version"],
                    )
                    continue
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
