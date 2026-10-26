#!/usr/bin/env python3
"""Kill JSON-label and bounded-sweep regressions, restoring committed sources."""

import argparse
import hashlib
import json
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parents[1]


def git(*args):
    return subprocess.check_output(["git", *args], cwd=ROOT, text=True).strip()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output-dir", type=Path, default=ROOT / "target/scanner-mutations")
    args = parser.parse_args()
    if git("status", "--porcelain"):
        raise SystemExit("Commit all changes before mutation probes; a clean checkout is required.")
    head = git("rev-parse", "HEAD")
    if "\ngpgsig " not in git("cat-file", "-p", head):
        raise SystemExit("Mutation probes require a signed HEAD.")
    out = args.output_dir.resolve()
    out.mkdir(parents=True, exist_ok=True)
    probes = [
        (
            'drop-json-label-ownership',
            'crates/gaze-recognizers/src/regex.rs',
            '    if let Some(scan) = index.scan_json_label_cell(input, capture.clone()) {',
            '    if let Some(scan) = None::<LabelledValueScan> {',
            'json_label_close_owns_following_value',
            'gaze-recognizers',
        ),
        (
            'rebuild-quote-events-per-query',
            'crates/gaze-recognizers/src/regex/scalar_index.rs',
            '        let index = self\n            .quotes',
            '        self.quotes.clear();\n        let index = self\n            .quotes',
            'doubled_apostrophe_prefix_work_is_linear',
            'gaze-recognizers',
        ),
        (
            'require-whole-json-document',
            'crates/gaze-recognizers/src/regex/scalar_index.rs',
            '    let regions = json_regions(input, _frames);',
            '    let regions = if serde_json::from_str::<serde_json::Value>(input).is_ok() { json_regions(input, _frames) } else { Vec::new() };',
            'json_label_close_owns_following_value',
            'gaze-recognizers',
        ),
        (
            'ignore-enclosing-label-key',
            'crates/gaze-recognizers/src/regex/scalar_index.rs',
            '        let enclosing_close = {',
            '        let enclosing_close = None::<usize>;\n        let _disabled_enclosing_close = {',
            'json_label_close_owns_following_value',
            'gaze-recognizers',
        ),
        (
            'drop-container-value-ownership',
            'crates/gaze-recognizers/src/regex/scalar_index.rs',
            '                            values.insert(opening, span.clone());',
            '                            let _ = (opening, span);',
            'json_label_close_owns_following_value',
            'gaze-recognizers',
        ),
        (
            'allow-header-label-as-value',
            'crates/gaze-recognizers/src/regex/scalar_index.rs',
            '!array_cell || !json_scalar_is_label(input, &values.frames, span)',
            '!array_cell || true || !json_scalar_is_label(input, &values.frames, span)',
            'array_header_label_is_not_a_private_value',
            'gaze-recognizers',
        ),
        (
            'remove-labelled-sweep-bound',
            'crates/gaze/src/sweep.rs',
            '!source.field_owned_scalar() || text.len() <= MAX_LABELLED_REPEAT_BYTES',
            '!source.field_owned_scalar() || text.len() <= usize::MAX',
            'pipeline_labelled_sweep_work_and_size_are_bounded',
            'gaze-pii',
        ),
        (
            'discard-oversized-session-provenance',
            'crates/gaze/src/session.rs',
            '    // Keep field ownership provenance so every turn applies the same repeat rules.',
            '    if raw.len() > crate::sweep::MAX_LABELLED_REPEAT_BYTES { return false; }',
            'pipeline_labelled_sweep_work_and_size_are_bounded',
            'gaze-pii',
        ),
        ('repeat-alphabetic-labelled-values', 'crates/gaze/src/sweep.rs', 'if source.field_owned_scalar() && !labelled_repeat_has_digit(value)', 'if false && source.field_owned_scalar() && !labelled_repeat_has_digit(value)', 'labelled_repeat_preserves_json_keys_literals_and_schema', 'gaze-pii'),
        ('sweep-json-keys-and-atoms', 'crates/gaze/src/sweep.rs', 'if source.repeat_seed.is_some() && !json.allows(&span)', 'if false && source.repeat_seed.is_some() && !json.allows(&span)', 'labelled_repeat_preserves_json_keys_literals_and_schema', 'gaze-pii'),
        ('own-first-benign-container-scalar', 'crates/gaze-recognizers/src/regex/scalar_index.rs', 'if eligible && first.is_none()', 'if first.is_none()', 'json_record_structure_and_covered_values_are_preserved', 'gaze-recognizers'),
        ('ignore-plain-key-value-ownership', 'crates/gaze-recognizers/src/regex/scalar_index.rs', '                if key {\n                    values.keys.insert(close);\n                    if matches!', '                if false && key {\n                    values.keys.insert(close);\n                    if matches!', 'json_record_structure_and_covered_values_are_preserved', 'gaze-recognizers'),
        ('retain-broken-prefix-quote-state', 'crates/gaze-types/src/json_regions.rs', "if matches!(byte, b'\\n' | b'\\r')", "if false && matches!(byte, b'\\n' | b'\\r')", 'json_record_structure_and_covered_values_are_preserved', 'gaze-recognizers'),
        ('ignore-unicode-label-whitespace', 'crates/gaze-recognizers/src/regex/scalar_index.rs', 'gaze_types::json_regions::json_whitespace_escape_len(tail)', 'None::<usize>', 'unicode_label_whitespace_owns_the_value', 'gaze-recognizers'),
        ('drop-padded-labelled-sources', 'crates/gaze/src/sweep.rs', '            .filter(|source| seen.insert(source.clone()))', '            .filter(|source| source.repeat_seed.is_none() || source.raw.len() <= MAX_LABELLED_REPEAT_BYTES)\n            .filter(|source| seen.insert(source.clone()))', 'padded_labelled_values_repeat_in_document_and_session', 'gaze-pii'),
        ('keep-trailing-padding-in-patterns', 'crates/gaze/src/sweep.rs', '        value = value.trim();', '        value = value.trim_start();', 'padded_labelled_values_repeat_in_document_and_session', 'gaze-pii'),
        ('tokenize-labels-inside-json-keys', 'crates/gaze-recognizers/src/regex/scalar_index.rs', '.or_else(|| enclosing_key.map(|_| capture.start..capture.start))', '.or_else(|| None::<std::ops::Range<usize>>.or_else(|| { let _ = enclosing_key; None }))', 'json_object_keys_are_never_private_value_spans', 'gaze-recognizers'),
        ('split-json-escape-on-repeat', 'crates/gaze/src/sweep.rs', 'if splits_escape {', 'if false && splits_escape {', 'labelled_repeats_cannot_split_json_escapes', 'gaze-pii'),
        ('discard-escaped-source-spelling', 'crates/gaze/src/sweep.rs', '        if patterns.is_empty() {', '        if true || patterns.is_empty() {', 'pipeline_deep_escaped_repeat_patterns_remain_bounded', 'gaze-pii'),
        ('cap-legacy-uncertain-prefixes', 'crates/gaze/src/sweep.rs', '!source.field_owned_scalar() || text.len() <= MAX_LABELLED_REPEAT_BYTES', 'source.repeat_seed.is_none() || text.len() <= MAX_LABELLED_REPEAT_BYTES', 'legacy_uncertain_prefixes_keep_their_floor_and_long_values', 'gaze-pii'),
        ('apply-field-digit-floor-to-legacy-prefixes', 'crates/gaze/src/sweep.rs', 'source.field_owned_scalar() && !labelled_repeat_has_digit(value)', 'source.repeat_seed.is_some() && !labelled_repeat_has_digit(value)', 'legacy_uncertain_prefixes_keep_their_floor_and_long_values', 'gaze-pii'),
        ('bound-decoded-padding-before-trimming', 'crates/gaze/src/sweep.rs', 'view.text.trim_end().len()', 'view.text.len()', 'deeply_escaped_padding_keeps_trimmed_repeats_across_turns', 'gaze-pii'),
        ('allow-unbounded-decoding-cascade', 'crates/gaze/src/sweep.rs', 'if depth >= MAX_ESCAPED_REPEAT_DEPTH {', 'if false && depth >= MAX_ESCAPED_REPEAT_DEPTH {', 'cascading_unicode_escapes_cannot_create_unbounded_repeat_work', 'gaze-pii'),
    ]
    receipts = []
    for name, relative, old, new, test, package in probes:
        path = ROOT / relative
        original = path.read_bytes()
        source = original.decode()
        if source.count(old) != 1:
            raise SystemExit(f"Mutation anchor changed: {name}; update this probe.")
        log = out / f"{name}.log"
        try:
            path.write_text(source.replace(old, new))
            command = ["cargo", "test", "-p", package, "--lib", test, "--", "--nocapture"]
            with log.open("w") as stream:
                result = subprocess.run(command, cwd=ROOT, stdout=stream, stderr=subprocess.STDOUT, check=False)
            output = log.read_text()
            compiled = "Finished `test` profile" in output and "Running unittests" in output
            killed = result.returncode == 101 and compiled and f"{test} ... FAILED" in output and "test result: FAILED. 0 passed; 1 failed" in output
            receipt = dict(mutation=name, head=head, command=command, test=test, compiled=compiled, exit=result.returncode, killed=killed, original_sha256=hashlib.sha256(original).hexdigest(), log=str(log))
        finally:
            path.write_bytes(original)
            if path.read_bytes() != original:
                raise SystemExit(f"Source restoration failed: {relative}")
        receipt["restored_sha256"] = hashlib.sha256(path.read_bytes()).hexdigest()
        receipts.append(receipt)
        (out / "receipts.json").write_text(json.dumps(receipts, indent=2) + "\n")
        print(json.dumps(receipt), flush=True)
        if not killed:
            raise SystemExit(f"Mutant survived or did not compile: {name}")
    if git("status", "--porcelain") or git("rev-parse", "HEAD") != head:
        raise SystemExit("Checkout changed during probes.")
    print("All mutants compiled, failed their named test, and were restored.")


if __name__ == "__main__":
    main()
