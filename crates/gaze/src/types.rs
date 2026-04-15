use std::collections::BTreeMap;

use serde::Serialize;

#[derive(Debug, Clone)]
pub enum RawDocument {
    Structured(BTreeMap<String, Value>),
    Text(String),
}

#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub enum CleanDocument {
    Structured(BTreeMap<String, serde_json::Value>),
    Text(String),
}

#[derive(Debug, Clone)]
pub enum Value {
    String(String),
    I64(i64),
}
