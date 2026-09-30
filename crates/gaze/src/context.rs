use std::collections::{BTreeSet, HashMap, HashSet};
use std::fs;
use std::io::Read;
use std::path::Path;

use serde::de::{self, MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::PiiClass;
use gaze_types::{RecordMatchKind, ValidatorKind};

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
    #[serde(default)]
    pub(crate) record_match_kinds: HashMap<String, BTreeSet<RecordMatchKind>>,
}

const MAX_CONTEXT_BYTES: usize = 4 * 1024 * 1024;
const MAX_RECORD_BYTES: usize = 65_536;
const MAX_RECORD_DEPTH: usize = 4;
const MAX_RECORD_FIELDS: usize = 32;
const MAX_VALUE_BYTES: usize = 256;
pub const RECORD_DICTIONARY_PREFIX: &str = "record-v2-";
const HIDDEN_CONTEXT_NAME: &str = "<context>";

/// Stable internal key for a typed record slot. The class digest avoids
/// exposing adopter class names in dictionary IDs and audit source IDs.
pub fn record_dictionary_name(class: &PiiClass, slot: usize) -> String {
    let digest = Sha256::digest(class.to_canonical_str().as_bytes());
    format!("{RECORD_DICTIONARY_PREFIX}{}-{slot}", hex::encode(digest))
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
    pub record_match_kinds: HashMap<String, BTreeSet<RecordMatchKind>>,
    /// Values refused by the record matcher. Paths and reasons never contain values.
    pub record_value_rejections: Vec<RecordValueRejection>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordValueRejection {
    pub path: String,
    pub reason: RecordValueRejectionReason,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum RecordValueRejectionReason {
    UnsafeShortMatch,
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
    #[error("failed to parse record JSON at {path}")]
    RecordJson { path: String },
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
    #[error("record field at /field_map requires /record")]
    IncompleteRecord,
    #[error("record field at {path} has no known class or valid mapping")]
    InvalidRecordMapping { path: String },
    #[error("record field at {path} exceeds a depth, field, or value limit")]
    RecordLimit { path: String },
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
        let strict = serde_json::from_str::<UniqueJsonValue>(raw)
            .map_err(|error| safe_context_json_error(error, raw))?;
        if let Value::Object(top) = &strict.0 {
            let has_record = top.contains_key("record");
            let has_field_map = top.contains_key("field_map");
            let has_match_kinds = top.contains_key("record_match_kinds");
            if (has_field_map || has_match_kinds) && !has_record {
                return Err(ContextError::IncompleteRecord);
            }
            if top.get("record").is_some_and(Value::is_null) {
                return Err(ContextError::InvalidRecordMapping {
                    path: "/record".into(),
                });
            }
        }
        let raw = serde_json::from_value::<RawContext>(strict.0)
            .map_err(|error| safe_context_json_error(error, raw))?;
        Self::from_raw(raw)
    }

    pub fn fields_typed(&self) -> ContextFieldsRef<'_> {
        ContextFieldsRef(&self.fields)
    }

    pub fn record_allowed_match_kinds(
        &self,
        class: &PiiClass,
        term: &str,
    ) -> BTreeSet<RecordMatchKind> {
        let group = record_match_group(class, term);
        if let Some(override_kinds) = self.record_match_kinds.get(&group) {
            return override_kinds.clone();
        }
        match group.as_str() {
            "name_single" => [
                RecordMatchKind::CaseFolded,
                RecordMatchKind::CorroboratedSingle,
            ]
            .into_iter()
            .collect(),
            "name_multi" => [
                RecordMatchKind::Exact,
                RecordMatchKind::CaseFolded,
                RecordMatchKind::WhitespaceCaseFolded,
            ]
            .into_iter()
            .collect(),
            "custom:credit_card" | "custom:iban" | "custom:national_id" | "custom:steuer_id" => {
                [RecordMatchKind::Exact, RecordMatchKind::WhitespaceFlexible]
                    .into_iter()
                    .collect()
            }
            "custom:passport" | "custom:phone" => [RecordMatchKind::Exact].into_iter().collect(),
            _ => BTreeSet::new(),
        }
    }

    fn from_raw(raw: RawContext) -> Result<Self, ContextError> {
        if raw.record_match_kinds.keys().any(|name| {
            !matches!(name.as_str(), "name_single" | "name_multi" | "address_part")
                && PiiClass::from_policy_name(name).is_none_or(|class| {
                    class.to_canonical_str() != *name
                        || matches!(class, PiiClass::Name | PiiClass::Location)
                })
        }) {
            return Err(ContextError::InvalidRecordMapping {
                path: "/record_match_kinds".into(),
            });
        }
        if raw.record_match_kinds.iter().any(|(name, kinds)| {
            (name != "name_single" && kinds.contains(&RecordMatchKind::CorroboratedSingle))
                || (name != "name_single"
                    && name != "name_multi"
                    && kinds.iter().any(|kind| {
                        matches!(
                            kind,
                            RecordMatchKind::CaseFolded | RecordMatchKind::WhitespaceCaseFolded
                        )
                    }))
                || (name == "name_single"
                    && kinds.iter().any(|kind| {
                        matches!(
                            kind,
                            RecordMatchKind::WhitespaceFlexible
                                | RecordMatchKind::WhitespaceCaseFolded
                        )
                    }))
        }) {
            return Err(ContextError::InvalidRecordMapping {
                path: "/record_match_kinds".into(),
            });
        }
        if raw
            .dictionaries
            .keys()
            .chain(raw.class_map.keys())
            .any(|name| name.starts_with(RECORD_DICTIONARY_PREFIX))
        {
            return Err(ContextError::InvalidRecordMapping { path: "/".into() });
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

        if let Some(record) = raw.record.as_ref() {
            if serde_json::to_vec(record).map_err(safe_json_error)?.len() > MAX_RECORD_BYTES {
                return Err(ContextError::RecordLimit {
                    path: "/record".into(),
                });
            }
            let mut leaves = Vec::new();
            collect_record_leaves(record, "", 0, &mut leaves)?;
            if leaves.is_empty() {
                return Err(ContextError::InvalidRecordMapping {
                    path: "/record".into(),
                });
            }
            for path in raw.field_map.keys() {
                if !leaves.iter().any(|(leaf, _)| leaf == path) {
                    return Err(ContextError::InvalidRecordMapping {
                        path: safe_record_path(path),
                    });
                }
            }
            let mut class_slots = HashMap::<PiiClass, usize>::new();
            let mut present_match_groups = HashSet::new();
            let mut seen_values = HashSet::new();
            let mut record_value_rejections = Vec::new();
            for (path, value) in leaves {
                let mapped = raw.field_map.get(&path);
                if mapped.is_some_and(|name| name == "ignore") {
                    continue;
                }
                let class = match mapped {
                    Some(name) => PiiClass::from_policy_name(name),
                    None => path.rsplit('/').next().and_then(inferred_record_class),
                }
                .ok_or_else(|| ContextError::InvalidRecordMapping {
                    path: safe_record_path(&path),
                })?;
                let canonical = canonical_record_value(value);
                present_match_groups.insert(record_match_group(&class, &canonical));
                if !safe_record_value(&canonical, &class) {
                    record_value_rejections.push(RecordValueRejection {
                        path: safe_record_path(&path),
                        reason: RecordValueRejectionReason::UnsafeShortMatch,
                    });
                    continue;
                }
                if !seen_values.insert((class.clone(), canonical.clone())) {
                    continue;
                }
                let slot = class_slots.entry(class.clone()).or_default();
                let name = record_dictionary_name(&class, *slot);
                *slot += 1;
                dictionaries.insert(
                    name.clone(),
                    ContextDictionary {
                        terms: vec![canonical],
                        // The record-name recognizer handles Unicode case matching.
                        case_sensitive: true,
                    },
                );
                class_map.insert(name, class);
            }
            if !raw
                .record_match_kinds
                .keys()
                .all(|group| present_match_groups.contains(group))
            {
                return Err(ContextError::InvalidRecordMapping {
                    path: "/record_match_kinds".into(),
                });
            }
            return Ok(Self {
                dictionaries,
                class_map,
                fields: raw.fields,
                record_match_kinds: raw.record_match_kinds,
                record_value_rejections,
            });
        }

        Ok(Self {
            dictionaries,
            class_map,
            fields: raw.fields,
            record_match_kinds: raw.record_match_kinds,
            record_value_rejections: Vec::new(),
        })
    }
}

fn record_match_group(class: &PiiClass, term: &str) -> String {
    match class {
        PiiClass::Name if term.split_whitespace().count() == 1 => "name_single".into(),
        PiiClass::Name => "name_multi".into(),
        PiiClass::Location => "address_part".into(),
        _ => class.to_canonical_str(),
    }
}

fn safe_json_error(_: serde_json::Error) -> ContextError {
    ContextError::Json(<serde_json::Error as de::Error>::custom(
        "invalid context JSON",
    ))
}

fn safe_context_json_error(error: serde_json::Error, raw: &str) -> ContextError {
    if raw.contains("\"record\"") || raw.contains("\"field_map\"") {
        ContextError::RecordJson {
            path: "/record".into(),
        }
    } else {
        safe_json_error(error)
    }
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
        return Err(ContextError::RecordLimit {
            path: safe_record_path(path),
        });
    }
    match value {
        Value::Object(fields) if !fields.is_empty() => {
            for (key, child) in fields {
                if key.is_empty()
                    || !key
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
                {
                    return Err(ContextError::InvalidRecordMapping {
                        path: safe_record_path(path),
                    });
                }
                collect_record_leaves(child, &format!("{path}/{key}"), depth + 1, leaves)?;
            }
        }
        Value::String(text) if !path.is_empty() && !text.trim().is_empty() => {
            if text.len() > MAX_VALUE_BYTES || leaves.len() >= MAX_RECORD_FIELDS {
                return Err(ContextError::RecordLimit {
                    path: safe_record_path(path),
                });
            }
            leaves.push((path.to_string(), text));
        }
        _ => {
            return Err(ContextError::InvalidRecordMapping {
                path: safe_record_path(path),
            })
        }
    }
    Ok(())
}

