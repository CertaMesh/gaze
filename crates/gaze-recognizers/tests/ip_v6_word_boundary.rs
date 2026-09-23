//! Regression fixtures for the `core` `ip.v6` word boundary.
//!
//! `::` shorthand makes a great many Rust and C++ path segments legal IPv6 addresses: `::a` in
//! `CleanOverrides::apply_to`, `::defa` in `Policy::default()`, `d::f` in `std::fs::read`, and a
//! bare `::` wherever a path has no hex on either side. The `ipv6_parse` validator accepts every
//! one of them, because they really are RFC 4291 addresses; only context separates them from an
//! address. Before todo 3710 the rule's guard class excluded hex digits alone, so any
//! non-hex identifier character (`s`, `y`, `p`, `>`) satisfied it and the candidate fired
//! mid-identifier, mangling code-heavy agentic text: PR bodies, stack traces, tool-call JSON.
//!
//! The guard is now a word-character class, the same edge rule as `gaze_types::is_inside_word`:
//! a candidate that begins right after an identifier character or ends right before one does not
//! fire. That class is a strict subset of the old one, so the change can only ever remove
//! matches, never add them.
//!
//! The URL recognizer used to shield `http://[::1]:8080/`; the IPv6 fragment is now protected.
//! A standalone `a::b` still tokenizes: with nothing on either side it is indistinguishable from
//! the address it literally is, and refusing it would cost real recall.
//!
//! Fixture addresses are documentation ranges (RFC 3849 `2001:db8::/32`, link-local `fe80::`,
//! loopback `::1`) and RFC 5737 `192.0.2.0/24`; none routes anywhere.

use gaze::Context;
use gaze::{
    Action, CleanDocument, DictionaryBundle, LocaleChain, LocaleTag, PiiClass, Pipeline,
    RawDocument, RuleSpec, Rulepack, RulepackSource, Scope, Session,
};
use gaze_recognizers::embedded;

fn empty_context() -> Context {
    Context {
        dictionaries: std::collections::HashMap::new(),
        class_map: std::collections::HashMap::new(),
        fields: serde_json::Map::new(),
    }
}

fn ip_class() -> PiiClass {
    PiiClass::custom("ip_address").expect("valid custom class")
}

/// The core bundle through the real activation path, with only `custom:ip_address` tokenized so
/// any change in the output is this rule's doing.
fn pipeline_for_locales(locales: &[LocaleTag]) -> Pipeline {
    let rulepack = Rulepack::load(RulepackSource::Embedded(
        embedded("core").expect("core rulepack"),
    ))
    .expect("core loads");
    let mut policy = gaze::Policy::default();
    policy.rules = vec![
        RuleSpec::Class {
            class: ip_class(),
            action: Action::Tokenize,
        },
        RuleSpec::Default {
            action: Action::Preserve,
        },
    ];
    policy.rulepacks.bundled = vec!["core".to_string()];
    policy.rulepacks.auto_activate_locale_gated = false;
    let chain = LocaleChain::merge_cli_policy_rulepack_default(None, None, Some(locales));
    gaze_assembly::build_pipeline(&policy, &empty_context(), &[rulepack], &chain, None)
        .expect("pipeline")
}

fn pipeline() -> Pipeline {
    pipeline_for_locales(&LOCALES)
}

fn clean(text: &str) -> String {
    let session = Session::new(Scope::Ephemeral).expect("session");
    let (clean, _, _) = pipeline()
        .clean_with_safety_net_detect_context(
            &session,
            RawDocument::Text(text.to_string()),
            &LOCALES,
            &DictionaryBundle::default(),
        )
        .expect("clean");
    match clean {
        CleanDocument::Text(text) => text,
        other => panic!("expected text, got {other:?}"),
    }
}

const LOCALES: [LocaleTag; 1] = [LocaleTag::EnUs];
const DE_LOCALES: [LocaleTag; 1] = [LocaleTag::DeDe];

/// Asserts the input survives byte for byte: nothing in it was taken for an address.
fn assert_untouched(text: &str) {
    let cleaned = clean(text);
    assert_eq!(
        cleaned, text,
        "code path {text:?} must survive verbatim, got {cleaned:?}"
    );
}

