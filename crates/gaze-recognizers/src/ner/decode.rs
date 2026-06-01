use gaze_types::Detection;

use super::types::{LabelMap, NerSpanResult};

pub(crate) fn merge_bio_spans(
    labels: &LabelMap,
    subword_spans: &[(usize, usize)],
    subword_labels: &[&str],
    source: &str,
) -> Vec<Detection> {
    let scores = vec![1.0; subword_labels.len()];
    merge_bio_span_results(labels, subword_spans, subword_labels, &scores, source)
        .into_iter()
        .map(|span| Detection::new(span.span, span.class, source))
        .collect()
}

pub(crate) fn merge_bio_span_results(
    labels: &LabelMap,
    subword_spans: &[(usize, usize)],
    subword_labels: &[&str],
    subword_scores: &[f32],
    source: &str,
) -> Vec<NerSpanResult> {
    let mut out = Vec::new();
    let (effective_labels, effective_scores) =
        bridge_joiner_tokens(source, subword_spans, subword_labels, subword_scores);
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
        out.push(NerSpanResult {
            span: start..end,
            class: class.clone(),
            score: span_score,
        });
        i = j;
    }
    out
}

fn bridge_joiner_tokens(
    source: &str,
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
        let Some(token_text) = source.get(start..end) else {
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

    fn token_spans(source: &str, tokens: &[&str]) -> Vec<(usize, usize)> {
        let mut cursor = 0usize;
        tokens
            .iter()
            .map(|token| {
                let rest = &source[cursor..];
                let offset = rest.find(token).expect("token exists after cursor");
                let start = cursor + offset;
                let end = start + token.len();
                cursor = end;
                (start, end)
            })
            .collect()
    }

    fn merge(
        source: &str,
        tokens: &[&str],
        tags: &[&str],
        label_map: &LabelMap,
    ) -> Vec<NerSpanResult> {
        let spans = token_spans(source, tokens);
        let scores = vec![0.9; tags.len()];
        merge_bio_span_results(label_map, &spans, tags, &scores, source)
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

    #[test]
    fn drops_entity_span_inside_identifier() {
        let source = "Artistfy";
        let label_map = labels(&[("ORG", PiiClass::Organization)]);
        let out = merge(source, &["Artist"], &["B-ORG"], &label_map);

        assert!(out.is_empty(), "unexpected partial identifier span: {out:?}");
    }
}
