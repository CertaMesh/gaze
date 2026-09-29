"""The release table must come from matching measured layer scorecards."""

import json
import tempfile
import unittest
from pathlib import Path

import render_agentic_adjacency_doc as render


def scorecard(corpus_sha256: str = render.CORPUS_SHA256) -> dict:
    return {
        "measured": "v0.15.1",
        "binary_sha256": "a" * 64,
        "binary_commit": {"revision": "b" * 40, "dirty": False},
        "parameters": {
            "policy_sha256": render.POLICY_SHA256,
            "manifest_replacing_actions": ["tokenize", "redact"],
            "split_composite_source_ids": False,
        },
        "layers": {
            "generator": {"generator_version": render.GENERATOR_VERSION, "corpus_sha256": corpus_sha256},
            "scored_label_contract": {"file_sha256": render.CONTRACT_SHA256},
            **{
                layer: {"runs": [{
                    "config": "policy-file",
                    "metrics": {"utf8_bytes": {"pii": 100, "leaked": 20, "false_positive": 3}},
                    "pipeline_availability": {"failed_closed_documents": 0},
                }]}
                for layer in ("A", "D", "R")
            },
        },
    }


class AdjacencyHistoryTests(unittest.TestCase):
    def test_record_extracts_measured_values_and_rejects_an_old_corpus(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / "scorecard.json"
            path.write_text(json.dumps(scorecard()), encoding="utf-8")
            (row,) = render.rows_from_scorecard(path)
            self.assertEqual(row["layers"]["A"], {"gold": 100, "leaked": 20, "false_positive": 3})
            self.assertEqual(row["scorecard_sha256"], render.sha256(path))
            path.write_text(json.dumps(scorecard("c" * 64)), encoding="utf-8")
            with self.assertRaisesRegex(render.HistoryError, "pinned v4 test corpus"):
                render.rows_from_scorecard(path)

    def test_render_is_idempotent_and_rejects_missing_rows(self) -> None:
        rows = [
            {
                "version": version, "arm": arm, "binary_sha256": "a" * 64,
                "scorecard_sha256": "b" * 64,
                "layers": {layer: {"gold": 100, "leaked": 20, "false_positive": 3}
                           for layer in ("A", "D", "R")},
            }
            for version, arm in render.EXPECTED_ROWS
        ]
        history = {
            "schema_version": 1, "generator_version": render.GENERATOR_VERSION,
            "corpus_sha256": render.CORPUS_SHA256,
            "policy_sha256": render.POLICY_SHA256,
            "contract_sha256": render.CONTRACT_SHA256,
            "rows": rows,
        }
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / "history.json"
            path.write_text(json.dumps(history), encoding="utf-8")
            loaded = render.load_history(path)
            doc = render.BEGIN + "\n\n" + render.END
            result = render.apply(doc, loaded)
            self.assertEqual(result, render.apply(result, loaded))
            self.assertIn("20 / 100", result)
            history["rows"] = rows[:-1]
            path.write_text(json.dumps(history), encoding="utf-8")
            with self.assertRaisesRegex(render.HistoryError, "exactly the displayed release arms"):
                render.load_history(path)


if __name__ == "__main__":
    unittest.main()
