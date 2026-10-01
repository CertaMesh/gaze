#!/usr/bin/env python3
"""Model-free tests for the agentic benchmark layers A and D."""

import contextlib
import copy
import csv
import dataclasses
import hashlib
import io
import ipaddress
import json
import os
import re
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parent))

import agentic_layers as agentic
import gaze_bench_score as score


REPO_ROOT = Path(__file__).resolve().parents[2]

# A generator change must bump GENERATOR_VERSION, the contract's
# generator_version and these hashes together: a silent corpus change would
# make base and candidate scorecards measure different documents.
PINNED_CORPUS_SHA256 = {
    "dev": "6d1d150332a5a36e0b1225fc5af95746eddae46377ca0b1aad80eb44ea8d21d8",
    "test": "8fc8021fb70e28cdaa2c581c0ca9f6701f95c85519231709052233afa8bc0054",
}
# v8: everything before the URL cells.
V8_CORPUS_SHA256 = {
    "dev": "60c3fe121db4ce07b0dfbc2397c1a48a0c1773a5c94eff324fdde597a6aa8ae6",
    "test": "ddd234551bcae00ab0f97026fd4b5b6d3d4b4b23cf08b8fa15926e87f598bd5c",
}
# v7: everything before the cued grammar and short-identifier cells.
V7_CORPUS_SHA256 = {
    "dev": "f7d45efdb7ac5bafeaa432ec1cb413e1137a1b78454fb3422a3bdcf1887c5168",
    "test": "ac9ff6e7b47824ec22c5201e6ff900618d3408eaa823f709d85381334d6aba69",
}
# The cued card twins join the card credit guard (generator v8).
CUE_CARD_TWINS = (
    "card_twin_grouped_ref_csv", "card_twin_order", "card_twin_reader_serial",
    "card_twin_timestamp_log", "card_twin_transaction_json",
)
# v6: everything before the phone-shape cells.
V6_CORPUS_SHA256 = {
    "dev": "e1b6bc315cb52d41aaf93fd48cf9719d67e665317fc927cc9c6a5e33a3e57af7",
    "test": "9e6597c4b38a6adf6fe5b034da3a4ca585819e044d3437aafc02bcb721607d4a",
}
# v5: everything before the address cells.
V5_CORPUS_SHA256 = {
    "dev": "b9a17a2d1b57c3adaba687f5f1051ac0e1769c4814e59cd971d098bc1f34cb6c",
    "test": "9a648a1c5cbb261ba9e3503ddfa5b65cbeb0d0d489cbc3d42464851bd88d8545",
}
# v4: everything before the labelled benign-lookalike cells.
V4_CORPUS_SHA256 = {
    "dev": "6df97e1ea7fbe49a0362335de0c0436913cf77689948156a1fd7e3701a40e9ad",
    "test": "387a35ac155153e9b58a26ec7946b3d459b0d094fffdd5f05de39558eb640604",
}
PREVIOUS_CORPUS_SHA256 = {
    "dev": "4cab04e2418b5f6ffff482e84bd1c90bb523726f8d5b3aa560409071b49c8459",
    "test": "c751da0b8b7d2e9e18663ad07458d75c70b26799b1c22d71004b3e0e351dd22b",
}


class ChecksumVectorTests(unittest.TestCase):
    """Published example values for each standard, valid and invalid."""

    VALID = {
        # ISO/IEC 7812 examples and the common Visa / Mastercard test PANs.
        "luhn": ("79927398713", "4111 1111 1111 1111", "5500 0000 0000 0004"),
        # ISO 13616 / national bank association example IBANs.
        "iban": (
            "GB82 WEST 1234 5698 7654 32",
            "DE89 3704 0044 0532 0130 00",
            "FR14 2004 1010 0505 0001 3M02 606",
            "FR76 3000 6000 0112 3456 7890 189",
            "NL91 ABNA 0417 1643 00",
            "AT61 1904 3002 3457 3201",
        ),
        # BZSt / python-stdnum examples.
        "steuer_id": ("36 574 261 809", "86095742719", "47036892816"),
        "bsn": ("111222333", "123456782"),
        # NHS Digital example number.
        "nhs": ("943 476 5919", "401 023 2137"),
        "cpf": ("529.982.247-25", "111.444.777-35"),
    }
    INVALID = {
        "luhn": ("79927398710", "4111 1111 1111 1112"),
        "iban": ("GB83 WEST 1234 5698 7654 32", "DE89 3704 0044 0532 0130 0", "XX89 3704"),
        # Wrong check digit; leading zero; no repeated digit.
        "steuer_id": ("36574261890", "01234567899", "12345678903"),
        "bsn": ("123456789", "111111111", "12345678"),
        "nhs": ("943 476 5918", "000 000 0000"),
        "cpf": ("529.982.247-24", "111.111.111-11"),
    }

    def test_published_valid_vectors_pass(self) -> None:
        for kind, values in self.VALID.items():
            for value in values:
                with self.subTest(kind=kind, value=value):
                    self.assertTrue(agentic.CHECKSUMS[kind](value))

    def test_published_invalid_vectors_fail(self) -> None:
        for kind, values in self.INVALID.items():
            for value in values:
                with self.subTest(kind=kind, value=value):
                    self.assertFalse(agentic.CHECKSUMS[kind](value))

    def test_french_rib_key_matches_the_published_example(self) -> None:
        self.assertEqual(agentic.fr_rib_key("30006", "00001", "12345678901"), "89")

    def test_nhs_payload_with_check_value_ten_has_no_valid_number(self) -> None:
        payload = next(
            f"{number:09d}"
            for number in range(400000000, 400001000)
            if agentic.nhs_check_digit(f"{number:09d}") is None
        )
        for digit in "0123456789":
            self.assertFalse(agentic.nhs_valid(payload + digit))


class GeneratorTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.corpora = {partition: agentic.generate(partition) for partition in agentic.PARTITIONS}

    def test_same_seed_is_byte_identical_and_matches_the_pin(self) -> None:
        for partition, records in self.corpora.items():
            with self.subTest(partition=partition):
                again = agentic.generate(partition)
                self.assertEqual(agentic.corpus_bytes(records), agentic.corpus_bytes(again))
                self.assertEqual(
                    agentic.manifest(partition, records)["corpus_sha256"],
                    PINNED_CORPUS_SHA256[partition],
                )

    def test_previous_partition_documents_are_byte_identical(self) -> None:
        for partition, records in self.corpora.items():
            v8 = agentic.records_as_of(8, records)
            self.assertEqual(
                hashlib.sha256(agentic.corpus_bytes(v8)).hexdigest(), V8_CORPUS_SHA256[partition]
            )
            v7 = agentic.records_as_of(7, records)
            self.assertEqual(
                hashlib.sha256(agentic.corpus_bytes(v7)).hexdigest(), V7_CORPUS_SHA256[partition]
            )
            v6 = agentic.records_as_of(6, records)
            self.assertEqual(
                hashlib.sha256(agentic.corpus_bytes(v6)).hexdigest(), V6_CORPUS_SHA256[partition]
            )
            v5 = agentic.records_as_of(5, records)
            self.assertEqual(
                hashlib.sha256(agentic.corpus_bytes(v5)).hexdigest(), V5_CORPUS_SHA256[partition]
            )
            v4 = agentic.records_as_of(4, records)
            self.assertEqual(
                hashlib.sha256(agentic.corpus_bytes(v4)).hexdigest(), V4_CORPUS_SHA256[partition]
            )
            previous = agentic.records_as_of(3, records)
            self.assertEqual(
                hashlib.sha256(agentic.corpus_bytes(previous)).hexdigest(),
                PREVIOUS_CORPUS_SHA256[partition],
            )

    def test_adjacency_cases_cover_both_orders_separators_and_surfaces(self) -> None:
        for partition, records in self.corpora.items():
            adjacent = [r for r in records if r.surface.startswith("adjacent_")]
            cases = (*agentic.ADJACENT_GOLD[partition], *agentic.ADJACENT_LOOKALIKES[partition])
            self.assertEqual(len(adjacent), len(cases) * 2 * (4 * 3 + 3))
            for case in cases:
                layer = "A" if case in agentic.ADJACENT_GOLD[partition] else "D"
                matching = [r for r in adjacent if r.family == case.family and r.layer == layer]
                self.assertEqual(len(matching), 30, case.family)
                self.assertEqual(
                    {r.surface for r in matching}, set(agentic.ADJACENT_TEMPLATES), case.family
                )
                self.assertFalse(
                    any(r.surface == "adjacent_json_array" and "-tab-" in r.uid for r in matching),
                    case.family,
                )
                for record in matching:
                    self.assertTrue(record.gold if layer == "A" else not record.gold, record.uid)

    def test_adjacency_json_arrays_preserve_logical_single_separators(self) -> None:
        for partition, records in self.corpora.items():
            cases = {
                ("A", c.family): c for c in agentic.ADJACENT_GOLD[partition]
            } | {
                ("D", c.family): c for c in agentic.ADJACENT_LOOKALIKES[partition]
            }
            for record in records:
                if record.surface != "adjacent_json_array":
                    continue
                _, _, _, _, direction, separator_name, _ = record.uid.split("-", 6)
                case = cases[(record.layer, record.family)]
                values = case.values if direction == "forward" else case.values[::-1]
                expected = agentic.ADJACENT_SEPARATORS[separator_name].join(
                    value.prefix + value.value + value.suffix for value in values
                )
                self.assertNotEqual(separator_name, "tab", record.uid)
                self.assertEqual(json.loads(record.text)["items"], [expected], record.uid)

    def test_ip_gold_is_private_host_address_and_nonidentifying_ranges_are_lookalikes(self) -> None:
        for partition in agentic.PARTITIONS:
            ip_gold = [
                value.value for case in agentic.ADJACENT_GOLD[partition]
                for value in case.values if value.label == "IPADDRESS"
            ]
            self.assertTrue(ip_gold)
            for value in ip_gold:
                address = ipaddress.ip_address(value)
                host = address.ipv4_mapped if isinstance(address, ipaddress.IPv6Address) else None
                host = host or address
                self.assertTrue(host.is_private, value)
                self.assertFalse(host.is_loopback or host.is_link_local, value)
            lookalikes = {
                case.family: [value.value for value in case.values]
                for case in agentic.ADJACENT_LOOKALIKES[partition]
            }
            for family in ("adjacent_loopback_ips", "adjacent_link_local_ips", "adjacent_mapped_loopback_ips"):
                self.assertIn(family, lookalikes)
                self.assertEqual(len(lookalikes[family]), 2)
            self.assertTrue(all(ipaddress.ip_address(value).is_loopback for value in lookalikes["adjacent_loopback_ips"]))
            self.assertTrue(all(ipaddress.ip_address(value).is_link_local for value in lookalikes["adjacent_link_local_ips"]))
            self.assertTrue(all(
                ipaddress.ip_address(value).ipv4_mapped.is_loopback
                for value in lookalikes["adjacent_mapped_loopback_ips"]
            ))

    def test_every_gold_span_selects_exactly_the_inserted_value(self) -> None:
        for records in self.corpora.values():
            for record in records:
                encoded = record.text.encode("utf-8")
                for gold in record.gold:
                    self.assertEqual(encoded[gold.start : gold.end].decode("utf-8"), gold.value)

    def test_gold_offsets_are_bytes_not_characters(self) -> None:
        # NBSP (2 bytes) or a non-ASCII letter before the value shifts offsets.
        shifted = [
            (record, gold)
            for record in self.corpora["test"]
            for gold in record.gold
            if not record.text[: record.text.index(gold.value)].isascii()
        ]
        self.assertTrue(shifted)
        for record, gold in shifted:
            self.assertGreater(gold.start, record.text.index(gold.value))

    def test_checksum_families_are_valid_and_their_twins_are_not(self) -> None:
        for records in self.corpora.values():
            for record in records:
                family = agentic.IDENTIFIER_FAMILY_BY_NAME.get(record.family)
                if family is None:
                    continue
                check = agentic.CHECKSUMS[family.checksum]
                (gold,) = record.gold
                with self.subTest(uid=record.uid):
                    self.assertEqual(check(gold.value), record.validity == agentic.VALID)

    def test_every_checksum_parent_has_a_twin_in_every_surface(self) -> None:
        records = self.corpora["test"]
        uids = {record.uid for record in records}
        for record in records:
            # Cued card cells alternate validity per document instead
            # (`CueCellTests.test_card_cells_carry_both_validities`).
            if record.validity == agentic.VALID and not record.surface.startswith("cue_"):
                self.assertIn(record.uid[: -len(agentic.VALID)] + agentic.INVALID, uids)

    def test_nbsp_surfaces_perturb_their_prose_cue_parent_only(self) -> None:
        by_uid = {record.uid: record for record in self.corpora["test"]}
        for record in by_uid.values():
            if record.layer != agentic.LAYER_IDENTIFIERS or record.surface not in ("nbsp", "narrow_nbsp"):
                continue
            separator = agentic.NBSP if record.surface == "nbsp" else agentic.NARROW_NBSP
            parent = by_uid[record.uid.replace(f"-{record.surface}-", "-prose_cue-")]
            self.assertEqual(record.text.replace(separator, " "), parent.text)
            self.assertEqual(record.template, parent.template)
            self.assertIn(separator, record.text)

    def test_tool_json_documents_are_single_encoded_json(self) -> None:
        for record in self.corpora["test"]:
            if record.surface == "tool_json":
                arguments = json.loads(record.text)
                self.assertIsInstance(arguments, dict)
                for gold in record.gold:
                    self.assertTrue(
                        any(gold.value in str(value) for value in arguments.values())
                    )

    def test_layer_d_carries_no_gold_and_skus_are_luhn_invalid(self) -> None:
        for record in self.corpora["test"]:
            if record.layer == agentic.LAYER_LOOKALIKES:
                self.assertEqual(record.gold, ())
                if record.family == "sku_4x4":
                    sku = re.search(r"\d{4}-\d{4}-\d{4}-\d{4}", record.text).group(0)
                    self.assertFalse(agentic.luhn_valid(sku))

    def test_layer_a_text_outside_gold_has_no_stray_identifiers(self) -> None:
        # Unlabelled PII would count as a false positive for a correct pipeline.
        for record in self.corpora["test"]:
            if record.layer != agentic.LAYER_IDENTIFIERS:
                continue
            encoded = bytearray(record.text.encode("utf-8"))
            for gold in record.gold:
                encoded[gold.start : gold.end] = b" " * (gold.end - gold.start)
            rest = encoded.decode("utf-8", errors="replace")
            self.assertNotIn("@", rest, record.uid)
            for name in agentic.GIVEN_NAMES["test"] + agentic.SURNAMES["test"]:
                self.assertNotRegex(rest, rf"\b{name}\b", record.uid)

    def test_machine_keys_differ_across_partitions_ignoring_case(self) -> None:
        for pool in agentic.KEYS.values():
            fold = lambda keys: {k.lower().replace("-", "_") for k in keys}
            self.assertFalse(fold(pool["dev"]) & fold(pool["test"]), pool)

    def test_every_published_surface_and_family_is_populated(self) -> None:
        cells = {(r.family, r.surface) for r in self.corpora["test"]}
        families = [f.name for f in agentic.IDENTIFIER_FAMILIES] + [
            "email", "phone_de", "phone_us", "dob", "header_name",
        ]
        for family in families:
            for surface in agentic.SURFACES:
                self.assertIn((family, surface), cells)
        for family in [*agentic.LOOKALIKE_FAMILIES, *agentic.INDEXED_LOOKALIKE_FAMILIES]:
            for surface in agentic.LOOKALIKE_SURFACES:
                self.assertIn((family, surface), cells)


class CounterweightTests(unittest.TestCase):
    """M1: every gold cell only a context-free rule can reach has an FP cost in D."""

    @classmethod
    def setUpClass(cls) -> None:
        cls.records = agentic.generate("test")

    def gold_shapes(self, cell: tuple[str, str, str]) -> set[str]:
        return {
            agentic.display_shape(g.value)
            for r in self.records
            if (r.family, r.surface, r.validity) == cell
            for g in r.gold
        }

    def lookalike_values(self, family: str) -> list[str]:
        key = agentic.LOOKALIKE_KEYS["test"][family]
        return [
            json.loads(r.text)[key]
            for r in self.records
            if r.layer == agentic.LAYER_LOOKALIKES and r.family == family and r.surface == "tool_json"
        ]

    def lookalike_shapes(self, family: str) -> set[str]:
        return {agentic.display_shape(v) for v in self.lookalike_values(family)}

    def test_every_context_free_only_cell_has_a_counterweight_or_an_exemption(self) -> None:
        cells = {
            (r.family, r.surface, r.validity)
            for r in self.records
            if r.layer == agentic.LAYER_IDENTIFIERS
            and agentic.is_context_free_only(r.family, r.surface, r.validity)
        }
        self.assertTrue(cells)
        ruled = set(agentic.COUNTERWEIGHTS) | set(agentic.COUNTERWEIGHT_EXEMPT)
        self.assertEqual(cells - ruled, set(), "context-free gold with no D counterweight")
        self.assertEqual(ruled - cells, set(), "stale counterweight entry")
        self.assertFalse(set(agentic.COUNTERWEIGHTS) & set(agentic.COUNTERWEIGHT_EXEMPT))

    def test_each_counterweight_renders_every_shape_of_its_gold(self) -> None:
        for cell, family in agentic.COUNTERWEIGHTS.items():
            with self.subTest(cell=cell):
                self.assertLessEqual(self.gold_shapes(cell), self.lookalike_shapes(family))

    def test_counterweight_values_fail_every_same_length_checksum(self) -> None:
        checks = {9: (agentic.bsn_valid,), 10: (agentic.nhs_valid,),
                  11: (agentic.steuer_id_valid, agentic.cpf_valid), 16: (agentic.luhn_valid,)}
        for family in ("ref_number_9", "ref_number_10", "ref_number_11", "ref_number_16", "sku_4x4"):
            for value in self.lookalike_values(family):
                digits = agentic._only_digits(value)
                self.assertFalse(any(check(digits) for check in checks[len(digits)]), value)

    def test_counterweight_dates_are_not_birth_dates(self) -> None:
        for r in self.records:
            if r.family == "local_date":
                year = int(re.search(r"(20\d\d)", r.text).group(1))
                self.assertGreaterEqual(year, 2024)

    def test_display_shape_keeps_separators_exact(self) -> None:
        self.assertEqual(agentic.display_shape("4111 1111-1111.1111"), "9999 9999-9999.9999")
        self.assertNotEqual(agentic.display_shape("1234 5678"), agentic.display_shape("1234-5678"))
        self.assertEqual(agentic.display_shape("DE89 3704"), "AA99 9999")

    def test_a_spaced_sixteen_digit_rule_would_pay_for_its_catch_in_layer_d(self) -> None:
        rule = re.compile(r"\b\d{4} \d{4} \d{4} \d{4}\b")
        catches = [r for r in self.records
                   if (r.family, r.surface, r.validity) == ("card", "prose_nocue", agentic.INVALID)
                   and rule.search(r.text)]
        costs = [r for r in self.records if r.family == "ref_number_16" and rule.search(r.text)]
        self.assertEqual(len(catches), agentic.DOCS_PER_FAMILY)
        self.assertEqual(len(costs), agentic.DOCS_PER_FAMILY * len(agentic.LOOKALIKE_SURFACES))

    def test_a_bare_nine_digit_rule_would_pay_for_its_catch_in_layer_d(self) -> None:
        # The model-free half of the mutant check: the over-broad shape rule
        # reaches the BSN twins it would "fix" and the D counterweight alike.
        rule = re.compile(r"(?<![\d.-])\d{9}(?![\d.-])")
        catches = [r for r in self.records
                   if (r.family, r.surface, r.validity) == ("bsn", "prose_nocue", agentic.INVALID)
                   and rule.search(r.text)]
        costs = [r for r in self.records if r.family == "ref_number_9" and rule.search(r.text)]
        self.assertEqual(len(catches), agentic.DOCS_PER_FAMILY)
        self.assertEqual(len(costs), agentic.DOCS_PER_FAMILY * len(agentic.LOOKALIKE_SURFACES))


class RepeatSliceTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.records = [r for r in agentic.generate("test") if r.layer == agentic.LAYER_REPEATS]

    def test_every_document_repeats_a_gold_value_two_to_four_times(self) -> None:
        self.assertTrue(self.records)
        for record in self.records:
            by_entity: dict[str, int] = {}
            for gold in record.gold:
                key = f"{gold.label}:{agentic._only_digits(gold.value) or gold.value.lower()}"
                by_entity[key] = by_entity.get(key, 0) + 1
            self.assertTrue(any(2 <= n <= 4 for n in by_entity.values()), record.uid)

    def test_decoys_are_in_the_text_and_never_overlap_gold(self) -> None:
        collision_families = {"word_names", "substring_names", "id_repeats"}
        for record in self.records:
            encoded = record.text.encode("utf-8")
            if record.family in collision_families:
                self.assertTrue(record.decoys, record.uid)
            for decoy in record.decoys:
                self.assertEqual(encoded[decoy.start : decoy.end].decode("utf-8"), decoy.value)
                for gold in record.gold:
                    self.assertFalse(decoy.start < gold.end and gold.start < decoy.end, record.uid)

    def test_word_decoys_spell_a_gold_name_part(self) -> None:
        for record in self.records:
            if record.family != "word_names":
                continue
            names = {g.value for g in record.gold if g.label in ("GIVENNAME", "SURNAME")}
            self.assertTrue(all(d.value in names for d in record.decoys), record.uid)

    def test_shared_digit_decoys_are_a_digit_run_of_the_repeated_id(self) -> None:
        for record in self.records:
            if record.family == "id_repeats":
                digits = agentic._only_digits(record.gold[0].value)
                self.assertTrue(all(d.value in digits for d in record.decoys))

    def test_case_variant_shapes_are_all_present(self) -> None:
        texts = {r.surface: r.text for r in self.records if r.family == "case_variants"}
        self.assertEqual(set(texts), {"lower", "upper", "nbsp", "linebreak"})
        self.assertIn(agentic.NBSP, texts["nbsp"])

    def test_layer_a_and_d_records_carry_no_decoy_key(self) -> None:
        # Address cells record their benign designators as decoys.
        for record in agentic.generate("test"):
            if record.layer != agentic.LAYER_REPEATS and not record.surface.startswith(("address_", "tel_", "cue_", "url_")):
                self.assertNotIn("decoys", record.to_json())


class LabelledLookalikeCellTests(unittest.TestCase):
    """Labelled PII inside benign structures is gold; each cell's
    layer D twin has the same shape, structure and position and no cue."""

    @classmethod
    def setUpClass(cls) -> None:
        cls.cells = {
            partition: [r for r in agentic.generate(partition) if r.surface.startswith("lookalike_")]
            for partition in agentic.PARTITIONS
        }

    def generate_with(self, cells) -> None:
        with mock.patch.object(agentic, "LOOKALIKE_GOLD_CELLS", cells):
            agentic.generate("test")

    def test_every_cell_is_generated_in_both_partitions(self) -> None:
        for partition, records in self.cells.items():
            for layer, cells in (("A", agentic.LOOKALIKE_GOLD_CELLS), ("D", agentic.LOOKALIKE_TWINS)):
                for cell in cells:
                    matching = [r for r in records if r.layer == layer and r.family == cell.family]
                    self.assertEqual(len(matching), agentic.DOCS_PER_LOOKALIKE_CELL[layer], (partition, cell.family))
            self.assertEqual(
                {r.surface for r in records if r.layer == "A"}, set(agentic.LOOKALIKE_CELL_SURFACES)
            )
            self.assertEqual(
                {cell.label for cell in agentic.LOOKALIKE_GOLD_CELLS}, set(agentic.LabelRelation)
            )

    def test_growth_stays_within_ten_percent_per_layer(self) -> None:
        records = agentic.generate("test")
        for layer in ("A", "D"):
            new = sum(1 for r in self.cells["test"] if r.layer == layer)
            old = sum(1 for r in records if r.layer == layer) - new
            self.assertLessEqual(new * 10, old, layer)

    def test_gold_cells_carry_one_labelled_value_inside_a_benign_structure(self) -> None:
        for records in self.cells.values():
            for record in (r for r in records if r.layer == "A"):
                cell = next(c for c in agentic.LOOKALIKE_GOLD_CELLS if c.family == record.family)
                self.assertEqual(len(record.gold), 1, record.uid)
                gold = record.gold[0]
                self.assertEqual(gold.label, agentic.LOOKALIKE_VALUE_KINDS[cell.twin.kind][0], record.uid)
                self.assertEqual(record.text.encode()[gold.start : gold.end].decode(), gold.value)
                value, start, end = agentic.lookalike_value(record)
                self.assertEqual(
                    agentic.value_structure(record.text, start, end), cell.twin.structure, record.uid
                )
                self.assertEqual(record.validity, agentic.UNCHECKED)

    def test_gold_cells_are_labelled_and_counterweights_carry_no_cue(self) -> None:
        for records in self.cells.values():
            for record in records:
                labelled = agentic.has_lookalike_cue(record.text) or agentic.has_non_latin_letter(record.text)
                if record.layer == "A":
                    self.assertTrue(labelled, record.uid)
                else:
                    self.assertFalse(labelled, record.uid)
                    self.assertEqual(record.gold, (), record.uid)
                    self.assertEqual(record.validity, agentic.BENIGN)

    def test_a_cued_counterweight_fails_generation(self) -> None:
        twin = agentic.ORDER_REF_ZIP
        cued = dataclasses.replace(twin, templates={**twin.templates, "test": "ZIP:\nORDER-{V}"})
        with mock.patch.object(agentic, "LOOKALIKE_TWINS", (cued, *agentic.LOOKALIKE_TWINS[1:])):
            with self.assertRaisesRegex(agentic.LayerError, "carries a cue"):
                agentic.generate("test")

    def test_cue_vocabulary_is_the_checked_single_source(self) -> None:
        vocabulary = json.loads(agentic.CUE_VOCABULARY_PATH.read_text(encoding="utf-8"))
        self.assertEqual(set(vocabulary["stems"]), {"postal", "phone", "address"})
        self.assertEqual(set(vocabulary["whole_words"]), {"postal", "phone", "address"})
        stems = [stem for family in vocabulary["stems"].values() for stem in family]
        self.assertEqual(tuple(stems), agentic.LOOKALIKE_CUE_STEMS)
        self.assertEqual(len(stems), len(set(stems)))
        for text in ("Téléphone:", "zipCode", "shippingAddress", "PLZ", "Straße", "CEP 1", "Kontakt"):
            self.assertTrue(agentic.has_lookalike_cue(text), text)
        for text in ("Order reference:", "Bestellnummer", "ticketRef", "capital", "record_no"):
            self.assertFalse(agentic.has_lookalike_cue(text), text)
        self.assertTrue(agentic.has_non_latin_letter("Телефон"))
        self.assertTrue(agentic.has_non_latin_letter("電話番号"))
        self.assertFalse(agentic.has_non_latin_letter("Número de telemóvel, Straße"))

    def test_counterweights_are_derived_from_the_cells(self) -> None:
        self.assertEqual(
            agentic.LOOKALIKE_COUNTERWEIGHTS,
            {cell.family: cell.twin.family for cell in agentic.LOOKALIKE_GOLD_CELLS},
        )
        unpaired = {t.family for t in agentic.LOOKALIKE_TWINS} - set(agentic.LOOKALIKE_COUNTERWEIGHTS.values())
        self.assertEqual(unpaired, set(agentic.UNPAIRED_TWINS))

    def test_each_cell_and_its_twin_share_shape_structure_and_position(self) -> None:
        for records in self.cells.values():
            for cell in agentic.LOOKALIKE_GOLD_CELLS:
                gold = next(r for r in records if r.layer == "A" and r.family == cell.family)
                benign = next(r for r in records if r.layer == "D" and r.family == cell.twin.family)
                (gold_value, *gold_span), (value, *span) = (
                    agentic.lookalike_value(gold), agentic.lookalike_value(benign)
                )
                self.assertEqual(agentic.display_shape(gold_value), agentic.display_shape(value), cell.family)
                self.assertEqual(
                    agentic.value_position(gold.text, *gold_span, gold.surface),
                    agentic.value_position(benign.text, *span, benign.surface),
                    cell.family,
                )

    def test_swapping_two_json_twins_fails_generation(self) -> None:
        # Same surface, value shape and structure; only the JSON topology differs.
        cells = list(agentic.LOOKALIKE_GOLD_CELLS)
        nested = next(i for i, c in enumerate(cells) if c.family == "zip_json_nested_path")
        entries = next(i for i, c in enumerate(cells) if c.family == "zip_json_entries_array")
        cells[nested], cells[entries] = (
            dataclasses.replace(cells[nested], twin=cells[entries].twin),
            dataclasses.replace(cells[entries], twin=cells[nested].twin),
        )
        with self.assertRaisesRegex(agentic.LayerError, "differ in shape or position"):
            self.generate_with(tuple(cells))

    def test_phone_values_use_reserved_ranges(self) -> None:
        checked = 0
        for records in self.cells.values():
            for record in records:
                value, _, _ = agentic.lookalike_value(record)
                twin = next(
                    (c.twin for c in agentic.LOOKALIKE_GOLD_CELLS if c.family == record.family)
                    if record.layer == "A"
                    else (t for t in agentic.LOOKALIKE_TWINS if t.family == record.family)
                )
                pattern = {
                    "phone_us": r"^\d{3}-555-01\d{2}$",
                    "phone_de": r"^01555\d{7}$",
                    "phone_de_run": r"^0155-5\d{3}(?:-\d{4}){2}$",
                }.get(twin.kind)
                if pattern:
                    checked += 1
                    self.assertRegex(value, pattern, record.uid)
        self.assertEqual(checked, 2 * sum(
            agentic.DOCS_PER_LOOKALIKE_CELL[layer]
            for layer, kinds in (("A", [c.twin.kind for c in agentic.LOOKALIKE_GOLD_CELLS]),
                                 ("D", [t.kind for t in agentic.LOOKALIKE_TWINS]))
            for kind in kinds if kind.startswith("phone")
        ))

    def test_structured_cells_are_valid_json(self) -> None:
        for records in self.cells.values():
            for record in (r for r in records if r.surface == "lookalike_tool_json"):
                json.loads(record.text)

    def test_digit_runs_fail_luhn_so_no_card_rule_can_cover_them(self) -> None:
        for records in self.cells.values():
            for record in records:
                for run in re.findall(r"\d{4}-\d{4}-\d{4}-\d{4}", record.text):
                    self.assertFalse(agentic.luhn_valid(run), record.uid)
                    self.assertEqual(record.layer, "D", record.uid)

    def test_padding_pools_carry_no_cue_and_split_by_partition(self) -> None:
        for pool in (agentic.LOOKALIKE_FILLER, agentic.LOOKALIKE_LOG_LINE,
                     agentic.LOOKALIKE_CSV_ROW, agentic.LOOKALIKE_JSON_MEMBERS):
            self.assertFalse(agentic.has_lookalike_cue(json.dumps(pool, ensure_ascii=False)))
            dev, test = (pool[p] if isinstance(pool[p], tuple) else (pool[p],) for p in ("dev", "test"))
            self.assertFalse(set(dev) & set(test))


class AddressCellTests(unittest.TestCase):
    """Layer A addresses are whole and every part is gold; each unit spelling
    they score has a layer D twin spelled the same way, with no address."""

    @classmethod
    def setUpClass(cls) -> None:
        cls.cells = {
            partition: [r for r in agentic.generate(partition) if r.surface.startswith("address_")]
            for partition in agentic.PARTITIONS
        }

    def test_every_cell_and_twin_is_generated_in_both_partitions(self) -> None:
        for partition, records in self.cells.items():
            for layer, cells in (("A", agentic.ADDRESS_CELLS), ("D", agentic.ADDRESS_TWINS)):
                for cell in cells:
                    matching = [r for r in records if r.layer == layer and r.family == cell.family]
                    self.assertEqual(len(matching), agentic.DOCS_PER_ADDRESS_CELL[layer], (partition, cell.family))
            self.assertEqual({r.surface for r in records if r.layer == "A"}, set(agentic.ADDRESS_SURFACES))
            self.assertEqual(
                {c.designator for c in agentic.ADDRESS_CELLS} - {None}, set(agentic.Designator)
            )

    def test_growth_stays_within_ten_percent_per_layer(self) -> None:
        records = agentic.generate("test")
        for layer in ("A", "D"):
            new = sum(1 for r in self.cells["test"] if r.layer == layer)
            old = sum(1 for r in records if r.layer == layer) - new
            self.assertLessEqual(new * 10, old, layer)

    def test_addresses_are_whole_with_only_separators_between_gold_parts(self) -> None:
        for records in self.cells.values():
            for record in (r for r in records if r.layer == "A"):
                encoded = record.text.encode("utf-8")
                self.assertTrue(record.gold, record.uid)
                self.assertLessEqual({g.label for g in record.gold}, agentic.ADDRESS_LABELS, record.uid)
                parts = sorted(record.gold, key=lambda g: g.start)
                for gold in parts:
                    self.assertEqual(encoded[gold.start : gold.end].decode("utf-8"), gold.value)
                if record.surface in ("address_csv", "address_tool_json"):
                    continue
                for left, right in zip(parts, parts[1:]):
                    between = encoded[left.end : right.start].decode("utf-8")
                    self.assertRegex(between, r"^[ ,\n]+$", record.uid)

    def test_decoys_stand_apart_from_the_address(self) -> None:
        checked = 0
        for records in self.cells.values():
            for record in (r for r in records if r.layer == "A" and r.decoys):
                (decoy,) = record.decoys
                last = max(g.end for g in record.gold)
                self.assertGreater(decoy.start, last, record.uid)
                self.assertRegex(record.text.encode()[last : decoy.start].decode(), r"[.\n]", record.uid)
                checked += 1
        self.assertEqual(checked, 2 * agentic.DOCS_PER_ADDRESS_CELL["A"] * sum(
            1 for cell in agentic.ADDRESS_CELLS if cell.decoy is not None
        ))

    def test_counterweights_carry_one_designator_and_no_address(self) -> None:
        for records in self.cells.values():
            for record in (r for r in records if r.layer == "D"):
                twin = next(t for t in agentic.ADDRESS_TWINS if t.family == record.family)
                self.assertEqual(record.gold, (), record.uid)
                self.assertEqual(record.validity, agentic.BENIGN)
                (decoy,) = record.decoys
                self.assertRegex(decoy.value, agentic.DESIGNATOR_WORDS[twin.designator], record.uid)
                self.assertNotRegex(record.text, r"\d{5}|ZZ\d", record.uid)

    def test_values_use_unassigned_postcode_ranges(self) -> None:
        pattern = {"US": r"^000\d\d$", "DE": r"^00\d{3}$", "GB": r"^ZZ\d\d \dZZ$"}
        for records in self.cells.values():
            for record in (r for r in records if r.layer == "A"):
                for gold in (g for g in record.gold if g.label == "ZIPCODE"):
                    self.assertRegex(gold.value, pattern[record.region], record.uid)

    def test_structured_cells_parse(self) -> None:
        for records in self.cells.values():
            for record in records:
                if record.surface == "address_tool_json":
                    json.loads(record.text)
                if record.surface == "address_csv":
                    rows = list(csv.reader(io.StringIO(record.text)))
                    self.assertEqual({len(row) for row in rows}, {len(rows[0])}, record.uid)

    def test_address_vocabularies_split_by_partition(self) -> None:
        for pool in (agentic.ADDRESS_STREET_STEMS, agentic.US_STATES, agentic.MILITARY_POST_OFFICES,
                     agentic.MILITARY_STATES, agentic.MILITARY_BOX_NUMBERS):
            self.assertFalse(set(pool["dev"]) & set(pool["test"]))
        for region in ("US", "GB", "DE"):
            self.assertFalse(
                set(agentic.ADDRESS_CITIES["dev"][region]) & set(agentic.ADDRESS_CITIES["test"][region])
            )
        for cell in (*agentic.ADDRESS_CELLS, *agentic.ADDRESS_TWINS):
            self.assertNotEqual(cell.templates["dev"], cell.templates["test"], cell.family)

    def generate_with(self, **patches) -> None:
        with contextlib.ExitStack() as stack:
            for name, value in patches.items():
                stack.enter_context(mock.patch.object(agentic, name, value))
            agentic.generate("test")

    def test_a_designator_without_a_twin_fails_generation(self) -> None:
        twins = tuple(t for t in agentic.ADDRESS_TWINS if t.designator is not agentic.Designator.ETAGE)
        with self.assertRaisesRegex(agentic.LayerError, "no layer D counterweight: .*Etage"):
            self.generate_with(ADDRESS_TWINS=twins)

    def test_a_twin_no_cell_uses_fails_generation(self) -> None:
        cells = tuple(c for c in agentic.ADDRESS_CELLS if c.designator is not agentic.Designator.ETAGE)
        with self.assertRaisesRegex(agentic.LayerError, "no layer A cell uses"):
            self.generate_with(ADDRESS_CELLS=cells)

    def test_a_twin_with_a_postcode_fails_generation(self) -> None:
        twins = list(agentic.ADDRESS_TWINS)
        twins[0] = dataclasses.replace(
            twins[0], templates={**twins[0].templates, "test": "Run {X} for 00042 today."}
        )
        with self.assertRaisesRegex(agentic.LayerError, "postcode shape"):
            self.generate_with(ADDRESS_TWINS=tuple(twins))

    def test_a_missing_address_part_fails_generation(self) -> None:
        cells = list(agentic.ADDRESS_CELLS)
        cells[0] = dataclasses.replace(
            cells[0], templates={**cells[0].templates, "test": "Deliver to {HN} {ST} {UN}, {SA} {ZP}."}
        )
        with self.assertRaisesRegex(agentic.LayerError, "placeholders"):
            self.generate_with(ADDRESS_CELLS=tuple(cells))

    def test_a_spelling_without_a_benign_twin_fails_generation(self) -> None:
        # A rule matching only `Ste.` must cost false positives somewhere: drop
        # that one spelling from the D side and generation refuses.
        forms = {**agentic.DESIGNATOR_FORMS, agentic.Designator.SUITE: ("Suite {n}", "STE {n}")}
        twins = tuple(
            dataclasses.replace(t, forms=forms[t.designator]) if t.designator is agentic.Designator.SUITE else t
            for t in agentic.ADDRESS_TWINS
        )
        with self.assertRaisesRegex(agentic.LayerError, r"no layer D counterweight: \['Ste\. \{n\}'\]"):
            self.generate_with(ADDRESS_TWINS=twins)

    def test_every_spelling_is_generated_on_both_sides(self) -> None:
        for records in self.cells.values():
            benign = {agentic.designator_spelling(d.value) for r in records if r.layer == "D" for d in r.decoys}
            scored = {
                agentic.designator_spelling(span.value)
                for r in records if r.layer == "A"
                for part, span in agentic.address_part_values(
                    r, next(c for c in agentic.ADDRESS_CELLS if c.family == r.family)
                )
                if part in ("UN", "BX")
            }
            forms = {
                form for c in agentic.ADDRESS_CELLS if c.designator is not None
                for form in agentic.DESIGNATOR_FORMS[c.designator]
            }
            self.assertEqual(scored, forms | {"Box {n}"})
            self.assertLessEqual(scored, benign)

    def test_a_house_number_left_unscored_fails_the_check(self) -> None:
        # House number and unit are both BUILDINGNUM: the label set alone
        # cannot tell a missing house number from a present unit.
        record = next(r for r in self.cells["test"] if r.family == "address_us_suite_prose")
        cell = next(c for c in agentic.ADDRESS_CELLS if c.family == record.family)
        house = dict(agentic.address_part_values(record, cell))["HN"]
        stripped = dataclasses.replace(record, gold=tuple(g for g in record.gold if g != house))
        self.assertIn("BUILDINGNUM", {g.label for g in stripped.gold})
        with self.assertRaisesRegex(agentic.LayerError, "gold parts for placeholders"):
            agentic.check_address_cells([stripped])

    def test_a_template_without_its_house_number_fails_generation(self) -> None:
        cells = list(agentic.ADDRESS_CELLS)
        cells[0] = dataclasses.replace(
            cells[0], templates={**cells[0].templates, "test": cells[0].templates["test"].replace("{HN} ", "")}
        )
        with self.assertRaisesRegex(agentic.LayerError, "placeholders .* are not the shape's"):
            self.generate_with(ADDRESS_CELLS=tuple(cells))

    def test_a_unit_without_its_designator_word_fails_generation(self) -> None:
        forms = {**agentic.DESIGNATOR_FORMS, agentic.Designator.SUITE: ("Room {n}",)}
        with self.assertRaisesRegex(agentic.LayerError, "no gold part carries the suite designator"):
            self.generate_with(DESIGNATOR_FORMS=forms)


