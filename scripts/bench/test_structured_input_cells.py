"""Structured-input slice: generated payloads, frozen history and the slice reader."""
import csv
import hashlib
import io
import json
from pathlib import Path
import unittest

import agentic_layers as api
import scorecard_record as records
import structured_input_cells as cells
import structured_slice as reader

ROOT = Path(__file__).resolve().parents[2]
V25_CORPUS_SHA256 = {
    "dev": "a88025bd7c995e3633738debf8b8bab45c8c7232bf222a4d72a9af36fc67ca59",
    "test": "133f8452d01fd517c77087e539ba9dc253e30a1886668bfab3e1631cc9ff6c32",
}
V25_CONTRACT_SHA256 = "676b4794016b83dcab811a83d626428caef2cc1992ef0eca6f452db22d95f278"
JSON_SHAPES = tuple(s for s in cells.SHAPES if s not in ("csv", "ndjson"))


def _rows(partition="test"):
    return cells.records(api, partition)


def _decoded(shape, raw):
    """The value a parser reads from the raw gold bytes."""
    if shape == "csv":
        return raw
    depth = 2 if shape in cells.DOUBLE_ESCAPED else 1
    for _ in range(depth):
        raw = json.loads(f'"{raw}"')
    return raw


def _leaves(value):
    if isinstance(value, dict):
        for item in value.values():
            yield from _leaves(item)
    elif isinstance(value, list):
        for item in value:
            yield from _leaves(item)
    else:
        yield value


