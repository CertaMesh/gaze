use std::ops::Range;
use std::path::Path;
use std::sync::Mutex;

use super::{NerBackend, NER_CHUNK_TOKEN_BUDGET, NER_CHUNK_TOKEN_OVERLAP};
use crate::ner::decode::softmax_confidence;
use crate::ner::detector::NerDetector;
use crate::ner::error::{NerLoadError, NerRuntimeError};
use crate::ner::types::{LabelMap, NerSpanResult, MODEL_FILE, TOKENIZER_FILE};

/// BERT-family token-classification backend. Owns its tokenizer, ONNX session,
/// label map, and `id2label` vocab. BIO/IOB2 subword tags are merged via
/// `decode_logits` -> `NerDetector::merge_bio_span_results`.
pub(crate) struct OrtBackend {
    tokenizer: tokenizers::Tokenizer,
    session: Mutex<ort::session::Session>,
    labels: LabelMap,
    id2label: Vec<String>,
    has_token_type_ids: bool,
}

impl OrtBackend {
    pub(crate) fn load(
        model_dir: &Path,
        labels: LabelMap,
        id2label: Vec<String>,
    ) -> Result<Self, NerLoadError> {
        let tokenizer = tokenizers::Tokenizer::from_file(model_dir.join(TOKENIZER_FILE))
            .map_err(|err| NerLoadError::Tokenizer(err.to_string()))?;
        let session = ort::session::Session::builder()
            .map_err(|err| NerLoadError::Runtime(err.to_string()))?
            .commit_from_file(model_dir.join(MODEL_FILE))
            .map_err(|err| NerLoadError::Runtime(err.to_string()))?;
        let has_token_type_ids = session
            .inputs()
            .iter()
            .any(|input| input.name() == "token_type_ids");
        Ok(Self {
            tokenizer,
            session: Mutex::new(session),
            labels,
            id2label,
            has_token_type_ids,
        })
    }
}

impl NerBackend for OrtBackend {
    fn chunk_ranges(&self, input: &str) -> Result<Vec<Range<usize>>, NerRuntimeError> {
        tokenized_chunk_ranges(&self.tokenizer, input)
    }

    fn detect(&self, input: &str) -> Result<Vec<NerSpanResult>, NerRuntimeError> {
        let labels = &self.labels;
        let id2label: &[String] = &self.id2label;
        let encoded = self
            .tokenizer
            .encode(input, true)
            .map_err(|err| NerRuntimeError::Tokenizer(err.to_string()))?;
        let offsets = encoded.get_offsets();
        let ids = encoded.get_ids();
        let attention = encoded.get_attention_mask();
        if ids.is_empty() {
            return Ok(Vec::new());
        }

        let seq_len = ids.len();
        let input_ids: Vec<i64> = ids.iter().map(|&v| v as i64).collect();
        let attn_mask: Vec<i64> = attention.iter().map(|&v| v as i64).collect();
        let shape = [1usize, seq_len];
        let input_ids_tensor = ort::value::Tensor::from_array((shape, input_ids))
            .map_err(|err| NerRuntimeError::InputTensor(err.to_string()))?;
        let attn_tensor = ort::value::Tensor::from_array((shape, attn_mask))
            .map_err(|err| NerRuntimeError::InputTensor(err.to_string()))?;
        let inputs = if self.has_token_type_ids {
            let token_type: Vec<i64> = vec![0i64; seq_len];
            let type_tensor = ort::value::Tensor::from_array((shape, token_type))
                .map_err(|err| NerRuntimeError::InputTensor(err.to_string()))?;
            ort::inputs![
                "input_ids" => input_ids_tensor,
                "attention_mask" => attn_tensor,
                "token_type_ids" => type_tensor,
            ]
        } else {
            ort::inputs![
                "input_ids" => input_ids_tensor,
                "attention_mask" => attn_tensor,
            ]
        };

        let mut session = self
            .session
            .lock()
            .map_err(|err| NerRuntimeError::Poisoned(err.to_string()))?;
        let outputs = session
            .run(inputs)
            .map_err(|err| NerRuntimeError::Inference(err.to_string()))?;

        let Some((_, value)) = outputs.iter().next() else {
            return decode_output(labels, id2label, offsets, None, input);
        };
        let (shape, flat) = value
            .try_extract_tensor::<f32>()
            .map_err(|_| NerRuntimeError::Output("expected a float32 logits tensor".into()))?;
        decode_output(labels, id2label, offsets, Some((shape, flat)), input)
    }
}

