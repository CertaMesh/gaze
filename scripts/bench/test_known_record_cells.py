#!/usr/bin/env python3
"""Model-free tests for the known-record match-kind cells."""

import dataclasses
import json
import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import agentic_layers as agentic
import known_record_cells as cells


REPO_ROOT = Path(__file__).resolve().parents[2]

# A generator change must bump GENERATOR_VERSION and this hash together.
PINNED_CORPUS_SHA256 = "16984c5264cb5e667a5931963a45fe212fa9f6a1c23fafb751d895103d5a6b6e"
PAIRS = 84


def recorder_kind(text: str, raw: str) -> str:
    """#718's round-3 recorder rule (known_record_attribution.match_group_and_kind):
    unfolded text against the raw record value."""
    canonical = " ".join(raw.split())
    collapsed = " ".join(text.split())
    if text == raw:
        return "exact"
    if collapsed == canonical:
        return "whitespace_flexible"
    if text.casefold() == canonical.casefold():
        return "case_folded"
    return "whitespace_case_folded"


def target_text(cell: cells.Cell, target: cells.Target) -> str:
    return cell.text.encode("utf-8")[target.start : target.end].decode("utf-8")


def pair_of(pairs, variant: str, index: int = 0) -> cells.Pair:
    return [p for p in pairs if p.positive.variant == variant][index]


class KnownRecordCellTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.pairs = cells.generate()
        cls.all_cells = cells.cells(cls.pairs)

    def test_every_bucket_variant_and_surface_has_a_pair(self) -> None:
        present = {(p.positive.bucket, p.positive.variant, p.positive.surface) for p in self.pairs}
        expected = {
            (bucket, variant, surface)
            for bucket, variants in cells.VARIANTS.items()
            for variant in variants
            for surface in cells.surfaces_for(variant)
        }
        self.assertEqual(present, expected)
        self.assertEqual(len(self.pairs), PAIRS)
        self.assertEqual({b for b, _, _ in present}, set(cells.MatchKind) | set(cells.ControlKind))
        for pair in self.pairs:
            self.assertIs(pair.positive.role, cells.Role.POSITIVE)
            self.assertIs(pair.counterweight.role, cells.Role.COUNTERWEIGHT)

    def test_generation_is_deterministic_and_pinned(self) -> None:
        again = cells.generate()
        self.assertEqual(cells.corpus_bytes(again), cells.corpus_bytes(self.pairs))
        manifest = cells.manifest(self.pairs)
        self.assertEqual(manifest["corpus_sha256"], PINNED_CORPUS_SHA256)
        self.assertEqual(manifest["generator_version"], cells.GENERATOR_VERSION)

    def test_gold_is_byte_exact_and_bounds_every_positive_target(self) -> None:
        for cell in self.all_cells:
            encoded = cell.text.encode("utf-8")
            for gold in cell.gold:
                self.assertEqual(encoded[gold.start : gold.end].decode("utf-8"), gold.value, cell.uid)
            if cell.role is cells.Role.POSITIVE:
                for target in cell.targets:
                    self.assertIn(target.start, {g.start for g in cell.gold}, cell.uid)
                    self.assertIn(target.end, {g.end for g in cell.gold}, cell.uid)

    def test_counterweights_carry_no_gold_and_count_as_decoys(self) -> None:
        for pair in self.pairs:
            document = pair.counterweight.to_document()
            self.assertEqual(document.spans, ())
            self.assertEqual(document.source_dataset, cells.SOURCE_COUNTERWEIGHT)
            self.assertIsNotNone(document.negative_category)
            self.assertTrue(pair.positive.to_document().spans)

    def test_gold_labels_are_scored_under_both_contracts(self) -> None:
        for contract in ("v1", "v2"):
            documents, contexts = cells.documents(REPO_ROOT, contract, self.pairs)
            self.assertEqual(len(documents), 2 * PAIRS)
            self.assertEqual(set(contexts), {d.uid for d in documents})
            for document in documents:
                self.assertEqual(document.excluded_spans, (), document.uid)

    def test_pairs_share_a_derived_descriptor(self) -> None:
        for pair in self.pairs:
            self.assertEqual(pair.positive.descriptor(), pair.counterweight.descriptor(), pair.positive.uid)
            self.assertEqual(pair.positive.record, pair.counterweight.record)

    def test_mapping_swap_fails_generation(self) -> None:
        # Same variant, other surface; same surface, other variant; probe and control.
        swaps = [
            (pair_of(self.pairs, "iban_double_space", 0), pair_of(self.pairs, "iban_double_space", 1)),
            (pair_of(self.pairs, "name_lower_double_space"), pair_of(self.pairs, "name_upper_double_space")),
            (pair_of(self.pairs, "listed_adjacent_peer"), pair_of(self.pairs, "listed_surname_comma")),
            (pair_of(self.pairs, "steuer_id_double_space"), pair_of(self.pairs, "steuer_id_nbsp")),
        ]
        for left, right in swaps:
            swapped = [
                cells.Pair(left.positive, right.counterweight),
                cells.Pair(right.positive, left.counterweight),
            ]
            with self.assertRaisesRegex(cells.CellError, "differ in shape or position"):
                cells.check(swapped)

    def test_every_probe_measures_its_declared_kind(self) -> None:
        measured: dict[object, set[str]] = {}
        for pair in self.pairs:
            positive = pair.positive
            kind = positive.measured_kind()
            if positive.probe_kind is not None:
                self.assertEqual(kind, positive.probe_kind.value, positive.uid)
            else:
                self.assertEqual(kind, cells.CONTROL_MEASURED_KIND[positive.control_kind], positive.uid)
            measured.setdefault(positive.bucket, set()).add(kind)
        for kind in cells.MatchKind:
            self.assertEqual(measured[kind], {kind.value})

    def test_a_probe_that_gaze_folds_to_exact_fails_generation(self) -> None:
        pair = pair_of(self.pairs, "steuer_id_double_space")
        folded = dataclasses.replace(pair.positive, text=pair.positive.text.replace("  ", cells.NBSP))
        self.assertEqual(folded.measured_kind(), "exact")
        with self.assertRaisesRegex(cells.CellError, "measures exact, bucket whitespace_flexible"):
            cells._check_bucket(folded)

    def test_gaze_fold_matches_normalize_rs(self) -> None:
        self.assertEqual(cells.gaze_fold("12 345 678 901"), "12 345 678 901")
        self.assertEqual(cells.gaze_fold("ＡＢ１２"), "AB12")
        self.assertEqual(cells.gaze_fold("a\n\tb"), "a\n\tb")
        self.assertEqual(cells.matcher_kind("12 345", "custom:steuer_id", "12 345"), "exact")
        self.assertEqual(cells.matcher_kind("12  345", "custom:steuer_id", "12 345"), "whitespace_flexible")
        self.assertEqual(cells.matcher_kind("ANNA WEBER", "Name", "Anna Weber"), "case_folded")
        self.assertEqual(cells.matcher_kind("ANNA  WEBER", "Name", "Anna Weber"), "whitespace_case_folded")

    def test_canonical_record_control_shows_the_recorder_divergence(self) -> None:
        pair = pair_of(self.pairs, "name_record_irregular")
        (target,) = pair.positive.primary_targets()
        raw = pair.positive.record[target.slot].raw
        self.assertEqual(pair.positive.measured_kind(), "exact")
        self.assertEqual(recorder_kind(target_text(pair.positive, target), raw), "whitespace_flexible")

    def test_record_model_matches_each_positive_target_through_one_slot(self) -> None:
        for pair in self.pairs:
            positive = pair.positive
            matches = cells.model_matches(positive.text, positive.record)
            for target in positive.targets:
                hits = [m for m in matches if m[1] < target.end and target.start < m[2]]
                if target.expect_match:
                    self.assertEqual(hits, [(target.slot, target.start, target.end)], positive.uid)
                else:
                    self.assertEqual(hits, [], positive.uid)

    def test_counterweights_match_only_the_priced_unlisted_lure(self) -> None:
        priced = 0
        for pair in self.pairs:
            twin = pair.counterweight
            matches = cells.model_matches(twin.text, twin.record)
            if twin.variant == "unlisted_alone":
                self.assertEqual(len(matches), 1, twin.uid)
                priced += 1
            else:
                self.assertEqual(matches, [], twin.uid)
        self.assertEqual(priced, 4)

    def test_corroborated_lure_fails_generation(self) -> None:
        twin = pair_of(self.pairs, "listed_adjacent_peer").counterweight
        surname = twin.record[1].raw
        end = len(twin.text.encode("utf-8")[: twin.targets[0].end].decode("utf-8"))
        corroborated = dataclasses.replace(twin, text=twin.text[:end] + " " + surname + twin.text[end:])
        with self.assertRaisesRegex(cells.CellError, "match is True, expected False"):
            cells._check_cell(corroborated)

    def test_listed_name_needs_corroboration_and_its_capital(self) -> None:
        record = (cells.RecordField("Name", "Mark"), cells.RecordField("Name", "Okafor"))
        self.assertEqual(cells.model_matches("Mark the date.", record), [])
        self.assertEqual(cells.model_matches("mark Okafor", record), [(1, 5, 11)])
        self.assertEqual(cells.model_matches("Mark Okafor", record), [(0, 0, 4), (1, 5, 11)])
        full = (cells.RecordField("Name", "Mark"), cells.RecordField("Name", "Mark Okafor"))
        self.assertEqual(cells.model_matches("Mark Okafor left. Hi Mark, ok", full), [(0, 21, 25), (1, 0, 11)])
        self.assertEqual(cells.model_matches("Mark Okafor left. So Mark, ok", full), [(1, 0, 11)])

    def test_record_value_floor_refuses_a_short_letter_value(self) -> None:
        self.assertFalse(cells.accepted_record_value("Al"))
        self.assertFalse(cells.accepted_record_value("123"))
        self.assertTrue(cells.accepted_record_value("NL91 ABNA 0417 1643 00"))
        self.assertTrue(cells.accepted_record_value("Ivy"))
        pair = pair_of(self.pairs, "iban_double_space")
        refused = dataclasses.replace(pair.positive, record=(cells.RecordField("custom:iban", "AB"),))
        with self.assertRaisesRegex(cells.CellError, "refuses a record value"):
            cells._check_cell(refused)

    def test_records_are_unique_synthetic_and_valid(self) -> None:
        validators = {"custom:steuer_id": agentic.steuer_id_valid, "custom:credit_card": agentic.luhn_valid}
        for cell in self.all_cells:
            values = [(f.class_name, " ".join(f.raw.split()).casefold()) for f in cell.record]
            self.assertEqual(len(values), len(set(values)), cell.uid)
            context = json.loads(cell.context_json())
            self.assertEqual(len(context["record"]), len(cell.record))
            for field in cell.record:
                compact = field.raw.replace(" ", "")
                if field.class_name == "custom:phone":
                    self.assertTrue(field.raw.startswith("+49 1555 01"), field.raw)
                if field.class_name == "custom:iban":
                    self.assertTrue(agentic.iban_valid(compact), field.raw)
                if field.class_name in validators:
                    self.assertTrue(validators[field.class_name](compact), field.raw)
                if field.class_name == "custom:national_id":
                    self.assertTrue(agentic.bsn_valid(compact) or agentic.nhs_valid(compact), field.raw)

    def test_nhs_numbers_stay_in_the_reserved_test_range(self) -> None:
        nhs = [cell for cell in self.all_cells if cell.variant.startswith("nhs")]
        self.assertEqual(len(nhs), 8)
        for cell in nhs:
            (field,) = cell.record
            self.assertTrue(field.raw.startswith(cells.NHS_TEST_PREFIX), field.raw)
            self.assertTrue(agentic.nhs_valid(field.raw.replace(" ", "")), field.raw)
            for target in cell.targets:
                digits = "".join(ch for ch in target_text(cell, target) if ch.isdigit())
                self.assertEqual(len(digits), 10, cell.uid)
                self.assertTrue(digits.startswith(cells.NHS_TEST_PREFIX), cell.uid)

    def test_single_line_surfaces_carry_no_line_break(self) -> None:
        for cell in self.all_cells:
            if cell.surface == "tool_json":
                json.loads(cell.text)
            if cell.surface in cells.SINGLE_LINE_SURFACES:
                self.assertNotIn("\n", cell.text)
            self.assertEqual(cells.surface_of(cell.text), cell.surface)