/// Asserts the whole address is gone and the surrounding text survives.
fn assert_tokenized(text: &str, address: &str, surviving_context: &[&str]) {
    let cleaned = clean(text);
    assert!(!cleaned.contains(address), "address survived cleaning");
    for fragment in surviving_context {
        assert!(cleaned.contains(fragment), "surrounding context changed");
    }
}

// ============================================== the defect: `::` paths must not fire (todo 3710)

// drift-ack: the drift corpus gained a Rust scope-separator line so the bundled no-policy gate
// can see this boundary too. Only `fixtures_sha256` moved in both snapshots; detections stayed
// at 12 for `core` and 2 for `secrets`, because the new line tokenizes nothing.
#[test]
fn rust_paths_from_the_report_survive_verbatim() {
    // Every one of these was mangled in a real PR body this cycle (todo 3710, comment 2127).
    for path in [
        "CleanOverrides::apply_to",
        "gaze_assembly::CorePipelineConfig",
        "Policy::default()",
        "review580_diagnostics::deadline_seconds",
        "Action::strictness_rank",
        "gaze::rule::resolve",
        "gaze_assembly::build_pipeline",
    ] {
        assert_untouched(path);
    }
}

#[test]
fn generic_rust_and_cpp_path_shapes_survive_verbatim() {
    for path in [
        "std::fs::read",
        "Foo::<T>::new",
        "::std::mem",
        "crate::x",
        "std::",
        "let x = Foo::bar();",
        "See `gaze::pipeline::clean` for details.",
        "std::collections::HashMap<String, Vec<u8>>",
        "namespace detail::inner",
        "auto v = std::vector<int>{};",
    ] {
        assert_untouched(path);
    }
}

#[test]
fn a_path_segment_that_is_all_hex_still_survives() {
    // `d::f` and `::defa` parse as addresses; only the boundary tells them apart from one.
    for path in ["std::fs", "Cafe::deadbeef", "x.fade::beef"] {
        assert_untouched(path);
    }
}

// ================================================================ recall: real IPv6 still fires

#[test]
fn bare_addresses_still_tokenize() {
    for address in [
        "::1",
        "::",
        "fe80::1",
        "2001:db8::a",
        "2001:0db8:0000:0000:0000:ff00:0042:8329",
        "::ffff:192.0.2.1",
        "2001:db8::1",
    ] {
        let cleaned = clean(address);
        assert!(!cleaned.contains(address), "bare address survived cleaning");
    }
}

#[test]
fn addresses_in_the_usual_delimiters_still_tokenize() {
    assert_tokenized("loopback ::1 here", "::1", &["loopback ", " here"]);
    assert_tokenized("[2001:db8::1]:443", "2001:db8::1", &["[", "]:443"]);
    assert_tokenized(
        "{\"ip\":\"2001:db8::1\"}",
        "2001:db8::1",
        &["{\"ip\":\"", "\"}"],
    );
    assert_tokenized("addr=2001:db8::1", "2001:db8::1", &["addr="]);
    assert_tokenized("ping(2001:db8::1)", "2001:db8::1", &["ping(", ")"]);
    assert_tokenized("host 2001:db8::1.", "2001:db8::1", &["host ", "."]);
    assert_tokenized(
        "host 2001:db8::1, next",
        "2001:db8::1",
        &["host ", ", next"],
    );
    assert_tokenized("prefix 2001:db8::/32", "2001:db8::", &["prefix ", "/32"]);
}

#[test]
fn a_zone_id_is_left_behind_exactly_as_before() {
    // The address tokenizes and `%eth0` survives: unchanged from the base rule, pinned so a
    // later guard change has to state its intent about zone ids.
    assert_tokenized("fe80::1%eth0", "fe80::1", &["%eth0"]);
}

// ============================================ base behaviour pinned so it cannot silently drift

