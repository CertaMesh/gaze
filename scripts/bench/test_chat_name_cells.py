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
    ("dev", "58333493d84c3753eb2163a14002670a95d010260c1bca495579d30034afacc6"),
    ("test", "5c259f8e24cbf4f691cddafbb4ac61f90d4b5112d0bfdc40cf90d73517b873cc"),
])
def test_v32_corpus_and_contract_are_frozen(partition, digest):
    rows = api.records_as_of(32, api.generate(partition))
    assert hashlib.sha256(api.corpus_bytes(rows)).hexdigest() == digest
    assert api.load_contract(ROOT, version=32).sha256 == "8c3ac7f6c8a62ac5c11af9897cfadbcccdfd8b1a9a7a784d36336958ceb1b3c1"


@pytest.mark.parametrize("partition,chat_v32,chat_v33", [
    ("dev", "14c6d8bdf320efce893ce993e9b3c2da61f72d73dc097d7044c2361998f5896f",
     "408d1a6720d2d2e33ddc3d62faa8eeca6c484886a1d7d3d19f89649a6eb9cd2b"),
    ("test", "0bc390fe15428d1c69d4250ae1e134b9f7102f97dc30a5ac9874affc204d980f",
     "753a918d23ce7eebd8cce61bed415d9e65aa3a547c98090f0023ed2b4c2f34bb"),
])
def test_integrated_cells_preserve_chat_prereleases(partition, chat_v32, chat_v33):
    integrated = api.records_as_of(34, api.generate(partition))
    # The chat branch extended v31 independently. Never alias either prerelease to main's v32.
    prior = [r for r in integrated if not r.surface.startswith(("address_gap_", "inventory_count_"))]
    assert hashlib.sha256(api.corpus_bytes(prior)).hexdigest() == chat_v33
    without_boundaries = [r for r in prior if not r.surface.startswith(cells.BOUNDARY_PREFIX)]
    assert hashlib.sha256(api.corpus_bytes(without_boundaries)).hexdigest() == chat_v32


@pytest.mark.parametrize("partition,main_v33,chat_v34", [
    ("dev", "01f148c88b6f438f66cea88e98ef5d160480b7c2fcb23250473ee16badbc2b6a",
     "53b40c4d505a17f802fb7790c0050b20c95e5c2b1499cba1111efab6300544ca"),
    ("test", "30c18e267516f1203e9592285a83d5a1563c0f7365618024fb101e6840e95ee2",
     "92db838aa2b9d4e31f7a9aa71b670b0c9ba947f36583a8032c9d0d1a729c5b1d"),
])
def test_prose_id_and_chat_histories_remain_distinct(partition, main_v33, chat_v34):
    rows = api.generate(partition)
    for version, expected in ((33, main_v33), (34, chat_v34)):
        assert hashlib.sha256(api.corpus_bytes(api.records_as_of(version, rows))).hexdigest() == expected


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
