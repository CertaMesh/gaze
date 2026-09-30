//! Declared evidence also ranks cross-class containment (review F1). A container from an emitter that declares nothing is `Learned`,
//! so it no longer swallows an enclosed rule candidate of another class: the
//! tokens split around the rule span. Declaring `Rule` keeps the v0.15
//! behaviour of one container token. Either way no raw byte ships. Synthetic
//! values only.

use gaze::*;
use regex::Regex;

const INPUT: &str = "Ticket: Acme Holdings a.b@acme.example Berlin.";
const ORGANIZATION: &str = "Acme Holdings a.b@acme.example Berlin";
const EMAIL: &str = "a.b@acme.example";

/// An adopter recognizer spanning the whole organization line.
struct Outer(Option<EvidenceKind>);

impl Recognizer for Outer {
    fn id(&self) -> &str {
        "adopter.organization"
    }
    fn supported_class(&self) -> &PiiClass {
        &PiiClass::Organization
    }
    fn token_family(&self) -> &str {
        "counter"
    }
    fn evidence(&self) -> EvidenceKind {
        self.0.unwrap_or_default()
    }
    fn detect(
        &self,
        input: &str,
        _: &DetectContext<'_>,
    ) -> std::result::Result<Vec<Candidate>, gaze_types::DetectError> {
        Ok(found(
            input,
            ORGANIZATION,
            PiiClass::Organization,
            self.id(),
        ))
    }
}

/// A rule finding the email inside the organization span.
struct Inner;

impl Recognizer for Inner {
    fn id(&self) -> &str {
        "rule.email"
    }
    fn supported_class(&self) -> &PiiClass {
        &PiiClass::Email
    }
    fn token_family(&self) -> &str {
        "counter"
    }
    fn evidence(&self) -> EvidenceKind {
        EvidenceKind::Rule
    }
    fn detect(
        &self,
        input: &str,
        _: &DetectContext<'_>,
    ) -> std::result::Result<Vec<Candidate>, gaze_types::DetectError> {
        Ok(found(input, EMAIL, PiiClass::Email, self.id()))
    }
}

fn found(input: &str, needle: &str, class: PiiClass, id: &str) -> Vec<Candidate> {
    input
        .find(needle)
        .map(|start| {
            Candidate::new(
                start..start + needle.len(),
                class,
                id,
                0.9,
                0,
                None,
                "counter",
                id,
                ConflictTier::None,
                vec![],
            )
        })
        .into_iter()
        .collect()
}

/// Token classes in output order.
fn clean(outer: Option<EvidenceKind>) -> Vec<String> {
    let pipeline = Pipeline::builder()
        .recognizer(Outer(outer))
        .recognizer(Inner)
        .rule(DefaultRule::new(Action::Tokenize))
        .build()
        .unwrap();
    let session = Session::new(Scope::Ephemeral).unwrap();
    let CleanDocument::Text(text) = pipeline
        .redact(&session, RawDocument::Text(INPUT.into()))
        .unwrap()
    else {
        panic!("text in, text out");
    };
    assert_eq!(session.restore_strict_text(&text).unwrap(), INPUT);
    assert!(!text.contains(EMAIL), "{text}");
    assert!(!text.contains("Acme"), "{text}");
    assert!(
        text.starts_with("Ticket: <") && text.ends_with(">."),
        "{text}"
    );
    Regex::new(r"<[0-9a-f]{8}:([A-Za-z]+)_\d+>")
        .unwrap()
        .captures_iter(&text)
        .map(|caps| caps[1].to_string())
        .collect()
}

#[test]
fn a_rule_container_swallows_an_enclosed_rule_candidate() {
    assert_eq!(clean(Some(EvidenceKind::Rule)), ["Organization"]);
}

#[test]
fn an_undeclared_container_splits_around_an_enclosed_rule_candidate() {
    assert_eq!(clean(None), ["Organization", "Email", "Organization"]);
}
