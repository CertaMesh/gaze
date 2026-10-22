"""Independent raw field-boundary coverage, including canonical scorer sensitivity."""
import hashlib
import json
import re
import unittest

import agentic_layers as api
import gaze_bench_score as score
import record_id_boundary_cells as cells


class RecordIdBoundaryTests(unittest.TestCase):
    def test_each_shape_has_all_families_partitions_and_scored_layers(self):
        for partition in api.PARTITIONS:
            rows = cells.records(api, partition)
            self.assertEqual(rows, cells.records(api, partition))
            self.assertEqual(len(rows), len({r.uid for r in rows}))
            for family in cells.LABELS:
                for shape in ("punctuation", "connectors", "serialized", "opening_lf", "tab_group", "unterminated"):
                    selected = [r for r in rows if r.family == family and r.surface.startswith("recordids_boundary_" + shape)]
                    self.assertEqual({r.layer for r in selected}, {"A", "D", "R"}, (partition, family, shape))
            for row in rows:
                self.assertEqual(bool(row.gold), row.layer != "D")
                for span in (*row.gold, *row.decoys):
                    self.assertEqual(row.text.encode()[span.start:span.end].decode(), span.value)

    def test_partition_values_and_previous_inputs_stay_disjoint_and_frozen(self):
        values = [{g.value for r in cells.records(api, p) for g in r.gold} for p in api.PARTITIONS]
        self.assertFalse(values[0] & values[1])
        pins = {"dev": "7c205507728f13b54e6ad299f2e414e7bdf37ad9a144c265bd839dee98359a83",
                "test": "8c62d49926fa90107fa538b03b39aed6d76a2ad9f2d38e38d14cb3e8b1706725"}
        for partition, digest in pins.items():
            self.assertEqual(hashlib.sha256(api.corpus_bytes(api.records_as_of(17, api.generate(partition)))).hexdigest(), digest)
        v18_pins = {"dev": "3dfd64aa881c3ccb0aec0860aa83246f0e423716af4066b5e6197cd790a3af6b",
                    "test": "56c7bbd0d9a0eeb290d595de5deac1d07273e7ff28b431039abb2d83bd71618c"}
        for partition, digest in v18_pins.items():
            self.assertEqual(hashlib.sha256(api.corpus_bytes(api.records_as_of(18, api.generate(partition)))).hexdigest(), digest)
        from pathlib import Path
        root = Path(__file__).resolve().parents[2]
        for version, digest in ((17, "a13af8a9773195adc0f8dee2a2ef0aa53a838158d876748b03a26cdfbb3a173a"),
                                (18, "5a638e5ff857d43c918a8840867c72d6d322f097964b8f946a5f4e310dac2dc6")):
            self.assertEqual(hashlib.sha256((root / api.HISTORICAL_CONTRACTS[version]).read_bytes()).hexdigest(), digest)

    def test_unterminated_suffix_cells_own_delimiters_and_preserve_previous_corpus(self):
        pins = {"dev": "a88025bd7c995e3633738debf8b8bab45c8c7232bf222a4d72a9af36fc67ca59",
                "test": "133f8452d01fd517c77087e539ba9dc253e30a1886668bfab3e1631cc9ff6c32"}
        for partition in api.PARTITIONS:
            self.assertEqual(hashlib.sha256(api.corpus_bytes(api.records_as_of(25, api.generate(partition)))).hexdigest(), pins[partition])
            rows = [r for r in cells.records(api, partition) if r.surface.startswith("recordids_boundary_unterminated_")]
            self.assertEqual(len(rows), 810)
            for row in rows:
                value = (row.gold or row.decoys)[0].value
                self.assertIn("É", value)
                self.assertTrue(any(d in value for d in (";", ",", "\n")))
                if row.layer != "D":
                    self.assertEqual(len(row.gold), 2 if row.layer == "R" else 1)
                if not row.surface.endswith("_eof"):
                    self.assertTrue(any(d.label == "public_column" for d in row.decoys))

    def test_terminal_internal_and_quoted_punctuation_own_different_bytes(self):
        for partition in api.PARTITIONS:
            rows = cells.records(api, partition)
            for family in cells.LABELS:
                for shape in ("period", "colon", "dotted", "internal_colon", "quoted_period", "quoted_colon", "short_dotted"):
                    selected = [r for r in rows if r.family == family and r.surface == "recordids_boundary_punctuation_" + shape]
                    self.assertEqual({r.layer for r in selected}, {"A", "D", "R"})
                    for row in selected:
                        spans = row.gold if row.gold else row.decoys
                        if shape.startswith("quoted"):
                            self.assertTrue(spans[0].value.endswith((".", ":")))
                        else:
                            self.assertFalse(spans[0].value.endswith((".", ":")))
                        if row.layer == "R":
                            self.assertEqual(row.gold[0].value, row.gold[1].value)
                if partition == "test":
                    self.assertTrue(any(r.family == family and r.layer == "R" and
                                        [g.value for g in r.gold] == ["AB.12.34", "AB.12.34"] for r in rows))

    def test_every_connector_bounds_the_next_personal_and_public_field(self):
        for partition in api.PARTITIONS:
            rows = cells.records(api, partition)
            for family in cells.LABELS:
                for connector in ("colon", "equals", "is", "ist", "lautet", "space"):
                    for relation in ("same", "different", "public"):
                        selected = [r for r in rows if r.family == family and r.surface ==
                                    f"recordids_boundary_connectors_{connector}_{relation}"]
                        self.assertEqual({r.layer for r in selected}, {"A", "D", "R"})
                        for row in selected:
                            if row.layer == "R":
                                self.assertEqual(row.gold[-1].value, row.gold[-2].value)
                            if row.gold and relation != "public":
                                self.assertLess(row.gold[0].end, row.gold[1].start)
                                self.assertNotEqual(row.gold[0].value, row.gold[1].value)
                                gap = row.text.encode()[row.gold[0].end:row.gold[1].start].decode()
                                expected = {"colon": ": ", "equals": "=", "is": " is ",
                                            "ist": " ist ", "lautet": " lautet ", "space": " "}[connector]
                                self.assertTrue(gap.endswith(expected), gap)
                                if connector == "space":
                                    self.assertFalse(re.search(r"[:=]|\b(?:is|ist|lautet)\b", gap, re.I))
                            if relation == "public":
                                self.assertTrue(any(d.label == "public_column" for d in row.decoys))

    def test_serialization_depth_and_raw_escaped_gold_reconstruct_actual_payloads(self):
        for partition in api.PARTITIONS:
            rows = cells.records(api, partition)
            for family in cells.LABELS:
                for depth in (0, 1, 2, 3, 8, 9):
                    for shape in ("unicode", "unicode_escape", "quote", "tab"):
                        selected = [r for r in rows if r.family == family and r.surface ==
                                    f"recordids_boundary_serialized_depth{depth}_{shape}"]
                        self.assertEqual({r.layer for r in selected}, {"A", "D", "R"})
                        for row in selected:
                            payload = json.loads(row.text.split("\nLookup ")[0])
                            for _ in range(depth):
                                payload = json.loads(payload["payload"])
                            value = next(iter(payload["arguments"].values()))
                            raw = row.gold[0].value if row.gold else row.decoys[0].value
                            span = row.gold[0] if row.gold else row.decoys[0]
                            prefix = row.text.encode()[:span.start]
                            self.assertEqual(prefix[-1:], b'"')
                            self.assertEqual(len(prefix[:-1]) - len(prefix[:-1].rstrip(b"\\")), (1 << depth) - 1)
                            for _ in range(depth + 1):
                                raw = json.loads('"' + raw + '"')
                            self.assertEqual(raw, value)
                            self.assertIn({"unicode": "É", "unicode_escape": "É", "quote": '"', "tab": "\t"}[shape], raw)
                            if shape == "unicode_escape":
                                self.assertIn("\\u", row.gold[0].value if row.gold else row.decoys[0].value)
                            if row.layer == "R":
                                self.assertEqual(row.gold[0].value, row.gold[1].value)
                            self.assertTrue(any(d.label == "public_column" for d in row.decoys))

    def test_lf_crlf_closed_and_malformed_tails_measure_public_bytes(self):
        for partition in api.PARTITIONS:
            rows = cells.records(api, partition)
            for family in cells.LABELS:
                for newline in ("lf", "crlf"):
                    for closure in ("closed", "malformed"):
                        selected = [r for r in rows if r.family == family and r.surface ==
                                    f"recordids_boundary_opening_lf_{newline}_{closure}"]
                        self.assertEqual({r.layer for r in selected}, {"A", "D", "R"})
                        for row in selected:
                            spans = row.gold if row.gold else row.decoys
                            self.assertTrue(spans[0].value.startswith("\n" if newline == "lf" else "\r\n"))
                            scalar = row.gold[-1] if row.gold else row.decoys[0]
                            self.assertEqual(row.text.encode()[scalar.start - 1:scalar.start], b'"')
                            if row.gold:
                                self.assertTrue(any(d.label == "public_column" for d in row.decoys))
                                self.assertLess(row.gold[-1].end, row.decoys[-1].start)

    def test_tab_groups_public_columns_placeholders_and_tsv_headers_exist(self):
        for partition in api.PARTITIONS:
            rows = cells.records(api, partition)
            for family in cells.LABELS:
                for shape in ("unicode", "combining", "public_equals", "public_is", "public_space", "ambiguous_column"):
                    selected = [r for r in rows if r.family == family and r.surface ==
                                "recordids_boundary_tab_group_" + shape]
                    self.assertEqual({r.layer for r in selected}, {"A", "D", "R"})
                    for row in selected:
                        spans = row.gold if row.gold else row.decoys
                        if shape in ("unicode", "combining", "ambiguous_column"):
                            self.assertIn("\t", spans[0].value)
                        else:
                            self.assertNotIn("\t", spans[0].value)
                            self.assertTrue(any(d.label == "public_column" for d in row.decoys))
                for shape in ("header", "placeholder", "untyped_public"):
                    self.assertTrue(any(r.family == family and r.layer == "D" and r.surface ==
                                        "recordids_boundary_tab_group_" + shape for r in rows))

    def test_canonical_scorer_exposes_every_failure_and_conservative_cost(self):
        rows = cells.records(api, "test")

        def metrics(row, predictions):
            doc = score.Document(row.uid, row.text, row.language, row.region, "authored",
                                 tuple(score.Span(g.start, g.end, g.label) for g in row.gold))
            accumulator = score.MetricAccumulator()
            accumulator.add(doc, predictions)
            return accumulator.result()["utf8_bytes"]

        for shape in ("punctuation_period", "connectors_is_same", "serialized_depth2_unicode",
                      "opening_lf_lf_malformed", "tab_group_unicode"):
            row = next(r for r in rows if r.layer == "R" and r.surface == "recordids_boundary_" + shape)
            g = row.gold[0]
            predictions = [score.Span(g.start, g.end, g.label)]
            if shape == "punctuation_period":
                predictions = [score.Span(g.start, g.end + 1, g.label)]
                self.assertGreater(metrics(row, predictions)["false_positive"], 0)
            elif shape == "tab_group_unicode":
                predictions = [score.Span(g.start, g.start + len(g.value.split("\t")[0].encode()), g.label)]
            elif shape.startswith(("serialized", "opening")):
                predictions = []
            elif shape.startswith("connectors"):
                predictions = [score.Span(g.start, row.gold[1].start, g.label)]
            self.assertGreater(metrics(row, predictions)["leaked"], 0, shape)
            self.assertEqual(metrics(row, [score.Span(g.start, g.end, g.label) for g in row.gold])["leaked"], 0)
        for shape in ("serialized_depth9_unicode", "opening_lf_lf_malformed", "tab_group_public_equals", "tab_group_ambiguous_column"):
            row = next(r for r in rows if r.layer == "A" and r.surface == "recordids_boundary_" + shape)
            g = row.gold[0]
            self.assertGreater(metrics(row, [score.Span(g.start, len(row.text.encode()), g.label)])["false_positive"], 0)


if __name__ == "__main__":
    unittest.main()