fn safe_record_value(value: &str, class: &PiiClass) -> bool {
    if matches!(class, PiiClass::Custom(name) if name == "iban")
        && ValidatorKind::IbanMod97.validates(value)
    {
        return true;
    }
    let letters = value.chars().filter(|ch| ch.is_alphabetic()).count();
    let digits = value.chars().filter(|ch| ch.is_numeric()).count();
    (letters == 0 && digits >= 4) || letters >= 3
}

fn canonical_record_value(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn safe_record_path(path: &str) -> String {
    if !path.is_empty()
        && path.len() <= 256
        && path
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'_' | b'-'))
    {
        path.to_owned()
    } else {
        "<invalid path>".into()
    }
}

/// Version 1 of the conservative EN/DE/FR/NL/PT key alias table. Unknown keys
/// require an explicit mapping; the table never infers from a record value.
fn inferred_record_class(key: &str) -> Option<PiiClass> {
    let normalized = key
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .map(|ch| ch.to_ascii_lowercase())
        .collect::<String>();
    let class = match normalized.as_str() {
        "email" | "emailaddress" | "mail" | "courriel" | "emailadres" | "correioeletronico" => {
            "Email"
        }
        "phone" | "phonenumber" | "tel" | "telephone" | "telefon" | "mobile" | "handy"
        | "telefono" | "telefoon" | "telemovel" | "celular" => "custom:phone",
        "name" | "fullname" | "firstname" | "givenname" | "vorname" | "lastname" | "surname"
        | "nachname" | "nom" | "prenom" | "achternaam" | "voornaam" | "nome" | "sobrenome" => {
            "Name"
        }
        "iban" => "custom:iban",
        "dob" | "dateofbirth" | "birthdate" | "geburtsdatum" | "datedenaissance"
        | "geboortedatum" | "datadenascimento" => "custom:date",
        "zip" | "postcode" | "plz" | "codepostal" | "cep" => "custom:postal_code",
        "address" | "street" | "strasse" | "city" | "stadt" | "adresse" | "rue" | "ville"
        | "adres" | "straat" | "plaats" | "endereco" | "rua" | "cidade" => "Location",
        _ => return None,
    };
    PiiClass::from_policy_name(class)
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
        assert_eq!(ctx.dictionaries[name].terms, ["Alice Smith"]);
        assert!(name
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-'));
        assert!(ctx.dictionaries[name].case_sensitive);
        let email = ctx
            .class_map
            .iter()
            .find(|(_, class)| **class == PiiClass::Email)
            .map(|(name, _)| name)
            .unwrap();
        assert!(ctx.dictionaries[email].case_sensitive);
    }

    #[test]
    fn record_match_defaults_and_overrides_are_explicit() {
        let context = Context::from_json_str(
            r#"{"record":{"first_name":"Maren","full_name":"Maren Okafor","email":"alice@example.invalid","phone":"+1-555-0104"}}"#,
        ).unwrap();
        assert_eq!(
            context.record_allowed_match_kinds(&PiiClass::Name, "Maren"),
            [
                RecordMatchKind::CaseFolded,
                RecordMatchKind::CorroboratedSingle
            ]
            .into_iter()
            .collect(),
        );
        assert_eq!(
            context.record_allowed_match_kinds(&PiiClass::Name, "Maren Okafor"),
            [
                RecordMatchKind::Exact,
                RecordMatchKind::CaseFolded,
                RecordMatchKind::WhitespaceCaseFolded,
            ]
            .into_iter()
            .collect(),
        );
        assert!(context
            .record_allowed_match_kinds(&PiiClass::Email, "alice@example.invalid")
            .is_empty());
        assert_eq!(
            context.record_allowed_match_kinds(&PiiClass::Custom("phone".into()), "+1-555-0104"),
            [RecordMatchKind::Exact].into_iter().collect(),
        );
        assert!(context
            .record_allowed_match_kinds(&PiiClass::Location, "Synthetic Value")
            .is_empty());
        assert_eq!(
            context.record_allowed_match_kinds(
                &PiiClass::Custom("passport".into()),
                "Synthetic Value"
            ),
            [RecordMatchKind::Exact].into_iter().collect(),
        );
        for class in ["credit_card", "iban", "national_id", "steuer_id"] {
            assert_eq!(
                context
                    .record_allowed_match_kinds(&PiiClass::Custom(class.into()), "Synthetic Value"),
                [RecordMatchKind::Exact, RecordMatchKind::WhitespaceFlexible]
                    .into_iter()
                    .collect(),
            );
        }

        let opted = Context::from_json_str(
            r#"{"record":{"name":"Maren Okafor"},"record_match_kinds":{"name_multi":["whitespace_flexible","whitespace_case_folded"]}}"#,
        ).unwrap();
        assert_eq!(
            opted.record_allowed_match_kinds(&PiiClass::Name, "Maren Okafor"),
            [
                RecordMatchKind::WhitespaceFlexible,
                RecordMatchKind::WhitespaceCaseFolded
            ]
            .into_iter()
            .collect(),
        );
        assert!(matches!(
            Context::from_json_str(
                r#"{"record":{"name":"Maren"},"record_match_kinds":{"Name":["exact"]}}"#
            ),
            Err(ContextError::InvalidRecordMapping { .. })
        ));
        assert!(matches!(
            Context::from_json_str(
                r#"{"record":{"email":"alice@example.invalid"},"record_match_kinds":{"email":["case_folded"]}}"#
            ),
            Err(ContextError::InvalidRecordMapping { .. })
        ));
        assert!(matches!(
            Context::from_json_str(
                r#"{"record":{"name":"Maren"},"record_match_kinds":{"name_multi":["exact"]}}"#
            ),
            Err(ContextError::InvalidRecordMapping { .. })
        ));
        assert!(matches!(
            Context::from_json_str(
                r#"{"record":{"name":"Maren"},"record_match_kinds":{"name_single":["unknown"]}}"#
            ),
            Err(ContextError::RecordJson { .. })
        ));
    }

    #[test]
    fn record_rejects_unmapped_ambiguous_or_oversized_values_without_echo() {
        for raw in [
            r#"{"record":{"customer":{"name":"Alice Smith","secret":"private marker"}},"field_map":{"/customer/name":"Name"}}"#.to_string(),
            r#"{"record":{"customer":{"name":"private marker"}},"field_map":{"/customer/name":"unknown:private marker"}}"#.to_string(),
            r#"{"record":{"customer":{"name":"private marker"}},"field_map":{"/wrong":"Name"}}"#.to_string(),
            serde_json::json!({"record":{"name":"X".repeat(MAX_VALUE_BYTES + 1)},"field_map":{"/name":"Name"}}).to_string(),
            format!(r#"{{"class_map":{{"{}collision":"Name"}}}}"#, RECORD_DICTIONARY_PREFIX),
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
        assert_eq!(err.to_string(), "failed to parse record JSON at /record");
        assert!(!format!("{err:?}").contains("private marker"));
    }

    #[test]
    fn record_canonicalizes_spacing_without_echo() {
        for value in [" Alice Smith ", "Alice  Smith", "Alice\u{a0}Smith"] {
            let raw = serde_json::json!({"record":{"name":value},"field_map":{"/name":"Name"}});
            let context = Context::from_json_str(&raw.to_string()).unwrap();
            let name = context.dictionaries.values().next().unwrap();
            assert_eq!(name.terms, ["Alice Smith"]);
        }
    }

    #[test]
    fn record_rejects_only_short_values_with_path() {
        for (value, class) in [
            ("A", "Name"),
            ("12", "custom:phone"),
            ("A12", "custom:tag"),
            ("A1234", "custom:tag"),
        ] {
            let raw = serde_json::json!({"record":{"value":value},"field_map":{"/value":class}});
            let context = Context::from_json_str(&raw.to_string()).unwrap();
            assert!(context.dictionaries.is_empty());
            assert_eq!(
                context.record_value_rejections,
                [RecordValueRejection {
                    path: "/value".into(),
                    reason: RecordValueRejectionReason::UnsafeShortMatch,
                }]
            );
            assert!(!format!("{:?}", context.record_value_rejections).contains(value));
        }
        Context::from_json_str(r#"{"record":{"name":"Will"}}"#).unwrap();
    }

    #[test]
    fn valid_two_letter_country_ibans_are_accepted_without_weakening_short_name_floor() {
        for iban in [
            "DE36000000000000000000",
            "AT180000000000000000",
            "FR7600000000000000000000000",
        ] {
            let raw = serde_json::json!({"record":{"iban":iban,"name":"A"}});
            let context = Context::from_json_str(&raw.to_string()).unwrap();
            assert_eq!(context.dictionaries.len(), 1);
            assert_eq!(context.dictionaries.values().next().unwrap().terms, [iban]);
            assert_eq!(
                context.record_value_rejections,
                [RecordValueRejection {
                    path: "/name".into(),
                    reason: RecordValueRejectionReason::UnsafeShortMatch,
                }]
            );
        }
        let invalid =
            Context::from_json_str(r#"{"record":{"iban":"DE00000000000000000000"}}"#).unwrap();
        assert!(invalid.dictionaries.is_empty());
        assert_eq!(invalid.record_value_rejections.len(), 1);
    }

    #[test]
    fn duplicate_record_values_share_one_source() {
        let context =
            Context::from_json_str(r#"{"record":{"a":{"name":"Maren"},"b":{"name":"Maren"}}}"#)
                .unwrap();
        assert_eq!(context.dictionaries.len(), 1);
        assert!(context.record_value_rejections.is_empty());
    }

    #[test]
    fn explicit_incomplete_or_null_record_envelope_fails_closed() {
        for raw in [
            r#"{"record":null,"field_map":{}}"#,
            r#"{"field_map":{}}"#,
            r#"{"record":{"unknown":"Alice Smith"}}"#,
        ] {
            assert!(Context::from_json_str(raw).is_err());
        }
    }

    #[test]
    fn inferred_aliases_and_override_or_ignore() {
        let context = Context::from_json_str(r#"{"record":{"customer":{"firstName":"Maren Okafor","e_mail":"alice@example.invalid","secret":"private marker"}},"field_map":{"/customer/secret":"ignore"}}"#).unwrap();
        assert_eq!(context.dictionaries.len(), 2);
        assert!(context
            .class_map
            .values()
            .any(|class| *class == PiiClass::Name));
        assert!(context
            .class_map
            .values()
            .any(|class| *class == PiiClass::Email));
        let override_context = Context::from_json_str(r#"{"record":{"customer":{"unknown":"alice@example.invalid"}},"field_map":{"/customer/unknown":"Email"}}"#).unwrap();
        assert_eq!(
            override_context.class_map.values().next(),
            Some(&PiiClass::Email)
        );
        let error =
            Context::from_json_str(r#"{"record":{"customer":{"unknown":"private marker"}}}"#)
                .unwrap_err();
        assert!(error.to_string().contains("/customer/unknown"));
        assert!(!error.to_string().contains("private marker"));
    }

    #[test]
    fn alias_table_v1_normalizes_case_camel_snake_and_kebab_across_locales() {
        for (key, class) in [
            ("E_MAIL", PiiClass::Email),
            ("firstName", PiiClass::Name),
            ("NachName", PiiClass::Name),
            ("date-de-naissance", PiiClass::Custom("date".into())),
            ("geboorte_datum", PiiClass::Custom("date".into())),
            ("correioEletronico", PiiClass::Email),
            ("Strasse", PiiClass::Location),
            ("plz", PiiClass::Custom("postal_code".into())),
            ("codePostal", PiiClass::Custom("postal_code".into())),
        ] {
            assert_eq!(inferred_record_class(key), Some(class), "{key}");
        }
    }
}
