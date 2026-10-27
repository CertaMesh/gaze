use std::sync::{Arc, Mutex};

use gaze::{Action, Candidate, DefaultRule, DetectContext, PiiClass, Pipeline, Recognizer};
use gaze_types::{DetectError, FieldContext, FieldEvidence, FieldProvenance, JsonScalarType};

#[derive(Clone, Default)]
pub struct FieldObserver {
    seen: Arc<Mutex<Vec<(String, FieldProvenance, bool)>>>,
    post: bool,
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
        if let Some(FieldContext::WholeInput(field)) = ctx.fields {
            self.seen
                .lock()
                .unwrap()
                .push((input.to_owned(), field.clone(), self.post));
        }
        Ok(Vec::new())
    }
}

impl FieldObserver {
    pub fn pipeline(&self) -> Pipeline {
        let mut post = self.clone();
        post.post = true;
        Pipeline::builder()
            .recognizer(self.clone())
            .recognizer(post)
            .rule(DefaultRule::new(Action::Preserve))
            .build()
            .unwrap()
    }
    pub fn assert_field(&self, input: &str, segments: &[&str]) {
        let seen = self.seen.lock().unwrap();
        for post in [false, true] {
            let fields: Vec<_> = seen
                .iter()
                .filter(|(value, _, pass)| value == input && *pass == post)
                .collect();
            assert!(
                !fields.is_empty(),
                "field evidence must reach both recognizer passes"
            );
            for (_, field, _) in fields {
                assert_eq!(field.evidence, FieldEvidence::ValidatedParse);
                assert_eq!(field.json_type, JsonScalarType::String);
                assert!(
                    field
                        .path
                        .segments()
                        .iter()
                        .map(String::as_str)
                        .eq(segments.iter().copied()),
                    "unescaped structural path must match"
                );
                assert!(!format!("{field:?}").contains(input));
            }
        }
    }
}
