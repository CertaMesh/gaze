//! Synthetic proof of truthful first-gap reports across owned replacements.
use std::sync::{Arc, Mutex};

use gaze::{
    Action, ClassRule, CleanDocument, DefaultRule, Detection, Detector, DictionaryBundle,
    LeakSuspect, LocaleTag, PiiClass, Pipeline, RawDocument, SafetyNet, SafetyNetContext,
    SafetyNetError, SafetyNetFallback, SafetyNetMode, SafetyNetPolicy, Scope, Session,
};

const RAW: &str = "pré alice@example.invalid 尾";

struct Primary;
impl Detector for Primary {
    fn detect(&self, _: &str) -> Vec<Detection> {
        vec![Detection::new(5..26, PiiClass::Email, "primary.fixture")]
    }
}

struct WholeParent {
    seen: Arc<Mutex<Vec<String>>>,
}
impl SafetyNet for WholeParent {
    fn id(&self) -> &str {
        "multigap.fixture"
    }
    fn supported_locales(&self) -> &[LocaleTag] {
        &[LocaleTag::Global]
    }
    fn check(
        &self,
        text: &str,
        context: SafetyNetContext<'_>,
    ) -> Result<Vec<LeakSuspect>, SafetyNetError> {
        self.seen.lock().unwrap().push(text.to_owned());
        let span = 0..text.len();
        Ok(context
            .manifest
            .diff_against(&span, &PiiClass::Email)
            .map(|kind| {
                LeakSuspect::new(
                    span,
                    PiiClass::Email,
                    self.id(),
                    Some(1.0),
                    kind,
                    "synthetic",
                    None,
                )
            })
            .into_iter()
            .collect())
    }
}

#[test]
fn truthful_first_gap_resolves_both_sides_and_restores_original_bytes() {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let pipeline = Pipeline::builder()
        .detector(Primary)
        .rule(ClassRule::new(PiiClass::Email, Action::Tokenize))
        .rule(DefaultRule::new(Action::Preserve))
        .register_safety_net(WholeParent { seen: seen.clone() })
        .build()
        .unwrap();
    let session = Session::new(Scope::Ephemeral).unwrap();
    let (clean, manifest, report) = pipeline
        .clean_with_safety_net_policy_detect_context(
            &session,
            RawDocument::Text(RAW.into()),
            &[LocaleTag::Global],
            &DictionaryBundle::default(),
            SafetyNetPolicy::new(SafetyNetMode::Resolve, SafetyNetFallback::Strict),
        )
        .expect("truthful multiple gaps must resolve");
    let CleanDocument::Text(clean) = clean else {
        panic!("text")
    };
    assert_eq!(
        manifest
            .iter()
            .map(|s| s.raw_span.clone())
            .collect::<Vec<_>>(),
        vec![0..5, 5..26, 26..30]
    );
    assert_eq!(session.restore_strict_text(&clean).unwrap(), RAW);
    for entry in &manifest {
        assert_eq!(
            session.restore(&clean[entry.clean_span.clone()]).unwrap(),
            RAW[entry.raw_span.clone()]
        );
    }
    let before = seen.lock().unwrap()[0].clone();
    assert_eq!(
        &clean[manifest[1].clean_span.clone()],
        &before[5..before.len() - 4]
    );
    assert!(
        matches!(report.suspects[0].kind, gaze::LeakKind::PartialBleed { ref uncovered } if *uncovered == (0..5))
    );
    assert_eq!(seen.lock().unwrap().len(), 2);
    assert_eq!(seen.lock().unwrap()[1], clean);
}
