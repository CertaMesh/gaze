"""Check wire spelling, single-pass decoding and source ownership without running a detector."""
from collections import Counter
import hashlib
import html
from html.parser import HTMLParser
import ipaddress
import json
from pathlib import Path
import re

import pytest

import agentic_layers as api
import escaped_separator_cells as cells


class TextNodes(HTMLParser):
    def __init__(self):
        super().__init__()
        self.nodes = []

    def handle_data(self, data):
        self.nodes.append(data)


def strings(value):
    if isinstance(value, str):
        yield value
    else:
        for child in value.values() if isinstance(value, dict) else value:
            yield from strings(child)


@pytest.mark.parametrize("partition", api.PARTITIONS)
def test_carriers_decode_once_and_raw_source_bytes_are_fully_annotated(partition):
    rows = cells.records(api, partition)
    assert len({r.uid for r in rows}) == len(rows)
    assert Counter(r.layer for r in rows) == {"A": 1290, "D": 1494, "R": 1290}
    # The non-matrix cases have different carrier and decoding contracts below.
    rows = [r for r in rows if not r.surface.startswith(cells.SURFACE_PREFIX + "carrier_")]
    by_surface = Counter((r.surface, r.layer) for r in rows)
    assert by_surface.pop((cells.SURFACE_PREFIX + "literal_zwsp_views", "D")) == 10
    assert len(by_surface) == 32 * 3
    for (surface, layer), count in by_surface.items():
        assert count == (52 if layer == "D" and not surface.endswith("_inside") else 40)
    for r in rows:
        assert r.surface.startswith(cells.SURFACE_PREFIX)
        if r.surface.endswith("literal_zwsp_views"):
            assert r.layer == "D" and not r.gold
            span, = r.decoys
            assert re.fullmatch(r"[1-9]\d\u200b\d{3}\u200b\d{3}", span.value)
            assert span.value + " views" in r.text
            assert r.text.encode()[span.start:span.end].decode() == span.value
            continue
        kind, syntax, codepoint = r.surface.removeprefix(cells.SURFACE_PREFIX).removesuffix("_inside").split("_")
        glyph = chr(int(codepoint, 16))
        assert all(chr(cp) not in r.text for cp, _ in cells.CONTROLS)
        if kind == "json":
            nodes = list(strings(json.loads(r.text)))
        else:
            parser = TextNodes()
            parser.feed(r.text)
            parser.close()
            nodes = parser.nodes
        assert len(r.gold) == {"A": 1, "D": 0, "R": 2}[r.layer]
        assert len(r.decoys) == (1 if r.layer == "D" else 0)
        spans = r.gold or r.decoys
        raw = spans[0].value
        assert raw.isascii()
        expected = json.loads('"' + raw + '"') if kind == "json" else html.unescape(raw)
        if r.family == "notation":
            assert glyph not in expected and expected.startswith(("\\u", "&"))
        else:
            assert glyph in expected
            pattern = {"unicode": r"\\u[\da-fA-F]{4}", "decimal": r"&#\d+;",
                       "hex": r"&#x[\da-fA-F]+;", "named": r"&\w+;"}[syntax]
            assert re.search(pattern, raw)
        assert sum(node.endswith(expected) for node in nodes) == len(spans)
        assert all(s.value == raw for s in spans)
        assert [(m.start(), m.end()) for m in re.finditer(re.escape(raw.encode()), r.text.encode())] == [
            (s.start, s.end) for s in spans]