#[test]
fn a_standalone_all_hex_path_still_tokenizes() {
    // Documented residual limit. `a::b` and `abc::def` are legal addresses with no context on
    // either side, so nothing distinguishes them from the address they are; the boundary rule
    // cannot reject them, and a rule that did would cost real recall. In practice a Rust path
    // whose BOTH segments are short all-hex words is vanishingly rare, and any surrounding
    // identifier character (`std::abc::def`, `abc::def()`) already restores the boundary.
    for ambiguous in ["a::b", "abc::def"] {
        let cleaned = clean(ambiguous);
        assert!(
            !cleaned.contains(ambiguous),
            "{ambiguous:?} still tokenizes, got {cleaned:?}"
        );
    }
    // The boundary restores itself only on the LEFT: a leading identifier character, `:` or
    // `.` before the candidate blocks it. A trailing `(` is a legal address delimiter, so
    // `abc::def()` still fires; that is the residual limit, not an oversight.
    assert_untouched("std::abc::def");
    assert_untouched("mod_abc::def");
}

#[test]
fn glued_cue_addresses_are_protected() {
    for (text, address, context) in [
        ("Address:2001:db8::1", "2001:db8::1", "Address:"),
        ("IP:fe80::1", "fe80::1", "IP:"),
        ("ipv6:2001:db8::a", "2001:db8::a", "ipv6:"),
        ("host:2001:db8::1", "2001:db8::1", "host:"),
        ("Adresse:2001:db8::1", "2001:db8::1", "Adresse:"),
        ("{\"ip\":\"2001:db8::1\"}", "2001:db8::1", "{\"ip\":\""),
    ] {
        assert_tokenized(text, address, &[context]);
    }
}

#[test]
fn glued_cue_manifest_spans_cover_only_the_address() {
    for locales in [&LOCALES[..], &DE_LOCALES[..]] {
        let pipeline = pipeline_for_locales(locales);
        for (prefix, address, suffix) in [
            ("Address:", "2001:db8::1", ""),
            ("address:", "2001:db8::1", ""),
            ("ADDRESS:", "2001:db8::1", ""),
            ("IP:", "fe80::1", ""),
            ("ipv6:", "2001:db8::a", ""),
            ("ip=", "2001:db8::1", ""),
            ("host:", "2001:db8::1", ""),
            ("addr:", "2001:db8::1", ""),
            ("Adresse:", "2001:db8::1", ""),
            ("{\"ip\":\"", "2001:db8::1", "\"}"),
        ] {
            let raw = format!("{prefix}{address}{suffix}");
            let session = Session::new(Scope::Ephemeral).expect("session");
            let (clean, manifest, _) = pipeline
                .clean_with_safety_net_detect_context(
                    &session,
                    RawDocument::Text(raw),
                    locales,
                    &DictionaryBundle::default(),
                )
                .expect("clean");
            let CleanDocument::Text(clean) = clean else {
                panic!("expected text");
            };
            assert_eq!(manifest.len(), 1, "one address token expected");
            let span = &manifest[0];
            assert_eq!(span.class, ip_class());
            assert_eq!(span.raw_span, prefix.len()..prefix.len() + address.len());
            assert_eq!(span.clean_span.start, prefix.len());
            let token = &clean[span.clean_span.clone()];
            assert_eq!(clean, format!("{prefix}{token}{suffix}"));
            assert_eq!(session.restore(token).as_deref(), Some(address));
        }
    }
}

#[test]
fn cue_words_followed_by_scope_separators_are_untouched() {
    for path in [
        "Foo::bar",
        "std::fs::read",
        "gaze::rule::resolve",
        "Policy::default()",
        "Address::new",
        "IP::from",
        "host::connect",
    ] {
        assert_untouched(path);
    }
}

/// The URL recognizer owns `http://[::1]:8080/` and the policy preserves URLs by default, but
/// the bracketed literal is an address the policy tokenizes: protection beats preservation, so
/// the literal leaves as an `ip_address` fragment inside the otherwise raw URL (todo #3740).
/// The rule fired on `::1` before as well; the preserved URL used to shield it.
#[test]
fn an_address_inside_a_preserved_url_is_still_protected() {
    assert_tokenized("http://[::1]:8080/", "::1", &["http://[", "]:8080/"]);
}
