use std::cell::Cell;

use gaze::{
    Action, ClassRule, CleanDocument, DefaultRule, DetectContext, DictionaryBundle, LocaleTag,
    PiiClass, Pipeline, RawDocument, RawMatch, Recognizer, RecognizerSpec, Rulepack, RulepackError,
    RulepackSource, Scope, Session,
};
use gaze_recognizers::{embedded, RegexDetector};
use serde_json::Map;

fn core_extended() -> Rulepack {
    Rulepack::load(RulepackSource::Embedded(
        embedded("core-extended").expect("core-extended embedded rulepack"),
    ))
    .expect("core-extended loads")
}

fn regex_from_spec(spec: &RecognizerSpec) -> RegexDetector {
    let RawMatch::Regex {
        pattern,
        pattern_template: None,
        capture_groups,
    } = &spec.matcher
    else {
        panic!("expected plain regex recognizer {}", spec.id);
    };

    RegexDetector::with_rulepack_fields(
        pattern.as_deref().expect("regex pattern"),
        spec.class.clone(),
        &spec.id,
        spec.locales.clone(),
        spec.scoring.base,
        spec.scoring.priority,
        spec.token.family.as_deref().unwrap_or("counter"),
        capture_groups.clone(),
        spec.context
            .as_ref()
            .map(|context| context.exclusions.clone())
            .unwrap_or_default(),
        None,
        None,
    )
    .expect("regex detector")
}

fn detect_recognizer(
    rulepack: &Rulepack,
    recognizer_id: &str,
    input: &str,
    locale: LocaleTag,
) -> Vec<String> {
    let dictionaries = DictionaryBundle::default();
    let fields = Map::new();
    let ctx = DetectContext {
        locale_chain: &[locale, LocaleTag::Global],
        dictionaries: &dictionaries,
        fields: &fields,
        degraded: Cell::new(false),
    };
    let spec = rulepack
        .recognizers
        .iter()
        .find(|recognizer| recognizer.id == recognizer_id)
        .unwrap_or_else(|| panic!("missing recognizer {recognizer_id}"));
    let detector = regex_from_spec(spec);

    Recognizer::detect(&detector, input, &ctx)
        .into_iter()
        .map(|candidate| input[candidate.span].to_string())
        .collect()
}

fn pipeline_from_rulepack(rulepack: &Rulepack) -> Pipeline {
    let mut builder = Pipeline::builder()
        .rule(ClassRule::new(PiiClass::Email, Action::Tokenize))
        .rule(ClassRule::new(
            PiiClass::Custom("phone".to_string()),
            Action::Tokenize,
        ))
        .rule(ClassRule::new(
            PiiClass::Custom("ip_address".to_string()),
            Action::Tokenize,
        ))
        .rule(ClassRule::new(
            PiiClass::Custom("postal_code".to_string()),
            Action::Tokenize,
        ))
        .rule(DefaultRule::new(Action::Preserve));

    for spec in &rulepack.recognizers {
        if spec.enabled {
            builder = builder.recognizer(regex_from_spec(spec));
        }
    }

    builder.build().expect("pipeline")
}

fn clean_text(pipeline: &Pipeline, session: &Session, input: &str, locale: LocaleTag) -> String {
    let clean = pipeline
        .redact_with_context(
            session,
            RawDocument::Text(input.to_string()),
            &[locale, LocaleTag::Global],
        )
        .expect("clean");
    let CleanDocument::Text(text) = clean else {
        panic!("expected text document");
    };
    text
}

#[test]
fn corpus_accepts_universal_shapes_and_rejects_tenant_like_phone_inputs() {
    let rulepack = core_extended();

    assert_eq!(
        detect_recognizer(
            &rulepack,
            "phone.structural",
            "Phone +4915112345678",
            LocaleTag::DeDe
        ),
        vec!["+4915112345678".to_string()]
    );
    assert_eq!(
        detect_recognizer(&rulepack, "ip.v4", "Host 192.168.1.1.", LocaleTag::EnUs),
        vec!["192.168.1.1".to_string()]
    );
    assert_eq!(
        detect_recognizer(&rulepack, "ip.v6", "Loopback ::1.", LocaleTag::EnUs),
        vec!["::1".to_string()]
    );
    assert_eq!(
        detect_recognizer(&rulepack, "ip.v6", "Host 2001:db8::1.", LocaleTag::EnUs),
        vec!["2001:db8::1".to_string()]
    );
    assert_eq!(
        detect_recognizer(&rulepack, "postal.de", "Berlin 10115", LocaleTag::DeDe),
        vec!["10115".to_string()]
    );
    assert_eq!(
        detect_recognizer(&rulepack, "postal.us", "ZIP 94103", LocaleTag::EnUs),
        vec!["94103".to_string()]
    );
    assert_eq!(
        detect_recognizer(&rulepack, "postal.us", "ZIP 94103-1234", LocaleTag::EnUs),
        vec!["94103-1234".to_string()]
    );

    for input in [
        "1.2.3.4567",
        "version v1.2.3.4",
        "2026-04-25",
        "0815 12345",
        "0123-456789",
        "Subscriber_0001234567",
        "Order_0815",
    ] {
        assert!(
            detect_recognizer(&rulepack, "phone.structural", input, LocaleTag::DeDe).is_empty(),
            "phone.structural must not fire for {input}"
        );
    }
}

