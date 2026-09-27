"""The latency script times exactly base plus the declared delta, nothing else."""

from __future__ import annotations

import tempfile
import tomllib
import unittest
from pathlib import Path

import mechanism_latency


class CandidatePolicyTest(unittest.TestCase):
    def setUp(self) -> None:
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        self.root = Path(tmp.name)
        self.base = self.root / "base.toml"
        self.base.write_text('[ner]\nthreshold = 0.3\n', encoding="utf-8")

    def test_candidate_is_base_plus_delta(self) -> None:
        delta = self.root / "delta.toml"
        delta.write_text("[dob_judge]\nenabled = true\n", encoding="utf-8")
        out = mechanism_latency.candidate_policy(self.base, delta, self.root / "candidate.toml")
        self.assertEqual(
            tomllib.loads(out.read_text(encoding="utf-8")),
            {"ner": {"threshold": 0.3}, "dob_judge": {"enabled": True}},
        )

    def test_delta_touching_a_base_section_is_refused(self) -> None:
        delta = self.root / "delta.toml"
        delta.write_text("[ner]\nthreshold = 0.9\n", encoding="utf-8")
        with self.assertRaisesRegex(SystemExit, "overlapping: \\['ner'\\]"):
            mechanism_latency.candidate_policy(self.base, delta, self.root / "candidate.toml")

    def test_empty_delta_is_refused(self) -> None:
        delta = self.root / "delta.toml"
        delta.write_text("# nothing\n", encoding="utf-8")
        with self.assertRaises(SystemExit):
            mechanism_latency.candidate_policy(self.base, delta, self.root / "candidate.toml")


if __name__ == "__main__":
    unittest.main()
