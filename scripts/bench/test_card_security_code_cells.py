"""Generator v32 ownership, UTF-8 source boundaries and v31 reproducibility."""
import hashlib
from itertools import product
from pathlib import Path
import re

import pytest

import agentic_layers as api
import card_security_code_cells as cells


@pytest.mark.parametrize("partition", api.PARTITIONS)
def test_gold_and_public_twins(partition):
    forms = ("bare", "label", "postlabel", "pre_owner", "post_owner")
    layouts = {(4, 4, 4), (4, 4, 4, 4), (4, 6, 5), (4, 6, 4), (4, 4, 4, 4, 3), *((n,) for n in range(12, 20))}
    positive, public, other = set(), set(), set()
    for row in cells.records(api, partition):
        assert row.surface.startswith(cells.SURFACE_PREFIX)
        encoded = row.text.encode()
        end = 0
        for span in sorted(row.gold + row.decoys, key=lambda s: s.start):
            assert end <= span.start < span.end <= len(encoded)
            assert encoded[span.start:span.end] == span.value.encode()
            end = span.end
        if row.surface.startswith(cells.BOUNDARY_PREFIX):
            continue  # Tested as paired disclosure/public-reference records below.
        if row.layer == "A":
            pan = row.gold[0]
            assert pan.label == "CREDITCARDNUMBER"
            assert api.luhn_valid(pan.value) == (row.validity == api.VALID)
            layout = tuple(map(len, re.findall(r"\d+", pan.value)))
            if sum(layout) < 14:
                assert re.match(r"5[06-9]|6[0-9]", pan.value)
            elif len(layout) == 1:
                prefix = r"(?:3|5[06-9]|6[0-9])" if layout[0] < 16 else r"[2-6]"
                assert re.match(prefix, pan.value)
            assert api.invalid_twin_credited(api.CUE_FAMILY_LABELS[row.family], row.surface)
            form, = [f for f in forms if row.surface.endswith("_" + f)]
            if form.endswith("owner"):
                assert len(row.gold) == 1
                short, = row.decoys
                assert short.label == "public_number"
                assert re.search(r"page|invoice|rechnung|seite|belegnummer", row.text, re.I)
            else:
                _, short = row.gold
                assert short.label == "SECURITYTOKEN" and not row.decoys
                between = encoded[pan.end:short.start].decode()
                tail = encoded[short.end:].decode()
                if form == "label":
                    assert re.fullmatch(r"; CV[VC]: ", between) and not tail
                elif form == "postlabel":
                    assert between == " " and re.fullmatch(r" \(CV[VC]\)", tail)
                else:
                    assert between == " " and not tail
            assert re.fullmatch(r"[0-9]{3,4}", short.value)
            positive.add((row.language, layout, len(short.value), row.validity, form))
        else:
            assert row.layer == "D" and not row.gold
            if row.family == cells.REFERENCE_TWIN:
                ref, number = row.decoys
                assert (ref.label, number.label) == ("public_reference", "public_number")
                assert not api.luhn_valid(ref.value)
                assert re.match(r"Invoice|Batch|Beleg|Charge", row.text)
                assert re.search(r"\((page|sheet number|Seite|Blattnummer)\)$", row.text)
                public.add((row.language, tuple(map(len, re.findall(r"\d+", ref.value))), len(number.value)))
            else:
                form = row.surface.removeprefix(cells.SURFACE_PREFIX)
                if form == "amount":
                    assert re.fullmatch(r"\d{3,4}[.,]\d{2}", row.decoys[0].value)
                other.add((row.language, form))
    assert positive == set(product(("en", "de"), layouts, (3, 4), (api.VALID, api.INVALID), forms))
    assert public == set(product(("en", "de"), layouts, (3, 4)))
    assert other == set(product(("en", "de"), ("reference", "loose", "metadata", "amount")))
    assert cells.REFERENCE_TWIN in api.guard_families(32) and cells.REFERENCE_TWIN not in api.guard_families(31)


