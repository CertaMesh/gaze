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

    def test_explicit_name_arm_is_validated_against_the_same_delta_as_the_gate(self) -> None:
        self.base.write_text('[safety_net]\nbackend="nym"\n[safety_net.nym]\n'
                             'model_dir="~/models/nym"\nlabels=["USERNAME"]\n'
                             'thresholds={USERNAME=0.5}\n', encoding="utf-8")
        candidate = self.root / "explicit-candidate.toml"
        candidate.write_text(self.base.read_text().replace('labels=["USERNAME"]',
                             'labels=["USERNAME", "GIVEN_NAME", "SURNAME"]').replace(
                             'thresholds={USERNAME=0.5}',
                             'thresholds={USERNAME=0.5, GIVEN_NAME=0.5, SURNAME=0.5}'))
        delta = self.root / "delta.toml"
        delta.write_text('[mechanism_delta]\nappend_nym_labels={GIVEN_NAME=0.5, SURNAME=0.5}\n')
        out = mechanism_latency.candidate_policy(self.base, delta, self.root / "out.toml",
                                                 candidate=candidate)
        self.assertEqual(out.read_text(), candidate.read_text())
        candidate.write_text(candidate.read_text().replace('USERNAME=0.5', 'USERNAME=0.9'))
        with self.assertRaisesRegex(SystemExit, "beyond the declared Nym-label append"):
            mechanism_latency.candidate_policy(self.base, delta, self.root / "out.toml",
                                               candidate=candidate)

    def test_explicit_candidate_cannot_bypass_a_generic_or_malformed_delta(self) -> None:
        delta = self.root / "delta.toml"
        for text in ('[ner]\nthreshold=0.9\n', 'mechanism_delta=1\n',
                     '[mechanism_delta]\nappend_nym_labels={GIVEN_NAME=0.5}\n'):
            with self.subTest(delta=text):
                delta.write_text(text)
                with self.assertRaises(SystemExit):
                    mechanism_latency.candidate_policy(self.base, delta, self.root / "out.toml",
                                                       candidate=self.base)
        with self.assertRaisesRegex(SystemExit, "requires --candidate-policy"):
            mechanism_latency.candidate_policy(self.base, delta, self.root / "out.toml")

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
