use super::RawSpan;
use crate::ner::decode::{softmax_confidence, split_bio};
use gaze_types::SafetyNetError;

pub(crate) const ID2LABEL: [&str; 9] = [
    "O", "B-PER", "I-PER", "B-ORG", "I-ORG", "B-LOC", "I-LOC", "B-MISC", "I-MISC",
];
const PINNED_LABEL_COUNT: usize = ID2LABEL.len();

pub(crate) fn decode_logits(
    clean: &str,
    offsets: &[(usize, usize)],
    flat: &[f32],
    seq_len: usize,
    num_labels: usize,
) -> Result<Vec<RawSpan>, SafetyNetError> {
    if num_labels != PINNED_LABEL_COUNT {
        return Err(invalid_output("kiji returned invalid classifier width"));
    }
    if seq_len != offsets.len() {
        return Err(invalid_output("kiji returned mismatched token offsets"));
    }
    let expected_len = expected_logit_len(seq_len)?;
    if flat.len() != expected_len {
        return Err(invalid_output("kiji returned invalid logits length"));
    }
    if flat.iter().any(|value| !value.is_finite()) {
        return Err(invalid_output("kiji returned non-finite logits"));
    }

    let mut subword_labels: Vec<&str> = Vec::with_capacity(seq_len);
    let mut subword_scores = Vec::with_capacity(seq_len);
    for pos in 0..seq_len {
        let base = pos * num_labels;
        let row = &flat[base..base + num_labels];
        let (label, score) = label_for_row(row)?;
        subword_labels.push(label);
        subword_scores.push(score);
    }
    Ok(merge_kiji_bio_spans(
        clean,
        offsets,
        &subword_labels,
        &subword_scores,
    ))
}

fn label_for_row(row: &[f32]) -> Result<(&'static str, f32), SafetyNetError> {
    if row.len() != PINNED_LABEL_COUNT {
        return Err(invalid_output("kiji returned invalid classifier row"));
    }
    if row.iter().any(|value| !value.is_finite()) {
        return Err(invalid_output("kiji returned non-finite logits"));
    }
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
    let label = ID2LABEL
        .get(argmax)
        .copied()
        .ok_or_else(|| SafetyNetError::InvalidOutput {
            message: "kiji returned unknown classifier label".to_string(),
        })?;
    Ok((label, softmax_confidence(row, argmax)))
}

fn expected_logit_len(seq_len: usize) -> Result<usize, SafetyNetError> {
    seq_len
        .checked_mul(PINNED_LABEL_COUNT)
        .ok_or_else(|| invalid_output("kiji returned overflowing logits shape"))
}

