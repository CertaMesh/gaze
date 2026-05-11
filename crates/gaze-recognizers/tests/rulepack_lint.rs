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

fn prefixed_shape_rulepack(
    class: &str,
    first_id: &str,
    first_pattern: &str,
    second_id: &str,
    second_pattern: &str,
    strict: bool,
) -> String {
    let lint = if strict {
        "[recognizers.lint]\nstrict_locale_overlap = true\n"
    } else {
        ""
    };
    format!(
        r#"
schema_version = "0.1.0"
rulepack_id = "prefixed-shape-test"
rulepack_version = "0.6.5"
default_locales = ["global", "de-DE", "fr-FR"]

{lint}
[[recognizers]]
id = "{first_id}"
class = "{class}"
cooperates_with = ["{second_id}"]
enabled = true
locales = ["de-DE"]

[recognizers.match]
kind = "regex"
pattern = '''{first_pattern}'''

[[recognizers]]
id = "{second_id}"
class = "{class}"
cooperates_with = ["{first_id}"]
enabled = true
locales = ["fr-FR"]

[recognizers.match]
kind = "regex"
pattern = '''{second_pattern}'''
"#
    )
}

fn collision_lint_rulepack(first_collision: &str, second_collision: &str) -> String {
    format!(
        r#"
schema_version = "0.1.0"
rulepack_id = "collision-lint"
rulepack_version = "0.7.0"
default_locales = ["global"]

[[recognizers]]
id = "doc.alpha"
class = "custom:alpha"
enabled = true

[recognizers.match]
kind = "regex"
pattern = "ALPHA-[0-9]+"

{first_collision}

[[recognizers]]
id = "doc.beta"
class = "custom:beta"
enabled = true

[recognizers.match]
kind = "regex"
pattern = "BETA-[0-9]+"

{second_collision}
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
fn lint_rejects_ambiguous_collision_family_precedence() {
    let raw = collision_lint_rulepack(
        r#"
[recognizers.collision]
family = "tenant-document"
variant = "alpha"
precedence = 10
"#,
        r#"
[recognizers.collision]
family = "tenant-document"
variant = "beta"
precedence = 10
"#,
    );
    let err = Rulepack::parse(&raw).expect_err("same precedence should reject");

    assert!(matches!(
        err,
        RulepackError::AmbiguousFamilyPrecedence { family, precedence: 10, .. }
            if family == "tenant-document"
    ));
}

#[test]
fn lint_rejects_inconsistent_collision_variant_precedence() {
    let raw = collision_lint_rulepack(
        r#"
[recognizers.collision]
family = "tenant-document"
variant = "alpha"
precedence = 10
"#,
        r#"
[recognizers.collision]
family = "tenant-document"
variant = "alpha"
precedence = 20
"#,
    );
    let err = Rulepack::parse(&raw).expect_err("inconsistent variant should reject");

    assert!(matches!(
        err,
        RulepackError::InconsistentCollisionVariant {
            family,
            variant,
            first_precedence: 10,
            second_precedence: 20,
            ..
        } if family == "tenant-document" && variant == "alpha"
    ));
}

#[test]
#[serial]
fn lint_does_not_fire_on_same_class_different_shape_pair() {
    let raw = prefixed_shape_rulepack(
        "custom:postal_code",
        "postal.de",
        r"\bDE\d{5}\b",
        "postal.fr",
        r"\bFR\d{5}\b",
        false,
    );
    let (result, logs) = capture_rulepack_parse_logs(&raw);

    result.expect("country-prefixed shapes should not warn");
    assert!(
        logs.iter()
            .all(|line| !line.contains("recognizers share class with naked-shape regex")),
        "unexpected postal collision warning in logs: {logs:?}"
    );

    let strict_raw = prefixed_shape_rulepack(
        "custom:postal_code",
        "postal.de",
        r"\bDE\d{5}\b",
        "postal.fr",
        r"\bFR\d{5}\b",
        true,
    );
    Rulepack::parse(&strict_raw).expect("strict mode should allow prefixed shapes");
}

#[test]
#[serial]
fn lint_does_not_fire_on_anchored_iban_shape_pair() {
    let raw = prefixed_shape_rulepack(
        "custom:iban",
        "iban.de",
        r"\bDE\d{2}[A-Z0-9]+\b",
        "iban.fr",
        r"\bFR\d{2}[A-Z0-9]+\b",
        false,
    );
    let (result, logs) = capture_rulepack_parse_logs(&raw);

    result.expect("country-prefixed IBAN shapes should not warn");
    assert!(
        logs.iter()
            .all(|line| !line.contains("recognizers share class with naked-shape regex")),
        "unexpected IBAN collision warning in logs: {logs:?}"
    );

    let strict_raw = prefixed_shape_rulepack(
        "custom:iban",
        "iban.de",
        r"\bDE\d{2}[A-Z0-9]+\b",
        "iban.fr",
        r"\bFR\d{2}[A-Z0-9]+\b",
        true,
    );
    Rulepack::parse(&strict_raw).expect("strict mode should allow prefixed IBAN shapes");
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
