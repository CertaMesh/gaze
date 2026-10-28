//! Shared behavioral probes for transport and pipeline tests.

use std::sync::{Arc, Mutex};

use crate::{Action, Candidate, DefaultRule, DetectContext, PiiClass, Pipeline, Recognizer};
use gaze_types::{DetectError, FieldContext, FieldEvidence, FieldProvenance, JsonScalarType};

#[derive(Clone, Default)]
pub struct FieldObserver {
    seen: Arc<Mutex<Vec<FieldObservation>>>,
    post: bool,
}

struct FieldObservation {
    input: String,
    field: FieldProvenance,
    post: bool,
    offset: Option<usize>,
}

impl Recognizer for FieldObserver {
    fn id(&self) -> &str {
        if self.post {
            "test.field.post"
        } else {
            "test.field"
        }
    }
    fn supported_class(&self) -> &PiiClass {
        &PiiClass::Name
    }
    fn token_family(&self) -> &str {
        "counter"
    }
    fn requires_prior_candidates(&self) -> bool {
        self.post
    }
    fn detect(&self, input: &str, ctx: &DetectContext<'_>) -> Result<Vec<Candidate>, DetectError> {
        let (field, offset) = match ctx.fields {
            Some(FieldContext::WholeInput(field)) => (field, None),
            Some(FieldContext::Fragment { field, offset }) => (field, Some(offset)),
            _ => return Ok(Vec::new()),
        };
        self.seen.lock().unwrap().push(FieldObservation {
            input: input.to_owned(),
            field: field.clone(),
            post: self.post,
            offset,
        });
        Ok(Vec::new())
    }
}

impl FieldObserver {
    pub fn builder(&self) -> crate::PipelineBuilder {
        let mut post = self.clone();
        post.post = true;
        Pipeline::builder()
            .recognizer(self.clone())
            .recognizer(post)
    }
    pub fn pipeline(&self) -> Pipeline {
        self.builder()
            .rule(DefaultRule::new(Action::Preserve))
            .build()
            .unwrap()
    }
    pub fn retained_path_bytes(&self) -> usize {
        let seen = self.seen.lock().unwrap();
        retained_path_bytes(seen.iter().map(|row| &row.field.path))
    }
    pub fn assert_fragment(&self, input: &str, expected_offset: usize) {
        let seen = self.seen.lock().unwrap();
        for post in [false, true] {
            let rows: Vec<_> = seen
                .iter()
                .filter(|row| row.input == input && row.post == post)
                .collect();
            assert!(!rows.is_empty());
            assert!(rows.iter().all(|row| row.offset == Some(expected_offset)));
        }
    }
    pub fn assert_field(&self, input: &str, segments: &[&str]) {
        let seen = self.seen.lock().unwrap();
        for post in [false, true] {
            let fields: Vec<_> = seen
                .iter()
                .filter(|row| row.input == input && row.post == post && row.offset.is_none())
                .collect();
            assert!(
                !fields.is_empty(),
                "field evidence must reach both recognizer passes"
            );
            for row in fields {
                let field = &row.field;
                assert_eq!(field.evidence, FieldEvidence::ValidatedParse);
                assert_eq!(field.json_type, JsonScalarType::String);
                assert!(
                    field
                        .path
                        .segments()
                        .iter()
                        .copied()
                        .eq(segments.iter().copied()),
                    "unescaped structural path must match"
                );
                assert!(!format!("{field:?}").contains(input));
            }
        }
    }
}

/// Count actual retained segment allocations once, including shared ancestors.
/// All supplied paths remain alive while their allocation addresses are inspected.
pub fn retained_path_bytes<'a>(
    paths: impl IntoIterator<Item = &'a gaze_types::FieldPath>,
) -> usize {
    let allocations: std::collections::HashSet<_> = paths
        .into_iter()
        .flat_map(|path| {
            path.segments()
                .into_iter()
                .map(|part| (part.as_ptr(), part.len()))
        })
        .collect();
    allocations.into_iter().map(|(_, length)| length).sum()
}

#[derive(Clone, Default)]
pub struct AuditCapture(Arc<Mutex<Vec<crate::RedactionEntry>>>);

impl crate::RedactionLogger for AuditCapture {
    fn log(&self, entry: &crate::RedactionEntry) -> Result<(), crate::RedactionLogError> {
        self.0.lock().unwrap().push(entry.clone());
        Ok(())
    }
}

impl AuditCapture {
    /// Compare every serialized audit field except the wall-clock timestamp.
    pub fn rows_without_clock(&self) -> Vec<serde_json::Value> {
        self.0
            .lock()
            .unwrap()
            .iter()
            .map(|entry| {
                let mut row = serde_json::to_value(entry).unwrap();
                row.as_object_mut().unwrap().remove("created_at");
                row
            })
            .collect()
    }
}
