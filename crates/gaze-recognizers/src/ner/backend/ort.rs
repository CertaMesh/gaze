use std::ops::Range;
use std::path::Path;
use std::sync::Mutex;

use super::{NerBackend, NER_CHUNK_TOKEN_BUDGET, NER_CHUNK_TOKEN_OVERLAP};
use crate::ner::decode::softmax_confidence;
use crate::ner::detector::NerDetector;
use crate::ner::error::{NerLoadError, NerRuntimeError};
use crate::ner::types::{LabelMap, NerBackendKind, NerSpanResult, MODEL_FILE, TOKENIZER_FILE};

/// BERT-family token-classification backend. Owns its tokenizer, ONNX session,
/// label map, `id2label` vocab, and pre-computed source tag. BIO/IOB2 subword
/// tags are merged via `NerDetector::merge_bio_spans`.
pub(crate) struct OrtBackend {
    tokenizer: tokenizers::Tokenizer,
    session: Mutex<ort::session::Session>,
    labels: LabelMap,
    id2label: Vec<String>,
    source: String,
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
        Ok(Self {
            tokenizer,
            session: Mutex::new(session),
            labels,
            id2label,
            source: format!("ner/{}", NerBackendKind::Ort.as_str()),
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
        let source = self.source.as_str();
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
        let token_type: Vec<i64> = vec![0i64; seq_len];

        let shape = [1usize, seq_len];
        let input_ids_tensor = ort::value::Tensor::from_array((shape, input_ids))
            .map_err(|err| NerRuntimeError::InputTensor(err.to_string()))?;
        let attn_tensor = ort::value::Tensor::from_array((shape, attn_mask))
            .map_err(|err| NerRuntimeError::InputTensor(err.to_string()))?;
        let type_tensor = ort::value::Tensor::from_array((shape, token_type))
            .map_err(|err| NerRuntimeError::InputTensor(err.to_string()))?;

        let inputs = ort::inputs![
            "input_ids" => input_ids_tensor,
            "attention_mask" => attn_tensor,
            "token_type_ids" => type_tensor,
        ];

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

        let num_labels = shape[2];
        let mut subword_labels: Vec<&str> = Vec::with_capacity(seq_len);
        let mut subword_scores: Vec<f32> = Vec::with_capacity(seq_len);
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
            let label = id2label.get(argmax).map(String::as_str).unwrap_or("O");
            subword_labels.push(label);
            subword_scores.push(softmax_confidence(row, argmax));
        }

        Ok(NerDetector::merge_bio_span_results(
            labels,
            offsets,
            &subword_labels,
            &subword_scores,
            source,
        )
        .into_iter()
        .filter(|span| span.span.end <= input.len())
        .collect())
    }
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

    debug_assert!(NER_CHUNK_TOKEN_OVERLAP < NER_CHUNK_TOKEN_BUDGET);
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