class GeneratedPayloadTests(unittest.TestCase):
    KEYS = {k for family in cells.KEYS["test"].values() for k in family}

    def test_every_family_and_shape_is_generated_deterministically(self):
        for partition in api.PARTITIONS:
            rows = _rows(partition)
            self.assertEqual(api.corpus_bytes(rows), api.corpus_bytes(_rows(partition)))
            self.assertEqual(len(rows), len({r.uid for r in rows}))
            for shape in cells.SHAPES:
                surface = cells.SURFACE_PREFIX + shape
                for family in cells.record_ids.LABELS:
                    selected = [r for r in rows if r.layer == "A" and r.family == family and r.surface == surface]
                    self.assertEqual(len(selected), api.DOCS_PER_FAMILY, (partition, family, shape))
                benign = [r for r in rows if r.layer == "D" and r.surface == surface]
                self.assertEqual(len(benign), api.DOCS_PER_FAMILY, (partition, shape))
                self.assertEqual({r.family for r in benign}, {cells.BENIGN_FAMILY})

    def test_gold_and_decoys_select_raw_bytes_and_positives_carry_decoys(self):
        kinds = set()
        for partition in api.PARTITIONS:
            for row in _rows(partition):
                encoded = row.text.encode("utf-8")
                for span in (*row.gold, *row.decoys):
                    self.assertEqual(encoded[span.start:span.end].decode("utf-8"), span.value)
                self.assertTrue(row.decoys, row.uid)
                self.assertEqual(bool(row.gold), row.layer == "A", row.uid)
                kinds |= {d.label for d in row.decoys}
                if row.layer == "D":
                    self.assertIn("benign_id", {d.label for d in row.decoys})
        self.assertEqual(kinds, set(cells.DECOY_KINDS))

    def test_payloads_parse_and_gold_is_the_field_value(self):
        for row in _rows():
            shape = row.surface.removeprefix(cells.SURFACE_PREFIX)
            ids = [_decoded(shape, g.value) for g in row.gold if g.label.endswith("_ID")]
            if shape == "csv":
                table = list(csv.reader(io.StringIO(row.text)))
                self.assertEqual(len(table), 3)
                self.assertEqual([line[0] for line in table[1:]], ids or [line[0] for line in table[1:]])
                continue
            if shape == "ndjson":
                lines = [json.loads(line) for line in row.text.splitlines()]
                self.assertEqual(len(lines), 3)
                if ids:
                    self.assertEqual(ids, [line[next(k for k in line if k in self.KEYS)] for line in lines])
                continue
            payload = json.loads(row.text)
            if shape == "anthropic_tool_result":
                payload = json.loads(payload["content"])
            if shape == "openai_tool_call":
                payload = json.loads(payload["tool_calls"][0]["function"]["arguments"])
            values = [str(v) for v in _leaves(payload)]
            for value in ids:
                self.assertIn(value, values)
            if shape == "numeric_label" and ids:
                self.assertEqual([v for v in payload.values() if isinstance(v, int) and str(v) == ids[0]], [int(ids[0])])

    def test_every_digit_bearing_value_is_gold_or_a_recorded_decoy(self):
        checked = 0
        for row in _rows():
            shape = row.surface.removeprefix(cells.SURFACE_PREFIX)
            recorded = {_decoded(shape, s.value) for s in (*row.gold, *row.decoys)}
            if shape == "csv":
                values = [cell for line in csv.reader(io.StringIO(row.text)) for cell in line]
            else:
                payloads = ([json.loads(line) for line in row.text.splitlines()] if shape == "ndjson"
                            else [json.loads(row.text)])
                for payload in list(payloads):
                    if shape == "anthropic_tool_result":
                        payloads.append(json.loads(payload["content"]))
                    if shape == "openai_tool_call":
                        payloads.append(json.loads(payload["tool_calls"][0]["function"]["arguments"]))
                values = [v for p in payloads for v in _leaves(p) if not str(v).startswith(("{", "["))]
            numbers = {float(r) for r in recorded if r.replace(".", "", 1).isdigit()}
            for value in values:
                if any(ch.isdigit() for ch in str(value)):
                    checked += 1
                    # A parsed float drops a trailing zero (19.40 -> 19.4).
                    self.assertTrue(str(value) in recorded or (
                        isinstance(value, (int, float)) and value in numbers), (row.uid, value))
        self.assertGreater(checked, 1000)

    def test_api_body_carries_the_whole_person_block(self):
        for row in _rows():
            if row.layer == "A" and row.surface.endswith("api_body"):
                labels = {g.label for g in row.gold}
                self.assertEqual(labels - {"CUSTOMER_ID", "EMPLOYEE_ID", "RECORD_ID"},
                                 {"GIVENNAME", "SURNAME", "EMAIL"})

    def test_escaped_payload_gold_sits_inside_the_outer_string(self):
        for row in _rows():
            if row.layer != "A" or not row.surface.endswith(("tool_result", "openai_tool_call")):
                continue
            encoded = row.text.encode("utf-8")
            gold = next(g for g in row.gold if g.label.endswith("_ID"))
            self.assertEqual(encoded[gold.start - 2:gold.start], b'\\"')
            self.assertEqual(encoded[gold.end:gold.end + 2], b'\\"')

    def test_escaped_ids_keep_the_escaped_bytes_as_gold(self):
        seen = set()
        for row in _rows():
            shape = row.surface.removeprefix(cells.SURFACE_PREFIX)
            # Layer A records the ID as gold, layer D as a benign decoy.
            for span in (*row.gold, *row.decoys):
                if not span.value.startswith(("SYN/", "SYN\\")):
                    continue
                decoded = _decoded(shape, span.value)
                self.assertRegex(decoded, r"^SYN/[68]\d{4}\u2013\d{3}$")
                expected = {"csv": decoded}.get(shape, decoded.replace("/", "\\/").replace("\u2013", "\\u2013"))
                if shape in cells.DOUBLE_ESCAPED:
                    expected = expected.replace("\\", "\\\\")
                self.assertEqual(span.value, expected, row.uid)
                self.assertEqual(span in row.gold, row.layer == "A", row.uid)
                seen.add((row.layer, shape))
        escaped = set(cells.SHAPES) - {"numeric_label"}
        self.assertEqual({shape for layer, shape in seen if layer == "A"}, escaped)
        self.assertEqual({shape for layer, shape in seen if layer == "D"}, escaped)

    def test_partitions_share_no_value_or_key(self):
        values = [{g.value for r in _rows(p) for g in r.gold} for p in api.PARTITIONS]
        self.assertTrue(values[0] and values[1])
        self.assertFalse(values[0] & values[1])
        keys = [{k for family in cells.KEYS[p].values() for k in family} | set(cells.PUBLIC_KEYS[p])
                for p in api.PARTITIONS]
        self.assertFalse(keys[0] & keys[1])
        self.assertFalse(set(cells.EMAIL_DOMAINS["dev"]) & set(cells.EMAIL_DOMAINS["test"]))

    def test_generator_carries_the_slice_and_v25_rebuilds_byte_for_byte(self):
        self.assertEqual(api.GENERATOR_VERSION, 26)
        for partition in api.PARTITIONS:
            generated = api.generate(partition)
            slice_rows = [r for r in generated if r.surface.startswith(cells.SURFACE_PREFIX)]
            self.assertEqual(api.corpus_bytes(slice_rows), api.corpus_bytes(_rows(partition)))
            old = api.records_as_of(25, generated)
            self.assertFalse([r for r in old if r.surface.startswith(cells.SURFACE_PREFIX)])
            self.assertEqual(hashlib.sha256(api.corpus_bytes(old)).hexdigest(), V25_CORPUS_SHA256[partition])
        self.assertEqual(hashlib.sha256((ROOT / api.HISTORICAL_CONTRACTS[25]).read_bytes()).hexdigest(),
                         V25_CONTRACT_SHA256)
        contract = api.load_contract(ROOT)
        documents = api.score.apply_scored_label_contract([r.to_document() for r in _rows()], contract)
        self.assertTrue(all(s.label in contract.scored_labels for d in documents for s in d.spans))


class SliceReaderTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.rows = _rows()
        cls.positives = [r for r in cls.rows if r.layer == "A"]

    def _measure(self, protect, refused=False):
        return reader.measure(self.rows, {r.uid: (protect(r), refused) for r in self.rows})

    def test_exact_gold_protection_keeps_string_payloads_usable_and_changes_number_types(self):
        result = self._measure(lambda r: [(g.start, g.end) for g in r.gold])
        a, d = result["A"]["totals"], result["D"]["totals"]
        self.assertEqual(a["documents"], 270)
        self.assertGreater(a["gold_bytes"], 0)
        self.assertEqual(a["leaked_gold_bytes"], 0)
        self.assertEqual(a["gold_values_protected"], a["gold_values"])
        self.assertEqual((a["false_positive_documents"], a["false_positive_values"], a["false_positive_bytes"]), (0, 0, 0))
        # A token is a string: in place of a JSON number it changes the payload's type.
        numeric = result["A"]["shapes"]["numeric_label"]
        self.assertEqual((numeric["type_changed_documents"], numeric["usable_documents"]), (30, 0))
        self.assertEqual(a["type_changed_documents"], 30)
        self.assertEqual(a["usable_documents"], 240)
        for shape, row in result["A"]["shapes"].items():
            if shape != "numeric_label":
                self.assertEqual((row["type_changed_documents"], row["usable_documents"]), (0, 30), shape)
        self.assertEqual(d["usable_documents"], 90)

    def test_refusing_a_number_payload_is_a_refusal_not_a_type_change(self):
        rows = [r for r in self.rows if r.surface.endswith("numeric_label")]
        result = reader.measure(rows, {r.uid: ([], True) for r in rows})["A"]["totals"]
        self.assertEqual((result["refused_documents"], result["type_changed_documents"], result["usable_documents"]),
                         (30, 0, 0))
        self.assertEqual(result["refused_gold_bytes"], sum(g.end - g.start for r in rows if r.layer == "A" for g in r.gold))
        self.assertGreater(result["refused_gold_bytes"], 0)

    def test_covering_a_key_name_makes_a_payload_unusable(self):
        target = next(r for r in self.positives if r.surface.endswith("gemini_function_call"))
        encoded = target.text.encode("utf-8")
        key = encoded.index(b'"', encoded.index(b'"args":{') + len(b'"args":{')) + 1
        result = self._measure(lambda r: [(g.start, g.end) for g in r.gold]
                               + ([(key, key + 3)] if r is target else []))
        a = result["A"]["totals"]
        # No decoy and no syntax byte: only the key name's bytes are covered.
        self.assertEqual((a["false_positive_values"], a["structural_bytes_covered"], a["false_positive_bytes"]), (0, 0, 3))
        self.assertEqual(a["usable_documents"], 239)
        self.assertEqual(result["A"]["shapes"]["gemini_function_call"]["usable_documents"], 29)

    def test_no_protection_leaks_every_gold_byte(self):
        a = self._measure(lambda r: [])["A"]["totals"]
        self.assertEqual(a["leaked_gold_bytes"], a["gold_bytes"])
        self.assertEqual((a["gold_values_protected"], a["gold_values_partial"]), (0, 0))

    def test_whole_document_protection_counts_every_value_and_breaks_every_payload(self):
        result = self._measure(lambda r: [(0, len(r.text.encode("utf-8")))])
        for layer, documents in (("A", 270), ("D", 90)):
            row = result[layer]["totals"]
            self.assertEqual(row["false_positive_documents"], documents)
            self.assertEqual(row["false_positive_values"], row["decoy_values"])
            self.assertGreater(row["structural_bytes_covered"], 0)
            self.assertEqual(row["usable_documents"], 0)
            self.assertEqual(sum(row["false_positive_values_by_kind"].values()), row["decoy_values"])

    def test_one_touched_decoy_is_one_value_one_document_and_one_unusable_payload(self):
        target = next(r for r in self.positives if r.surface.endswith("gemini_function_call"))
        decoy = target.decoys[0]
        result = self._measure(lambda r: [(g.start, g.end) for g in r.gold]
                               + ([(decoy.start, decoy.start + 1)] if r is target else []))
        a = result["A"]["totals"]
        self.assertEqual((a["false_positive_values"], a["false_positive_documents"], a["false_positive_bytes"]), (1, 1, 1))
        self.assertEqual(a["false_positive_values_by_kind"], {decoy.label: 1})
        self.assertEqual(a["usable_documents"], 239)
        self.assertEqual(result["A"]["shapes"]["gemini_function_call"]["usable_documents"], 29)

    def test_syntax_byte_coverage_alone_makes_a_payload_unusable(self):
        target = self.positives[0]
        self.assertFalse(target.surface.endswith("numeric_label"))
        gold = target.gold[0]
        # Gold plus its closing quote: no decoy touched, one syntax byte covered.
        result = self._measure(lambda r: [(g.start, g.end) for g in r.gold]
                               + ([(gold.end, gold.end + 1)] if r is target else []))
        a = result["A"]["totals"]
        self.assertEqual((a["false_positive_values"], a["structural_bytes_covered"]), (0, 1))
        self.assertEqual(a["usable_documents"], 239)

    def test_covering_the_backslash_before_escaped_gold_counts_both_syntax_bytes(self):
        target = next(r for r in self.positives if r.surface.endswith("anthropic_tool_result"))
        gold = next(g for g in target.gold if g.label.endswith("_ID"))
        self.assertEqual(target.text.encode("utf-8")[gold.start - 2:gold.start], b'\\"')
        result = self._measure(lambda r: [(g.start, g.end) for g in r.gold]
                               + ([(gold.start - 2, gold.start)] if r is target else []))
        shape = result["A"]["shapes"]["anthropic_tool_result"]
        self.assertEqual((shape["structural_bytes_covered"], shape["false_positive_bytes"]), (2, 2))
        self.assertEqual(shape["usable_documents"], 29)

    def test_partial_gold_and_refusals_are_counted_apart(self):
        partial = self._measure(lambda r: [(g.start, g.start + 1) for g in r.gold])["A"]["totals"]
        self.assertEqual(partial["gold_values_partial"], partial["gold_values"])
        self.assertEqual(partial["gold_values_protected"], 0)
        refused = self._measure(lambda r: [], refused=True)["A"]["totals"]
        self.assertEqual(refused["refused_documents"], 270)
        self.assertEqual(refused["gold_values_refused"], refused["gold_values"])
        self.assertEqual((refused["gold_bytes"], refused["leaked_gold_bytes"], refused["usable_documents"]), (0, 0, 0))
        exposed = self._measure(lambda r: [])["A"]["totals"]
        self.assertEqual(refused["refused_gold_bytes"], exposed["gold_bytes"])

    def test_missing_observation_and_older_generator_fail_closed(self):
        with self.assertRaises(records.RecordError):
            reader.measure(self.rows, {})
        header = {"scorecard": {"layers": {"generator": {"generator_version": 25}}}, "documents": []}
        with self.assertRaisesRegex(records.RecordError, "predates"):
            reader._check_inventory(header, self.rows)
        manifest = api.manifest("test", api.generate("test"))
        header["scorecard"]["layers"]["generator"] = manifest
        documents = [{"id": r.uid, "layer": r.layer, "utf8_bytes": len(r.text.encode()),
                      "gold": [[g.start, g.end, g.label] for g in r.gold]} for r in self.rows]
        header["documents"] = documents
        reader._check_inventory(header, self.rows)
        documents[0]["utf8_bytes"] += 1
        with self.assertRaisesRegex(records.RecordError, "inventory"):
            reader._check_inventory(header, self.rows)
        documents[0]["utf8_bytes"] -= 1
        documents[0]["gold"] = []
        with self.assertRaisesRegex(records.RecordError, "inventory"):
            reader._check_inventory(header, self.rows)
        documents[0]["gold"] = [[g.start, g.end, g.label] for g in self.rows[0].gold]
        reader._check_inventory(header, self.rows)
        header["scorecard"]["layers"]["generator"] = {**manifest, "corpus_sha256": "0" * 64}
        with self.assertRaisesRegex(records.RecordError, "different generated corpus"):
            reader._check_inventory(header, self.rows)

    def test_markdown_lists_every_shape_and_both_layers(self):
        text = reader.markdown({"config": "x", "layers": self._measure(lambda r: [])})
        for shape in cells.SHAPES:
            self.assertEqual(text.count(f"| {shape} |"), 2)
        self.assertEqual(text.count("**total**"), 2)
        self.assertIn("| Refused gold bytes |", text)
        self.assertIn("| Type changed |", text)


if __name__ == "__main__":
    unittest.main()
