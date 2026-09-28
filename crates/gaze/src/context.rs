use std::collections::HashMap;
use std::fs;
use std::io::Read;
use std::path::Path;

use serde::de::{self, MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::PiiClass;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawContext {
    #[serde(default)]
    pub(crate) dictionaries: HashMap<String, RawContextDictionary>,
    #[serde(default)]
    pub(crate) class_map: HashMap<String, String>,
    #[serde(default)]
    pub(crate) fields: Map<String, Value>,
    #[serde(default)]
    pub(crate) record: Option<Value>,
    #[serde(default)]
    pub(crate) field_map: HashMap<String, String>,
}

const MAX_CONTEXT_BYTES: usize = 4 * 1024 * 1024;
const MAX_RECORD_BYTES: usize = 65_536;
const MAX_RECORD_DEPTH: usize = 4;
const MAX_RECORD_FIELDS: usize = 32;
const MAX_VALUE_BYTES: usize = 256;
const MAX_VARIANTS_PER_FIELD: usize = 2;
pub const RECORD_DICTIONARY_PREFIX: &str = "__record_v2_";
const HIDDEN_CONTEXT_NAME: &str = "<context>";

/// Stable internal key for a typed record slot. The class digest avoids
/// exposing adopter class names in dictionary IDs and audit source IDs.
pub fn record_dictionary_name(class: &PiiClass, slot: usize) -> String {
    let digest = Sha256::digest(class.to_canonical_str().as_bytes());
    format!("{RECORD_DICTIONARY_PREFIX}{}_{slot}", hex::encode(digest))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawContextDictionary {
    pub(crate) terms: Vec<String>,
    #[serde(default)]
    pub(crate) case_sensitive: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Context {
    pub dictionaries: HashMap<String, ContextDictionary>,
    pub class_map: HashMap<String, PiiClass>,
    pub fields: Map<String, Value>,
}

#[derive(Debug, Clone, Copy)]
pub struct ContextFieldsRef<'a>(&'a Map<String, Value>);

impl<'a> ContextFieldsRef<'a> {
    pub fn as_map(&self) -> &'a Map<String, Value> {
        self.0
    }

    pub fn get(&self, key: &str) -> Option<&'a Value> {
        self.0.get(key)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&'a String, &'a Value)> + 'a {
        self.0.iter()
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextDictionary {
    pub terms: Vec<String>,
    pub case_sensitive: bool,
}

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum ContextError {
    #[error("failed to read context JSON")]
    Io(#[source] std::io::Error),
    #[error("failed to parse context JSON")]
    Json(#[source] serde_json::Error),
    #[error("unknown pii class in context class_map")]
    UnknownClass(String),
    #[error("context dictionary has no terms")]
    EmptyDictionary { name: String },
    #[error(
        "unicode dictionary insensitive matching unsupported in v0.4.0, use case_sensitive = true"
    )]
    UnicodeInsensitiveDictionaryUnsupported { name: String },
    #[error("context JSON exceeds the size limit")]
    TooLarge,
    #[error("record and field_map must be supplied together")]
    IncompleteRecord,
    #[error("record field mapping is incomplete or invalid")]
    InvalidRecordMapping,
    #[error("record exceeds depth, field, value, or variant limits")]
    RecordLimit,
}

impl Context {
    pub fn load(path: &Path) -> Result<Self, ContextError> {
        let mut raw = String::new();
        fs::File::open(path)
            .map_err(ContextError::Io)?
            .take((MAX_CONTEXT_BYTES + 1) as u64)
            .read_to_string(&mut raw)
            .map_err(ContextError::Io)?;
        Self::from_json_str(&raw)
    }

    pub fn from_json_str(raw: &str) -> Result<Self, ContextError> {
        if raw.len() > MAX_CONTEXT_BYTES {
            return Err(ContextError::TooLarge);
        }
        let strict = serde_json::from_str::<UniqueJsonValue>(raw).map_err(safe_json_error)?;
        let raw = serde_json::from_value::<RawContext>(strict.0).map_err(safe_json_error)?;
        Self::from_raw(raw)
    }

    pub fn fields_typed(&self) -> ContextFieldsRef<'_> {
        ContextFieldsRef(&self.fields)
    }

    fn from_raw(raw: RawContext) -> Result<Self, ContextError> {
        if raw
            .dictionaries
            .keys()
            .any(|name| name.starts_with(RECORD_DICTIONARY_PREFIX))
        {
            return Err(ContextError::InvalidRecordMapping);
        }
        let mut class_map = HashMap::with_capacity(raw.class_map.len());
        for (name, class) in raw.class_map {
            let parsed = PiiClass::from_policy_name(&class)
                .ok_or_else(|| ContextError::UnknownClass(HIDDEN_CONTEXT_NAME.into()))?;
            class_map.insert(name, parsed);
        }

        let mut dictionaries = HashMap::with_capacity(raw.dictionaries.len());
        for (name, dictionary) in raw.dictionaries {
            if dictionary.terms.is_empty() {
                return Err(ContextError::EmptyDictionary {
                    name: HIDDEN_CONTEXT_NAME.into(),
                });
            }
            if !dictionary.case_sensitive && dictionary.terms.iter().any(|term| !term.is_ascii()) {
                return Err(ContextError::UnicodeInsensitiveDictionaryUnsupported {
                    name: HIDDEN_CONTEXT_NAME.into(),
                });
            }
            dictionaries.insert(
                name,
                ContextDictionary {
                    terms: dictionary.terms,
                    case_sensitive: dictionary.case_sensitive,
                },
            );
        }

        match (raw.record.as_ref(), raw.field_map.is_empty()) {
            (None, false) | (Some(_), true) => return Err(ContextError::IncompleteRecord),
            _ => {}
        }
        if let Some(record) = raw.record.as_ref() {
            if serde_json::to_vec(record).map_err(safe_json_error)?.len() > MAX_RECORD_BYTES {
                return Err(ContextError::RecordLimit);
            }
            let mut leaves = Vec::new();
            collect_record_leaves(record, "", 0, &mut leaves)?;
            if leaves.is_empty() || leaves.len() != raw.field_map.len() {
                return Err(ContextError::InvalidRecordMapping);
            }
            let mut class_slots = HashMap::<PiiClass, usize>::new();
            for (path, value) in leaves {
                let class = raw
                    .field_map
                    .get(&path)
                    .and_then(|name| PiiClass::from_policy_name(name))
                    .ok_or(ContextError::InvalidRecordMapping)?;
                let mut terms = vec![value.to_string()];
                if class == PiiClass::Name {
                    if let Some(reversed) = reversed_full_name(value) {
                        terms.push(reversed);
                    }
                }
                if terms.len() > MAX_VARIANTS_PER_FIELD {
                    return Err(ContextError::RecordLimit);
                }
                let slot = class_slots.entry(class.clone()).or_default();
                let name = record_dictionary_name(&class, *slot);
                *slot += 1;
                dictionaries.insert(
                    name.clone(),
                    ContextDictionary {
                        terms,
                        // Only ASCII email values use insensitive matching.
                        case_sensitive: !(class == PiiClass::Email && value.is_ascii()),
                    },
                );
                class_map.insert(name, class);
            }
        }

        Ok(Self {
            dictionaries,
            class_map,
            fields: raw.fields,
        })
    }
}

