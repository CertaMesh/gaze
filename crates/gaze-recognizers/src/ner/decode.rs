use std::ops::Range;

use gaze_types::{Detection, PiiClass};

use super::types::{LabelMap, NerSpanResult};

/// `document_text` is the string the `subword_spans` byte offsets index into
/// (the tokenizer input); `provenance` is the detector source id recorded on
/// each `Detection` (e.g. `"ner/ort"`). Never pass the provenance as the text:
/// joiner bridging and span validation read the document text.
pub(crate) fn merge_bio_spans(
    labels: &LabelMap,
    subword_spans: &[(usize, usize)],
    subword_labels: &[&str],
    document_text: &str,
    provenance: &str,
) -> Vec<Detection> {
    let scores = vec![1.0; subword_labels.len()];
    merge_bio_span_results(
        labels,
        subword_spans,
        subword_labels,
        &scores,
        document_text,
    )
    .into_iter()
    .map(|span| Detection::new(span.span, span.class, provenance))
    .collect()
}

/// `document_text` is the string the `subword_spans` byte offsets index into
/// (the tokenizer input, i.e. the chunk handed to the backend). Joiner
/// bridging reads the bytes between neighbouring tokens from it and span
/// validation slices it, so it must be the real text, not a provenance label.
pub(crate) fn merge_bio_span_results(
    labels: &LabelMap,
    subword_spans: &[(usize, usize)],
    subword_labels: &[&str],
    subword_scores: &[f32],
    document_text: &str,
) -> Vec<NerSpanResult> {
    let mut out = Vec::new();
    let (effective_labels, effective_scores) =
        bridge_joiner_tokens(document_text, subword_spans, subword_labels, subword_scores);
    let mut i = 0usize;
    while i < effective_labels.len() {
        let tag = effective_labels[i].as_str();
        let (prefix, entity) = split_bio(tag);
        if prefix == 'O' || entity.is_empty() {
            i += 1;
            continue;
        }
        let Some(class) = labels.resolve(tag, entity) else {
            i += 1;
            continue;
        };
        let (start, mut end) = subword_spans[i];
        if start == end {
            i += 1;
            continue;
        }
        let mut span_score = *effective_scores.get(i).unwrap_or(&0.0);
        let mut j = i + 1;
        while j < effective_labels.len() {
            let (p2, e2) = split_bio(effective_labels[j].as_str());
            if p2 == 'I' && e2 == entity {
                let (s, e) = subword_spans[j];
                if s != e {
                    end = e;
                    span_score = span_score.min(*effective_scores.get(j).unwrap_or(&0.0));
                }
                j += 1;
            } else {
                break;
            }
        }
        if is_valid_entity_span(document_text, &(start..end), class) {
            out.push(NerSpanResult {
                span: start..end,
                class: class.clone(),
                score: span_score,
            });
        }
        i = j;
    }
    out
}

/// Merge-time span validity against `document_text`. Deliberately does NOT
/// suppress spans that sit inside a larger identifier (`Artist` in
/// `Artistfy`): that suppression was never active in production and turning
/// it on is a measured recall/precision decision (solo todo #2904), not a
/// side effect of passing the right text.
pub(crate) fn is_valid_entity_span(
    document_text: &str,
    span: &Range<usize>,
    class: &PiiClass,
) -> bool {
    let start = span.start;
    let end = span.end;
    if is_suppressed_single_token_organization(document_text, start, end, class) {
        return false;
    }
    if !matches!(class, PiiClass::Name | PiiClass::Organization) {
        return true;
    }
    let Some(text) = document_text.get(span.clone()) else {
        return true;
    };
    !is_command_argv_identifier_span(text)
}

