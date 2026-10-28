use serde_json::json;

use super::*;

fn rows<'a>(
    text: &'a str,
    spans: &[(Range<usize>, FieldProvenance)],
) -> Vec<(&'a str, Vec<String>, JsonScalarType, FieldEvidence)> {
    spans
        .iter()
        .map(|(range, field)| {
            (
                &text[range.clone()],
                field
                    .path
                    .segments()
                    .into_iter()
                    .map(str::to_owned)
                    .collect(),
                field.json_type,
                field.evidence,
            )
        })
        .collect()
}

fn path(segments: &[&str]) -> Vec<String> {
    segments
        .iter()
        .map(|segment| (*segment).to_owned())
        .collect()
}

fn caller(value: serde_json::Value) -> CallerFields {
    CallerFields::from_map(value.as_object().unwrap())
}

#[test]
fn strict_json_documents_yield_every_scalar_leaf_in_source_spelling() {
    let text = "\n {\"customer_id\":\"C-731284\",\"n\":[731284,true,null,\"\"],\"q\":\"a\\\"b\",\"straße\":\"Müller\"}";
    let spans = document_field_spans(text, &CallerFields::default());
    use FieldEvidence::ValidatedParse as V;
    assert_eq!(
        rows(text, &spans),
        [
            (
                "C-731284",
                path(&["customer_id"]),
                JsonScalarType::String,
                V
            ),
            ("731284", path(&["n", "0"]), JsonScalarType::Number, V),
            ("true", path(&["n", "1"]), JsonScalarType::Bool, V),
            ("null", path(&["n", "2"]), JsonScalarType::Null, V),
            ("a\\\"b", path(&["q"]), JsonScalarType::String, V),
            ("Müller", path(&["straße"]), JsonScalarType::String, V),
        ]
    );
}

#[test]
fn documents_that_do_not_parse_strictly_get_no_parse_context() {
    for text in [
        "",
        "customer_id: C-731284",
        "\"C-731284\"",
        "731284",
        "{\"customer_id\":\"C-731284\"",
        "{\"customer_id\":\"A\",\"customer_id\":\"B\"}",
        "{\"customer_id\":\"C-731284\"} trailing prose",
        "{'customer_id': 'C-731284'}",
        "prefix {\"customer_id\":\"C-731284\"}",
    ] {
        assert!(
            document_field_spans(text, &CallerFields::default()).is_empty(),
            "{text:?} must fall back to legacy detection"
        );
    }
}

#[test]
fn caller_fields_locate_declared_values_without_cutting_words() {
    let fields = caller(json!({
        "customer": {"id": "C-731284", "tags": ["vip", 731284]},
        "flag": true,
        "empty": "",
        "nothing": null
    }));
    assert_eq!(
        fields.len(),
        3,
        "booleans, nulls and empty strings are not locatable"
    );
    let text = "Ticket C-731284 for vip; vipers and XC-731284 and 7312840 stay; total 731284.";
    let spans = document_field_spans(text, &fields);
    use FieldEvidence::CallerDeclared as C;
    let id = || path(&["customer", "id"]);
    let tag = || path(&["customer", "tags", "0"]);
    let number = || path(&["customer", "tags", "1"]);
    // A hyphen is a word boundary, so the declared number inside `XC-731284` is located too;
    // `XC-731284`, `vipers` and `7312840` would cut a word and are not.
    assert_eq!(
        rows(text, &spans),
        [
            ("C-731284", id(), JsonScalarType::String, C),
            ("731284", number(), JsonScalarType::Number, C),
            ("vip", tag(), JsonScalarType::String, C),
            ("731284", number(), JsonScalarType::Number, C),
            ("731284", number(), JsonScalarType::Number, C),
        ]
    );
    let starts: Vec<_> = spans.iter().map(|(range, _)| range.start).collect();
    assert_eq!(
        starts,
        [
            7,
            9,
            text.find("vip").unwrap(),
            text.find("XC-").unwrap() + 3,
            text.rfind("731284").unwrap(),
        ]
    );
}

#[test]
fn parsed_and_caller_spans_merge_in_document_order() {
    let text = r#"{"note":"call C-1","id":"C-1"}"#;
    let spans = document_field_spans(text, &caller(json!({"ref": "C-1"})));
    let starts: Vec<_> = spans
        .iter()
        .map(|(range, field)| (range.start, field.evidence))
        .collect();
    let note = text.find("call").unwrap();
    let first = text.find("C-1").unwrap();
    let second = text.rfind("C-1").unwrap();
    assert_eq!(
        starts,
        [
            (note, FieldEvidence::ValidatedParse),
            (first, FieldEvidence::CallerDeclared),
            (second, FieldEvidence::ValidatedParse),
            (second, FieldEvidence::CallerDeclared),
        ]
    );
}

#[test]
fn caller_field_debug_never_shows_keys_or_values() {
    let fields = caller(json!({"private_key_name": "private-value"}));
    let debug = format!("{fields:?}");
    assert!(!debug.contains("private"), "{debug}");
    assert!(debug.contains("count: 1"));
}

#[test]
fn spans_project_onto_the_normalized_view() {
    // NBSP folds to a space (2 bytes -> 1), a joiner disappears, fullwidth digits fold to ASCII.
    let text = "{\"a\":\"x\u{00A0}y\",\"b\":\"j\u{200D}k\",\"c\":\"\u{FF11}\u{FF12}\"}";
    let normalized = crate::normalize::normalize(text);
    let spans = document_field_spans(text, &CallerFields::default());
    let projected = project_to_normalized(&spans, &normalized.spans, &normalized.text);
    let values: Vec<_> = projected
        .iter()
        .map(|(range, _)| &normalized.text[range.clone()])
        .collect();
    assert_eq!(values, ["x y", "jk", "12"]);
}

#[test]
fn projection_drops_ranges_that_would_split_a_normalized_character() {
    let text = "ab\u{00A0}cd";
    let normalized = crate::normalize::normalize(text);
    let field = FieldProvenance::new(
        FieldPath::new(["k".to_owned()]),
        JsonScalarType::String,
        FieldEvidence::CallerDeclared,
    );
    // Starts inside the two-byte NBSP: no normalized byte lies wholly inside, so the
    // range shrinks to "cd" rather than claiming the folded space.
    let inside = vec![(3..6, field.clone()), (2..3, field)];
    let projected = project_to_normalized(&inside, &normalized.spans, &normalized.text);
    let values: Vec<_> = projected
        .iter()
        .map(|(range, _)| &normalized.text[range.clone()])
        .collect();
    assert_eq!(values, ["cd"]);
}
