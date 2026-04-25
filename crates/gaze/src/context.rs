use std::collections::HashMap;
use std::fs;
use std::path::Path;

use serde::Deserialize;
use serde_json::{Map, Value};
use thiserror::Error;

use crate::PiiClass;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawContext {
    pub dictionaries: HashMap<String, RawContextDictionary>,
    pub class_map: HashMap<String, String>,
    pub fields: Map<String, Value>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawContextDictionary {
    pub terms: Vec<String>,
    #[serde(default)]
    pub case_sensitive: bool,
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
pub enum ContextError {
    #[error("failed to read context JSON: {0}")]
    Io(#[source] std::io::Error),
    #[error("failed to parse context JSON: {0}")]
    Json(#[source] serde_json::Error),
    #[error("unknown pii class in context class_map: {0}")]
    UnknownClass(String),
    #[error("context dictionary '{name}' has no terms")]
    EmptyDictionary { name: String },
    #[error(
        "unicode dictionary insensitive matching unsupported in v0.4.0, use case_sensitive = true"
    )]
    UnicodeInsensitiveDictionaryUnsupported { name: String },
}

impl Context {
    pub fn load(path: &Path) -> Result<Self, ContextError> {
        let raw = fs::read_to_string(path).map_err(ContextError::Io)?;
        let raw = serde_json::from_str::<RawContext>(&raw).map_err(ContextError::Json)?;
        Self::try_from(raw)
    }

    pub fn fields_typed(&self) -> ContextFieldsRef<'_> {
        ContextFieldsRef(&self.fields)
    }
}

impl TryFrom<RawContext> for Context {
    type Error = ContextError;

    fn try_from(raw: RawContext) -> Result<Self, Self::Error> {
        let mut class_map = HashMap::with_capacity(raw.class_map.len());
        for (name, class) in raw.class_map {
            let parsed = PiiClass::from_policy_name(&class)
                .ok_or_else(|| ContextError::UnknownClass(class.clone()))?;
            class_map.insert(name, parsed);
        }

        let mut dictionaries = HashMap::with_capacity(raw.dictionaries.len());
        for (name, dictionary) in raw.dictionaries {
            if dictionary.terms.is_empty() {
                return Err(ContextError::EmptyDictionary { name });
            }
            if !dictionary.case_sensitive && dictionary.terms.iter().any(|term| !term.is_ascii()) {
                return Err(ContextError::UnicodeInsensitiveDictionaryUnsupported { name });
            }
            dictionaries.insert(
                name,
                ContextDictionary {
                    terms: dictionary.terms,
                    case_sensitive: dictionary.case_sensitive,
                },
            );
        }

        Ok(Self {
            dictionaries,
            class_map,
            fields: raw.fields,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_typed_context_envelope() {
        let raw = serde_json::from_str::<RawContext>(
            r#"{
              "dictionaries": {
                "dict_alpha": { "terms": ["AAA-12345"], "case_sensitive": true }
              },
              "class_map": { "dict_alpha": "custom:class_alpha" },
              "fields": { "tenant": "demo" }
            }"#,
        )
        .expect("raw context");

        let ctx = Context::try_from(raw).expect("context");
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
        let raw = serde_json::from_str::<RawContext>(
            r#"{
              "dictionaries": {
                "songs": { "terms": ["Beyoncé"], "case_sensitive": false }
              },
              "class_map": { "songs": "custom:song" },
              "fields": {}
            }"#,
        )
        .expect("raw context");

        assert!(matches!(
            Context::try_from(raw),
            Err(ContextError::UnicodeInsensitiveDictionaryUnsupported { .. })
        ));
    }

    #[test]
    fn context_fields_ref_iter_matches_underlying_map() {
        let raw = serde_json::from_str::<RawContext>(
            r#"{
              "dictionaries": {},
              "class_map": {},
              "fields": { "tenant": "demo", "region": "eu" }
            }"#,
        )
        .expect("raw context");
        let ctx = Context::try_from(raw).expect("context");

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
        let raw = serde_json::from_str::<RawContext>(
            r#"{
              "dictionaries": {},
              "class_map": {},
              "fields": { "tenant": "demo" }
            }"#,
        )
        .expect("raw context");
        let ctx = Context::try_from(raw).expect("context");

        assert!(std::ptr::eq(ctx.fields_typed().as_map(), &ctx.fields));
    }
}