fn safe_json_error(_: serde_json::Error) -> ContextError {
    ContextError::Json(<serde_json::Error as de::Error>::custom(
        "invalid context JSON",
    ))
}

// serde_json::Value silently keeps the last duplicate object key. A duplicate
// CRM field could otherwise hide a value that was never mapped for protection.
struct UniqueJsonValue(Value);

impl<'de> Deserialize<'de> for UniqueJsonValue {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct UniqueVisitor;

        impl<'de> Visitor<'de> for UniqueVisitor {
            type Value = UniqueJsonValue;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("JSON value with unique object keys")
            }

            fn visit_bool<E: de::Error>(self, value: bool) -> Result<Self::Value, E> {
                Ok(UniqueJsonValue(Value::Bool(value)))
            }

            fn visit_i64<E: de::Error>(self, value: i64) -> Result<Self::Value, E> {
                Ok(UniqueJsonValue(Value::Number(value.into())))
            }

            fn visit_u64<E: de::Error>(self, value: u64) -> Result<Self::Value, E> {
                Ok(UniqueJsonValue(Value::Number(value.into())))
            }

            fn visit_f64<E: de::Error>(self, value: f64) -> Result<Self::Value, E> {
                let number = serde_json::Number::from_f64(value)
                    .ok_or_else(|| E::custom("invalid JSON number"))?;
                Ok(UniqueJsonValue(Value::Number(number)))
            }

