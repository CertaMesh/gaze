"""The frozen inactive URL corpus must see quoted self-closing precision."""
import hashlib
import sys
import unittest
from collections import Counter
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import agentic_layers as agentic
import gaze_bench_score as score
import url_cells as urls
import url_serialized_cells as serialized


class HtmlBoundaryCellTests(unittest.TestCase):
    def test_both_selfclosing_tags_have_positive_counterweight_and_repeat_cells(self):
        for partition in ("dev", "test"):
            records = [r for r in urls.generate_extended(partition)
                       if r.family == "url_serialized_selfclosing"]
            self.assertEqual(Counter((r.layer, r.surface) for r in records),
                             {(layer, "url_serialized_" + tag): 4
                              for layer in ("A", "D", "R") for tag in ("html_img", "html_link")})
            for record in records:
                raw = record.text.encode()
                spans = record.decoys if record.layer == "D" else record.gold
                for span in spans:
                    self.assertEqual(raw[span.start:span.end], span.value.encode())
                    if span.label == "near_miss":
                        continue
                    self.assertEqual(raw[span.start - 1:span.start], b"'")
                    self.assertTrue(raw[span.end:].startswith(b"'/>") )
                if record.layer == "D":
                    self.assertEqual(record.gold, ())
                    self.assertFalse(record.decoys[0].value.startswith(("http", "www.")))
                if record.layer == "R":
                    self.assertEqual(len(record.gold), 2)
                    self.assertEqual(record.gold[0].value, record.gold[1].value)
                    self.assertLess(record.gold[0].end, record.gold[1].start)
                    self.assertEqual(len(record.decoys), 1)

    def test_existing_scorer_prices_every_quote_slash_overspan_and_bare_counterweight(self):
        for partition in ("dev", "test"):
            for record in serialized.generate(partition):
                if record.family != "url_serialized_selfclosing":
                    continue
                document = record.to_document()
                predictions = [score.Span(g.start, g.end + 2, "URL") for g in record.gold]
                oversized = score.MetricAccumulator()
                oversized.add(document, predictions)
                self.assertEqual(oversized.result()["utf8_bytes"]["leaked"], 0)
                self.assertEqual(oversized.result()["utf8_bytes"]["false_positive"], 2 * len(record.gold))
                exact = score.MetricAccumulator()
                exact.add(document, [score.Span(g.start, g.end, "URL") for g in record.gold])
                self.assertEqual(exact.result()["utf8_bytes"]["leaked"], 0)
                self.assertEqual(exact.result()["utf8_bytes"]["false_positive"], 0)
                if record.layer == "D":
                    decoy = record.decoys[0]
                    benign = score.MetricAccumulator()
                    benign.add(document, [score.Span(decoy.start, decoy.end, "URL")])
                    self.assertEqual(benign.result()["utf8_bytes"]["false_positive"], len(decoy.value.encode()))

    def test_literal_img_link_unicode_and_repeated_insertion_offsets(self):
        value = r"https:\u002F\/portal.example.invalid/users/\uD83D\uDE80\u002f"
        near = "portal.example.invalid/users/synthetic"
        for tag, attribute, prefix_bytes in (("img", "src", 26), ("link", "href", 28)):
            text, gold, decoys = serialized.render("test", "html_" + tag, "literal", (value, value), near=near)
            prefix = f"é:case=literal <{tag} {attribute}='"
            separator = f"'/> then <{tag} {attribute}='"
            suffix = "'/> units=81.9 near="
            self.assertEqual(text, prefix + value + separator + value + suffix + near)
            size = len(value.encode())
            second = prefix_bytes + size + len(separator.encode())
            self.assertEqual([(g.start, g.end, g.label, g.value) for g in gold],
                             [(prefix_bytes, prefix_bytes + size, "URL", value),
                              (second, second + size, "URL", value)])
            self.assertEqual([(d.start, d.end, d.value) for d in decoys],
                             [(second + size + len(suffix.encode()), second + size + len(suffix.encode()) + len(near.encode()), near)])

    def test_both_extended_pins_remain_exact_and_no_records_are_added(self):
        pins = {
            "dev": "a0f10f48bf64d0c436704e6ff67ab03a50728eaaeaa467e1ac0a63eddbd4fc76",
            "test": "b9bce06fed12371990993280ef68d1fb8183cd4b7ed4bee2928fa1f061ec4783",
        }
        for partition, pin in pins.items():
            records = urls.generate_extended(partition)
            self.assertEqual(len(records), 1588)
            self.assertEqual(Counter(r.layer for r in records), {"A": 812, "D": 448, "R": 328})
            self.assertEqual(hashlib.sha256(agentic.corpus_bytes(records)).hexdigest(), pin)


if __name__ == "__main__":
    unittest.main()