#[test]
fn core_and_core_extended_compose_without_counter_collision() {
    let core = Rulepack::load(RulepackSource::Embedded(embedded("core").unwrap())).unwrap();
    let extended = core_extended();
    let mut builder = Pipeline::builder()
        .rule(ClassRule::new(PiiClass::Email, Action::Tokenize))
        .rule(ClassRule::new(
            PiiClass::Custom("phone".to_string()),
            Action::Tokenize,
        ))
        .rule(DefaultRule::new(Action::Preserve));

    let email_spec = core
        .recognizers
        .iter()
        .find(|recognizer| recognizer.id == "email.global")
        .expect("email.global");
    builder = builder.recognizer(regex_from_spec(email_spec));
    let phone_spec = extended
        .recognizers
        .iter()
        .find(|recognizer| recognizer.id == "phone.structural")
        .expect("phone.structural");
    builder = builder.recognizer(regex_from_spec(phone_spec));

    let pipeline = builder.build().expect("pipeline");
    let session = Session::new(Scope::Ephemeral).expect("session");
    let clean = clean_text(
        &pipeline,
        &session,
        "Contact alice@example.invalid or +4915112345678",
        LocaleTag::EnUs,
    );

    let tokens = clean
        .split_whitespace()
        .filter(|part| part.starts_with('<'))
        .collect::<Vec<_>>();
    assert_eq!(tokens.len(), 2);
    assert!(tokens[0].ends_with(":Email_1>"), "{clean}");
    assert!(tokens[1].ends_with(":Custom:phone_1>"), "{clean}");
}

#[test]
fn every_phase1_recognizer_round_trips_through_restore() {
    let rulepack = core_extended();
    let pipeline = pipeline_from_rulepack(&rulepack);

    for (input, locale) in [
        ("Call +4915112345678", LocaleTag::DeDe),
        ("Host 192.168.1.1", LocaleTag::EnUs),
        ("Loopback ::1", LocaleTag::EnUs),
        ("Host 2001:db8::1", LocaleTag::EnUs),
        ("Berlin 10115", LocaleTag::DeDe),
        ("ZIP 94103-1234", LocaleTag::EnUs),
    ] {
        let session = Session::new(Scope::Ephemeral).expect("session");
        let clean = clean_text(&pipeline, &session, input, locale);
        assert_ne!(clean, input, "expected tokenized output for {input}");
        let restored =
            gaze::token_shape::pattern().replace_all(&clean, |captures: &regex::Captures<'_>| {
                session.restore_strict(&captures[0]).expect("known token")
            });
        assert_eq!(restored, input);
    }
}

#[test]
fn embedded_core_extended_load_smoke_has_at_least_five_recognizers() {
    let rulepack = Rulepack::load(RulepackSource::Embedded(
        embedded("core-extended").expect("core-extended embedded rulepack"),
    ))
    .expect("core-extended loads");

    assert!(rulepack.recognizers.len() >= 5);
}

#[test]
fn same_class_cooperation_is_data_and_unilateral_failure_behavior() {
    let raw = embedded("core-extended").unwrap();
    let rulepack = Rulepack::load(RulepackSource::Embedded(raw)).unwrap();
    let ip_v4 = rulepack
        .recognizers
        .iter()
        .find(|recognizer| recognizer.id == "ip.v4")
        .unwrap();
    let ip_v6 = rulepack
        .recognizers
        .iter()
        .find(|recognizer| recognizer.id == "ip.v6")
        .unwrap();
    assert_eq!(ip_v4.cooperates_with, vec!["ip.v6"]);
    assert_eq!(ip_v6.cooperates_with, vec!["ip.v4"]);

    let one_side_removed = raw.replace("cooperates_with = [\"ip.v4\"]\n", "");
    Rulepack::parse(&one_side_removed).expect("one-sided cooperation remains valid");

    let both_sides_removed = one_side_removed.replace("cooperates_with = [\"ip.v6\"]\n", "");
    let err = Rulepack::parse(&both_sides_removed)
        .expect_err("both-side cooperation drop must fail closed");
    assert!(matches!(
        err,
        RulepackError::SameClassWithoutCooperation {
            class: PiiClass::Custom(ref name),
            ..
        } if name == "ip_address"
    ));

    let one_postal_side_removed = raw.replace("cooperates_with = [\"postal.de\"]\n", "");
    Rulepack::parse(&one_postal_side_removed).expect("one-sided postal cooperation remains valid");

    let both_postal_sides_removed =
        one_postal_side_removed.replace("cooperates_with = [\"postal.us\"]\n", "");
    let err = Rulepack::parse(&both_postal_sides_removed)
        .expect_err("both-side postal cooperation drop must fail closed");
    assert!(matches!(
        err,
        RulepackError::SameClassWithoutCooperation {
            class: PiiClass::Custom(ref name),
            ..
        } if name == "postal_code"
    ));
}
