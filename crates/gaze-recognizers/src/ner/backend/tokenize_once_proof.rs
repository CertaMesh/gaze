//! Model-input and pipeline parity against the frozen per-chunk encoding path.

use super::*;
use gaze_types::Detector;
use serde_json::{json, Value};
use std::path::PathBuf;

fn model_dir() -> PathBuf {
    std::env::var_os("GAZE_NER_MODEL_DIR")
        .map(PathBuf::from)
        .expect("set GAZE_NER_MODEL_DIR to the pinned Davlan bundle")
}

#[path = "../../../tests/support/ner_preparation_fixtures.rs"]
mod fixture_set;
use fixture_set::fixtures;

fn dump_window(range: &Range<usize>, encoding: &tokenizers::Encoding) -> Value {
    json!({
        "byte_range": [range.start, range.end],
        "input_ids": encoding.get_ids().iter().map(|&id| i64::from(id)).collect::<Vec<_>>(),
        "attention_mask": encoding.get_attention_mask().iter().map(|&id| i64::from(id)).collect::<Vec<_>>(),
        // Production OrtBackend supplies zeros, independent of Encoding's type IDs.
        "token_type_ids": vec![0i64; encoding.len()],
        "special_tokens_mask": encoding.get_special_tokens_mask(),
        "offsets": encoding.get_offsets(),
        "tokens": encoding.get_tokens(),
    })
}

#[test]
#[ignore = "manual gate: requires the pinned Davlan model bundle"]
fn tokenize_once_proof_actual_encode_counts() {
    let detector = NerDetector::load(&model_dir()).unwrap();
    let tokenizer = tokenizers::Tokenizer::from_file(model_dir().join(TOKENIZER_FILE)).unwrap();
    let mut rows = Vec::new();
    for (name, input) in fixtures().into_iter().take(5) {
        let chunks = super::tests::original_chunk_ranges(&tokenizer, &input)
            .unwrap()
            .len();
        ENCODE_CALLS.with(|calls| calls.set(0));
        detector.try_detect(&input).unwrap();
        let encodes = ENCODE_CALLS.with(|calls| calls.get());
        println!(
            "{name}: bytes={} windows={chunks} encodes={encodes}",
            input.len()
        );
        assert_eq!(encodes, if chunks == 1 { 1 } else { 1 + chunks });
        rows.push(
            json!({"fixture": name, "bytes": input.len(), "windows": chunks, "encodes": encodes}),
        );
    }
    std::fs::write(
        proof_path("encode-counts.json"),
        serde_json::to_vec_pretty(&rows).unwrap(),
    )
    .unwrap();
}

