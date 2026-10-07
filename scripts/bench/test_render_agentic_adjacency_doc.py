"""The release table must come from matching measured layer scorecards."""

import json
import tempfile
import unittest
from pathlib import Path
from unittest import mock

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
    def test_incomplete_recording_preserves_the_existing_ledger(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            bench = root / 'docs/reference/benchmarks'
            bench.mkdir(parents=True)
            contract = render.agentic_layers.HISTORICAL_CONTRACTS.get(
                render.GENERATOR_VERSION, render.agentic_layers.SCORED_LABELS_PATH)
            frozen = render.ROOT / contract
            (bench / frozen.name).write_bytes(frozen.read_bytes())
            history = bench / render.HISTORY.name
            original = render.HISTORY.read_bytes()
            history.write_bytes(original)
            path = root / 'scorecard.json'
            path.write_text(json.dumps(scorecard()), encoding='utf-8')
            with mock.patch.object(render, 'ROOT', root), mock.patch('sys.argv', [render.__file__, '--record', str(path)]):
                with self.assertRaises(SystemExit) as caught:
                    render.main()
            self.assertEqual(caught.exception.code, 2)
            self.assertEqual(history.read_bytes(), original)

    def test_current_inputs_cannot_relabel_the_frozen_ledger(self) -> None:
        current = render.HistoryInputs.for_version(11)
        frozen = render.HistoryInputs.for_version(10)
        with self.assertRaisesRegex(render.HistoryError, 'generator version differs'):
            render.load_history(frozen.path, current)
        self.assertEqual(current.path.name, 'agentic-adjacency-v11-history.json')
        self.assertNotEqual(current.corpus_sha256, frozen.corpus_sha256)
        self.assertNotEqual(current.contract_sha256, frozen.contract_sha256)

    def test_current_scorecard_requires_its_own_corpus_and_contract(self) -> None:
        current = render.HistoryInputs.for_version(11)
        frozen = render.HistoryInputs.for_version(10)
        value = scorecard(current.corpus_sha256)
        value['layers']['generator']['generator_version'] = current.version
        value['layers']['scored_label_contract']['file_sha256'] = current.contract_sha256
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / 'scorecard.json'
            path.write_text(json.dumps(value), encoding='utf-8')
            self.assertEqual(render.rows_from_scorecard(path, current)[0]['layers']['A']['leaked'], 20)
            for field, stale in (('corpus_sha256', frozen.corpus_sha256),
                                 ('generator_version', frozen.version)):
                changed = json.loads(json.dumps(value))
                changed['layers']['generator'][field] = stale
                path.write_text(json.dumps(changed), encoding='utf-8')
                with self.assertRaisesRegex(render.HistoryError, 'pinned v11 test corpus'):
                    render.rows_from_scorecard(path, current)
            value['layers']['scored_label_contract']['file_sha256'] = frozen.contract_sha256
            path.write_text(json.dumps(value), encoding='utf-8')
            with self.assertRaisesRegex(render.HistoryError, 'committed contract'):
                render.rows_from_scorecard(path, current)

    def test_historical_policy_identity_stays_bound_to_its_ledger(self) -> None:
        frozen = render.HistoryInputs.for_version(10)
        self.assertNotEqual(frozen.policy_sha256, render.MEASURED_INPUTS.policy_sha256)
        for version in (4, 8, 9, 10):
            inputs = render.HistoryInputs.for_version(version)
            with self.subTest(version=version):
                self.assertEqual(render.load_history(inputs.path, inputs)['policy_sha256'], inputs.policy_sha256)
        value = scorecard()
        value['parameters']['policy_sha256'] = frozen.policy_sha256
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / 'scorecard.json'
            path.write_text(json.dumps(value), encoding='utf-8')
            with self.assertRaisesRegex(render.HistoryError, 'setup policy'):
                render.rows_from_scorecard(path)

    def test_record_extracts_measured_values_and_rejects_an_old_corpus(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / "scorecard.json"
            path.write_text(json.dumps(scorecard()), encoding="utf-8")
            (row,) = render.rows_from_scorecard(path)
            self.assertEqual(row["layers"]["A"], {"gold": 100, "leaked": 20, "false_positive": 3})
            self.assertEqual(row["scorecard_sha256"], render.sha256(path))
            path.write_text(json.dumps(scorecard("c" * 64)), encoding="utf-8")
            with self.assertRaisesRegex(render.HistoryError, f"pinned v{render.GENERATOR_VERSION} test corpus"):
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
