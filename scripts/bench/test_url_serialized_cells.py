"""Independent raw-gold obligations for the inactive serialization addition."""
import sys
import unittest
import hashlib
import json
from collections import Counter
from itertools import product
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import url_cells as urls
import url_serialized_cells as serialized


class SerializedRawSpanTests(unittest.TestCase):
    def test_literal_unicode_values_and_repeats_keep_original_utf8_gold(self):
        raw = r"https:\u002f\/portal.example.invalid/users/\uD83D\uDE80\u002f"
        text, gold, decoys = serialized.render("test", "json_compact", "literal", (raw, raw))
        prefix = '{"note":"é","case":"literal","w":"'
        expected = prefix + raw + '","again":"' + raw + '","n":81.9,"status":"open"}'
        self.assertEqual(text, expected)
        self.assertEqual([(g.start, g.end, g.label, g.value) for g in gold], [
            (35, 35 + len(raw.encode()), "URL", raw),
            (35 + len(raw.encode()) + 11, 35 + 2 * len(raw.encode()) + 11, "URL", raw),
        ])
        self.assertEqual(decoys, ())

    def test_single_quoted_void_neighbors_are_non_gold(self):
        raw = "https://portal.example.invalid/users/synthetic"
        text, gold, decoys = serialized.render("test", "html_img", "literal", (raw,))
        self.assertEqual(text, "é:case=literal <img src='" + raw + "'/> units=81.9")
        self.assertEqual([(g.start, g.end, g.value) for g in gold],
                         [(26, 26 + len(raw.encode()), raw)])
        self.assertEqual(text.encode()[gold[0].end:], b"'/> units=81.9")
        self.assertEqual(decoys, ())


