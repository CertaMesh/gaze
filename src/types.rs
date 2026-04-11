//! Core data types. `RawRow` is a private wrapper around untrusted source data;
//! `CleanRow` is the only type allowed to cross the MCP boundary.
//!
//! Invariant: `RawRow` MUST NOT implement `serde::Serialize`. This is enforced
//! by `tests/ui/rawrow_no_serialize.rs`.

#![allow(dead_code)]

use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Text(String),
    Bytes(Vec<u8>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColumnType {
    Int,
    Float,
    Text,
    Bool,
    Bytes,
    Date,
    DateTime,
}

/// Untrusted row straight from the adapter. Not serializable on purpose.
#[derive(Debug, Clone)]
pub struct RawRow {
    pub columns: BTreeMap<String, Value>,
}

/// Row that has been through the anonymizer. Only constructable inside
/// `crate::anon` — outside callers receive these and can serialize them.
#[derive(Debug, Clone, Serialize)]
pub struct CleanRow {
    #[serde(flatten)]
    columns: BTreeMap<String, serde_json::Value>,
}

impl CleanRow {
    /// Module-private constructor. Only `crate::anon` may build a `CleanRow`.
    pub(crate) fn from_columns(columns: BTreeMap<String, serde_json::Value>) -> Self {
        Self { columns }
    }

    pub fn columns(&self) -> &BTreeMap<String, serde_json::Value> {
        &self.columns
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raw_row_holds_typed_values() {
        let mut cols = BTreeMap::new();
        cols.insert("id".into(), Value::Int(42));
        cols.insert("name".into(), Value::Text("Krishan".into()));
        let row = RawRow { columns: cols };
        assert_eq!(row.columns.len(), 2);
    }

    #[test]
    fn clean_row_is_serializable() {
        let mut cols = BTreeMap::new();
        cols.insert("id".into(), serde_json::json!(1043782));
        cols.insert("name".into(), serde_json::json!("Person_7"));
        let row = CleanRow::from_columns(cols);
        let json = serde_json::to_string(&row).unwrap();
        assert!(json.contains("Person_7"));
        assert!(json.contains("1043782"));
    }
}
