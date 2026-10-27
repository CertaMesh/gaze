#!/usr/bin/env python3
"""Disable committed separator protections and require behavioural test failures."""
import os
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parents[2]
SOURCE = ROOT / "crates/gaze/src/normalize.rs"
REGEX_SOURCE = ROOT / "crates/gaze-recognizers/src/regex.rs"
SOURCE_REGEX = ROOT / "crates/gaze-recognizers/src/source_regex.rs"
ANCHOR_SOURCE = ROOT / "crates/gaze-recognizers/src/anchored_match.rs"
PIPELINE_SOURCE = ROOT / "crates/gaze/src/pipeline.rs"
# One representative per Unicode range, plus every original removed character.
CODEPOINTS = (0xAD, 0x34F, 0x61C, 0x115F, 0x17B4, 0x180E, 0x200B, 0x200C,
              0x200D, 0x202A, 0x2060, 0x3164, 0xFE0F, 0xFEFF, 0xFFA0,
              0xFFF0, 0x1BCA0, 0x1D173, 0xE0067)


def main():
    paths = (SOURCE, REGEX_SOURCE, SOURCE_REGEX, ANCHOR_SOURCE, PIPELINE_SOURCE)
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
            print(f"{name}: regression rejected mutation", flush=True)
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
        reject("source_boundaries", REGEX_SOURCE,
               "let additional = self.scanned_spans_in_view(input, source_spans, Some(regex));",
               "let additional = self.scanned_spans_in_view(input, source_spans, None);",
               "every_default_ignorable_preserves_value_boundaries", codepoint=0x200B)
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
    print(f"MUTATION DONE: {len(CODEPOINTS) + 9} mutations rejected; source restored")


if __name__ == "__main__":
    main()
