//! Bundled `field_labelled` record-ID rules on whole-JSON documents through the full `core`
//! pipeline: labelled strings protect and restore byte-exactly with the JSON shape intact,
//! labelled numbers refuse with every refused field named, and unlabelled values, keys and
//! repeats in other carriers stay usable. All values are synthetic.
use gaze::{
    CleanDocument, Context, DictionaryBundle, Error, FieldPath, LocaleChain, Pipeline, Policy,
    ProtectionError, RawDocument, Rulepack, RulepackSource, SafetyNetPolicy, Scope, Session,
    UntransformableReason,
};
use gaze_assembly::build_pipeline;
use serde_json::Value;

const LOCALES: &str = r#"["de-DE", "en-US", "en-GB", "global"]"#;

fn pipeline(outcomes: &str, context: &str) -> (Pipeline, LocaleChain) {
    let text = format!(
        "schema_version = \"0.1.0\"\n\n[session]\nscope = \"persistent\"\nttl_secs = 86400\n\n\
         [policy.rulepacks]\nbundled = [\"core\", \"locale-de\", \"locale-en\"]\n\n\
         [locale]\nactive = {LOCALES}\n\n{outcomes}\n[[rule]]\nkind = \"default\"\naction = \"tokenize\"\n"
    );
    let path = std::env::temp_dir().join(format!(
        "gaze-field-labelled-{}-{:?}.toml",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::write(&path, text).expect("write policy");
    let policy = Policy::load(&path).expect("policy");
    let _ = std::fs::remove_file(&path);
    let packs = ["core", "locale-de", "locale-en"].map(|name| {
        Rulepack::load(RulepackSource::Embedded(
            gaze_recognizers::embedded(name).expect("embedded rulepack"),
        ))
        .expect("rulepack")
    });
    let context = Context::from_json_str(context).expect("context");
    let active = LocaleChain::merge_policy_and_cli(policy.locale.as_deref(), None);
    let pipeline = build_pipeline(&policy, &context, &packs, &active, None).expect("pipeline");
    (pipeline, active)
}

fn stock() -> (Pipeline, LocaleChain) {
    pipeline("", r#"{"dictionaries":{},"class_map":{},"fields":{}}"#)
}

fn clean(
    pipeline: &Pipeline,
    active: &LocaleChain,
    session: &Session,
    input: &str,
) -> gaze::Result<String> {
    let (clean, _, _) = pipeline.clean_with_safety_net_policy_detect_context(
        session,
        RawDocument::Text(input.to_string()),
        active.as_slice(),
        &DictionaryBundle::default(),
        SafetyNetPolicy::default(),
    )?;
    let CleanDocument::Text(text) = clean else {
        panic!("expected text");
    };
    Ok(text)
}

/// Same keys, same tree shape, same scalar types; only string leaves may change.
fn same_shape(before: &Value, after: &Value) -> bool {
    match (before, after) {
        (Value::Object(a), Value::Object(b)) => {
            a.len() == b.len()
                && a.iter()
                    .all(|(key, value)| b.get(key).is_some_and(|other| same_shape(value, other)))
        }
        (Value::Array(a), Value::Array(b)) => {
            a.len() == b.len() && a.iter().zip(b).all(|(a, b)| same_shape(a, b))
        }
        (Value::String(_), Value::String(_)) => true,
        (a, b) => a == b,
    }
}

/// Clean, check the JSON shape and strict restore, and return the cleaned text.
fn protected(pipeline: &Pipeline, active: &LocaleChain, input: &str) -> String {
    let session = Session::new(Scope::Ephemeral).expect("session");
    let cleaned = clean(pipeline, active, &session, input).expect("clean");
    let before: Value = serde_json::from_str(input).expect("fixture is JSON");
    let after: Value = serde_json::from_str(&cleaned)
        .unwrap_or_else(|error| panic!("output must stay JSON ({error}): {cleaned}"));
    assert!(
        same_shape(&before, &after),
        "shape changed: {input} -> {cleaned}"
    );
    let restored = pipeline
        .restore_strict_text(&session, &cleaned)
        .expect("restore");
    assert_eq!(restored, input, "restore must be byte-exact");
    cleaned
}

fn refusal(pipeline: &Pipeline, active: &LocaleChain, input: &str) -> ProtectionError {
    let session = Session::new(Scope::Ephemeral).expect("session");
    match clean(pipeline, active, &session, input) {
        Err(Error::Protection(error)) => error,
        other => panic!("{input} must refuse, got {other:?}"),
    }
}

fn path(segments: &[&str]) -> FieldPath {
    FieldPath::new(segments.iter().map(|segment| segment.to_string()))
}

#[test]
fn labelled_strings_protect_with_shape_and_restore() {
    let (pipeline, active) = stock();
    for (input, raw) in [
        (r#"{"customer_id":"C-731284"}"#, "C-731284"),
        (r#"{"customerId":"C-731284","limit":731284}"#, "C-731284"),
        (r#"{"Kundennummer":"K-731284"}"#, "K-731284"),
        (
            r#"{"record":{"record":{"employee_id":"E-731284"}}}"#,
            "E-731284",
        ),
        (r#"{"items":[{"staffNumber":"S-731284"}]}"#, "S-731284"),
        (
            r#"{"customer":{"id":"C-731284","name":"Acme"}}"#,
            "C-731284",
        ),
        (r#"{"member_id":["M-731284","M-731285"]}"#, "M-731284"),
        (
            r#"{"contact_record_id":"3f2a9c1e-77b4-4d1a-9e2f-0a1b2c3d4e5f"}"#,
            "3f2a9c1e",
        ),
        (r#"{"customer_id":"SYN\/61234–567"}"#, "61234"),
        (r#"{"customer_id":"C\"731284"}"#, "731284"),
        (r#"{"personnel_number":"731284"}"#, "731284"),
    ] {
        let cleaned = protected(&pipeline, &active, input);
        assert!(!cleaned.contains(raw), "{raw} leaked: {cleaned}");
    }
}

#[test]
fn labelled_numbers_refuse_and_name_every_refused_field() {
    let (pipeline, active) = stock();
    let error = refusal(
        &pipeline,
        &active,
        r#"{"customer_id":731284,"limit":20,"items":[{"employeeId":44190}]}"#,
    );
    let ProtectionError::Untransformable {
        reason,
        field_path,
        others,
    } = error
    else {
        panic!("typed refusal expected, got {error:?}");
    };
    assert_eq!(reason, UntransformableReason::JsonNumberNoTextOnlyPath);
    assert_eq!(field_path, path(&["customer_id"]));
    assert_eq!(
        others,
        vec![(
            UntransformableReason::JsonNumberNoTextOnlyPath,
            path(&["items", "0", "employeeId"])
        )]
    );
}

#[test]
fn unlabelled_values_keys_and_literals_stay_unchanged() {
    let (pipeline, active) = stock();
    for input in [
        r#"{"limit":731284,"offset":20,"order_id":"O-731284"}"#,
        r#"{"invoiceNo":"INV-731284","requestId":"req-731284"}"#,
        r#"{"customer_id_label":"x","customer_id":null,"customerNumber":true}"#,
        r#"{"id":"C-731284","value":"731284"}"#,
    ] {
        assert_eq!(protected(&pipeline, &active, input), input);
    }
}

#[test]
fn repeats_of_a_bound_value_keep_keys_numbers_and_syntax_valid() {
    let (pipeline, active) = stock();
    for input in [
        r#"{"customer_id":"731284","note":"call about 731284 today"}"#,
        r#"{"customer_id":"C-731284","other":{"customer_id":"C-731284"}}"#,
        r#"{"customer_id":"12","limit":12,"page":1}"#,
        r#"{"customer_id":"C-2026","year":2026,"qty":3}"#,
    ] {
        let cleaned = protected(&pipeline, &active, input);
        assert!(!cleaned.contains("731284"), "repeat leaked: {cleaned}");
    }
}

/// D6: a repeat of a bound ID never becomes a token inside a JSON number or object key. An
/// exact whole-literal repeat refuses, typed, before any output; overlap with a different
/// literal is left alone; short values never trigger it.
#[test]
fn repeats_into_numbers_and_keys_refuse_typed() {
    let (pipeline, active) = stock();
    let number = refusal(
        &pipeline,
        &active,
        r#"{"customer_id":"731284","limit":731284}"#,
    );
    assert!(
        matches!(
            &number,
            ProtectionError::Untransformable {
                reason: UntransformableReason::JsonNumberNoTextOnlyPath,
                field_path,
                others,
            } if *field_path == path(&["limit"]) && others.is_empty()
        ),
        "{number:?}"
    );
    let key = refusal(
        &pipeline,
        &active,
        r#"{"customer_id":"731284","731284":"x"}"#,
    );
    assert!(
        matches!(
            &key,
            ProtectionError::Untransformable {
                reason: UntransformableReason::NoSafeRendering,
                field_path,
                ..
            } if *field_path == path(&["731284"])
        ),
        "{key:?}"
    );
    // A different number that merely contains the ID's digits is another value.
    let input = r#"{"customer_id":"731284","ratio":731284.5}"#;
    let cleaned = protected(&pipeline, &active, input);
    assert!(cleaned.contains("731284.5"), "{cleaned}");
    // Second turn: the session's bound value repeats as a number in a later document.
    let session = Session::new(Scope::Ephemeral).expect("session");
    clean(&pipeline, &active, &session, r#"{"customer_id":"731284"}"#).expect("first turn");
    match clean(&pipeline, &active, &session, r#"{"page":2,"limit":731284}"#) {
        Err(Error::Protection(ProtectionError::Untransformable { reason, .. })) => {
            assert_eq!(reason, UntransformableReason::JsonNumberNoTextOnlyPath)
        }
        other => panic!("second-turn repeat must refuse, got {other:?}"),
    }
}

#[test]
fn possibly_sensitive_values_follow_the_outcomes_disposition() {
    let input = r#"{"customer_id":"pending review","limit":3}"#;
    let (default, active) = stock();
    let cleaned = protected(&default, &active, input);
    assert!(
        !cleaned.contains("pending review"),
        "D1 protects: {cleaned}"
    );

    let (refuse, active) = pipeline(
        "[outcomes]\npossibly_sensitive = \"refuse\"\n",
        r#"{"dictionaries":{},"class_map":{},"fields":{}}"#,
    );
    match refusal(&refuse, &active, input) {
        ProtectionError::Untransformable {
            reason, field_path, ..
        } => {
            assert_eq!(reason, UntransformableReason::NoSafeRendering);
            assert_eq!(field_path, path(&["customer_id"]));
        }
        other => panic!("typed refusal expected, got {other:?}"),
    }

    let (pass, active) = pipeline(
        "[outcomes]\npossibly_sensitive = \"pass_audited\"\n",
        r#"{"dictionaries":{},"class_map":{},"fields":{}}"#,
    );
    assert_eq!(protected(&pass, &active, input), input);
}

#[test]
fn caller_declared_fields_bind_in_prose_and_refuse_in_numbers() {
    let (pipeline, active) = pipeline(
        "",
        r#"{"dictionaries":{},"class_map":{},"fields":{"customer_id":"C-731284","employee_id":44190}}"#,
    );
    let session = Session::new(Scope::Ephemeral).expect("session");
    let cleaned = clean(
        &pipeline,
        &active,
        &session,
        "Please look up C-731284 today.",
    )
    .expect("prose protects");
    assert!(!cleaned.contains("731284"), "{cleaned}");
    assert!(matches!(
        refusal(&pipeline, &active, r#"{"count":44190}"#),
        ProtectionError::Untransformable {
            reason: UntransformableReason::JsonNumberNoTextOnlyPath,
            ..
        }
    ));
}

#[derive(Clone, Default)]
struct Rows(std::sync::Arc<std::sync::Mutex<Vec<gaze::RedactionEntry>>>);

impl gaze::RedactionLogger for Rows {
    fn log(&self, entry: &gaze::RedactionEntry) -> Result<(), gaze::RedactionLogError> {
        self.0.lock().unwrap().push(entry.clone());
        Ok(())
    }
}

/// Field-bound winners, refusals and passes each write a value-free audit row with the
/// outcome, its reason, and the field's depth and evidence.
#[test]
fn audit_rows_carry_outcomes_and_field_evidence() {
    use gaze_types::{FieldEvidence, PossiblySensitiveReason, ProtectionOutcome};
    let custom = |name: &str| gaze::PiiClass::custom(name).unwrap();
    let rows = Rows::default();
    let (pipeline, active) = stock();
    let pipeline = pipeline.with_redaction_logger(rows.clone());
    protected(
        &pipeline,
        &active,
        r#"{"record":{"customer_id":"C-731284"}}"#,
    );
    refusal(&pipeline, &active, r#"{"employee_id":44190}"#);
    let entries = std::mem::take(&mut *rows.0.lock().unwrap());
    let winner = entries
        .iter()
        .find(|row| row.class == custom("customer_id") && !row.conflict_loser)
        .expect("winner row");
    assert_eq!(
        winner.protection_outcome().unwrap(),
        Some(ProtectionOutcome::Protected)
    );
    assert_eq!(winner.field_path_depth, Some(2));
    assert_eq!(winner.field_evidence, Some(FieldEvidence::ValidatedParse));
    let refused = entries
        .iter()
        .find(|row| row.class == custom("employee_id"))
        .expect("refusal row");
    assert_eq!(
        refused.protection_outcome().unwrap(),
        Some(ProtectionOutcome::ConfirmedUntransformable(
            UntransformableReason::JsonNumberNoTextOnlyPath
        ))
    );
    assert_eq!(refused.field_path_depth, Some(1));
    let serialized = serde_json::to_string(&entries).unwrap();
    assert!(!serialized.contains("731284") && !serialized.contains("44190"));

    let rows = Rows::default();
    let (pass, active) = pipeline_for_probe("[outcomes]\npossibly_sensitive = \"pass_audited\"\n");
    let pass = pass.with_redaction_logger(rows.clone());
    protected(&pass, &active, r#"{"customer_id":"pending review"}"#);
    let entries = rows.0.lock().unwrap();
    let passed = entries
        .iter()
        .find(|row| row.class == custom("customer_id"))
        .expect("pass row");
    assert_eq!(passed.action, gaze::Action::Preserve);
    assert_eq!(
        passed.protection_outcome().unwrap(),
        Some(ProtectionOutcome::PossiblySensitive(
            PossiblySensitiveReason::LabelValueShapeUnclear
        ))
    );
}

fn pipeline_for_probe(outcomes: &str) -> (Pipeline, LocaleChain) {
    pipeline(
        outcomes,
        r#"{"dictionaries":{},"class_map":{},"fields":{}}"#,
    )
}

/// D6 cost: a bound all-digit ID that coincides with an unlabelled quantity, page, year or
/// six-digit number in the same document. The sweep's digit-run floor (six digits) keeps
/// short shapes from ever refusing; six-digit coincidences refuse, typed. Counts are printed
/// by document and by affected value for the PR evidence and pinned here.
#[test]
fn coincident_unlabelled_numbers_refuse_only_at_the_sweep_floor() {
    let (pipeline, active) = stock();
    let shapes: [(&str, Vec<u64>); 4] = [
        ("quantity", (1..=60).collect()),
        ("page", (1..=60).map(|page| page * 7).collect()),
        ("year", (1990..=2049).collect()),
        (
            "six_digit",
            (0..60).map(|step| 100_000 + step * 14_983).collect(),
        ),
    ];
    for (shape, values) in shapes {
        let (mut documents, mut refused_documents, mut refused_values) = (0, 0, 0);
        for value in &values {
            let input = format!(
                r#"{{"customer_id":"{value}","{shape}":{value},"page_size":{value},"note":"ok"}}"#
            );
            documents += 1;
            let session = Session::new(Scope::Ephemeral).expect("session");
            match clean(&pipeline, &active, &session, &input) {
                Err(Error::Protection(ProtectionError::Untransformable { others, .. })) => {
                    refused_documents += 1;
                    refused_values += 1 + others.len();
                }
                Ok(cleaned) => {
                    serde_json::from_str::<Value>(&cleaned).expect("stays JSON");
                }
                Err(other) => panic!("{shape} {value}: {other:?}"),
            }
        }
        println!(
            "D6_COST shape={shape} documents={documents} refused_documents={refused_documents} \
             benign_values={} refused_values={refused_values}",
            documents * 2
        );
        let expected = if shape == "six_digit" { documents } else { 0 };
        assert_eq!(refused_documents, expected, "{shape}");
        assert_eq!(
            refused_values,
            expected * 2,
            "{shape}: both coincident values refuse"
        );
    }
}
