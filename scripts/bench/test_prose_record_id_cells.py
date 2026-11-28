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
    assert Counter(r.layer for r in records) == {"A": 800, "D": 605}
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


@pytest.mark.parametrize("partition", api.PARTITIONS)
def test_case_plus_digits_has_exact_switch_and_test_case_twins(partition):
    records = cells.records(api, partition)
    positive = Counter(r.gold[0].value for r in records if r.surface.endswith("_case_digits"))
    assert positive.total() == api.DOCS_PER_FAMILY
    for surface, syntax in (("switch", r"switch \(\w+\) \{ case (\d+):"),
                            ("test_case", r"\btest case (\d+)\b")):
        twins = [r for r in records if r.family == "public_case" and r.surface.endswith("_" + surface)]
        assert all(r.layer == "D" and not r.gold for r in twins)
        assert Counter(n for r in twins for n in re.findall(syntax, r.text)) == positive


@pytest.mark.parametrize("partition", api.PARTITIONS)
def test_same_login_spelling_in_public_release_and_build_tags(partition):
    rows = cells.records(api, partition)
    personal = {r.gold[0].value for r in rows if r.surface.endswith("_ownership_login")}
    assert len(personal) == api.DOCS_PER_FAMILY
    for surface, syntax in (("release", r"Release (ACCT_\d{5})\b"),
                            ("quoted", r"`(ACCT_\d{5})`"), ("build", r"Build tag: (ACCT_\d{5})\b")):
        controls = [r for r in rows if r.surface.endswith("_ownership_" + surface)]
        assert all(r.layer == "D" and not r.gold and not r.decoys for r in controls)
        assert Counter(v for r in controls for v in re.findall(syntax, r.text)) == Counter(personal)
    assert any(r.text == "Release ACCT_57293" for r in rows) == (partition == "dev")


@pytest.mark.parametrize("partition", api.PARTITIONS)
@pytest.mark.parametrize("case,stem", [("lower", "[a-z]+"), ("title", "[A-Z][a-z]+")])
def test_word_counter_login_and_software_metadata_have_identical_values(partition, case, stem):
    rows = cells.records(api, partition)
    surface = cells.SURFACE_PREFIX + f"counter_{case}_"
    syntax = re.compile(rf"\b{stem}_[0-9]{{5}}\b")
    logins = [r for r in rows if r.surface == surface + "login"]
    counters = [r for r in rows if r.surface == surface + "metadata"]
    assert len(logins) == len(counters) == api.DOCS_PER_FAMILY
    personal = Counter()
    for r in logins:
        g, = r.gold
        assert r.layer == "A" and g.label == "RECORD_ID" and syntax.findall(r.text) == [g.value]
        personal[g.value] += 1
    assert len(personal) == len(logins)
    assert all(r.layer == "D" and not r.gold and not r.decoys for r in counters)
    observed = []
    for r in counters:
        value, = syntax.findall(r.text)
        observed.append(value)
    assert Counter(observed) == personal


def test_partitions_have_disjoint_values_and_wording():
    dev, test = [cells.records(api, p) for p in api.PARTITIONS]
    values = [{g.value for r in rows for g in r.gold} for rows in (dev, test)]
    assert values[0].isdisjoint(values[1])
    def wording(rows, identifiers):
        pattern = re.compile("|".join(map(re.escape, sorted(identifiers, key=len, reverse=True))))
        return {pattern.sub("{V}", r.text) for r in rows}
    assert wording(dev, values[0]).isdisjoint(wording(test, values[1]))


def test_v32_contract_and_both_corpora_rebuild_byte_for_byte():
    root = Path(__file__).resolve().parents[2]
    assert hashlib.sha256((root / api.HISTORICAL_CONTRACTS[32]).read_bytes()).hexdigest() == (
        "8c3ac7f6c8a62ac5c11af9897cfadbcccdfd8b1a9a7a784d36336958ceb1b3c1")
    for partition, expected in (
        ("dev", "58333493d84c3753eb2163a14002670a95d010260c1bca495579d30034afacc6"),
        ("test", "5c259f8e24cbf4f691cddafbb4ac61f90d4b5112d0bfdc40cf90d73517b873cc"),
    ):
        old = api.records_as_of(32, api.generate(partition))
        assert hashlib.sha256(api.corpus_bytes(old)).hexdigest() == expected
