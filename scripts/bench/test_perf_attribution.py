"""Model-free contract tests for attribution, not latency measurements."""
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

import perf_attribution as perf


class AttributionTests(unittest.TestCase):
    def test_nearest_rank_tails(self):
        self.assertEqual(perf.percentiles(list(range(1, 101))), {"p50": 50, "p95": 95, "p99": 99})

    def test_load_gate_and_dry_run_fail_closed(self):
        with patch.object(perf, "quiet_threshold", return_value=2.0):
            quiet = [{"load": [1.9, 0, 0]}]
            self.assertTrue(perf.publishable(quiet, dry=False, passes=2, warmups=1))
            for probes, dry, passes, warmups in [
                (quiet, True, 2, 1), ([{"load": [2.0, 0, 0]}], False, 2, 1),
                (quiet, False, 1, 1), (quiet, False, 2, 0),
            ]:
                self.assertFalse(perf.publishable(probes, dry=dry, passes=passes, warmups=warmups))

    def test_model_inventory_detects_changed_bytes_without_paths(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / "model.onnx").write_bytes(b"old")
            before = perf.model_inventory([root])
            (root / "model.onnx").write_bytes(b"new")
            self.assertNotEqual(before, perf.model_inventory([root]))
            self.assertNotIn(tmp, json.dumps(before))

    def test_vernio_policy_is_core_only_and_no_safety_net(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = perf.vernio_policy(Path(tmp) / "policy.toml", Path(tmp))
            config = perf.tomllib.loads(path.read_text())
            self.assertEqual(config["policy"]["rulepacks"]["bundled"], ["core"])
            self.assertEqual(config["locale"]["active"], ["de-DE", "en-US", "global"])
            self.assertEqual(config["safety_net"]["backend"], "none")
            self.assertTrue(config["address_blocks"]["enabled"])

    def test_daemon_protocol_old_and_instrumented_binaries(self):
        with tempfile.TemporaryDirectory() as tmp:
            binary = Path(tmp) / "fake-gaze"
            for timers in [False, True]:
                binary.write_text('#!/usr/bin/env python3\nimport sys,json\n'
                    'for line in sys.stdin:\n'
                    ' row=json.loads(line)\n'
                    + (' print("GAZE_PERF " + json.dumps({"schema":1,"metrics":{"rules_ns":{"ns":7}},"counters":{},"recognizers":{}}), file=sys.stderr, flush=True)\n' if timers else '')
                    + ' print(json.dumps({"clean_text":"safe","session_id":row["session_id"]}),flush=True)\n')
                binary.chmod(0o700)
                docs = [perf.Document(str(i), "private test input", "digest") for i in range(5)]
                rows = perf.run_pass(binary, Path(tmp) / "policy", docs, warmups=2,
                                     dry=True, timeout=5, per_file=False)
                self.assertEqual(len(rows), 5)
                self.assertTrue(all(row["e2e_ns"] is None for row in rows))
                self.assertEqual(rows[0]["trace"] is not None, timers)
                self.assertNotIn("private test input", json.dumps(rows))

    def test_identical_binaries_rejected_before_models_or_setup(self):
        with tempfile.TemporaryDirectory() as tmp:
            binary = Path(tmp) / "gaze"
            binary.write_bytes(b"same binary")
            with patch.object(perf, "commit", return_value="a" * 40):
                with self.assertRaisesRegex(ValueError, "identical binaries"):
                    perf.main(["--binary-a", str(binary), "--binary-b", str(binary),
                               "--common-policy", "unused", "--ner-model-dir", "unused", "--dry-run",
                               "--out", str(Path(tmp) / "out.json")])


class SummaryTests(unittest.TestCase):
    def test_stages_include_zero_work_documents_and_unknown_old_stages(self):
        def trace(metrics):
            return {"metrics": metrics, "recognizers": {}}
        rows = [{"id": "one", "e2e_ns": 100, "trace": trace({"rules_ns": {"ns": 40}})},
                {"id": "two", "e2e_ns": 200, "trace": trace({})}]
        result = perf.summary(rows)
        self.assertEqual(result["stages_ns"]["rules_ns"]["p50"], 0)
        self.assertEqual(result["per_document"]["two"]["stages_ns"]["rules_ns"]["p99"], 0)
        result = perf.summary([{**rows[0], "trace": None}])
        self.assertIsNone(result["stages_ns"])

    def test_each_one_shot_file_gets_its_own_record(self):
        with tempfile.TemporaryDirectory() as tmp:
            binary = Path(tmp) / "fake-gaze"
            binary.write_text('#!/usr/bin/env python3\nimport sys,json\nsys.stdin.read()\n'
                              'print(json.dumps({"clean_text":"safe"}))\n'
                              'print("GAZE_PERF " + json.dumps({"schema":1,"metrics":{},"counters":{},"recognizers":{}}),file=sys.stderr)\n')
            binary.chmod(0o700)
            rows = perf.run_pass(binary, Path(tmp) / "policy", [perf.Document("one", "text", "digest")],
                                 warmups=1, dry=True, timeout=5, per_file=True)
            self.assertEqual(len(rows), 1)
            self.assertEqual(rows[0]["trace"]["schema"], 1)
            self.assertIsNone(rows[0]["e2e_ns"])


class PrivacyTests(unittest.TestCase):
    def test_unknown_fields_and_raw_ids_are_rejected(self):
        base = {"schema": 1, "metrics": {}, "counters": {}, "recognizers": {}}
        for trace in [{**base, "text": "private"}, {**base, "recognizers": {"private": {"ns": 1}}},
                      {**base, "metrics": {"private": {"ns": 1}}},
                      {**base, "counters": {"input_bytes": "private"}}]:
            with self.assertRaisesRegex(ValueError, "contents suppressed"):
                perf.validate_trace(trace)
        self.assertEqual(perf.validate_trace(base), base)



class ReceiptTests(unittest.TestCase):
    def test_complete_receipt_alternates_passes_and_preserves_pairing(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            a, b = root / "a", root / "b"
            a.write_bytes(b"binary a")
            b.write_bytes(b"binary b")
            policy = root / "policy.toml"
            policy.write_text("# fixed policy")
            docs = [perf.Document(str(i), "never copied", "digest") for i in range(400)]
            def run(binary, policy, documents, **kwargs):
                return [{"id": d.id, "input_bytes": len(d.text), "e2e_ns": 100 if binary == a.resolve() else 120,
                         "trace": None} for d in documents]
            with patch.object(perf, "commit", return_value="a" * 40), \
                 patch.object(perf, "documents", return_value=("a" * 40, docs)), \
                 patch.object(perf, "model_inventory", return_value=[]), \
                 patch.object(perf, "policy_models", return_value=[]), \
                 patch.object(perf, "host", return_value={"load": [0, 0, 0]}), \
                 patch.object(perf, "run_pass", side_effect=run):
                out = root / "receipt.json"
                perf.main(["--binary-a", str(a), "--binary-b", str(b),
                           "--common-policy", str(policy), "--shipped-policy-a", str(policy),
                           "--shipped-policy-b", str(policy), "--ner-model-dir", str(root),
                           "--measure", "--passes", "3", "--out", str(out)])
            receipt = json.loads(out.read_text())
            self.assertTrue(receipt["publishable"])
            self.assertNotIn("never copied", out.read_text())
            for arm in receipt["arms"].values():
                orders = [run["order"] for run in arm["runs"]]
                self.assertEqual(orders[0], orders[2])
                self.assertEqual(orders[0][::-1], orders[1])
                self.assertEqual(arm["paired_delta_ns_b_minus_a"]["0"]["p50"], 20)
                self.assertIsNone(arm["summary"]["a"]["stages_ns"])



class FrozenPolicyTests(unittest.TestCase):
    def test_external_unpinned_inputs_and_implicit_models_are_rejected(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "policy.toml"
            for content in ['[policy.rulepacks]\npaths=["outside.toml"]\n',
                            '[[detector]]\nterms_file="outside.txt"\n',
                            '[ner]\nthreshold=0.3\n',
                            '[safety_net]\nbackend="nym"\n']:
                path.write_text(content)
                with self.assertRaises(ValueError):
                    perf.policy_models(path)


if __name__ == "__main__":
    unittest.main()