fn is_suppressed_single_token_organization(
    document_text: &str,
    start: usize,
    end: usize,
    class: &PiiClass,
) -> bool {
    if class != &PiiClass::Organization {
        return false;
    }
    let Some(text) = document_text.get(start..end) else {
        return false;
    };
    if text.chars().any(char::is_whitespace) || !text.eq_ignore_ascii_case("workspace") {
        return false;
    }
    document_text
        .get(..start)
        .is_some_and(|before| before.ends_with("~/") || before.ends_with("/"))
}

fn is_command_argv_identifier_span(text: &str) -> bool {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return false;
    }
    if trimmed == "~" || is_cli_flag(trimmed) || is_apple_script_literal(trimmed) {
        return true;
    }
    if trimmed.starts_with("osascript ") && trimmed.contains("tell application") {
        return true;
    }
    let mut parts = trimmed.split_ascii_whitespace().peekable();
    if parts.peek().is_some() {
        return parts.all(|part| {
            let token = part.trim_matches(|ch| matches!(ch, '\'' | '"'));
            token == "~" || is_cli_flag(token) || is_program_identifier_token(token)
        });
    }
    false
}

fn is_cli_flag(token: &str) -> bool {
    token
        .strip_prefix('-')
        .filter(|rest| !rest.is_empty())
        .is_some_and(|rest| {
            rest.chars()
                .all(|ch| ch.is_ascii_alphanumeric() || ch == '-')
        })
}

// KNOWN DEBT (stop-gap): this literal command/argv allowlist is a per-symptom
// suppressor, not the real fix. It does not generalize (grep/awk/git/jq/sed still
// over-redact) and each enumerated token is a latent under-redaction surface (a real
// PII string colliding with a command word is silently suppressed). The structural
// fix is an NER span provenance trust-tier + corroboration gate (reuse anchored_match
// + locale cues) plus an idempotence xtask invariant and a code/argv distractor
// corpus. See gaze todo #1024 and scratchpad analysis/gaze-overredaction-architecture.
// Do not grow this list reflexively; add a corpus row and let the structural gate handle it.
fn is_program_identifier_token(token: &str) -> bool {
    if token.is_empty() || !token.chars().all(|ch| ch.is_ascii_alphanumeric()) {
        return false;
    }
    matches!(
        token,
        "cal" | "eventsToday" | "icalBuddy" | "ls" | "osascript"
    ) || token.ends_with("Script")
}

fn is_apple_script_literal(text: &str) -> bool {
    text.starts_with("tell application ") && text.contains(" to ")
}

fn bridge_joiner_tokens(
    document_text: &str,
    subword_spans: &[(usize, usize)],
    subword_labels: &[&str],
    subword_scores: &[f32],
) -> (Vec<String>, Vec<f32>) {
    let mut effective_labels = subword_labels
        .iter()
        .map(|label| (*label).to_string())
        .collect::<Vec<_>>();
    let mut effective_scores = (0..subword_labels.len())
        .map(|index| *subword_scores.get(index).unwrap_or(&0.0))
        .collect::<Vec<_>>();

    for index in 1..subword_labels.len().saturating_sub(1) {
        let (prefix, _) = split_bio(subword_labels[index]);
        if prefix != 'O' {
            continue;
        }

        let (start, end) = subword_spans[index];
        let Some(token_text) = document_text.get(start..end) else {
            continue;
        };
        if !is_entity_joiner_token(token_text) {
            continue;
        }

        let (prev_prefix, prev_entity) = split_bio(subword_labels[index - 1]);
        if !matches!(prev_prefix, 'B' | 'I') || prev_entity.is_empty() {
            continue;
        }

        let (next_prefix, next_entity) = split_bio(subword_labels[index + 1]);
        if !matches!(next_prefix, 'B' | 'I') || next_entity != prev_entity {
            continue;
        }
        if next_prefix == 'B' && token_text.trim() == "," {
            continue;
        }

        effective_labels[index] = format!("I-{prev_entity}");
        if next_prefix == 'B' {
            effective_labels[index + 1] = format!("I-{prev_entity}");
        }
        let prev_score = *subword_scores.get(index - 1).unwrap_or(&0.0);
        let next_score = *subword_scores.get(index + 1).unwrap_or(&0.0);
        effective_scores[index] = (prev_score + next_score) / 2.0;
    }

    (effective_labels, effective_scores)
}

