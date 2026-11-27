"""Check wire spelling, single-pass decoding and source ownership without running a detector."""
from collections import Counter
import hashlib
import html
from html.parser import HTMLParser
import ipaddress
import json
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
    assert Counter(r.layer for r in rows) == {"A": 640, "D": 832, "R": 640}
    by_surface = Counter((r.surface, r.layer) for r in rows)
    assert len(by_surface) == 16 * 3
    for (surface, layer), count in by_surface.items():
        assert count == {"A": 40, "D": 52, "R": 40}[layer]
    for r in rows:
        assert r.surface.startswith(cells.SURFACE_PREFIX)
        kind, syntax, codepoint = r.surface.removeprefix(cells.SURFACE_PREFIX).split("_")
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
        elif r.family in ("phone", "phone_twin"):
            prefix = "1" if r.family == "phone" else "0"
            assert re.fullmatch(r"\+" + prefix + r"(?:312|415)55501\d{2}", value)
        elif r.family == "views":
            assert re.fullmatch(r"[1-9]\\u200[bB]\d{3}\\u200[bB]\d{3}", span.value)
            assert "views" in r.text


def test_partitions_are_disjoint_and_reproducible():
    sets = []
    for partition, digest in (("dev", "9b8063ce07adcd89ef1236827e51255188c9e5ee422f358bf95eb2f48a26911c"),
                              ("test", "1703c2cd0cdfd7e4cfa5623c2c50123b159a8301a54969a4ab788aaf95eff577")):
        rows = cells.records(api, partition)
        assert hashlib.sha256(api.corpus_bytes(rows)).hexdigest() == digest
        values = {json.loads('"' + r.gold[0].value + '"').replace("\u200b", "")
                  for r in rows if r.layer == "A" and r.surface.endswith("json_unicode_200b")}
        assert len(values) == 4 * api.DOCS_PER_FAMILY
        sets.append(({r.text for r in rows}, values))
    assert all(a.isdisjoint(b) for a, b in zip(*sets))
