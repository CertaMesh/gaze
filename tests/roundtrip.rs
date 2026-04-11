//! Property: for any realistic set of input rows with an email column,
//! cleaning them and then looking each fake back up via raw_for_fake
//! must return the exact raw value. This is the core invariant that
//! db.sample followed by a filter on the returned token depends on.

use gaze::anon::Anonymizer;
use gaze::policy::classifier::{Classifier, PiiClass};
use gaze::types::{RawRow, Value};
use proptest::prelude::*;
use std::collections::BTreeMap;

fn anonymizer() -> Anonymizer {
    Anonymizer::new(
        Classifier::new()
            .with_column("email", PiiClass::Email)
            .with_column("id", PiiClass::Id),
    )
}

fn raw(email: &str, id: i64) -> RawRow {
    let mut cols = BTreeMap::new();
    cols.insert("email".into(), Value::Text(email.into()));
    cols.insert("id".into(), Value::Int(id));
    RawRow { columns: cols }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(1000))]

    #[test]
    fn round_trip_is_bijective(
        emails in proptest::collection::vec("[a-z]{1,20}@[a-z]{1,10}\\.com", 1..50),
        ids in proptest::collection::vec(any::<i64>(), 1..50),
    ) {
        let a = anonymizer();
        let n = emails.len().min(ids.len());
        let mut fakes = Vec::new();

        for i in 0..n {
            let cleaned = a.clean(raw(&emails[i], ids[i]));
            let fake_email = cleaned.columns()["email"].as_str().unwrap().to_string();
            fakes.push(fake_email);
        }

        for i in 0..n {
            prop_assert_eq!(
                a.raw_for_fake("email", &fakes[i]),
                Some(emails[i].clone())
            );
        }
    }
}
