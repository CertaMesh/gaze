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
    "dev": "8a707e9f36d33ffda37c0fab104500353d52dabd7ba1cb9ddd6c2594e314a718",
    "test": "253af8e0a65b730b8da58625bb7066bedf0dce813a6d40d1e43732891c9c2608",
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

    def test_every_published_surface_and_family_is_populated(self) -> None:
        cells = {(r.family, r.surface) for r in self.corpora["test"]}
        families = [f.name for f in agentic.IDENTIFIER_FAMILIES] + [
            "email", "phone_de", "phone_us", "dob", "header_name",
        ]
        for family in families:
            for surface in agentic.SURFACES:
                self.assertIn((family, surface), cells)
        for family in agentic.LOOKALIKE_FAMILIES:
            for surface in agentic.LOOKALIKE_SURFACES:
                self.assertIn((family, surface), cells)


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


def _scorecard(leaks: dict[str, int], fps: dict[str, int] | None = None, refused: dict[str, int] | None = None) -> dict:
    fps = fps or {}
    refused = refused or {}

    def run(layer: str) -> dict:
        return {
            "config": "policy-file",
            "metrics": {"utf8_bytes": {"leaked": leaks[layer], "false_positive": fps.get(layer, 0)}},
            "pipeline_availability": {"failed_closed_documents": refused.get(layer, 0)},
        }

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


class GateTests(unittest.TestCase):
    BASE = {"C": 100, "A": 50, "D": 0, "R": 30}
    FP = {"C": 10, "A": 5, "D": 7, "R": 3}

    def verdict(self, candidate: dict) -> str:
        return agentic.gate(_scorecard(self.BASE, self.FP), candidate)["verdict"]

    def test_leak_falling_in_one_layer_and_rising_in_none_passes(self) -> None:
        self.assertEqual(self.verdict(_scorecard({**self.BASE, "R": 10}, self.FP)), "pass")

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


if __name__ == "__main__":
    unittest.main()
