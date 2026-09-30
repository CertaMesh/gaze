//! Address-block growth through the whole pipeline.
//!
//! Stubs stand in for the address winners: a NER stub reports words as
//! locations under the recognizer id `ner`, and a postcode stub reports digit
//! runs as `custom:postal_code` under `postal.us`, as the bundled rule does.
//! Every value is synthetic: invented street and city names and ZIPs in the
//! unassigned `000xx` range.
use gaze::*;
use proptest::prelude::*;
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
            .flat_map(|word| {
                input
                    .match_indices(word)
                    .map(|(start, _)| start..start + word.len())
                    .collect::<Vec<_>>()
            })
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
        id: NER_RECOGNIZER_ID,
        class: PiiClass::Location,
        words: words.to_vec(),
    }
}

fn postcode(words: &[&'static str]) -> Spans {
    Spans {
        id: "postal.us",
        class: PiiClass::custom("postal_code").unwrap(),
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

fn names(list: &[&str]) -> Vec<String> {
    list.iter().map(|name| name.to_string()).collect()
}

fn builder(recognizers: Vec<Spans>, rows: &Rows) -> PipelineBuilder {
    let mut builder = Pipeline::builder()
        .register_address_vocabulary(
            LocaleTag::EnUs,
            AddressVocabulary::UnitDesignators,
            names(&["suite", "apt.", "apt", "unit", "po box", "psc", "cmr"]),
        )
        .register_address_vocabulary(
            LocaleTag::EnUs,
            AddressVocabulary::RegionCodes,
            names(&["IL", "OR", "AE", "AP", "AA"]),
        )
        .register_address_vocabulary(
            LocaleTag::EnUs,
            AddressVocabulary::MilitaryPostOffices,
            names(&["APO", "FPO", "DPO"]),
        )
        .register_address_vocabulary(
            LocaleTag::DeDe,
            AddressVocabulary::UnitDesignators,
            names(&["wohnung", "postfach"]),
        )
        .register_address_vocabulary(
            LocaleTag::DeDe,
            AddressVocabulary::UnitDesignatorsNumberBefore,
            names(&["etage", "og"]),
        )
        .register_street_lexicon(
            LocaleTag::EnUs,
            StreetNumberOrder::NumberBefore,
            vec!["street".into(), "lane".into()],
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
            &[LocaleTag::EnUs, LocaleTag::DeDe],
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
fn a_whole_us_address_is_protected_with_its_unit_and_state() {
    let rows = Rows::default();
    let pipeline = builder(
        vec![ner(&["Drusk Street", "Brinmoor"]), postcode(&["00096"])],
        &rows,
    )
    .build()
    .unwrap();

    assert_eq!(
        tokenized(
            &pipeline,
            "Ship to 117 Drusk Street Suite 522, Brinmoor, IL 00096 by Friday."
        ),
        [
            "117",
            "Drusk Street",
            "Suite 522",
            "Brinmoor",
            "IL",
            "00096"
        ]
    );

    let rows = rows.0.lock().unwrap();
    for (id, value) in [
        (ADDRESS_UNIT_RECOGNIZER_ID, "Suite 522"),
        (ADDRESS_REGION_CODE_RECOGNIZER_ID, "IL"),
    ] {
        let row = rows
            .iter()
            .find(|row| row.recognizer_id.as_deref() == Some(id))
            .unwrap_or_else(|| panic!("{value} has its own audit row"));
        assert_eq!(row.class, PiiClass::Location);
        assert!(!row.conflict_loser);
        assert_eq!(
            AddressGrowth::from_recognizer_id(id).map(AddressGrowth::recognizer_id),
            Some(id)
        );
    }
}

#[test]
fn a_military_address_is_protected_from_its_zip_alone() {
    let pipeline = builder(vec![postcode(&["00090"])], &Rows::default())
        .build()
        .unwrap();
    assert_eq!(
        tokenized(&pipeline, "Ship to:\nPSC 806, Box 9504\nFPO AA 00090\n"),
        ["PSC 806", "Box 9504", "FPO", "AA", "00090"]
    );
}

#[test]
fn a_german_address_grows_over_etage_and_postfach() {
    let pipeline = builder(
        vec![
            ner(&["Pellinorallee", "Kornhelm"]),
            postcode(&["00937", "00724"]),
        ],
        &Rows::default(),
    )
    .build()
    .unwrap();
    assert_eq!(
        tokenized(
            &pipeline,
            "Das Büro ist in der Pellinorallee, 2. OG, 00937 Kornhelm."
        ),
        ["Pellinorallee", "2. OG", "00937", "Kornhelm"]
    );
    assert_eq!(
        tokenized(&pipeline, "Antwort an Postfach 505, 00724 Kornhelm"),
        ["Postfach 505", "00724", "Kornhelm"]
    );
}

#[test]
fn a_designator_without_an_address_stays_raw() {
    let pipeline = builder(
        vec![ner(&["Brinmoor"]), postcode(&["00068"])],
        &Rows::default(),
    )
    .build()
    .unwrap();
    assert!(tokenized(&pipeline, "Run test Suite 431 before merging.").is_empty());
    assert!(tokenized(&pipeline, "Minutes from PSC 311 are attached.").is_empty());
    assert_eq!(
        tokenized(
            &pipeline,
            "Deliver to Brinmoor, IL 00068.\nThe regression Suite 810 is still red."
        ),
        ["Brinmoor", "IL", "00068"]
    );
}

#[test]
fn only_address_winners_start_growth() {
    let name = Spans {
        id: NER_RECOGNIZER_ID,
        class: PiiClass::Name,
        words: vec!["Kalvik"],
    };
    let pipeline = builder(vec![name], &Rows::default()).build().unwrap();
    assert_eq!(
        tokenized(&pipeline, "Ask Kalvik, Suite 5 is booked."),
        ["Kalvik"]
    );
}

#[test]
fn without_a_vocabulary_nothing_grows() {
    let pipeline = Pipeline::builder()
        .recognizer(postcode(&["00096"]))
        .rule(DefaultRule::new(Action::Tokenize))
        .build()
        .unwrap();
    assert_eq!(
        tokenized(&pipeline, "Brinmoor, IL 00096, Suite 5"),
        ["00096"]
    );
}

#[test]
fn a_grown_piece_is_not_swept_into_a_later_document_of_the_session() {
    // The session manifest seeds the sweep with its non-learned entries only;
    // a grown piece is learned-tier, so a later bare `Suite 522` stays raw.
    let pipeline = builder(vec![ner(&["Drusk Street"])], &Rows::default())
        .build()
        .unwrap();
    let session = Session::new(Scope::Ephemeral).unwrap();
    let clean = |raw: &str| {
        let (_, spans, _) = pipeline
            .clean_with_safety_net_policy_detect_context(
                &session,
                RawDocument::Text(raw.into()),
                &[LocaleTag::EnUs],
                &DictionaryBundle::default(),
                SafetyNetPolicy::default(),
            )
            .unwrap();
        spans
            .into_iter()
            .map(|span| raw[span.raw_span].to_string())
            .collect::<Vec<_>>()
    };
    assert_eq!(
        clean("Drusk Street Suite 522 is new."),
        ["Drusk Street", "Suite 522"]
    );
    assert!(clean("We booked Suite 522 for the review.").is_empty());
}

#[test]
fn a_grown_piece_is_never_swept_to_another_copy() {
    // `Suite 522` is learned-tier evidence: its copy with no address beside
    // it in the same document stays raw.
    let pipeline = builder(vec![ner(&["Drusk Street"])], &Rows::default())
        .build()
        .unwrap();
    assert_eq!(
        tokenized(
            &pipeline,
            "Drusk Street Suite 522 is new. We booked Suite 522 for the review."
        ),
        ["Drusk Street", "Suite 522"]
    );
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 256, ..ProptestConfig::default() })]

    /// Any mix of address pieces, designators, separators and noise cleans
    /// without a panic and restores byte for byte.
    #[test]
    fn address_text_never_panics_and_restores_exactly(
        parts in proptest::collection::vec(
            prop_oneof![
                Just("Drusk Street"), Just("Brinmoor"), Just("00096"), Just("Suite 5"),
                Just("Apt. 4B"), Just("IL"), Just("PSC 806"), Just("Box 12"), Just("APO"),
                Just("AE"), Just("2. OG"), Just("Postfach 7"), Just("Suite"), Just("#"),
                Just(", "), Just(" "), Just("\n"), Just("."), Just("ä"), Just("\u{a0}"),
                Just("\u{202f}"), Just("12"), Just("x"),
            ],
            0..24,
        )
    ) {
        let pipeline = builder(
            vec![ner(&["Drusk Street", "Brinmoor"]), postcode(&["00096"])],
            &Rows::default(),
        )
        .build()
        .unwrap();
        let raw = parts.concat();
        let _ = tokenized(&pipeline, &raw);
    }
}
