//! Audit proof (spawn 4759): safety-net RESOLVE must record `EmittedTokenSpan.raw_span`
//! in ORIGINAL-request byte coordinates, not clean-text coordinates.
//!
//! Synthetic fixtures only. Uses the public `clean_with_safety_net_policy_detect_context`
//! surface plus the same MockNet shape as `tests/safety_net.rs`.

use std::ops::Range;
use std::sync::{Arc, Mutex};

use gaze::{
    Action, ClassRule, CleanDocument, DefaultRule, Detection, Detector, EmittedTokenSpan, LeakKind,
    LeakSuspect, PiiClass, Pipeline, RawDocument, SafetyNet, SafetyNetContext, SafetyNetError,
    Scope, Session,
};

const EMAIL: &str = "alice@example.invalid";
const RAW: &str = "alice@example.invalid tail";

#[derive(Clone)]
struct FixedDetector {
    span: Range<usize>,
    class: PiiClass,
}

impl Detector for FixedDetector {
    fn detect(&self, _input: &str) -> Vec<Detection> {
        vec![Detection::new(
            self.span.clone(),
            self.class.clone(),
            "fixed",
        )]
    }
}

#[derive(Clone)]
struct MockNet {
    span: Arc<Mutex<Option<Range<usize>>>>,
    class: PiiClass,
    whole_clean: bool,
}

impl MockNet {
    fn whole_clean(class: PiiClass) -> Self {
        Self {
            span: Arc::new(Mutex::new(None)),
            class,
            whole_clean: true,
        }
    }
}

impl SafetyNet for MockNet {
    fn id(&self) -> &str {
        "mock"
    }

    fn supported_locales(&self) -> &[gaze::LocaleTag] {
        &[gaze::LocaleTag::Global]
    }

    fn check(
        &self,
        clean_text: &str,
        context: SafetyNetContext<'_>,
    ) -> Result<Vec<LeakSuspect>, SafetyNetError> {
        let span = if self.whole_clean {
            0..clean_text.len()
        } else {
            match self.span.lock().unwrap().clone() {
                Some(span) => span,
                None => return Ok(Vec::new()),
            }
        };
        if span.start >= span.end || span.end > clean_text.len() {
            return Ok(Vec::new());
        }
        let Some(kind) = context.manifest.diff_against(&span, &self.class) else {
            return Ok(Vec::new());
        };
        Ok(vec![LeakSuspect::new(
            span,
            self.class.clone(),
            self.id(),
            Some(0.99),
            kind,
            "private_email",
            context.field_path.map(str::to_string),
        )])
    }
}

fn text(clean: CleanDocument) -> String {
    match clean {
        CleanDocument::Text(text) => text,
        _ => panic!("expected text"),
    }
}

/// Every manifest entry must satisfy: restore(clean[clean_span]) == raw[raw_span].
///
/// This is the axis-2 reversibility contract: `raw_span` is consumed by downstream
/// crates (`gaze-token-bridge::ingest::build_index_hit` slices the ORIGINAL text by it;
/// `gaze-proxy::PipelineResponseResidualValidator` compares it against authorized
/// ORIGINAL-coordinate ranges). If safety-net RESOLVE records clean coordinates there,
/// both consumers silently read the wrong bytes of the original document.
#[test]
fn safety_net_resolve_manifest_raw_span_indexes_original_request_bytes() {
    // Primary pipeline tokenizes the email, so the clean text is SHORTER than the raw
    // text and clean coordinates diverge from original coordinates after that point.
    let pipeline = Pipeline::builder()
        .detector(FixedDetector {
            span: 0..EMAIL.len(),
            class: PiiClass::Email,
        })
        .rule(ClassRule::new(PiiClass::Email, Action::Tokenize))
        .rule(DefaultRule::new(Action::Preserve))
        .register_safety_net(MockNet::whole_clean(PiiClass::Email))
        .build()
        .expect("pipeline");
    let session = Session::new(Scope::Ephemeral).expect("session");

    let (clean, manifest, report) = pipeline
        .clean_with_safety_net_policy_detect_context(
            &session,
            RawDocument::Text(RAW.to_string()),
            &[gaze::LocaleTag::Global],
            &gaze::DictionaryBundle::default(),
            gaze::SafetyNetPolicy::new(
                gaze::SafetyNetMode::Resolve,
                gaze::SafetyNetFallback::Redact,
            ),
        )
        .expect("resolve clean");
    let clean_text = text(clean);

    assert!(
        matches!(report.suspects[0].kind, LeakKind::PartialBleed { .. }),
        "fixture must exercise the partial-bleed resolve path, got {:?}",
        report.suspects[0].kind
    );
    assert_eq!(manifest.len(), 2, "primary token + safety-net promotion");

    for span in &manifest {
        assert_valid(&span.clean_span, &clean_text, "clean");
        assert_valid(&span.raw_span, RAW, "raw");

        let token = &clean_text[span.clean_span.clone()];
        let restored = session
            .restore(token)
            .unwrap_or_else(|| panic!("manifest token {token:?} must restore"));
        assert_eq!(
            restored,
            &RAW[span.raw_span.clone()],
            "manifest raw_span {:?} must address the ORIGINAL bytes that token {token:?} \
             restores to; raw[{:?}] = {:?}",
            span.raw_span,
            span.raw_span,
            &RAW[span.raw_span.clone()],
        );
    }

    // And the whole document must round-trip exactly.
    assert_eq!(
        session
            .restore_strict_text(&clean_text)
            .expect("clean text restores"),
        RAW
    );
}

fn assert_valid(span: &Range<usize>, text: &str, label: &str) {
    assert!(
        span.start < span.end
            && span.end <= text.len()
            && text.is_char_boundary(span.start)
            && text.is_char_boundary(span.end),
        "{label}_span {span:?} is not a valid range into {label} text of len {}",
        text.len()
    );
}

// Keep the unused-field lint quiet on the branch where `span` is never populated.
#[allow(dead_code)]
fn _unused(net: &MockNet) -> Option<Range<usize>> {
    net.span.lock().unwrap().clone()
}

#[allow(dead_code)]
fn _unused_manifest(spans: &[EmittedTokenSpan]) -> usize {
    spans.len()
}
