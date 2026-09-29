#!/usr/bin/env python3
"""Model-free tests for the known-record match-kind cells."""

import dataclasses
import json
import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import agentic_layers as agentic
import gaze_bench_score as score
import known_record_cells as cells


REPO_ROOT = Path(__file__).resolve().parents[2]

# A generator change must bump GENERATOR_VERSION and this hash together.
PINNED_CORPUS_SHA256 = "224f7369e154c05bb5543181fe4dd501b5d2afadd194772bdc41ea0e4c96920b"


def attribution_kind(text: str, record_class: str, raw: str) -> tuple[str, str]:
    """The oracle recorder's bucket rule (known_record_attribution.match_group_and_kind)."""
    canonical = " ".join(raw.split())
    collapsed = " ".join(text.split())
    if collapsed.casefold() != canonical.casefold():
        return record_class, "unmatched_term"
    single = len(canonical.split()) == 1
    listed = {name.casefold() for name in cells.LISTED_COMMON_NAMES}
    if record_class == "Name" and single and canonical.casefold() in listed:
        kind = "corroborated_single"
    elif text == raw:
        kind = "exact"
    elif collapsed == canonical:
        kind = "whitespace_flexible"
    elif text.casefold() == canonical.casefold():
        kind = "case_folded"
    else:
        kind = "whitespace_case_folded"
    if record_class == "Name":
        group = "name_single" if single else "name_multi"
    else:
        group = "address_part" if record_class == "Location" else record_class
    return group, kind


def target_text(cell: cells.Cell, target: cells.Target) -> str:
    return cell.text.encode("utf-8")[target.start : target.end].decode("utf-8")


class KnownRecordCellTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.pairs = cells.generate()
        cls.all_cells = cells.cells(cls.pairs)

    def test_every_kind_variant_and_surface_has_a_pair(self) -> None:
        present = {(p.positive.kind, p.positive.variant, p.positive.surface) for p in self.pairs}
        expected = {
            (kind, variant, surface)
            for kind, variants in cells.VARIANTS.items()
            for variant in variants
            for surface in cells.surfaces_for(variant)
        }
        self.assertEqual(present, expected)
        self.assertEqual(len(self.pairs), 60)
        self.assertEqual({kind for kind, _, _ in present}, set(cells.MatchKind))
        for pair in self.pairs:
            self.assertIs(pair.positive.role, cells.Role.POSITIVE)
            self.assertIs(pair.counterweight.role, cells.Role.COUNTERWEIGHT)

    def test_generation_is_deterministic_and_pinned(self) -> None:
        again = cells.generate()
        self.assertEqual(cells.corpus_bytes(again), cells.corpus_bytes(self.pairs))
        manifest = cells.manifest(self.pairs)
        self.assertEqual(manifest["corpus_sha256"], PINNED_CORPUS_SHA256)
        self.assertEqual(manifest["generator_version"], cells.GENERATOR_VERSION)

    def test_gold_is_byte_exact_and_covers_every_positive_target(self) -> None:
        for cell in self.all_cells:
            encoded = cell.text.encode("utf-8")
            for gold in cell.gold:
                self.assertEqual(encoded[gold.start : gold.end].decode("utf-8"), gold.value, cell.uid)
            if cell.role is cells.Role.POSITIVE:
                gold = score.merge_intervals((g.start, g.end) for g in cell.gold)
                for target in cell.targets:
                    covered = score.intersection_length([(target.start, target.end)], gold)
                    whitespace = sum(ch.isspace() for ch in target_text(cell, target).encode("utf-8").decode("utf-8"))
                    self.assertGreaterEqual(covered, target.end - target.start - 3 * whitespace, cell.uid)
                    self.assertTrue(target.start in {g.start for g in cell.gold}, cell.uid)
                    self.assertTrue(target.end in {g.end for g in cell.gold}, cell.uid)

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
            self.assertEqual(len(documents), 120)
            self.assertEqual(set(contexts), {d.uid for d in documents})
            for document in documents:
                self.assertEqual(document.excluded_spans, (), document.uid)

    def test_pairs_share_a_derived_descriptor(self) -> None:
        for pair in self.pairs:
            self.assertEqual(pair.positive.descriptor(), pair.counterweight.descriptor(), pair.positive.uid)
            self.assertEqual(pair.positive.record, pair.counterweight.record)

    def test_mapping_swap_fails_generation(self) -> None:
        by_variant: dict[str, list[cells.Pair]] = {}
        for pair in self.pairs:
            by_variant.setdefault(pair.positive.variant, []).append(pair)
        # Same variant, other surface; and same surface, other variant.
        swaps = [
            tuple(by_variant["iban_nbsp"][:2]),
            (by_variant["name_lower_nbsp"][0], by_variant["name_upper_double_space"][0]),
            (by_variant["listed_adjacent_peer"][0], by_variant["listed_surname_comma"][0]),
        ]
        for left, right in swaps:
            swapped = [
                cells.Pair(left.positive, right.counterweight),
                cells.Pair(right.positive, left.counterweight),
            ]
            with self.assertRaisesRegex(cells.CellError, "differ in shape or position"):
                cells.check(swapped)

    def test_primary_targets_land_in_the_declared_attribution_bucket(self) -> None:
        seen = set()
        for cell in self.all_cells:
            # A lure that must not match lands in no bucket; the pair's bucket
            # is what its FP would cost if the matcher ever took it.
            for target in (t for t in cell.targets if t.expect_match):
                field = cell.record[target.slot]
                bucket = attribution_kind(target_text(cell, target), field.class_name, field.raw)
                self.assertEqual(bucket, target.attribution, cell.uid)
                if cell.role is cells.Role.POSITIVE and target.slot == 0:
                    seen.add(bucket[1])
        self.assertEqual(seen, {kind.value for kind in cells.MatchKind} | {"exact"})

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
        pair = next(p for p in self.pairs if p.positive.variant == "listed_adjacent_peer")
        twin = pair.counterweight
        surname = twin.record[1].raw
        target = twin.targets[0]
        lure = twin.text.encode("utf-8")[target.start : target.end].decode("utf-8")
        start = len(twin.text.encode("utf-8")[: target.start].decode("utf-8"))
        text = twin.text[: start + len(lure)] + " " + surname + twin.text[start + len(lure) :]
        corroborated = dataclasses.replace(twin, text=text)
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

    def test_records_are_unique_synthetic_and_valid(self) -> None:
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
                if field.class_name == "custom:credit_card":
                    self.assertTrue(agentic.luhn_valid(compact), field.raw)
                    self.assertEqual(context["field_map"]["/v00/value"], "custom:credit_card")

    def test_record_value_floor_refuses_a_two_letter_iban(self) -> None:
        self.assertFalse(cells.accepted_record_value("DE08 2377 9544 7859 6896 38"))
        self.assertTrue(cells.accepted_record_value("NL91 ABNA 0417 1643 00"))
        self.assertTrue(cells.accepted_record_value("Ivy"))
        self.assertFalse(cells.accepted_record_value("Al"))
        pair = next(p for p in self.pairs if p.positive.variant == "iban_nbsp")
        refused = dataclasses.replace(pair.positive, record=(cells.RecordField("custom:iban", "DE08 2377 9544 7859 6896 38"),))
        with self.assertRaisesRegex(cells.CellError, "refuses a record value"):
            cells._check_cell(refused)

    def test_single_line_surfaces_carry_no_line_break(self) -> None:
        for cell in self.all_cells:
            if cell.surface == "tool_json":
                json.loads(cell.text)
            if cell.surface in cells.SINGLE_LINE_SURFACES:
                self.assertNotIn("\n", cell.text)
            self.assertEqual(cells.surface_of(cell.text), cell.surface)


class VariantTallyTests(unittest.TestCase):
    def test_tally_splits_recovered_gold_and_added_false_positives(self) -> None:
        pairs = cells.generate()
        pair = next(p for p in pairs if p.positive.variant == "unlisted_alone")
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
