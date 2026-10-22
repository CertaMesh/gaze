"""Reusable benchmark captures need evidence bound to their exact artifacts."""

import copy
import json
from pathlib import Path
import tempfile
import unittest
from unittest import mock

import agentic_layers as agentic
import benchmark_cache as cache
import mechanism_arms as arms
import stage_timing as stages
from test_agentic_layers import _scorecard


class CaptureExecutionTests(unittest.TestCase):
    def capture(self, directory, *, leaked=10, status="miss"):
        directory.mkdir(parents=True)
        record = directory / "observations-v1.jsonl.gz"
        card_path = directory / "scorecard-v4.json"
        evidence_path = directory / "run-evidence.json"
        record.write_bytes(b"synthetic observation")
        card = _scorecard({layer: leaked for layer in "CADR"})
        card["capture_cache"] = cache.provenance({"fixture": "synthetic"})
        card["cache_replay"] = False
        card["observation_record"] = {"file": record.name, "sha256": stages.file_sha256(record)}
        with stages.session(evidence_path, scorecard=card_path, record=record) as evidence:
            evidence.cache = {"status": status, "key_sha256": card["capture_cache"]["key_sha256"]}
            card_path.write_text(json.dumps(card) + "\n")
        return card, card_path, record, evidence_path

    def test_completed_fresh_and_hit_captures_are_admitted(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            base, base_path, base_record, _ = self.capture(root / "base")
            candidate, candidate_path, candidate_record, _ = self.capture(root / "candidate", leaked=9, status="hit")
            self.assertEqual(arms._load_arm(root / "base"), (base, base_record))
            self.assertEqual(arms._load_arm(root / "candidate"), (candidate, candidate_record))
            # This fixture binds arbitrary artifact bytes, not a valid observation stream.
            # Byte-differential parsing has its own real-record coverage.
            with mock.patch('leak_diff.report', return_value={'report_only': True}) as differential:
                result = agentic.gate(base, candidate, scorecard_paths=(base_path, candidate_path))
            differential.assert_called_once_with(base_record, candidate_record, config='policy-file')
            self.assertEqual(result["verdict"], "pass")
            with self.assertRaisesRegex(agentic.LayerError, "execution evidence"):
                agentic.gate(base, candidate)

    def test_missing_malformed_failed_stale_and_swapped_evidence_is_rejected(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            base, base_path, _, sidecar = self.capture(root / "base")
            candidate, candidate_path, _, other = self.capture(root / "candidate", leaked=9)
            original = sidecar.read_bytes()
            evidence = json.loads(original)
            mutations = {
                "missing": None,
                "malformed": b"{",
                "wrong type": b"[]",
                "swapped": other.read_bytes(),
            }
            for field, value in (("outcome", "failed"), ("schema_version", 1), ("artifacts", None), ("stages", None)):
                mutations[field] = json.dumps({**evidence, field: value}).encode()
            for field in ("scorecard", "record"):
                changed = copy.deepcopy(evidence)
                changed["artifacts"][field]["sha256"] = "0" * 64
                mutations[f"stale {field}"] = json.dumps(changed).encode()
            for field, value in (("status", "running"), ("key_sha256", "0" * 64)):
                mutations[f"cache {field}"] = json.dumps({**evidence, "cache": {**evidence["cache"], field: value}}).encode()
            for name, content in mutations.items():
                with self.subTest(case=name):
                    if content is None:
                        sidecar.unlink()
                    else:
                        sidecar.write_bytes(content)
                    with self.assertRaises(arms.MechanismError):
                        arms._load_arm(base_path.parent)
                    with self.assertRaises(agentic.LayerError):
                        agentic.gate(base, candidate, scorecard_paths=(base_path, candidate_path))
                    sidecar.write_bytes(original)

    def test_artifact_changes_and_admitted_mapping_changes_are_rejected(self):
        with tempfile.TemporaryDirectory() as temporary:
            card, path, record, _ = self.capture(Path(temporary) / "base")
            original = path.read_bytes()
            path.write_bytes(original + b" ")
            with self.assertRaisesRegex(ValueError, "artifact hash"):
                stages.validate_execution(card, path)
            path.write_bytes(original)
            changed = {**card, "cache_replay": True}
            with self.assertRaisesRegex(ValueError, "differs"):
                stages.validate_execution(changed, path)
            record.write_bytes(b"synthetic changed observation")
            with self.assertRaisesRegex(ValueError, "artifact hash"):
                stages.validate_execution(card, path)

    def test_failed_session_never_produces_admissible_evidence(self):
        with tempfile.TemporaryDirectory() as temporary:
            card, path, record, sidecar = self.capture(Path(temporary) / "base")
            with self.assertRaisesRegex(RuntimeError, "synthetic"):
                with stages.session(sidecar, scorecard=path, record=record):
                    raise RuntimeError("synthetic failure")
            self.assertEqual(json.loads(sidecar.read_text())["outcome"], "failed")
            with self.assertRaisesRegex(ValueError, "incomplete"):
                stages.validate_execution(card, path)
