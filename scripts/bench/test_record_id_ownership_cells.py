"""Field ownership assertions authored independently of detector rules."""
import hashlib
import json
import unittest
import unicodedata
from pathlib import Path

import agentic_layers as api
import record_id_ownership_cells as cells
import gaze_bench_score as score


ROOT = Path(__file__).resolve().parents[2]


class RecordIdOwnershipTests(unittest.TestCase):
    def test_unicode_and_newline_ownership_cells_exist_with_exact_raw_boundaries(self):
        for partition in api.PARTITIONS:
            rows = cells.records(api, partition)
            for family in cells.LABELS:
                for shape in ("grouped_unicode_letters", "grouped_combining_marks",
                              "quoted_unicode_escape", "quoted_unicode_escape_embedded",
                              "adjacent_personal_same_newline",
                              "adjacent_personal_different_newline"):
                    selected = [r for r in rows if r.family == family and shape in r.surface]
                    if shape.startswith("quoted_unicode_escape"):
                        exact = {"recordids_ownership_" + shape + suffix for suffix in ("", "_repeat", "_public")}
                        selected = [r for r in selected if r.surface in exact]
                    self.assertEqual({r.layer for r in selected}, {"A", "D", "R"}, (partition, family, shape))
                    for row in selected:
                        raw = row.text.encode()
                        spans = row.gold if row.layer != "D" else row.decoys
                        for span in spans:
                            self.assertEqual(raw[span.start:span.end].decode(), span.value)
                        if shape.startswith("grouped"):
                            value = spans[0].value
                            self.assertTrue(any(ord(c) > 127 and c.isalpha() for c in value)
                                            if "letters" in shape else
                                            any(unicodedata.category(c).startswith("M") for c in value))
                            self.assertIn(" ", value.replace("\u00a0", " "))
                        elif shape.startswith("quoted_unicode_escape"):
                            value = spans[0].value
                            self.assertIn("\\u", value)
                            payload = json.loads(row.text.split("\nLookup ")[0])
                            embedded = "embedded" in row.surface
                            if embedded:
                                payload = json.loads(payload["payload"])
                                value = json.loads('"' + value + '"')
                            decoded = next(iter(payload["arguments"].values()))
                            self.assertEqual(json.loads('"' + value + '"'), decoded)
                            self.assertTrue(any(unicodedata.category(c).startswith("M") for c in decoded))
                            self.assertTrue(any(ord(c) > 127 and c.isalpha() for c in decoded))
                        else:
                            first, second = spans[:2]
                            self.assertEqual(raw[first.end:first.end + 1], b"\n")
                            self.assertNotIn("\n", first.value)
                            self.assertNotIn("\n", second.value)
                            if row.layer == "R":
                                self.assertEqual(row.gold[1].value, row.gold[2].value)
                public = [r for r in rows if r.family == family and
                          "adjacent_public" in r.surface and "newline" in r.surface]
                self.assertEqual({r.layer for r in public}, {"A", "D", "R"})
                for row in public:
                    first = row.gold[0] if row.gold else row.decoys[0]
                    self.assertEqual(row.text.encode()[first.end:first.end + 1], b"\n")
                    self.assertTrue(any(d.label == "public_reference" for d in row.decoys))
                    if row.layer == "R":
                        self.assertEqual(len(row.gold), 2)
                        self.assertEqual(row.gold[0].value, row.gold[1].value)

    def test_previous_corpus_and_contract_remain_reconstructible(self):
        expected = {"dev": "3504fb17f9dd74834aa85d60c89c33ad9b9ece658f73d0839fc94fd875c1b607",
                    "test": "d6dac67a85d555f3997f0684a2e58a29570937a82fb16347daeadf4b63967622"}
        for partition, digest in expected.items():
            old = api.records_as_of(16, api.generate(partition))
            self.assertEqual(hashlib.sha256(api.corpus_bytes(old)).hexdigest(), digest)
            self.assertFalse(any(r.surface.startswith("recordids_ownership_") for r in old))
        self.assertEqual(api.corpus_identity(ROOT, 16)[1],
                         "177cdd59eb82ec1700ffc3983261d6b459644f3b8499d4f828dd9f29c8cf1317")

    def test_independent_cells_have_exact_bytes_and_disjoint_partitions(self):
        pools = []
        for partition in api.PARTITIONS:
            rows = cells.records(api, partition)
            self.assertEqual(rows, cells.records(api, partition))
            self.assertEqual(len(rows), len({r.uid for r in rows}))
            self.assertEqual({r.layer for r in rows}, {"A", "D", "R"})
            self.assertEqual({r.family for r in rows}, set(cells.LABELS))
            self.assertTrue(all(r.surface.startswith("recordids_ownership_") for r in rows))
            for record in rows:
                raw = record.text.encode()
                self.assertEqual(bool(record.gold), record.layer != "D")
                for span in (*record.gold, *record.decoys):
                    self.assertEqual(raw[span.start:span.end].decode(), span.value)
            pools.append({g.value for r in rows for g in r.gold})
        self.assertFalse(pools[0] & pools[1])

    def test_grouped_values_and_word_only_controls(self):
        rows = cells.records(api, "test")
        grouped = [r for r in rows if "grouped" in r.surface and r.gold]
        self.assertTrue(grouped)
        self.assertEqual({r.language for r in grouped}, {"en", "de"})
        for row in grouped:
            self.assertTrue(any(ch.isspace() for ch in row.gold[0].value))
            self.assertTrue(any(ch.isalpha() for ch in row.gold[0].value.split()[0]))
            self.assertGreaterEqual(sum(ch.isdigit() for ch in row.gold[0].value), 6)
        controls = [r for r in rows if "placeholder" in r.surface and r.layer == "D"]
        self.assertTrue(controls)
        self.assertTrue(all(any(d.label == "placeholder" for d in r.decoys) for r in controls))

    def test_quoted_scalar_gold_is_the_whole_raw_json_value(self):
        rows = [r for r in cells.records(api, "test") if "quoted" in r.surface and "_log_" not in r.surface and r.layer == "A"]
        self.assertTrue(rows)
        encountered = set()
        for row in rows:
            if "embedded" in row.surface:
                parsed = json.loads(json.loads(row.text)["payload"])
            else:
                parsed = json.loads(row.text)
            value = next(iter(parsed["arguments"].values()))
            self.assertIsInstance(value, str)
            encountered.update(ch for ch in value if ch in "@\t\\\"é")
            self.assertEqual(len(row.gold), 1)
            raw = row.gold[0].value
            if " " in raw:
                self.assertNotEqual(raw, raw.split()[0])
            if "embedded" not in row.surface:
                self.assertEqual(json.loads('"' + raw + '"'), value)
        self.assertLessEqual(set("@\t\\\"é"), encountered)

    def test_adjacent_fields_own_separate_values_and_repeat_the_second(self):
        rows = [r for r in cells.records(api, "test") if "adjacent_personal" in r.surface and r.gold]
        self.assertTrue(rows)
        for row in rows:
            self.assertGreaterEqual(len(row.gold), 2)
            self.assertNotEqual(row.gold[0].value, row.gold[1].value)
            self.assertLess(row.gold[0].end, row.gold[1].start)
            if row.layer == "R":
                self.assertEqual(len(row.gold), 3)
                self.assertEqual(row.gold[1].value, row.gold[2].value)
                self.assertGreaterEqual(sum(c.isdigit() for c in row.gold[1].value), 6)
        public = [r for r in cells.records(api, "test") if "adjacent_public" in r.surface]
        self.assertTrue(public)
        self.assertEqual({r.layer for r in public}, {"A", "D", "R"})
        self.assertTrue(all(r.decoys for r in public))
        self.assertTrue(all(len(r.gold) == 1 for r in public if r.layer == "A"))

    def test_coverage_contains_all_three_failure_classes_and_cue_resumption(self):
        rows = cells.records(api, "test")
        for family in cells.LABELS:
            selected = [r for r in rows if r.family == family]
            for failure in ("grouped", "quoted", "adjacent"):
                self.assertEqual({r.layer for r in selected if failure in r.surface}, {"A", "D", "R"})
            resumed = [r for r in selected if "resumption" in r.surface]
            self.assertEqual({r.layer for r in resumed}, {"A", "D", "R"})
            self.assertTrue(all(r.decoys for r in resumed))

    def test_canonical_scorer_detects_prefix_veto_suffix_and_field_swallowing_mutants(self):
        rows = cells.records(api, "test")

        def metrics(row, predictions):
            document = score.Document(row.uid, row.text, row.language, row.region,
                                      "authored", tuple(score.Span(g.start, g.end, g.label) for g in row.gold))
            result = score.MetricAccumulator()
            result.add(document, predictions)
            return result.result()["utf8_bytes"]

        grouped = next(r for r in rows if r.layer == "A" and "grouped" in r.surface)
        self.assertGreater(metrics(grouped, [])["leaked"], 0)
        quoted = next(r for r in rows if r.layer == "A" and r.surface.endswith("quoted_unicode"))
        first = quoted.gold[0]
        prefix_end = first.start + len(first.value.split()[0].encode())
        self.assertGreater(metrics(quoted, [score.Span(first.start, prefix_end, first.label)])["leaked"], 0)
        adjacent = next(r for r in rows if r.layer == "R" and r.surface.endswith("adjacent_personal_same_space_repeat"))
        a, b, repeated = adjacent.gold
        swallowed = metrics(adjacent, [score.Span(a.start, b.end, a.label)])
        self.assertGreater(swallowed["false_positive"], 0)
        self.assertEqual(swallowed["leaked"], repeated.end - repeated.start)
        public = next(r for r in rows if r.layer == "A" and "adjacent_public" in r.surface)
        a = public.gold[0]
        self.assertGreater(metrics(public, [score.Span(a.start, public.decoys[-1].end, a.label)])["false_positive"], 0)
        benign = next(r for r in rows if r.layer == "D" and "quoted" in r.surface)
        d = benign.decoys[0]
        self.assertGreater(metrics(benign, [score.Span(d.start, d.end, "CUSTOMER_ID")])["false_positive"], 0)
        for row in (grouped, quoted, adjacent, public, benign):
            oracle = metrics(row, [score.Span(g.start, g.end, g.label) for g in row.gold])
            self.assertEqual((oracle["leaked"], oracle["false_positive"]), (0, 0))


if __name__ == "__main__":
    unittest.main()
