//! Static browser-asset contract for the shipped dashboard frontend.
//!
//! These tests read the compile-time-embedded production assets and prove the
//! frozen frontend contract holds at the byte level: no prohibited sink, no
//! external resource, no storage surface, no success/affirmative style token,
//! and the exact closed-code label vocabulary. The rendered/behavioral half of
//! the contract lives in `browser-tests/dashboard.spec.mjs`; this file is the
//! part that must fail the Rust test suite if the assets regress.
//!
//! Deliberately std-only: it must not depend on the crate's runtime API so the
//! asset contract cannot be weakened by runtime refactors.

use std::fs;
use std::path::PathBuf;

fn asset(name: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("assets")
        .join(name);
    fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("asset {} must exist and be UTF-8: {err}", path.display()))
}

/// Blocker-class prohibited sinks and primitives (plan #4740 §11).
#[test]
fn app_js_contains_no_prohibited_sink() {
    let js = asset("app.js");
    let forbidden = [
        "innerHTML",
        "outerHTML =",
        "insertAdjacentHTML",
        "document.write",
        "createContextualFragment",
        "eval(",
        "new Function",
        "setTimeout(\"",
        "setInterval(\"",
        "javascript:",
        "EventSource",
        "WebSocket",
        "XMLHttpRequest",
        "importScripts",
        "serviceWorker.register",
        "localStorage",
        "sessionStorage",
        "indexedDB",
        "document.cookie",
        "window.open",
        "<svg",
        "<iframe",
        "srcdoc",
    ];
    for needle in forbidden {
        assert!(
            !js.contains(needle),
            "prohibited sink/primitive {needle:?} must not appear in app.js"
        );
    }
}

/// `outerHTML` reads are also excluded from production code entirely.
#[test]
fn app_js_never_touches_html_serialization() {
    let js = asset("app.js");
    assert!(!js.contains("outerHTML"), "app.js must not touch outerHTML");
}

#[test]
fn app_js_uses_hardened_fetch_and_canonical_auth() {
    let js = asset("app.js");
    for needle in [
        "credentials: \"omit\"",
        "cache: \"no-store\"",
        "redirect: \"error\"",
        "referrerPolicy: \"no-referrer\"",
        "GazeDashboardV1 ",
        "AbortController",
        "/^[A-Za-z0-9_-]{43}$/",
    ] {
        assert!(js.contains(needle), "app.js must contain {needle:?}");
    }
}

#[test]
fn app_js_renders_via_safe_dom_construction_only() {
    let js = asset("app.js");
    assert!(js.contains("document.createElement"));
    assert!(js.contains("textContent"));
}

/// The exhaustive closed-code label vocabulary must be present verbatim so a
/// wire code can never leak its raw spelling and no private/numeric value can
/// masquerade as a label (plan #4740 §10).
#[test]
fn app_js_carries_exact_closed_label_vocabulary() {
    let js = asset("app.js");
    let labels = [
        // provider profiles
        "ANTHROPIC MESSAGES DIRECT",
        "OPENAI COMPATIBLE",
        "GEMINI COMPATIBLE",
        "GENERIC LOCAL ANTHROPIC COMPATIBLE",
        "OTHER REVIEWED",
        // endpoint selection
        "FIXED HTTPS ORIGIN",
        "CONFIGURED HTTPS ORIGIN",
        "OS-ASSIGNED LOOPBACK",
        "DYNAMICALLY DISCOVERED LOOPBACK",
        "REVALIDATED LOOPBACK",
        "UNAVAILABLE FAIL-CLOSED",
        // port selection
        "NOT APPLICABLE",
        "FIXED CONFIGURED",
        "OS ASSIGNED",
        "DYNAMICALLY DISCOVERED",
        "CHANGED AND REVALIDATED",
        "SELECTION CHANGE",
        // decision engines/outcomes
        "DETERMINISTIC RECOGNIZER",
        "NEURAL SAFETY NET",
        "RESIDUAL SCAN",
        "RAN — NO FINDINGS (not a verified-clean claim)",
        "FAIL-CLOSED — TIMEOUT",
        "FAIL-CLOSED — ERROR",
        // attestation
        "EXACT HIT",
        "MISS — NO RECORD",
        "MISS — EXPIRED",
        "MISS — EVICTED",
        "INVALIDATED — CONTENT BYTES",
        "INVALIDATED — STRUCTURAL IDENTITY",
        "INVALIDATED — POLICY OR RULEPACK",
        "INVALIDATED — RECOGNIZER OR ARTIFACT",
        "INVALIDATED — LOCALE OR DICTIONARY",
        "INVALIDATED — NORMALIZATION VERSION",
        "INVALIDATED — SESSION EPOCH",
        "INVALIDATED — CONFIGURATION GENERATION",
        "AMBIGUOUS FAIL-CLOSED",
        // omission reasons
        "NOT CAPTURED BY POLICY",
        "UNSUPPORTED FORMAT",
        "MALFORMED — FAIL-CLOSED",
        "LIMIT EXCEEDED",
        "SIGNED OR ENCRYPTED SURFACE",
        "PROJECTION FAILED CLOSED",
        "QUEUE CLOSED",
        // lanes
        "OWNER RAW — PII may be present",
        "PROVIDER VISIBLE — pseudonymized, not verified",
        "OWNER RESTORED — re-identified PII may be present",
        // capture badges
        "PROVIDER-VISIBLE CAPTURE — confidential pseudonymized payload, not verified clean",
        "OWNER RAW CAPTURE ENABLED",
        "OWNER RESTORED CAPTURE ENABLED",
        "BOTH OWNER DOMAINS CAPTURED",
        // accepted limitations
        "QUEUE TELEMETRY: UNAVAILABLE — NOT MEASURED",
    ];
    for needle in labels {
        assert!(
            js.contains(needle),
            "app.js must carry exact label {needle:?}"
        );
    }
}