/// Validate the model boundary before any label selection or span filtering.
fn decode_output(
    labels: &LabelMap,
    id2label: &[String],
    offsets: &[(usize, usize)],
    output: Option<(&[i64], &[f32])>,
    input: &str,
) -> Result<Vec<NerSpanResult>, NerRuntimeError> {
    let (shape, logits) =
        output.ok_or_else(|| NerRuntimeError::Output("missing logits tensor".into()))?;
    if shape.len() != 3
        || shape[0] != 1
        || usize::try_from(shape[1]).ok() != Some(offsets.len())
        || usize::try_from(shape[2]).ok() != Some(id2label.len())
    {
        return Err(NerRuntimeError::Output(
            "invalid logits tensor shape".into(),
        ));
    }
    decode_logits(
        labels,
        id2label,
        offsets,
        logits,
        offsets.len(),
        id2label.len(),
        input,
    )
}

/// Post-inference decode step: per-subword argmax + softmax confidence, then
/// BIO merge against the input the tokenizer offsets index into. Kept free of
/// `ort` types so the production decode contract can be exercised with
/// synthetic logits and no model.
///
/// `logits` is the flat `[1, seq_len, num_labels]` output tensor.
fn decode_logits(
    labels: &LabelMap,
    id2label: &[String],
    offsets: &[(usize, usize)],
    logits: &[f32],
    seq_len: usize,
    num_labels: usize,
    input: &str,
) -> Result<Vec<NerSpanResult>, NerRuntimeError> {
    if num_labels == 0
        || num_labels != id2label.len()
        || offsets.len() != seq_len
        || seq_len.checked_mul(num_labels) != Some(logits.len())
    {
        return Err(NerRuntimeError::Output("invalid logits dimensions".into()));
    }
    // Include O, low-confidence labels, and special tokens: none may hide corruption.
    if logits.iter().any(|value| !value.is_finite()) {
        return Err(NerRuntimeError::Output("nonfinite logits".into()));
    }
    let mut subword_labels: Vec<&str> = Vec::with_capacity(seq_len);
    let mut subword_scores: Vec<f32> = Vec::with_capacity(seq_len);
    for pos in 0..seq_len {
        let base = pos * num_labels;
        let row = &logits[base..base + num_labels];
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
        let label = id2label.get(argmax).map(String::as_str).unwrap_or("O");
        subword_labels.push(label);
        subword_scores.push(softmax_confidence(row, argmax));
    }

    // `input` is the text the tokenizer offsets index into; the merge reads
    // joiner bytes between tokens from it. Provenance (`ner/ort`) is attached
    // later by `NerRecognizer` / `NerDetector::try_detect`, never here.
    Ok(NerDetector::merge_bio_span_results(
        labels,
        offsets,
        &subword_labels,
        &subword_scores,
        input,
    )
    .into_iter()
    .filter(|span| span.span.end <= input.len())
    .collect())
}