            fn visit_str<E: de::Error>(self, value: &str) -> Result<Self::Value, E> {
                Ok(UniqueJsonValue(Value::String(value.to_owned())))
            }

            fn visit_string<E: de::Error>(self, value: String) -> Result<Self::Value, E> {
                Ok(UniqueJsonValue(Value::String(value)))
            }

            fn visit_none<E: de::Error>(self) -> Result<Self::Value, E> {
                Ok(UniqueJsonValue(Value::Null))
            }

            fn visit_unit<E: de::Error>(self) -> Result<Self::Value, E> {
                Ok(UniqueJsonValue(Value::Null))
            }

            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
                let mut values = Vec::new();
                while let Some(value) = seq.next_element::<UniqueJsonValue>()? {
                    values.push(value.0);
                }
                Ok(UniqueJsonValue(Value::Array(values)))
            }

            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
                let mut values = Map::new();
                while let Some((key, value)) = map.next_entry::<String, UniqueJsonValue>()? {
                    if values.insert(key, value.0).is_some() {
                        return Err(de::Error::custom("duplicate JSON object key"));
                    }
                }
                Ok(UniqueJsonValue(Value::Object(values)))
            }
        }

        deserializer.deserialize_any(UniqueVisitor)
    }
}

fn collect_record_leaves<'a>(
    value: &'a Value,
    path: &str,
    depth: usize,
    leaves: &mut Vec<(String, &'a str)>,
) -> Result<(), ContextError> {
    if depth > MAX_RECORD_DEPTH || leaves.len() > MAX_RECORD_FIELDS {
        return Err(ContextError::RecordLimit);
    }
    match value {
        Value::Object(fields) if !fields.is_empty() => {
            for (key, child) in fields {
                if key.is_empty()
                    || !key
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
                {
                    return Err(ContextError::InvalidRecordMapping);
                }
                collect_record_leaves(child, &format!("{path}/{key}"), depth + 1, leaves)?;
            }
        }
        Value::String(text) if !path.is_empty() && !text.trim().is_empty() => {
            if text.len() > MAX_VALUE_BYTES || leaves.len() >= MAX_RECORD_FIELDS {
                return Err(ContextError::RecordLimit);
            }
            leaves.push((path.to_string(), text));
        }
        _ => return Err(ContextError::InvalidRecordMapping),
    }
    Ok(())
}