/// SSE entries render ordinal/kind/delta/content-block only; the no-byte /
/// no-timing constraint is explicit UI copy (seam audit #5953 INFO-2).
#[test]
fn app_js_states_sse_no_byte_no_timing_rule() {
    let js = asset("app.js");
    assert!(js.contains("Per-entry byte counts and timing are not measured and never shown"));
    assert!(js.contains("SSE_KIND_LABELS"));
    assert!(js.contains("SSE_DELTA_LABELS"));
    // No per-entry byte or timing field is ever read from a timeline entry.
    for needle in [
        "entry.bytes",
        "entry.timing",
        "entry.timestamp",
        "entry.latency",
        "entry.elapsed",
    ] {
        assert!(
            !js.contains(needle),
            "SSE entry field {needle:?} must not exist"
        );
    }
}

/// ProviderVisible can never render NOT CAPTURED: the renderer has no such
/// branch and re-maps a hostile wire value to the fail-closed unknown state.
#[test]
fn provider_visible_lane_cannot_express_not_captured() {
    let js = asset("app.js");
    assert!(js.contains(r#"d.kind === "NotCaptured" ? "unknown-fail-closed" : d.kind"#));
}

/// No success/affirmative style token exists in the shipped stylesheet.
#[test]
fn stylesheet_has_no_success_token() {
    let css = asset("app.css").to_lowercase();
    for needle in [
        "--success",
        ".success",
        "--ok",
        "--safe",
        "--good",
        "--green",
        "--verified",
        "--pass",
        "--healthy",
        "--positive",
        "--affirmative",
    ] {
        assert!(
            !css.contains(needle),
            "success/affirmative token {needle:?} must not exist"
        );
    }
    // Lane border grammar is contractual and survives forced colors.
    assert!(css.contains("double"));
    assert!(css.contains("dashed"));
    assert!(css.contains("forced-colors: active"));
    assert!(css.contains("prefers-reduced-motion: reduce"));
    assert!(css.contains("prefers-color-scheme: dark"));
    assert!(!css.contains("@import"));
    assert!(
        !css.contains("url("),
        "stylesheet must reference no resource at all"
    );
}

#[test]
fn shell_is_static_data_free_and_csp_compatible() {
    let html = asset("index.html");
    // No inline script/style, no external resource, no form.
    assert!(!html.contains("<style"), "no inline style block");
    assert!(!html.contains("style="), "no style attribute");
    assert!(!html.contains("http://"), "no external URL");
    assert!(!html.contains("https://"), "no external URL");
    assert!(!html.contains("<form"), "no form element by contract");
    assert!(!html.contains("<svg"));
    assert!(!html.contains("<iframe"));
    assert!(!html.contains("<img"));
    // Exactly one script element, and it is the embedded application source.
    assert_eq!(html.matches("<script").count(), 1);
    assert!(html.contains(r#"<script src="/assets/app.js" defer>"#));
    // Inline handlers are impossible.
    assert!(!html.to_lowercase().contains("onclick"));
    assert!(!html.to_lowercase().contains("onerror"));
    assert!(!html.to_lowercase().contains("onload"));
}

#[test]
fn token_input_matches_frozen_pairing_contract() {
    let html = asset("index.html");
    assert!(html.contains(r#"type="password""#));
    assert!(html.contains(r#"autocomplete="off""#));
    assert!(html.contains(r#"spellcheck="false""#));
    assert!(html.contains(r#"autocapitalize="off""#));
    assert!(html.contains(r#"autocorrect="off""#));
    assert!(html.contains(r#"maxlength="43""#));
    // The token input element carries no name attribute (meta name= is fine).
    let input_start = html
        .find("<input id=\"token-input\"")
        .expect("token input exists");
    let input_end = input_start + html[input_start..].find('>').expect("input tag closes");
    let input_tag = &html[input_start..input_end];
    assert!(
        !input_tag.contains("name="),
        "token input must have no name attribute"
    );
    // The status/alert surfaces exist for accessible announcements.
    assert!(html.contains(r#"role="alert""#));
    assert!(html.contains(r#"role="status""#));
    assert!(html.contains(r#"aria-live="polite""#));
    assert!(html.contains(r#"lang="en""#));
    assert!(html.contains(r#"dir="ltr""#));
}

/// Synthetic fixture discipline: no real-looking PII in any shipped asset.
#[test]
fn assets_contain_no_real_pii_shapes() {
    for name in ["index.html", "app.css", "app.js"] {
        let content = asset(name);
        assert!(
            !content.contains("@gmail."),
            "{name} must not carry real mail domains"
        );
        assert!(
            !content.contains("@example.com"),
            "{name}: use .invalid fixtures only"
        );
        assert!(
            !content.contains("+1-202"),
            "{name}: only documented synthetic phone ranges"
        );
    }
}

/// The disconnected surface states dashboard purge + provider continuity and
/// never implies proxy failure (plan #4740 §10).
#[test]
fn disconnected_copy_preserves_provider_continuity_claim() {
    let js = asset("app.js");
    assert!(js.contains("The proxy is unaffected and continues running without capture."));
    assert!(js.contains("DASHBOARD DISABLED ("));
    for needle in [
        "proxy failed",
        "proxy down",
        "provider failed",
        "provider down",
    ] {
        assert!(
            !js.to_lowercase().contains(needle),
            "must never imply provider impairment: {needle:?}"
        );
    }
}