fn tokenized_chunk_ranges(
    tokenizer: &tokenizers::Tokenizer,
    input: &str,
) -> Result<Vec<Range<usize>>, NerRuntimeError> {
    let mut tokenizer = tokenizer.clone();
    tokenizer
        .with_truncation(None)
        .map_err(|err| NerRuntimeError::Tokenizer(err.to_string()))?;
    let encoded = tokenizer
        .encode(input, true)
        .map_err(|err| NerRuntimeError::Tokenizer(err.to_string()))?;
    let tokens: Vec<Range<usize>> = encoded
        .get_offsets()
        .iter()
        .filter_map(|&(start, end)| {
            if start < end
                && end <= input.len()
                && input.is_char_boundary(start)
                && input.is_char_boundary(end)
            {
                Some(start..end)
            } else {
                None
            }
        })
        .collect();

    if tokens.len() <= NER_CHUNK_TOKEN_BUDGET {
        return Ok(std::iter::once(0..input.len()).collect());
    }

    const _: () = assert!(NER_CHUNK_TOKEN_OVERLAP < NER_CHUNK_TOKEN_BUDGET);
    let stride = NER_CHUNK_TOKEN_BUDGET - NER_CHUNK_TOKEN_OVERLAP;
    let mut chunks = Vec::new();
    let mut token_start = 0;
    while token_start < tokens.len() {
        let token_end = (token_start + NER_CHUNK_TOKEN_BUDGET).min(tokens.len());
        chunks.push(tokens[token_start].start..tokens[token_end - 1].end);
        if token_end == tokens.len() {
            break;
        }
        token_start += stride;
    }

    Ok(chunks)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use gaze_types::PiiClass;

    use super::*;

    /// `id2label` vocab shared by every fixture: index 0 = `O`, 1 = `B-PER`,
    /// 2 = `I-PER`, 3 = `B-LOC`, 4 = `I-LOC`.
    const ID2LABEL: [&str; 5] = ["O", "B-PER", "I-PER", "B-LOC", "I-LOC"];

    fn id2label() -> Vec<String> {
        ID2LABEL.iter().map(|label| (*label).to_string()).collect()
    }

    fn labels() -> LabelMap {
        LabelMap(BTreeMap::from([
            ("PER".to_string(), PiiClass::Name),
            ("LOC".to_string(), PiiClass::Location),
        ]))
    }

    /// Byte offsets for `tokens` located left-to-right in `input`, mirroring
    /// what the tokenizer reports (no whitespace tokens).
    fn offsets(input: &str, tokens: &[&str]) -> Vec<(usize, usize)> {
        let mut cursor = 0usize;
        tokens
            .iter()
            .map(|token| {
                let offset = input[cursor..]
                    .find(token)
                    .expect("token exists after cursor");
                let start = cursor + offset;
                let end = start + token.len();
                cursor = end;
                (start, end)
            })
            .collect()
    }

    /// One confident logit row per tag: 10.0 at the tag's vocab index, 0.0
    /// elsewhere, so argmax picks the tag and softmax confidence is ~1.0.
    fn logits(tags: &[&str]) -> Vec<f32> {
        tags.iter()
            .flat_map(|tag| {
                let index = ID2LABEL
                    .iter()
                    .position(|candidate| candidate == tag)
                    .expect("tag in vocab");
                let mut row = vec![0.0f32; ID2LABEL.len()];
                row[index] = 10.0;
                row
            })
            .collect()
    }

    fn decode(input: &str, tokens: &[&str], tags: &[&str]) -> Vec<NerSpanResult> {
        assert_eq!(tokens.len(), tags.len(), "fixture: one tag per token");
        decode_logits(
            &labels(),
            &id2label(),
            &offsets(input, tokens),
            &logits(tags),
            tags.len(),
            ID2LABEL.len(),
            input,
        )
        .unwrap()
    }

    /// Axis 1 / axis 3: the joiner between `Anne` and `Marie` is read from the
    /// document text, so a hyphenated name decodes as ONE span. If the decoder
    /// looks at anything other than the document text the name splits in two.
    #[test]
    fn decode_bridges_hyphenated_name_across_joiner_token() {
        let input = "Anne-Marie";
        let out = decode(input, &["Anne", "-", "Marie"], &["B-PER", "O", "I-PER"]);

        assert_eq!(out.len(), 1, "expected one bridged span: {out:?}");
        assert_eq!(out[0].span, 0..input.len());
        assert_eq!(out[0].class, PiiClass::Name);
    }

    /// Axis 3: short structured field values (tool-call JSON values) are
    /// first-class inputs. A single-token entity that IS the whole document
    /// must survive decoding regardless of how many bytes the document has.
    #[test]
    fn decode_keeps_short_structured_field_span() {
        for input in ["Anna", "Alice", "Berlin"] {
            let out = decode(input, &[input], &["B-PER"]);

            assert_eq!(out.len(), 1, "short field {input:?} lost its span: {out:?}");
            assert_eq!(out[0].span, 0..input.len(), "span for {input:?}");
            assert_eq!(out[0].class, PiiClass::Name);
        }
    }

    /// The text between two entity tokens is read from the document text: a
    /// comma between two independently tagged names never bridges them into
    /// one span, whatever bytes happen to sit at those offsets elsewhere.
    #[test]
    fn decode_does_not_bridge_comma_separated_names() {
        let input = "Ann,Bob";
        let out = decode(input, &["Ann", ",", "Bob"], &["B-PER", "O", "B-PER"]);

        assert_eq!(out.len(), 2, "expected two separate names: {out:?}");
        assert_eq!(out[0].span, 0..3);
        assert_eq!(out[1].span, 4..7);
    }

    /// Span bounds are checked against the document text: a span that ends
    /// exactly at the end of the document is accepted whatever its length, and
    /// an offset past the end of the document is dropped without a panic while
    /// in-range spans survive.
    #[test]
    fn decode_bounds_spans_against_document_text() {
        let input = "Wolfgang Amadeus";
        let out = decode(input, &["Wolfgang", "Amadeus"], &["B-PER", "I-PER"]);
        assert_eq!(out.len(), 1, "{out:?}");
        assert_eq!(out[0].span, 0..input.len());

        let input = "Anna";
        let out = decode_logits(
            &labels(),
            &id2label(),
            &[(0, 4), (10, 20)],
            &logits(&["B-PER", "B-LOC"]),
            2,
            ID2LABEL.len(),
            input,
        )
        .unwrap();
        assert_eq!(out.len(), 1, "out-of-range span must be dropped: {out:?}");
        assert_eq!(out[0].span, 0..4);
        assert_eq!(out[0].class, PiiClass::Name);
    }

    struct SyntheticLogitsBackend {
        invalid: f32,
    }

    impl NerBackend for SyntheticLogitsBackend {
        fn chunk_ranges(&self, _input: &str) -> Result<Vec<Range<usize>>, NerRuntimeError> {
            Ok(vec![0..4, 5..9])
        }

        fn detect(&self, input: &str) -> Result<Vec<NerSpanResult>, NerRuntimeError> {
            let mut values = logits(&["B-PER"]);
            // A valid first chunk must not escape if a later chunk is corrupt.
            if input == "Beta" {
                values[0] = self.invalid;
            }
            decode_output(
                &labels(),
                &id2label(),
                &[(0, 4)],
                Some((&[1, 1, 5], &values)),
                input,
            )
        }
    }

    fn corrupt_recognizer(invalid: f32) -> crate::ner::NerRecognizer {
        crate::ner::NerRecognizer {
            detector: NerDetector {
                model_dir: std::path::PathBuf::new(),
                backend_kind: crate::ner::NerBackendKind::Ort,
                recognizer_version_id: "ner.synthetic.v1".into(),
                locale: None,
                threshold: 0.99,
                backend: std::sync::Arc::new(SyntheticLogitsBackend { invalid }),
            },
        }
    }

    #[test]
    fn invalid_output_reaches_fallible_callers_before_filtering() {
        use gaze_types::{DetectContext, Detector, DictionaryBundle, Recognizer};
        let dictionaries = DictionaryBundle::default();
        let ctx = DetectContext::new(&[], &dictionaries);
        for invalid in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            let recognizer = corrupt_recognizer(invalid);
            assert!(
                recognizer.detector.try_detect("Anna Beta").is_err(),
                "corrupt logits must fail the whole chunked detection"
            );
            assert!(
                Recognizer::detect(&recognizer, "Anna Beta", &ctx).is_err(),
                "threshold filtering must not hide corrupt logits"
            );
        }
    }

    #[test]
    fn invalid_output_blocks_pipeline_clean_output() {
        use gaze::{Pipeline, RawDocument, Scope, Session};
        let pipeline = Pipeline::builder()
            .recognizer(corrupt_recognizer(f32::NAN))
            .build()
            .unwrap();
        let session = Session::new(Scope::Ephemeral).unwrap();
        assert!(matches!(
            pipeline.redact(&session, RawDocument::Text("Anna Beta".into())),
            Err(gaze::Error::RecognizerDetect(
                gaze_types::DetectError::Backend { .. }
            ))
        ));
    }

    #[test]
    #[should_panic(expected = "ner detector backend failure is fail-closed")]
    fn invalid_output_infallible_detector_panics() {
        use gaze_types::Detector;
        corrupt_recognizer(f32::NAN).detector.detect("Anna Beta");
    }

    #[test]
    fn invalid_output_rejects_every_nonfinite_label_and_special_token() {
        for tag in ID2LABEL {
            for index in 0..ID2LABEL.len() {
                for invalid in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
                    let mut values = logits(&[tag]);
                    values[index] = invalid;
                    for (input, offsets) in [("Anna", [(0, 4)]), ("", [(0, 0)])] {
                        let err = decode_output(
                            &labels(),
                            &id2label(),
                            &offsets,
                            Some((&[1, 1, 5], &values)),
                            input,
                        )
                        .unwrap_err();
                        assert!(matches!(err, NerRuntimeError::Output(_)));
                        assert_eq!(err.to_string(), "logits extract failed: nonfinite logits");
                    }
                }
            }
        }
    }

    #[test]
    fn invalid_output_rejects_missing_and_malformed_tensors() {
        assert!(matches!(
            decode_output(&labels(), &id2label(), &[(0, 4)], None, "Anna"),
            Err(NerRuntimeError::Output(_))
        ));
        for shape in [
            vec![],
            vec![1, 5],
            vec![2, 1, 5],
            vec![1, 2, 5],
            vec![1, 1, 0],
            vec![1, 1, 4],
            vec![1, 1, 6],
            vec![1, -1, 5],
            vec![1, 1, -1],
        ] {
            assert!(matches!(
                decode_output(
                    &labels(),
                    &id2label(),
                    &[(0, 4)],
                    Some((&shape, &[0.0; 5])),
                    "Anna"
                ),
                Err(NerRuntimeError::Output(_))
            ));
        }
        for values in [vec![], vec![0.0; 4], vec![0.0; 6]] {
            assert!(matches!(
                decode_output(
                    &labels(),
                    &id2label(),
                    &[(0, 4)],
                    Some((&[1, 1, 5], &values)),
                    "Anna"
                ),
                Err(NerRuntimeError::Output(_))
            ));
        }
    }

    #[test]
    fn valid_output_preserves_empty_special_tokens_and_finite_score_oracle() {
        assert!(
            decode_output(&labels(), &id2label(), &[], Some((&[1, 0, 5], &[])), "")
                .unwrap()
                .is_empty()
        );
        assert!(decode_output(
            &labels(),
            &id2label(),
            &[(0, 0)],
            Some((&[1, 1, 5], &logits(&["B-PER"]))),
            ""
        )
        .unwrap()
        .is_empty());
        assert!(decode_output(
            &labels(),
            &id2label(),
            &[(0, 4)],
            Some((&[1, 1, 5], &[0.0; 5])),
            "Anna"
        )
        .unwrap()
        .is_empty());
        // Independent f64 probability oracle, without the decoder's max subtraction.
        let values = [0.0f32, 2.0, 1.0, -1.0, -2.0];
        let expected = 2.0f64.exp() / values.iter().map(|v| f64::from(*v).exp()).sum::<f64>();
        let out = decode_output(
            &labels(),
            &id2label(),
            &[(0, 4)],
            Some((&[1, 1, 5], &values)),
            "Anna",
        )
        .unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].span, 0..4);
        assert_eq!(out[0].class, PiiClass::Name);
        assert!((f64::from(out[0].score) - expected).abs() < 1e-6);
    }
}
