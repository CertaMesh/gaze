//! Behavior guards for the classes the shipped rulepacks emit.
//!
//! Each class keeps its `custom:<name>` wire behavior through the public pipeline: token
//! label, audit class, strict restore, generalization, conflict priority and containment.
//! The tests only use APIs that also existed before the classes became first-class variants,
//! so the same file proves the old and the new type behave alike.

use std::sync::{Arc, Mutex};

use gaze::{
    Action, CleanDocument, Context, DictionaryBundle, LocaleChain, PiiClass, Policy, RawDocument,
    RedactionEntry, RedactionLogError, RedactionLogger, RestoreEventKind, RuleSpec, Rulepack,
    RulepackSource, SafetyNetPolicy, Scope, Session,
};
use gaze_assembly::{build_pipeline, build_pipeline_builder};

/// Every class name the shipped rulepacks emit.
const SHIPPED_CLASS_NAMES: &[&str] = &[
    "postal_code",
    "phone",
    "security_token",
    "password",
    "credit_card",
    "vat_id",
    "tax_number",
    "ssn",
    "national_id",
    "ip_address",
    "iban",
    "driver_license",
    "birth_date",
    "age",
    "url",
    "steuer_id",
    "passport",
    "pan",
    "nir",
    "nino",
    "nhs_number",
    "eth_address",
    "cpf",
    "cnpj",
    "bsn",
    "aadhaar",
    "customer_id",
    "employee_id",
    "record_id",
];

/// Documented example IBAN with a valid mod-97 checksum and only two letters.
const SYNTHETIC_IBAN: &str = "DE89370400440532013000";

#[derive(Clone, Default)]
struct Rows(Arc<Mutex<Vec<RedactionEntry>>>);

impl RedactionLogger for Rows {
    fn log(&self, entry: &RedactionEntry) -> Result<(), RedactionLogError> {
        self.0.lock().unwrap().push(entry.clone());
        Ok(())
    }
}

/// One regex recognizer of the guard rulepack.
struct Rule<'a> {
    id: &'a str,
    class: &'a str,
    pattern: &'a str,
    priority: i32,
    validator: Option<&'a str>,
}

fn rule<'a>(id: &'a str, class: &'a str, pattern: &'a str, priority: i32) -> Rule<'a> {
    Rule {
        id,
        class,
        pattern,
        priority,
        validator: None,
    }
}

