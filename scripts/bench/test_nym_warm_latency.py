#!/usr/bin/env python3
"""Model-free tests for scripts/bench/nym-warm-latency.py."""

from __future__ import annotations

import importlib.util
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[1]
_SPEC = importlib.util.spec_from_file_location("nym_warm_latency", HERE / "nym-warm-latency.py")
assert _SPEC is not None and _SPEC.loader is not None
latency = importlib.util.module_from_spec(_SPEC)
_SPEC.loader.exec_module(latency)


class LatencySummaryTest(unittest.TestCase):
    def test_percentiles_and_mean(self) -> None:
        # Shuffled on purpose: the summary must sort.
        samples = [float(value) for value in [7, 3, 20, 1, 15, 9, 2, 18, 11, 5, 13, 4, 19, 6, 17, 8, 12, 10, 16, 14]]
        self.assertEqual(
            latency.latency_summary(samples),
            {"warm_p50_ms": 10.5, "warm_p95_ms": 19.0, "warm_mean_ms": 10.5, "samples": 20},
        )

    def test_p95_is_nearest_rank(self) -> None:
        # 100 samples 1..100: nearest-rank p95 is the 95th value, never the max or an
        # interpolated one.
        summary = latency.latency_summary([float(value) for value in range(100, 0, -1)])
        self.assertEqual(summary["warm_p95_ms"], 95.0)
        self.assertEqual(summary["warm_p50_ms"], 50.5)
        # 10 samples: nearest rank is ceil(9.5) = the 10th value; a floor-index p95
        # (int((n - 1) * 0.95), as in ner-warm-latency.py) would report the 9th.
        self.assertEqual(latency.latency_summary([float(value) for value in range(1, 11)])["warm_p95_ms"], 10.0)

    def test_single_sample(self) -> None:
        self.assertEqual(
            latency.latency_summary([4.25]),
            {"warm_p50_ms": 4.25, "warm_p95_ms": 4.25, "warm_mean_ms": 4.25, "samples": 1},
        )

    def test_no_samples_is_an_error(self) -> None:
        with self.assertRaises(ValueError):
            latency.latency_summary([])


class HostInfoTest(unittest.TestCase):
    def test_ort_version_and_bundle_sha_come_from_the_repo(self) -> None:
        lock = (REPO / "Cargo.lock").read_text(encoding="utf-8")
        self.assertRegex(latency.ort_version(lock), r"^\d+\.\d+\.\d+")
        artifacts = (REPO / "crates/gaze-recognizers/src/safety_net/nym/artifacts.rs").read_text(encoding="utf-8")
        self.assertRegex(latency.bundle_sha(artifacts), r"^[0-9a-f]{64}$")
        with self.assertRaises(ValueError):
            latency.ort_version('name = "orts"\nversion = "1.0.0"')

    def test_hardware_line_names_every_field(self) -> None:
        line = latency.hardware_line(
            {
                "chip": "Chip X",
                "cores": "8",
                "ram_gib": "16",
                "os": "OS 1",
                "ort": "2.0.0-rc.12",
                "bundle_sha": "a" * 64,
                "intra_threads": "1",
            }
        )
        for part in ("Chip X", "8 cores", "16 GiB RAM", "OS 1", "ort 2.0.0-rc.12", "aaaaaaaaaaaa", "intra-op threads 1"):
            self.assertIn(part, line)


class SyntheticDocumentTest(unittest.TestCase):
    def test_timing_is_valid_only_on_a_quiet_host(self) -> None:
        self.assertTrue(latency.timing_validity(0.4, 1.9)["timing_valid"])
        for start, end in ((2.0, 0.1), (0.1, 2.0), (15.0, 17.3)):
            validity = latency.timing_validity(start, end)
            self.assertFalse(validity["timing_valid"])
            self.assertTrue(str(validity["timing_note"]).startswith("timing invalid"))

    def test_every_nym_configuration_is_an_arm(self) -> None:
        self.assertEqual(
            latency.ARMS,
            ("pass2-ner", "full-stack-nym-resolve", "single-pass-nym", "single-pass-nym-observed"),
        )

    def test_word_counts_are_exact_and_stable(self) -> None:
        for words, _pieces in latency.SYNTHETIC_DOCUMENTS.values():
            text = latency.synthetic_text(words)
            self.assertEqual(len(text.split()), words)
            self.assertEqual(text, latency.synthetic_text(words))
            self.assertEqual(text, text.strip())

    def test_piece_counts_are_verified_unless_skipped(self) -> None:
        self.assertFalse(latency.parse_args([]).skip_verify_pieces)
        self.assertTrue(latency.parse_args(["--skip-verify-pieces"]).skip_verify_pieces)


if __name__ == "__main__":
    unittest.main()