@pytest.mark.parametrize("partition", api.PARTITIONS)
def test_targeted_carrier_locations_and_source_ownership(partition):
    positive = {"json_before_cue", "json_phone_space", "html_doctype", "html_zwj", "json_bom"}
    negative = {"json_double", "html_double", "json_key", "html_attribute", "html_comment", "html_script"}
    prefix = cells.SURFACE_PREFIX + "carrier_"
    rows = [r for r in cells.records(api, partition) if r.surface.startswith(prefix)]
    assert Counter((r.surface.removeprefix(prefix), r.layer) for r in rows) == {
        **{(s, layer): 2 for s in positive for layer in "AR"}, **{(s, "D"): 2 for s in negative}}
    ssns = {}
    for r in rows:
        case = r.surface.removeprefix(prefix)
        assert len(r.gold) == {"A": 1, "R": 2, "D": 0}[r.layer]
        assert len(r.decoys) == (r.layer == "D")
        spans = r.gold or r.decoys
        raw = spans[0].value
        assert raw.isascii() and all(s.value == raw for s in spans)
        assert [(s.start, s.end) for s in spans] == [
            (m.start(), m.end()) for m in re.finditer(re.escape(raw.encode()), r.text.encode())]
        if case.startswith("json_"):
            doc = json.loads(r.text)
            decoded = json.loads('"' + raw + '"')
            if case == "json_key":
                assert list(doc["schema"]) == ["SSN: " + decoded]
                assert all(decoded not in v for v in strings(doc))
            else:
                assert sum(v.endswith(decoded) for v in strings(doc)) == len(spans)
        else:
            decoded = html.unescape(raw)
            parser = TextNodes()
            parser.feed(r.text)
            if case in positive or case == "html_double":
                assert sum(n.endswith(decoded) for n in parser.nodes) == len(spans)
            elif case == "html_attribute":
                assert parser.nodes == ["Preview"] and f'data-sample="SSN: {raw}"' in r.text
            elif case == "html_comment":
                assert parser.nodes == ["Preview"] and f"<!-- SSN: {raw} -->" in r.text
            elif case == "html_script":
                # Script data is raw text: no HTML entity decoding takes place here.
                assert parser.nodes == [f'const sample = "SSN: {raw}";'] and "<script>" in r.text
        if case == "json_phone_space":
            assert r"\t" in raw and all(s.label == "TELEPHONENUM" for s in spans)
            assert re.fullmatch(r"\+1 \((?:312|415)\)\t5\u200b55-01\d{2}", decoded)
            continue
        if r.gold:
            assert all(s.label == "SSN" for s in spans)
        if case == "json_double":
            assert r"\\u200b" in raw and r"\u200b" in decoded and "\u200b" not in decoded
            decoded = json.loads('"' + decoded + '"')
        elif case == "html_double":
            assert "&amp;#8203;" in raw and "&#8203;" in decoded and "\u200b" not in decoded
            decoded = html.unescape(decoded)
        elif case == "html_doctype":
            assert r.text.startswith("<!DOCTYPE html>SSN: " + raw)
        elif case == "json_before_cue":
            assert r"\nSSN: " + raw in r.text and any("\nSSN: " + decoded in v for v in strings(doc))
        glyph = {"html_zwj": "\u200d", "json_bom": "\ufeff"}.get(case, "\u200b")
        assert re.fullmatch(r"9\d\d-\d" + glyph + r"\d-\d{4}", decoded)
        ssns.setdefault(case, set()).add(decoded.replace(glyph, ""))
    assert len(ssns["json_before_cue"]) == 2
    assert all(values == ssns["json_before_cue"] for values in ssns.values())


@pytest.mark.parametrize("partition", api.PARTITIONS)
def test_identifiers_are_unassignable_or_reserved_and_twins_benign(partition):
    # Inspect decoded identifiers separately from the carrier/offset checks above.
    for r in cells.records(api, partition):
        if not r.surface.endswith("json_unicode_200b"):
            continue
        span = (r.gold or r.decoys)[0]
        value = json.loads('"' + span.value + '"').replace("\u200b", "")
        if r.gold:
            assert span.label == {"ip": "IPADDRESS", "ssn": "SSN", "iban": "IBAN", "phone": "TELEPHONENUM"}[r.family]
        if r.family == "ip":
            assert ipaddress.ip_address(value) in ipaddress.ip_network("10.0.0.0/8")
        elif r.family == "ip_twin":
            assert any(ipaddress.ip_address(value) in ipaddress.ip_network(net) for net in ("192.0.2.0/24", "198.51.100.0/24"))
        elif r.family in ("ssn", "ssn_twin"):
            assert re.fullmatch(r"9\d{2}-\d{2}-\d{4}", value)
            assert ("SSN" in r.text) == (r.family == "ssn")
        elif r.family == "iban":
            assert value[4:12] == "00000000" and api.iban_valid(value)
        elif r.family == "iban_twin":
            assert value.startswith("ZZ") and not api.iban_valid(value)
        elif r.family == "phone":
            assert re.fullmatch(r"\+1 \((?:312|415)\) 555-01\d{2}", value)
        elif r.family == "phone_twin":
            assert re.fullmatch(r"\+0 \(000\) 555-01\d{2}", value)
        elif r.family == "views":
            assert re.fullmatch(r"[1-9]\d\\u200[bB]\d{3}\\u200[bB]\d{3}", span.value)
            assert "views" in r.text