class PhoneShapeCellTests(unittest.TestCase):
    """Layer A phones are whole numbers in shapes the `+CC` and US/DE national
    rules miss; each shape's over-broad rule pays in its layer D twins."""

    @classmethod
    def setUpClass(cls) -> None:
        cls.cells = {
            partition: [r for r in agentic.generate(partition) if r.surface.startswith("tel_")]
            for partition in agentic.PARTITIONS
        }

    def test_every_cell_and_twin_is_generated_in_both_partitions(self) -> None:
        for partition, records in self.cells.items():
            for layer, cells in (("A", agentic.PHONE_CELLS), ("D", agentic.PHONE_TWINS)):
                for cell in cells:
                    matching = [r for r in records if r.layer == layer and r.family == cell.family]
                    self.assertEqual(len(matching), agentic.DOCS_PER_PHONE_CELL[layer], (partition, cell.family))
            for layer in ("A", "D"):
                self.assertEqual({r.surface for r in records if r.layer == layer}, set(agentic.PHONE_SURFACES))
            self.assertEqual({c.shape for c in agentic.PHONE_CELLS}, set(agentic.PhoneShape))

    def test_growth_stays_within_ten_percent_per_layer(self) -> None:
        records = agentic.generate("test")
        for layer in ("A", "D"):
            new = sum(1 for r in self.cells["test"] if r.layer == layer)
            old = sum(1 for r in records if r.layer == layer) - new
            self.assertLessEqual(new * 10, old, layer)

    def test_every_value_region_of_a_multi_block_shape_is_generated(self) -> None:
        for partition, records in self.cells.items():
            for cell in agentic.PHONE_CELLS:
                regions = {r.region for r in records if r.family == cell.family}
                expected = {region for region, _ in agentic.PHONE_VALUES[cell.shape][partition]}
                self.assertEqual(regions, expected, (partition, cell.family))

    def test_values_come_from_documented_fictional_blocks(self) -> None:
        blocks = (
            r"0(?:2\.61\.91|4\.65\.71)(?:\.\d\d){2}",  # ARCEP fiction, dotted
            r"(?:\+33 \(0\)|0033 )1 99 00 \d\d \d\d",  # ARCEP fiction
            r"(?:\+49 \(0\)|0049 )(?:30 23125 ?|69 90009 |89 99998 )\d{3}",  # BNetzA media numbers
            r"(?:\+44 \(0\)|0044 )20 7946 0\d{3}",  # Ofcom drama numbers
            r"001[- ]\d{3}[- ]555[- ]01\d\d",  # NANPA fictitious
            r"0\d\d \d{3} \d{3}|0\d(?: \d\d){3}",  # synthesized: leading 0 never used
        )
        for records in self.cells.values():
            for record in (r for r in records if r.layer == "A"):
                (gold,) = record.gold
                self.assertTrue(any(re.fullmatch(block, gold.value) for block in blocks), gold.value)

    def test_the_whole_number_is_gold_prefix_and_trunk_included(self) -> None:
        for records in self.cells.values():
            for record in (r for r in records if r.layer == "A"):
                (gold,) = record.gold
                encoded = record.text.encode("utf-8")
                self.assertEqual(encoded[gold.start : gold.end].decode("utf-8"), gold.value)
                before = encoded[: gold.start].decode("utf-8")
                after = encoded[gold.end :].decode("utf-8")
                self.assertNotRegex(before, r"[\d+]$", record.uid)
                self.assertNotRegex(after, r"^[\d)]", record.uid)

    def test_dotted_values_carry_an_ipv4_shaped_tail(self) -> None:
        # The layer that exposes an IPv4 rule claiming the last four groups.
        for records in self.cells.values():
            for record in (r for r in records if r.family.startswith("phone_dotted")):
                (gold,) = record.gold
                tail = gold.value.split(".", 1)[1]
                self.assertEqual(str(ipaddress.IPv4Address(tail)), tail, record.uid)

    def test_structured_cells_parse(self) -> None:
        for records in self.cells.values():
            for record in records:
                if record.surface == "tel_tool_json":
                    json.loads(record.text)
                if record.surface == "tel_csv":
                    rows = list(csv.reader(io.StringIO(record.text)))
                    self.assertEqual({len(row) for row in rows}, {len(rows[0])}, record.uid)

    def test_templates_split_by_partition(self) -> None:
        for cell in (*agentic.PHONE_CELLS, *agentic.PHONE_TWINS):
            self.assertNotEqual(cell.templates["dev"], cell.templates["test"], cell.family)
        values = {p: {g.value for r in rs for g in r.gold} for p, rs in self.cells.items()}
        self.assertFalse(values["dev"] & values["test"])

    def test_each_broad_rule_catches_its_shape_and_pays_in_layer_d(self) -> None:
        # The model-free half of the mutant check, shape by shape.
        for shape in agentic.PhoneShape:
            rule = re.compile(agentic.PHONE_BROAD_PATTERNS[shape])
            families = {c.family for c in agentic.PHONE_CELLS if c.shape is shape}
            twins = {t.family for t in agentic.PHONE_TWINS if t.shape is shape}
            for records in self.cells.values():
                catches = [r for r in records if r.family in families and rule.search(r.text)]
                costs = [r for r in records if r.family in twins and rule.search(r.text)]
                self.assertEqual(len(catches), agentic.DOCS_PER_PHONE_CELL["A"] * len(families), shape)
                self.assertEqual(len(costs), agentic.DOCS_PER_PHONE_CELL["D"] * len(twins), shape)

    def test_mutant_policies_carry_the_broad_and_narrow_patterns(self) -> None:
        for name, table in (("broad", agentic.PHONE_BROAD_PATTERNS), ("narrow", agentic.PHONE_NARROW_PATTERNS)):
            path = REPO_ROOT / f"scripts/bench/fixtures/agentic/mutant-{name}-phone-shapes.toml"
            patterns = re.findall(r"^pattern = '(.*)'$", path.read_text(), flags=re.MULTILINE)
            self.assertEqual(sorted(patterns), sorted(table.values()), name)

    # Layer D documents (all of layer D, not only the phone twins) that each
    # narrow rule matches, per partition. A narrow rule with no D cost here
    # would ship its false positives unmeasured.
    NARROW_D_COST = {
        "dev": {"dotted": 8, "national_3x3": 21, "national_2x4": 8, "prefix_00": 4, "prefix_001": 8},
        "test": {"dotted": 8, "national_3x3": 25, "national_2x4": 8, "prefix_00": 4, "prefix_001": 8},
    }

    def test_each_narrow_rule_catches_its_shape_and_pays_in_layer_d(self) -> None:
        for partition in agentic.PARTITIONS:
            layer_d = [r for r in agentic.generate(partition) if r.layer == "D"]
            records = self.cells[partition]
            costs = {}
            for shape, pattern in agentic.PHONE_NARROW_PATTERNS.items():
                rule = re.compile(pattern)
                families = {c.family for c in agentic.PHONE_CELLS if c.shape is shape}
                catches = [r for r in records if r.family in families and rule.search(r.text)]
                self.assertEqual(len(catches), agentic.DOCS_PER_PHONE_CELL["A"] * len(families), shape)
                costs[shape.value] = sum(1 for r in layer_d if rule.search(r.text))
            self.assertEqual(costs, self.NARROW_D_COST[partition], partition)

    def test_review_shape_rules_pay_in_layer_d(self) -> None:
        # Shape-specific rules that once matched every A positive and no D
        # document: each must now cost D false positives in both partitions.
        rules = {
            "dotted": r"\b0\d(?:\.\d{2}){4}\b",
            "prefix_00": r"\b00\d{2} \d{1,2}(?: \d{2,8}){1,4}\b",
            "prefix_001": r"\b001 \d{3} \d{3} \d{4}\b",
        }
        expected = {
            "dev": {"dotted": 8, "prefix_00": 4, "prefix_001": 4},
            "test": {"dotted": 8, "prefix_00": 4, "prefix_001": 8},
        }
        for partition in agentic.PARTITIONS:
            layer_d = [r for r in agentic.generate(partition) if r.layer == "D"]
            costs = {shape: sum(1 for r in layer_d if re.search(pattern, r.text)) for shape, pattern in rules.items()}
            self.assertEqual(costs, expected[partition], partition)

    def test_same_shape_twins_have_no_phone_reading(self) -> None:
        self.assertEqual(agentic.phone_reading("02.61.91.23.45", "Firmware "), None)
        self.assertEqual(agentic.phone_reading("02.61.91.23.45", "Call "), agentic.PhoneShape.DOTTED)
        self.assertEqual(agentic.phone_reading("0089 12 3456 7890", ""), None)
        self.assertEqual(agentic.phone_reading("0049 12 3456 7890", ""), agentic.PhoneShape.PREFIX_00)
        self.assertEqual(agentic.phone_reading("001 212 055 0142", ""), None)
        self.assertEqual(agentic.phone_reading("001 212 555 0142", ""), agentic.PhoneShape.PREFIX_001)
        self.assertEqual(agentic.phone_reading("+28 (0)14 5521 773", "Build "), agentic.PhoneShape.TRUNK_PARENS)

    def test_a_shape_whose_narrow_rule_pays_nothing_fails_generation(self) -> None:
        twins = tuple(t for t in agentic.PHONE_TWINS if t.family != "phone_twin_ticket_00")
        with self.assertRaisesRegex(agentic.LayerError, r"narrow rule pays nothing in layer D: \['prefix_00'\]"):
            self.generate_with(PHONE_TWINS=twins)

    def test_a_dotted_twin_without_a_benign_context_fails_generation(self) -> None:
        twins = list(agentic.PHONE_TWINS)
        index = next(i for i, t in enumerate(twins) if t.family == "phone_twin_dotted_firmware")
        twins[index] = dataclasses.replace(twins[index], templates={**twins[index].templates, "test": "Noted {X}."})
        with self.assertRaisesRegex(agentic.LayerError, "reads as a dotted phone"):
            self.generate_with(PHONE_TWINS=tuple(twins))

    def generate_with(self, **patches) -> None:
        with contextlib.ExitStack() as stack:
            for name, value in patches.items():
                stack.enter_context(mock.patch.object(agentic, name, value))
            agentic.generate("test")

    def test_a_shape_without_a_twin_fails_generation(self) -> None:
        twins = tuple(t for t in agentic.PHONE_TWINS if t.shape is not agentic.PhoneShape.PREFIX_00)
        with self.assertRaisesRegex(agentic.LayerError, r"no layer D counterweight: \['prefix_00'\]"):
            self.generate_with(PHONE_TWINS=twins)

    def test_a_twin_no_cell_uses_fails_generation(self) -> None:
        cells = tuple(c for c in agentic.PHONE_CELLS if c.shape is not agentic.PhoneShape.PREFIX_001)
        with self.assertRaisesRegex(agentic.LayerError, "no layer A cell uses"):
            self.generate_with(PHONE_CELLS=cells)

    def test_a_national_phone_without_a_label_fails_generation(self) -> None:
        cells = list(agentic.PHONE_CELLS)
        index = next(i for i, c in enumerate(cells) if c.family == "phone_national_3x3_prose")
        cells[index] = dataclasses.replace(cells[index], templates={**cells[index].templates, "test": "Noted {V}."})
        with self.assertRaisesRegex(agentic.LayerError, "no phone label before it"):
            self.generate_with(PHONE_CELLS=tuple(cells))

    def test_a_twin_with_a_phone_label_fails_generation(self) -> None:
        twins = list(agentic.PHONE_TWINS)
        twins[0] = dataclasses.replace(twins[0], templates={**twins[0].templates, "test": "Call about {X} today."})
        with self.assertRaisesRegex(agentic.LayerError, "carries a phone label"):
            self.generate_with(PHONE_TWINS=tuple(twins))

    def test_a_twin_value_in_a_phone_shape_fails_generation(self) -> None:
        twins = list(agentic.PHONE_TWINS)
        index = next(i for i, t in enumerate(twins) if t.shape is agentic.PhoneShape.PREFIX_001)
        twins[index] = dataclasses.replace(twins[index], make=lambda rng, partition: "001-212-555-0142")
        with self.assertRaisesRegex(agentic.LayerError, "reads as a prefix_001 phone"):
            self.generate_with(PHONE_TWINS=tuple(twins))

    def test_a_value_outside_its_shape_fails_generation(self) -> None:
        values = {**agentic.PHONE_VALUES,
                  agentic.PhoneShape.DOTTED: {"dev": (("FR", lambda rng: "02 61 91 23 45"),),
                                              "test": (("FR", lambda rng: "04 65 71 23 45"),)}}
        with self.assertRaisesRegex(agentic.LayerError, "is not a dotted phone"):
            self.generate_with(PHONE_VALUES=values)

    def test_a_twin_the_broad_rule_misses_fails_generation(self) -> None:
        twins = list(agentic.PHONE_TWINS)
        index = next(i for i, t in enumerate(twins) if t.shape is agentic.PhoneShape.TRUNK_PARENS)
        twins[index] = dataclasses.replace(twins[index], make=lambda rng, partition: "+12")
        with self.assertRaisesRegex(agentic.LayerError, "broad pattern misses the decoy"):
            self.generate_with(PHONE_TWINS=tuple(twins))


