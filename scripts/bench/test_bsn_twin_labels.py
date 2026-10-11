"""BSN counterweights name inventory/equipment, preserving prior corpus bytes."""
import hashlib
import json
import unittest
from pathlib import Path

import agentic_layers as agentic

ROOT = Path(__file__).resolve().parents[2]
V15_HASHES = {
    "dev": "3aa6c164957056dbad916fae6959a7ed75701cc656d2ade35f5492e044f669d1",
    "test": "0a5f6576f2d5f5c8143628e88ec9d2985197fa3bd74c3823de86f611979c09ba",
}


class BsnTwinLabelTests(unittest.TestCase):
    def test_prior_corpus_and_contract_remain_byte_identical(self):
        for partition, expected in V15_HASHES.items():
            records = agentic.records_as_of(15, agentic.generate(partition))
            self.assertEqual(hashlib.sha256(agentic.corpus_bytes(records)).hexdigest(), expected)
        frozen = ROOT / agentic.HISTORICAL_CONTRACTS[15]
        self.assertEqual(hashlib.sha256(frozen.read_bytes()).hexdigest(),
                         "c8542632aad9373bc251c8ab10f2eaabd5df01f9f66ba217e914f93a4ffecabb")
        self.assertEqual(json.loads(frozen.read_text())["corpus"]["generator_version"], 15)

    def test_only_the_twenty_person_linked_labels_change_per_partition(self):
        for partition in agentic.PARTITIONS:
            current = agentic.generate(partition)
            self.assertEqual(agentic.corpus_bytes(current), agentic.corpus_bytes(agentic.generate(partition)))
            previous = agentic.records_as_of(15, current)
            changes = [(old, new) for old, new in zip(previous, current) if old != new]
            self.assertEqual(len(changes), 20)
            self.assertEqual({new.surface for _, new in changes}, {"log_kv", "tool_json"})
            for old, new in changes:
                self.assertEqual((new.layer, new.family, new.validity), ("D", "ref_number_9", "benign"))
                self.assertFalse(new.gold)
                self.assertFalse(new.decoys)
                index = int(new.group.rsplit("-", 1)[1])
                label = agentic.neutral_reference_label(partition, index)
                self.assertIn(label, agentic.NEUTRAL_REFERENCE_LABELS[partition])
                self.assertEqual(new.text.replace(label, agentic.LOOKALIKE_KEYS[partition]["ref_number_9"]), old.text)
                self.assertEqual({k: v for k, v in new.to_json().items() if k != "text"},
                                 {k: v for k, v in old.to_json().items() if k != "text"})
            rows = [new for _, new in changes if new.surface == "tool_json"]
            labels = set()
            for row in rows:
                index = int(row.group.rsplit("-", 1)[1])
                label = agentic.neutral_reference_label(partition, index)
                value = json.loads(row.text)[label]
                self.assertEqual(len(value), 9)
                self.assertTrue(value.isdigit())
                self.assertFalse(agentic.bsn_valid(value))
                labels.add(label)
            self.assertEqual(labels, set(agentic.NEUTRAL_REFERENCE_LABELS[partition]))


if __name__ == "__main__":
    unittest.main()
