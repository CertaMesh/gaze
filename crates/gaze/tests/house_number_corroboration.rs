//! Street-corroborated house numbers through the whole pipeline (todo 3670).
//!
//! A stub stands in for the NER recognizer: it reports the given words as
//! locations under the recognizer id `ner`, exactly as the Davlan backend does.
use gaze::*;
use std::sync::{Arc, Mutex};

struct Spans {
    id: &'static str,
    class: PiiClass,
    words: Vec<&'static str>,
}

impl Recognizer for Spans {
    fn id(&self) -> &str {
        self.id
    }
    fn supported_class(&self) -> &PiiClass {
        &self.class
    }
    fn token_family(&self) -> &str {
        "counter"
    }
    fn detect(
        &self,
        input: &str,
        _: &DetectContext<'_>,
    ) -> std::result::Result<Vec<Candidate>, gaze_types::DetectError> {
        Ok(self
            .words
            .iter()
            .filter_map(|word| input.find(word).map(|start| start..start + word.len()))
            .map(|span| {
                Candidate::new(
                    span,
                    self.class.clone(),
                    self.id,
                    0.9,
                    0,
                    None,
                    "counter",
                    self.id,
                    ConflictTier::None,
                    vec![],
                )
            })
            .collect())
    }
}

fn ner(words: &[&'static str]) -> Spans {
    Spans {
        id: "ner",
        class: PiiClass::Location,
        words: words.to_vec(),
    }
}

#[derive(Default, Clone)]
struct Rows(Arc<Mutex<Vec<RedactionEntry>>>);

impl RedactionLogger for Rows {
    fn log(&self, entry: &RedactionEntry) -> std::result::Result<(), RedactionLogError> {
        self.0.lock().unwrap().push(entry.clone());
        Ok(())
    }
}

fn builder(recognizers: Vec<Spans>, rows: &Rows) -> PipelineBuilder {
    let mut builder = Pipeline::builder()
        .register_street_lexicon(
            LocaleTag::DeDe,
            StreetNumberOrder::NumberAfter,
            vec!["weg".into(), "straße".into()],
        )
        .register_street_lexicon(
            LocaleTag::EnUs,
            StreetNumberOrder::NumberBefore,
            vec!["street".into()],
        )
        .rule(DefaultRule::new(Action::Tokenize))
        .redaction_logger(rows.clone());
    for recognizer in recognizers {
        builder = builder.recognizer(recognizer);
    }
    builder
}

/// Cleans `raw`, checks the exact restore round trip, and returns the raw
/// substrings that were tokenized.
fn tokenized(pipeline: &Pipeline, raw: &str) -> Vec<String> {
    let session = Session::new(Scope::Ephemeral).unwrap();
    let (clean, spans, _) = pipeline
        .clean_with_safety_net_policy_detect_context(
            &session,
            RawDocument::Text(raw.into()),
            &[LocaleTag::DeDe, LocaleTag::EnUs],
            &DictionaryBundle::default(),
            SafetyNetPolicy::default(),
        )
        .unwrap();
    let CleanDocument::Text(text) = clean else {
        panic!("text document")
    };
    assert_eq!(session.restore_strict_text(&text).unwrap(), raw);
    spans
        .into_iter()
        .map(|span| raw[span.raw_span].to_string())
        .collect()
}

#[test]
fn german_house_number_is_tokenized_beside_a_ner_street() {
    let rows = Rows::default();
    let pipeline = builder(vec![ner(&["Musterweg"])], &rows).build().unwrap();

    assert_eq!(
        tokenized(&pipeline, "Bitte an Musterweg 17b, 10115 Berlin"),
        ["Musterweg", "17b"]
    );

    let rows = rows.0.lock().unwrap();
    let number = rows
        .iter()
        .find(|row| row.recognizer_id.as_deref() == Some(HOUSE_NUMBER_RECOGNIZER_ID))
        .expect("house number has its own audit row");
    assert_eq!(number.class, PiiClass::Location);
    assert!(!number.conflict_loser);
}

#[test]
fn english_house_number_is_tokenized_before_a_ner_street() {
    let pipeline = builder(vec![ner(&["Example Street"])], &Rows::default())
        .build()
        .unwrap();
    assert_eq!(
        tokenized(&pipeline, "Ship to 17 Example Street today"),
        ["17", "Example Street"]
    );
}

#[test]
fn without_a_lexicon_the_number_is_left_alone() {
    let pipeline = Pipeline::builder()
        .recognizer(ner(&["Musterweg"]))
        .rule(DefaultRule::new(Action::Tokenize))
        .build()
        .unwrap();
    assert_eq!(tokenized(&pipeline, "Musterweg 17b"), ["Musterweg"]);
}

#[test]
fn a_city_licenses_no_number() {
    let pipeline = builder(vec![ner(&["Berlin"])], &Rows::default())
        .build()
        .unwrap();
    assert_eq!(tokenized(&pipeline, "Berlin 2026 Einwohner"), ["Berlin"]);
}

#[test]
fn a_street_found_by_another_recognizer_licenses_no_number() {
    let dictionary_street = Spans {
        id: "dictionary.street",
        class: PiiClass::Location,
        words: vec!["Musterweg"],
    };
    let pipeline = builder(vec![dictionary_street], &Rows::default())
        .build()
        .unwrap();
    assert_eq!(tokenized(&pipeline, "Musterweg 17b"), ["Musterweg"]);
}

#[test]
fn a_ner_street_that_resolves_to_another_class_licenses_no_number() {
    // A name candidate on the same span outranks the location, so the settled
    // span is a name and no street evidence remains.
    let name = Spans {
        id: "name.fixed",
        class: PiiClass::Name,
        words: vec!["Musterweg"],
    };
    let pipeline = builder(vec![ner(&["Musterweg"]), name], &Rows::default())
        .build()
        .unwrap();
    assert_eq!(tokenized(&pipeline, "Musterweg 17b"), ["Musterweg"]);
}

#[test]
fn a_number_already_covered_is_not_added_twice() {
    let number = Spans {
        id: "number.fixed",
        class: PiiClass::custom("postal_code").unwrap(),
        words: vec!["17b"],
    };
    let rows = Rows::default();
    let pipeline = builder(vec![ner(&["Musterweg"]), number], &rows)
        .build()
        .unwrap();
    assert_eq!(tokenized(&pipeline, "Musterweg 17b"), ["Musterweg", "17b"]);
    assert!(!rows
        .0
        .lock()
        .unwrap()
        .iter()
        .any(|row| row.recognizer_id.as_deref() == Some(HOUSE_NUMBER_RECOGNIZER_ID)));
}

#[test]
fn every_other_detection_is_unchanged_by_the_second_resolution() {
    let email = Spans {
        id: "email.fixed",
        class: PiiClass::Email,
        words: vec!["a@example.org"],
    };
    let pipeline = builder(vec![ner(&["Musterweg", "Berlin"]), email], &Rows::default())
        .build()
        .unwrap();
    assert_eq!(
        tokenized(&pipeline, "a@example.org, Musterweg 3, Berlin"),
        ["a@example.org", "Musterweg", "3", "Berlin"]
    );
}

#[test]
fn a_ner_street_inside_an_organization_licenses_no_number() {
    // Containment: the organization wholly contains the NER street and wins
    // the span, so the settled selection is an organization, not a street.
    let org = Spans {
        id: "org.fixed",
        class: PiiClass::Organization,
        words: vec!["Bäckerei am Musterweg"],
    };
    let pipeline = builder(vec![ner(&["Musterweg"]), org], &Rows::default())
        .build()
        .unwrap();
    assert_eq!(
        tokenized(&pipeline, "Bäckerei am Musterweg 17"),
        ["Bäckerei am Musterweg"]
    );
}