fn invalid_output(message: &str) -> SafetyNetError {
    SafetyNetError::InvalidOutput {
        message: message.to_string(),
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    const EXPECTED_LABELS: [&str; 9] = [
        "O", "B-PER", "I-PER", "B-ORG", "I-ORG", "B-LOC", "I-LOC", "B-MISC", "I-MISC",
    ];

    #[test]
    fn maps_every_pinned_classifier_id() {
        assert_eq!(ID2LABEL, EXPECTED_LABELS);
        for (expected_id, expected_label) in EXPECTED_LABELS.iter().enumerate() {
            let mut row = [0.0; 9];
            row[expected_id] = 1.0;
            let (actual_label, _) = label_for_row(&row).unwrap();
            assert_eq!(actual_label, *expected_label, "classifier id {expected_id}");
        }
    }

    #[test]
    fn keeps_organization_and_location_ids_distinct() {
        assert_eq!(ID2LABEL[3], "B-ORG");
        assert_eq!(ID2LABEL[4], "I-ORG");
        assert_eq!(ID2LABEL[5], "B-LOC");
        assert_eq!(ID2LABEL[6], "I-LOC");
    }

    #[test]
    fn preserves_bio_merge_and_joiner_behavior() {
        let source = "Dr. Schmidt visits Berlin";
        let spans = [(0, 2), (2, 3), (4, 11), (12, 18), (19, 25)];
        let labels = ["B-PER", "O", "B-PER", "O", "B-LOC"];
        let scores = [0.9, 0.1, 0.8, 0.1, 0.7];
        let out = merge_kiji_bio_spans(source, &spans, &labels, &scores);
        assert_eq!(
            out,
            vec![
                RawSpan::new(0, 11, "person", Some(0.8)),
                RawSpan::new(19, 25, "location", Some(0.7)),
            ]
        );
    }

    #[test]
    fn rejects_wrong_classifier_width() {
        let error = decode_logits("", &[], &[], 0, 8).unwrap_err();
        assert!(matches!(error, SafetyNetError::InvalidOutput { .. }));
    }

    #[test]
    fn rejects_offset_sequence_mismatch() {
        let error = decode_logits("x", &[], &[0.0; 9], 1, 9).unwrap_err();
        assert!(matches!(error, SafetyNetError::InvalidOutput { .. }));
    }

    #[test]
    fn rejects_flat_length_mismatch() {
        let error = decode_logits("", &[(0, 0)], &[0.0; 8], 1, 9).unwrap_err();
        assert!(matches!(error, SafetyNetError::InvalidOutput { .. }));
    }

    #[test]
    fn rejects_overflowing_shape() {
        let error = expected_logit_len(usize::MAX).unwrap_err();
        assert!(matches!(error, SafetyNetError::InvalidOutput { .. }));
    }

    #[test]
    fn rejects_non_finite_logits() {
        let mut logits = [0.0; 9];
        logits[4] = f32::NAN;
        let error = decode_logits("", &[(0, 0)], &logits, 1, 9).unwrap_err();
        assert!(matches!(error, SafetyNetError::InvalidOutput { .. }));
    }

    #[test]
    fn accepts_o_for_zero_length_special_token() {
        let spans = decode_logits(
            "",
            &[(0, 0)],
            &[1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
            1,
            9,
        )
        .unwrap();
        assert!(spans.is_empty());
    }
}

/// Label-registry parity harness.
///
/// The Kiji bundle ships no `config.json` / `id2label` artifact (only
/// `SHA256SUMS`, `labels.json` (vocabulary, no ids), `model.onnx`,
/// `tokenizer.json`), so the classifier-id order cannot be verified at bundle
/// load. It is pinned here against the upstream model card instead:
///
/// * Source: `https://huggingface.co/onnx-community/distilbert-NER-ONNX/raw/3a19fe9404a4469d91aa3d551558a97f68872f67/config.json`
/// * SHA-256 of that `config.json`: `f109facddb205dac712adf5877e4315fae62041bc0916fe808d92abdb594d1fe`
/// * `id2label`: `{0:O, 1:B-PER, 2:I-PER, 3:B-ORG, 4:I-ORG, 5:B-LOC, 6:I-LOC, 7:B-MISC, 8:I-MISC}`
///
/// Before the decoders were unified, the private ORT decoder on `main`
/// (`backend/ort.rs` @ 963773c) and this file both carried a permuted table
/// with `B-LOC,I-LOC` at ids 3–4 and `B-ORG,I-ORG` at ids 5–6, so every real
/// location was emitted as `organization` and vice versa. That table is kept
/// below as `MAIN_PERMUTED_ID2LABEL` and the shared decoder must NOT reproduce
/// it.
#[cfg(test)]
mod label_registry_parity {
    use super::{decode_logits, RawSpan, ID2LABEL};
    use crate::ner::decode::{softmax_confidence, split_bio};

    /// Upstream `id2label` from the pinned model card (see module docs).
    const UPSTREAM_ID2LABEL: [&str; 9] = [
        "O", "B-PER", "I-PER", "B-ORG", "I-ORG", "B-LOC", "I-LOC", "B-MISC", "I-MISC",
    ];

    /// The known-wrong permutation shipped by `main` before unification
    /// (`backend/ort.rs` @ 963773c): ids 3–6 swapped LOC and ORG.
    const MAIN_PERMUTED_ID2LABEL: [&str; 9] = [
        "O", "B-PER", "I-PER", "B-LOC", "I-LOC", "B-ORG", "I-ORG", "B-MISC", "I-MISC",
    ];

    // ---- BEGIN reference decoder ----
    // Verbatim copy of the pre-unification ORT decode logic from
    // crates/gaze-recognizers/src/safety_net/kiji_distilbert/backend/ort.rs @ 963773c,
    // with the only change that the label table is passed in instead of being a
    // module constant, so the same reference can be driven by either table.
    fn reference_decode(
        id2label: &[&'static str; 9],
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
            subword_labels.push(id2label.get(argmax).copied().unwrap_or("O"));
            subword_scores.push(softmax_confidence(row, argmax));
        }
        reference_merge_kiji_bio_spans(clean, offsets, &subword_labels, &subword_scores)
    }

    fn reference_merge_kiji_bio_spans(
        source: &str,
        subword_spans: &[(usize, usize)],
        subword_labels: &[&str],
        subword_scores: &[f32],
    ) -> Vec<RawSpan> {
        let (effective_labels, effective_scores) =
            reference_bridge_joiner_tokens(source, subword_spans, subword_labels, subword_scores);
        let mut out = Vec::new();
        let mut index = 0usize;
        while index < effective_labels.len() {
            let tag = effective_labels[index].as_str();
            let (prefix, entity) = split_bio(tag);
            if prefix == 'O' || entity.is_empty() {
                index += 1;
                continue;
            }
            let Some(label) = reference_kiji_entity_label(entity) else {
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

    fn reference_bridge_joiner_tokens(
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
            if !reference_is_entity_joiner_token(token_text) {
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

    fn reference_is_entity_joiner_token(text: &str) -> bool {
        let trimmed = text.trim();
        !trimmed.is_empty() && trimmed.chars().all(|ch| ".,@_-+:/#%&=".contains(ch))
    }

    fn reference_kiji_entity_label(entity: &str) -> Option<&'static str> {
        match entity {
            "PER" => Some("person"),
            "LOC" => Some("location"),
            "ORG" => Some("organization"),
            "MISC" => Some("miscellaneous"),
            _ => None,
        }
    }
    // ---- END reference decoder ----

    const NUM_LABELS: usize = 9;

    /// One synthetic classifier row: `peak` logit at `label_id`, zero elsewhere.
    fn row(label_id: usize, peak: f32) -> [f32; NUM_LABELS] {
        let mut out = [0.0; NUM_LABELS];
        out[label_id] = peak;
        out
    }

    /// Fixed synthetic sequence exercising every classifier id 0..8 plus the
    /// merge edge cases: multi-subtoken entity (ids 3,4), adjacent entity of a
    /// different class (ids 5,6), `I-` without `B-` (ids 8 and 2), a low-score
    /// span (id 7 at a small peak), a plain `O`, and zero-width specials.
    fn synthetic_sequence() -> (&'static str, Vec<(usize, usize)>, Vec<f32>) {
        let clean = "Ann Berlin Siemens Cup Foo Bar Baz Qux";
        let tokens: [((usize, usize), usize, f32); 12] = [
            ((0, 0), 0, 6.0),   // [CLS] -> O
            ((0, 3), 1, 6.0),   // Ann   -> id 1 (B-PER)
            ((4, 7), 3, 6.0),   // Ber   -> id 3 (multi-subtoken start)
            ((7, 10), 4, 5.0),  // lin   -> id 4 (continuation)
            ((11, 14), 5, 6.0), // Sie   -> id 5 (adjacent, other class)
            ((14, 18), 6, 5.0), // mens  -> id 6 (continuation)
            ((19, 22), 8, 6.0), // Cup   -> id 8 (I- without B-)
            ((23, 26), 7, 0.5), // Foo   -> id 7 (low-score span)
            ((27, 30), 2, 6.0), // Bar   -> id 2 (I- without B-, class change)
            ((31, 34), 0, 6.0), // Baz   -> id 0 (O)
            ((35, 38), 1, 6.0), // Qux   -> id 1
            ((38, 38), 0, 6.0), // [SEP] -> O
        ];
        let offsets = tokens.iter().map(|(span, _, _)| *span).collect::<Vec<_>>();
        let flat = tokens
            .iter()
            .flat_map(|(_, id, peak)| row(*id, *peak))
            .collect::<Vec<_>>();
        (clean, offsets, flat)
    }

    fn describe(spans: &[RawSpan]) -> String {
        spans
            .iter()
            .map(|span| {
                format!(
                    "({},{},{},{:.4})",
                    span.start,
                    span.end,
                    span.label,
                    span.score.unwrap_or(f32::NAN)
                )
            })
            .collect::<Vec<_>>()
            .join(" ")
    }

    #[test]
    fn pinned_registry_matches_upstream_id2label() {
        assert_eq!(ID2LABEL, UPSTREAM_ID2LABEL);
        assert_ne!(ID2LABEL, MAIN_PERMUTED_ID2LABEL);
    }

    #[test]
    fn shared_decoder_matches_upstream_reference_on_synthetic_logits() {
        let (clean, offsets, flat) = synthetic_sequence();
        let seq_len = offsets.len();

        let reference = reference_decode(
            &UPSTREAM_ID2LABEL,
            clean,
            &offsets,
            &flat,
            seq_len,
            NUM_LABELS,
        );
        let shared = decode_logits(clean, &offsets, &flat, seq_len, NUM_LABELS)
            .expect("well-formed synthetic logits must decode");

        assert_eq!(
            shared,
            reference,
            "\nupstream id2label reference: {}\nbranch shared decoder:       {}",
            describe(&reference),
            describe(&shared),
        );
        // Every label id 0..8 produced the expected class at the expected span.
        assert_eq!(
            describe(&shared),
            "(0,3,person,0.9806) (4,10,organization,0.9489) (11,18,location,0.9489) \
             (19,22,miscellaneous,0.9806) (23,26,miscellaneous,0.1709) (27,30,person,0.9806) \
             (35,38,person,0.9806)"
        );
    }

    #[test]
    fn shared_decoder_does_not_reproduce_main_loc_org_permutation() {
        let (clean, offsets, flat) = synthetic_sequence();
        let seq_len = offsets.len();

        let permuted = reference_decode(
            &MAIN_PERMUTED_ID2LABEL,
            clean,
            &offsets,
            &flat,
            seq_len,
            NUM_LABELS,
        );
        let shared = decode_logits(clean, &offsets, &flat, seq_len, NUM_LABELS)
            .expect("well-formed synthetic logits must decode");

        // Same span geometry and scores: the divergence is label-only.
        assert_eq!(shared.len(), permuted.len());
        for (ours, theirs) in shared.iter().zip(&permuted) {
            assert_eq!((ours.start, ours.end), (theirs.start, theirs.end));
            assert_eq!(ours.score, theirs.score);
        }
        // Exactly the id 3–6 spans differ, and only as a LOC<->ORG swap.
        let differing = shared
            .iter()
            .zip(&permuted)
            .filter(|(ours, theirs)| ours.label != theirs.label)
            .map(|(ours, theirs)| {
                (
                    ours.start,
                    ours.end,
                    ours.label.as_str(),
                    theirs.label.as_str(),
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(
            differing,
            vec![
                (4, 10, "organization", "location"),
                (11, 18, "location", "organization"),
            ],
            "\nmain@963773c permuted reference: {}\nbranch shared decoder:           {}",
            describe(&permuted),
            describe(&shared),
        );
    }
}
