use super::*;
use gaze_types::{DictionaryBundle, FieldPath, HintKind};

fn recognizer() -> FieldLabelRecognizer {
    FieldLabelRecognizer::new(
        "customer_id.field",
        PiiClass::custom("customer_id").unwrap(),
        ["customer_id", "customer_number", "Kundennummer"],
        0.9,
        100,
        "counter",
    )
}

fn path(segments: &[&str]) -> FieldPath {
    FieldPath::new(segments.iter().map(|segment| segment.to_string()))
}

fn spans_for(
    text: &str,
    leaves: &[(&str, &[&str], JsonScalarType, FieldEvidence)],
) -> Vec<(Range<usize>, FieldProvenance)> {
    leaves
        .iter()
        .map(|(value, segments, json_type, evidence)| {
            let start = text.find(value).expect("fixture value present");
            (
                start..start + value.len(),
                FieldProvenance::new(path(segments), *json_type, *evidence),
            )
        })
        .collect()
}

fn detect(text: &str, spans: &[(Range<usize>, FieldProvenance)]) -> Vec<Candidate> {
    let dictionaries = DictionaryBundle::default();
    let ctx = DetectContext::new(&[LocaleTag::Global], &dictionaries)
        .with_fields(FieldContext::Spans(spans));
    recognizer().detect(text, &ctx).unwrap()
}

#[test]
fn label_spellings_and_bindings() {
    let r = recognizer();
    for bound in [
        &["customer_id"][..],
        &["customerId"],
        &["customer-id"],
        &["CUSTOMER ID"],
        &["customerNumber"],
        &["kundennummer"],
        &["record", "customer_id"],
        &["customer_id", "0"],
        &["customer_id", "value"],
        &["customer", "id"],
        &["customer", "number"],
        &["items", "3", "customer", "id"],
        // Arrays between a bound parent and its value key are transparent.
        &["customer_id", "0", "value"],
        &["customer_id", "2", "id"],
        &["customer_id", "0", "1"],
        &["customer_id", "0", "value", "0"],
        &["customer", "0", "id"],
        &["customer", "1", "number"],
        // Labels compare through the shared detection normalization.
        &["\u{ff43}\u{ff55}\u{ff53}\u{ff54}\u{ff4f}\u{ff4d}\u{ff45}\u{ff52}\u{ff3f}\u{ff49}\u{ff44}"],
        &["customer\u{200b}_id"],
        &["cust\u{00ad}omer\u{2060}Id"],
        &["customer\u{3000}id"],
        &["customer_id", "\u{ff56}\u{ff41}\u{ff4c}\u{ff55}\u{ff45}"],
        &["\u{ff23}\u{ff35}\u{ff33}\u{ff34}\u{ff2f}\u{ff2d}\u{ff25}\u{ff32}", "\u{ff49}\u{ff44}"],
    ] {
        assert!(r.binds(bound), "{bound:?} must bind");
    }
    for unbound in [
        &["id"][..],
        &["customer"],
        &["batch_id"],
        &["customer_id", "name"],
        &["customer_id", "0", "name"],
        &["customer", "value"],
        &["customer_id_hash", "x"],
        &["customers", "id"],
        &[],
        // Array transparency never reaches through an unrelated object.
        &["customer_id", "0", "meta", "value"],
        &["customer_id", "0", "name"],
        &["customer", "0", "value"],
        &["customer_id", "value", "id"],
        // Normalized benign keys stay benign.
        &["\u{ff42}\u{ff41}\u{ff54}\u{ff43}\u{ff48}\u{ff3f}\u{ff49}\u{ff44}"],
        &["customer\u{200b}s", "id"],
        &["customer_id\u{200b}_hash"],
        &[
            "\u{ff43}\u{ff55}\u{ff53}\u{ff54}\u{ff4f}\u{ff4d}\u{ff45}\u{ff52}",
            "name",
        ],
    ] {
        assert!(!r.binds(unbound), "{unbound:?} must not bind");
    }
}

#[test]
fn string_outcomes_follow_shape_and_evidence() {
    let text = r#"{"customer_id":"C-731284","customer_number":"unknown","batch_id":"O-1"}"#;
    let parse = FieldEvidence::ValidatedParse;
    let spans = spans_for(
        text,
        &[
            ("C-731284", &["customer_id"], JsonScalarType::String, parse),
            (
                "unknown",
                &["customer_number"],
                JsonScalarType::String,
                parse,
            ),
            ("O-1", &["batch_id"], JsonScalarType::String, parse),
        ],
    );
    let found = detect(text, &spans);
    assert_eq!(found.len(), 2, "the unlabelled order id is not a candidate");
    assert_eq!(&text[found[0].span.clone()], "C-731284");
    assert_eq!(found[0].outcome, Some(ProtectionOutcome::Protected));
    assert_eq!(
        found[0].field.as_ref().unwrap().path,
        path(&["customer_id"])
    );
    assert_eq!(
        found[1].outcome,
        Some(ProtectionOutcome::PossiblySensitive(
            PossiblySensitiveReason::LabelValueShapeUnclear
        ))
    );
}