class CueCellTests(unittest.TestCase):
    """Layer A values that only their wording makes personal; each shape's
    broad and narrow rules pay in its layer D twins."""

    @classmethod
    def setUpClass(cls) -> None:
        cls.full = {partition: agentic.generate(partition) for partition in agentic.PARTITIONS}
        cls.cells = {
            partition: [r for r in records if r.surface.startswith("cue_")]
            for partition, records in cls.full.items()
        }

    def generate_with(self, **patches) -> None:
        with contextlib.ExitStack() as stack:
            for name, value in patches.items():
                stack.enter_context(mock.patch.object(agentic, name, value))
            agentic.generate("test")

    @staticmethod
    def replaced(cells: tuple, family: str, **changes) -> tuple:
        return tuple(dataclasses.replace(c, **changes) if c.family == family else c for c in cells)

    def test_every_cell_and_twin_is_generated_in_both_partitions(self) -> None:
        for partition, records in self.cells.items():
            for layer, cells in (("A", agentic.CUE_CELLS), ("D", agentic.CUE_TWINS)):
                for cell in cells:
                    matching = [r for r in records if r.layer == layer and r.family == cell.family]
                    self.assertEqual(len(matching), agentic.DOCS_PER_CUE_CELL[layer], (partition, cell.family))
            for layer in ("A", "D"):
                self.assertEqual({r.surface for r in records if r.layer == layer}, set(agentic.CUE_SURFACES))
        self.assertEqual({c.shape for c in agentic.CUE_CELLS}, set(agentic.CueShape))
        self.assertEqual({c.shape.label for c in agentic.CUE_CELLS},
                         {"AGE", "DATEOFBIRTH", "CREDITCARDNUMBER", "ZIPCODE"})

    def test_growth_stays_within_ten_percent_per_layer(self) -> None:
        for layer in ("A", "D"):
            new = sum(1 for r in self.cells["test"] if r.layer == layer)
            old = sum(1 for r in self.full["test"] if r.layer == layer) - new
            self.assertLessEqual(new * 10, old, layer)

    def test_the_value_alone_is_gold(self) -> None:
        for records in self.cells.values():
            for record in (r for r in records if r.layer == "A"):
                (gold,) = record.gold
                encoded = record.text.encode("utf-8")
                self.assertEqual(encoded[gold.start : gold.end].decode("utf-8"), gold.value)
                self.assertNotRegex(encoded[: gold.start].decode("utf-8"), r"\d$", record.uid)
                self.assertNotRegex(encoded[gold.end :].decode("utf-8"), r"^\d", record.uid)

    def test_structured_cells_parse(self) -> None:
        for records in self.cells.values():
            for record in records:
                if record.surface == "cue_tool_json":
                    json.loads(record.text)
                if record.surface == "cue_csv":
                    rows = list(csv.reader(io.StringIO(record.text)))
                    self.assertEqual({len(row) for row in rows}, {len(rows[0])}, record.uid)

    def test_templates_split_by_partition(self) -> None:
        for cell in (*agentic.CUE_CELLS, *agentic.CUE_TWINS):
            self.assertNotEqual(cell.templates["dev"], cell.templates["test"], cell.family)
        values = {p: {g.value for r in rs for g in r.gold} for p, rs in self.cells.items()}
        self.assertFalse(values["dev"] & values["test"])

    def test_card_cells_carry_both_validities_and_every_length(self) -> None:
        for records in self.cells.values():
            cards = [r for r in records if r.layer == "A" and r.gold[0].label == "CREDITCARDNUMBER"]
            self.assertEqual({r.validity for r in cards}, {agentic.VALID, agentic.INVALID})
            compact = [r for r in cards if " " not in r.gold[0].value]
            for length in (12, 15):
                validities = {r.validity for r in compact if len(r.gold[0].value) == length}
                self.assertEqual(validities, {agentic.VALID, agentic.INVALID}, length)
            self.assertEqual({len(r.gold[0].value.replace(" ", "")) for r in cards}, {12, 13, 14, 15})
            for record in cards:
                digits = record.gold[0].value.replace(" ", "")
                self.assertEqual(agentic.luhn_valid(digits), record.validity == agentic.VALID, record.uid)

    def test_cued_card_twins_are_credited_and_guarded(self) -> None:
        for family in (c.family for c in agentic.CUE_CELLS if c.shape.label == "CREDITCARDNUMBER"):
            self.assertEqual(agentic.CUE_FAMILY_LABELS[family], "CREDITCARDNUMBER")
            self.assertTrue(agentic.invalid_twin_credited(agentic.CUE_FAMILY_LABELS[family], "cue_prose"))
        self.assertEqual(agentic.guard_families(7), ["ref_number_10", "ref_number_11", "ref_number_16",
                                                      "ref_number_9"])
        self.assertEqual(agentic.guard_families(8), sorted([*CUE_CARD_TWINS, *agentic.guard_families(7)]))
        # A v8 corpus without a guarded card twin's count fails closed.
        scorecard = _scorecard({"C": 0, "A": 0, "D": 0, "R": 0})
        scorecard["layers"]["generator"] = {"generator_version": 8}
        del scorecard["layers"]["D"]["runs"][0]["per_cell"]["D|card_twin_order|prose|benign"]
        with self.assertRaisesRegex(agentic.LayerError, "card_twin_order"):
            agentic.layer_totals(scorecard, "policy-file")
        scorecard["layers"]["generator"] = {"generator_version": 7}
        self.assertNotIn("card_twin_order",
                         agentic.layer_totals(scorecard, "policy-file")["D"]["guard_false_positive"])

    def test_a_cued_card_twin_fp_rise_fails_the_gate(self) -> None:
        base = _scorecard({"C": 10, "A": 600, "D": 0, "R": 0}, {"D": 7})
        candidate = _scorecard({"C": 10, "A": 0, "D": 0, "R": 0}, {"D": 8})
        candidate["layers"]["D"]["runs"][0]["per_cell"]["D|card_twin_reader_serial|prose|benign"] = {
            "utf8_bytes": {"leaked": 0, "false_positive": 1}}
        result = agentic.gate(base, candidate)
        self.assertEqual(result["verdict"], "fail")
        self.assertIn("card_twin_reader_serial", result["reason"])

    def test_each_broad_rule_catches_its_shape_and_pays_on_every_twin(self) -> None:
        for shape in agentic.CueShape:
            rule = re.compile(agentic.CUE_BROAD_PATTERNS[shape])
            families = {c.family for c in agentic.CUE_CELLS if c.shape is shape}
            twins = {t.family for t in agentic.CUE_TWINS if t.shape is shape}
            for records in self.cells.values():
                catches = [r for r in records if r.family in families and rule.search(r.text)]
                costs = [r for r in records if r.family in twins and rule.search(r.text)]
                self.assertEqual(len(catches), agentic.DOCS_PER_CUE_CELL["A"] * len(families), shape)
                self.assertEqual(len(costs), agentic.DOCS_PER_CUE_CELL["D"] * len(twins), shape)

    def test_mutant_policies_carry_the_broad_and_narrow_patterns(self) -> None:
        for name, table in (("broad", agentic.CUE_BROAD_PATTERNS), ("narrow", agentic.CUE_NARROW_PATTERNS)):
            path = REPO_ROOT / f"scripts/bench/fixtures/agentic/mutant-{name}-cued-shapes.toml"
            patterns = re.findall(r"^pattern = '(.*)'$", path.read_text(encoding="utf-8"), flags=re.MULTILINE)
            self.assertEqual(sorted(patterns), sorted(table.values()), name)

    # Layer D documents (all of layer D) that each narrow rule matches, per
    # partition. A narrow rule with no D cost would ship its false positives
    # unmeasured.
    NARROW_D_COST = {
        # dev grouped: also 21 `ref_number_16` references; three digits: the
        # three-digit near twin and the Swedish-shape one (`ticket 900 00`).
        "dev": {"age_turned": 4, "age_at_the_age_of": 12, "age_yo": 4, "age_year_old_gender": 4, "dob_sentence_break": 4,
                "card_short_compact": 8, "card_short_grouped": 29, "zip_se": 4, "zip_pl": 4, "zip_six": 4,
                "zip_br": 4, "zip_three": 8},
        "test": {"age_turned": 4, "age_at_the_age_of": 12, "age_yo": 4, "age_year_old_gender": 4, "dob_sentence_break": 4,
                 "card_short_compact": 8, "card_short_grouped": 8, "zip_se": 4, "zip_pl": 4, "zip_six": 4,
                 "zip_br": 4, "zip_three": 8},
    }

    def test_each_narrow_rule_catches_its_shape_and_pays_in_layer_d(self) -> None:
        for partition in agentic.PARTITIONS:
            layer_d = [r for r in self.full[partition] if r.layer == "D"]
            costs = {}
            for shape, pattern in agentic.CUE_NARROW_PATTERNS.items():
                rule = re.compile(pattern)
                families = {c.family for c in agentic.CUE_CELLS if c.shape is shape}
                catches = [r for r in self.cells[partition] if r.family in families and rule.search(r.text)]
                self.assertEqual(len(catches), agentic.DOCS_PER_CUE_CELL["A"] * len(families), shape)
                costs[shape.value] = sum(1 for r in layer_d if rule.search(r.text))
            self.assertEqual(costs, self.NARROW_D_COST[partition], partition)

    def test_extending_the_shipped_card_cue_window_to_short_cards_pays_in_layer_d(self) -> None:
        # `card.cued`'s window (any 32 non-digit characters after `card`) with
        # 12- to 15-digit layouts would reach the terminal and reader twins.
        rule = re.compile(
            r"(?i)\bcards?\b[^\d\n.;!?:,=]{0,32}?\b(?:(?:5[0678]|6\d)\d{10,13}|(?:5[0678]|6\d)\d\d \d{4} \d{4})\b"
        )
        for partition, records in self.cells.items():
            costs = [r.family for r in records if r.layer == "D" and rule.search(r.text)]
            self.assertEqual(costs, ["card_twin_reader_serial"] * 4, partition)

    def test_cue_reading_examples(self) -> None:
        def reads(shape, text, value):
            start = text.index(value)
            return agentic.cue_reading(shape, text, start, start + len(value), "cue_prose")

        shape = agentic.CueShape
        self.assertTrue(reads(shape.AGE_TURNED, "I just turned 47.", "47"))
        self.assertFalse(reads(shape.AGE_TURNED, "The bridge turned 47.", "47"))
        self.assertFalse(reads(shape.AGE_TURNED, "She turned 90 degrees.", "90"))
        self.assertTrue(reads(shape.AGE_YEAR_OLD_GENDER, "A 28 year old female.", "28"))
        self.assertFalse(reads(shape.AGE_YEAR_OLD_GENDER, "A 28 year old female cat.", "28"))
        self.assertFalse(reads(shape.AGE_YO, "My 12 y/o laptop.", "12"))
        self.assertTrue(reads(shape.DOB_SENTENCE_BREAK, "My date of birth? It's 3/11/1987.", "3/11/1987"))
        self.assertFalse(reads(shape.DOB_SENTENCE_BREAK, "My date of birth. Login 3/11/2026.", "3/11/2026"))
        self.assertTrue(reads(shape.CARD_SHORT_COMPACT, "card number: 504712345678", "504712345678"))
        self.assertFalse(reads(shape.CARD_SHORT_COMPACT, "card reader 504712345678", "504712345678"))
        self.assertTrue(reads(shape.ZIP_PL, "PLZ: 53-320", "53-320"))
        self.assertFalse(reads(shape.ZIP_PL, "PLZ missing, error 53-320", "53-320"))
        text = "a,zip,b\nx,53-320,y\n"
        start = text.index("53-320")
        self.assertTrue(agentic.cue_reading(shape.ZIP_PL, text, start, start + 6, "cue_csv"))

    def test_a_shape_without_a_twin_fails_generation(self) -> None:
        twins = tuple(t for t in agentic.CUE_TWINS if t.shape is not agentic.CueShape.ZIP_BR)
        with self.assertRaisesRegex(agentic.LayerError, r"no layer D counterweight: \['zip_br'\]"):
            self.generate_with(CUE_TWINS=twins)

    def test_a_twin_no_cell_uses_fails_generation(self) -> None:
        cells = tuple(c for c in agentic.CUE_CELLS if c.shape is not agentic.CueShape.ZIP_THREE)
        with self.assertRaisesRegex(agentic.LayerError, "no layer A cell uses"):
            self.generate_with(CUE_CELLS=cells)

    def test_a_shape_whose_narrow_rule_pays_nothing_fails_generation(self) -> None:
        twins = tuple(t for t in agentic.CUE_TWINS if t.family != "age_twin_turned_degrees")
        with self.assertRaisesRegex(agentic.LayerError, r"narrow rule pays nothing in layer D: \['age_turned'\]"):
            self.generate_with(CUE_TWINS=twins)

    def test_an_object_age_in_layer_a_fails_generation(self) -> None:
        cells = self.replaced(agentic.CUE_CELLS, "age_turned_prose",
                              templates={"dev": "x", "test": "Then she turned {V} degrees."})
        with self.assertRaisesRegex(agentic.LayerError, "does not read as a age_turned value"):
            self.generate_with(CUE_CELLS=cells)

    def test_a_person_age_in_layer_d_fails_generation(self) -> None:
        twins = self.replaced(agentic.CUE_TWINS, "age_twin_turned_degrees",
                              templates={"dev": "x", "test": "She turned {X} last week."})
        with self.assertRaisesRegex(agentic.LayerError, "reads as a age_turned value"):
            self.generate_with(CUE_TWINS=twins)

    def test_a_birth_date_answer_in_layer_d_fails_generation(self) -> None:
        twins = self.replaced(agentic.CUE_TWINS, "dob_twin_last_login",
                              templates={"dev": "x", "test": "Your date of birth? It is {X}."})
        with self.assertRaisesRegex(agentic.LayerError, "reads as a dob_sentence_break value"):
            self.generate_with(CUE_TWINS=twins)

    def test_an_uncued_card_in_layer_a_fails_generation(self) -> None:
        cells = self.replaced(agentic.CUE_CELLS, "card_short_compact_prose",
                              templates={"dev": "x", "test": "Order {V} shipped."})
        with self.assertRaisesRegex(agentic.LayerError, "does not read as a card_short_compact value"):
            self.generate_with(CUE_CELLS=cells)

    def test_a_plain_card_twin_with_a_card_word_fails_generation(self) -> None:
        twins = self.replaced(agentic.CUE_TWINS, "card_twin_order",
                              templates={"dev": "x", "test": "Card reader order {X} shipped."})
        with self.assertRaisesRegex(agentic.LayerError, "carries a cue word"):
            self.generate_with(CUE_TWINS=twins)

    def test_a_near_cue_twin_without_its_cue_word_fails_generation(self) -> None:
        twins = self.replaced(agentic.CUE_TWINS, "zip_twin_se_near",
                              templates={"dev": "x", "test": "Lookup failed for batch {X}."})
        with self.assertRaisesRegex(agentic.LayerError, "lacks a cue word"):
            self.generate_with(CUE_TWINS=twins)

    def test_a_value_outside_its_shape_fails_generation(self) -> None:
        cells = self.replaced(agentic.CUE_CELLS, "zip_pl_csv", make=lambda rng, partition, index: "53320")
        with self.assertRaisesRegex(agentic.LayerError, "is not a zip_pl value"):
            self.generate_with(CUE_CELLS=cells)

    def test_a_twin_the_broad_rule_misses_fails_generation(self) -> None:
        twins = self.replaced(agentic.CUE_TWINS, "zip_twin_six_order_json",
                              make=lambda rng, partition, index: "12345")
        with self.assertRaisesRegex(agentic.LayerError, "broad pattern misses the decoy"):
            self.generate_with(CUE_TWINS=twins)


class UrlCellTests(unittest.TestCase):
    """Whole URLs that a structural delimiter ends in compact JSON, escaped
    JSON, HTML and Markdown; each shape's broad and narrow rules pay in its
    layer D twins, which carry no URL anchor."""

    @classmethod
    def setUpClass(cls) -> None:
        cls.full = {partition: agentic.generate(partition) for partition in agentic.PARTITIONS}
        cls.cells = {
            partition: [r for r in records if r.surface.startswith("url_")]
            for partition, records in cls.full.items()
        }

    def generate_with(self, **patches) -> None:
        with contextlib.ExitStack() as stack:
            for name, value in patches.items():
                stack.enter_context(mock.patch.object(agentic, name, value))
            agentic.generate("test")

    @staticmethod
    def replaced(cells: tuple, family: str, **changes) -> tuple:
        return tuple(dataclasses.replace(c, **changes) if c.family == family else c for c in cells)

    def test_every_cell_and_twin_is_generated_in_both_partitions(self) -> None:
        for partition, records in self.cells.items():
            for layer, cells in (("A", agentic.URL_CELLS), ("D", agentic.URL_TWINS)):
                for cell in cells:
                    matching = [r for r in records if r.layer == layer and r.family == cell.family]
                    self.assertEqual(len(matching), agentic.DOCS_PER_URL_CELL[layer], (partition, cell.family))
            for layer in ("A", "D"):
                self.assertEqual({r.surface for r in records if r.layer == layer}, set(agentic.URL_SURFACES))
        self.assertEqual({c.shape for c in agentic.URL_CELLS}, set(agentic.UrlShape))
        self.assertEqual({t.shape for t in agentic.URL_TWINS}, set(agentic.UrlShape))

    def test_growth_stays_within_ten_percent_per_layer(self) -> None:
        for layer in ("A", "D"):
            new = sum(1 for r in self.cells["test"] if r.layer == layer)
            old = sum(1 for r in self.full["test"] if r.layer == layer) - new
            self.assertLessEqual(new * 10, old, layer)

    def test_the_whole_url_is_gold(self) -> None:
        for records in self.cells.values():
            for record in (r for r in records if r.layer == "A"):
                (gold,) = record.gold
                self.assertEqual(gold.label, "URL")
                encoded = record.text.encode("utf-8")
                self.assertEqual(encoded[gold.start : gold.end].decode("utf-8"), gold.value)
                self.assertRegex(gold.value, r"^(?:https://|https:\\/\\/|www\.)")

    def test_structured_cells_parse(self) -> None:
        for records in self.cells.values():
            for record in records:
                if record.surface == "url_tool_json":
                    json.loads(record.text)

    def test_escaped_cells_decode_to_a_plain_url(self) -> None:
        # The escaped JSON value is a URL like the plain cells write; only the
        # slashes are escaped, as `json_encode()` writes them.
        families = {c.family for c in agentic.URL_CELLS if c.shape is agentic.UrlShape.ESCAPED}
        values = [r.gold[0].value for records in self.cells.values() for r in records
                  if r.layer == "A" and r.family in families]
        self.assertTrue(values)
        for value in values:
            decoded = json.loads(f'"{value}"')
            self.assertEqual(decoded, value.replace("\\/", "/"))
            self.assertRegex(decoded, f"^{agentic.URL_SHAPE_PATTERNS[agentic.UrlShape.PLAIN]}$")

    def test_templates_and_values_split_by_partition(self) -> None:
        for cell in (*agentic.URL_CELLS, *agentic.URL_TWINS):
            self.assertNotEqual(cell.templates["dev"], cell.templates["test"], cell.family)
        values = {p: {g.value for r in rs for g in r.gold} for p, rs in self.cells.items()}
        self.assertFalse(values["dev"] & values["test"])
        decoys = {p: {d.value for r in rs for d in r.decoys} for p, rs in self.cells.items()}
        self.assertFalse(decoys["dev"] & decoys["test"])

    def test_www_cells_write_the_scheme_and_bare_www_in_turn(self) -> None:
        for records in self.cells.values():
            for family in ("url_json_www_sibling", "url_json_escaped_www", "url_html_single_quoted"):
                values = [r.gold[0].value for r in records if r.family == family]
                self.assertEqual({v.startswith("https:") for v in values}, {True, False}, family)

    def test_each_broad_and_narrow_rule_catches_its_shape_and_pays_in_layer_d(self) -> None:
        for shape in agentic.UrlShape:
            families = {c.family for c in agentic.URL_CELLS if c.shape is shape}
            twins = {t.family for t in agentic.URL_TWINS if t.shape is shape}
            broad = re.compile(agentic.URL_BROAD_PATTERNS[shape])
            narrow = re.compile(agentic.URL_NARROW_PATTERNS[shape])
            for records in self.cells.values():
                for rule in (broad, narrow):
                    catches = [r for r in records if r.family in families and rule.search(r.text)]
                    self.assertEqual(len(catches), agentic.DOCS_PER_URL_CELL["A"] * len(families), (shape, rule))
                costs = [r for r in records if r.family in twins and broad.search(r.text)]
                self.assertEqual(len(costs), agentic.DOCS_PER_URL_CELL["D"] * len(twins), shape)
                self.assertTrue(any(r.family in twins and narrow.search(r.text) for r in records), shape)

    def test_no_layer_d_twin_carries_a_url_anchor(self) -> None:
        anchor = re.compile(agentic.URL_ANCHOR)
        for records in self.cells.values():
            for record in (r for r in records if r.layer == "D"):
                self.assertIsNone(anchor.search(record.text), record.uid)

    def test_mutant_policies_carry_the_broad_and_narrow_patterns(self) -> None:
        for name, table in (("broad", agentic.URL_BROAD_PATTERNS), ("narrow", agentic.URL_NARROW_PATTERNS)):
            path = REPO_ROOT / f"scripts/bench/fixtures/agentic/mutant-{name}-url-shapes.toml"
            patterns = re.findall(r"^pattern = '(.*)'$", path.read_text(encoding="utf-8"), flags=re.MULTILINE)
            self.assertEqual(sorted(patterns), sorted(table.values()), name)

    def test_the_shipped_url_rule_reaches_every_cell_and_no_twin(self) -> None:
        # Python spelling of `url.anchored` (`(?-u:\b)` is Rust syntax). Whatever
        # its body, its scheme or `www.` anchor never reaches a twin.
        core = (REPO_ROOT / "crates/gaze-recognizers/embedded/core.toml").read_text(encoding="utf-8")
        block = core[core.index('id = "url.anchored"'):]
        pattern = re.search(r"^pattern = '{3}(.*)'{3}$", block, flags=re.MULTILINE).group(1)
        rule = re.compile(pattern.replace("(?-u:\\b)", "\\b"))
        for records in self.cells.values():
            for record in records:
                if record.layer == "D":
                    self.assertIsNone(rule.search(record.text), record.uid)
                elif record.family not in {"url_json_escaped"}:
                    # The plain-anchor rule reaches every cell but the escaped
                    # scheme without `www.`.
                    self.assertIsNotNone(rule.search(record.text), record.uid)

    def test_self_closing_cell_ends_right_before_the_closing_quote(self) -> None:
        # `href='URL'/>`: the `'` is followed by `/`, which a URL may contain,
        # so only the quote itself can end the match.
        for records in self.cells.values():
            cells = [r for r in records if r.family == "url_html_self_closing"]
            self.assertEqual(len(cells), agentic.DOCS_PER_URL_CELL["A"])
            for record in cells:
                end = agentic._char_span(record.text, record.gold[0])[1]
                self.assertTrue(record.text[end:].startswith("'/>"), record.uid)

    def test_apostrophe_control_has_a_letter_after_a_mid_path_apostrophe(self) -> None:
        for records in self.cells.values():
            values = [r.gold[0].value for r in records if r.family == "url_prose_apostrophe"]
            self.assertEqual(len(values), agentic.DOCS_PER_URL_CELL["A"])
            for value in values:
                self.assertRegex(value, r"/[^/]*'[A-Za-z][^/]*$")

    def test_url_reading_examples(self) -> None:
        def reads(text, value):
            start = text.index(value)
            return agentic.url_reading(text, start, start + len(value))

        url = "https://members.example.invalid/u/anna_schmidt"
        self.assertTrue(reads(f'{{"w":"{url}","x":"1"}}', url))
        self.assertTrue(reads(f"<p>{url}</p>", url))
        self.assertTrue(reads(f"see [p]({url}).", url))
        self.assertFalse(reads(f'{{"w":"{url}x","x":"1"}}', url))
        self.assertFalse(reads(f'{{"w":"x{url}","x":"1"}}', url))
        self.assertFalse(reads('{"host":"cdn.example.invalid\\/v2"}', "cdn.example.invalid\\/v2"))

    def test_a_shape_without_a_twin_fails_generation(self) -> None:
        twins = tuple(t for t in agentic.URL_TWINS if t.shape is not agentic.UrlShape.ESCAPED)
        with self.assertRaisesRegex(agentic.LayerError, r"no layer D counterweight: \['escaped'\]"):
            self.generate_with(URL_TWINS=twins)

    def test_a_twin_no_cell_uses_fails_generation(self) -> None:
        cells = tuple(c for c in agentic.URL_CELLS if c.shape is not agentic.UrlShape.ESCAPED)
        with self.assertRaisesRegex(agentic.LayerError, "no layer A cell uses"):
            self.generate_with(URL_CELLS=cells)

    def test_a_shape_whose_narrow_rule_pays_nothing_fails_generation(self) -> None:
        twins = tuple(t for t in agentic.URL_TWINS if t.family != "url_twin_json_escaped_host")
        with self.assertRaisesRegex(agentic.LayerError, r"narrow rule pays nothing in layer D: \['escaped'\]"):
            self.generate_with(URL_TWINS=twins)

    def test_a_cut_url_in_layer_a_fails_generation(self) -> None:
        cells = self.replaced(agentic.URL_CELLS, "url_json_sibling",
                              templates={"dev": "x", "test": '{"w":"{V}/extra","x":"1"}'})
        with self.assertRaisesRegex(agentic.LayerError, "is not a whole URL in place"):
            self.generate_with(URL_CELLS=cells)

    def test_a_second_url_in_layer_a_fails_generation(self) -> None:
        cells = self.replaced(agentic.URL_CELLS, "url_html_text",
                              templates={"dev": "x", "test": "<p>{V}</p><p>www.example.invalid/a</p>"})
        with self.assertRaisesRegex(agentic.LayerError, "a URL anchor outside the gold value"):
            self.generate_with(URL_CELLS=cells)

    def test_a_value_outside_its_shape_fails_generation(self) -> None:
        cells = self.replaced(agentic.URL_CELLS, "url_json_escaped",
                              make=lambda rng, partition, index: "https://members.example.invalid/u/x")
        with self.assertRaisesRegex(agentic.LayerError, "is not a escaped URL"):
            self.generate_with(URL_CELLS=cells)

    def test_a_twin_with_a_url_anchor_fails_generation(self) -> None:
        twins = self.replaced(agentic.URL_TWINS, "url_twin_json_service_host",
                              make=lambda rng, partition, index: "https://svc.example.invalid/v2/health")
        with self.assertRaisesRegex(agentic.LayerError, "carries a URL anchor"):
            self.generate_with(URL_TWINS=twins)

    def test_a_twin_the_broad_rule_misses_fails_generation(self) -> None:
        twins = self.replaced(agentic.URL_TWINS, "url_twin_json_mime",
                              make=lambda rng, partition, index: "plain")
        with self.assertRaisesRegex(agentic.LayerError, "broad pattern misses the decoy"):
            self.generate_with(URL_TWINS=twins)


