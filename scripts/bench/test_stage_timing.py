"""Profiling reports metadata without changing producer responses or retaining diagnostics."""

import json
import os
from pathlib import Path
import sys
import tempfile
import unittest
from unittest import mock

import bench_subprocess as transport
import stage_timing as stages


PROFILE = {name: {"wall_seconds": 0.1, "cpu_seconds": 0.2, "samples": 1}
           for name in stages.NATIVE_STAGES}


class StageTimingTests(unittest.TestCase):
    def test_native_phase_metadata_arrives_in_sidecar(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            child = root / "producer.py"
            child.write_text(
                "import sys,json\n"
                "for line in sys.stdin:\n print(json.dumps({'ok':True}),flush=True)\n"
                f"print('GAZE_BENCH_PROFILE '+json.dumps({PROFILE!r}),file=sys.stderr,flush=True)\n"
            )
            evidence_path = root / "run-evidence.json"
            with stages.session(evidence_path), stages.labels(layer="C"):
                with transport.BenchSubprocess(
                    [sys.executable, str(child)],
                    env={**os.environ, "GAZE_BENCH_PROFILE": "1"},
                ) as process:
                    self.assertEqual(process.exchange({"id": "synthetic"}), {"ok": True})
            evidence = json.loads(evidence_path.read_text())
            self.assertEqual(len(evidence["stages"]), 3)
            self.assertEqual({row["stage"] for row in evidence["stages"]}, stages.NATIVE_STAGES)
            self.assertTrue(all(row["layer"] == "C" for row in evidence["stages"]))
            self.assertTrue(all(row["cpu_seconds"] == 0.2 for row in evidence["stages"]))

    def test_transport_time_is_excluded_from_capture_scoring(self):
        evidence = stages.Evidence()
        token = stages.CURRENT.set(evidence)
        self.addCleanup(stages.CURRENT.reset, token)
        with mock.patch.object(stages, "clock", side_effect=[
            (0.0, 0.0, 0.0), (1.0, 2.0, 0.0),
            (6.0, 3.0, 0.0), (10.0, 6.0, 0.0),
        ]):
            scoring = stages.ScoringClock(True)
            self.assertEqual(scoring.excluding(lambda: 7)(), 7)
            scoring.finish()
        self.assertEqual(evidence.events[0]["wall_seconds"], 5.0)
        self.assertEqual(evidence.events[0]["cpu_self_seconds"], 5.0)

    def test_arbitrary_diagnostics_are_discarded_and_profile_is_bounded(self):
        reader = stages.NativeProfileReader()
        reader.feed(b"synthetic-private-canary" * 1000 + b"\n")
        self.assertEqual(reader.pending, b"")
        raw = stages.NATIVE_PREFIX + json.dumps(PROFILE).encode() + b"\n"
        for byte in raw:
            reader.feed(bytes([byte]))
        self.assertEqual(reader.result, PROFILE)
        with self.assertRaises(ValueError):
            reader.feed(raw)

    def test_unknown_native_fields_never_enter_evidence(self):
        for value in ({"synthetic-private-canary": PROFILE},
                      {name: {**row, "payload": "synthetic-private-canary"}
                       for name, row in PROFILE.items()}):
            with self.assertRaises(ValueError) as error:
                stages.validate_native(value)
            self.assertNotIn("synthetic-private-canary", str(error.exception))

    def test_failed_stage_still_writes_closed_execution_outcome(self):
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / "evidence.json"
            with self.assertRaisesRegex(RuntimeError, "synthetic"):
                with stages.session(path), stages.measure("build"):
                    raise RuntimeError("synthetic")
            evidence = json.loads(path.read_text())
            self.assertEqual(evidence["outcome"], "failed")
            self.assertEqual(evidence["stages"][0]["stage"], "build")
            self.assertGreaterEqual(evidence["stages"][0]["cpu_self_seconds"], 0)


if __name__ == "__main__":
    unittest.main()
