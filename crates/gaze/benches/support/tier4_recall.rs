use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use gaze::{
    Action, ClassRule, CleanDocument, DefaultRule, Detection, Detector, DictionaryBundle, LeakKind,
    LeakSuspect, Pipeline, PipelineOptimizationConfig, RawDocument, SafetyNet, SafetyNetContext,
    SafetyNetError, SafetyNetFallback, SafetyNetMode, SafetyNetPolicy, Scope, Session,
};

#[derive(Clone)]
pub(crate) struct EmailDetector;

impl Detector for EmailDetector {
    fn detect(&self, input: &str) -> Vec<Detection> {
        input
            .find("alice@example.invalid")
            .map(|start| {
                Detection::new(
                    start..start + "alice@example.invalid".len(),
                    gaze::PiiClass::Email,
                    "bench.email",
                )
            })
            .into_iter()
            .collect()
    }
}

pub(crate) struct SlowSafetyNet {
    pub(crate) calls: Arc<AtomicUsize>,
}

impl SafetyNet for SlowSafetyNet {
    fn id(&self) -> &str {
        "bench-slow"
    }

    fn supported_locales(&self) -> &[gaze::LocaleTag] {
        &[gaze::LocaleTag::Global]
    }

    fn check(
        &self,
        clean_text: &str,
        _context: SafetyNetContext<'_>,
    ) -> Result<Vec<LeakSuspect>, SafetyNetError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        thread::sleep(Duration::from_micros(100));
        Ok(clean_text
            .find("class_alpha_secret")
            .map(|start| {
                LeakSuspect::new(
                    start..start + "class_alpha_secret".len(),
                    gaze::PiiClass::Name,
                    self.id(),
                    Some(1.0),
                    LeakKind::Uncovered,
                    "synthetic required positive",
                    None,
                )
            })
            .into_iter()
            .collect())
    }
}

/// Required protection on an input that either observer heuristic would skip.
/// This uses the same net as the timing loop, with a residual the rule floor misses.
pub(crate) fn assert_required_positive() {
    let input = "Reach alice@example.invalid about class_alpha_secret";
    for mode in [SafetyNetMode::Redact, SafetyNetMode::Resolve] {
        let mut baseline = None;
        for config in configs() {
            let pipeline = Pipeline::builder()
                .detector(EmailDetector)
                .rule(ClassRule::new(gaze::PiiClass::Email, Action::Tokenize))
                .rule(ClassRule::new(gaze::PiiClass::Name, Action::Tokenize))
                .rule(DefaultRule::new(Action::Preserve))
                .register_safety_net(SlowSafetyNet {
                    calls: Arc::default(),
                })
                .pipeline_optimizations(config.clone())
                .build()
                .expect("pipeline");
            let session = Session::new(Scope::Ephemeral).expect("session");
            let (clean, manifest, report) = pipeline
                .clean_with_safety_net_policy_detect_context(
                    &session,
                    RawDocument::Text(input.to_owned()),
                    &[gaze::LocaleTag::Global],
                    &DictionaryBundle::default(),
                    SafetyNetPolicy::new(mode, SafetyNetFallback::Strict),
                )
                .expect("required positive must be protected");
            let CleanDocument::Text(text) = clean else {
                panic!("text fixture")
            };
            assert_eq!(
                report.stats.suspect_count, 1,
                "required positive: {mode:?}, {config:?}"
            );
            assert!(
                !text.contains("class_alpha_secret"),
                "net must protect the residual"
            );
            assert!(
                !text.contains("alice@example.invalid"),
                "rule floor must protect email"
            );
            assert_eq!(manifest.len(), 2, "both protected spans must be recorded");
            if mode == SafetyNetMode::Resolve {
                assert_eq!(
                    pipeline
                        .restore_strict_text(&session, &text)
                        .expect("restore"),
                    input
                );
            }
            let outcome = (
                report.stats.suspect_count,
                manifest.len(),
                text.contains("class_alpha_secret"),
            );
            if let Some(expected) = &baseline {
                assert_eq!(
                    &outcome, expected,
                    "gating must preserve protection: {mode:?}, {config:?}"
                );
            } else {
                baseline = Some(outcome);
            }
        }
    }
}

fn configs() -> [PipelineOptimizationConfig; 4] {
    [
        PipelineOptimizationConfig::default(),
        PipelineOptimizationConfig::new().with_skip_class_gating(true),
        PipelineOptimizationConfig::new().with_capitals_heuristic_gate(true),
        PipelineOptimizationConfig::new()
            .with_skip_class_gating(true)
            .with_capitals_heuristic_gate(true),
    ]
}

/// The timed corpus has no residual: observer gates may skip the net here.
/// Compare output, restore and reports while proving each intended skip occurs.
pub(crate) fn assert_observer_skips_preserve_semantics() {
    let mut baseline = None;
    for (config, expected_calls) in configs().into_iter().zip([3, 2, 1, 0]) {
        let calls = Arc::new(AtomicUsize::new(0));
        let pipeline = Pipeline::builder()
            .detector(EmailDetector)
            .rule(ClassRule::new(gaze::PiiClass::Email, Action::Tokenize))
            .rule(DefaultRule::new(Action::Preserve))
            .register_safety_net(SlowSafetyNet {
                calls: Arc::clone(&calls),
            })
            .pipeline_optimizations(config.clone())
            .build()
            .expect("pipeline");
        let mut outcomes = Vec::new();
        for (input, expected_spans) in [
            ("Reach alice@example.invalid about case 0", 1),
            ("lowercase status update number 0", 0),
            ("20260515-0000-0000-999999", 0),
        ] {
            let session = Session::new(Scope::Ephemeral).expect("session");
            let (clean, manifest, report) = pipeline
                .clean_with_safety_net_policy_detect_context(
                    &session,
                    RawDocument::Text(input.to_owned()),
                    &[gaze::LocaleTag::Global],
                    &DictionaryBundle::default(),
                    SafetyNetPolicy::new(SafetyNetMode::Strict, SafetyNetFallback::Redact),
                )
                .expect("benign observer fixture");
            let CleanDocument::Text(text) = clean else {
                panic!("text fixture")
            };
            assert!(
                !text.contains("alice@example.invalid"),
                "email must remain protected"
            );
            assert_eq!(manifest.len(), expected_spans);
            assert_eq!(report.stats.suspect_count, 0);
            let restored = pipeline
                .restore_strict_text(&session, &text)
                .expect("restore");
            assert_eq!(restored, input);
            outcomes.push((restored, manifest.len(), report.stats.suspect_count));
        }
        assert_eq!(
            calls.load(Ordering::SeqCst),
            expected_calls,
            "observer gating: {config:?}"
        );
        if let Some(expected) = &baseline {
            assert_eq!(&outcomes, expected, "observer semantics: {config:?}");
        } else {
            baseline = Some(outcomes);
        }
    }
}
