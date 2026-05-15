use super::RawSpan;
use crate::ner::decode::{softmax_confidence, split_bio};

pub(crate) const ID2LABEL: [&str; 9] = [
    "O", "B-PER", "I-PER", "B-LOC", "I-LOC", "B-ORG", "I-ORG", "B-MISC", "I-MISC",
];

pub(crate) fn decode_logits(
    clean: &str,
    offsets: &[(usize, usize)],
    flat: &[f32],
    seq_len: usize,
    num_labels: usize,
) -> Vec<RawSpan> {
    let mut subword_labels: Vec<&str> = Vec::with_capacity(seq_len);
    let mut subword_scores = Vec::with_capacity(seq_len);
    for pos in 0..seq_len {
        let base = pos * num_labels;
        let row = &flat[base..base + num_labels];
        let (argmax, _) =
            row.iter()
                .enumerate()
                .fold((0usize, f32::NEG_INFINITY), |acc, (index, &value)| {
                    if value > acc.1 {
                        (index, value)
                    } else {
                        acc
                    }
                });
        subword_labels.push(ID2LABEL.get(argmax).copied().unwrap_or("O"));
        subword_scores.push(softmax_confidence(row, argmax));
    }
    merge_kiji_bio_spans(clean, offsets, &subword_labels, &subword_scores)
}

fn merge_kiji_bio_spans(
    source: &str,
    subword_spans: &[(usize, usize)],
    subword_labels: &[&str],
    subword_scores: &[f32],
) -> Vec<RawSpan> {
    let (effective_labels, effective_scores) =
        bridge_joiner_tokens(source, subword_spans, subword_labels, subword_scores);
    let mut out = Vec::new();
    let mut index = 0usize;
    while index < effective_labels.len() {
        let tag = effective_labels[index].as_str();
        let (prefix, entity) = split_bio(tag);
        if prefix == 'O' || entity.is_empty() {
            index += 1;
            continue;
        }
        let Some(label) = kiji_entity_label(entity) else {
            index += 1;
            continue;
        };
        let (start, mut end) = subword_spans[index];
        if start == end {
            index += 1;
            continue;
        }
        let mut span_score = *effective_scores.get(index).unwrap_or(&0.0);
        let mut next = index + 1;
        while next < effective_labels.len() {
            let (next_prefix, next_entity) = split_bio(effective_labels[next].as_str());
            if next_prefix == 'I' && next_entity == entity {
                let (next_start, next_end) = subword_spans[next];
                if next_start != next_end {
                    end = next_end;
                    span_score = span_score.min(*effective_scores.get(next).unwrap_or(&0.0));
                }
                next += 1;
            } else {
                break;
            }
        }
        out.push(RawSpan::new(start, end, label, Some(span_score)));
        index = next;
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
    let trimmed = text.trim();
    !trimmed.is_empty() && trimmed.chars().all(|ch| ".,@_-+:/#%&=".contains(ch))
}

fn kiji_entity_label(entity: &str) -> Option<&'static str> {
    match entity {
        "PER" => Some("person"),
        "LOC" => Some("location"),
        "ORG" => Some("organization"),
        "MISC" => Some("miscellaneous"),
        _ => None,
    }
}
