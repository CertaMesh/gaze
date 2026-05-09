use std::sync::{Arc, Mutex};

use gaze::{LocaleTag, PiiClass, Rulepack, RulepackError};
use serial_test::serial;
use tracing::field::{Field, Visit};
use tracing::{Event, Id, Level, Metadata, Subscriber};

fn postal_collision_rulepack(default_locales: &str, strict: bool) -> String {
    let lint = if strict {
        "[recognizers.lint]\nstrict_locale_overlap = true\n"
    } else {
        ""
    };
    format!(
        r#"
schema_version = "0.1.0"
rulepack_id = "postal-collision-test"
rulepack_version = "0.6.5"
default_locales = [{default_locales}]

{lint}
[[recognizers]]
id = "postal.de"
class = "custom:postal_code"
cooperates_with = ["postal.us"]
enabled = true
locales = ["de-DE"]

[recognizers.match]
kind = "regex"
pattern = '''\b\d{{5}}\b'''

[[recognizers]]
id = "postal.us"
class = "custom:postal_code"
cooperates_with = ["postal.de"]
enabled = true
locales = ["en-US"]

[recognizers.match]
kind = "regex"
pattern = '''\b\d{{5}}(-\d{{4}})?\b'''
"#
    )
}

fn capture_rulepack_parse_logs(raw: &str) -> (Result<Rulepack, RulepackError>, Vec<String>) {
    let logs = Arc::new(Mutex::new(Vec::new()));
    let subscriber = CaptureSubscriber { logs: logs.clone() };
    let result = tracing::subscriber::with_default(subscriber, || Rulepack::parse(raw));
    let logs = logs.lock().expect("logs").clone();
    (result, logs)
}

#[derive(Clone)]
struct CaptureSubscriber {
    logs: Arc<Mutex<Vec<String>>>,
}

impl Subscriber for CaptureSubscriber {
    fn enabled(&self, metadata: &Metadata<'_>) -> bool {
        metadata.level() <= &Level::WARN
    }

    fn new_span(&self, _span: &tracing::span::Attributes<'_>) -> Id {
        Id::from_u64(1)
    }

    fn record(&self, _span: &Id, _values: &tracing::span::Record<'_>) {}

    fn record_follows_from(&self, _span: &Id, _follows: &Id) {}

    fn event(&self, event: &Event<'_>) {
        let mut visitor = CaptureVisitor::default();
        event.record(&mut visitor);
        self.logs.lock().expect("logs").push(format!(
            "{} {}",
            event.metadata().level(),
            visitor.fields.join(" ")
        ));
    }

    fn enter(&self, _span: &Id) {}

    fn exit(&self, _span: &Id) {}
}

#[derive(Default)]
struct CaptureVisitor {
    fields: Vec<String>,
}

impl Visit for CaptureVisitor {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        self.fields.push(format!("{}={value:?}", field.name()));
    }
}

#[test]
#[serial]
fn lint_warns_on_postal_collision_with_overlapping_chain() {
    let raw = postal_collision_rulepack(r#""global", "en-US", "de-DE""#, false);
    let (result, logs) = capture_rulepack_parse_logs(&raw);

    result.expect("overlap is warn-only by default");
    assert!(
        logs.iter().any(|line| {
            line.contains("recognizers share class with naked-shape regex")
                && line.contains("postal.de")
                && line.contains("postal.us")
                && line.contains("Custom:postal_code")
        }),
        "missing postal collision warning in logs: {logs:?}"
    );
}

#[test]
fn lint_strict_mode_rejects_overlap() {
    let raw = postal_collision_rulepack(r#""global", "en-US", "de-DE""#, true);
    let err = Rulepack::parse(&raw).expect_err("strict mode must reject overlap");

    assert!(matches!(
        err,
        RulepackError::ConflictingLocaleProjection {
            class: PiiClass::Custom(ref class),
            ref recognizer_ids,
            ref locale_overlap,
        } if class == "postal_code"
            && recognizer_ids == &vec!["postal.de".to_string(), "postal.us".to_string()]
            && locale_overlap == &vec![LocaleTag::DeDe, LocaleTag::EnUs]
    ));
}

#[test]
#[serial]
fn lint_disjoint_locales_load_clean() {
    let raw = postal_collision_rulepack(r#""de-DE""#, false);
    let (result, logs) = capture_rulepack_parse_logs(&raw);

    result.expect("single-locale projection should load");
    assert!(
        logs.iter()
            .all(|line| !line.contains("recognizers share class with naked-shape regex")),
        "unexpected postal collision warning in logs: {logs:?}"
    );
}
