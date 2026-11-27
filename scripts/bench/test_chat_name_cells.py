"""Check the chat producer's labels without running a recognizer."""
from collections import Counter
import hashlib
from pathlib import Path
import re

import pytest

import agentic_layers as api
import chat_name_cells as cells

ROOT = Path(__file__).resolve().parents[2]
V31_SHA256 = {
    "dev": "1a4d44deff1a09c5020ee80db548698de292bc7ca25524206fe66a846feaf27b",
    "test": "8380d500b949265ef5e01f9a8b801819fd8d83373474f87abcdc535a4df33944",
}


@pytest.mark.parametrize("partition", api.PARTITIONS)
def test_v31_corpus_and_contract_are_frozen(partition):
    rows = api.records_as_of(31, api.generate(partition))
    assert hashlib.sha256(api.corpus_bytes(rows)).hexdigest() == V31_SHA256[partition]
    assert api.load_contract(ROOT, version=31).sha256 == "9480508e4ddeca0c4ce82e40acede4c18aca64f318dfdc75eeb04457ee7cd695"


@pytest.mark.parametrize("partition", api.PARTITIONS)
@pytest.mark.parametrize("language", ["en", "de"])
def test_chat_gold_and_counterweights(partition, language):
    rows = [r for r in cells.records(api, partition) if r.language == language]
    assert Counter(r.layer for r in rows) == {"A": 63, "D": 9}
    given, surname, word, established_given, established_surname = cells.CAST[partition, language]
    assert established_given in api.GIVEN_NAMES[partition] and given not in api.GIVEN_NAMES[partition]
    assert established_surname in api.SURNAMES[partition] and surname not in api.SURNAMES[partition]
    if language == "en":
        asset = ROOT / "crates/gaze-recognizers/assets/record-common-name-words-en-v1.txt"
        words = set(asset.read_text().splitlines())
        assert word.lower() in words
        for cue in (r"\bask", r"\bcc", r"@\w+bot"):
            following = {w for r in rows if r.layer == "D"
                         for w in re.findall(cue + r" (\w+)", r.text.casefold())}
            assert following & words, cue
    for row in rows:
        assert not row.decoys
        if row.layer == "D":
            assert not row.gold
        else:
            labels = [g.label for g in row.gold]
            if "_pair_" in row.surface:
                assert labels == ["GIVENNAME", "SURNAME"]
                assert row.gold[1].start == row.gold[0].end + 1
            else:
                assert labels == ["GIVENNAME"]
            for g in row.gold:
                assert row.text.encode()[g.start:g.end].decode() == g.value
        for mention in re.findall(r"@\w+", row.text):
            assert mention in {"@guestlistbot", "@rosterbot", "@gaestebot", "@planungsbot"}
    for cue in ("intro", "ask", "cc", "after_handle"):
        assert {r.layer for r in rows if f"_{cue}_" in r.surface} == {"A", "D"}
    intros = [r for r in rows if "_intro_" in r.surface]
    assert any([g.value for g in r.gold] == [word.lower()] for r in intros)
    assert any(r.layer == "D" and word.lower() in r.text.lower().split() for r in intros)


def test_partitions_have_disjoint_text_and_values():
    dev, test = [cells.records(api, p) + cells.boundary_records(api, p) for p in api.PARTITIONS]
    assert not {r.text for r in dev} & {r.text for r in test}
    assert not {g.value.casefold() for r in dev for g in r.gold} & {
        g.value.casefold() for r in test for g in r.gold}


@pytest.mark.parametrize("partition,digest", [
    ("dev", "14c6d8bdf320efce893ce993e9b3c2da61f72d73dc097d7044c2361998f5896f"),
    ("test", "0bc390fe15428d1c69d4250ae1e134b9f7102f97dc30a5ac9874affc204d980f"),
])
def test_v32_corpus_and_contract_are_frozen(partition, digest):
    rows = api.records_as_of(32, api.generate(partition))
    assert hashlib.sha256(api.corpus_bytes(rows)).hexdigest() == digest
    assert api.load_contract(ROOT, version=32).sha256 == "882911932297d6f459d34f5971a9ec7266eda9290a23afcf838f88d4776366d1"


@pytest.mark.parametrize("partition", api.PARTITIONS)
@pytest.mark.parametrize("language", ["en", "de"])
def test_boundary_gold_ends_at_the_name(partition, language):
    rows = [r for r in cells.boundary_records(api, partition) if r.language == language]
    assert Counter(r.layer for r in rows) == {"A": 7, "D": 3}
    _, _, word, given, surname = cells.CAST[partition, language]
    for row in rows:
        shape = row.surface.removeprefix(f"{cells.BOUNDARY_PREFIX}{language}_")
        source = row.text.encode()
        if shape in ("calendar", "demonym", "word"):
            assert row.layer == "D" and not row.gold
            continue
        assert row.layer == "A"
        if shape == "word_again":
            (g,) = row.gold
            assert (g.label, g.value.casefold()) == ("GIVENNAME", word.casefold())
            assert source.count(g.value.encode()) == 2 and source.index(g.value.encode()) == g.start
        else:
            expected = [("GIVENNAME", given)]
            if shape.startswith("pair_"):
                expected.append(("SURNAME", surname))
            assert [(g.label, g.value.casefold()) for g in row.gold] == [(k, v.casefold()) for k, v in expected]
            tail = source[row.gold[-1].end:].decode()
            if shape.endswith("colon"):
                assert tail.startswith(":")
            elif shape.endswith("possessive"):
                assert re.match(r"(?:'s|’s|s|’) ", tail)
            else:
                assert shape in ("ask_verb", "handle_verb") and re.match(r" \w+ ", tail)
        for g in row.gold:
            assert source[g.start:g.end].decode() == g.value
