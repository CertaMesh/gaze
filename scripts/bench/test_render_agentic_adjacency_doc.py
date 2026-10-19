"""The release table must come from matching measured layer scorecards."""

import gzip
import json
import tempfile
import unittest
from pathlib import Path
from unittest import mock

import render_agentic_adjacency_doc as render


def failure_receipt(root: Path, inputs=render.MEASURED_INPUTS) -> Path:
    folder = root / 'docs/reference/benchmarks'
    folder.mkdir(parents=True, exist_ok=True)
    driver = root / 'scripts/bench/rescore_past_release.py'
    driver.parent.mkdir(parents=True, exist_ok=True)
    driver.write_bytes((render.ROOT / 'scripts/bench/rescore_past_release.py').read_bytes())
    value = {
        'schema_version': 1, 'kind': 'historical-producer-unavailable', 'version': 'v0.14.0',
        'binary_revision': '616e8a10b957961f9f7dcb745610afa978783c3a', 'binary_sha256': 'a' * 64,
        'harness_revision': 'b' * 40, 'driver_sha256': render.sha256(driver),
        'generator_version': inputs.version, 'corpus_sha256': inputs.corpus_sha256,
        'contract_sha256': inputs.contract_sha256,
        'configs': ['rule-floor-extended', 'pass2-ner', 'full-stack-kiji-resolve'],
        'manifest_replacing_actions': ['tokenize'],
        'attempts': [{'scored_label_contract': contract, 'exit_code': 1,
                      'error_phase': 'payload_processing', 'error_code': 'payload',
                      'log_sha256': 'c' * 64} for contract in (1, 2)],
    }
    path = folder / f'unavailable-v0.14.0-generated-v{inputs.version}.json'
    path.write_text(json.dumps(value))
    return path


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
                    "pipeline_availability": {"failed_closed_documents": 0, "completed_documents": 100},
                    "pipeline_contract": {"documents": 100, "restore_exact_documents": 99,
                                          "restore_success_decisions": 100, "manifest_valid_documents": 99},
                }]}
                for layer in ("A", "D", "R")
            },
        },
    }


