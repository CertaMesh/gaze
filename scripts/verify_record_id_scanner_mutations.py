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
    parser.add_argument("--only", action="append", help="Run a named probe; repeat to select several")
    parser.add_argument("--keep-going", action="store_true", help="Collect all compiled mutation results before failing")
    args = parser.parse_args()
    if git("status", "--porcelain"):
        raise SystemExit("Commit all changes before mutation probes; a clean checkout is required.")
    head = git("rev-parse", "HEAD")
    if "\ngpgsig " not in git("cat-file", "-p", head):
        raise SystemExit("Mutation probes require a signed HEAD.")
    out = args.output_dir.resolve()
    out.mkdir(parents=True, exist_ok=True)
    probes = [
        ('drop-json-label-ownership', 'crates/gaze-recognizers/src/regex/scalar_index.rs', '        if self.json_label_values.is_none() {', '        if !input.is_empty() { return None; }\n        if self.json_label_values.is_none() {', 'json_label_close_owns_following_value', 'gaze-recognizers'),
        ('rebuild-quote-events-per-query', 'crates/gaze-recognizers/src/regex/scalar_index.rs', '        let index = self\n            .quotes', '        self.quotes.clear();\n        let index = self\n            .quotes', 'doubled_apostrophe_prefix_work_is_linear', 'gaze-recognizers'),
        ('require-whole-json-document', 'crates/gaze-recognizers/src/regex/scalar_index.rs', '    let regions = json_regions(input, _frames);', '    let regions = if serde_json::from_str::<serde_json::Value>(input).is_ok() { json_regions(input, _frames) } else { Vec::new() };', 'json_label_close_owns_following_value', 'gaze-recognizers'),
        ('ignore-enclosing-label-key', 'crates/gaze-recognizers/src/regex/scalar_index.rs', '        let enclosing_close = {', '        let enclosing_close = None::<usize>;\n        let _disabled_enclosing_close = {', 'json_label_close_owns_following_value', 'gaze-recognizers'),
        ('drop-container-value-ownership', 'crates/gaze-recognizers/src/regex/scalar_index.rs', '                        values.insert(container.opening, container.spans.clone());', '                        let _ = container.opening;', 'json_label_close_owns_following_value', 'gaze-recognizers'),
        ('allow-header-label-as-value', 'crates/gaze-recognizers/src/regex/scalar_index.rs', '!array_cell\n                            || !span\n                                .iter()\n                                .any(|span| json_scalar_is_label(input, &values.frames, span))', 'true', 'array_header_label_is_not_a_private_value', 'gaze-recognizers'),
        ('remove-labelled-sweep-bound', 'crates/gaze/src/sweep.rs', '!source.field_owned_scalar() || text.len() <= MAX_LABELLED_REPEAT_BYTES', '!source.field_owned_scalar() || text.len() <= usize::MAX', 'pipeline_labelled_sweep_work_and_size_are_bounded', 'gaze-pii'),
        ('discard-oversized-session-provenance', 'crates/gaze/src/session.rs', '    // Keep field ownership provenance so every turn applies the same repeat rules.', '    if raw.len() > crate::sweep::MAX_LABELLED_REPEAT_BYTES { return false; }', 'pipeline_labelled_sweep_work_and_size_are_bounded', 'gaze-pii'),
        ('repeat-alphabetic-labelled-values', 'crates/gaze/src/sweep.rs', 'if source.field_owned_scalar() && !labelled_repeat_has_digit(value)', 'if false && source.field_owned_scalar() && !labelled_repeat_has_digit(value)', 'labelled_repeat_preserves_json_keys_literals_and_schema', 'gaze-pii'),
        ('sweep-json-keys-and-atoms', 'crates/gaze/src/sweep.rs', 'if !json.allows(&span) || json.urls.contains(text, &span)', 'if json.urls.contains(text, &span)', 'labelled_repeat_preserves_json_keys_literals_and_schema', 'gaze-pii'),
        ('own-incidental-container-strings', 'crates/gaze-recognizers/src/regex/scalar_index.rs', "container.kind == b'[' || container.id_key", 'true', 'containers_own_all_and_only_explicit_record_values', 'gaze-recognizers'),
        ('ignore-plain-key-value-ownership', 'crates/gaze-recognizers/src/regex/scalar_index.rs', '                if key {\n                    values.keys.insert(close);\n                    if matches!', '                if false && key {\n                    values.keys.insert(close);\n                    if matches!', 'json_record_structure_and_covered_values_are_preserved', 'gaze-recognizers'),
        ('hide-same-line-json-after-stray-quotes', 'crates/gaze-types/src/json_regions.rs', 'for start in quoted_openings {', 'for start in quoted_openings.into_iter().take(0) {', 'truncated_prefixes_do_not_hide_complete_records', 'gaze-types'),
        ('ignore-unicode-label-whitespace', 'crates/gaze-recognizers/src/regex/scalar_index.rs', 'gaze_types::json_regions::json_whitespace_escape_len(tail)', 'None::<usize>', 'unicode_label_whitespace_owns_the_value', 'gaze-recognizers'),
        ('drop-padded-labelled-sources', 'crates/gaze/src/sweep.rs', '            .filter(|source| seen.insert(source.clone()))', '            .filter(|source| source.repeat_seed.is_none() || source.raw.len() <= MAX_LABELLED_REPEAT_BYTES)\n            .filter(|source| seen.insert(source.clone()))', 'padded_labelled_values_repeat_in_document_and_session', 'gaze-pii'),
        ('tokenize-labels-inside-json-keys', 'crates/gaze-recognizers/src/regex/scalar_index.rs', 'enclosing_key.map(|_| std::iter::once(capture.start..capture.start).collect())', 'enclosing_key.and_then(|_| None::<Vec<std::ops::Range<usize>>>)', 'json_object_keys_are_never_private_value_spans', 'gaze-recognizers'),
        ('split-json-escape-on-repeat', 'crates/gaze/src/sweep.rs', 'if splits_escape {', 'if false && splits_escape {', 'labelled_repeats_cannot_split_json_escapes', 'gaze-pii'),
        ('discard-derived-exact-patterns', 'crates/gaze/src/sweep.rs', '        if patterns.is_empty() {', '        if true || patterns.is_empty() {', 'pipeline_deep_escaped_repeat_patterns_remain_bounded', 'gaze-pii'),
        ('cap-legacy-uncertain-prefixes', 'crates/gaze/src/sweep.rs', '!source.field_owned_scalar() || text.len() <= MAX_LABELLED_REPEAT_BYTES', 'source.repeat_seed.is_none() || text.len() <= MAX_LABELLED_REPEAT_BYTES', 'legacy_uncertain_prefixes_keep_their_floor_and_long_values', 'gaze-pii'),
        ('apply-field-digit-floor-to-legacy-prefixes', 'crates/gaze/src/sweep.rs', 'source.field_owned_scalar() && !labelled_repeat_has_digit(value)', 'source.repeat_seed.is_some() && !labelled_repeat_has_digit(value)', 'legacy_uncertain_prefixes_keep_their_floor_and_long_values', 'gaze-pii'),
        ('bound-decoded-padding-before-trimming', 'crates/gaze/src/sweep.rs', 'view.text.trim_end().len()', 'view.text.len()', 'deeply_escaped_padding_keeps_trimmed_repeats_across_turns', 'gaze-pii'),
        ('allow-unbounded-decoding-cascade', 'crates/gaze/src/sweep.rs', 'if depth >= MAX_ESCAPED_REPEAT_DEPTH {', 'if false && depth >= MAX_ESCAPED_REPEAT_DEPTH {', 'cascading_unicode_escapes_cannot_create_unbounded_repeat_work', 'gaze-pii'),
        ('fold-exact-family-scalar-seeds', 'crates/gaze/src/sweep.rs', '            if uncertain {\n                push(PatternKind::Folded, fold(body).text, link);', '            if true || uncertain {\n                push(PatternKind::Folded, fold(body).text, link);', 'field_owned_families_keep_byte_exact_matching', 'gaze-pii'),
        ('skip-mixed-source-json-gate', 'crates/gaze/src/sweep.rs', 'if !json.allows(&span) || json.urls.contains(text, &span)', 'if (source.repeat_seed.is_some() && !json.allows(&span)) || json.urls.contains(text, &span)', 'mixed_carriers_preserve_json_keys_types_and_string_boundaries', 'gaze-pii'),
        ('ignore-escaped-json-sweep-layers', 'crates/gaze/src/sweep.rs', 'let Some(decoded) = view.decode() else {\n                break;\n            };\n            view = decoded;\n        }\n        Self {', 'let Some(decoded) = None::<EscapedView> else {\n                break;\n            };\n            view = decoded;\n        }\n        Self {', 'mixed_carriers_preserve_json_keys_types_and_string_boundaries', 'gaze-pii'),
        ('allow-schema-container-ownership', 'crates/gaze-recognizers/src/regex/scalar_index.rs', 'if container.schema {', 'if false && container.schema {', 'schema_containers_never_own_metadata_or_id_children', 'gaze-recognizers'),
        ('own-only-first-container-scalar', 'crates/gaze-recognizers/src/regex.rs', 'additional.extend(scans);', 'additional.extend(scans.take(0));', 'containers_own_all_and_only_explicit_record_values', 'gaze-recognizers'),
        ('require-identical-raw-repeat-spelling', 'crates/gaze/src/sweep.rs', 'derived.sources.get(&view.text[found.range()])', 'derived.sources.get(&text[span.clone()])', 'decoded_record_ids_repeat_between_raw_spellings_and_turns', 'gaze-pii'),
        ('skip-escaped-prose-label-index', 'crates/gaze-recognizers/src/regex/scalar_index.rs', 'if let Some(decoded) = embedded {', 'if let Some(decoded) = embedded.as_mut().filter(|_| false) {', 'escaped_prose_label_index_owns_the_value_cell', 'gaze-recognizers'),
        ('ignore-escaped-zero-width-label-tail', 'crates/gaze-types/src/json_regions.rs', "ch.is_whitespace() || *ch == '\\u{200b}'", 'ch.is_whitespace()', 'unicode_label_whitespace_owns_the_value', 'gaze-recognizers'),
        ('ignore-literal-zero-width-label-tail', 'crates/gaze-recognizers/src/regex/scalar_index.rs', "ch.is_whitespace() || ch == '\\u{200b}'", 'ch.is_whitespace()', 'unicode_label_whitespace_owns_the_value', 'gaze-recognizers'),
        ('let-empty-container-value-veto-later-ids', 'crates/gaze-recognizers/src/regex/scalar_index.rs', 'span.retain(|span| !span.is_empty());', 'span.retain(|_| true);', 'containers_own_all_and_only_explicit_record_values', 'gaze-recognizers'),
        ('allow-non-id-owned-values', 'crates/gaze-recognizers/src/regex.rs', '                            constrain_owned_identifier(\n                                input,\n                                scan,\n                                markup,\n                                full.start(),\n                                sql_schema,\n                                !scan.parent_scalar\n                                    && scalar_index.inside_sql_statement(input, full.start()),\n                            );', '                            let _ = (markup, sql_schema);', 'record_value_shape_preserves_carrier_syntax', 'gaze-pii'),
        ('skip-lexical-json-like-gate', 'crates/gaze/src/sweep.rs', '            collect_lexical_sweep_blocks(&view.text, &view.source, &mut lexical_atoms);', '        // Lexical protection disabled.', 'lexical_sweep_preserves_non_strict_keys_numbers_and_literals', 'gaze-pii'),
        ('rebuild-url-runs-per-hit', 'crates/gaze/src/sweep.rs', '        let at = self.urls.partition_point(|run| run.end <= span.start);', '        let _rebuilt = UrlRuns::new(_text);\n        let at = self.urls.partition_point(|run| run.end <= span.start);', 'minified_record_url_lookup_work_is_linear', 'gaze-pii'),
        ('drop-leading-symbol-ids', 'crates/gaze-recognizers/src/regex.rs', "strip_prefix(['-', '@', '_'])", "strip_prefix(['-'])", 'owned_identifier_relaxations_preserve_covered_shapes', 'gaze-pii'),
        ('drop-negative-ids', 'crates/gaze-recognizers/src/regex.rs', "strip_prefix(['-', '@', '_'])", "strip_prefix(['@', '_'])", 'owned_identifier_relaxations_preserve_covered_shapes', 'gaze-pii'),
        ('drop-grouped-ids', 'crates/gaze-recognizers/src/regex.rs', '        let grouping = input', '        let grouping = false && input', 'owned_identifier_relaxations_preserve_covered_shapes', 'gaze-pii'),
        ('let-unterminated-quotes-cross-lines', 'crates/gaze-recognizers/src/regex.rs', 'uncertain_scalar_end(input, start, enclosing.end)', 'enclosing.end', 'unterminated_id_quotes_never_own_next_line_syntax', 'gaze-pii'),
        ('block-whole-id-on-key-looking-content', 'crates/gaze/src/sweep.rs', 'atom.start <= span.start && span.end <= atom.end', 'atom.start < span.end', 'lexical_sweep_keeps_whole_private_values_with_key_looking_content', 'gaze-pii'),
        ('drop-digit-free-base64-ids', 'crates/gaze-recognizers/src/regex.rs', '    let base64 = base64_context', '    let base64 = false && base64_context', 'base64_record_ids_keep_padding_without_owning_operators', 'gaze-pii'),
        ('allow-unquoted-html-value-emission', 'crates/gaze-recognizers/src/regex.rs', '        || (in_markup_tag && !scan.quoted_value)', '        || false', 'record_value_shape_preserves_carrier_syntax', 'gaze-pii'),
        ('allow-unquoted-html-repeat-emission', 'crates/gaze/src/sweep.rs', '        if self.markup.blocks_repeat(span) {', '        if false && self.markup.blocks_repeat(span) {', 'seeded_html_repeats_preserve_unquoted_attribute_syntax', 'gaze-pii'),
        ('drop-decoded-markup-data-context', 'crates/gaze-types/src/markup_regions.rs', '        for _ in 0..32 {', '        for _ in 0..0 {', 'seeded_html_repeats_preserve_unquoted_attribute_syntax', 'gaze-pii'),
        ('allow-digit-free-space-only-base64', 'crates/gaze-recognizers/src/regex.rs', '    let base64_context = scan.quoted_value', '    let base64_context = true || scan.quoted_value', 'record_value_shape_preserves_carrier_syntax', 'gaze-pii'),
        ('allow-opening-quote-after-bare-base64', 'crates/gaze-recognizers/src/regex.rs', 'quote_end_allowed && matches!(ch, \'"\' | \'\\\'\')', 'true && matches!(ch, \'"\' | \'\\\'\')', 'record_value_shape_preserves_carrier_syntax', 'gaze-pii'),
        ('reject-quotes-inside-record-wrappers', 'crates/gaze-recognizers/src/regex.rs', '|| body.chars().any(|ch| "<>&|!${}[]()?=;".contains(ch))', '|| body.chars().any(|ch| "<>&|!${}[]()?=;\\"\'".contains(ch))', 'record_ids_keep_label_suffixes_and_quotes_inside_wrappers', 'gaze-recognizers'),
        ('drop-customer-id-number-suffix', 'crates/gaze-recognizers/embedded/core.toml', 'pattern = \'\'\'(?i)(?:\\b|(?-i:\\\\+(?:[ntrbf]|u000[9a-dA-D])))(?:customer[ _-]?(?:record[ _-]?)?(?:id|number|no\\.?|nr\\.?)|kunden(?:[ _-]?(?:id|nummer|nr\\.?)))(?:[ \\t\\u{00A0}\\u{202F}]+number)?[ \\t\\u{00A0}\\u{202F}]*(?:\\\\*["\'])?[ \\t\\u{00A0}\\u{202F}]*(?:[:=][ \\t\\u{00A0}\\u{202F}]*|[ \\t\\u{00A0}\\u{202F}]+(?:(?:is|ist|lautet|[-–—])[ \\t\\u{00A0}\\u{202F}]+)?)(?:(?:\\\\*["\'])([\\s\\S])|([\\p{L}\\p{N}+/@_#\\-"\'\\\\\\s\\u{200b}(\\[{«༺༼᚛‘‚‛“„‟‹⁅⁽₍⌈⌊〈❨❪❬❮❰❲❴⟅⟦⟨⟪⟬⟮⦃⦅⦇⦉⦋⦍⦏⦑⦓⦕⦗⧘⧚⧼⸂⸄⸉⸌⸜⸠⸢⸤⸦⸨⹂〈《「『【〔〖〘〚〝﴿︗︵︷︹︻︽︿﹁﹃﹇﹙﹛﹝（［｛｟｢`<\u2e55\u2e57\u2e59\u2e5b`§🔐⏣]))\'\'\'', 'pattern = \'\'\'(?i)(?:\\b|(?-i:\\\\+(?:[ntrbf]|u000[9a-dA-D])))(?:customer[ _-]?(?:record[ _-]?)?(?:id|number|no\\.?|nr\\.?)|kunden(?:[ _-]?(?:id|nummer|nr\\.?)))[ \\t\\u{00A0}\\u{202F}]*(?:\\\\*["\'])?[ \\t\\u{00A0}\\u{202F}]*(?:[:=][ \\t\\u{00A0}\\u{202F}]*|[ \\t\\u{00A0}\\u{202F}]+(?:(?:is|ist|lautet|[-–—])[ \\t\\u{00A0}\\u{202F}]+)?)(?:(?:\\\\*["\'])([\\s\\S])|([\\p{L}\\p{N}+/@_#\\-"\'\\\\\\s\\u{200b}(\\[{«༺༼᚛‘‚‛“„‟‹⁅⁽₍⌈⌊〈❨❪❬❮❰❲❴⟅⟦⟨⟪⟬⟮⦃⦅⦇⦉⦋⦍⦏⦑⦓⦕⦗⧘⧚⧼⸂⸄⸉⸌⸜⸠⸢⸤⸦⸨⹂〈《「『【〔〖〘〚〝﴿︗︵︷︹︻︽︿﹁﹃﹇﹙﹛﹝（［｛｟｢`<\u2e55\u2e57\u2e59\u2e5b`§🔐⏣]))\'\'\'', 'record_ids_keep_label_suffixes_and_quotes_inside_wrappers', 'gaze-recognizers'),
        ('drop-employee-id-number-suffix', 'crates/gaze-recognizers/embedded/core.toml', 'pattern = \'\'\'(?i)(?:\\b|(?-i:\\\\+(?:[ntrbf]|u000[9a-dA-D])))(?:employee[ _-]?(?:record[ _-]?)?(?:id|number|no\\.?)|personnel[ _-]?(?:id|number|no\\.?)|personal[ _-]?(?:nummer|nr\\.?)|mitarbeiter[ _-]?(?:id|nummer|nr\\.?))(?:[ \\t\\u{00A0}\\u{202F}]+number)?[ \\t\\u{00A0}\\u{202F}]*(?:\\\\*["\'])?[ \\t\\u{00A0}\\u{202F}]*(?:[:=][ \\t\\u{00A0}\\u{202F}]*|[ \\t\\u{00A0}\\u{202F}]+(?:(?:is|ist|lautet|[-–—])[ \\t\\u{00A0}\\u{202F}]+)?)(?:(?:\\\\*["\'])([\\s\\S])|([\\p{L}\\p{N}+/@_#\\-"\'\\\\\\s\\u{200b}(\\[{«༺༼᚛‘‚‛“„‟‹⁅⁽₍⌈⌊〈❨❪❬❮❰❲❴⟅⟦⟨⟪⟬⟮⦃⦅⦇⦉⦋⦍⦏⦑⦓⦕⦗⧘⧚⧼⸂⸄⸉⸌⸜⸠⸢⸤⸦⸨⹂〈《「『【〔〖〘〚〝﴿︗︵︷︹︻︽︿﹁﹃﹇﹙﹛﹝（［｛｟｢`<\u2e55\u2e57\u2e59\u2e5b`§🔐⏣]))\'\'\'', 'pattern = \'\'\'(?i)(?:\\b|(?-i:\\\\+(?:[ntrbf]|u000[9a-dA-D])))(?:employee[ _-]?(?:record[ _-]?)?(?:id|number|no\\.?)|personnel[ _-]?(?:id|number|no\\.?)|personal[ _-]?(?:nummer|nr\\.?)|mitarbeiter[ _-]?(?:id|nummer|nr\\.?))[ \\t\\u{00A0}\\u{202F}]*(?:\\\\*["\'])?[ \\t\\u{00A0}\\u{202F}]*(?:[:=][ \\t\\u{00A0}\\u{202F}]*|[ \\t\\u{00A0}\\u{202F}]+(?:(?:is|ist|lautet|[-–—])[ \\t\\u{00A0}\\u{202F}]+)?)(?:(?:\\\\*["\'])([\\s\\S])|([\\p{L}\\p{N}+/@_#\\-"\'\\\\\\s\\u{200b}(\\[{«༺༼᚛‘‚‛“„‟‹⁅⁽₍⌈⌊〈❨❪❬❮❰❲❴⟅⟦⟨⟪⟬⟮⦃⦅⦇⦉⦋⦍⦏⦑⦓⦕⦗⧘⧚⧼⸂⸄⸉⸌⸜⸠⸢⸤⸦⸨⹂〈《「『【〔〖〘〚〝﴿︗︵︷︹︻︽︿﹁﹃﹇﹙﹛﹝（［｛｟｢`<\u2e55\u2e57\u2e59\u2e5b`§🔐⏣]))\'\'\'', 'record_ids_keep_label_suffixes_and_quotes_inside_wrappers', 'gaze-recognizers'),
        ('drop-record-id-number-suffix', 'crates/gaze-recognizers/embedded/core.toml', 'pattern = \'\'\'(?i)(?:\\b|(?-i:\\\\+(?:[ntrbf]|u000[9a-dA-D])))(?:(?:person|contact|member)[ _-]?(?:id|number|no\\.?)|membership[ _-]?number|(?:person|contact)[ _-]?record[ _-]?id|mitglieds[ _-]?(?:nummer|nr\\.?)|personen[ _-]?id|kontakt[ _-]?id|(?:personen|kontakt)[ _-]?datensatz[ _-]?id)(?:[ \\t\\u{00A0}\\u{202F}]+number)?[ \\t\\u{00A0}\\u{202F}]*(?:\\\\*["\'])?[ \\t\\u{00A0}\\u{202F}]*(?:[:=][ \\t\\u{00A0}\\u{202F}]*|[ \\t\\u{00A0}\\u{202F}]+(?:(?:is|ist|lautet|[-–—])[ \\t\\u{00A0}\\u{202F}]+)?)(?:(?:\\\\*["\'])([\\s\\S])|([\\p{L}\\p{N}+/@_#\\-"\'\\\\\\s\\u{200b}(\\[{«༺༼᚛‘‚‛“„‟‹⁅⁽₍⌈⌊〈❨❪❬❮❰❲❴⟅⟦⟨⟪⟬⟮⦃⦅⦇⦉⦋⦍⦏⦑⦓⦕⦗⧘⧚⧼⸂⸄⸉⸌⸜⸠⸢⸤⸦⸨⹂〈《「『【〔〖〘〚〝﴿︗︵︷︹︻︽︿﹁﹃﹇﹙﹛﹝（［｛｟｢`<\u2e55\u2e57\u2e59\u2e5b`§🔐⏣]))\'\'\'', 'pattern = \'\'\'(?i)(?:\\b|(?-i:\\\\+(?:[ntrbf]|u000[9a-dA-D])))(?:(?:person|contact|member)[ _-]?(?:id|number|no\\.?)|membership[ _-]?number|(?:person|contact)[ _-]?record[ _-]?id|mitglieds[ _-]?(?:nummer|nr\\.?)|personen[ _-]?id|kontakt[ _-]?id|(?:personen|kontakt)[ _-]?datensatz[ _-]?id)[ \\t\\u{00A0}\\u{202F}]*(?:\\\\*["\'])?[ \\t\\u{00A0}\\u{202F}]*(?:[:=][ \\t\\u{00A0}\\u{202F}]*|[ \\t\\u{00A0}\\u{202F}]+(?:(?:is|ist|lautet|[-–—])[ \\t\\u{00A0}\\u{202F}]+)?)(?:(?:\\\\*["\'])([\\s\\S])|([\\p{L}\\p{N}+/@_#\\-"\'\\\\\\s\\u{200b}(\\[{«༺༼᚛‘‚‛“„‟‹⁅⁽₍⌈⌊〈❨❪❬❮❰❲❴⟅⟦⟨⟪⟬⟮⦃⦅⦇⦉⦋⦍⦏⦑⦓⦕⦗⧘⧚⧼⸂⸄⸉⸌⸜⸠⸢⸤⸦⸨⹂〈《「『【〔〖〘〚〝﴿︗︵︷︹︻︽︿﹁﹃﹇﹙﹛﹝（［｛｟｢`<\u2e55\u2e57\u2e59\u2e5b`§🔐⏣]))\'\'\'', 'record_ids_keep_label_suffixes_and_quotes_inside_wrappers', 'gaze-recognizers'),
        ('skip-repeat-provenance-budget', 'crates/gaze/src/session.rs', '    pub(crate) fn record_repeat_seed(\n        &mut self,\n        family: Option<&str>,\n        class: &PiiClass,\n        raw: &str,\n        seed: RepeatSeed,\n    ) {\n        let family_name = family.unwrap_or(DEFAULT_COUNTER_FAMILY);\n        let payload = if self.manifest_limit.is_some() {', '    pub(crate) fn record_repeat_seed(\n        &mut self,\n        family: Option<&str>,\n        class: &PiiClass,\n        raw: &str,\n        seed: RepeatSeed,\n    ) {\n        let family_name = family.unwrap_or(DEFAULT_COUNTER_FAMILY);\n        let payload = if false && self.manifest_limit.is_some() {', 'repeat_seed_growth_respects_manifest_limit_before_retaining_data', 'gaze-pii'),
        ('omit-repeat-provenance-capacity', 'crates/gaze/src/session.rs', '        state.repeat_seeds_by_value.capacity(),', '        0,', 'repeat_seed_charge_tracks_growth_and_duplicate_updates', 'gaze-pii'),
        ('omit-repeat-provenance-buckets', 'crates/gaze/src/session.rs', '.saturating_add(state.repeat_seeds_by_value.capacity().saturating_mul(256))', '.saturating_add(0)', 'repeat_seed_charge_tracks_growth_and_duplicate_updates', 'gaze-pii'),
        ('omit-repeat-provenance-key-payload', 'crates/gaze/src/session.rs', '        .chain(state.repeat_seeds_by_value.keys().map(|key| {\n            key.raw\n                .capacity()\n                .saturating_add(key.family.capacity())\n                .saturating_add(key.class.class_name().len())\n        }))\n', '', 'repeat_seed_charge_tracks_growth_and_duplicate_updates', 'gaze-pii'),
        ('omit-decoded-matcher-allocation', 'crates/gaze/src/sweep.rs', '.map_or(0, DerivedExactMatcher::allocation_bytes)', '.map_or(0, |_| 0)', 'derived_matcher_and_repeat_payloads_are_charged', 'gaze-pii'),
        ('omit-sweep-repeat-seed-allocation', 'crates/gaze/src/sweep.rs', 's.repeat_seed.as_ref().map_or(0, |seed| seed.raw.capacity())', '0', 'derived_matcher_and_repeat_payloads_are_charged', 'gaze-pii'),
        ('omit-repeat-provenance-semantic-revision', 'crates/gaze/src/session.rs', '    state.semantic_revision = state\n        .semantic_revision\n        .checked_add(1)\n        .expect("semantic revision exhausted");\n    state.repeat_seeds_by_value.insert(key, seed);', '    state.repeat_seeds_by_value.insert(key, seed);', 'repeat_provenance_advances_only_semantic_changes', 'gaze-pii'),
        ('remove-tag-byte-bound', 'crates/gaze-types/src/markup_regions.rs', 'const MAX_TAG_BYTES: usize = 2048;', 'const MAX_TAG_BYTES: usize = usize::MAX;', 'non_tags_never_hide_record_values_or_repeats', 'gaze-pii'),
        ('accept-non-attribute-tag-token', 'crates/gaze-types/src/markup_regions.rs', '                if b"<\\"\'`".contains(&bytes[at]) {', '                if false && b"<\\"\'`".contains(&bytes[at]) {', 'non_tags_never_hide_record_values_or_repeats', 'gaze-pii'),
        ('drop-numeric-prose-wrappers', 'crates/gaze-recognizers/src/regex.rs', 'let numeric_wrapper = paired && !in_sql_schema && numeric_prose_wrapper(input, scan, cue_start);', 'let numeric_wrapper = false && paired && !in_sql_schema && numeric_prose_wrapper(input, scan, cue_start);', 'labelled_value_starts_keep_connectors_hashes_equals_and_numeric_wrappers', 'gaze-pii'),
        ('drop-embedded-equals-fallback', 'crates/gaze-recognizers/src/regex.rs', '        if !prefix.chars().any(char::is_numeric)', '        if false && !prefix.chars().any(char::is_numeric)', 'labelled_value_starts_keep_connectors_hashes_equals_and_numeric_wrappers', 'gaze-pii'),
        ('allow-code-type-identifiers', 'crates/gaze-recognizers/src/regex.rs', '|| type_name.is_match(token)', '|| false', 'code_and_sql_record_fields_never_seed_the_sweep', 'gaze-pii'),
        ('allow-code-function-identifiers', 'crates/gaze-recognizers/src/regex.rs', "    (!token.contains('-') && suffix.starts_with('('))", '    false', 'code_and_sql_record_fields_never_seed_the_sweep', 'gaze-pii'),
        ('allow-code-variable-identifiers', 'crates/gaze-recognizers/src/regex.rs', '        || reference', '        || false', 'code_and_sql_record_fields_never_seed_the_sweep', 'gaze-pii'),
        ('allow-code-numeric-literals', 'crates/gaze-recognizers/src/regex.rs', '        || (assignment && matches!(body, "0" | "1"))', '        || false', 'code_and_sql_record_fields_never_seed_the_sweep', 'gaze-pii'),
        ('allow-pandas-non-null-counts', 'crates/gaze-recognizers/src/regex.rs', '&& suffix.trim_start().starts_with("non-null")', '&& false', 'code_and_sql_record_fields_never_seed_the_sweep', 'gaze-pii'),
        ('allow-sql-group-continuations', 'crates/gaze-recognizers/src/regex.rs', '|| sql_continuation(next.split_whitespace().next().unwrap_or(""))', '|| false', 'labelled_value_starts_keep_connectors_hashes_equals_and_numeric_wrappers', 'gaze-pii'),
        ('drop-hash-led-identifiers', 'crates/gaze-recognizers/src/regex.rs', "if let Some(body) = value.strip_prefix('#') {", "if let Some(body) = value.strip_prefix('#').filter(|_| false) {", 'labelled_value_starts_keep_connectors_hashes_equals_and_numeric_wrappers', 'gaze-pii'),
        ('drop-space-surrounded-dash-connectors', 'crates/gaze-recognizers/embedded/core.toml', 'pattern = \'\'\'(?i)(?:\\b|(?-i:\\\\+(?:[ntrbf]|u000[9a-dA-D])))(?:customer[ _-]?(?:record[ _-]?)?(?:id|number|no\\.?|nr\\.?)|kunden(?:[ _-]?(?:id|nummer|nr\\.?)))(?:[ \\t\\u{00A0}\\u{202F}]+number)?[ \\t\\u{00A0}\\u{202F}]*(?:\\\\*["\'])?[ \\t\\u{00A0}\\u{202F}]*(?:[:=][ \\t\\u{00A0}\\u{202F}]*|[ \\t\\u{00A0}\\u{202F}]+(?:(?:is|ist|lautet|[-–—])[ \\t\\u{00A0}\\u{202F}]+)?)(?:(?:\\\\*["\'])([\\s\\S])|([\\p{L}\\p{N}+/@_#\\-"\'\\\\\\s\\u{200b}(\\[{«༺༼᚛‘‚‛“„‟‹⁅⁽₍⌈⌊〈❨❪❬❮❰❲❴⟅⟦⟨⟪⟬⟮⦃⦅⦇⦉⦋⦍⦏⦑⦓⦕⦗⧘⧚⧼⸂⸄⸉⸌⸜⸠⸢⸤⸦⸨⹂〈《「『【〔〖〘〚〝﴿︗︵︷︹︻︽︿﹁﹃﹇﹙﹛﹝（［｛｟｢`<\u2e55\u2e57\u2e59\u2e5b`§🔐⏣]))\'\'\'', 'pattern = \'\'\'(?i)(?:\\b|(?-i:\\\\+(?:[ntrbf]|u000[9a-dA-D])))(?:customer[ _-]?(?:record[ _-]?)?(?:id|number|no\\.?|nr\\.?)|kunden(?:[ _-]?(?:id|nummer|nr\\.?)))(?:[ \\t\\u{00A0}\\u{202F}]+number)?[ \\t\\u{00A0}\\u{202F}]*(?:\\\\*["\'])?[ \\t\\u{00A0}\\u{202F}]*(?:[:=][ \\t\\u{00A0}\\u{202F}]*|[ \\t\\u{00A0}\\u{202F}]+(?:(?:is|ist|lautet)[ \\t\\u{00A0}\\u{202F}]+)?)(?:(?:\\\\*["\'])([\\s\\S])|([\\p{L}\\p{N}+/@_#\\-"\'\\\\\\s\\u{200b}(\\[{«༺༼᚛‘‚‛“„‟‹⁅⁽₍⌈⌊〈❨❪❬❮❰❲❴⟅⟦⟨⟪⟬⟮⦃⦅⦇⦉⦋⦍⦏⦑⦓⦕⦗⧘⧚⧼⸂⸄⸉⸌⸜⸠⸢⸤⸦⸨⹂〈《「『【〔〖〘〚〝﴿︗︵︷︹︻︽︿﹁﹃﹇﹙﹛﹝（［｛｟｢`<\u2e55\u2e57\u2e59\u2e5b`§🔐⏣]))\'\'\'', 'labelled_value_starts_keep_connectors_hashes_equals_and_numeric_wrappers', 'gaze-pii'),
        ('allow-code-wrapped-identifiers', 'crates/gaze-recognizers/src/regex.rs', '        || (paired && code_carrier_prefix(input, cue_start))', '        || false', 'code_and_sql_record_fields_never_seed_the_sweep', 'gaze-pii'),
        ('allow-multiline-numeric-wrappers', 'crates/gaze-recognizers/src/regex.rs', "&& !input[opening..scan.span.end].contains(['\\r', '\\n'])", '&& true', 'code_and_sql_record_fields_never_seed_the_sweep', 'gaze-pii'),
        ('allow-sql-schema-wrapped-identifiers', 'crates/gaze-recognizers/src/regex/scalar_index.rs', '        let regions = self.sql_schema.get_or_insert_with(|| {', '        if !input.is_empty() { return false; }\n        let regions = self.sql_schema.get_or_insert_with(|| {', 'rule_boundary_sql_context_and_keywords', 'gaze-pii'),
        ('allow-closing-tag-attributes', 'crates/gaze-types/src/markup_regions.rs', "    if bytes.get(at) == Some(&b'/') {\n        return None;\n    }", "    if bytes.get(at) == Some(&b'/') {\n        let mut next = text.to_owned(); next.remove(at); return assignment_tag(&next, start, closers);\n    }", 'non_tags_never_hide_record_values_or_repeats', 'gaze-pii'),
        ('allow-hashes-inside-id-runs', 'crates/gaze-recognizers/src/regex.rs', "hash_lead && *at == 0 && *ch == '#'", "*ch == '#'", 'code_and_sql_record_fields_never_seed_the_sweep', 'gaze-pii'),
        ('veto-uppercase-id-prefixes', 'crates/gaze-recognizers/src/regex.rs', '\\A[a-z][a-zA-Z0-9_]*(?:_id|Id)[0-9]*\\z', '(?i)\\A[a-z]+_?id[0-9]*\\z', 'rule_boundary_references_and_types', 'gaze-pii'),
        ('veto-arbitrary-type-digits', 'crates/gaze-recognizers/src/regex.rs', '|int16|int32|int64|', '|int[0-9]+|', 'rule_boundary_references_and_types', 'gaze-pii'),
        ('veto-uppercase-short-id', 'crates/gaze-recognizers/src/regex.rs', '|[iu](?:8|16|32|64|128)|', '|[iIuU](?:8|16|32|64|128)|', 'rule_boundary_references_and_types', 'gaze-pii'),
        ('veto-hyphenated-id-before-paren', 'crates/gaze-recognizers/src/regex.rs', "!token.contains('-') && suffix.starts_with('(')", "suffix.starts_with('(')", 'rule_boundary_references_and_types', 'gaze-pii'),
        ('allow-small-code-variable', 'crates/gaze-recognizers/src/regex.rs', '        || variable', '        || false', 'rule_boundary_references_and_types', 'gaze-pii'),
        ('allow-sql-parameters', 'crates/gaze-recognizers/src/regex.rs', '        || parameter', '        || false', 'rule_boundary_sql_context_and_keywords', 'gaze-pii'),
        ('allow-lowercase-sql-grouping', 'crates/gaze-recognizers/src/regex.rs', 'token.to_ascii_uppercase().as_str(),', 'token,', 'rule_boundary_sql_context_and_keywords', 'gaze-pii'),
        ('allow-dml-grouping', 'crates/gaze-recognizers/src/regex.rs', '"AND"\n            | "OR"', '"_DISABLED_AND"\n            | "_DISABLED_OR"', 'rule_boundary_sql_context_and_keywords', 'gaze-pii'),
        ('veto-prose-select-without-from', 'crates/gaze-recognizers/src/regex/scalar_index.rs', 'if select && source.is_none_or', 'if false && select && source.is_none_or', 'rule_boundary_sql_context_and_keywords', 'gaze-pii'),
        ('veto-prose-sql-at-any-position', 'crates/gaze-recognizers/src/regex/scalar_index.rs', '(?:\\A|[;\\r\\n])[ \\t]*', '[ \\t]*', 'rule_boundary_sql_context_and_keywords', 'gaze-pii'),
        ('allow-short-letter-hash-id', 'crates/gaze-recognizers/src/regex.rs', 'body.chars().count() >= 7', 'body.chars().count() >= 1', 'rule_boundary_starts_wrappers_and_grouping', 'gaze-pii'),
        ('allow-long-hex-id', 'crates/gaze-recognizers/src/regex.rs', 'ch.is_alphabetic() && !ch.is_ascii_hexdigit()', 'ch.is_alphabetic()', 'rule_boundary_starts_wrappers_and_grouping', 'gaze-pii'),
        ('restrict-html-attribute-names', 'crates/gaze-types/src/markup_regions.rs', 'name_char(byte) || b":@.".contains(&byte)', 'name_char(byte)', 'rule_boundary_markup_grammar', 'gaze-pii'),
        ('restrict-html-unquoted-punctuation', 'crates/gaze-types/src/markup_regions.rs', 'if b"<\\"\'`".contains(&bytes[at]) {', 'if b"<,;:=\\"\'`".contains(&bytes[at]) {', 'rule_boundary_markup_grammar', 'gaze-pii'),
        ('drop-paired-html-evidence', 'crates/gaze-types/src/markup_regions.rs', '(paired || html_attributes || fragment)', '(html_attributes || fragment)', 'rule_boundary_markup_grammar', 'gaze-pii'),
        ('drop-html-attribute-evidence', 'crates/gaze-types/src/markup_regions.rs', '(paired || html_attributes || fragment)', '(paired || fragment)', 'rule_boundary_markup_grammar', 'gaze-pii'),
        ('drop-html-fragment-evidence', 'crates/gaze-types/src/markup_regions.rs', '(paired || html_attributes || fragment)', '(paired || html_attributes)', 'rule_boundary_markup_grammar', 'gaze-pii'),
        ('drop-component-html-evidence', 'crates/gaze-types/src/markup_regions.rs', '|| (component && paired)', '|| (false && component && paired)', 'rule_boundary_markup_grammar', 'gaze-pii'),
        ('ignore-tag-name-evidence', 'crates/gaze-types/src/markup_regions.rs', "let known_tag = known_tag(&folded_tag) || tag.contains('-');", 'let known_tag = true;', 'rule_boundary_markup_grammar', 'gaze-pii'),
        ('allow-shell-fragment-tail', 'crates/gaze-types/src/markup_regions.rs', "byte.is_ascii_whitespace() || *byte == b'<'", 'true', 'rule_boundary_markup_grammar', 'gaze-pii'),
        ('allow-sql-keyword-attributes', 'crates/gaze-types/src/markup_regions.rs', 'let fragment = non_sql_attributes', 'let fragment = true', 'rule_boundary_markup_grammar', 'gaze-pii'),
    ]
    probes.append(('ignore-html-fragment-spacing', 'crates/gaze-types/src/markup_regions.rs', 'let compact_end = at > start && !bytes[at - 1].is_ascii_whitespace();', 'let compact_end = true;', 'rule_boundary_markup_grammar', 'gaze-pii'))
    probes.append(('extend-schema-through-prose', 'crates/gaze-recognizers/src/regex/scalar_index.rs', 'if let Some(&end) = closes.get(&(statement.end()-1)) {', 'if let Some(&_end) = closes.get(&(statement.end()-1)) { let end = input.len();', 'rule_boundary_sql_context_and_keywords', 'gaze-pii'))
    probes.append(('drop-svg-known-tag', 'crates/gaze-types/src/markup_regions.rs', '| "animatemotion"', '| "_mutant_animatemotion"', 'rule_boundary_standard_tag_names', 'gaze-pii'))
    probes.append(('drop-mathml-known-tag', 'crates/gaze-types/src/markup_regions.rs', '| "apply"', '| "_mutant_apply"', 'rule_boundary_standard_tag_names', 'gaze-pii'))
    probes.append(('count-hash-bytes-instead-of-characters', 'crates/gaze-recognizers/src/regex.rs', 'body.chars().count() >= 7', 'body.len() >= 7', 'rule_boundary_starts_wrappers_and_grouping', 'gaze-pii'))
    probes.append(('drop-unicode-hash-letters', 'crates/gaze-recognizers/src/regex.rs', 'ch.is_alphabetic() && !ch.is_ascii_hexdigit()', 'ch.is_ascii_alphabetic() && !ch.is_ascii_hexdigit()', 'rule_boundary_starts_wrappers_and_grouping', 'gaze-pii'))
    probes.append(('drop-quoted-sql-table-context', 'crates/gaze-recognizers/src/regex/scalar_index.rs', r'update\s+{table}\s+set', r'update\s+[a-z_][a-z0-9_.]*\s+set', 'rule_boundary_sql_context_and_keywords', 'gaze-pii'))
    if args.only:
        unknown = set(args.only) - {probe[0] for probe in probes}
        if unknown:
            raise SystemExit(f"Unknown probes: {sorted(unknown)}")
        probes = [probe for probe in probes if probe[0] in args.only]
    receipts = []
    baselines = {}
    for name, relative, old, new, test, package in probes:
        target = (["--test", "record_id_boundaries"]
                  if test == "record_ids_keep_label_suffixes_and_quotes_inside_wrappers"
                  else ["--lib"])
        command = ["cargo", "test", "-p", package, *target, test, "--", "--nocapture"]
        baseline = out / f"{package}-{test}.baseline.log"
        if (package, test) not in baselines:
            with baseline.open("w") as stream:
                result = subprocess.run(command, cwd=ROOT, stdout=stream, stderr=subprocess.STDOUT, check=False)
            output = baseline.read_text()
            if result.returncode or "test result: ok. 1 passed; 0 failed" not in output:
                raise SystemExit(f"Baseline did not execute and pass exactly one test: {baseline}")
            baselines[package, test] = str(baseline)
        path = ROOT / relative
        original = path.read_bytes()
        source = original.decode()
        if source.count(old) != 1:
            raise SystemExit(f"Mutation anchor changed: {name}; update this probe.")
        log = out / f"{name}.log"
        try:
            path.write_text(source.replace(old, new))
            with log.open("w") as stream:
                result = subprocess.run(command, cwd=ROOT, stdout=stream, stderr=subprocess.STDOUT, check=False)
            output = log.read_text()
            compiled = "Finished `test` profile" in output and ("Running unittests" in output or "Running tests/" in output)
            killed = result.returncode == 101 and compiled and f"{test} ... FAILED" in output and "test result: FAILED. 0 passed; 1 failed" in output
            receipt = dict(mutation=name, head=head, baseline=baselines[package, test], command=command, test=test, compiled=compiled, exit=result.returncode, killed=killed, original_sha256=hashlib.sha256(original).hexdigest(), log=str(log))
        finally:
            path.write_bytes(original)
            if path.read_bytes() != original:
                raise SystemExit(f"Source restoration failed: {relative}")
        receipt["restored_sha256"] = hashlib.sha256(path.read_bytes()).hexdigest()
        receipts.append(receipt)
        (out / "receipts.json").write_text(json.dumps(receipts, indent=2) + "\n")
        print(json.dumps(receipt), flush=True)
        if not killed and not args.keep_going:
            raise SystemExit(f"Mutant survived or did not compile: {name}")
    if git("status", "--porcelain") or git("rev-parse", "HEAD") != head:
        raise SystemExit("Checkout changed during probes.")
    failures = [receipt["mutation"] for receipt in receipts if not receipt["killed"]]
    if failures:
        raise SystemExit(f"Mutants survived or did not compile: {failures}")
    print("All mutants compiled, failed their named test, and were restored.")


if __name__ == "__main__":
    main()