@pytest.mark.parametrize("partition", api.PARTITIONS)
def test_interior_cut_and_group_boundaries_reconstruct_the_same_value(partition):
    rows = cells.records(api, partition)
    natural = {(r.group, r.surface, r.layer): r for r in rows if not r.surface.endswith("_inside")}
    positions = {family: set() for family in ("ip", "ssn", "iban", "phone")}
    for r in rows:
        if not r.surface.endswith("_inside"):
            continue
        surface = r.surface.removesuffix("_inside")
        counterpart = natural[r.group, surface, r.layer]
        glyph = chr(int(surface.split("_")[-1], 16))
        def decode(row):
            raw = (row.gold or row.decoys)[0].value
            return json.loads('"' + raw + '"') if "_json_" in surface else html.unescape(raw)
        grouped = decode(counterpart)
        assert grouped.count(glyph) >= 2
        left, right = decode(r).split(glyph)
        assert left[-1].isdigit() and right[0].isdigit()
        assert grouped.replace(glyph, "") == left + right
        family = r.family.removesuffix("_twin")
        if family == "ip":
            assert left.count(".") == 3 and right.isdigit()
            offset = len(left.rsplit(".", 1)[1])
        elif family == "ssn":
            assert left.count("-") == 1 and right.count("-") == 1
            offset = len(left.split("-")[1])
        elif family == "phone":
            assert left.split(") ")[1] + right.split("-")[0] == "555"
            offset = len(left.split(") ")[1])
        else:
            assert family == "iban" and 12 < len(left) < 20 and len(left) % 4
            offset = len(left) % 4
        positions[family].add(offset)
    assert positions == {"ip": {1, 2}, "ssn": {1}, "phone": {1, 2}, "iban": {1, 2, 3}}


def test_partitions_are_disjoint_and_reproducible():
    sets = []
    for partition, digest in (("dev", "f4dcb1e6ceacfc418f6e56320b50df06ce729e3b5210301c6ee177ce2fe07740"),
                              ("test", "06a7c5dc31fa3648b16391532cf8b46c5e94fe27e96ea98ded26ba795ec09735")):
        rows = cells.records(api, partition)
        assert hashlib.sha256(api.corpus_bytes(rows)).hexdigest() == digest
        values = {json.loads('"' + r.gold[0].value + '"').replace("\u200b", "")
                  for r in rows if r.layer == "A" and r.surface.endswith("json_unicode_200b")}
        assert len(values) == 4 * api.DOCS_PER_FAMILY
        sets.append(({r.text for r in rows}, values))
    assert all(a.isdisjoint(b) for a, b in zip(*sets))


def test_integrated_generator_includes_source_rows_and_rebuilds_v32():
    root = Path(__file__).resolve().parents[2]
    assert api.load_contract(root, version=32).sha256 == "a49ca4d597ec9666b16884214df0f9f8b9e9b0f30bcd3e227dabe93fe2e693c5"
    for partition, digest in (("dev", "521676aff4c3fa5a482bb3f8dff86572d23d1654393102a13c8477dc6e559d23"),
                              ("test", "b21073eca54d70bd53ba49aea7e924f6e028077a39e17a57cf8910dca8d95fb0")):
        rows = api.generate(partition)
        added = [r for r in rows if r.surface.startswith(cells.SURFACE_PREFIX)]
        assert added == cells.records(api, partition)
        frozen = api.records_as_of(32, rows)
        assert hashlib.sha256(api.corpus_bytes(frozen)).hexdigest() == digest
