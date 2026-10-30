//! Whole-document field context reaches recognizers in detection-input coordinates.

use std::sync::{Arc, Mutex};

use gaze::{
    Action, CallerFields, Candidate, CleanDocument, DefaultRule, DetectContext, DictionaryBundle,
    LocaleTag, PiiClass, Pipeline, RawDocument, Recognizer, SafetyNetPolicy, Scope, Session,
};
use gaze_types::{DetectError, FieldContext, FieldEvidence, JsonScalarType};
use serde_json::json;

type Seen = (String, Vec<String>, JsonScalarType, FieldEvidence);

/// Records each detection input's field spans as (input slice, path, type, evidence).
#[derive(Clone, Default)]
struct SpanObserver(Arc<Mutex<Vec<Option<Vec<Seen>>>>>);

impl Recognizer for SpanObserver {
    fn id(&self) -> &str {
        "test.field.spans"
    }
    fn supported_class(&self) -> &PiiClass {
        &PiiClass::Name
    }
    fn token_family(&self) -> &str {
        "counter"
    }
    fn detect(&self, input: &str, ctx: &DetectContext<'_>) -> Result<Vec<Candidate>, DetectError> {
        let seen = match ctx.fields {
            None => None,
            Some(FieldContext::Spans(spans)) => Some(
                spans
                    .iter()
                    .map(|(range, field)| {
                        (
                            input[range.clone()].to_owned(),
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
                    .collect(),
            ),
            Some(_) => panic!("a whole document carries spans, never a single field"),
        };
        self.0.lock().unwrap().push(seen);
        Ok(Vec::new())
    }
}

impl SpanObserver {
    fn pipeline(&self, caller: CallerFields) -> Pipeline {
        Pipeline::builder()
            .recognizer(self.clone())
            .rule(DefaultRule::new(Action::Preserve))
            .caller_fields(caller)
            .build()
            .unwrap()
    }

    /// Clean `text` and return the one field observation the detection pass made.
    fn clean(&self, pipeline: &Pipeline, text: &str) -> Option<Vec<Seen>> {
        self.0.lock().unwrap().clear();
        let session = Session::new(Scope::Ephemeral).unwrap();
        let (clean, _, _) = pipeline
            .clean_with_safety_net_policy_detect_context(
                &session,
                RawDocument::Text(text.to_owned()),
                &[LocaleTag::Global],
                &DictionaryBundle::default(),
                SafetyNetPolicy::default(),
            )
            .unwrap();
        assert!(matches!(clean, CleanDocument::Text(ref clean) if clean == text));
        let seen = self.0.lock().unwrap();
        assert_eq!(seen.len(), 1, "one detection pass per document");
        seen[0].clone()
    }
}

fn seen(value: &str, path: &[&str], json_type: JsonScalarType, evidence: FieldEvidence) -> Seen {
    (
        value.to_owned(),
        path.iter().map(|segment| (*segment).to_owned()).collect(),
        json_type,
        evidence,
    )
}

#[test]
fn strict_json_document_reaches_recognizers_as_parsed_field_spans() {
    let observer = SpanObserver::default();
    let pipeline = observer.pipeline(CallerFields::default());
    // NBSP and fullwidth digits fold in the detection input; the spans follow them.
    let text = "{\"customer_id\":\"C\u{00A0}\u{FF17}31284\",\"items\":[{\"qty\":2}]}";
    use FieldEvidence::ValidatedParse as V;
    assert_eq!(
        observer.clean(&pipeline, text),
        Some(vec![
            seen("C 731284", &["customer_id"], JsonScalarType::String, V),
            seen("2", &["items", "0", "qty"], JsonScalarType::Number, V),
        ])
    );
}

#[test]
fn documents_without_a_strict_parse_keep_legacy_detection_context() {
    let observer = SpanObserver::default();
    let pipeline = observer.pipeline(CallerFields::default());
    for text in [
        "Customer C-731284 called.",
        "{\"customer_id\":\"C-731284\",\"customer_id\":\"C-1\"}",
        "{\"customer_id\":\"C-731284\"",
        "\"C-731284\"",
    ] {
        assert_eq!(observer.clean(&pipeline, text), None, "{text:?}");
    }
}

#[test]
fn caller_declared_fields_reach_recognizers_in_any_document() {
    let observer = SpanObserver::default();
    let fields = json!({"customer": {"id": "C-731284"}});
    let pipeline = observer.pipeline(CallerFields::from_map(fields.as_object().unwrap()));
    use FieldEvidence::CallerDeclared as C;
    assert_eq!(
        observer.clean(&pipeline, "Customer C-731284 called."),
        Some(vec![seen(
            "C-731284",
            &["customer", "id"],
            JsonScalarType::String,
            C
        )])
    );
    assert_eq!(observer.clean(&pipeline, "Nobody called."), None);
}

#[test]
fn request_pipelines_carry_caller_fields_with_their_detection_policy() {
    let observer = SpanObserver::default();
    let fields = json!({"id": "C-731284"});
    let request = observer.pipeline(CallerFields::from_map(fields.as_object().unwrap()));
    let resident = observer.pipeline(CallerFields::default());
    assert!(resident.caller_fields().is_empty());
    assert_eq!(
        resident
            .clone()
            .with_detection_policy_from(&request)
            .caller_fields()
            .len(),
        1
    );
    assert_eq!(
        resident
            .with_caller_fields(request.caller_fields().clone())
            .caller_fields()
            .len(),
        1
    );
}

/// Work counter, no timing: a JSON-shaped document is parsed exactly once per clean, and
/// other text is never handed to the parser.
#[cfg(feature = "perf-timers")]
#[test]
fn whole_document_parse_runs_once_per_clean() {
    let observer = SpanObserver::default();
    let pipeline = observer.pipeline(CallerFields::default());
    for (text, parses) in [
        ("{\"a\":[1,2,{\"b\":\"c\"}]}", 1),
        ("[\"not\", \"strict\",]", 1),
        ("plain text with {braces}", 0),
    ] {
        let capture = gaze_types::perf::Capture::new();
        observer.clean(&pipeline, text);
        let record = capture.finish();
        assert_eq!(
            record
                .counters
                .get("document_json_parses")
                .copied()
                .unwrap_or(0),
            parses,
            "{text:?}"
        );
    }
}
