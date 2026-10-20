"""The external release projection must never turn byte scores into character scores."""
import copy
import hashlib
import json
from pathlib import Path
import shutil
import tempfile
import unittest
import xml.etree.ElementTree as ET

import external_release_panels as external
from tagged_gaze import check_public

REPO = Path(__file__).resolve().parents[2]
DATA = REPO / "docs/reference/benchmarks"


class ExternalReleaseTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.review = external.load(DATA)

    def test_fresh_versions_and_separate_metric_are_visible(self):
        body = external.markdown(self.review, root=False)
        check_public(body, "external release")
        for title in external.TITLES.values():
            self.assertIn(f"| {title} | v0.17.0 |", body)
            self.assertIn(f"| {title} | v0.16.0 |", body)
        self.assertIn("64.2375059624 | 100,557 | 49,124 | PASS | PASS", body)
        self.assertIn("76.2383386217 | 15,455 | 710 | FAIL | PASS", body)
        self.assertIn("not_comparable", body)
        self.assertIn("zero-leak readiness remains unmet", body)

    def test_svg_values_and_metric_match_receipts_in_both_themes(self):
        for svg in external.files(self.review).values():
            root = ET.fromstring(svg)
            text = " ".join(root.itertext())
            self.assertIn(external.METRIC, text)
            self.assertNotIn("Character-level F2", text)
            self.assertIn("0.64238", text)
            self.assertIn("100,557", text)
            self.assertIn("Original net-byte gate: FAIL", text)

    def test_root_explains_metric_and_limits(self):
        body = external.markdown(self.review, root=True)
        self.assertIn("byte-micro F2", body)
        self.assertIn("docs/assets/benchmarks/external-release-panels", body)
        self.assertIn("TRACE's original net-byte gate fails", body)

    def test_digest_and_recomputed_arithmetic_reject_tampering(self):
        cases = [(False, "f2", "digest differs"), (True, "f2", "does not reproduce"),
                 (True, "formula", "scoring contract"), (True, "pass", "do not all pass"),
                 (True, "legacy", "original net-byte verdict differs")]
        for repin, mutation, message in cases:
            with self.subTest(repin=repin, mutation=mutation), tempfile.TemporaryDirectory() as tmp:
                target = Path(tmp)
                shutil.copytree(DATA / "release-v0.17.0", target / "release-v0.17.0")
                receipt_name = "release-measurement-receipts-v0.17.0.json"
                shutil.copyfile(DATA / receipt_name, target / receipt_name)
                path = target / "release-v0.17.0/external-f2-release-review.json"
                changed = copy.deepcopy(self.review)
                if mutation == "f2":
                    changed["results"]["piibench-commercial"]["candidate"]["f2_percent"] = 99.0
                elif mutation == "formula":
                    changed["formula"] = "character_f2"
                elif mutation == "pass":
                    changed["all_benchmarks_pass"] = False
                else:
                    changed["results"]["pii-trace"]["legacy_gate"]["verdict"] = "pass"
                path.write_text(json.dumps(changed))
                if repin:
                    receipt_path = target / receipt_name
                    receipt = json.loads(receipt_path.read_text())
                    receipt["versions"][external.VERSION]["aggregate_evidence"]["external-f2-release-review.json"]["sha256"] = hashlib.sha256(path.read_bytes()).hexdigest()
                    receipt_path.write_text(json.dumps(receipt))
                with self.assertRaisesRegex(ValueError, message):
                    external.load(target, repo=REPO)


if __name__ == "__main__":
    unittest.main()
