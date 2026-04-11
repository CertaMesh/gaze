//! Public anonymizer facade. Owns a `SessionKey` + `SessionMap` and
//! exposes `clean(RawRow)` → `CleanRow`. Callers outside this module
//! cannot build a `CleanRow` any other way.

use std::collections::BTreeMap;
use std::sync::Once;

use crate::anon::replacer::Replacer;
use crate::anon::session::{SessionKey, SessionMap};
use crate::policy::classifier::{Classifier, PiiClass};
use crate::types::{CleanRow, RawRow, Value};

pub struct Anonymizer {
    key: SessionKey,
    map: SessionMap,
    classifier: Classifier,
}

static MLOCKALL_ONCE: Once = Once::new();

impl Anonymizer {
    pub fn new(classifier: Classifier) -> Self {
        let key = SessionKey::generate().unwrap_or_else(|_| SessionKey::generate_unlocked());
        Self::with_key(classifier, key)
    }

    pub fn with_key(classifier: Classifier, key: SessionKey) -> Self {
        MLOCKALL_ONCE.call_once(crate::anon::session::try_mlockall_current);
        Self {
            key,
            map: SessionMap::new(),
            classifier,
        }
    }

    /// Anonymize every column in the row according to its PII class.
    /// Returns a `CleanRow` safe to serialize across the MCP boundary.
    pub fn clean(&self, row: RawRow) -> CleanRow {
        let replacer = Replacer::new(&self.key, &self.map);
        let mut out: BTreeMap<String, serde_json::Value> = BTreeMap::new();

        for (col, value) in row.columns.into_iter() {
            let class = self.classifier.classify(&col);
            let json = clean_value(&replacer, class, value);
            out.insert(col, json);
        }

        CleanRow::from_columns(out)
    }

    /// Reverse a fake value back to the raw that produced it, for a given
    /// column. Returns `None` if the fake is not a known session token.
    /// Used by `restore()` on incoming filter values.
    pub fn raw_for_fake(&self, column: &str, fake: &str) -> Option<String> {
        let class = self.classifier.classify(column);
        if class == PiiClass::NonPii {
            return Some(fake.to_string());
        }
        let class_name = match class {
            PiiClass::NonPii => "nonpii",
            PiiClass::Id => "id",
            PiiClass::Name => "name",
            PiiClass::Email => "email",
            PiiClass::Phone => "phone",
            PiiClass::Address => "address",
            PiiClass::Iban => "iban",
            PiiClass::Ip => "ip",
            PiiClass::Date => "date",
            PiiClass::GenericText => "generic",
        };
        self.map.get_raw(class_name, fake)
    }
}

fn clean_value(replacer: &Replacer<'_>, class: PiiClass, value: Value) -> serde_json::Value {
    match (class, value) {
        (_, Value::Null) => serde_json::Value::Null,
        (PiiClass::NonPii, Value::Bool(b)) => serde_json::Value::Bool(b),
        (PiiClass::NonPii, Value::Int(i)) => serde_json::json!(i),
        (PiiClass::NonPii, Value::Float(f)) => serde_json::json!(f),
        (PiiClass::NonPii, Value::Text(t)) => serde_json::Value::String(t),
        (PiiClass::NonPii, Value::Bytes(_)) => serde_json::Value::String("<bytes>".into()),

        (PiiClass::Id, Value::Int(i)) => serde_json::json!(replacer.replace_id(i)),
        (PiiClass::Id, Value::Text(t)) => {
            // Some MySQL drivers return ids as text; parse or fall through.
            match t.parse::<i64>() {
                Ok(i) => serde_json::json!(replacer.replace_id(i)),
                Err(_) => {
                    serde_json::Value::String(replacer.replace_text(PiiClass::GenericText, &t))
                }
            }
        }
        (PiiClass::Id, other) => {
            // Non-integer id column — fall back to generic text replacement.
            let s = value_display(&other);
            serde_json::Value::String(replacer.replace_text(PiiClass::GenericText, &s))
        }

        (class, Value::Text(t)) => serde_json::Value::String(replacer.replace_text(class, &t)),
        (class, other) => {
            let s = value_display(&other);
            serde_json::Value::String(replacer.replace_text(class, &s))
        }
    }
}

fn value_display(v: &Value) -> String {
    match v {
        Value::Null => "".into(),
        Value::Bool(b) => b.to_string(),
        Value::Int(i) => i.to_string(),
        Value::Float(f) => f.to_string(),
        Value::Text(t) => t.clone(),
        Value::Bytes(_) => "<bytes>".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn anonymizer_with(classes: &[(&str, PiiClass)]) -> Anonymizer {
        let mut c = Classifier::new();
        for (col, class) in classes {
            c = c.with_column(*col, *class);
        }
        Anonymizer::new(c)
    }

    fn raw(pairs: &[(&str, Value)]) -> RawRow {
        let mut cols = BTreeMap::new();
        for (k, v) in pairs {
            cols.insert((*k).to_string(), v.clone());
        }
        RawRow { columns: cols }
    }

    #[test]
    fn non_pii_columns_pass_through() {
        let a = anonymizer_with(&[]);
        let row = raw(&[
            ("created_at", Value::Text("2025-01-01".into())),
            ("count", Value::Int(5)),
        ]);
        let cleaned = a.clean(row);
        assert_eq!(cleaned.columns()["count"], serde_json::json!(5));
        assert_eq!(
            cleaned.columns()["created_at"],
            serde_json::json!("2025-01-01")
        );
    }

    #[test]
    fn email_column_is_anonymized() {
        let a = anonymizer_with(&[("email", PiiClass::Email)]);
        let row = raw(&[("email", Value::Text("krishan@example.com".into()))]);
        let cleaned = a.clean(row);
        let e = cleaned.columns()["email"].as_str().unwrap();
        assert!(e.starts_with("user_") && e.ends_with("@example.com"));
        assert_ne!(e, "krishan@example.com");
    }

    #[test]
    fn id_column_is_anonymized_and_stable() {
        let a = anonymizer_with(&[("id", PiiClass::Id)]);
        let r1 = a.clean(raw(&[("id", Value::Int(42))]));
        let r2 = a.clean(raw(&[("id", Value::Int(42))]));
        assert_eq!(r1.columns()["id"], r2.columns()["id"]);
        assert_ne!(r1.columns()["id"], serde_json::json!(42));
    }

    #[test]
    fn raw_for_fake_round_trips() {
        let a = anonymizer_with(&[("email", PiiClass::Email)]);
        let cleaned = a.clean(raw(&[("email", Value::Text("krishan@example.com".into()))]));
        let fake = cleaned.columns()["email"].as_str().unwrap().to_string();
        assert_eq!(
            a.raw_for_fake("email", &fake).as_deref(),
            Some("krishan@example.com")
        );
    }

    #[test]
    fn raw_for_fake_returns_none_on_unknown_token() {
        let a = anonymizer_with(&[("email", PiiClass::Email)]);
        assert_eq!(a.raw_for_fake("email", "user_999@example.com"), None);
    }
}
