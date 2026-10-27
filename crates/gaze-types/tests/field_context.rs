use gaze_types::{
    Candidate, ConflictTier, DetectContext, DictionaryBundle, FieldContext, FieldEvidence,
    FieldPath, FieldProvenance, JsonScalarType, PiiClass,
};

#[test]
fn public_constructor_keeps_legacy_context_and_candidate_unset() {
    let dictionaries = DictionaryBundle::default();
    let context = DetectContext::new(&[], &dictionaries);
    let fields: Option<FieldContext<'_>> = context.fields;
    assert_eq!(fields, None);
    let candidate = Candidate::new(
        0..1,
        PiiClass::Email,
        "synthetic",
        1.0,
        0,
        None,
        "Email",
        "rule",
        ConflictTier::RulePriority,
        vec![],
    );
    assert_eq!(candidate.outcome, None);
}

#[test]
fn public_builder_accepts_whole_fields_and_spans() {
    let dictionaries = DictionaryBundle::default();
    let field = FieldProvenance::new(
        FieldPath::new(["customer_id".into()]),
        JsonScalarType::String,
        FieldEvidence::CallerDeclared,
    );
    let spans = [(0..5, field.clone())];
    for fields in [
        FieldContext::WholeInput(&field),
        FieldContext::Spans(&spans),
    ] {
        let context = DetectContext::new(&[], &dictionaries).with_fields(fields);
        assert_eq!(context.fields, Some(fields));
    }
}
