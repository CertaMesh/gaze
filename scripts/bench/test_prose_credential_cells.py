"""Keep the v31 prose credential cells independent of detector output."""
import base64
import hashlib
import unittest

import agentic_layers as api
import prose_credential_cells as cells

V30_CORPUS_SHA256 = {
    "dev": "7e61a442997f20c251a9415f1ec0e4945e49ed92e41af5ec71d1e87043083b06",
    "test": "c8f6d8ca34c5f9d63538e993503c615fbfa24ae15295bd260090ed3f5214459e",
}


class ProseCredentialCellsTests(unittest.TestCase):
    def test_layers_labels_and_source_offsets(self):
        for partition in api.PARTITIONS:
            records = cells.records(api, partition)
            self.assertEqual(
                {layer: sum(r.layer == layer for r in records) for layer in "ADR"},
                {"A": 230, "D": 200, "R": 190},
            )
            for record in records:
                self.assertTrue(record.surface.startswith(cells.SURFACE_PREFIX))
                self.assertEqual(len(record.gold), {"A": 1, "D": 0, "R": 2}[record.layer], record.uid)
                if record.layer in "AD":
                    # Every protected D byte and non-gold A byte already counts as a false positive.
                    self.assertFalse(record.decoys, record.uid)
                for gold in record.gold:
                    self.assertEqual(record.text.encode()[gold.start:gold.end].decode(), gold.value)
                    self.assertIn(gold.label, {"PASSWORD", "SECURITYTOKEN"})
                if record.layer == "R":
                    self.assertEqual(record.gold[0].value, record.gold[1].value)

    def test_encoded_private_keys_decode_to_private_armour_and_decoys_to_public(self):
        for record in cells.records(api, "test"):
            if record.family != "pem_base64":
                continue
            for gold in record.gold:
                decoded = base64.b64decode(gold.value.replace("\n", "")).decode()
                self.assertRegex(decoded, r"-----BEGIN [A-Z ]*PRIVATE KEY-----")
            if record.layer == "D":
                encoded = record.text.split(": ", 1)[1].strip()
                self.assertNotIn("PRIVATE KEY", base64.b64decode(encoded).decode())
        alignments = {
            record.surface: (len(base64.b64decode(record.gold[0].value).split(b"-----BEGIN")[0]) % 3)
            for record in cells.records(api, "test")
            if record.family == "pem_base64" and record.layer == "A" and "wrapped" not in record.surface
        }
        self.assertEqual(set(alignments.values()), {0, 1, 2})

    def test_partitions_use_different_values_and_rendered_texts(self):
        dev, test = [cells.records(api, p) for p in api.PARTITIONS]
        self.assertFalse({r.text for r in dev} & {r.text for r in test})
        self.assertFalse({g.value for r in dev for g in r.gold} & {g.value for r in test for g in r.gold})

    def test_generator_v30_rebuilds_byte_for_byte(self):
        for partition, expected in V30_CORPUS_SHA256.items():
            rows = api.records_as_of(30, api.generate(partition))
            self.assertEqual(hashlib.sha256(api.corpus_bytes(rows)).hexdigest(), expected)


if __name__ == "__main__":
    unittest.main()