#[test]
fn overlong_and_many_group_values_are_unclear_but_never_truncated() {
    let long = "C".repeat(10) + &"7".repeat(31);
    let text = format!(r#"{{"customer_id":"{long}","customerNumber":"1-2-3-4-5"}}"#);
    let parse = FieldEvidence::ValidatedParse;
    let spans = spans_for(
        &text,
        &[
            (&long, &["customer_id"], JsonScalarType::String, parse),
            (
                "1-2-3-4-5",
                &["customerNumber"],
                JsonScalarType::String,
                parse,
            ),
        ],
    );
    let found = detect(&text, &spans);
    assert_eq!(found.len(), 2);
    for candidate in &found {
        assert!(matches!(
            candidate.outcome,
            Some(ProtectionOutcome::PossiblySensitive(
                PossiblySensitiveReason::LabelValueShapeUnclear
            ))
        ));
    }
    assert_eq!(&text[found[0].span.clone()], long, "whole leaf, no prefix");
}

#[test]
fn escaped_values_are_shaped_after_decoding_and_cover_source_bytes() {
    let text = r#"{"customer_id":"SYN\/61234–567"}"#;
    let raw = r#"SYN\/61234–567"#;
    let spans = spans_for(
        text,
        &[(
            raw,
            &["customer_id"],
            JsonScalarType::String,
            FieldEvidence::ValidatedParse,
        )],
    );
    let found = detect(text, &spans);
    assert_eq!(found.len(), 1);
    assert_eq!(&text[found[0].span.clone()], raw);
    assert_eq!(found[0].outcome, Some(ProtectionOutcome::Protected));
}

#[test]
fn labelled_numbers_refuse_and_literals_and_unlabelled_numbers_do_not() {
    let text = r#"{"customer_id":731284,"customer_number":true,"limit":731284,"customerId":null}"#;
    let parse = FieldEvidence::ValidatedParse;
    let mut spans = spans_for(
        text,
        &[
            ("731284", &["customer_id"], JsonScalarType::Number, parse),
            ("true", &["customer_number"], JsonScalarType::Bool, parse),
            ("null", &["customerId"], JsonScalarType::Null, parse),
        ],
    );
    let unlabelled = text.rfind("731284").unwrap();
    spans.push((
        unlabelled..unlabelled + 6,
        FieldProvenance::new(path(&["limit"]), JsonScalarType::Number, parse),
    ));
    let found = detect(text, &spans);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].span.start, text.find("731284").unwrap());
    assert_eq!(
        found[0].outcome,
        Some(ProtectionOutcome::ConfirmedUntransformable(
            UntransformableReason::JsonNumberNoTextOnlyPath
        ))
    );
}

#[test]
fn caller_declaration_inside_a_parsed_number_refuses_and_in_text_protects() {
    let text = r#"{"count":731284,"note":"see 731284"}"#;
    let first = text.find("731284").unwrap();
    let second = text.rfind("731284").unwrap();
    let declared = FieldProvenance::new(
        path(&["customer_id"]),
        JsonScalarType::Number,
        FieldEvidence::CallerDeclared,
    );
    let note = text.find("see 731284").unwrap();
    let spans = vec![
        (
            first..first + 6,
            FieldProvenance::new(
                path(&["count"]),
                JsonScalarType::Number,
                FieldEvidence::ValidatedParse,
            ),
        ),
        (first..first + 6, declared.clone()),
        (
            note..note + 10,
            FieldProvenance::new(
                path(&["note"]),
                JsonScalarType::String,
                FieldEvidence::ValidatedParse,
            ),
        ),
        (second..second + 6, declared),
    ];
    let found = detect(text, &spans);
    assert_eq!(found.len(), 2);
    assert!(matches!(
        found[0].outcome,
        Some(ProtectionOutcome::ConfirmedUntransformable(_))
    ));
    assert_eq!(found[1].span, second..second + 6);
    assert_eq!(found[1].outcome, Some(ProtectionOutcome::Protected));
}

