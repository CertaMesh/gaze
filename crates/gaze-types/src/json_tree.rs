//! Strict, span-preserving JSON parsing shared by structured boundaries.
//!
//! The parser accepts exactly one RFC 8259 value and refuses what a lenient parser would
//! silently resolve: duplicate object keys, lone surrogates, trailing bytes and limit
//! overruns. Every node keeps its byte span in the input and its [`FieldPath`], so callers
//! can locate a scalar without re-scanning. Errors carry a closed code and never input
//! bytes. Tree types hold decoded values and deliberately implement no `Debug`.

use std::collections::HashSet;
use std::ops::Range;

use thiserror::Error;

use crate::FieldPath;

/// Finite parse limits. A limit overrun is an error, never a truncated tree.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct JsonTreeLimits {
    max_body_bytes: usize,
    max_depth: usize,
    max_nodes: usize,
    max_string_bytes: usize,
    max_number_bytes: usize,
    ascii_keys_only: bool,
}

impl Default for JsonTreeLimits {
    fn default() -> Self {
        Self {
            max_body_bytes: 32 * 1024 * 1024,
            max_depth: 128,
            max_nodes: 100_000,
            max_string_bytes: 4 * 1024 * 1024,
            max_number_bytes: 128,
            ascii_keys_only: false,
        }
    }
}

impl JsonTreeLimits {
    #[must_use]
    pub const fn with_max_body_bytes(mut self, value: usize) -> Self {
        self.max_body_bytes = value;
        self
    }

    #[must_use]
    pub const fn with_max_depth(mut self, value: usize) -> Self {
        self.max_depth = value;
        self
    }

    #[must_use]
    pub const fn with_max_nodes(mut self, value: usize) -> Self {
        self.max_nodes = value;
        self
    }

    #[must_use]
    pub const fn with_max_string_bytes(mut self, value: usize) -> Self {
        self.max_string_bytes = value;
        self
    }

    #[must_use]
    pub const fn with_max_number_bytes(mut self, value: usize) -> Self {
        self.max_number_bytes = value;
        self
    }

    /// Refuse object keys outside ASCII as [`JsonTreeError::InvalidJson`].
    #[must_use]
    pub const fn with_ascii_keys_only(mut self, value: bool) -> Self {
        self.ascii_keys_only = value;
        self
    }
}

/// Closed, value-free parse failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Error)]
#[non_exhaustive]
pub enum JsonTreeError {
    #[error("input is not valid UTF-8")]
    InvalidUtf8,
    #[error("input is not one strict JSON value")]
    InvalidJson,
    #[error("an object repeats a key")]
    DuplicateObjectKey,
    #[error("nesting exceeds the depth limit")]
    DepthLimitExceeded,
    #[error("value count exceeds the node limit")]
    NodeLimitExceeded,
    #[error("a string exceeds the string limit")]
    StringLimitExceeded,
    #[error("a number exceeds the number limit")]
    NumberLimitExceeded,
    #[error("input exceeds the body limit")]
    BodyLimitExceeded,
}

/// A parsed document. Occurrences number every value and key in document order.
pub struct JsonTree {
    pub root: JsonNode,
    pub occurrence_count: usize,
}

pub struct JsonNode {
    /// Unescaped keys and decimal array indices from the root.
    pub path: FieldPath,
    pub occurrence: usize,
    /// Input bytes of the complete value, including a string's quotes.
    pub span: Range<usize>,
    pub kind: JsonKind,
}

pub enum JsonKind {
    Object(Vec<JsonMember>),
    Array(Vec<JsonNode>),
    String(JsonString),
    /// The number's exact source spelling.
    Number(String),
    Bool(bool),
    Null,
}

pub struct JsonMember {
    pub key_occurrence: usize,
    pub key: JsonString,
    pub value: JsonNode,
}

/// A decoded string value.
pub struct JsonString {
    pub value: String,
}

/// Parse `input` as exactly one strict JSON value.
pub fn parse(input: &[u8], limits: JsonTreeLimits) -> Result<JsonTree, JsonTreeError> {
    if input.len() > limits.max_body_bytes {
        return Err(JsonTreeError::BodyLimitExceeded);
    }
    let text = validate_utf8(input)?;
    let mut parser = Parser {
        text,
        bytes: input,
        position: 0,
        limits,
        nodes: 0,
        next_occurrence: 0,
    };
    parser.skip_whitespace();
    let root = parser.parse_value(0, &FieldPath::default())?;
    parser.skip_whitespace();
    if parser.position != input.len() {
        return Err(JsonTreeError::InvalidJson);
    }
    Ok(JsonTree {
        root,
        occurrence_count: parser.next_occurrence,
    })
}

