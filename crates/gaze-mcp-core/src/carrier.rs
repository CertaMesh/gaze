//! Producer-owned carrier declarations. JSON schemas confer no authority.
use serde_json::Value;

/// One unambiguous edge in a JSON path.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum CarrierSegment {
    /// Exact object member, including punctuation or numeric-looking names.
    Member(String),
    /// Exactly one array edge; never matches an object member.
    AnyIndex,
}

/// Full path from the operation root.
pub type CarrierPath = Vec<CarrierSegment>;

/// Trusted static object edges and explicitly non-sensitive numeric leaves.
/// Empty declarations allow strings, arrays, booleans and null, but no object
/// members or numbers. Parent paths never authorize descendants.
#[derive(Debug, Clone, Default)]
pub struct CarrierDeclaration {
    members: Vec<CarrierPath>,
    numbers: Vec<CarrierPath>,
}

/// Class-only failure; rejected keys, paths and values are never included.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum CarrierError {
    /// Declaration has duplicate or invalid paths.
    #[error("invalid carrier declaration")]
    InvalidDeclaration,
    /// An object edge lacks a full static-key declaration.
    #[error("undeclared object member")]
    UndeclaredMember,
    /// A number lacks a non-sensitive numeric declaration.
    #[error("undeclared numeric carrier")]
    UndeclaredNumber,
}

impl CarrierDeclaration {
    /// Explicit declarations are validated when the producer registers.
    pub fn new(members: Vec<CarrierPath>, numbers: Vec<CarrierPath>) -> Self {
        Self { members, numbers }
    }
    /// Convenience for a flat object with text/boolean/null fields only.
    pub fn text_fields(fields: &[&str]) -> Self {
        Self::new(
            fields
                .iter()
                .map(|field| vec![CarrierSegment::Member((*field).into())])
                .collect(),
            vec![],
        )
    }
    pub(crate) fn validate(&self) -> Result<(), CarrierError> {
        use std::collections::HashSet;
        if self
            .members
            .iter()
            .any(|path| !matches!(path.last(), Some(CarrierSegment::Member(_))))
            || self.members.iter().collect::<HashSet<_>>().len() != self.members.len()
            || self.numbers.iter().collect::<HashSet<_>>().len() != self.numbers.len()
        {
            return Err(CarrierError::InvalidDeclaration);
        }
        Ok(())
    }
    pub(crate) fn preflight(&self, value: &Value) -> Result<(), CarrierError> {
        self.validate()?;
        self.walk(value, &mut vec![])
    }
    fn walk(&self, value: &Value, path: &mut CarrierPath) -> Result<(), CarrierError> {
        match value {
            Value::Object(map) => {
                for (key, value) in map {
                    path.push(CarrierSegment::Member(key.clone()));
                    if !self.members.contains(path) {
                        return Err(CarrierError::UndeclaredMember);
                    }
                    self.walk(value, path)?;
                    path.pop();
                }
            }
            Value::Array(values) => {
                path.push(CarrierSegment::AnyIndex);
                for value in values {
                    self.walk(value, path)?;
                }
                path.pop();
            }
            Value::Number(_) if !self.numbers.contains(path) => {
                return Err(CarrierError::UndeclaredNumber)
            }
            _ => {}
        }
        Ok(())
    }
}
