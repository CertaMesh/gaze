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


@pytest.mark.parametrize("partition,expected", [
    ("dev", "1a4d44deff1a09c5020ee80db548698de292bc7ca25524206fe66a846feaf27b"),
    ("test", "8380d500b949265ef5e01f9a8b801819fd8d83373474f87abcdc535a4df33944"),
])
def test_v31_rebuilds_byte_for_byte(partition, expected):
    old = api.records_as_of(31, api.generate(partition))
    assert hashlib.sha256(api.corpus_bytes(old)).hexdigest() == expected
    assert api.load_contract(Path(__file__).resolve().parents[2], version=31).sha256 == (
        "9480508e4ddeca0c4ce82e40acede4c18aca64f318dfdc75eeb04457ee7cd695")