#[test]
#[ignore = "manual gate: requires the pinned Davlan tokenizer; any mismatch stops the refactor"]
fn tokenize_once_proof_tensor_equality() {
    let tokenizer = tokenizers::Tokenizer::from_file(model_dir().join(TOKENIZER_FILE)).unwrap();
    assert!(tokenizer.get_truncation().is_none());
    let mut documents = Vec::new();
    let mut mismatches = Vec::new();
    for (name, input) in fixtures() {
        let ranges = super::tests::original_chunk_ranges(&tokenizer, &input).unwrap();
        ENCODE_CALLS.with(|calls| calls.set(0));
        let baseline: Vec<_> = ranges
            .iter()
            .map(|range| {
                dump_window(
                    range,
                    &encode_input(&tokenizer, &input[range.clone()]).unwrap(),
                )
            })
            .collect();
        let baseline_encodes = ENCODE_CALLS.with(|calls| calls.get()) + 1;

        ENCODE_CALLS.with(|calls| calls.set(0));
        let candidate: Vec<_> = prepare_windows(&tokenizer, &input)
            .unwrap()
            .map(|window| {
                let (range, encoded) = window.unwrap();
                dump_window(&range, &encoded)
            })
            .collect();
        let candidate_encodes = ENCODE_CALLS.with(|calls| calls.get());
        assert_eq!(
            candidate_encodes,
            if ranges.len() == 1 {
                1
            } else {
                ranges.len() + 1
            }
        );
        assert_eq!(baseline.len(), candidate.len());
        for (index, (base, head)) in baseline.iter().zip(&candidate).enumerate() {
            if base != head {
                let fields: Vec<_> = [
                    "input_ids",
                    "attention_mask",
                    "token_type_ids",
                    "special_tokens_mask",
                    "offsets",
                    "tokens",
                ]
                .into_iter()
                .filter(|field| base[field] != head[field])
                .collect();
                mismatches.push(json!({"fixture": name, "window": index, "fields": fields}));
            }
        }
        documents.push(
            json!({"fixture": name, "input": input, "baseline_encodes": baseline_encodes,
            "candidate_encodes": candidate_encodes, "baseline": baseline, "candidate": candidate}),
        );
    }
    let receipt = json!({"documents": documents, "mismatches": mismatches});
    std::fs::write(
        proof_path("tensor-proof.json"),
        serde_json::to_vec_pretty(&receipt).unwrap(),
    )
    .unwrap();
    assert!(mismatches.is_empty(), "model-input parity failed: {} mismatched windows; first={}; see GAZE_NER_PROOF_DIR/tensor-proof.json", mismatches.len(), mismatches.first().unwrap_or(&Value::Null));
}

fn proof_path(name: &str) -> PathBuf {
    let directory = std::env::var_os("GAZE_NER_PROOF_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("target/ner-prepare-proof"));
    std::fs::create_dir_all(&directory).unwrap();
    directory.join(name)
}

#[test]
fn preparation_preserves_wordpiece_windows_and_truncation() {
    use tokenizers::{
        models::wordpiece::WordPiece, pre_tokenizers::whitespace::WhitespaceSplit,
        processors::bert::BertProcessing, TruncationParams,
    };
    let words = [
        "[UNK]",
        "[CLS]",
        "[SEP]",
        "hello",
        "revers",
        "##ibility",
        "ibi",
        "##lity",
    ];
    let vocab: [(String, u32); 8] = std::array::from_fn(|id| (words[id].to_owned(), id as u32));
    let mut tokenizer =
        tokenizers::Tokenizer::new(WordPiece::builder().vocab(vocab).build().unwrap());
    tokenizer.with_pre_tokenizer(Some(WhitespaceSplit));
    tokenizer.with_post_processor(Some(BertProcessing::new(
        ("[SEP]".into(), 2),
        ("[CLS]".into(), 1),
    )));
    let mut inputs = fixtures();
    inputs.push((
        "continuation_boundary".into(),
        format!(
            "{}reversibility {}",
            "hello ".repeat(449),
            "hello ".repeat(520)
        ),
    ));
    for truncated in [false, true] {
        if truncated {
            tokenizer
                .with_truncation(Some(TruncationParams {
                    max_length: 16,
                    stride: 3,
                    ..Default::default()
                }))
                .unwrap();
        }
        for (name, input) in &inputs {
            let ranges = super::tests::original_chunk_ranges(&tokenizer, input).unwrap();
            let expected: Vec<_> = ranges
                .iter()
                .map(|range| {
                    (
                        range.clone(),
                        tokenizer.encode(&input[range.clone()], true).unwrap(),
                    )
                })
                .collect();
            ENCODE_CALLS.with(|calls| calls.set(0));
            let actual: Vec<_> = prepare_windows(&tokenizer, input)
                .unwrap()
                .map(Result::unwrap)
                .collect();
            assert!(
                actual == expected,
                "model-input mismatch: {name}, truncated={truncated}"
            );
            assert_eq!(
                ENCODE_CALLS.with(|calls| calls.get()),
                if !truncated && ranges.len() == 1 && ranges[0] == (0..input.len()) {
                    1
                } else {
                    ranges.len() + 1
                }
            );
        }
    }
}