class AdjacencyHistoryTests(unittest.TestCase):
    def test_unavailable_cells_require_receipts_and_never_infer_zero_counts(self):
        history = render.load_history(render.HISTORY)
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            path = failure_receipt(root)
            with mock.patch.object(render, 'ROOT', root):
                rows = render.rows_from_unavailable_receipt(path, render.MEASURED_INPUTS)
                history['rows'] = [row for row in history['rows'] if row['version'] != 'v0.14.0'] + rows
                render.validate_history(history)
                table = render.render(history)
                self.assertIn('`v0.14.0` `pass2-ner` | N/A | N/A | N/A | N/A | N/A |', table)
                self.assertIn('`v0.14.0` `pass2-ner` | N/A | N/A |', table)
                self.assertIn('payload_processing:payload', table)
                self.assertIn(path.name, table)
                self.assertNotIn('layers', rows[0])
                rows[0]['layers'] = {}
                with self.assertRaisesRegex(render.HistoryError, 'cannot contain measured metrics'):
                    render.validate_history(history)

    def test_unavailability_rejects_wrong_inputs_incomplete_attempts_and_raw_text(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            path = failure_receipt(root)
            original = json.loads(path.read_text())
            mutations = [
                lambda value: value.update(corpus_sha256='d' * 64),
                lambda value: value.update(driver_sha256='d' * 64),
                lambda value: value.update(version='v0.16.0'),
                lambda value: value.update(schema_version=True),
                lambda value: value.update(text='synthetic fixture text'),
                lambda value: value['attempts'].pop(),
                lambda value: value['attempts'][0].update(exit_code=0),
            ]
            with mock.patch.object(render, 'ROOT', root):
                for mutate in mutations:
                    value = json.loads(json.dumps(original))
                    mutate(value)
                    path.write_text(json.dumps(value))
                    with self.subTest(value=value), self.assertRaises(render.HistoryError):
                        render.rows_from_unavailable_receipt(path, render.MEASURED_INPUTS)

    def test_unavailability_digest_source_and_file_boundary_cannot_be_bypassed(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            path = failure_receipt(root)
            with mock.patch.object(render, 'ROOT', root):
                row = render.rows_from_unavailable_receipt(path, render.MEASURED_INPUTS)[0]
                for field, value in [('evidence_sha256', 'd' * 64), ('version', 'v0.16.0'),
                                     ('binary_revision', 'd' * 40), ('evidence_file', '../receipt.json')]:
                    changed = {**row, field: value}
                    with self.subTest(field=field), self.assertRaises(render.HistoryError):
                        render.validate_unavailable_row(changed, render.MEASURED_INPUTS)
                outside = root / 'outside.json'
                path.rename(outside)
                path.symlink_to(outside)
                with self.assertRaisesRegex(render.HistoryError, 'regular committed'):
                    render.validate_unavailable_row(row, render.MEASURED_INPUTS)

    def test_v16_records_restore_and_manifest_counts_without_hiding_legacy_errors(self):
        inputs = render.HistoryInputs.for_version(16)
        value = scorecard(inputs.corpus_sha256)
        value["layers"]["generator"]["generator_version"] = 16
        value["layers"]["scored_label_contract"]["file_sha256"] = inputs.contract_sha256
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / "scorecard.json"
            path.write_text(json.dumps(value))
            row, = render.rows_from_scorecard(path, inputs)
            for layer in ("A", "D", "R"):
                self.assertEqual(row["layers"][layer]["restore_exact"], 99)
                self.assertEqual(row["layers"][layer]["valid_manifests"], 99)
                self.assertEqual(row["layers"][layer]["restore_failures"], 1)
                self.assertEqual(row["layers"][layer]["invalid_manifests"], 1)
            for bad in (None, True, -1, 101):
                value["layers"]["A"]["runs"][0]["pipeline_contract"]["restore_exact_documents"] = bad
                path.write_text(json.dumps(value))
                with self.assertRaises(render.HistoryError):
                    render.rows_from_scorecard(path, inputs)

    def test_compressed_scorecard_digest_matches_the_committed_file(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / 'scorecard.json.gz'
            path.write_bytes(gzip.compress(json.dumps(scorecard()).encode(), mtime=0))
            row, = render.rows_from_scorecard(path)
            self.assertEqual(row['scorecard_sha256'], render.sha256(path))
            self.assertEqual(row['layers']['A']['leaked'], 20)

    def test_new_history_preserves_each_releases_own_policy(self) -> None:
        current = render.HistoryInputs.for_version(13)
        value = scorecard(current.corpus_sha256)
        value['measured'] = 'v0.16.0'
        value['layers']['generator']['generator_version'] = 13
        value['layers']['scored_label_contract']['file_sha256'] = current.contract_sha256
        value['parameters']['policy_sha256'] = render.V16_POLICY_SHA256
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / 'scorecard.json'
            path.write_text(json.dumps(value))
            row, = render.rows_from_scorecard(path, current)
            self.assertEqual(row['policy_sha256'], render.V16_POLICY_SHA256)
            self.assertEqual(row['refused'], {'A': 0, 'D': 0, 'R': 0})
            value['parameters']['policy_sha256'] = current.policy_sha256
            path.write_text(json.dumps(value))
            with self.assertRaisesRegex(render.HistoryError, 'setup policy'):
                render.rows_from_scorecard(path, current)
        self.assertEqual(render.expected_rows(render.HistoryInputs.for_version(12)), render.LEGACY_ROWS)

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
        current = render.HistoryInputs.for_version(12)
        frozen = render.HistoryInputs.for_version(10)
        with self.assertRaisesRegex(render.HistoryError, 'generator version differs'):
            render.load_history(frozen.path, current)
        self.assertEqual(current.path.name, 'agentic-adjacency-v12-history.json')
        self.assertNotEqual(current.corpus_sha256, frozen.corpus_sha256)
        self.assertNotEqual(current.contract_sha256, frozen.contract_sha256)

    def test_current_scorecard_requires_its_own_corpus_and_contract(self) -> None:
        current = render.HistoryInputs.for_version(12)
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
                with self.assertRaisesRegex(render.HistoryError, 'pinned v12 test corpus'):
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
            self.assertEqual(row["layers"]["A"], {"gold": 100, "leaked": 20, "false_positive": 3,
                "documents": 100, "restore_exact": 99, "restore_failures": 1,
                "valid_manifests": 99, "invalid_manifests": 1})
            self.assertEqual(row["scorecard_sha256"], render.sha256(path))
            path.write_text(json.dumps(scorecard("c" * 64)), encoding="utf-8")
            with self.assertRaisesRegex(render.HistoryError, f"pinned v{render.GENERATOR_VERSION} test corpus"):
                render.rows_from_scorecard(path)

    def test_render_is_idempotent_and_rejects_missing_rows(self) -> None:
        rows = [
            {
                "version": version, "arm": arm, "binary_sha256": "a" * 64,
                "scorecard_sha256": "b" * 64,
                "policy_sha256": render.release_policy_sha256(version, render.MEASURED_INPUTS),
                "refused": {layer: 0 for layer in ("A", "D", "R")},
                "layers": {layer: {"gold": 100, "leaked": 20, "false_positive": 3,
                    "documents": 100, "restore_exact": 99, "restore_failures": 1,
                    "valid_manifests": 99, "invalid_manifests": 1}
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
