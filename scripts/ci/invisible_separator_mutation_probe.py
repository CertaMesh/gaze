#!/usr/bin/env python3
"""Disable committed separator protections and require behavioural test failures."""
import argparse
import os
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parents[2]
SOURCE = ROOT / "crates/gaze-types/src/normalize.rs"
REGEX_SOURCE = ROOT / "crates/gaze-recognizers/src/regex.rs"
SOURCE_REGEX = ROOT / "crates/gaze-recognizers/src/source_regex.rs"
ANCHOR_SOURCE = ROOT / "crates/gaze-recognizers/src/anchored_match.rs"
PIPELINE_SOURCE = ROOT / "crates/gaze/src/pipeline.rs"
DICTIONARY_SOURCE = ROOT / "crates/gaze-recognizers/src/dictionary.rs"
VETO_SOURCE = ROOT / "crates/gaze/src/validator_veto.rs"
PAYMENT_SOURCE = ROOT / "crates/gaze-types/src/payment_card.rs"
CORE_SOURCE = ROOT / "crates/gaze-recognizers/embedded/core.toml"
TYPES_SOURCE = ROOT / "crates/gaze-types/src/lib.rs"
# One representative per Unicode range, plus every original removed character.
CODEPOINTS = (0xAD, 0x34F, 0x61C, 0x115F, 0x17B4, 0x180E, 0x200B, 0x200C,
              0x200D, 0x202A, 0x2060, 0x3164, 0xFE0F, 0xFEFF, 0xFFA0,
              0xFFF0, 0x1BCA0, 0x1D173, 0xE0067)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--probe", action="append", default=[], help="Run only this named mutation (repeatable)")
    args = parser.parse_args()
    seen = set()
    completed = 0
    total = len(set(args.probe)) or len(CODEPOINTS) + 28
    paths = (SOURCE, REGEX_SOURCE, SOURCE_REGEX, ANCHOR_SOURCE, PIPELINE_SOURCE, DICTIONARY_SOURCE, VETO_SOURCE, PAYMENT_SOURCE, CORE_SOURCE, TYPES_SOURCE)
    originals = {path: path.read_bytes() for path in paths}
    for path, original in originals.items():
        relative = path.relative_to(ROOT).as_posix()
        if original != subprocess.check_output(["git", "show", f"HEAD:{relative}"], cwd=ROOT):
            raise SystemExit(f"Commit {relative} before probing; source must match HEAD")
    environment = dict(os.environ, RUSTUP_TOOLCHAIN="1.96.0", CARGO_BUILD_JOBS="2",
                       GAZE_DISABLE_OS_KEYSTORE="1")
    output = ROOT / "target/separator-checks/mutations"
    output.mkdir(parents=True, exist_ok=True)

    def reject(name, path, before, after, test, binary="default_ignorable_boundaries", codepoint=None, features=None):
        nonlocal completed
        seen.add(name)
        if args.probe and name not in args.probe:
            return
        print(f"MUTATION {completed + 1}/{total}: {name}", flush=True)
        source = originals[path].decode()
        if source.count(before) != 1:
            raise SystemExit(f"{name}: expected exactly one mutation site")
        env = dict(environment)
        if codepoint is not None:
            env["GAZE_SEPARATOR_PROBE"] = f"{codepoint:x}"
        try:
            path.write_text(source.replace(before, after, 1))
            target = ["--lib"] if binary is None else ["--test", binary]
            extra = ["--features", features] if features else []
            result = subprocess.run(
                ["nice", "-n", "10", "cargo", "test", "-p", "gaze-recognizers",
                 *extra, *target, test, "--", "--exact"], cwd=ROOT, env=env,
                stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
            (output / f"{name}.log").write_bytes(result.stdout)
            if result.returncode != 101 or b"test result: FAILED" not in result.stdout:
                raise SystemExit(f"{name}: mutation did not fail its regression; inspect {output}")
            completed += 1
            print(f"MUTATION {completed}/{total}: {name} rejected", flush=True)
        finally:
            path.write_bytes(originals[path])

    try:
        for cp in CODEPOINTS:
            before = "fn is_default_ignorable(ch: char) -> bool {"
            reject(f"mapping_{cp:x}", SOURCE, before,
                   before + f"\n    if ch == '\\u{{{cp:x}}}' {{ return false; }}",
                   "every_default_ignorable_inside_values_keeps_all_source_bytes", codepoint=cp)
        reject("nb_hyphen", SOURCE, "        '\\u{2011}' => '-',\n", "",
               "nonbreaking_hyphen_identifiers_keep_exact_source_spans", "invisible_separators")
        reject("postcode_run_guard", REGEX_SOURCE,
               "scan.rejected |= gaze_types::word_run_extends_identifier(input, scan.span.end);",
               "scan.rejected |= false && gaze_types::word_run_extends_identifier(input, scan.span.end);",
               "invisible_code_fragments_do_not_create_a_postcode_prefix", "invisible_separators")
        reject("zero_country_phone_guard", REGEX_SOURCE,
               "scan.rejected |= inside_zero_country_phone(input, &scan.span);",
               "scan.rejected |= false && inside_zero_country_phone(input, &scan.span);",
               "invalid_international_phone_prefix_does_not_license_a_national_suffix", "invisible_separators")
        reject("iban_hyphen_pattern", CORE_SOURCE,
               r"(?:DK|FI|FK|FO|GL|NL|SD)\d{2}(?:[\x20-]?[A-Z0-9]{4}){3}[\x20-]?[A-Z0-9]{2}",
               r"(?:DK|FI|FK|FO|GL|NL|SD)\d{2}(?:\x20?[A-Z0-9]{4}){3}\x20?[A-Z0-9]{2}",
               "hyphen_grouped_ibans_keep_the_complete_value_protected", "invisible_separators")
        reject("iban_hyphen_validator", TYPES_SOURCE,
               ".filter(|ch| !ch.is_ascii_whitespace() && *ch != '-')",
               ".filter(|ch| !ch.is_ascii_whitespace())",
               "hyphen_grouped_ibans_keep_the_complete_value_protected", "invisible_separators")
        reject("source_boundaries", REGEX_SOURCE,
               "let additional = self.scanned_spans_in_view(input, source_spans, Some(regex));",
               "let additional = self.scanned_spans_in_view(input, source_spans, None);",
               "mixed_outer_and_inner_separators_keep_complete_values_protected", codepoint=0x200B)
        reject("hard_boundary", REGEX_SOURCE,
               "                    .scanned_spans_in_view(&view.text, None, None)\n",
               "                    .scanned_spans_in_view(&view.text, None, None)\n                    .into_iter()\n                    .filter(|_| false)\n                    .collect::<Vec<_>>()\n",
               "gap_neighbours_never_hide_validated_values", "invisible_separators")
        reject("hard_boundary_containment", REGEX_SOURCE,
               "scans.extend(boundaries);",
               "merge_recovered_scans(&mut scans, &[], boundaries, false, || {});",
               "hard_boundary_value_survives_enclosing_joined_validator_failure", "validators")
        reject("hard_boundary_empty_candidate", REGEX_SOURCE,
               ".filter(|scan| !view.joined(scan.span.clone()).is_empty())",
               ".filter(|_| true)",
               "hard_boundary_marker_only_captures_do_not_hide_real_values", "legacy_source_boundaries")
        reject("luhn_enclosing_union", VETO_SOURCE,
               "cards.contains(&candidate.span)", "!cards.is_empty()",
               "hard_boundary_value_survives_enclosing_joined_validator_failure", "validators")
        reject("valid_joined_value_kept", REGEX_SOURCE,
               "                    joined_valid\n", "                    false && joined_valid\n",
               "valid_joined_values_never_ship_split_remainders_raw", "invisible_separators")
        reject("valid_joined_value_one_token", REGEX_SOURCE,
               "                    joined_valid\n", "                    false && joined_valid\n",
               "hard_boundary_and_joined_valid_candidates_emit_one_token", "validators")
        reject("luhn_union_preference", REGEX_SOURCE,
               "|| preserves_card_union\n", "|| false && preserves_card_union\n",
               "hard_boundary_preserves_overlapping_valid_card_windows", "validators")
        reject("recorded_card_layout", REGEX_SOURCE,
               "|| preserves_recorded_card_layout", "|| false && preserves_recorded_card_layout",
               "hard_boundary_preserves_recorded_card_and_verification_code", "validators")
        reject("recorded_code_total_length", REGEX_SOURCE,
               "matches!(suffix.len(), 3 | 4)",
               "matches!(suffix.len(), 3 | 4) && input[scan.span.clone()].bytes().filter(u8::is_ascii_digit).count() == 19",
               "hard_boundary_recorded_codes_preserve_all_lengths_and_separators", "validators")
        reject("card_window_source_gaps", PAYMENT_SOURCE,
               "if source_gap {", "if false && source_gap {",
               "hard_boundary_preserves_overlapping_valid_card_windows", "validators")
        reject("hard_boundary_preference", REGEX_SOURCE,
               "let valid = !scan.rejected", "let valid = false && !scan.rejected",
               "hard_boundary_preserves_recorded_card_and_verification_code", "validators")
        reject("adopter_ignorable_literal", SOURCE_REGEX,
               "normalize_literals(&mut ast, true)?;", "",
               "adopter_ignorable_literals_and_class_members_keep_matching", "legacy_source_boundaries")
        reject("adopter_edge_literal", REGEX_SOURCE,
               "self.normalizes_ignorable_literals || ", "",
               "adopter_ignorable_edge_literals_match_without_interior_gaps", "legacy_source_boundaries")
        reject("adopter_prefilter", SOURCE_REGEX,
               "    normalize_literals(&mut ast, false)?;\n    Ok(ast.to_string())", "    Ok(ast.to_string())",
               "adopter_ignorable_literals_and_class_members_keep_matching", "legacy_source_boundaries")
        reject("inner_gap_skip", SOURCE_REGEX,
               r'r"(?:{}(?u:\u{{2060}})*)"', r'r"{}"',
               "mixed_outer_and_inner_separators_keep_complete_values_protected")
        reject("anchored_name_components", ANCHOR_SOURCE,
               "let view = boundary_view.as_ref()?;",
               "let view = boundary_view.as_ref().filter(|_| false)?;",
               "anchored_match::tests::source_gaps_separate_name_components_without_losing_an_inner_suffix", None)
        reject("unfiltered_gap_compilation", REGEX_SOURCE,
               "if !prefilter.could_match(input) {", "if false && !prefilter.could_match(input) {",
               "one_zwsp_document_records_actual_lazy_compilation_work", "source_regex_cold_work", features="test-support")
        reject("legacy_context", PIPELINE_SOURCE,
               ".try_detect_with_context(input, ctx)", ".try_detect(input)",
               "legacy_regex_detectors_keep_glued_and_internal_source_boundaries", "legacy_source_boundaries")
        reject("folded_peer_context", DICTIONARY_SOURCE,
               "                                    before_gap,\n                                    after_gap,",
               "                                    false,\n                                    false,",
               "dictionary::tests::folded_record_peer_keeps_source_boundaries_after_length_changing_case_fold", None)
        reject("joined_run", REGEX_SOURCE, "if !self.complete_labelled_value {", "if true {",
               "regex::tests::joined_recovery_uses_source_gaps_for_bounded_captures", None)
        reject("linear_gap_search", REGEX_SOURCE,
               "let gap = gaps.partition_point(|&at| {\n            visit();\n            at <= scan.span.start\n        });",
               "let gap = gaps.iter().position(|&at| { visit(); at > scan.span.start }).unwrap_or(gaps.len());",
               "regex::tests::joined_recovery_containment_work_is_bounded", None)
        reject("linear_containment_search", REGEX_SOURCE, "if max_end < scan.span.end {",
               "if !scans.iter().any(|existing| { visit(); existing.span.start <= scan.span.start && existing.span.end >= scan.span.end }) {",
               "regex::tests::joined_recovery_containment_work_is_bounded", None)
    finally:
        for path, original in originals.items():
            path.write_bytes(original)
    if unknown := set(args.probe) - seen:
        raise SystemExit(f"Unknown mutation names: {sorted(unknown)}")
    print(f"MUTATION DONE: {completed} mutations rejected; source restored")


if __name__ == "__main__":
    main()
