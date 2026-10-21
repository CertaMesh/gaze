"""README publication uses byte counts and preserves measurement limits."""
import copy
import hashlib
import json
from pathlib import Path
import tempfile
import unittest
from unittest import mock
import xml.etree.ElementTree as ET

from gaze_bench_score import SCORED_LABEL_CONTRACT_V1
import readme_byte_panels as panels
import readme_byte_measurements as measurements


def counts():
    return {"documents": 3, "processed": 3, "refused": 0, "gold_bytes": 20,
            "leaked_bytes": 4, "false_positive_bytes": 2, "restore_exact": 3, "manifest_valid": 3}


def measured():
    return {kind: {version: {"totals": measurements.totals({"test": counts()}), "receipt": "receipt.json"}
                   for version in panels.VERSIONS} for kind in panels.DATASETS}


def competitor_inputs():
    block = {"true_positive_bytes": 16, "leaked_bytes": 4, "false_positive_bytes": 2,
             "char_level": {"f2": 0.1}}
    comparison = {"tools": {"measured": {"contracts": {"v4": {"C": {
        "metrics": {"product_coverage": {"full": block}}, "skipped_documents": 0}}}}}}
    their = {kind: {"rows": {"measured": {"product_coverage": block}},
                    "receipt_status": {"measured": "pre-receipt"}}
             for kind in ("presidio-research", "piibench-commercial")}
    their["pii-trace"] = {"rows": {"pii-tracer": {"product_coverage": block}},
                          "receipt_status": {"pii-tracer": "pre-receipt"}}
    return comparison, their, {"gliner": "measured", "opf": "unmeasured"}


