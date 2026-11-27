"""Pin the source-authored record-ID labels and their false-positive counterweights."""
from collections import Counter
import hashlib
import json
from pathlib import Path
import re

import pytest

import agentic_layers as api
import prose_record_id_cells as cells


@pytest.mark.parametrize("partition", api.PARTITIONS)
def test_whole_value_gold_and_public_twins(partition):
    records = cells.records(api, partition)
    assert Counter(r.layer for r in records) == {"A": 770, "D": 535}
    personal, public, selectors, names, numeric = set(), set(), set(), set(), set()
    for r in records:
        assert r.surface.startswith(cells.SURFACE_PREFIX)
        assert not r.decoys
        if r.layer == "A":
            g, = r.gold
            assert g.label == r.family.upper()
            assert r.text.encode()[g.start:g.end].decode() == g.value
            if r.surface.endswith("_digits"):
                assert g.value.isdigit()
                assert ("case " if r.language == "en" else "Fall-Nr.") + g.value in r.text
                numeric.add(g.value)
            else:
                # Guard against crediting only the serial or the first group.
                token, = re.findall(r"[A-Za-z]+[-_]?\d+(?:[/_][A-Z]\d+)?", r.text)
                assert token == g.value
            if "_uncued_" in r.surface:
                personal.add(g.value)
                assert not re.search(r"(?i)\b(?:id|case|claim|employee|account|nummer)\b", r.text.replace(g.value, ""))
        else:
            assert not r.gold
            if r.family == "public_reference":
                token, = re.findall(r"[A-Za-z]+[-_]?\d+(?:[/_][A-Z]\d+)?", r.text)
                public.add(token)
            if r.family == "tool_name":
                names.add(json.loads(r.text)["name"])
            if r.family == "tool_selector":
                tool = json.loads(r.text)
                args = tool.get("arguments", tool.get("input"))
                code = args.get("tag", args.get("reference"))
                assert "/" + code + "/" in args.get("selector", args.get("path"))
                selectors.add(code)
    assert personal == public
    assert selectors == personal | numeric
    assert names == {v for v in personal if "/" not in v}


def test_partitions_have_disjoint_values_and_wording():
    dev, test = [cells.records(api, p) for p in api.PARTITIONS]
    values = [{g.value for r in rows for g in r.gold} for rows in (dev, test)]
    assert values[0].isdisjoint(values[1])
    def wording(rows, identifiers):
        pattern = re.compile("|".join(map(re.escape, sorted(identifiers, key=len, reverse=True))))
        return {pattern.sub("{V}", r.text) for r in rows}
    assert wording(dev, values[0]).isdisjoint(wording(test, values[1]))


def test_v31_contract_and_both_corpora_rebuild_byte_for_byte():
    root = Path(__file__).resolve().parents[2]
    assert hashlib.sha256((root / api.HISTORICAL_CONTRACTS[31]).read_bytes()).hexdigest() == (
        "9480508e4ddeca0c4ce82e40acede4c18aca64f318dfdc75eeb04457ee7cd695")
    for partition, expected in (
        ("dev", "1a4d44deff1a09c5020ee80db548698de292bc7ca25524206fe66a846feaf27b"),
        ("test", "8380d500b949265ef5e01f9a8b801819fd8d83373474f87abcdc535a4df33944"),
    ):
        old = api.records_as_of(31, api.generate(partition))
        assert hashlib.sha256(api.corpus_bytes(old)).hexdigest() == expected
