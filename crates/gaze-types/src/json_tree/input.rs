//! The byte-to-text boundary. The parser cannot borrow input bytes or text: it
//! receives individual bytes/scalars and owned spellings only. Keep the validated
//! text private, with no Deref, AsRef, indexing or borrowed-slice escape hatch.
//!
//! Parser construction lives in a separate function that receives only
//! ValidatedInput and limits. The raw input slice is out of scope there, and
//! types prevent direct suffix borrowing through ValidatedInput. Owned copies
//! and byte-by-byte reconstruction remain possible, so every accessor counts
//! calls and the bytes it reads or copies. The scaling test catches repeated
//! suffix reconstruction through these accessors. Changes to accessors inside
//! this module and repeated work on already-returned values still require review.

use std::ops::Range;

use super::JsonTreeError;

pub(super) struct ValidatedInput<'a> {
    text: &'a str,
}

impl<'a> ValidatedInput<'a> {
    pub(super) fn new(bytes: &'a [u8]) -> Result<Self, JsonTreeError> {
        crate::perf_count!("json_tree_utf8_validated_bytes", bytes.len());
        let text = std::str::from_utf8(bytes).map_err(|_| JsonTreeError::InvalidUtf8)?;
        Ok(Self { text })
    }

    pub(super) fn len(&self) -> usize {
        crate::perf_count!("json_tree_input_work", 1);
        self.text.len()
    }

    pub(super) fn byte_at(&self, position: usize) -> Option<u8> {
        // Charge the call and its possible one-byte read, including failed reads.
        crate::perf_count!("json_tree_input_work", 2);
        self.text.as_bytes().get(position).copied()
    }

    pub(super) fn matches_literal(&self, start: usize, literal: &[u8]) -> bool {
        crate::perf_count!("json_tree_input_work", 1 + literal.len());
        self.text.as_bytes().get(start..start + literal.len()) == Some(literal)
    }

    pub(super) fn scalar_at(&self, start: usize) -> Option<char> {
        crate::perf_count!("json_tree_input_work", 1);
        let scalar = self.text.get(start..)?.chars().next()?;
        crate::perf_count!("json_tree_input_work", scalar.len_utf8());
        Some(scalar)
    }

    pub(super) fn copy_range(
        &self,
        range: Range<usize>,
        limit: usize,
    ) -> Result<String, JsonTreeError> {
        crate::perf_count!("json_tree_input_work", 1);
        if range.len() > limit {
            return Err(JsonTreeError::NumberLimitExceeded);
        }
        crate::perf_count!("json_tree_input_work", range.len());
        Ok(self.text[range].to_owned())
    }
}
