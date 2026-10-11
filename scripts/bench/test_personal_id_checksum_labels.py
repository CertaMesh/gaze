"""Checksum errors do not make an explicitly labelled personal ID benign."""
import hashlib
import json
import unittest
from collections import Counter
from dataclasses import replace
from pathlib import Path

import agentic_layers as api

ROOT = Path(__file__).resolve().parents[2]
V17_HASHES = {
    "dev": "7c205507728f13b54e6ad299f2e414e7bdf37ad9a144c265bd839dee98359a83",
    "test": "8c62d49926fa90107fa538b03b39aed6d76a2ad9f2d38e38d14cb3e8b1706725",
}
V18_HASHES = {
    "dev": "3dfd64aa881c3ccb0aec0860aa83246f0e423716af4066b5e6197cd790a3af6b",
    "test": "56c7bbd0d9a0eeb290d595de5deac1d07273e7ff28b431039abb2d83bd71618c",
}


class PersonalIdChecksumLabelTests(unittest.TestCase):
    def test_frozen_v17_and_deterministic_v18_match_byte_pins(self):
        for partition in api.PARTITIONS:
            current = api.generate(partition)
            self.assertEqual(api.corpus_bytes(current), api.corpus_bytes(api.generate(partition)))
            self.assertEqual(hashlib.sha256(api.corpus_bytes(current)).hexdigest(), V18_HASHES[partition])
            old = api.records_as_of(17, current)
            self.assertEqual(hashlib.sha256(api.corpus_bytes(old)).hexdigest(), V17_HASHES[partition])
        frozen = ROOT / api.HISTORICAL_CONTRACTS[17]
        self.assertEqual(hashlib.sha256(frozen.read_bytes()).hexdigest(),
                         "a13af8a9773195adc0f8dee2a2ef0aa53a838158d876748b03a26cdfbb3a173a")

    def test_exact_cell_diff_changes_only_labelled_aadhaar_and_nir_twins(self):
        expected = Counter({(family, surface): 10 for family in ("aadhaar", "nir")
                            for surface in ("coverage_prose_invalid", "coverage_log_invalid", "coverage_json_invalid")})
        for partition in api.PARTITIONS:
            current = api.generate(partition)
            old = api.records_as_of(17, current)
            self.assertEqual(len(old), len(current))
            changes = [(before, after) for before, after in zip(old, current, strict=True) if before != after]
            self.assertEqual(Counter((r.family, r.surface) for _, r in changes), expected)
            for before, after in changes:
                self.assertEqual((before.layer, before.validity, before.gold), ("D", "benign", ()))
                self.assertEqual((after.layer, after.validity, after.decoys), ("A", "invalid", ()))
                self.assertEqual(len(before.decoys), 1)
                self.assertEqual(after.gold, (replace(before.decoys[0], label=api.PERSONAL_CHECKSUM_TWIN_LABELS[after.family]),))
                self.assertEqual(after.uid, before.uid.replace("-D-", "-A-", 1))
                # Every value, text byte, offset and semantic/template identity stays fixed.
                self.assertEqual(replace(after, uid=before.uid, layer=before.layer,
                                         validity=before.validity, gold=before.gold, decoys=before.decoys), before)
                value = after.gold[0].value
                self.assertEqual(after.text.encode()[after.gold[0].start:after.gold[0].end].decode(), value)
                if after.family == "aadhaar":
                    self.assertFalse(api.verhoeff_valid(value))
                else:
                    self.assertNotEqual(int(value[-2:]), 97 - int(value[:-2]) % 97)
            self.assertEqual(sum(len(r.gold[0].value.encode()) for _, r in changes), 810)
            self.assertEqual(Counter(r.layer for r in current), {"A": 5018, "D": 3314, "R": 1390})

    def test_committed_cell_diff_matches_every_changed_record(self):
        evidence = json.loads((ROOT / "docs/reference/benchmarks/personal-id-checksum-v18-cell-diff.json").read_text())
        for partition in api.PARTITIONS:
            current = api.generate(partition)
            old = api.records_as_of(17, current)
            changes = evidence["partitions"][partition]
            self.assertEqual(changes["changed_documents_by_class"], {"AADHAAR": 30, "NIR": 30})
            self.assertEqual(changes["new_gold_bytes"], 810)
            actual = []
            for before, after in zip(old, current, strict=True):
                if before != after:
                    actual.append({"old_id": before.uid, "new_id": after.uid,
                                   "old_cell": before.cell, "new_cell": after.cell,
                                   "gold_label": after.gold[0].label,
                                   "gold_start": after.gold[0].start, "gold_end": after.gold[0].end,
                                   "text_sha256": hashlib.sha256(after.text.encode()).hexdigest()})
            self.assertEqual(changes["changes"], actual)

    def test_company_wallet_and_public_ownership_controls_stay_benign(self):
        for partition in api.PARTITIONS:
            rows = api.generate(partition)
            selected = [r for r in rows if
                        (r.family in ("cnpj", "eth_address") and r.surface.endswith("_invalid"))
                        or (r.surface == "coverage_json" and r.layer == "D")
                        or (r.family == "ref_number_10" and r.layer == "D")
                        or r.family in ("order_json_entries_array", "order_json_type_above")]
            self.assertTrue(selected)
            account_controls = [r for r in selected if r.family in ("order_json_entries_array", "order_json_type_above")]
            self.assertEqual(Counter(r.family for r in account_controls),
                             {"order_json_entries_array": 4, "order_json_type_above": 4})
            self.assertTrue(all(r.layer == "D" and not r.gold for r in selected))
            # Inventory/batch-owned twins remain benign even for personal-ID shapes.
            public = [r for r in rows if r.family in ("aadhaar", "nir") and r.layer == "D"]
            self.assertEqual(len(public), 60)
            self.assertTrue(all(r.decoys and not r.gold for r in public))


if __name__ == "__main__":
    unittest.main()