fn reversed_full_name(value: &str) -> Option<String> {
    let parts = value.split_whitespace().collect::<Vec<_>>();
    if parts.len() != 2
        || !parts
            .iter()
            .all(|part| part.chars().count() >= 3 && part.chars().all(char::is_alphabetic))
    {
        return None;
    }
    Some(format!("{} {}", parts[1], parts[0]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_typed_context_envelope() {
        let ctx = Context::from_json_str(
            r#"{
              "dictionaries": {
                "dict_alpha": { "terms": ["AAA-12345"], "case_sensitive": true }
              },
              "class_map": { "dict_alpha": "custom:class_alpha" },
              "fields": { "tenant": "demo" }
            }"#,
        )
        .expect("context");
        assert_eq!(ctx.dictionaries["dict_alpha"].terms, vec!["AAA-12345"]);
        assert_eq!(
            ctx.class_map["dict_alpha"],
            PiiClass::Custom("class_alpha".to_string())
        );
        assert_eq!(ctx.fields["tenant"], Value::String("demo".to_string()));
    }

    #[test]
    fn rejects_unknown_top_level_context_keys() {
        let err = serde_json::from_str::<RawContext>(
            r#"{
              "dictionaries": {},
              "class_map": {},
              "fields": {},
              "extra": true
            }"#,
        )
        .expect_err("unknown key must fail");
        assert!(err.to_string().contains("unknown field"));
    }

    #[test]
    fn rejects_unicode_case_insensitive_terms() {
        assert!(matches!(
            Context::from_json_str(
                r#"{
              "dictionaries": {
                "songs": { "terms": ["Beyoncé"], "case_sensitive": false }
              },
              "class_map": { "songs": "custom:song" },
              "fields": {}
            }"#,
            ),
            Err(ContextError::UnicodeInsensitiveDictionaryUnsupported { .. })
        ));
    }

    #[test]
    fn context_fields_ref_iter_matches_underlying_map() {
        let ctx = Context::from_json_str(
            r#"{
              "dictionaries": {},
              "class_map": {},
              "fields": { "tenant": "demo", "region": "eu" }
            }"#,
        )
        .expect("context");

        let typed = ctx.fields_typed();
        let from_ref = typed.iter().collect::<Vec<_>>();
        let from_map = ctx.fields.iter().collect::<Vec<_>>();

        assert_eq!(from_ref, from_map);
        assert_eq!(typed.len(), ctx.fields.len());
        assert_eq!(
            typed.get("tenant"),
            Some(&Value::String("demo".to_string()))
        );
        assert!(!typed.is_empty());
    }

    #[test]
    fn context_fields_ref_borrows_without_clone() {
        let ctx = Context::from_json_str(
            r#"{
              "dictionaries": {},
              "class_map": {},
              "fields": { "tenant": "demo" }
            }"#,
        )
        .expect("context");

        assert!(std::ptr::eq(ctx.fields_typed().as_map(), &ctx.fields));
    }

    #[test]
    fn record_generates_bounded_typed_terms() {
        let ctx = Context::from_json_str(
            r#"{"record":{"customer":{"name":"Alice Smith","email":"alice@example.invalid"}},"field_map":{"/customer/name":"Name","/customer/email":"Email"}}"#,
        )
        .unwrap();
        let name = ctx
            .class_map
            .iter()
            .find(|(_, class)| **class == PiiClass::Name)
            .map(|(name, _)| name)
            .unwrap();
        assert_eq!(ctx.dictionaries[name].terms, ["Alice Smith", "Smith Alice"]);
        assert!(ctx.dictionaries[name].case_sensitive);
        let email = ctx
            .class_map
            .iter()
            .find(|(_, class)| **class == PiiClass::Email)
            .map(|(name, _)| name)
            .unwrap();
        assert!(!ctx.dictionaries[email].case_sensitive);
    }

    #[test]
    fn record_rejects_unmapped_ambiguous_or_oversized_values_without_echo() {
        for raw in [
            r#"{"record":{"customer":{"name":"Alice Smith","secret":"private marker"}},"field_map":{"/customer/name":"Name"}}"#.to_string(),
            r#"{"record":{"customer":{"name":"private marker"}},"field_map":{"/customer/name":"unknown:private marker"}}"#.to_string(),
            r#"{"record":{"customer":{"name":"private marker"}},"field_map":{"/wrong":"Name"}}"#.to_string(),
            serde_json::json!({"record":{"name":"X".repeat(MAX_VALUE_BYTES + 1)},"field_map":{"/name":"Name"}}).to_string(),
        ] {
            let err = Context::from_json_str(&raw).unwrap_err();
            assert!(!err.to_string().contains("private marker"));
            assert!(!err.to_string().contains("Alice Smith"));
            assert!(!format!("{err:?}").contains("private marker"));
            assert!(!format!("{err:?}").contains("Alice Smith"));
        }
    }

    #[test]
    fn context_json_parse_error_hides_untrusted_field_names() {
        let err = Context::from_json_str(r#"{"private marker":"value"}"#).unwrap_err();
        assert_eq!(err.to_string(), "failed to parse context JSON");
        assert!(!format!("{err:?}").contains("private marker"));
    }

    #[test]
    fn duplicate_record_field_is_rejected_without_echo() {
        let err = Context::from_json_str(
            r#"{"record":{"name":"Alice Smith","name":"private marker"},"field_map":{"/name":"Name"}}"#,
        )
        .unwrap_err();
        assert_eq!(err.to_string(), "failed to parse context JSON");
        assert!(!format!("{err:?}").contains("private marker"));
    }
}
