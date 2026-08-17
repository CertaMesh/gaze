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

        let logits = match outputs.iter().next() {
            Some((_, value)) => value,
            None => return Ok(Vec::new()),
        };
        let (shape_obj, flat) = logits
            .try_extract_tensor::<f32>()
            .map_err(|err| NerRuntimeError::Output(err.to_string()))?;
        let shape: Vec<usize> = shape_obj.iter().map(|d| *d as usize).collect();
        if shape.len() != 3 || shape[0] != 1 || shape[1] != seq_len {
            return Ok(Vec::new());
        }

        Ok(decode_logits(
            labels, id2label, offsets, flat, seq_len, shape[2], input,
        ))
    }
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
) -> Vec<NerSpanResult> {
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
    NerDetector::merge_bio_span_results(labels, offsets, &subword_labels, &subword_scores, input)
        .into_iter()
        .filter(|span| span.span.end <= input.len())
        .collect()
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
        );
        assert_eq!(out.len(), 1, "out-of-range span must be dropped: {out:?}");
        assert_eq!(out[0].span, 0..4);
        assert_eq!(out[0].class, PiiClass::Name);
    }
}
