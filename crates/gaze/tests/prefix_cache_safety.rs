#![cfg(feature = "bundled-recognizers")]

use gaze::{
    Action, Candidate, CleanDocument, DefaultRule, DetectContext, DictionaryBundle, LocaleTag,
    PiiClass, Pipeline, RawDocument, Recognizer, Rule, RuleContext, Scope, Session,
};
use gaze_recognizers::RegexDetector;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

struct MutableRule(Arc<AtomicBool>);

impl Rule for MutableRule {
    fn action(&self, _: &PiiClass, _: &RuleContext) -> Option<Action> {
        Some(if self.0.load(Ordering::SeqCst) {
            Action::Tokenize
        } else {
            Action::Preserve
        })
    }
}

struct MutableRecognizer {
    active: Arc<AtomicBool>,
    emails: RegexDetector,
}

impl Recognizer for MutableRecognizer {
    fn id(&self) -> &str {
        "mutable.email.fixture"
    }

    fn supported_class(&self) -> &PiiClass {
        &PiiClass::Email
    }

    fn token_family(&self) -> &str {
        "counter"
    }

    fn detect(
        &self,
        input: &str,
        ctx: &DetectContext<'_>,
    ) -> Result<Vec<Candidate>, gaze_types::DetectError> {
        if self.active.load(Ordering::SeqCst) {
            Recognizer::detect(&self.emails, input, ctx)
        } else {
            Ok(Vec::new())
        }
    }
}

fn check_rescan(kind: &str, staged: bool) {
    let active = Arc::new(AtomicBool::new(false));
    let builder = Pipeline::builder().enable_prefix_cache();
    let pipeline = match kind {
        "boundary" => builder
            .detector(RegexDetector::emails().unwrap())
            .rule(DefaultRule::new(Action::Tokenize)),
        "rule" => builder
            .detector(RegexDetector::emails().unwrap())
            .rule(MutableRule(Arc::clone(&active))),
        "recognizer" => builder
            .recognizer(MutableRecognizer {
                active: Arc::clone(&active),
                emails: RegexDetector::emails().unwrap(),
            })
            .rule(DefaultRule::new(Action::Tokenize)),
        _ => unreachable!(),
    }
    .build()
    .unwrap();
    let session = Session::new(Scope::Ephemeral).unwrap();
    let mut transaction = session.begin_transaction();
    let dictionaries = DictionaryBundle::default();
    let locale = [LocaleTag::Global];
    let mut protect = |text: &str| {
        let raw = RawDocument::Text(text.into());
        let clean = if staged {
            pipeline.pseudonymize_transaction_with_detect_context(
                &mut transaction,
                raw,
                &locale,
                &dictionaries,
            )
        } else {
            pipeline.pseudonymize_with_detect_context(&session, raw, &locale, &dictionaries)
        }
        .unwrap();
        let CleanDocument::Text(text) = clean else {
            panic!("expected text");
        };
        text
    };
    let prefix = if kind == "boundary" {
        "alice@"
    } else {
        "alice@example.invalid"
    };
    assert_eq!(protect(prefix), prefix);
    active.store(true, Ordering::SeqCst);
    let extended = "alice@example.invalid extra";
    // The same pipeline and borrowed request context now require full protection.
    let fresh = Session::new(Scope::Ephemeral).unwrap();
    let control = pipeline
        .pseudonymize_with_detect_context(
            &fresh,
            RawDocument::Text(extended.into()),
            &locale,
            &dictionaries,
        )
        .unwrap();
    let CleanDocument::Text(control) = control else {
        panic!("expected text");
    };
    assert!(!control.contains("alice@example.invalid"));
    let clean = protect(extended);
    assert!(
        !clean.contains("alice@example.invalid"),
        "{kind}, staged={staged}"
    );
    assert_eq!(
        if staged {
            transaction.restore_strict_text(&clean).unwrap()
        } else {
            session.restore_strict_text(&clean).unwrap()
        },
        extended
    );
}

#[test]
fn prefix_cache_boundary_live() {
    check_rescan("boundary", false);
}

#[test]
fn prefix_cache_boundary_staged() {
    check_rescan("boundary", true);
}

#[test]
fn prefix_cache_stateful_rule_live() {
    check_rescan("rule", false);
}

#[test]
fn prefix_cache_stateful_rule_staged() {
    check_rescan("rule", true);
}

#[test]
fn prefix_cache_stateful_recognizer_live() {
    check_rescan("recognizer", false);
}

#[test]
fn prefix_cache_stateful_recognizer_staged() {
    check_rescan("recognizer", true);
}
