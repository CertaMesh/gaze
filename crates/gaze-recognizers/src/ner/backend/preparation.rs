use std::ops::Range;

use tokenizers::{Encoding, Tokenizer};

pub(crate) const NER_CHUNK_TOKEN_BUDGET: usize = 480;
pub(crate) const NER_CHUNK_TOKEN_OVERLAP: usize = 30;

#[cfg(test)]
thread_local! {
    pub(super) static ENCODE_CALLS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

pub(crate) fn encode_input(tokenizer: &Tokenizer, input: &str) -> tokenizers::Result<Encoding> {
    #[cfg(test)]
    ENCODE_CALLS.with(|calls| calls.set(calls.get() + 1));
    tokenizer.encode(input, true)
}

pub(crate) fn planning_encoding(
    tokenizer: &Tokenizer,
    input: &str,
) -> tokenizers::Result<Encoding> {
    let mut untruncated;
    let tokenizer = if tokenizer.get_truncation().is_none() {
        tokenizer
    } else {
        untruncated = tokenizer.clone();
        untruncated.with_truncation(None)?;
        &untruncated
    };
    encode_input(tokenizer, input)
}

pub(crate) fn prepare_windows<'a>(
    tokenizer: &'a Tokenizer,
    input: &'a str,
) -> tokenizers::Result<impl Iterator<Item = tokenizers::Result<(Range<usize>, Encoding)>> + 'a> {
    let encoded = planning_encoding(tokenizer, input)?;
    let ranges = chunk_ranges_from_encoding(&encoded, input);
    // Planning disables truncation. Reuse only when inference would encode exactly
    // the same input/configuration; mid-word chunk starts must be encoded afresh.
    let stochastic = matches!(tokenizer.get_model(), tokenizers::models::ModelWrapper::BPE(bpe)
        if bpe.dropout.is_some_and(|probability| probability > 0.0));
    let reuse = !stochastic
        && tokenizer.get_truncation().is_none()
        && ranges.len() == 1
        && ranges[0] == (0..input.len());
    let mut shared = reuse.then_some(encoded);
    Ok(ranges.into_iter().map(move |range| {
        let encoding = match shared.take() {
            Some(encoded) => Ok(encoded),
            None => encode_input(tokenizer, &input[range.clone()]),
        };
        encoding.map(|encoded| (range, encoded))
    }))
}

pub(crate) fn chunk_ranges_from_encoding(encoded: &Encoding, input: &str) -> Vec<Range<usize>> {
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
        return std::iter::once(0..input.len()).collect();
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
    chunks
}