fn is_entity_joiner_token(text: &str) -> bool {
    // Mirrors Kiji `isEntityJoinerToken` in onnx_model_detector.go:410-421.
    let trimmed = text.trim();
    !trimmed.is_empty() && trimmed.chars().all(|ch| ".,@_-+:/#%&=".contains(ch))
}

pub(crate) fn softmax_confidence(row: &[f32], index: usize) -> f32 {
    let max = row.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    let denom = row.iter().map(|value| (*value - max).exp()).sum::<f32>();
    if denom == 0.0 {
        return 0.0;
    }
    row.get(index)
        .map(|value| (*value - max).exp() / denom)
        .unwrap_or(0.0)
}

/// `B-PER` -> ('B', "PER"); `O` -> ('O', ""); `PER` (no prefix) -> ('B', "PER").
pub(crate) fn split_bio(tag: &str) -> (char, &str) {
    if tag == "O" || tag.is_empty() {
        return ('O', "");
    }
    if let Some(rest) = tag.strip_prefix("B-") {
        return ('B', rest);
    }
    if let Some(rest) = tag.strip_prefix("I-") {
        return ('I', rest);
    }
    ('B', tag)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use gaze_types::PiiClass;

    use super::*;

    fn labels(entries: &[(&str, PiiClass)]) -> LabelMap {
        LabelMap(
            entries
                .iter()
                .map(|(label, class)| ((*label).to_string(), class.clone()))
                .collect::<BTreeMap<_, _>>(),
        )
    }

    fn token_spans(document_text: &str, tokens: &[&str]) -> Vec<(usize, usize)> {
        let mut cursor = 0usize;
        tokens
            .iter()
            .map(|token| {
                let rest = &document_text[cursor..];
                let offset = rest.find(token).expect("token exists after cursor");
                let start = cursor + offset;
                let end = start + token.len();
                cursor = end;
                (start, end)
            })
            .collect()
    }

    fn merge(
        document_text: &str,
        tokens: &[&str],
        tags: &[&str],
        label_map: &LabelMap,
    ) -> Vec<NerSpanResult> {
        let spans = token_spans(document_text, tokens);
        let scores = vec![0.9; tags.len()];
        merge_bio_span_results(label_map, &spans, tags, &scores, document_text)
    }

    #[test]
    fn bridges_email() {
        let source = "john.doe@example.invalid";
        let label_map = labels(&[("PER", PiiClass::Name), ("ORG", PiiClass::Organization)]);
        let out = merge(
            source,
            &["john", ".", "doe", "@", "example.invalid"],
            &["B-PER", "O", "I-PER", "O", "B-ORG"],
            &label_map,
        );

        assert_eq!(out.len(), 2);
        assert_eq!(out[0].span, 0..8);
        assert_eq!(out[0].class, PiiClass::Name);
        assert_eq!(out[1].span, 9..24);
        assert_eq!(out[1].class, PiiClass::Organization);
    }

    #[test]
    fn bridges_same_label_b_fragment() {
        let source = "alice@example.invalid";
        let label_map = labels(&[("EMAIL", PiiClass::Email)]);
        let out = merge(
            source,
            &["alice", "@", "example.invalid"],
            &["B-EMAIL", "O", "B-EMAIL"],
            &label_map,
        );

        assert_eq!(out.len(), 1);
        assert_eq!(out[0].span, 0..source.len());
        assert_eq!(out[0].class, PiiClass::Email);
    }

    #[test]
    fn bridges_url() {
        let source = "https://example.invalid/path";
        let label_map = labels(&[("URL", PiiClass::custom("url"))]);
        let out = merge(
            source,
            &["https", "://", "example.invalid", "/", "path"],
            &["B-URL", "O", "I-URL", "O", "I-URL"],
            &label_map,
        );

        assert_eq!(out.len(), 1);
        assert_eq!(out[0].span, 0..source.len());
        assert_eq!(out[0].class, PiiClass::custom("url"));
    }

    #[test]
    fn bridges_phone() {
        let source = "+49-1555-0112233";
        let label_map = labels(&[("PHONE", PiiClass::custom("phone"))]);
        let out = merge(
            source,
            &["+49", "-", "1555", "-", "0112233"],
            &["B-PHONE", "O", "I-PHONE", "O", "I-PHONE"],
            &label_map,
        );

        assert_eq!(out.len(), 1);
        assert_eq!(out[0].span, 0..source.len());
        assert_eq!(out[0].class, PiiClass::custom("phone"));
    }

    #[test]
    fn does_not_bridge_at_eof() {
        let source = "john.";
        let label_map = labels(&[("PER", PiiClass::Name)]);
        let out = merge(source, &["john", "."], &["B-PER", "O"], &label_map);

        assert_eq!(out.len(), 1);
        assert_eq!(out[0].span, 0..4);
    }

    #[test]
    fn does_not_bridge_at_bof() {
        let source = ".john";
        let label_map = labels(&[("PER", PiiClass::Name)]);
        let out = merge(source, &[".", "john"], &["O", "B-PER"], &label_map);

        assert_eq!(out.len(), 1);
        assert_eq!(out[0].span, 1..5);
    }

    #[test]
    fn does_not_bridge_across_labels() {
        let source = "john,example.invalid";
        let label_map = labels(&[("PER", PiiClass::Name), ("ORG", PiiClass::Organization)]);
        let out = merge(
            source,
            &["john", ",", "example.invalid"],
            &["B-PER", "O", "B-ORG"],
            &label_map,
        );

        assert_eq!(out.len(), 2);
        assert_eq!(out[0].span, 0..4);
        assert_eq!(out[1].span, 5..20);
    }

    #[test]
    fn does_not_bridge_two_same_label_b_tags() {
        let source = "john,bob";
        let label_map = labels(&[("PER", PiiClass::Name)]);
        let out = merge(
            source,
            &["john", ",", "bob"],
            &["B-PER", "O", "B-PER"],
            &label_map,
        );

        assert_eq!(out.len(), 2);
        assert_eq!(out[0].span, 0..4);
        assert_eq!(out[1].span, 5..8);
    }

    /// Guard against silent activation: partial-identifier spans were never
    /// suppressed in production (the old check compared against the
    /// provenance label, not the text). Activating suppression is a measured
    /// decision tracked in solo todo #2904, so the merge must keep the span.
    #[test]
    fn does_not_suppress_partial_identifier_span_at_merge() {
        let source = "Artistfy";
        let label_map = labels(&[("ORG", PiiClass::Organization)]);
        let out = merge(source, &["Artist"], &["B-ORG"], &label_map);

        assert_eq!(
            out.len(),
            1,
            "partial identifier span must survive: {out:?}"
        );
        assert_eq!(out[0].span, 0..6);
    }

    #[test]
    fn suppresses_single_token_workspace_organization() {
        let source = "list all folders in ~/Workspace";
        let label_map = labels(&[("ORG", PiiClass::Organization)]);
        let out = merge(source, &["Workspace"], &["B-ORG"], &label_map);

        assert!(out.is_empty(), "unexpected Workspace org span: {out:?}");
    }

    #[test]
    fn keeps_multi_token_organization_recall() {
        let source = "Acme Corp";
        let label_map = labels(&[("ORG", PiiClass::Organization)]);
        let out = merge(source, &["Acme", "Corp"], &["B-ORG", "I-ORG"], &label_map);

        assert_eq!(out.len(), 1);
        assert_eq!(out[0].span, 0..source.len());
        assert_eq!(out[0].class, PiiClass::Organization);
    }
}