class PartitionTests(unittest.TestCase):
    def test_vocabularies_are_split_before_generation(self) -> None:
        pools = [
            agentic.GIVEN_NAMES, agentic.SURNAMES, agentic.EMAIL_DOMAINS,
            agentic.US_AREA_CODES, agentic.DE_MOBILE_PREFIXES,
            *agentic.KEYS.values(), *agentic.TEMPLATES.values(),
            *agentic.NAME_TEMPLATES.values(), *agentic.LOOKALIKE_TEMPLATES.values(),
            *agentic.REPEAT_BODIES.values(), agentic.REPEAT_HEADERS, agentic.REPEAT_SIGNOFFS,
            agentic.ID_REPEAT_TEMPLATES,
        ]
        for pool in pools:
            self.assertEqual(set(pool), set(agentic.PARTITIONS))
            dev, test = (
                {pool[p]} if isinstance(pool[p], str) else set(pool[p]) for p in ("dev", "test")
            )
            self.assertFalse(dev & test, pool)
        self.assertFalse(set(agentic.LOCAL_DATE_PROSE["dev"]) & set(agentic.LOCAL_DATE_PROSE["test"]))
        self.assertFalse(
            set(agentic.LOOKALIKE_KEYS["dev"].values())
            & set(agentic.LOOKALIKE_KEYS["test"].values())
        )
        self.assertNotEqual(agentic.PARTITION_SEEDS["dev"], agentic.PARTITION_SEEDS["test"])
        for pool in (agentic.WORD_NAMES, agentic.SUBSTRING_NAMES):
            dev_words = {entry[1] for entry in pool["dev"]} | {entry[2] for entry in pool["dev"]}
            test_words = {entry[1] for entry in pool["test"]} | {entry[2] for entry in pool["test"]}
            self.assertFalse(dev_words & test_words)

    def test_cue_text_is_not_a_split_axis_for_shared_standard_cues(self) -> None:
        # Standard cue words ("IBAN", "BSN") are the identifier's own name and
        # appear in both partitions; the prose around them never does.
        for cues in agentic.CUES.values():
            self.assertEqual(set(cues), set(agentic.PARTITIONS))

    def test_generated_values_templates_and_groups_are_disjoint(self) -> None:
        dev = agentic.generate("dev")
        test = agentic.generate("test")
        # An age cannot avoid the dev partition's 1-99 house numbers; ages are
        # split between the partitions themselves (19-56 dev, 57-94 test).
        def values(records: list, ages: bool) -> set[str]:
            return {g.value for r in records for g in r.gold if (g.label == "AGE") == ages}

        self.assertFalse(values(dev, False) & values(test, False))
        self.assertFalse(values(dev, True) & values(test, True))
        self.assertFalse({r.template for r in dev} & {r.template for r in test})
        self.assertFalse({r.group for r in dev} & {r.group for r in test})
        for records, partition in ((dev, "dev"), (test, "test")):
            groups: dict[str, set[str]] = {}
            for record in records:
                groups.setdefault(record.group, set()).add(record.partition)
            self.assertTrue(all(value == {partition} for value in groups.values()))