#[test]
fn hints_never_protect_and_fragments_never_prove_a_complete_id() {
    let dictionaries = DictionaryBundle::default();
    let hinted = FieldProvenance::new(
        path(&["customer_id"]),
        JsonScalarType::String,
        FieldEvidence::Hint(HintKind::ToolName),
    );
    let ctx = DetectContext::new(&[LocaleTag::Global], &dictionaries)
        .with_fields(FieldContext::WholeInput(&hinted));
    let found = recognizer().detect("C-731284", &ctx).unwrap();
    assert_eq!(
        found[0].outcome,
        Some(ProtectionOutcome::PossiblySensitive(
            PossiblySensitiveReason::LabelBindingUnproven
        ))
    );

    let parsed = FieldProvenance::new(
        path(&["customer_id"]),
        JsonScalarType::String,
        FieldEvidence::ValidatedParse,
    );
    // A fragment cannot prove a complete identifier, but the rest of a labelled leaf is
    // still sensitive: possibly sensitive, never `Protected`.
    let ctx = DetectContext::new(&[LocaleTag::Global], &dictionaries).with_fields(
        FieldContext::Fragment {
            field: &parsed,
            offset: 2,
        },
    );
    let found = recognizer().detect("731284", &ctx).unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].span, 0..6);
    assert_eq!(
        found[0].outcome,
        Some(ProtectionOutcome::PossiblySensitive(
            PossiblySensitiveReason::LabelValueShapeUnclear
        ))
    );

    let ctx = DetectContext::new(&[LocaleTag::Global], &dictionaries)
        .with_fields(FieldContext::WholeInput(&parsed));
    let found = recognizer().detect("C-731284", &ctx).unwrap();
    assert_eq!(found[0].span, 0..8);
    assert_eq!(found[0].outcome, Some(ProtectionOutcome::Protected));
}

#[test]
fn no_field_context_means_no_candidates() {
    let dictionaries = DictionaryBundle::default();
    let ctx = DetectContext::new(&[LocaleTag::Global], &dictionaries);
    assert!(recognizer()
        .detect(r#"customer_id: C-731284"#, &ctx)
        .unwrap()
        .is_empty());
}

#[test]
fn fragments_of_a_bound_leaf_cover_their_sensitive_bytes_only() {
    let dictionaries = DictionaryBundle::default();
    let fragment = |field: &FieldProvenance, input: &str| {
        let ctx = DetectContext::new(&[LocaleTag::Global], &dictionaries)
            .with_fields(FieldContext::Fragment { field, offset: 0 });
        recognizer().detect(input, &ctx).unwrap()
    };
    let bound = FieldProvenance::new(
        path(&["customer_id"]),
        JsonScalarType::String,
        FieldEvidence::ValidatedParse,
    );
    // The gap after an owned token: separators stay, the identifier is covered.
    let found = fragment(&bound, " C-731284.");
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].span, 1..9);
    assert!(matches!(
        found[0].outcome,
        Some(ProtectionOutcome::PossiblySensitive(_))
    ));
    // Separators alone carry no identifier bytes.
    assert!(fragment(&bound, " / ").is_empty());
    assert!(fragment(&bound, "").is_empty());
    // An unbound leaf's fragment is not this recognizer's business.
    let unbound = FieldProvenance::new(
        path(&["note"]),
        JsonScalarType::String,
        FieldEvidence::ValidatedParse,
    );
    assert!(fragment(&unbound, " C-731284").is_empty());
    // A hinted fragment stays unproven.
    let hinted = FieldProvenance::new(
        path(&["customer_id"]),
        JsonScalarType::String,
        FieldEvidence::Hint(HintKind::ToolName),
    );
    assert_eq!(
        fragment(&hinted, "C-731284")[0].outcome,
        Some(ProtectionOutcome::PossiblySensitive(
            PossiblySensitiveReason::LabelBindingUnproven
        ))
    );
}

#[test]
fn array_wrapped_values_bind_through_the_parse() {
    let text =
        r#"{"customer_id":[{"value":"C-731284"},"C-731285"],"items":[{"meta":{"value":"9"}}]}"#;
    let parse = FieldEvidence::ValidatedParse;
    let spans = spans_for(
        text,
        &[
            (
                "C-731284",
                &["customer_id", "0", "value"],
                JsonScalarType::String,
                parse,
            ),
            (
                "C-731285",
                &["customer_id", "1"],
                JsonScalarType::String,
                parse,
            ),
            (
                "9",
                &["items", "0", "meta", "value"],
                JsonScalarType::String,
                parse,
            ),
        ],
    );
    let found = detect(text, &spans);
    assert_eq!(found.len(), 2);
    assert!(found
        .iter()
        .all(|c| c.outcome == Some(ProtectionOutcome::Protected)));
}