class SerializedPopulationTests(unittest.TestCase):
    def test_valid_unicode_source_values_are_visible_in_each_layer(self):
        records = getattr(urls, "generate_extended", urls.generate)("test")
        additions = [r for r in records if r.family.startswith("url_serialized_")]
        self.assertEqual({r.layer for r in additions}, {"A", "D", "R"})
        for layer in ("A", "R"):
            values = [g.value for r in additions if r.layer == layer for g in r.gold]
            self.assertTrue(any(r"\u" in v for v in values))
            self.assertTrue(any(v.startswith(r"https:\u002f\u002F") for v in values))
            self.assertTrue(any(r"\uD83D\uDE80" in v for v in values))
            self.assertTrue(any(r"\u003fowner\u003d" in v for v in values))
        self.assertTrue(any(r"\u" in d.value for r in additions if r.layer == "D" for d in r.decoys))

    def test_corrupt_neighbors_and_missing_decoys_are_rejected(self):
        from dataclasses import replace
        from agentic_layers import LayerError
        records = serialized.generate("test")
        corrupted = list(records)
        corrupted[0] = replace(corrupted[0], text=corrupted[0].text.replace('"n":81.9', '"n":82.9'))
        with self.assertRaisesRegex(LayerError, "surrounding"):
            serialized.check(corrupted, "test")
        repeated = next(index for index, r in enumerate(records) if r.layer == "R")
        corrupted = list(records)
        corrupted[repeated] = replace(corrupted[repeated], decoys=())
        with self.assertRaisesRegex(LayerError, "decoy"):
            serialized.check(corrupted, "test")

    def test_frozen_760_row_prefix_and_both_pins_are_byte_identical(self):
        import agentic_layers as agentic
        pins = {
            "dev": "7d122c171d3e74443198542daccd3b2e5ebe73e491e0923c2439864a0988bbaa",
            "test": "09bb55bb163bf59303b3bd2197b15b19873697924aaad98e8e1d45f093aefb1e",
        }
        for partition in ("dev", "test"):
            old = urls.generate(partition)
            extended = urls.generate_extended(partition)
            self.assertEqual(len(old), 760)
            self.assertEqual(Counter(r.layer for r in old), {"A": 520, "D": 168, "R": 72})
            self.assertEqual(agentic.corpus_bytes(extended[:760]), agentic.corpus_bytes(old))
            self.assertEqual(hashlib.sha256(agentic.corpus_bytes(extended[:760])).hexdigest(), pins[partition])
            self.assertEqual(len(extended), 1588)
            self.assertEqual(Counter(r.layer for r in extended[760:]), {"A": 292, "D": 280, "R": 256})

    def test_extended_partitions_templates_values_ids_and_texts_are_disjoint(self):
        dev, test = urls.generate_extended("dev"), urls.generate_extended("test")
        for records in (dev, test):
            self.assertEqual(len({r.text for r in records}), len(records))
            self.assertEqual(len({r.uid for r in records}), len(records))
        for field in ("uid", "text", "template", "group"):
            self.assertFalse({getattr(r, field) for r in dev} & {getattr(r, field) for r in test}, field)
        self.assertFalse({g.value for r in dev for g in r.gold} & {g.value for r in test for g in r.gold})

    def test_valid_json_schema_reference_gold_and_exact_repeat_ownership(self):
        for partition in ("dev", "test"):
            for record in serialized.generate(partition):
                row = json.loads(json.dumps(record.to_json()))
                self.assertEqual(row["text"], record.text)
                self.assertEqual([(s.start, s.end, s.label) for s in record.to_document().spans],
                                 [(g.start, g.end, "URL") for g in record.gold])
                raw = record.text.encode()
                for span in (*record.gold, *record.decoys):
                    self.assertEqual(raw[span.start:span.end], span.value.encode())
                if record.surface in {"url_serialized_json_compact", "url_serialized_json_nested"}:
                    parsed = json.loads(record.text)
                    payload = parsed.get("profile", parsed.get("account", parsed))
                    selected = record.decoys if record.layer == "D" else record.gold
                    self.assertEqual(payload["w"], json.loads('"' + selected[0].value + '"'))
                    self.assertIn("n", parsed)
                    if record.layer == "R":
                        self.assertEqual(payload["again"], payload["w"])
                if record.layer == "R":
                    self.assertEqual(len(record.gold), 2)
                    self.assertEqual(record.gold[0].value, record.gold[1].value)
                    self.assertLess(record.gold[0].end, record.gold[1].start)
                    self.assertEqual(len(record.decoys), 1)
                if record.family in {"url_serialized_reference_docs", "url_serialized_reference_repo"} and record.layer != "D":
                    self.assertTrue(record.gold)
                    self.assertIn(".example.invalid/guide/", record.gold[0].value)

    def test_all_scheme_combinations_and_counterparts_have_literal_evidence(self):
        values = {g.value for r in serialized.generate("test")
                  if r.layer == "A" for g in r.gold}
        for left, right in product(("/", r"\/", r"\u002f", r"\u002F"), repeat=2):
            self.assertTrue(any(v.startswith("https:" + left + right + "portal.") for v in values))
        records = serialized.generate("test")
        for variant in ("account_first", "account_middle", "account_final", "surrogate",
                        "query_owner", "query_separators", "reference_docs", "reference_repo"):
            self.assertEqual({r.layer for r in records if r.family == "url_serialized_" + variant},
                             {"A", "D", "R"})
        for record in records:
            if record.layer == "D" and "malformed_scheme_" not in record.family:
                self.assertFalse(record.decoys[0].value.startswith(("http", "www.")))

    def test_existing_scorer_prices_raw_units_and_non_gold_neighbors(self):
        import gaze_bench_score as score
        record = next(r for r in serialized.generate("test")
                      if r.family == "url_serialized_terminal_unicode_slash" and r.layer == "A")
        document = record.to_document()
        gold = document.spans[0]
        self.assertTrue(record.gold[0].value.endswith((r"\u002f", r"\u002F")))
        truncated = score.MetricAccumulator()
        truncated.add(document, [score.Span(gold.start, gold.end - 6, "URL")])
        self.assertEqual(truncated.result()["utf8_bytes"]["leaked"], 6)
        self.assertEqual(truncated.result()["utf8_bytes"]["false_positive"], 0)
        oversized = score.MetricAccumulator()
        oversized.add(document, [score.Span(gold.start, gold.end + 1, "URL")])
        self.assertEqual(oversized.result()["utf8_bytes"]["false_positive"], 1)
        self.assertEqual(oversized.result()["utf8_bytes"]["leaked"], 0)
        html = next(r for r in serialized.generate("test") if r.layer == "A" and r.surface == "url_serialized_html_img")
        accumulator = score.MetricAccumulator()
        span = html.to_document().spans[0]
        accumulator.add(html.to_document(), [score.Span(span.start, span.end + 2, "URL")])
        self.assertEqual(accumulator.result()["utf8_bytes"]["false_positive"], 2)