class ContractTests(unittest.TestCase):
    def documents(self) -> list[score.Document]:
        return [record.to_document() for record in agentic.generate("test")]

    def write_contract(self, directory: Path, mutate) -> Path:
        value = json.loads((REPO_ROOT / agentic.SCORED_LABELS_PATH).read_text(encoding="utf-8"))
        mutate(value)
        path = directory / "contract.json"
        path.write_text(json.dumps(value), encoding="utf-8")
        return path

    def test_committed_contract_rules_on_exactly_the_generated_labels(self) -> None:
        prepared = agentic.prepare(REPO_ROOT)
        self.assertEqual(prepared.manifest["corpus_sha256"], PINNED_CORPUS_SHA256["test"])
        self.assertTrue(prepared.identifiers and prepared.lookalikes)

    def test_unruled_label_fails_closed(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            path = self.write_contract(
                Path(temporary),
                lambda v: v.update(labels=[e for e in v["labels"] if e["label"] != "BSN"]),
            )
            contract = agentic.load_contract(REPO_ROOT, path)
            with self.assertRaisesRegex(agentic.LayerError, "BSN"):
                agentic.apply_contract(self.documents(), contract)

    def test_ruling_on_a_label_never_generated_fails_closed(self) -> None:
        def add_stale(value) -> None:
            value["labels"].append(
                {"label": "PASSPORTNUM", "scored": True, "ruling": "settled", "reason": "stale"}
            )

        with tempfile.TemporaryDirectory() as temporary:
            contract = agentic.load_contract(REPO_ROOT, self.write_contract(Path(temporary), add_stale))
            with self.assertRaisesRegex(agentic.LayerError, "PASSPORTNUM"):
                agentic.apply_contract(self.documents(), contract)

    def test_an_older_generator_loads_its_own_committed_contract(self) -> None:
        # A record measured on v4, v5 or v6 is rescored under the contract that
        # ruled on exactly the labels that generator emitted.
        for version in (4, 5):
            contract = agentic.load_contract(REPO_ROOT, version=version)
            self.assertNotIn("STREET", contract.scored_labels)
        self.assertIn("STREET", agentic.load_contract(REPO_ROOT, version=6).scored_labels)
        for version in (6, 7):
            self.assertNotIn("AGE", agentic.load_contract(REPO_ROOT, version=version).scored_labels)
        self.assertIn("AGE", agentic.load_contract(REPO_ROOT).scored_labels)
        for version in (7, 8):
            self.assertNotIn("URL", agentic.load_contract(REPO_ROOT, version=version).scored_labels)
        self.assertIn("URL", agentic.load_contract(REPO_ROOT).scored_labels)
        self.assertIn("STREET", agentic.load_contract(REPO_ROOT).scored_labels)
        with self.assertRaisesRegex(agentic.LayerError, "no committed scored-label contract"):
            agentic.load_contract(REPO_ROOT, version=3)
        with self.assertRaisesRegex(agentic.LayerError, "generator_version 9"):
            agentic.load_contract(REPO_ROOT, agentic.SCORED_LABELS_PATH, version=8)

    def test_generator_version_mismatch_fails_closed(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            path = self.write_contract(
                Path(temporary), lambda v: v["corpus"].update(generator_version=99)
            )
            with self.assertRaisesRegex(agentic.LayerError, "generator_version"):
                agentic.load_contract(REPO_ROOT, path)

    def test_excluded_label_keeps_the_cell(self) -> None:
        def exclude_dob(value) -> None:
            for entry in value["labels"]:
                if entry["label"] == "DATEOFBIRTH":
                    entry["scored"] = False

        with tempfile.TemporaryDirectory() as temporary:
            contract = agentic.load_contract(REPO_ROOT, self.write_contract(Path(temporary), exclude_dob))
            applied = agentic.apply_contract(self.documents(), contract)
        dob = [d for d in applied if d.excluded_spans]
        self.assertTrue(dob)
        self.assertTrue(all(
            d.cell and ("|dob|" in d.cell or "|birth_date_cue|" in d.cell or "|dob_sentence_break" in d.cell)
            for d in dob
        ))


def _success_response(document: score.Document) -> dict[str, object]:
    return {
        "fixture_id": document.uid,
        "clean_text": document.text,
        "manifest_spans": [],
        "pre_safety_text_len": None,
        "pre_safety_manifest_spans": None,
        "leak_suspects": [],
        "safety_net_mode": "off",
        "strict_would_reject": False,
        "initial_safety_net_stats": {
            "suspect_count": 0, "uncovered_count": 0, "partial_bleed_count": 0,
            "class_mismatch_count": 0, "locale_skipped_count": 0,
        },
        "post_policy_safety_net_stats": None,
        "restore": {
            "exact": True, "decision": "success", "unknown_token_count": 0,
            "manifest_bypass_count": 0, "fresh_pii_detected_count": 0,
            "phase_execution_mask": 1,
        },
        "manifest_integrity": {
            "spans": 0, "invalid_clean_bounds": 0, "invalid_raw_bounds": 0,
            "overlapping_clean_spans": 0, "non_monotonic_raw_spans": 0,
            "token_restore_failures": 0, "raw_value_mismatches": 0,
        },
        "timing": {"clean_ms": 1.0, "restore_ms": 0.3, "post_policy_scan_ms": None},
        "final_protection_trace": [],
    }


class PerCellTests(unittest.TestCase):
    def run_cell(self, documents: list[score.Document]) -> dict[str, object]:
        process = mock.Mock()
        process.__enter__ = mock.Mock(return_value=process)
        process.__exit__ = mock.Mock(return_value=False)
        process.message_deadline = 0
        process.exchange.side_effect = [_success_response(d) for d in documents]
        with tempfile.TemporaryDirectory(dir=REPO_ROOT) as temporary:
            with mock.patch.object(score, "BenchSubprocess", return_value=process):
                return score.run_config(
                    repo_root=REPO_ROOT, binary=Path(temporary) / "synthetic-runner",
                    config="rule-floor-extended", documents=documents,
                    model_dir=Path(temporary), opf_command=None, opf_checkpoint=None,
                    opf_daemon_socket=None, threshold=0.3, diagnostics_dir=Path(temporary),
                )

    def test_corpus_without_cells_emits_no_per_cell_block(self) -> None:
        document = score.Document("kiji-1", "Anna", "en", "US", "unit", (score.Span(0, 4, "GIVENNAME"),))
        self.assertNotIn("per_cell", self.run_cell([document]))

    def test_generated_documents_are_aggregated_per_cell(self) -> None:
        records = [r for r in agentic.generate("test") if r.family == "bsn" and r.surface == "csv"]
        result = self.run_cell([r.to_document() for r in records])
        cells = result["per_cell"]
        self.assertEqual(set(cells), {"A|bsn|csv|valid", "A|bsn|csv|invalid"})
        leaked = sum(cell["utf8_bytes"]["leaked"] for cell in cells.values())
        self.assertEqual(leaked, result["metrics"]["utf8_bytes"]["leaked"])
        self.assertEqual(cells["A|bsn|csv|valid"]["documents"], agentic.DOCS_PER_FAMILY)


def _scorecard(
    leaks: dict[str, int],
    fps: dict[str, int] | None = None,
    refused: dict[str, int] | None = None,
    twin_leak: int = 0,
    c_invalid_leak: int = 0,
    restore: dict[str, int] | None = None,
    manifests: dict[str, int] | None = None,
    guard_fp: int = 0,
) -> dict:
    """`leaks` is gated (valid) gold; `twin_leak` / `c_invalid_leak` add
    checksum-failed bytes to layers A and C. `guard_fp` is the part of layer
    D's FP bytes that falls on the credit guard's `ref_number_16` family."""
    fps = fps or {}
    refused = refused or {}

    def run(layer: str) -> dict:
        leaked = leaks[layer] + {"A": twin_leak, "C": c_invalid_leak}.get(layer, 0)
        completed = 10 - refused.get(layer, 0)
        block = {
            "config": "policy-file",
            "metrics": {"utf8_bytes": {"leaked": leaked, "false_positive": fps.get(layer, 0)}},
            "pipeline_availability": {
                "attempted_documents": 10,
                "completed_documents": completed,
                "failed_closed_documents": refused.get(layer, 0),
            },
            "pipeline_contract": {
                "documents": completed,
                "restore_exact_documents": (restore or {}).get(layer, completed),
                "manifest_valid_documents": (manifests or {}).get(layer, completed),
            },
        }
        if layer == "C":
            block["validator_recall_by_label"] = {
                "OTHER": {"production_recall_by_gold_validity": {
                    "validator_passed_gold": {"leaked_utf8_bytes": 0},
                    "validator_failed_gold": {"leaked_utf8_bytes": c_invalid_leak},
                }},
                "CITY": {"production_recall_by_gold_validity": None},
            }
        if layer == "D":
            block["per_cell"] = {
                "D|ref_number_16|prose|benign": {"utf8_bytes": {"leaked": 0, "false_positive": guard_fp}},
                **{
                    f"D|{family}|prose|benign": {"utf8_bytes": {"leaked": 0, "false_positive": 0}}
                    for family in ("ref_number_9", "ref_number_10", "ref_number_11", *CUE_CARD_TWINS)
                },
            }
        if layer == "A":
            block["per_cell"] = {
                "A|other|csv|valid": {"utf8_bytes": {"leaked": leaks["A"]}},
                "A|other|csv|invalid": {"utf8_bytes": {"leaked": twin_leak}},
            }
        return block

    return {
        "parameters": {"configs": ["policy-file"], "policy_sha256": "p", "ner_threshold": 0.3},
        "runner_provenance": {"model_bundles": [], "policy_dependencies": {"files": {}}},
        "dataset": {"integrity": {"sha256": "k"}},
        "scoring": {"scored_label_contract": {"id": "scored-labels-v2", "version": 2, "file_sha256": "c"}},
        "runs": [run("C")],
        "layers": {
            "generator": {"corpus_sha256": "g"},
            "scored_label_contract": {"file_sha256": "a"},
            "gold_validity": {"C": {"algorithm": "sha256", "entities": 3, "value": "a" * 64}},
            "A": {"runs": [run("A")]},
            "D": {"runs": [run("D")]},
            "R": {"runs": [run("R")]},
        },
    }


class CompositeSourceIdTests(unittest.TestCase):
    """v0.14.0 emits `email.header.name+ner`; only an explicit opt-in accepts it."""

    def response(self, source_id: str) -> tuple[score.Document, dict]:
        document = score.Document("t1", "From: Anna Weber <a@b.de>", "en", "US", "unit",
                                  (score.Span(6, 10, "GIVENNAME"),))
        response = _success_response(document)
        response["clean_text"] = "From: <tok> Weber <a@b.de>"
        response["manifest_spans"] = [
            {"raw_start": 6, "raw_end": 10, "clean_start": 6, "clean_end": 11, "class": "name"}
        ]
        response["manifest_integrity"]["spans"] = 1
        response["final_protection_trace"] = [{
            "raw_start": 6, "raw_end": 10, "class": "name", "action": "tokenize",
            "provenance": {"stage": "primary_pipeline", "decision": "policy", "source_ids": [source_id]},
        }]
        return document, response

    def test_composite_id_is_refused_by_default(self) -> None:
        document, response = self.response("email.header.name+ner")
        with self.assertRaisesRegex(score.ResponseValidationError, "stable identifier"):
            score.validate_response(document, response)

    def test_opt_in_accepts_a_composite_of_valid_ids(self) -> None:
        document, response = self.response("email.header.name+ner")
        score.validate_response(document, response, split_composite_source_ids=True)

    def test_opt_in_still_checks_every_part(self) -> None:
        document, response = self.response("email.header.name+Anna")
        with self.assertRaisesRegex(score.ResponseValidationError, "stable identifier"):
            score.validate_response(document, response, split_composite_source_ids=True)


class GateTests(unittest.TestCase):
    BASE = {"C": 100, "A": 50, "D": 0, "R": 30}
    FP = {"C": 10, "A": 5, "D": 7, "R": 3}

    def test_policy_input_and_model_digest_changes_are_not_comparable_on_either_side(self) -> None:
        for kind in ("file", "davlan", "nym", "gliner"):
            for side in ("base", "candidate"):
                with self.subTest(kind=kind, side=side):
                    cards = {name: _scorecard(self.BASE, self.FP) for name in ("base", "candidate")}
                    if kind == "file":
                        cards[side]["runner_provenance"]["policy_dependencies"]["files"] = {
                            "policy.rulepacks.paths[0]": "a" * 64
                        }
                    else:
                        model_id = {"davlan": "davlan-mbert-ner-hrl-onnx",
                                    "nym": "nym-small-int8",
                                    "gliner": "gliner-multi-pii-dob-int8"}[kind]
                        for card in cards.values():
                            card["runner_provenance"]["model_bundles"] = [
                                {"model_id": model_id, "observed_sha256": "a" * 64}]
                        cards[side]["runner_provenance"]["model_bundles"][0]["observed_sha256"] = "b" * 64
                    result = agentic.gate(cards["base"], cards["candidate"])
                    self.assertEqual(result["verdict"], "not_comparable")
                    self.assertTrue(any("policy input" in value or "model bundle" in value
                                        for value in result["differing"]))

    def test_missing_policy_dependency_identity_requires_explicit_legacy_flag(self) -> None:
        for side in ("base", "candidate"):
            cards = {name: _scorecard(self.BASE, self.FP) for name in ("base", "candidate")}
            del cards[side]["runner_provenance"]["policy_dependencies"]
            with self.subTest(side=side), self.assertRaisesRegex(agentic.LayerError, "policy-dependency identity"):
                agentic.gate(cards["base"], cards["candidate"])
            self.assertEqual(agentic.gate(cards["base"], cards["candidate"],
                                          allow_legacy_policy_inputs=True)["verdict"], "not_comparable")
        cards = {name: _scorecard(self.BASE, self.FP) for name in ("base", "candidate")}
        for card in cards.values():
            del card["runner_provenance"]["policy_dependencies"]
        self.assertEqual(agentic.gate(cards["base"], cards["candidate"],
                                      allow_legacy_policy_inputs=True)["verdict"], "fail")

    def test_policy_dependency_files_hash_referenced_inputs(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "pack.toml").write_text("pack=1", encoding="utf-8")
            (root / "terms.txt").write_text("synthetic-term", encoding="utf-8")
            (root / "model").mkdir()
            (root / "model" / "artifact.bin").write_bytes(b"model-a")
            policy = {"policy": {"rulepacks": {"paths": ["pack.toml"]},
                                 "custom_recognizers": [{"terms_file": "terms.txt"}]},
                      "ner": {"model_dir": "model"}}
            first = agentic.policy_dependency_files(policy, root)
            self.assertEqual(set(first), {"policy.rulepacks.paths[0]",
                                          "policy.custom_recognizers[0].terms_file",
                                          "ner.model_dir/artifact.bin"})
            for path, key in ((root / "pack.toml", "policy.rulepacks.paths[0]"),
                              (root / "terms.txt", "policy.custom_recognizers[0].terms_file"),
                              (root / "model" / "artifact.bin", "ner.model_dir/artifact.bin")):
                original = path.read_bytes()
                path.write_bytes(original + (b"\nchanged=2\n" if path.suffix == ".toml" else b"x"))
                changed = agentic.policy_dependency_files(policy, root)
                self.assertNotEqual(changed[key], first[key])
                path.write_bytes(original)

    def test_rulepack_dictionary_terms_change_makes_gate_not_comparable(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "pack.toml").write_text(
                '[[recognizers]]\nid="songs"\n[recognizers.match]\nkind="dictionary"\nterms_file="songs.txt"\n',
                encoding="utf-8",
            )
            terms = root / "songs.txt"
            terms.write_text("synthetic song A\n", encoding="utf-8")
            policy = {"policy": {"rulepacks": {"paths": ["pack.toml"]}}}
            first = agentic.policy_dependency_files(policy, root)
            terms.write_text("synthetic song B\n", encoding="utf-8")
            second = agentic.policy_dependency_files(policy, root)
            key = "policy.rulepacks.paths[0].recognizers[0].terms_file"
            self.assertEqual(first["policy.rulepacks.paths[0]"], second["policy.rulepacks.paths[0]"])
            self.assertNotEqual(first[key], second[key])
            base = _scorecard(self.BASE, self.FP)
            candidate = _scorecard({**self.BASE, "R": 10}, self.FP)
            base["runner_provenance"]["policy_dependencies"]["files"] = first
            candidate["runner_provenance"]["policy_dependencies"]["files"] = second
            result = agentic.gate(base, candidate)
            self.assertEqual(result["verdict"], "not_comparable")
            self.assertIn(f"policy input {key}", result["differing"])

    def test_active_model_directories_are_hashed_and_disabled_dob_is_skipped(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for name in ("nym", "dob"):
                (root / name).mkdir()
                (root / name / "weights.bin").write_bytes(name.encode())
            policy = {"safety_net": {"backend": "nym", "nym": {"model_dir": "nym"}},
                      "dob_judge": {"enabled": False, "model_dir": "dob"}}
            self.assertEqual(set(agentic.policy_dependency_files(policy, root)),
                             {"safety_net.nym.model_dir/weights.bin"})
            policy["dob_judge"]["enabled"] = True
            self.assertEqual(set(agentic.policy_dependency_files(policy, root)),
                             {"safety_net.nym.model_dir/weights.bin", "dob_judge.model_dir/weights.bin"})

    def test_unreferenced_model_dotfiles_do_not_change_identity(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            model = root / "model"
            model.mkdir()
            (model / "weights.bin").write_bytes(b"model")
            policy = {"ner": {"model_dir": "model"}}
            first = agentic.policy_dependency_files(policy, root)
            (model / ".DS_Store").write_bytes(b"local metadata")
            (model / ".cache").mkdir()
            (model / ".cache" / "index").write_bytes(b"cache")
            self.assertEqual(first, agentic.policy_dependency_files(policy, root))

    def test_active_model_under_dotted_parent_is_hashed_and_empty_model_is_refused(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            model = root / ".local" / "share" / "gaze" / "models" / "ner"
            model.mkdir(parents=True)
            weight = model / "weights.bin"
            weight.write_bytes(b"synthetic model")
            policies = {
                "ner": {"ner": {"model_dir": str(model)}},
                "nym": {"safety_net": {"backend": "nym", "nym": {"model_dir": str(model)}}},
                "dob": {"dob_judge": {"enabled": True, "model_dir": str(model)}},
            }
            references = {
                "ner": "ner.model_dir/weights.bin",
                "nym": "safety_net.nym.model_dir/weights.bin",
                "dob": "dob_judge.model_dir/weights.bin",
            }
            for name, policy in policies.items():
                with self.subTest(name=name):
                    self.assertEqual(
                        agentic.policy_dependency_files(policy, root),
                        {references[name]: hashlib.sha256(weight.read_bytes()).hexdigest()},
                    )
            weight.unlink()
            (model / ".DS_Store").write_bytes(b"ignored metadata")
            for name, policy in policies.items():
                with self.subTest(name=name), self.assertRaisesRegex(
                    agentic.LayerError, "no hashable files"
                ):
                    agentic.policy_dependency_files(policy, root)
    def test_restore_and_manifest_drop_fail_on_every_layer(self) -> None:
        for layer in agentic.GATE_LAYERS:
            for field in ("restore", "manifests"):
                with self.subTest(layer=layer, field=field):
                    base = _scorecard(self.BASE, self.FP)
                    candidate = _scorecard({**self.BASE, "R": self.BASE["R"] - 1}, self.FP,
                                           **{field: {layer: 9}})
                    result = agentic.gate(base, candidate)
                    self.assertEqual(result["verdict"], "fail")
                    self.assertEqual(result["reason"],
                                     f"{'exact-restore documents fell' if field == 'restore' else 'valid-manifest documents fell'} in ['{layer}']")

    def test_restore_and_manifest_rise_count_as_improvement(self) -> None:
        for layer in agentic.GATE_LAYERS:
            for field in ("restore", "manifests"):
                with self.subTest(layer=layer, field=field):
                    base = _scorecard(self.BASE, self.FP, **{field: {layer: 9}})
                    candidate = _scorecard(self.BASE, self.FP)
                    self.assertEqual(agentic.gate(base, candidate)["verdict"], "pass")
                    # A byte regression cannot be masked by a restore gain.
                    candidate = _scorecard(self.BASE, {**self.FP, "D": 8})
                    self.assertEqual(agentic.gate(base, candidate)["verdict"], "fail")

    def test_missing_or_invalid_restore_counts_fail_closed_on_both_sides(self) -> None:
        for side in ("base", "candidate"):
            for layer in agentic.GATE_LAYERS:
                for field in ("restore_exact_documents", "manifest_valid_documents"):
                    with self.subTest(side=side, layer=layer, field=field):
                        cards = {"base": _scorecard(self.BASE, self.FP),
                                 "candidate": _scorecard(self.BASE, self.FP)}
                        agentic._layer_run(cards[side], layer, "policy-file")["pipeline_contract"].pop(field)
                        with self.assertRaisesRegex(agentic.LayerError, f"layer {layer}.*{field}"):
                            agentic.gate(cards["base"], cards["candidate"])

    def test_missing_pipeline_contract_fails_closed_on_both_sides(self) -> None:
        for side in ("base", "candidate"):
            for layer in agentic.GATE_LAYERS:
                with self.subTest(side=side, layer=layer):
                    cards = {name: _scorecard(self.BASE, self.FP) for name in ("base", "candidate")}
                    del agentic._layer_run(cards[side], layer, "policy-file")["pipeline_contract"]
                    with self.assertRaisesRegex(agentic.LayerError, f"layer {layer} has no pipeline_contract"):
                        agentic.gate(cards["base"], cards["candidate"])

    def test_contract_counts_cannot_exceed_completed_documents(self) -> None:
        for field in ("restore_exact_documents", "manifest_valid_documents"):
            candidate = _scorecard(self.BASE, self.FP)
            candidate["runs"][0]["pipeline_contract"][field] = 11
            with self.subTest(field=field), self.assertRaisesRegex(agentic.LayerError, "counts exceed documents"):
                agentic.gate(_scorecard(self.BASE, self.FP), candidate)

    def test_restore_gain_cannot_come_from_more_documents(self) -> None:
        base = _scorecard(self.BASE, self.FP, refused={"C": 1})
        candidate = _scorecard(self.BASE, self.FP)
        result = agentic.gate(base, candidate)
        self.assertEqual(result["verdict"], "fail")
        self.assertIn("without an eligible gain", result["reason"])

    def test_refusal_fix_can_pass_and_refusal_rise_keeps_its_reason(self) -> None:
        base = _scorecard(self.BASE, self.FP, refused={"C": 1})
        candidate = _scorecard({**self.BASE, "C": 98}, self.FP)
        self.assertEqual(agentic.gate(base, candidate)["verdict"], "pass")
        result = agentic.gate(candidate, base)
        self.assertEqual(result["verdict"], "fail")
        self.assertIn("failed-closed documents rose", result["reason"])

    def test_refusal_fix_cannot_hide_new_restore_or_manifest_failure(self) -> None:
        for field, reason in (("restore", "exact-restore failures rose"),
                              ("manifests", "invalid-manifest documents rose")):
            with self.subTest(field=field):
                base = _scorecard(self.BASE, self.FP, refused={"C": 1})
                candidate = _scorecard({**self.BASE, "C": 98}, self.FP,
                                       **{field: {"C": 9}})
                result = agentic.gate(base, candidate)
                self.assertEqual(result["verdict"], "fail")
                self.assertIn(reason, result["reason"])
                self.assertEqual(result["layers"]["C"]["restore_failures_base"], 0)
                self.assertEqual(result["layers"]["C"]["manifest_invalid_base"], 0)

    def test_missing_availability_counts_fail_closed_on_both_sides(self) -> None:
        for side in ("base", "candidate"):
            for layer in agentic.GATE_LAYERS:
                for field in ("attempted_documents", "completed_documents", "failed_closed_documents"):
                    with self.subTest(side=side, layer=layer, field=field):
                        cards = {name: _scorecard(self.BASE, self.FP) for name in ("base", "candidate")}
                        agentic._layer_run(cards[side], layer, "policy-file")["pipeline_availability"].pop(field)
                        with self.assertRaisesRegex(agentic.LayerError, f"layer {layer}.*{field}"):
                            agentic.gate(cards["base"], cards["candidate"])

    def test_inconsistent_availability_counts_fail_closed(self) -> None:
        for field, value in (("completed_documents", 9), ("attempted_documents", 11)):
            with self.subTest(field=field):
                candidate = _scorecard(self.BASE, self.FP)
                candidate["runs"][0]["pipeline_availability"][field] = value
                with self.assertRaisesRegex(agentic.LayerError, "pipeline document counts disagree"):
                    agentic.gate(_scorecard(self.BASE, self.FP), candidate)

    def test_attempted_population_change_fails(self) -> None:
        candidate = _scorecard(self.BASE, self.FP)
        candidate["runs"][0]["pipeline_availability"]["attempted_documents"] = 11
        candidate["runs"][0]["pipeline_availability"]["completed_documents"] = 11
        candidate["runs"][0]["pipeline_contract"]["documents"] = 11
        result = agentic.gate(_scorecard(self.BASE, self.FP), candidate)
        self.assertEqual(result["verdict"], "fail")
        self.assertIn("attempted document counts differ", result["reason"])

    def test_fp_and_restore_gain_are_both_named(self) -> None:
        base = _scorecard(self.BASE, self.FP, restore={"C": 9})
        candidate = _scorecard(self.BASE, {**self.FP, "D": 6})
        result = agentic.gate(base, candidate)
        self.assertEqual(result["verdict"], "pass")
        self.assertIn("FP bytes fell", result["reason"])
        self.assertIn("exact restore rose", result["reason"])

    def verdict(self, candidate: dict, base: dict | None = None) -> str:
        return agentic.gate(base or _scorecard(self.BASE, self.FP), candidate)["verdict"]

    def test_leak_fix_with_smaller_fp_rise_passes(self) -> None:
        # 20 leaked bytes saved, 19 FP bytes added across layers: net better.
        candidate = _scorecard({**self.BASE, "R": 10}, {**self.FP, "D": 7 + 19})
        self.assertEqual(self.verdict(candidate), "pass")

    def test_leak_fix_with_equal_or_larger_fp_rise_fails(self) -> None:
        for fp_rise in (20, 21):
            with self.subTest(fp_rise=fp_rise):
                candidate = _scorecard({**self.BASE, "R": 10}, {**self.FP, "D": 7 + fp_rise})
                self.assertEqual(self.verdict(candidate), "fail")

    def test_fp_rise_is_summed_over_all_layers(self) -> None:
        candidate = _scorecard({**self.BASE, "R": 10}, {"C": 20, "A": 15, "D": 7, "R": 3})
        result = agentic.gate(_scorecard(self.BASE, self.FP), candidate)
        self.assertEqual(result["summary"], {"leaked_bytes_decrease": 20, "false_positive_bytes_increase": 20})
        self.assertEqual(result["verdict"], "fail")

    def test_leak_rising_in_any_layer_fails_even_if_another_falls(self) -> None:
        self.assertEqual(self.verdict(_scorecard({**self.BASE, "C": 101, "A": 10}, self.FP)), "fail")
        self.assertEqual(self.verdict(_scorecard({**self.BASE, "R": 31, "A": 10}, self.FP)), "fail")

    def test_more_refusals_fail_because_refused_documents_leave_the_leak_count(self) -> None:
        candidate = _scorecard({**self.BASE, "C": 90}, self.FP, {"C": 1})
        self.assertEqual(self.verdict(candidate), "fail")

    def test_false_positive_only_fix_passes(self) -> None:
        self.assertEqual(self.verdict(_scorecard(self.BASE, {**self.FP, "D": 2})), "pass")

    def test_no_movement_fails(self) -> None:
        self.assertEqual(self.verdict(_scorecard(self.BASE, self.FP)), "fail")

    def test_checksum_invalid_twins_are_reported_not_gated(self) -> None:
        base = _scorecard(self.BASE, self.FP, twin_leak=500)
        # Tagging every twin saves 500 "leaked" bytes that no precise rule can reach.
        candidate = _scorecard(self.BASE, {**self.FP, "D": 7 + 100}, twin_leak=0)
        result = agentic.gate(base, candidate)
        self.assertEqual(result["verdict"], "fail")
        self.assertEqual(result["layers"]["A"]["twin_leaked_base"], 500)
        self.assertEqual(result["summary"]["leaked_bytes_decrease"], 0)

    @staticmethod
    def _with_invalid(scorecard: dict, a_cell: str, c_label: str, leaked: int) -> dict:
        """Add `leaked` checksum-invalid bytes to one layer A twin cell and one layer C label."""
        scorecard = copy.deepcopy(scorecard)
        run_a = scorecard["layers"]["A"]["runs"][0]
        run_a["per_cell"][a_cell] = {"utf8_bytes": {"leaked": leaked}}
        run_a["metrics"]["utf8_bytes"]["leaked"] += leaked
        run_c = scorecard["runs"][0]
        run_c["validator_recall_by_label"][c_label] = {"production_recall_by_gold_validity": {
            "validator_passed_gold": {"leaked_utf8_bytes": 0},
            "validator_failed_gold": {"leaked_utf8_bytes": leaked},
        }}
        run_c["metrics"]["utf8_bytes"]["leaked"] += leaked
        return scorecard

    def test_checksum_invalid_iban_and_card_gold_is_gated(self) -> None:
        # User ruling 2026-09-27: IBAN and card numbers are tokenized without a
        # checksum, so their invalid gold counts like valid gold.
        for a_cell, c_label in (("A|iban_de|prose_cue|invalid", "IBAN"),
                                ("A|card|csv|invalid", "CREDITCARDNUMBER")):
            with self.subTest(label=c_label):
                base = self._with_invalid(_scorecard(self.BASE, self.FP), a_cell, c_label, 300)
                candidate = self._with_invalid(
                    _scorecard(self.BASE, {**self.FP, "D": 7 + 100}), a_cell, c_label, 0)
                result = agentic.gate(base, candidate)
                self.assertEqual(result["layers"]["A"]["leaked_base"], self.BASE["A"] + 300)
                self.assertEqual(result["layers"]["A"]["twin_leaked_base"], 0)
                self.assertEqual(result["layers"]["C"]["leaked_base"], self.BASE["C"] + 300)
                self.assertEqual(result["layers"]["C"]["twin_leaked_base"], 0)
                self.assertEqual(result["summary"]["leaked_bytes_decrease"], 600)
                self.assertEqual(result["verdict"], "pass")

    def test_iban_and_card_uncued_invalid_twins_keep_all_surface_credit(self) -> None:
        for family, label in (("iban_de", "IBAN"), ("card", "CREDITCARDNUMBER")):
            with self.subTest(label=label):
                cell = f"A|{family}|prose_nocue|invalid"
                base = self._with_invalid(_scorecard(self.BASE, self.FP), cell, "OTHER", 300)
                candidate = self._with_invalid(_scorecard(self.BASE, self.FP), cell, "OTHER", 0)
                result = agentic.gate(base, candidate)
                self.assertEqual(result["layers"]["A"]["twin_leaked_base"], 0)
                self.assertEqual(result["layers"]["A"]["leaked_base"], self.BASE["A"] + 300)
                self.assertEqual(result["summary"]["leaked_bytes_decrease"], 300)

    def test_checksum_invalid_gold_of_other_labels_stays_ungated(self) -> None:
        base = self._with_invalid(_scorecard(self.BASE, self.FP), "A|other|csv|invalid", "OTHER", 300)
        candidate = self._with_invalid(
            _scorecard(self.BASE, {**self.FP, "D": 7 + 100}), "A|other|csv|invalid", "OTHER", 0)
        result = agentic.gate(base, candidate)
        self.assertEqual(result["layers"]["A"]["twin_leaked_base"], 300)
        self.assertEqual(result["layers"]["C"]["twin_leaked_base"], 300)
        self.assertEqual(result["summary"]["leaked_bytes_decrease"], 0)
        self.assertEqual(result["verdict"], "fail")

    def test_credit_guard_fails_any_fp_rise_on_the_card_counterweight(self) -> None:
        # 600 credited card bytes saved cannot pay for 1 FP byte on ref_number_16.
        base = self._with_invalid(_scorecard(self.BASE, self.FP), "A|card|csv|invalid", "CREDITCARDNUMBER", 300)
        candidate = self._with_invalid(
            _scorecard(self.BASE, {**self.FP, "D": 7 + 1}, guard_fp=1), "A|card|csv|invalid", "CREDITCARDNUMBER", 0)
        result = agentic.gate(base, candidate)
        self.assertEqual(result["summary"]["leaked_bytes_decrease"], 600)
        self.assertEqual(result["verdict"], "fail")
        self.assertIn("credit guard", result["reason"])

    def test_credit_guard_fails_even_without_any_leak_change(self) -> None:
        result = agentic.gate(_scorecard(self.BASE, self.FP),
                              _scorecard(self.BASE, {**self.FP, "D": 7 + 3}, guard_fp=3))
        self.assertEqual(result["verdict"], "fail")

    def test_credit_guard_allows_an_fp_fall(self) -> None:
        base = _scorecard(self.BASE, self.FP, guard_fp=5)
        candidate = _scorecard(self.BASE, {**self.FP, "D": 7 - 5}, guard_fp=0)
        self.assertEqual(agentic.gate(base, candidate)["verdict"], "pass")

    def test_credit_guard_without_its_family_fails_closed(self) -> None:
        candidate = _scorecard(self.BASE, self.FP)
        candidate["layers"]["D"]["runs"][0]["per_cell"] = {}
        with self.assertRaises(agentic.LayerError):
            agentic.gate(_scorecard(self.BASE, self.FP), candidate)

    def test_credit_guard_families_come_from_the_counterweights(self) -> None:
        self.assertEqual(agentic.CREDIT_GUARD_FAMILIES,
                         {"CREDITCARDNUMBER": (*CUE_CARD_TWINS, "ref_number_16"), "IBAN": (),
                          "TAXNUM": ("ref_number_11",), "CPF": ("ref_number_11",),
                          "BSN": ("ref_number_9",), "NHSNUMBER": ("ref_number_10",),
                          "PHONENUMBER": ()})
        self.assertEqual(set(agentic.CREDIT_GUARD_FAMILIES), set(agentic.CREDIT_SCOPE_BY_LABEL))

    def test_invalid_gold_credit_scopes_match_the_two_user_rulings(self) -> None:
        self.assertEqual(agentic.GATE_CREDIT_VERSION, 2)
        self.assertEqual(agentic.CREDIT_SCOPE_BY_LABEL, {
            "IBAN": agentic.CreditScope.ALL,
            "CREDITCARDNUMBER": agentic.CreditScope.ALL,
            "PHONENUMBER": agentic.CreditScope.CUED,
            "TAXNUM": agentic.CreditScope.CUED,
            "CPF": agentic.CreditScope.CUED,
            "BSN": agentic.CreditScope.CUED,
            "NHSNUMBER": agentic.CreditScope.CUED,
        })
        labels = {family.label for family in agentic.IDENTIFIER_FAMILIES}
        self.assertLessEqual(set(agentic.CREDIT_SCOPE_BY_LABEL) - {"PHONENUMBER"}, labels)
        self.assertNotIn("PHONENUMBER", labels)

    def test_cued_invalid_gold_is_credited_for_each_new_class(self) -> None:
        for family, label in (("steuer_id", "TAXNUM"), ("cpf", "CPF"),
                              ("bsn", "BSN"), ("nhs", "NHSNUMBER")):
            with self.subTest(label=label):
                cell = f"A|{family}|prose_cue|invalid"
                base = self._with_invalid(_scorecard(self.BASE, self.FP), cell, label, 300)
                candidate = self._with_invalid(_scorecard(self.BASE, self.FP), cell, label, 0)
                result = agentic.gate(base, candidate)
                self.assertEqual(result["gate_credit_version"], 2)
                self.assertEqual(result["layers"]["A"]["twin_leaked_base"], 0)
                self.assertEqual(result["layers"]["C"]["twin_leaked_base"], 0)
                self.assertEqual(result["summary"]["leaked_bytes_decrease"], 600)
                self.assertEqual(result["verdict"], "pass")

    def test_uncued_invalid_twins_stay_excluded_for_each_new_layer_a_class(self) -> None:
        for family in ("steuer_id", "cpf", "bsn", "nhs"):
            with self.subTest(family=family):
                cell = f"A|{family}|prose_nocue|invalid"
                base = self._with_invalid(_scorecard(self.BASE, self.FP), cell, "OTHER", 300)
                candidate = self._with_invalid(
                    _scorecard(self.BASE, {**self.FP, "D": self.FP["D"] + 100}), cell, "OTHER", 0)
                result = agentic.gate(base, candidate)
                self.assertEqual(result["layers"]["A"]["twin_leaked_base"], 300)
                self.assertEqual(result["layers"]["A"]["leaked_base"], self.BASE["A"])
                self.assertEqual(result["summary"]["leaked_bytes_decrease"], 0)
                self.assertEqual(result["verdict"], "fail")

    def test_failed_validator_phone_gold_is_credited_in_layer_c(self) -> None:
        base = self._with_invalid(_scorecard(self.BASE, self.FP),
                                  "A|other|prose_cue|invalid", "PHONENUMBER", 300)
        candidate = self._with_invalid(_scorecard(self.BASE, self.FP),
                                       "A|other|prose_cue|invalid", "PHONENUMBER", 0)
        result = agentic.gate(base, candidate)
        self.assertEqual(result["layers"]["A"]["twin_leaked_base"], 300)
        self.assertEqual(result["layers"]["C"]["twin_leaked_base"], 0)
        self.assertEqual(result["summary"]["leaked_bytes_decrease"], 300)
        self.assertEqual(result["verdict"], "pass")

    def test_uncued_counterweight_fp_cannot_be_paid_for_by_credit(self) -> None:
        for family in ("ref_number_9", "ref_number_10", "ref_number_11", "ref_number_16"):
            with self.subTest(family=family):
                base = _scorecard(self.BASE, self.FP)
                candidate = _scorecard({**self.BASE, "C": self.BASE["C"] - 100},
                                       {**self.FP, "D": self.FP["D"] + 1})
                candidate["layers"]["D"]["runs"][0]["per_cell"][f"D|{family}|prose|benign"]["utf8_bytes"]["false_positive"] = 1
                result = agentic.gate(base, candidate)
                self.assertEqual(result["verdict"], "fail")
                self.assertIn("credit guard", result["reason"])

    def test_kiji_gold_that_fails_its_validator_is_reported_not_gated(self) -> None:
        base = _scorecard(self.BASE, self.FP, c_invalid_leak=400)
        candidate = _scorecard(self.BASE, {**self.FP, "D": 7 + 100}, c_invalid_leak=0)
        result = agentic.gate(base, candidate)
        self.assertEqual(result["verdict"], "fail")
        self.assertEqual(result["layers"]["C"]["twin_leaked_base"], 400)
        self.assertEqual(result["layers"]["C"]["leaked_base"], self.BASE["C"])

    def test_reviewer_demo_validator_regression_fails_on_headline_leak(self) -> None:
        # REVIEW 666 G1: a phone-validator regression vetoes 140 B of valid
        # gold; the candidate's own probe then calls that gold validator-failed,
        # so the gated C leak stays flat while the headline goes 13,045 -> 13,185
        # and 5 FP bytes disappear. It must not pass as an FP-only fix.
        base = _scorecard({**self.BASE, "C": 7_793}, self.FP, c_invalid_leak=5_252)
        candidate = _scorecard(
            {**self.BASE, "C": 7_793}, {**self.FP, "C": self.FP["C"] - 5}, c_invalid_leak=5_392
        )
        result = agentic.gate(base, candidate)
        self.assertEqual(result["layers"]["C"]["headline_leaked_base"], 13_045)
        self.assertEqual(result["layers"]["C"]["headline_leaked_candidate"], 13_185)
        self.assertEqual(result["layers"]["C"]["leaked_candidate"], result["layers"]["C"]["leaked_base"])
        self.assertEqual(result["verdict"], "fail")
        self.assertIn("leaked bytes rose in ['C']", result["reason"])

    def test_a_different_gold_classification_is_not_comparable(self) -> None:
        base = _scorecard(self.BASE, self.FP)
        candidate = _scorecard(self.BASE, self.FP)
        base["layers"]["gold_validity"] = {"C": {"algorithm": "sha256", "entities": 3, "value": "a" * 64}}
        candidate["layers"]["gold_validity"] = {"C": {"algorithm": "sha256", "entities": 3, "value": "b" * 64}}
        result = agentic.gate(base, candidate)
        self.assertEqual(result["verdict"], "not_comparable")
        self.assertIn("layer_c_gold_validity", result["differing"])

    def test_missing_gold_validity_digest_is_refused_on_either_side(self) -> None:
        for missing_side in ("both", "base", "candidate"):
            with self.subTest(missing_side=missing_side):
                base = _scorecard(self.BASE, self.FP)
                candidate = _scorecard(self.BASE, self.FP)
                if missing_side in ("both", "base"):
                    del base["layers"]["gold_validity"]["C"]
                if missing_side in ("both", "candidate"):
                    del candidate["layers"]["gold_validity"]["C"]
                with self.assertRaisesRegex(agentic.LayerError, "predates the gold-validity digest"):
                    agentic.gate(base, candidate)

    def test_reviewer_demo_relabelling_without_digest_is_refused(self) -> None:
        # REVIEW 666 G2: reclassifying 100 already leaked valid phone bytes as
        # validator-failed creates a gated gain without changing headline leak.
        base = _scorecard({**self.BASE, "C": 200}, self.FP)
        candidate = _scorecard({**self.BASE, "C": 100}, self.FP, c_invalid_leak=100)
        del base["layers"]["gold_validity"]
        del candidate["layers"]["gold_validity"]
        with self.assertRaisesRegex(agentic.LayerError, "predates the gold-validity digest"):
            agentic.gate(base, candidate)

    def test_implicit_v1_contract_pair_reaches_a_verdict(self) -> None:
        base = _scorecard(self.BASE, self.FP)
        candidate = _scorecard(self.BASE, self.FP)
        for card in (base, candidate):
            del card["scoring"]["scored_label_contract"]
        self.assertEqual(agentic.gate(base, candidate)["verdict"], "fail")

    def test_v1_and_v2_contracts_are_not_comparable(self) -> None:
        base = _scorecard(self.BASE, self.FP)
        candidate = _scorecard(self.BASE, self.FP)
        del base["scoring"]["scored_label_contract"]
        result = agentic.gate(base, candidate)
        self.assertEqual(result["verdict"], "not_comparable")
        self.assertIn("kiji_contract", result["differing"])

    def test_other_contracts_with_missing_file_sha_are_refused(self) -> None:
        for contract_id, version in ((score.SCORED_LABEL_CONTRACT_V1_ID, 2), ("scored-labels-v2", 1)):
            with self.subTest(contract_id=contract_id, version=version):
                candidate = _scorecard(self.BASE, self.FP)
                candidate["scoring"]["scored_label_contract"] = {
                    "id": contract_id, "version": version, "file_sha256": None,
                }
                with self.assertRaisesRegex(agentic.LayerError, "no gate identity for kiji_contract"):
                    agentic.gate(_scorecard(self.BASE, self.FP), candidate)

    def test_missing_other_gate_identity_is_refused(self) -> None:
        for path in (
            ("dataset", "integrity"),
            ("layers", "generator", "corpus_sha256"),
            ("layers", "scored_label_contract", "file_sha256"),
            ("parameters", "configs"),
            ("parameters", "policy_sha256"),
            ("scoring", "scored_label_contract", "file_sha256"),
        ):
            with self.subTest(path=path):
                base = _scorecard(self.BASE, self.FP)
                candidate = _scorecard(self.BASE, self.FP)
                for card in (base, candidate):
                    target = card
                    for key in path[:-1]:
                        target = target[key]
                    del target[path[-1]]
                with self.assertRaisesRegex(agentic.LayerError, "no gate identity"):
                    agentic.gate(base, candidate)

    def test_gold_validity_digest_moves_with_any_single_verdict(self) -> None:
        documents = [score.Document("d1", "Tel 0301234567", "de", "DE", "unit",
                                    (score.Span(4, 14, "PHONENUMBER"),))]
        def measurements(passed: bool) -> dict:
            return {"documents": {"d1": {"gold_validation": [
                {"label": "PHONENUMBER", "applicable": True, "validator_passed": passed}]}}}
        passed = agentic.gold_validity_digest(documents, measurements(True))
        failed = agentic.gold_validity_digest(documents, measurements(False))
        self.assertEqual(passed, agentic.gold_validity_digest(documents, measurements(True)))
        self.assertNotEqual(passed["value"], failed["value"])
        self.assertEqual(passed["entities"], 1)

    def test_layer_c_without_a_validator_split_fails_closed(self) -> None:
        candidate = _scorecard(self.BASE, self.FP)
        del candidate["runs"][0]["validator_recall_by_label"]
        with self.assertRaisesRegex(agentic.LayerError, "validator split"):
            agentic.gate(_scorecard(self.BASE, self.FP), candidate)

    def test_different_corpus_or_contract_is_not_comparable(self) -> None:
        for path in (("layers", "generator", "corpus_sha256"), ("scoring", "scored_label_contract", "file_sha256")):
            candidate = _scorecard({"C": 1, "A": 1, "D": 0, "R": 1})
            target = candidate
            for key in path[:-1]:
                target = target[key]
            target[path[-1]] = "other"
            with self.subTest(path=path):
                self.assertEqual(self.verdict(candidate), "not_comparable")

    def test_scorecard_without_layers_is_refused(self) -> None:
        candidate = copy.deepcopy(_scorecard(self.BASE))
        del candidate["layers"]
        with self.assertRaises(agentic.LayerError):
            agentic.gate(_scorecard(self.BASE), candidate)


class PolicyDeltaGateTests(unittest.TestCase):
    BASE_LEAKS = {"C": 100, "A": 50, "D": 0, "R": 30}

    def test_delta_accepts_only_dependencies_owned_by_new_section(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "model").mkdir()
            (root / "model" / "artifact.bin").write_bytes(b"gliner-bundle")
            base_text = "[rules]\nenabled=true\n"
            added = f'[dob_judge]\nenabled=true\nmodel_dir="{root / "model"}"\n'
            paths = {name: root / f"{name}.toml" for name in ("base", "candidate", "delta")}
            for name, content in (("base", base_text), ("candidate", base_text + added), ("delta", added)):
                paths[name].write_text(content, encoding="utf-8")
            cards = {name: _scorecard(self.BASE_LEAKS if name == "base" else
                                      {**self.BASE_LEAKS, "R": 10}) for name in ("base", "candidate")}
            for name, card in cards.items():
                digest = hashlib.sha256(paths[name].read_bytes()).hexdigest()
                card["parameters"]["policy_sha256"] = digest
                card["runner_provenance"]["policy"] = {"path": str(paths[name]), "sha256": digest}
                card["runner_provenance"]["policy_dependencies"]["files"] = (
                    agentic.policy_dependency_files({"dob_judge": {"enabled": True,
                                                    "model_dir": str(root / "model")}}, root)
                    if name == "candidate" else {}
                )
            model_id = "gliner-multi-pii-dob-int8"
            cards["candidate"]["runner_provenance"]["model_bundles"] = [
                {"model_id": model_id, "observed_sha256": "a" * 64}]
            result = agentic.gate(cards["base"], cards["candidate"], policy_delta=paths["delta"])
            self.assertEqual(result["verdict"], "pass")
            for other_model in ("davlan-mbert-ner-hrl-onnx", "nym-small-int8"):
                with self.subTest(other_model=other_model):
                    cards["base"]["runner_provenance"]["model_bundles"] = [
                        {"model_id": other_model, "observed_sha256": "a" * 64}]
                    cards["candidate"]["runner_provenance"]["model_bundles"].append(
                        {"model_id": other_model, "observed_sha256": "b" * 64})
                    result = agentic.gate(cards["base"], cards["candidate"], policy_delta=paths["delta"])
                    self.assertEqual(result["verdict"], "not_comparable")
                    self.assertIn(f"model bundle {other_model}", result["differing"])
                    cards["base"]["runner_provenance"]["model_bundles"] = []
                    cards["candidate"]["runner_provenance"]["model_bundles"].pop()
            reference = "dob_judge.model_dir/artifact.bin"
            cards["base"]["runner_provenance"]["policy_dependencies"]["files"][reference] = "a" * 64
            result = agentic.gate(cards["base"], cards["candidate"], policy_delta=paths["delta"])
            self.assertEqual(result["verdict"], "not_comparable")
            self.assertIn(f"policy input {reference}", result["differing"])
            del cards["base"]["runner_provenance"]["policy_dependencies"]["files"][reference]
            cards["base"]["runner_provenance"]["policy_dependencies"]["files"][
                "policy.rulepacks.paths[0]"] = "a" * 64
            cards["candidate"]["runner_provenance"]["policy_dependencies"]["files"][
                "policy.rulepacks.paths[0]"] = "b" * 64
            result = agentic.gate(cards["base"], cards["candidate"], policy_delta=paths["delta"])
            self.assertEqual(result["verdict"], "not_comparable")
            self.assertIn("policy input policy.rulepacks.paths[0]", result["differing"])

    def compare(
        self, base_text: str, candidate_text: str, delta_text: str,
        missing_field: str | None = None,
        tamper_policy: str | None = None,
        remove_delta: bool = False,
        wrong_digest: tuple[str, str] | None = None,
        ner_thresholds: tuple[float | None, float | None] = (0.3, 0.3),
    ) -> dict:
        with tempfile.TemporaryDirectory() as directory:
            paths = {name: Path(directory) / f"{name}.toml" for name in ("base", "candidate", "delta")}
            for name, contents in (("base", base_text), ("candidate", candidate_text), ("delta", delta_text)):
                paths[name].write_text(contents, encoding="utf-8")
            base = _scorecard(self.BASE_LEAKS)
            candidate = _scorecard({**self.BASE_LEAKS, "R": 10})
            base["parameters"]["ner_threshold"], candidate["parameters"]["ner_threshold"] = ner_thresholds
            for label, card in (("base", base), ("candidate", candidate)):
                digest = hashlib.sha256(paths[label].read_bytes()).hexdigest()
                card["parameters"]["policy_sha256"] = digest
                card["runner_provenance"]["policy"] = {"path": str(paths[label]), "sha256": digest}
                if wrong_digest and wrong_digest[0] == label:
                    altered = ("0" if digest[0] != "0" else "1") + digest[1:]
                    self.assertRegex(altered, r"^[0-9a-f]{64}$")
                    self.assertEqual(hashlib.sha256(paths[label].read_bytes()).hexdigest(), digest)
                    if wrong_digest[1] == "provenance":
                        card["runner_provenance"]["policy"]["sha256"] = altered
                    else:
                        card["parameters"]["policy_sha256"] = altered
                if missing_field:
                    field = missing_field.removesuffix("_none")
                    target, key = {
                        "scorecard_sha256": (card["parameters"], "policy_sha256"),
                        "provenance_sha256": (card["runner_provenance"]["policy"], "sha256"),
                        "provenance_path": (card["runner_provenance"]["policy"], "path"),
                    }[field]
                    if missing_field.endswith("_none"):
                        target[key] = None
                    else:
                        del target[key]
            if tamper_policy:
                paths[tamper_policy].write_text(
                    paths[tamper_policy].read_text(encoding="utf-8") + "# changed after measurement\n",
                    encoding="utf-8",
                )
            if remove_delta:
                paths["delta"].unlink()
            return agentic.gate(base, candidate, policy_delta=paths["delta"])

    def test_declared_new_section_passes_with_parsed_toml_equality(self) -> None:
        result = self.compare(
            "[rules]\nenabled = true\n",
            "[extension]\nthreshold = 0.5\n\n[rules]\nenabled=true\n",
            "[extension]\nthreshold=0.5\n",
        )
        self.assertEqual(result["verdict"], "pass")
        self.assertEqual(set(result["policy_digests"]), {"base", "candidate", "delta"})
        self.assertIn("Policy SHA-256 digests", agentic.gate_markdown(result))
        self.assertIn("delta.toml", agentic.gate_markdown(result).splitlines()[0])

    def test_ner_threshold_changes_only_with_declared_ner_section(self) -> None:
        result = self.compare(
            "[rules]\nenabled=true\n",
            "[rules]\nenabled=true\n[ner]\nthreshold=0.42\n",
            "[ner]\nthreshold=0.42\n",
            ner_thresholds=(None, 0.42),
        )
        self.assertEqual(result["verdict"], "pass")
        result = self.compare(
            "[rules]\nenabled=true\n",
            "[rules]\nenabled=true\n[extension]\nenabled=true\n",
            "[extension]\nenabled=true\n",
            ner_thresholds=(None, 0.42),
        )
        self.assertEqual(result["verdict"], "not_comparable")
        self.assertIn("ner_threshold", result["differing"])

    def test_missing_ner_threshold_is_refused(self) -> None:
        card = _scorecard(self.BASE_LEAKS)
        del card["parameters"]["ner_threshold"]
        with self.assertRaisesRegex(agentic.LayerError, "ner_threshold"):
            agentic.gate(card, _scorecard(self.BASE_LEAKS))

    def test_policy_delta_normalizes_home_paths_only(self) -> None:
        home = Path.home()
        self.assertEqual(agentic.normalize_home_path(str(home / "models/davlan")), "~/models/davlan")
        self.assertEqual(agentic.normalize_home_path("/private/tmp/davlan"), "/private/tmp/davlan")
        result = self.compare(
            f'[rules]\nmodel_dir="{home}/models/base"\n',
            '[rules]\nmodel_dir="~/models/base"\n[ner]\nmodel_dir="~/models/davlan"\n',
            f'[ner]\nmodel_dir="{home}/models/davlan"\n',
        )
        self.assertEqual(result["verdict"], "pass")
        result = self.compare(
            '[rules]\nmodel_dir="/private/tmp/base"\n',
            '[rules]\nmodel_dir="/private/tmp/other"\n[ner]\nenabled=true\n',
            '[ner]\nenabled=true\n',
        )
        self.assertEqual(result["verdict"], "not_comparable")

    def test_policy_provenance_home_path_is_read_and_checked(self) -> None:
        with tempfile.TemporaryDirectory() as directory, mock.patch.dict(os.environ, {"HOME": directory}):
            policy = Path(directory) / "policy.toml"
            policy.write_text("[rules]\nenabled=true\n", encoding="utf-8")
            digest = hashlib.sha256(policy.read_bytes()).hexdigest()
            card = _scorecard(self.BASE_LEAKS)
            card["parameters"]["policy_sha256"] = digest
            card["runner_provenance"]["policy"] = {
                "path": "~/policy.toml", "sha256": digest,
            }
            self.assertEqual(agentic._scorecard_policy(card, "base"), ({"rules": {"enabled": True}}, digest))

    def test_undeclared_extra_key_is_not_comparable(self) -> None:
        result = self.compare(
            "[rules]\nenabled = true\n",
            "[rules]\nenabled = true\nextra = true\n[extension]\nthreshold = 0.5\n",
            "[extension]\nthreshold = 0.5\n",
        )
        self.assertEqual(result["verdict"], "not_comparable")
        self.assertIn("policy_sha256", result["differing"])

    def test_toml_type_change_is_not_comparable(self) -> None:
        result = self.compare(
            "[rules]\nenabled = true\n",
            "[rules]\nenabled = 1\n[extension]\nthreshold = 0.5\n",
            "[extension]\nthreshold = 0.5\n",
        )
        self.assertEqual(result["verdict"], "not_comparable")

    def test_delta_cannot_change_an_existing_base_section(self) -> None:
        result = self.compare(
            "[rules]\nenabled = true\n",
            "[rules]\nenabled = false\n",
            "[rules]\nenabled = false\n",
        )
        self.assertEqual(result["verdict"], "not_comparable")
        self.assertIn("existing base sections", result["policy_delta_reason"])

    def test_missing_policy_identity_on_both_sides_is_refused(self) -> None:
        for field in (
            "scorecard_sha256", "scorecard_sha256_none",
            "provenance_sha256", "provenance_sha256_none",
            "provenance_path", "provenance_path_none",
        ):
            with self.subTest(field=field), self.assertRaises(agentic.LayerError):
                self.compare(
                    "[rules]\nenabled = true\n",
                    "[rules]\nenabled = true\n[extension]\nthreshold = 0.5\n",
                    "[extension]\nthreshold = 0.5\n",
                    missing_field=field,
                )

    def test_policy_file_changed_after_measurement_is_refused(self) -> None:
        for label in ("base", "candidate"):
            with self.subTest(label=label), self.assertRaisesRegex(
                agentic.LayerError, f"{label} policy file differs from its scorecard SHA-256"
            ):
                self.compare(
                    "[rules]\nenabled = true\n",
                    "[rules]\nenabled = true\n[extension]\nthreshold = 0.5\n",
                    "[extension]\nthreshold = 0.5\n",
                    tamper_policy=label,
                )

    def test_wrong_provenance_digest_with_unchanged_policy_is_refused(self) -> None:
        for label in ("base", "candidate"):
            with self.subTest(label=label), self.assertRaisesRegex(
                agentic.LayerError, f"{label} policy file differs from its scorecard SHA-256"
            ):
                self.compare(
                    "[rules]\nenabled = true\n",
                    "[rules]\nenabled = true\n[extension]\nthreshold = 0.5\n",
                    "[extension]\nthreshold = 0.5\n",
                    wrong_digest=(label, "provenance"),
                )

    def test_wrong_parameters_digest_with_unchanged_policy_is_refused(self) -> None:
        for label in ("base", "candidate"):
            with self.subTest(label=label), self.assertRaisesRegex(
                agentic.LayerError, f"{label} policy file differs from its scorecard SHA-256"
            ):
                self.compare(
                    "[rules]\nenabled = true\n",
                    "[rules]\nenabled = true\n[extension]\nthreshold = 0.5\n",
                    "[extension]\nthreshold = 0.5\n",
                    wrong_digest=(label, "parameters"),
                )

    def test_empty_delta_is_not_comparable(self) -> None:
        result = self.compare(
            "[rules]\nenabled = true\n",
            "[rules]\nenabled = true\n[extension]\nthreshold = 0.5\n",
            "",
        )
        self.assertEqual(result["verdict"], "not_comparable")
        self.assertIn("at least one TOML section", result["policy_delta_reason"])

    def test_missing_delta_file_is_refused(self) -> None:
        with self.assertRaisesRegex(agentic.LayerError, "cannot read declared policy delta"):
            self.compare(
                "[rules]\nenabled = true\n",
                "[rules]\nenabled = true\n[extension]\nthreshold = 0.5\n",
                "[extension]\nthreshold = 0.5\n",
                remove_delta=True,
            )

    def test_invalid_delta_toml_is_refused(self) -> None:
        with self.assertRaisesRegex(agentic.LayerError, "invalid declared policy delta TOML"):
            self.compare(
                "[rules]\nenabled = true\n",
                "[rules]\nenabled = true\n[extension]\nthreshold = 0.5\n",
                "[extension\nthreshold = 0.5\n",
            )


class ReleaseGateCreditTests(unittest.TestCase):
    """The displayed releases re-scored from their committed records under the
    cued-class credit (user ruling 2026-09-28): newly credited invalid bytes move
    from the twin column into gated leaks; the headline never changes."""

    BENCH = REPO_ROOT / "docs/reference/benchmarks"
    # release: (C gated before, C gated after, A gated before, A gated after), contract v2.
    EXPECTED = {
        "v0.14.0": (19_832, 22_144, 19_409, 22_132),
        "v0.15.0": (11_043, 13_319, 12_835, 15_280),
        "v0.15.1": (11_043, 13_319, 12_662, 15_107),
    }

    def totals(self, release: str, scopes: dict[str, agentic.CreditScope]) -> dict:
        import gzip
        layers = json.loads(gzip.open(self.BENCH / f"agentic-layers-{release}.json.gz").read())["layers"]
        scorecard = json.loads((self.BENCH / f"scorecard-{release}-scored-labels-v2.json").read_text())
        with mock.patch.object(agentic, "CREDIT_SCOPE_BY_LABEL", scopes):
            # Each release is gated on the arm it shipped, recorded on its layer runs.
            config = layers["A"]["runs"][0]["config"]
            return agentic.layer_totals({**scorecard, "layers": layers}, config)

    def test_release_totals_before_and_after_the_credit(self) -> None:
        for release, (c_before, c_after, a_before, a_after) in self.EXPECTED.items():
            with self.subTest(release=release):
                before = self.totals(release, {
                    "IBAN": agentic.CreditScope.ALL,
                    "CREDITCARDNUMBER": agentic.CreditScope.ALL,
                })
                after = self.totals(release, agentic.CREDIT_SCOPE_BY_LABEL)
                self.assertEqual((before["C"]["leaked"], after["C"]["leaked"]), (c_before, c_after))
                self.assertEqual((before["A"]["leaked"], after["A"]["leaked"]), (a_before, a_after))
                for layer in agentic.GATE_LAYERS:
                    self.assertEqual(before[layer]["headline_leaked"], after[layer]["headline_leaked"])
                    self.assertEqual(before[layer]["false_positive"], after[layer]["false_positive"])
                    self.assertEqual(
                        before[layer]["leaked"] + before[layer]["twin_leaked"],
                        after[layer]["leaked"] + after[layer]["twin_leaked"],
                    )


class MutantGatePinTests(unittest.TestCase):
    """Historical v1-credit verdicts of full-harness runs against over-broad rules.

    `fixtures/agentic/gate-pin-mutants.json` holds `layer_totals` of three full
    runs (provenance inside). A mutant changes the policy, so `gate` rightly
    calls the pair not comparable; `decide` is the rule it faces. The gate is
    necessary, not sufficient. This pin records only the card counterweight,
    so it cannot judge the later v2-credit gate's additional guard families.
    """

    @classmethod
    def setUpClass(cls) -> None:
        path = Path(__file__).resolve().parent / "fixtures/agentic/gate-pin-mutants.json"
        cls.pin = json.loads(path.read_text(encoding="utf-8"))

    def verdict(self, mutant: str) -> dict:
        with mock.patch.object(agentic, "CREDIT_GUARD_FAMILIES", self.old_credit_guard()):
            result = agentic.decide(self.with_equal_restore(self.pin["totals"]["main"]),
                                    self.with_equal_restore(self.pin["totals"][mutant]))
        self.assertEqual({**result["summary"], "verdict": result["verdict"]}, self.pin["expected"][mutant])
        return result

    @staticmethod
    def old_credit_guard() -> dict[str, tuple[str, ...]]:
        return {"CREDITCARDNUMBER": ("ref_number_16",), "IBAN": ()}

    @staticmethod
    def with_equal_restore(totals: dict, guard_fp: int = 0) -> dict:
        # This historical pin predates restore totals and the credit guard; these
        # tests isolate the byte rule unless a measured guard count is passed.
        return {layer: {**row, "attempted": 0, "documents": 0, "restore_exact": 0, "manifest_valid": 0,
                        "guard_false_positive": {"ref_number_16": guard_fp} if layer == "D" else {}}
                for layer, row in totals.items()}

    @staticmethod
    def all_gold(totals: dict) -> dict:
        # Credits every checksum-invalid twin: a superset of the IBAN/card credit,
        # so a verdict that fails here fails under the real credit too.
        return {layer: {**row, "leaked": row["leaked"] + row["twin_leaked"], "twin_leaked": 0}
                for layer, row in totals.items()}

    def measured(self, mutant: str) -> dict:
        guard = self.pin["credit_guard"]["ref_number_16_false_positive"]
        return self.all_gold(self.with_equal_restore(self.pin["totals"][mutant], guard[mutant]))

    def test_spaced_sixteen_digit_mutant_failed_on_net_bytes_before_the_credit(self) -> None:
        result = self.verdict("mutant_spaced_sixteen_digits")
        self.assertEqual(result["verdict"], "fail")
        self.assertIn("net bytes", result["reason"])
        self.assertEqual(result["summary"], {"leaked_bytes_decrease": 15, "false_positive_bytes_increase": 551})

    def test_bare_nine_digit_mutant_passes_on_net_valid_bytes(self) -> None:
        result = self.verdict("mutant_bare_nine_digits")
        self.assertEqual(result["verdict"], "pass")
        self.assertEqual(result["summary"], {"leaked_bytes_decrease": 353, "false_positive_bytes_increase": 295})

    def test_credited_twins_alone_would_let_the_spaced_sixteen_digit_mutant_pass(self) -> None:
        # Why the credit guard exists: with checksum-failed gold credited, the
        # spaced 16-digit rule "saves" thousands of bytes and passes on net bytes.
        with mock.patch.object(agentic, "CREDIT_GUARD_FAMILIES", self.old_credit_guard()):
            result = agentic.decide(self.all_gold(self.with_equal_restore(self.pin["totals"]["main"])),
                                    self.all_gold(self.with_equal_restore(self.pin["totals"]["mutant_spaced_sixteen_digits"])))
        self.assertEqual(result["verdict"], "pass")

    def test_spaced_sixteen_digit_mutant_fails_the_v1_credit_guard(self) -> None:
        with mock.patch.object(agentic, "CREDIT_GUARD_FAMILIES", self.old_credit_guard()):
            result = agentic.decide(self.measured("main"), self.measured("mutant_spaced_sixteen_digits"))
        self.assertEqual(result["verdict"], "fail")
        self.assertIn("credit guard", result["reason"])
        self.assertIn("ref_number_16", result["reason"])

    def test_bare_nine_digit_mutant_passed_the_v1_credit_guard(self) -> None:
        with mock.patch.object(agentic, "CREDIT_GUARD_FAMILIES", self.old_credit_guard()):
            result = agentic.decide(self.measured("main"), self.measured("mutant_bare_nine_digits"))
        self.assertEqual(result["verdict"], "pass")

    def test_credit_guard_measurement_records_its_provenance(self) -> None:
        guard = self.pin["credit_guard"]
        for key in ("harness_commit", "corpus_sha256", "policy_sha256", "binary_sha256", "commands",
                    "generator_version", "measured"):
            self.assertTrue(guard["provenance"].get(key), key)
        self.assertEqual(guard["ref_number_16_false_positive"]["main"], 0)

    def test_pin_records_its_provenance(self) -> None:
        provenance = self.pin["provenance"]
        for key in ("harness_commit", "corpus_sha256", "policy_sha256", "binary_sha256", "commands"):
            self.assertTrue(provenance.get(key), key)
        # The mutant pin is a published v3 measurement, retained as historical evidence.
        self.assertEqual(provenance["generator_version"], 3)


if __name__ == "__main__":
    unittest.main()
