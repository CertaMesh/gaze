//! The byte-to-text boundary. The parser cannot borrow input bytes or text: it
//! receives individual bytes/scalars and owned spellings only. Keep the validated
//! text private, with no Deref, AsRef, indexing or borrowed-slice escape hatch.

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
        self.text.len()
    }

    pub(super) fn byte_at(&self, position: usize) -> Option<u8> {
        self.text.as_bytes().get(position).copied()
    }

    pub(super) fn matches_literal(&self, start: usize, literal: &[u8]) -> bool {
        self.text.as_bytes().get(start..start + literal.len()) == Some(literal)
    }

    pub(super) fn scalar_at(&self, start: usize) -> Option<char> {
        self.text.get(start..)?.chars().next()
    }

    pub(super) fn copy_range(&self, range: Range<usize>) -> String {
        self.text[range].to_owned()
    }
}
