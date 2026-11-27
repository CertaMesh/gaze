"""Keep the v33 chat mention cells independent of detector output."""
import hashlib
import re
import unittest

import agentic_layers as api
import chat_mention_cells as cells

V32_CORPUS_SHA256 = {
    "dev": "dc6691f6e149a969db2f4a978e26df4940610feb26abbba010bc1f6488315971",
    "test": "2ec777e6577f7e304f4d6a20a85f34f63bf8d06f1c00c7c77fcb9ac5a0c0744b",
}
# The platforms' own user-mention syntax: a Slack user ID (U or W, then uppercase letters and
# digits) with an optional display name, or a Discord snowflake.
USER_MENTION = re.compile(r"<@!?((?:[UW][A-Z0-9_]+(?:\|[^<>]*)?)|\d+)>")


class ChatMentionCellsTests(unittest.TestCase):
    def test_layers_labels_and_source_offsets(self):
        for partition in api.PARTITIONS:
            records = cells.records(api, partition)
            self.assertEqual(
                {layer: sum(r.layer == layer for r in records) for layer in "ADR"},
                {"A": 90, "D": 140, "R": 90},
            )
            for record in records:
                self.assertTrue(record.surface.startswith(cells.SURFACE_PREFIX))
                self.assertEqual(len(record.gold), {"A": 1, "D": 0, "R": 2}[record.layer], record.uid)
                self.assertFalse(record.decoys, record.uid)
                for gold in record.gold:
                    self.assertEqual(gold.label, "USERNAME")
                    self.assertEqual(record.text.encode()[gold.start:gold.end].decode(), gold.value)
                # Gold is exactly the value inside each user mention, never the markup around it.
                self.assertEqual(USER_MENTION.findall(record.text), [g.value for g in record.gold], record.uid)

    def test_both_platforms_and_display_names_are_measured(self):
        values = [g.value for r in cells.records(api, "test") if r.layer == "A" for g in r.gold]
        self.assertTrue(any(v.isdigit() for v in values))
        self.assertTrue(any(v[0] in "UW" and "|" in v for v in values))
        self.assertTrue(any(r.layer == "A" and "<@!" in r.text for r in cells.records(api, "test")))

    def test_partitions_use_different_values_and_rendered_texts(self):
        dev, test = [cells.records(api, p) for p in api.PARTITIONS]
        self.assertFalse({r.text for r in dev} & {r.text for r in test})
        self.assertFalse({g.value for r in dev for g in r.gold} & {g.value for r in test for g in r.gold})

    def test_generator_v32_rebuilds_byte_for_byte(self):
        for partition, expected in V32_CORPUS_SHA256.items():
            rows = api.records_as_of(32, api.generate(partition))
            self.assertEqual(hashlib.sha256(api.corpus_bytes(rows)).hexdigest(), expected)


if __name__ == "__main__":
    unittest.main()