class ProofArmTests(unittest.TestCase):
    def test_arm_contexts_state_defaults_and_add_only_probe_kinds(self) -> None:
        for cell in cells.cells(cells.generate()):
            off = json.loads(cells.arm_context(cell, False))["record_match_kinds"]
            on = json.loads(cells.arm_context(cell, True))["record_match_kinds"]
            self.assertEqual(set(off), set(on))
            for group, kinds in on.items():
                self.assertEqual(off[group], list(cells.DEFAULT_MATCH_KINDS[group]))
                added = set(kinds) - set(off[group])
                if group == "name_single":
                    self.assertEqual(added, {"corroborated_single"})
                elif group == "name_multi":
                    self.assertEqual(added, {"whitespace_flexible", "whitespace_case_folded"})
                else:
                    self.assertEqual(added, {"whitespace_flexible"})

    def test_kind_switch_detects_an_ignored_override(self) -> None:
        a = {"x": {"final_protection_trace": [{"raw_start": 0, "raw_end": 4}]}}
        b = {"x": {"final_protection_trace": []}}
        self.assertFalse(cells.kind_switch_effective(a, a))
        self.assertTrue(cells.kind_switch_effective(a, b))


class VariantTallyTests(unittest.TestCase):
    def test_tally_splits_recovered_gold_and_added_false_positives(self) -> None:
        pairs = cells.generate()
        pair = pair_of(pairs, "unlisted_alone")
        tally = cells.VariantTally.create(pairs)
        positive, twin = pair.positive.to_document(), pair.counterweight.to_document()
        empty = {"final_protection_trace": []}

        def protect(cell: cells.Cell) -> dict[str, object]:
            target = cell.targets[0]
            return {"final_protection_trace": [{"raw_start": target.start, "raw_end": target.end, "class": "Name"}]}

        for document in (positive, twin):
            tally.record_baseline(document, empty)
        tally.record_candidate(positive, protect(pair.positive))
        tally.record_candidate(twin, protect(pair.counterweight))
        rows = {row["role"]: row for row in tally.result()["rows"]}
        width = pair.positive.targets[0].end - pair.positive.targets[0].start
        self.assertEqual(rows["positive"]["type"], "control")
        self.assertEqual(rows["positive"]["bucket"], cells.ControlKind.EXACT_UNLISTED_NAME.value)
        self.assertEqual(rows["positive"]["gold_recovered_bytes"], width)
        self.assertEqual(rows["positive"]["false_positive_added_bytes"], 0)
        self.assertEqual(rows["counterweight"]["gold_recovered_bytes"], 0)
        self.assertEqual(rows["counterweight"]["false_positive_added_bytes"], width)
        self.assertEqual(rows["counterweight"]["target_protected_bytes"], width)

    def test_unpaired_baseline_fails_closed(self) -> None:
        pairs = cells.generate()
        tally = cells.VariantTally.create(pairs)
        tally.record_baseline(pairs[0].positive.to_document(), {"final_protection_trace": []})
        with self.assertRaises(cells.CellError):
            tally.result()


if __name__ == "__main__":
    unittest.main()
