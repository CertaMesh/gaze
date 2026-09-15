//! Scripted policy-boundary proofs. All values are synthetic; no model evidence.
use gaze::*;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

type Step = (String, std::result::Result<Vec<LeakSuspect>, SafetyNetError>);
struct Script(Arc<Mutex<VecDeque<Step>>>);
impl SafetyNet for Script {
    fn id(&self) -> &str { "second.fixture" }
    fn supported_locales(&self) -> &[LocaleTag] { &[LocaleTag::Global] }
    fn check(&self, text: &str, _: SafetyNetContext<'_>) -> std::result::Result<Vec<LeakSuspect>, SafetyNetError> {
        let (expected, result) = self.0.lock().unwrap().pop_front().expect("no extra sweep");
        assert_eq!(text, expected, "each sweep must inspect its actual phase output");
        result
    }
}
fn raw(span: std::ops::Range<usize>) -> LeakSuspect {
    LeakSuspect::new(span, PiiClass::Name, "second.fixture", Some(1.0), LeakKind::Uncovered, "synthetic", Some("field".into()))
}
fn pipeline(steps: Vec<Step>) -> (Pipeline, Arc<Mutex<VecDeque<Step>>>) {
    let steps = Arc::new(Mutex::new(steps.into()));
    (Pipeline::builder().rule(DefaultRule::new(Action::Preserve))
        .register_safety_net(Script(steps.clone())).build().unwrap(), steps)
}

#[test]
fn second_batch_after_noop_preserves_raw_and_scans_exact_reversible_output() {
    let session = Session::new(Scope::Ephemeral).unwrap();
    // Pre-own the expected replacement to make the exact random-session bytes deterministic.
    let token = session.tokenize_with_family("safety_net", &PiiClass::Name, "pré").unwrap();
    let (pipeline, steps) = pipeline(vec![
        ("pré tail".into(), Ok(vec![])),
        ("pré tail".into(), Ok(vec![raw(0..4)])),
        (format!("{token} tail"), Ok(vec![raw(0..token.len())])),
    ]);
    let (doc, manifest, report, trace) = pipeline
        .clean_text_with_safety_net_policy_detect_context_and_protection_trace(
            &session, "pré tail", &[LocaleTag::Global], &DictionaryBundle::default(), SafetyNetPolicy::default())
        .expect("eligible no-op first resolve must get one complete reversible batch");
    let CleanDocument::Text(text) = doc else { panic!("text") };
    assert_eq!(text, format!("{token} tail"));
    assert_eq!(session.restore_strict_text(&text).unwrap(), "pré tail");
    assert_eq!(manifest[0].raw_span, 0..4);
    assert_eq!(trace[0].raw_start()..trace[0].raw_end(), 0..4);
    assert_eq!(trace[0].source_ids(), &["second.fixture"]);
    assert_eq!(report.suspects.len(), 1);
    assert_eq!(report.suspects[0].field_path.as_deref(), Some("field"));
    assert!(steps.lock().unwrap().is_empty());
}
