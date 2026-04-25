use gaze::Detection;

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
        .map(|span| Detection {
            span: span.span,
            class: span.class,
            source: source.to_string(),
        })
        .collect()
}

pub(crate) fn merge_bio_span_results(
    labels: &LabelMap,
    subword_spans: &[(usize, usize)],
    subword_labels: &[&str],
    subword_scores: &[f32],
    _source: &str,
) -> Vec<NerSpanResult> {
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < subword_labels.len() {
        let tag = subword_labels[i];
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
        let mut span_score = *subword_scores.get(i).unwrap_or(&0.0);
        let mut j = i + 1;
        while j < subword_labels.len() {
            let (p2, e2) = split_bio(subword_labels[j]);
            if p2 == 'I' && e2 == entity {
                let (s, e) = subword_spans[j];
                if s != e {
                    end = e;
                    span_score = span_score.min(*subword_scores.get(j).unwrap_or(&0.0));
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