@pytest.mark.parametrize("partition", api.PARTITIONS)
def test_boundary_forms_and_paired_public_references(partition):
    rows = [r for r in cells.records(api, partition) if r.surface.startswith(cells.BOUNDARY_PREFIX)]
    groups = {r.group for r in rows}
    assert len(groups) == 2 * api.DOCS_PER_FAMILY
    seen = set()
    for group in groups:
        variants = {(r.surface.removeprefix(cells.BOUNDARY_PREFIX), r.validity): r for r in rows if r.group == group}
        help_row = variants["help", api.BENIGN]
        ref, public_number = help_row.decoys
        assert help_row.layer == "D" and not help_row.gold and not api.luhn_valid(ref.value)
        assert "\n" + ref.value in help_row.text  # No preceding card cue.
        assert re.search(r"CV[VC][ -](?:help|Hilfe|form|Formular)", help_row.text)
        code = public_number.value
        seen.add((help_row.language, len(code)))
        forms = {(f, v) for f, v in product(("parentheses", "conjunction", "note", "number"), (api.VALID, api.INVALID))}
        forms.add(("code_only", api.VALID))
        assert {k for k, r in variants.items() if r.layer == "A"} == forms
        for form, validity in forms:
            r = variants[form, validity]
            pan, secret = r.gold
            assert [pan.label, secret.label] == ["CREDITCARDNUMBER", "SECURITYTOKEN"] and not r.decoys
            assert secret.value == code and api.luhn_valid(pan.value) == (validity == api.VALID)
            before, rest = r.text.split(pan.value)
            if form == "code_only":
                assert pan.value.isdigit() and pan.value[:-1] == ref.value[:-1]
                assert not re.search("card|karte", before, re.I)
                assert re.fullmatch(rf" (?:CV[VC]|Prüfnummer): {code}", rest)
            elif form == "number":
                assert re.search(r"(?:card no\.|Karte Nr\.) $", before, re.I) and rest == " " + code
            elif form == "parentheses":
                assert re.fullmatch(rf" \(CV[VC]: {code}\)", rest)
            elif form == "conjunction":
                if partition == "test":
                    assert re.fullmatch(rf" (?:and the CVV is|und die Prüfnummer ist) {code}", rest)
                else:
                    assert re.fullmatch(rf" (?:and CVV|und Prüfnummer): {code}", rest)
            else:
                assert re.fullmatch(rf"[;,] CV[VC]: {code} \([^)]+\)", rest)
        if len(code) == 4:
            long = variants["long", api.BENIGN]
            assert long.layer == "D" and not long.gold
            reference, = long.decoys
            assert re.fullmatch(r"[2-6]\d{3}(?:-\d{4}){4}", reference.value)
            assert re.search(r"(?:Card|Kartennummer): " + reference.value + "$", long.text)
            pan, secret = variants["number", api.INVALID].gold
            assert reference.value == pan.value + "-" + secret.value
            digits = reference.value.replace("-", "")
            assert not any(api.luhn_valid(digits[i:j]) for i in range(len(digits))
                           for j in range(i + 12, min(i + 19, len(digits)) + 1))
    assert seen == set(product(("en", "de"), (3, 4)))


def test_boundary_prose_is_held_out_after_removing_the_values():
    prose = {}
    for partition in api.PARTITIONS:
        prose[partition] = set()
        for row in cells.records(api, partition):
            if row.surface.startswith(cells.BOUNDARY_PREFIX):
                text = row.text.encode()
                for span in sorted(row.gold + row.decoys, key=lambda s: -s.start):
                    text = text[:span.start] + b"#" + text[span.end:]
                prose[partition].add(text)
    assert prose["dev"].isdisjoint(prose["test"])


@pytest.mark.parametrize("partition,expected", [
    ("dev", "1a4d44deff1a09c5020ee80db548698de292bc7ca25524206fe66a846feaf27b"),
    ("test", "8380d500b949265ef5e01f9a8b801819fd8d83373474f87abcdc535a4df33944"),
])
def test_v31_rebuilds_byte_for_byte(partition, expected):
    old = api.records_as_of(31, api.generate(partition))
    assert hashlib.sha256(api.corpus_bytes(old)).hexdigest() == expected
    assert api.load_contract(Path(__file__).resolve().parents[2], version=31).sha256 == (
        "9480508e4ddeca0c4ce82e40acede4c18aca64f318dfdc75eeb04457ee7cd695")