fn pipeline(rules: &[Rule<'_>], action: Action, rows: &Rows) -> gaze::Pipeline {
    let mut toml = String::from(
        "schema_version = \"0.1.0\"\nrulepack_id = \"first-class-guards\"\n\
         rulepack_version = \"0.1.0\"\ndefault_locales = [\"global\"]\n",
    );
    for rule in rules {
        toml.push_str(&format!(
            "\n[[recognizers]]\nid = \"{}\"\nclass = \"{}\"\nenabled = true\n\
             locale_basis = \"document\"\nlocales = [\"global\"]\nsafety_tier = \"safe_default\"\n\
             \n[recognizers.match]\nkind = \"regex\"\npattern = '''{}'''\n\
             \n[recognizers.scoring]\nbase = 0.90\npriority = {}\n",
            rule.id, rule.class, rule.pattern, rule.priority
        ));
        if let Some(kind) = rule.validator {
            toml.push_str(&format!("\n[recognizers.validator]\nkind = \"{kind}\"\n"));
        }
    }
    let pack = Rulepack::load(RulepackSource::Embedded(Box::leak(toml.into_boxed_str())))
        .expect("guard rulepack loads");
    let mut policy = Policy::default();
    policy.rules = vec![RuleSpec::Default { action }];
    let context = Context::from_json_str("{}").unwrap();
    let locales = LocaleChain::merge_policy_and_cli(None, None);
    build_pipeline_builder(&policy, &context, &[pack], &locales, None)
        .expect("builder")
        .redaction_logger(rows.clone())
        .build()
        .expect("pipeline")
}

fn clean(pipeline: &gaze::Pipeline, session: &Session, raw: &str) -> String {
    let locales = LocaleChain::merge_policy_and_cli(None, None);
    let (clean, _, _) = pipeline
        .clean_with_safety_net_policy_detect_context(
            session,
            RawDocument::Text(raw.to_string()),
            locales.as_slice(),
            &DictionaryBundle::default(),
            SafetyNetPolicy::default(),
        )
        .expect("clean");
    let CleanDocument::Text(text) = clean else {
        panic!("expected text")
    };
    text
}

#[test]
fn every_shipped_class_keeps_its_wire_behavior() {
    let mut failures = Vec::new();
    for name in SHIPPED_CLASS_NAMES {
        let policy_name = format!("custom:{name}");
        let class = PiiClass::from_policy_name(&policy_name).expect("policy class");
        let value = format!("tenant-{}-xyz", name.replace('_', "-"));
        let raw = format!("Value {value} is on file.");
        let rules = [rule("guard.term", &policy_name, &value, 50)];

        let rows = Rows::default();
        let session = Session::new(Scope::Ephemeral).unwrap();
        let text = clean(&pipeline(&rules, Action::Tokenize, &rows), &session, &raw);
        let winners = rows
            .0
            .lock()
            .unwrap()
            .iter()
            .filter(|row| !row.conflict_loser)
            .map(|row| row.class.clone())
            .collect::<Vec<_>>();
        let generalized = clean(
            &pipeline(&rules, Action::Generalize, &Rows::default()),
            &Session::new(Scope::Ephemeral).unwrap(),
            &raw,
        );

        let checks = [
            (
                "token label",
                text.contains(&format!(":Custom:{name}_1>")) && !text.contains(&value),
            ),
            (
                "class name",
                class.class_name() == format!("Custom:{name}")
                    && class.to_canonical_str() == policy_name,
            ),
            (
                "audit class",
                winners.len() == 1
                    && winners[0] == class
                    && winners[0].to_canonical_str() == policy_name,
            ),
            (
                "strict restore",
                session.restore_strict_text(&text).ok().as_deref() == Some(raw.as_str()),
            ),
            (
                "generalize",
                generalized == format!("Value [{}] is on file.", name.to_ascii_uppercase()),
            ),
        ];
        for (check, passed) in checks {
            if !passed {
                failures.push(format!(
                    "{name}: {check} (clean {text:?}, generalized {generalized:?}, winners {winners:?})"
                ));
            }
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

/// A shipped class shares the adopter-class priority: a built-in class wins a partial
/// overlap on class priority alone, and an adopter class ties so rule priority decides. The
/// winner keeps its whole term; the loser keeps only its remainder.
#[test]
fn shipped_classes_keep_the_custom_class_priority() {
    let raw = "Ref delta echo foxtrot now.";
    for name in SHIPPED_CLASS_NAMES {
        let shipped = format!("custom:{name}");
        let cases = [
            ("location", 90, 10, "location", "echo foxtrot"),
            (
                "custom:tenant_ref",
                10,
                90,
                "custom:tenant_ref",
                "echo foxtrot",
            ),
            ("custom:tenant_ref", 90, 10, shipped.as_str(), "delta echo"),
        ];
        for (rival, shipped_priority, rival_priority, winner, winner_value) in cases {
            let rules = [
                rule("guard.shipped", &shipped, "delta echo", shipped_priority),
                rule("guard.rival", rival, "echo foxtrot", rival_priority),
            ];
            let session = Session::new(Scope::Ephemeral).unwrap();
            let text = clean(
                &pipeline(&rules, Action::Tokenize, &Rows::default()),
                &session,
                raw,
            );
            let held = session
                .snapshot_entries()
                .into_iter()
                .find(|entry| entry.class.to_canonical_str() == winner)
                .map(|entry| entry.raw);
            assert_eq!(
                held.as_deref(),
                Some(winner_value),
                "{name} vs {rival} (rule priorities {shipped_priority}/{rival_priority}): {text}"
            );
            assert_eq!(session.restore_strict_text(&text).unwrap(), raw, "{name}");
        }
    }
}

/// A shipped class that fully encloses a validated built-in span keeps the whole span through
/// the structured-containment rung, although `Email` has the higher class priority and the
/// stronger evidence.
#[test]
fn shipped_class_containers_keep_an_enclosed_builtin_span() {
    let raw = "See ref alice@example.invalid end now.";
    for name in SHIPPED_CLASS_NAMES {
        let shipped = format!("custom:{name}");
        let rules = [
            rule(
                "guard.container",
                &shipped,
                r"ref alice@example\.invalid end",
                50,
            ),
            Rule {
                validator: Some("email_rfc"),
                ..rule("guard.enclosed", "email", r"alice@example\.invalid", 50)
            },
        ];
        let session = Session::new(Scope::Ephemeral).unwrap();
        let text = clean(
            &pipeline(&rules, Action::Tokenize, &Rows::default()),
            &session,
            raw,
        );
        let entries = session.snapshot_entries();
        assert_eq!(entries.len(), 1, "{name}: one container token: {text}");
        assert_eq!(
            entries[0].class.to_canonical_str(),
            shipped,
            "{name}: {text}"
        );
        assert_eq!(entries[0].raw, "ref alice@example.invalid end", "{name}");
        assert_eq!(session.restore_strict_text(&text).unwrap(), raw, "{name}");
    }
}

/// A checksum-failed IBAN or card in the manifest is authorized at the restore boundary, so
/// echoing it back is a manifest bypass rather than fresh PII.
#[test]
fn manifest_iban_and_card_values_are_authorized_at_the_restore_boundary() {
    for (name, stored, echoed) in [
        (
            "iban",
            "DE00 3704 0044 0532 0130 00",
            "IBAN DE00370400440532013000",
        ),
        (
            "credit_card",
            "4111 1111 1111 1112",
            "card 4111-1111-1111-1112",
        ),
    ] {
        let class = PiiClass::from_policy_name(&format!("custom:{name}")).unwrap();
        let fresh = Session::new(Scope::Ephemeral).unwrap();
        let kinds = |session: &Session| {
            session
                .restore_boundary_events(echoed)
                .into_iter()
                .map(|event| event.kind)
                .collect::<Vec<_>>()
        };
        assert_eq!(
            kinds(&fresh),
            [RestoreEventKind::FreshPiiDetected],
            "{name}: an unknown value is fresh PII"
        );
        let session = Session::new(Scope::Ephemeral).unwrap();
        session.tokenize(&class, stored).unwrap();
        assert_eq!(
            kinds(&session),
            [RestoreEventKind::ManifestBypass],
            "{name}: a manifest value is authorized"
        );
    }
}

#[test]
#[ignore = "needs the context.rs IBAN precursor"]
fn policy_mapped_iban_record_is_accepted_and_tokenized() {
    let class = PiiClass::from_policy_name("custom:iban").expect("iban policy class");
    assert_eq!(class.to_canonical_str(), "custom:iban");

    let context = Context::from_json_str(&format!(
        r#"{{"record":{{"account":"{SYNTHETIC_IBAN}"}},"field_map":{{"/account":"custom:iban"}}}}"#
    ))
    .expect("record context");
    assert!(
        context.record_value_rejections.is_empty(),
        "a checksum-valid IBAN record must not be refused as too short"
    );

    let mut policy = Policy::default();
    policy.rules = vec![RuleSpec::Default {
        action: Action::Tokenize,
    }];
    let locales = LocaleChain::merge_policy_and_cli(None, None);
    let pipeline = build_pipeline(&policy, &context, &[], &locales, None).expect("record pipeline");
    let session = Session::new(Scope::Ephemeral).unwrap();
    let raw = format!("Account {SYNTHETIC_IBAN} is on file.");
    let bundle = gaze::dictionary_bundle_from_context(&context).unwrap();
    let CleanDocument::Text(clean) = pipeline
        .pseudonymize_with_detect_context(
            &session,
            RawDocument::Text(raw.clone()),
            locales.as_slice(),
            &bundle,
        )
        .unwrap()
    else {
        panic!("expected text")
    };
    assert!(!clean.contains(SYNTHETIC_IBAN));
    assert!(clean.contains("Custom:iban"), "clean text: {clean}");
    assert_eq!(
        pipeline
            .restore_with_telemetry(&session, &clean)
            .unwrap()
            .0
            .text,
        raw
    );
}