struct Parser<'a> {
    /// The input, validated once; decoding reads scalars from it without revalidating.
    text: &'a str,
    bytes: &'a [u8],
    position: usize,
    limits: JsonTreeLimits,
    nodes: usize,
    next_occurrence: usize,
}

impl Parser<'_> {
    fn parse_value(&mut self, depth: usize, path: &FieldPath) -> Result<JsonNode, JsonTreeError> {
        if depth > self.limits.max_depth {
            return Err(JsonTreeError::DepthLimitExceeded);
        }
        self.nodes = self
            .nodes
            .checked_add(1)
            .ok_or(JsonTreeError::NodeLimitExceeded)?;
        if self.nodes > self.limits.max_nodes {
            return Err(JsonTreeError::NodeLimitExceeded);
        }
        let occurrence = self.occurrence();
        let start = self.position;
        let kind = match self.peek() {
            Some(b'{') => self.parse_object(depth + 1, path)?,
            Some(b'[') => self.parse_array(depth + 1, path)?,
            Some(b'"') => JsonKind::String(self.parse_string()?),
            Some(b't') => {
                self.consume_literal(b"true")?;
                JsonKind::Bool(true)
            }
            Some(b'f') => {
                self.consume_literal(b"false")?;
                JsonKind::Bool(false)
            }
            Some(b'n') => {
                self.consume_literal(b"null")?;
                JsonKind::Null
            }
            Some(b'-' | b'0'..=b'9') => JsonKind::Number(self.parse_number()?),
            _ => return Err(JsonTreeError::InvalidJson),
        };
        Ok(JsonNode {
            path: path.clone(),
            occurrence,
            span: start..self.position,
            kind,
        })
    }

    fn parse_object(&mut self, depth: usize, path: &FieldPath) -> Result<JsonKind, JsonTreeError> {
        self.position += 1;
        self.skip_whitespace();
        let mut members = Vec::new();
        let mut keys = HashSet::new();
        if self.peek() == Some(b'}') {
            self.position += 1;
            return Ok(JsonKind::Object(members));
        }
        loop {
            if self.peek() != Some(b'"') {
                return Err(JsonTreeError::InvalidJson);
            }
            let key_occurrence = self.occurrence();
            let key = self.parse_string()?;
            if self.limits.ascii_keys_only && !key.value.is_ascii() {
                return Err(JsonTreeError::InvalidJson);
            }
            if !keys.insert(key.value.clone()) {
                return Err(JsonTreeError::DuplicateObjectKey);
            }
            self.skip_whitespace();
            if self.take() != Some(b':') {
                return Err(JsonTreeError::InvalidJson);
            }
            self.skip_whitespace();
            let child = path.child(key.value.clone());
            let value = self.parse_value(depth, &child)?;
            members.push(JsonMember {
                key_occurrence,
                key,
                value,
            });
            self.skip_whitespace();
            match self.take() {
                Some(b',') => self.skip_whitespace(),
                Some(b'}') => break,
                _ => return Err(JsonTreeError::InvalidJson),
            }
        }
        Ok(JsonKind::Object(members))
    }

    fn parse_array(&mut self, depth: usize, path: &FieldPath) -> Result<JsonKind, JsonTreeError> {
        self.position += 1;
        self.skip_whitespace();
        let mut values = Vec::new();
        if self.peek() == Some(b']') {
            self.position += 1;
            return Ok(JsonKind::Array(values));
        }
        loop {
            let child = path.child(values.len().to_string());
            values.push(self.parse_value(depth, &child)?);
            self.skip_whitespace();
            match self.take() {
                Some(b',') => self.skip_whitespace(),
                Some(b']') => break,
                _ => return Err(JsonTreeError::InvalidJson),
            }
        }
        Ok(JsonKind::Array(values))
    }

    fn parse_string(&mut self) -> Result<JsonString, JsonTreeError> {
        if self.take() != Some(b'"') {
            return Err(JsonTreeError::InvalidJson);
        }
        let mut value = String::new();
        loop {
            let byte = self.take().ok_or(JsonTreeError::InvalidJson)?;
            match byte {
                b'"' => break,
                b'\\' => self.parse_escape(&mut value)?,
                0x00..=0x1f => return Err(JsonTreeError::InvalidJson),
                0x20..=0x7f => value.push(char::from(byte)),
                _ => {
                    let start = self.position - 1;
                    let character = self
                        .text
                        .get(start..)
                        .and_then(|remaining| remaining.chars().next())
                        .ok_or(JsonTreeError::InvalidUtf8)?;
                    value.push(character);
                    self.position = start + character.len_utf8();
                }
            }
            if value.len() > self.limits.max_string_bytes {
                return Err(JsonTreeError::StringLimitExceeded);
            }
        }
        Ok(JsonString { value })
    }

    fn parse_escape(&mut self, output: &mut String) -> Result<(), JsonTreeError> {
        match self.take().ok_or(JsonTreeError::InvalidJson)? {
            b'"' => output.push('"'),
            b'\\' => output.push('\\'),
            b'/' => output.push('/'),
            b'b' => output.push('\u{0008}'),
            b'f' => output.push('\u{000c}'),
            b'n' => output.push('\n'),
            b'r' => output.push('\r'),
            b't' => output.push('\t'),
            b'u' => {
                let first = self.parse_hex_quad()?;
                let scalar = if (0xd800..=0xdbff).contains(&first) {
                    if self.take() != Some(b'\\') || self.take() != Some(b'u') {
                        return Err(JsonTreeError::InvalidJson);
                    }
                    let second = self.parse_hex_quad()?;
                    if !(0xdc00..=0xdfff).contains(&second) {
                        return Err(JsonTreeError::InvalidJson);
                    }
                    0x1_0000 + (((u32::from(first) - 0xd800) << 10) | (u32::from(second) - 0xdc00))
                } else if (0xdc00..=0xdfff).contains(&first) {
                    return Err(JsonTreeError::InvalidJson);
                } else {
                    u32::from(first)
                };
                output.push(char::from_u32(scalar).ok_or(JsonTreeError::InvalidJson)?);
            }
            _ => return Err(JsonTreeError::InvalidJson),
        }
        Ok(())
    }

    fn parse_hex_quad(&mut self) -> Result<u16, JsonTreeError> {
        let mut value = 0u16;
        for _ in 0..4 {
            let digit = self
                .take()
                .and_then(hex_value)
                .ok_or(JsonTreeError::InvalidJson)?;
            value = (value << 4) | u16::from(digit);
        }
        Ok(value)
    }

    fn parse_number(&mut self) -> Result<String, JsonTreeError> {
        let start = self.position;
        if self.peek() == Some(b'-') {
            self.position += 1;
        }
        match self.take() {
            Some(b'0') if matches!(self.peek(), Some(b'0'..=b'9')) => {
                return Err(JsonTreeError::InvalidJson);
            }
            Some(b'0'..=b'9') => {
                while matches!(self.peek(), Some(b'0'..=b'9')) {
                    self.position += 1;
                }
            }
            _ => return Err(JsonTreeError::InvalidJson),
        }
        if self.peek() == Some(b'.') {
            self.position += 1;
            if !matches!(self.peek(), Some(b'0'..=b'9')) {
                return Err(JsonTreeError::InvalidJson);
            }
            while matches!(self.peek(), Some(b'0'..=b'9')) {
                self.position += 1;
            }
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            self.position += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.position += 1;
            }
            if !matches!(self.peek(), Some(b'0'..=b'9')) {
                return Err(JsonTreeError::InvalidJson);
            }
            while matches!(self.peek(), Some(b'0'..=b'9')) {
                self.position += 1;
            }
        }
        let raw = &self.text[start..self.position];
        if raw.len() > self.limits.max_number_bytes {
            return Err(JsonTreeError::NumberLimitExceeded);
        }
        Ok(raw.to_owned())
    }

    fn consume_literal(&mut self, literal: &[u8]) -> Result<(), JsonTreeError> {
        if self.bytes.get(self.position..self.position + literal.len()) != Some(literal) {
            return Err(JsonTreeError::InvalidJson);
        }
        self.position += literal.len();
        Ok(())
    }

    fn occurrence(&mut self) -> usize {
        let occurrence = self.next_occurrence;
        self.next_occurrence += 1;
        occurrence
    }

    fn skip_whitespace(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\n' | b'\r' | b'\t')) {
            self.position += 1;
        }
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.position).copied()
    }

    fn take(&mut self) -> Option<u8> {
        let value = self.peek()?;
        self.position += 1;
        Some(value)
    }
}

/// Every UTF-8 validation in the parser goes through here, so the work counter sees all of it.
fn validate_utf8(bytes: &[u8]) -> Result<&str, JsonTreeError> {
    crate::perf_count!("json_tree_utf8_validated_bytes", bytes.len());
    std::str::from_utf8(bytes).map_err(|_| JsonTreeError::InvalidUtf8)
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests;
