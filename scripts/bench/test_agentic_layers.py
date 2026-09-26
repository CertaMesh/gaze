#!/usr/bin/env python3
"""Model-free tests for the agentic benchmark layers A and D."""

import copy
import json
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
            if record.validity == agentic.VALID:
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
        for record in agentic.generate("test"):
            if record.layer != agentic.LAYER_REPEATS:
                self.assertNotIn("decoys", record.to_json())


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
        self.assertFalse({g.value for r in dev for g in r.gold} & {g.value for r in test for g in r.gold})
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
        self.assertTrue(all(d.cell and "|dob|" in d.cell for d in dob))


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
) -> dict:
    """`leaks` is gated (valid) gold; `twin_leak` / `c_invalid_leak` add
    checksum-failed bytes to layers A and C."""
    fps = fps or {}
    refused = refused or {}

    def run(layer: str) -> dict:
        leaked = leaks[layer] + {"A": twin_leak, "C": c_invalid_leak}.get(layer, 0)
        block = {
            "config": "policy-file",
            "metrics": {"utf8_bytes": {"leaked": leaked, "false_positive": fps.get(layer, 0)}},
            "pipeline_availability": {"failed_closed_documents": refused.get(layer, 0)},
        }
        if layer == "C":
            block["validator_recall_by_label"] = {
                "TAXNUM": {"production_recall_by_gold_validity": {
                    "validator_passed_gold": {"leaked_utf8_bytes": 0},
                    "validator_failed_gold": {"leaked_utf8_bytes": c_invalid_leak},
                }},
                "CITY": {"production_recall_by_gold_validity": None},
            }
        if layer == "A":
            block["per_cell"] = {
                "A|bsn|csv|valid": {"utf8_bytes": {"leaked": leaks["A"]}},
                "A|bsn|csv|invalid": {"utf8_bytes": {"leaked": twin_leak}},
            }
        return block

    return {
        "parameters": {"configs": ["policy-file"], "policy_sha256": "p"},
        "dataset": {"integrity": {"sha256": "k"}},
        "scoring": {"scored_label_contract": {"id": "scored-labels-v2", "version": 2, "file_sha256": "c"}},
        "runs": [run("C")],
        "layers": {
            "generator": {"corpus_sha256": "g"},
            "scored_label_contract": {"file_sha256": "a"},
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


class MutantGatePinTests(unittest.TestCase):
    """True verdicts of real full-harness runs: main vs main plus each over-broad rule.

    `fixtures/agentic/gate-pin-mutants.json` holds `layer_totals` of three full
    runs (provenance inside). A mutant changes the policy, so `gate` rightly
    calls the pair not comparable; `decide` is the rule it faces. The gate is
    necessary, not sufficient: the bare 9-digit rule passes it on these
    corpora and would still be refused in review for its FP on reference
    numbers outside them.
    """

    @classmethod
    def setUpClass(cls) -> None:
        path = Path(__file__).resolve().parent / "fixtures/agentic/gate-pin-mutants.json"
        cls.pin = json.loads(path.read_text(encoding="utf-8"))

    def verdict(self, mutant: str) -> dict:
        result = agentic.decide(self.pin["totals"]["main"], self.pin["totals"][mutant])
        self.assertEqual({**result["summary"], "verdict": result["verdict"]}, self.pin["expected"][mutant])
        return result

    def test_spaced_sixteen_digit_mutant_fails(self) -> None:
        result = self.verdict("mutant_spaced_sixteen_digits")
        self.assertEqual(result["verdict"], "fail")
        self.assertIn("net bytes", result["reason"])
        self.assertEqual(result["summary"], {"leaked_bytes_decrease": 15, "false_positive_bytes_increase": 551})

    def test_bare_nine_digit_mutant_passes_on_net_valid_bytes(self) -> None:
        result = self.verdict("mutant_bare_nine_digits")
        self.assertEqual(result["verdict"], "pass")
        self.assertEqual(result["summary"], {"leaked_bytes_decrease": 353, "false_positive_bytes_increase": 295})

    def test_counted_over_all_gold_both_mutants_would_pass(self) -> None:
        # Why the twin exclusion exists: with checksum-failed gold counted,
        # the spaced 16-digit rule "saves" thousands of bytes no precise rule
        # could reach.
        def all_gold(totals: dict) -> dict:
            return {layer: {**row, "leaked": row["leaked"] + row["twin_leaked"], "twin_leaked": 0}
                    for layer, row in totals.items()}
        result = agentic.decide(all_gold(self.pin["totals"]["main"]),
                                all_gold(self.pin["totals"]["mutant_spaced_sixteen_digits"]))
        self.assertEqual(result["verdict"], "pass")

    def test_pin_records_its_provenance(self) -> None:
        provenance = self.pin["provenance"]
        for key in ("harness_commit", "corpus_sha256", "policy_sha256", "binary_sha256", "commands"):
            self.assertTrue(provenance.get(key), key)
        self.assertEqual(provenance["generator_version"], agentic.GENERATOR_VERSION)


if __name__ == "__main__":
    unittest.main()