class BytePanelTests(unittest.TestCase):
    def test_retained_baselines_cover_all_five_datasets_without_inference(self):
        directory = Path(__file__).resolve().parents[2] / "docs/reference/benchmarks"
        result = panels.baselines(directory)
        self.assertEqual(set(result), set(panels.DATASETS))
        self.assertEqual(result["own"]["totals"]["leaked_bytes"], 13291)
        self.assertEqual(result["presidio-research"]["totals"]["leaked_bytes"], 7984)
        self.assertEqual(result["sealed"]["totals"]["leaked_bytes"], 399158)
        self.assertEqual(result["piibench-commercial"]["totals"]["leaked_bytes"], 105077)
        self.assertEqual(result["pii-trace"]["totals"]["leaked_bytes"], 15739)
        for kind, receipt in result.items():
            self.assertEqual({name: row["documents"] for name, row in receipt["sets"].items()}, panels.POPULATIONS[kind])
            self.assertEqual(receipt["method"], "retained-byte-counts")
            self.assertEqual(receipt["source"]["sha256"], hashlib.sha256((directory / receipt["source"]["file"]).read_bytes()).hexdigest())

    def test_micro_f2_pools_counts_instead_of_averaging_documents(self):
        first = counts()
        second = {key: 0 for key in first}
        second.update(documents=1, processed=1, gold_bytes=100, leaked_bytes=100,
                      restore_exact=1, manifest_valid=1)
        total = measurements.totals({"first": first, "second": second})
        self.assertAlmostEqual(total["byte_micro_f2"], 80 / (80 + 416 + 2))

    def test_five_datasets_two_tags_one_metric_and_no_unmeasured_bar(self):
        comparison, their, declared = competitor_inputs()
        result = panels.panels(measured(), comparison, their, declared)
        self.assertEqual([p.title for p in result], list(panels.DATASETS.values()))
        for panel in result:
            self.assertEqual([b.name for b in panel.bars if b.gaze],
                             ["Gaze " + v for v in panels.VERSIONS])
            self.assertTrue(all(b.f2 is not None for b in panel.bars))
            self.assertNotIn("OPF", [b.name for b in panel.bars])
        for svg in panels.files(result).values():
            text = " ".join(ET.fromstring(svg).itertext())
            self.assertEqual(text.count(panels.METRIC), 5)
            self.assertNotIn("Character-level", text)
            self.assertNotIn("N/A", text)
            self.assertIn("†", text)
            self.assertIn("0.816†", text)

    def test_competitor_conversion_keeps_the_pre_receipt_marker(self):
        comparison, their, declared = competitor_inputs()
        result = panels.panels(measured(), comparison, their, declared)
        bar = result[2].bars[-1]
        self.assertTrue(bar.receipt_unverified)
        self.assertAlmostEqual(bar.f2, 80 / 98)
        self.assertNotEqual(bar.f2, 0.1)
        markdown = panels.markdown(measured(), comparison, their, declared)
        self.assertIn("Refused", markdown)
        self.assertIn("Leaked on all processed", markdown)
        self.assertIn("GLiNER†", markdown)

    def test_every_rendered_panel_names_its_scoring_contract(self):
        comparison, their, declared = competitor_inputs()
        result = panels.panels(measured(), comparison, their, declared)
        external = f"All gold labels (contract v{SCORED_LABEL_CONTRACT_V1.version})"
        for name, svg in panels.files(result).items():
            with self.subTest(theme=name):
                captions = list(ET.fromstring(svg).itertext())
                self.assertEqual(captions.count("Scored labels (contract v4)"), 1)
                self.assertEqual(captions.count(external), 4)

    def test_generated_score_prose_names_its_scoring_contract(self):
        comparison, their, declared = competitor_inputs()
        self.assertIn("contract v4", panels.readme(measured()))
        markdown = panels.markdown(measured(), comparison, their, declared)
        self.assertIn("contract v4", markdown)
        self.assertIn(f"contract v{SCORED_LABEL_CONTRACT_V1.version}", markdown)

    def test_receipt_digest_tag_contract_and_counters_are_checked(self):
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary)
            manifest = {"receipts": {}}
            for kind in panels.DATASETS:
                manifest["receipts"][kind] = {}
                for version in panels.VERSIONS:
                    receipt = {"tag": version, "revision": "synthetic-tag-commit", "dataset": kind,
                               "contract": "v4" if kind == "own" else "all-gold",
                               "sets": {"test": counts()}, "totals": measurements.totals({"test": counts()}),
                               "inputs_sha256": {"dataset": "a" * 64}}
                    name = f"{kind}-{version}.json"
                    payload = json.dumps(receipt).encode()
                    (directory / name).write_bytes(payload)
                    manifest["receipts"][kind][version] = {"file": name, "sha256": hashlib.sha256(payload).hexdigest()}
            (directory / "readme-byte-panels.json").write_text(json.dumps(manifest))
            with (mock.patch.object(panels, "tag_commit", return_value="synthetic-tag-commit"),
                  mock.patch.object(panels, "POPULATIONS", {kind: {"test": 3} for kind in panels.DATASETS})):
                self.assertEqual(len(panels.load(directory)), 5)
                history = directory / "release-history.json"
                history.write_text(json.dumps({"releases": [{"version": "v0.18.0"}]}))
                with self.assertRaisesRegex(ValueError, "latest measured release"):
                    panels.load(directory)
                history.unlink()
                pin = manifest["receipts"]["own"]["v0.17.0"]
                original = json.loads((directory / pin["file"]).read_text())
                for field, value, message in (("revision", "other", "release tag"),
                                               ("contract", "v3", "contract"),
                                               ("totals", {"byte_micro_f2": 1}, "reproduce")):
                    changed = copy.deepcopy(original)
                    changed[field] = value
                    payload = json.dumps(changed).encode()
                    (directory / pin["file"]).write_bytes(payload)
                    with self.assertRaisesRegex(ValueError, "digest"):
                        panels.load(directory)
                    pin["sha256"] = hashlib.sha256(payload).hexdigest()
                    (directory / "readme-byte-panels.json").write_text(json.dumps(manifest))
                    with self.assertRaisesRegex(ValueError, message):
                        panels.load(directory)


if __name__ == "__main__":
    unittest.main()
