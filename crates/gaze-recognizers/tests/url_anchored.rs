//! Regression fixtures for the `url.anchored` core recognizer.
//!
//! The original holdout fixtures encode structural shapes MEASURED in the Dataiku EN/DE
//! holdout, not a phrasing invented alongside the implementation. The previous attempt at this
//! bucket shipped 962 green tests for cue phrasings — "my profile is <url>" — that
//! occur in 5 of 276 gold spans, and produced a zero real-corpus delta. The measured distribution
//! of the 232 gold URL spans this rule covers is:
//!
//! | dimension      | counts                                            |
//! |----------------|---------------------------------------------------|
//! | anchor         | scheme 169, `www.` 63                             |
//! | shape          | host-only 149, has-path 83                        |
//! | host labels    | 2 labels 57, 3 labels 154, 4 labels 21            |
//! | query string   | 1                                                 |
//! | reference host | 16 (docs./github/wiki/example. shapes — all GOLD) |
//!
//! Every row above has at least one fixture below, and the trailing-boundary cases exist because
//! the boundary rule was chosen on measured false-positive counts (107 documents vs 12).
//!
//! Fixture values are synthetic and use reserved `.invalid` hosts only.

use gaze::Context;
use gaze::{
    Action, CleanDocument, DictionaryBundle, LocaleChain, LocaleTag, PiiClass, Pipeline,
    RawDocument, RuleSpec, Rulepack, RulepackSource, Scope, Session,
};
use gaze_recognizers::embedded;

/// `global` only, which is what `core`'s `default_locales` already resolves to.
fn global_chain() -> LocaleChain {
    LocaleChain::merge_cli_policy_rulepack_default(None, None, Some(&[LocaleTag::Global]))
}

fn empty_context() -> Context {
    Context {
        dictionaries: std::collections::HashMap::new(),
        class_map: std::collections::HashMap::new(),
        fields: serde_json::Map::new(),
        record_match_kinds: Default::default(),
        record_value_rejections: Default::default(),
    }
}

fn url_class() -> PiiClass {
    PiiClass::custom("url").expect("valid custom class")
}

/// The core bundle assembled through the real activation path, with `global` as the only active
/// locale and locale-gated auto-activation OFF.
///
/// That combination is deliberate: it is the weakest configuration a default adopter can have.
/// `url.anchored` is declared `safety_tier = "safe_default"` with `locales = ["global"]`, so
/// `gaze_assembly::detector_wiring::recognizer_activates` admits it unconditionally. If the
/// recognizer were instead declared `locale_gated`, every test in this file would still pass under
/// the benchmark's configuration (which sets `auto_activate_locale_gated = true` for all three
/// cells) while a default adopter got no URL protection at all. Building the pipeline this way is
/// what makes that distinction fail loudly rather than silently.
fn pipeline() -> Pipeline {
    pipeline_with_actions(Action::Tokenize, Action::Preserve)
}

fn pipeline_with_actions(url_action: Action, email_action: Action) -> Pipeline {
    let rulepack = Rulepack::load(RulepackSource::Embedded(
        embedded("core").expect("core rulepack"),
    ))
    .expect("core loads");
    let mut policy = gaze::Policy::default();
    policy.rules = vec![
        RuleSpec::Class {
            class: url_class(),
            action: url_action,
        },
        RuleSpec::Class {
            class: PiiClass::Email,
            action: email_action,
        },
        RuleSpec::Default {
            action: Action::Preserve,
        },
    ];
    policy.rulepacks.bundled = vec!["core".to_string()];
    policy.rulepacks.auto_activate_locale_gated = false;
    gaze_assembly::build_pipeline(
        &policy,
        &empty_context(),
        &[rulepack],
        &global_chain(),
        None,
    )
    .expect("pipeline")
}

fn clean_with(pipeline: &Pipeline, session: &Session, text: &str) -> String {
    let (clean, _, _) = pipeline
        .clean_with_safety_net_detect_context(
            session,
            RawDocument::Text(text.to_string()),
            &[LocaleTag::Global],
            &DictionaryBundle::default(),
        )
        .expect("clean");
    match clean {
        CleanDocument::Text(text) => text,
        _ => panic!("expected text"),
    }
}

fn clean(text: &str) -> String {
    let session = Session::new(Scope::Ephemeral).expect("session");
    clean_with(&pipeline(), &session, text)
}

/// Asserts the whole URL is gone from the clean text and that the surrounding prose survives.
///
/// Whole-span coverage is the point: the scorecard shows the NER passes already overlap ~100 URL
/// entities while fully covering zero of them, at roughly 8.4 covered bytes of ~26 per entity.
/// A fragment is not a fix.
fn assert_url_removed(text: &str, url: &str, surviving_context: &[&str]) {
    let cleaned = clean(text);
    assert!(
        !cleaned.contains(url),
        "url {url:?} survived tokenization in {cleaned:?}"
    );
    for fragment in surviving_context {
        assert!(
            cleaned.contains(fragment),
            "context {fragment:?} should survive but is missing from {cleaned:?}"
        );
    }
}

fn assert_unchanged(text: &str) {
    assert_eq!(clean(text), text, "text must pass through untouched");
}

// drift-ack: url.anchored adds one `custom:url` detection to the committed core-no-policy
// bundle tokenization snapshot (11 -> 12 detections on the drift corpus). No pre-existing
// detection changes class, span, or shape. Rationale and measurements: CHANGELOG.md [Unreleased].
#[test]
fn drift_corpus_url_line_is_the_only_new_core_detection() {
    // Pins the shape the snapshot change records: the drift corpus' URL line is tokenized whole,
    // and the surrounding prose on that line is untouched.
    assert_url_removed(
        "URL fragment https://example.invalid/orders#id12345 remains a URL fragment.",
        "https://example.invalid/orders#id12345",
        &["URL fragment ", " remains a URL fragment."],
    );
}

// ---------------------------------------------------------------- anchor: scheme (169 of 232)

#[test]
fn https_scheme_host_only_is_tokenized() {
    assert_url_removed(
        "Reach the portal at https://portal.example.invalid for the update.",
        "https://portal.example.invalid",
        &["Reach the portal at ", " for the update."],
    );
}

#[test]
fn http_scheme_is_tokenized_like_https() {
    assert_url_removed(
        "Legacy mirror http://mirror.example.invalid is deprecated.",
        "http://mirror.example.invalid",
        &["Legacy mirror ", " is deprecated."],
    );
}

#[test]
fn uppercase_scheme_is_tokenized() {
    assert_url_removed(
        "See HTTPS://PORTAL.EXAMPLE.INVALID for details.",
        "HTTPS://PORTAL.EXAMPLE.INVALID",
        &["See ", " for details."],
    );
}

// ---------------------------------------------------------------- anchor: www. (63 of 232)

#[test]
fn www_prefixed_host_without_scheme_is_tokenized() {
    assert_url_removed(
        "Details live at www.profile.example.invalid today.",
        "www.profile.example.invalid",
        &["Details live at ", " today."],
    );
}

#[test]
fn uppercase_www_is_tokenized() {
    assert_url_removed(
        "Mirror WWW.PROFILE.EXAMPLE.INVALID is stale.",
        "WWW.PROFILE.EXAMPLE.INVALID",
        &["Mirror ", " is stale."],
    );
}

// ---------------------------------------------------------------- shape: has-path (83 of 232)

#[test]
fn scheme_with_path_segments_is_tokenized_whole() {
    assert_url_removed(
        "Account page https://profile.example.invalid/users/alice/settings was updated.",
        "https://profile.example.invalid/users/alice/settings",
        &["Account page ", " was updated."],
    );
}

#[test]
fn trailing_slash_is_part_of_the_url() {
    assert_url_removed(
        "Open https://portal.example.invalid/orders/ now.",
        "https://portal.example.invalid/orders/",
        &["Open ", " now."],
    );
}

#[test]
fn fragment_is_part_of_the_url() {
    // Mirrors the committed drift-corpus URL line shape.
    assert_url_removed(
        "URL fragment https://example.invalid/orders#id12345 remains a fragment.",
        "https://example.invalid/orders#id12345",
        &["URL fragment ", " remains a fragment."],
    );
}

// ---------------------------------------------------------------- query string (1 of 232)

#[test]
fn query_string_is_part_of_the_url() {
    assert_url_removed(
        "Search https://portal.example.invalid/find?q=alice&page=2 returned one row.",
        "https://portal.example.invalid/find?q=alice&page=2",
        &["Search ", " returned one row."],
    );
}

// ---------------------------------------------------------------- host label counts (57/154/21)

#[test]
fn two_label_host_is_tokenized() {
    assert_url_removed(
        "Root site https://example.invalid is live.",
        "https://example.invalid",
        &["Root site ", " is live."],
    );
}

#[test]
fn four_label_host_is_tokenized() {
    assert_url_removed(
        "Deep host https://eu.west.portal.example.invalid responded.",
        "https://eu.west.portal.example.invalid",
        &["Deep host ", " responded."],
    );
}

// ---------------------------------------------------------------- trailing-boundary rule
//
// The boundary rule gives back trailing sentence and markup punctuation. Measured: without the
// give-back, coverage is identical (232 spans / 6,385 bytes) but non-gold matched bytes appear in
// 107 holdout documents instead of 12.

#[test]
fn sentence_final_period_is_not_part_of_the_url() {
    let cleaned = clean("Log in at https://portal.example.invalid.");
    assert!(
        cleaned.ends_with('.'),
        "the sentence period must survive outside the token: {cleaned:?}"
    );
    assert!(!cleaned.contains("https://portal.example.invalid"));
}

#[test]
fn trailing_comma_in_a_list_is_not_part_of_the_url() {
    let cleaned = clean("Mirrors: https://a.example.invalid, https://b.example.invalid.");
    assert!(
        !cleaned.contains("a.example.invalid") && !cleaned.contains("b.example.invalid"),
        "both list members must be tokenized: {cleaned:?}"
    );
    assert!(
        cleaned.contains(", "),
        "the list separator must survive: {cleaned:?}"
    );
}

#[test]
fn enclosing_parentheses_are_not_part_of_the_url() {
    let cleaned = clean("Details (https://portal.example.invalid) follow.");
    assert!(!cleaned.contains("https://portal.example.invalid"));
    assert!(
        cleaned.contains('(') && cleaned.contains(')'),
        "both parentheses must survive outside the token: {cleaned:?}"
    );
}

#[test]
fn angle_bracket_markup_is_not_part_of_the_url() {
    let cleaned = clean("Link <https://portal.example.invalid> in the footer.");
    assert!(!cleaned.contains("https://portal.example.invalid"));
    assert!(
        cleaned.contains('<') && cleaned.contains('>'),
        "markup delimiters must survive outside the token: {cleaned:?}"
    );
}

#[test]
fn quoted_url_keeps_its_quotes_outside_the_token() {
    let cleaned = clean("Href is \"https://portal.example.invalid\" exactly.");
    assert!(!cleaned.contains("https://portal.example.invalid"));
    assert_eq!(
        cleaned.matches('"').count(),
        2,
        "both quotes must survive outside the token: {cleaned:?}"
    );
}

#[test]
fn interior_punctuation_is_kept_when_the_url_continues() {
    // A path that genuinely contains `,` and `:` mid-URL must not be truncated there.
    assert_url_removed(
        "Batch https://portal.example.invalid/ids/a,b,c:v2/list ran.",
        "https://portal.example.invalid/ids/a,b,c:v2/list",
        &["Batch ", " ran."],
    );
}

// ---------------------------------------------------------------- reference hosts (16 of 232)
//
// PRECISION OBLIGATION. This rule DOES tokenize documentation, repository,
// and example URLs. That is deliberate, and it is what the corpus asks for: of the 232 gold URL
// spans this rule covers, 16 are themselves documentation/repository/reference-host shaped, and
// ZERO of the 9 non-gold anchored matches are. The benchmark's labelling policy treats a
// reference URL inside a data-owner document as PII to protect, and axis 1 says an over-tokenized
// public URL is a recoverable ergonomics cost while an under-tokenized private one is a leak.
// Both are restorable through the manifest, so nothing is destroyed either way.

#[test]
fn documentation_url_is_tokenized_by_design() {
    assert_url_removed(
        "Full guide at https://docs.example.invalid/guide/getting-started explains it.",
        "https://docs.example.invalid/guide/getting-started",
        &["Full guide at ", " explains it."],
    );
}

#[test]
fn repository_url_is_tokenized_by_design() {
    assert_url_removed(
        "Source at https://github.example.invalid/org/repo builds cleanly.",
        "https://github.example.invalid/org/repo",
        &["Source at ", " builds cleanly."],
    );
}

#[test]
fn documentation_url_round_trips_through_restore() {
    // The axis-2 half of the precision argument: an over-tokenized reference URL is not lost.
    let session = Session::new(Scope::Ephemeral).expect("session");
    let text = "Guide: https://docs.example.invalid/guide/getting-started here.";
    let cleaned = clean_with(&pipeline(), &session, text);
    assert_ne!(cleaned, text, "the reference URL must have been tokenized");
    assert_eq!(
        session
            .restore_strict_text(&cleaned)
            .expect("reference URL restores"),
        text
    );
}

// ---------------------------------------------------------------- HARD NEGATIVES
//
// The bare-host tail (47 spans / 887 bytes / 12.3%) is out of scope: 97 of the 1,024 committed A4
// negative documents contain bare-host shapes, so a bare-host rule cannot clear the negative
// gate. These fixtures pin that boundary so a future widening has to break a test.

#[test]
fn bare_host_without_scheme_or_www_is_not_matched() {
    assert_unchanged("The vendor is portal.example.invalid according to the contract.");
}

#[test]
fn bare_host_with_a_path_is_not_matched() {
    assert_unchanged("Path reference example.invalid/orders/2026 appears in the ticket.");
}

#[test]
fn file_name_with_a_dotted_extension_is_not_matched() {
    assert_unchanged("Attachment quarterly.report.pdf was received.");
}

#[test]
fn package_version_string_is_not_matched() {
    // Shape drawn from the A4 temporal_numeric negative category.
    assert_unchanged("Numeric benchmark 00: package version v3.1.16 shipped.");
}

#[test]
fn scheme_prefix_alone_is_not_matched() {
    assert_unchanged("The scheme https:// is not a URL on its own.");
}

#[test]
fn www_prefix_alone_is_not_matched() {
    assert_unchanged("Prefix www. carries no host.");
}

#[test]
fn word_ending_in_www_is_not_matched() {
    assert_unchanged("The token notwww.example is not anchored.");
}

#[test]
fn decimal_number_is_not_matched() {
    assert_unchanged("Length 81.9 cm and price EUR 400.13 stay intact.");
}

// ---------------------------------------------------------------- interaction with Email

#[test]
fn an_email_address_outside_a_url_still_wins_its_own_class() {
    // Email sits at class-priority 90 and `custom:url` at 50, so an address is never absorbed
    // into a URL token. Measured: the holdout contains 0 userinfo-style URLs, so this is a
    // contract guard rather than a corpus-driven case.
    let rulepack = Rulepack::load(RulepackSource::Embedded(
        embedded("core").expect("core rulepack"),
    ))
    .expect("core loads");
    let mut policy = gaze::Policy::default();
    policy.rules = vec![
        RuleSpec::Class {
            class: PiiClass::Email,
            action: Action::Tokenize,
        },
        RuleSpec::Class {
            class: url_class(),
            action: Action::Tokenize,
        },
        RuleSpec::Default {
            action: Action::Preserve,
        },
    ];
    policy.rulepacks.bundled = vec!["core".to_string()];
    let pipeline = gaze_assembly::build_pipeline(
        &policy,
        &empty_context(),
        &[rulepack],
        &global_chain(),
        None,
    )
    .expect("pipeline");
    let session = Session::new(Scope::Ephemeral).expect("session");
    let (clean, manifest, _) = pipeline
        .clean_with_safety_net_detect_context(
            &session,
            RawDocument::Text(
                "Write alice@example.invalid or open https://portal.example.invalid now."
                    .to_string(),
            ),
            &[LocaleTag::Global],
            &DictionaryBundle::default(),
        )
        .expect("clean");
    let cleaned = match clean {
        CleanDocument::Text(text) => text,
        _ => panic!("expected text"),
    };
    assert!(!cleaned.contains("alice@example.invalid"));
    assert!(!cleaned.contains("https://portal.example.invalid"));
    assert_eq!(manifest.len(), 2, "one Email token and one URL token");
    assert!(manifest.iter().any(|span| span.class == PiiClass::Email));
    assert!(manifest.iter().any(|span| span.class == url_class()));
}

// Compact serialized text is a RawDocument::Text surface, not a parsed JSON document.
fn assert_exact_url_token(text: &str, url: &str) {
    let session = Session::new(Scope::Conversation("url-round-trip".to_string())).expect("session");
    let (clean, manifest, _) = pipeline()
        .clean_with_safety_net_detect_context(
            &session,
            RawDocument::Text(text.to_string()),
            &[LocaleTag::Global],
            &DictionaryBundle::default(),
        )
        .expect("clean");
    let CleanDocument::Text(cleaned) = clean else {
        panic!("expected text");
    };
    assert_eq!(manifest.len(), 1, "one whole URL token for {text:?}");
    let span = manifest.iter().next().expect("URL span");
    assert_eq!(span.class, url_class());
    assert!(
        span.origin.is_whole(),
        "URL must not be a residual fragment"
    );
    let start = text.find(url).expect("fixture contains URL");
    assert_eq!(
        span.raw_span,
        start..start + url.len(),
        "exact raw URL span"
    );
    let token = &cleaned[span.clean_span.clone()];
    assert_eq!(
        cleaned,
        text.replacen(url, token, 1),
        "only URL is replaced"
    );
    let entries = session.snapshot_entries();
    assert_eq!(entries.len(), 1, "one manifest-owned URL value");
    assert_eq!(entries[0].class, url_class());
    assert_eq!(
        entries[0].raw, url,
        "original serialized bytes are retained"
    );
    assert_eq!(entries[0].token, token);
    assert_eq!(
        session.restore_strict_text(&cleaned).expect("restore"),
        text
    );
    let imported = Session::import(session.export().expect("export")).expect("import");
    assert_eq!(
        imported
            .restore_strict_text(&cleaned)
            .expect("imported restore"),
        text
    );
}

#[test]
fn compact_json_keeps_fields_after_plain_url_and_restores_exactly() {
    assert_exact_url_token(
        r#"{"w":"https://portal.example.invalid/users/alice","amount":"1.500,00 EUR","status":"open"}"#,
        "https://portal.example.invalid/users/alice",
    );
}

#[test]
fn compact_json_escaped_scheme_and_path_are_one_restorable_url() {
    assert_exact_url_token(
        r#"{"w":"https:\/\/portal.example.invalid\/users\/alice","x":"1"}"#,
        r"https:\/\/portal.example.invalid\/users\/alice",
    );
}

#[test]
fn serialized_url_boundaries_keep_markup_punctuation_and_numeric_neighbors() {
    for (text, url) in [
        (
            r#"{"customer":{"website":"https://www.example.invalid/kontakt"},"amount":"1.500,00 EUR","status":"open"}"#,
            "https://www.example.invalid/kontakt",
        ),
        (
            r#"<a href="https://www.example.invalid/a">Link</a>"#,
            "https://www.example.invalid/a",
        ),
        (
            "<a href='https://portal.example.invalid/a'>Link</a>",
            "https://portal.example.invalid/a",
        ),
        (
            "<a href='https://portal.example.invalid/a' rel='next'>Link</a>",
            "https://portal.example.invalid/a",
        ),
        (
            "<p>https://portal.example.invalid/a</p>",
            "https://portal.example.invalid/a",
        ),
        (
            "[profile](https://portal.example.invalid/users/alice)",
            "https://portal.example.invalid/users/alice",
        ),
        (
            "<https://portal.example.invalid/a>",
            "https://portal.example.invalid/a",
        ),
        (
            "https://portal.example.invalid/a.,;:!?)]",
            "https://portal.example.invalid/a",
        ),
        (
            "https://portal.example.invalid/a}42",
            "https://portal.example.invalid/a",
        ),
        (
            "https://portal.example.invalid/a{42",
            "https://portal.example.invalid/a",
        ),
        (
            r#"{"w":"https://portal.example.invalid/ids/a,b,c:v2/list","n":81.9}"#,
            "https://portal.example.invalid/ids/a,b,c:v2/list",
        ),
        (
            r#"{"w":"https://portal.example.invalid/wiki/Conan_O'Brien?q=O'Brien","n":400.13}"#,
            "https://portal.example.invalid/wiki/Conan_O'Brien?q=O'Brien",
        ),
    ] {
        assert_exact_url_token(text, url);
    }
}

#[test]
fn escaped_url_units_cover_scheme_path_query_and_terminal_slash() {
    for url in [
        r"https:\/\/portal.example.invalid\/users\/alice",
        r"HTTP:\/\/PORTAL.EXAMPLE.INVALID\/users\/alice",
        r"https:\/\/www.example.invalid\/a",
        r"https:\//portal.example.invalid\/users/alice",
        r"https:/\/portal.example.invalid/users\/alice",
        r"https://portal.example.invalid\/users\/alice",
        r"https:\/\/portal.example.invalid/users/alice",
        r"www.example.invalid\/users\/alice",
        r"https:\/\/portal.example.invalid\/",
        r"https:\/\/portal.example.invalid\/wiki\/Conan_O'Brien?q=O'Brien&next=\/orders",
    ] {
        let text = format!(r#"{{"w":"{url}","n":42,"status":"open"}}"#);
        assert_exact_url_token(&text, url);
    }
}

#[test]
fn unsupported_json_escapes_are_boundaries_not_url_units() {
    for (text, url) in [
        (
            r"https://portal.example.invalid/a\qtail",
            "https://portal.example.invalid/a",
        ),
        (
            r"https://portal.example.invalid/a\",
            "https://portal.example.invalid/a",
        ),
        (
            r"https://portal.example.invalid/a\\/tail",
            "https://portal.example.invalid/a",
        ),
        (
            r#"{"w":"https://portal.example.invalid/a\"tail","n":42}"#,
            "https://portal.example.invalid/a",
        ),
    ] {
        assert_exact_url_token(text, url);
    }
    for text in [
        r"https:\/ is not a URL",
        r"https:\q\/portal.example.invalid",
        r"https:\\/\\/portal.example.invalid",
        r#"{"w":"portal.example.invalid\/users\/alice","n":42}"#,
    ] {
        assert_unchanged(text);
    }
}

#[test]
fn preserved_urls_leave_email_evidence_protected_inside_and_after_the_url() {
    for url in [
        "https://portal.example.invalid/users/alice@example.invalid",
        r"https:\/\/portal.example.invalid\/users\/alice@example.invalid",
    ] {
        let text = format!(r#"{{"w":"{url}","email":"bob@example.invalid","n":42}}"#);
        let session = Session::new(Scope::Ephemeral).expect("session");
        let (clean, manifest, _) = pipeline_with_actions(Action::Preserve, Action::Tokenize)
            .clean_with_safety_net_detect_context(
                &session,
                RawDocument::Text(text.clone()),
                &[LocaleTag::Global],
                &DictionaryBundle::default(),
            )
            .expect("clean");
        let CleanDocument::Text(cleaned) = clean else {
            panic!("expected text");
        };
        let mut expected = text.clone();
        assert_eq!(manifest.len(), 2, "two protected emails, preserved URL");
        for span in manifest.iter() {
            assert_eq!(span.class, PiiClass::Email);
            let raw = &text[span.raw_span.clone()];
            assert!(matches!(
                raw,
                "alice@example.invalid" | "bob@example.invalid"
            ));
            let token = &cleaned[span.clean_span.clone()];
            expected = expected.replacen(raw, token, 1);
        }
        assert_eq!(cleaned, expected, "only emails change under URL preserve");
        assert!(!cleaned.contains("alice@example.invalid"));
        assert!(!cleaned.contains("bob@example.invalid"));
        assert_eq!(
            session.restore_strict_text(&cleaned).expect("restore"),
            text
        );
    }
}

#[test]
fn escaped_url_regex_preserves_raw_evidence_without_canonical_rewriting() {
    let detector = configured_url_detector();
    let dictionaries = DictionaryBundle::default();
    let context = gaze_types::DetectContext::new(&[LocaleTag::Global], &dictionaries);
    let url = r"HTTPS:\/\/portal.example.invalid\/users\/alice\/";
    let text = format!(r#"{{"w":"{url}","n":42}}"#);
    let candidates = gaze_types::Recognizer::detect(&detector, &text, &context).expect("detect");
    assert_eq!(candidates.len(), 1);
    assert_eq!(&text[candidates[0].span.clone()], url);
    assert_eq!(
        candidates[0].canonical_form, None,
        "URL has no normalizer or validator"
    );
    assert_exact_url_token(&text, url);
}

#[test]
fn encoded_delimiters_and_unicode_context_keep_original_byte_coordinates() {
    let url = r"https:\/\/portal.example.invalid\/users\/alice%22%3C%3E%7B%7D%5C?q=O%27Brien";
    let text = format!(r#"é:{{"w":"{url}","status":"offen"}}"#);
    assert_exact_url_token(&text, url);
}

// JSON validation describes the fixture; detection still sees only the original source bytes.
fn assert_serialized_url(url: &str, decoded: &str) {
    let text = format!(r#"{{"context":"é","w":"{url}","n":42,"status":"open"}}"#);
    let parsed: serde_json::Value = serde_json::from_str(&text).expect("valid JSON fixture");
    assert_eq!(parsed["w"].as_str(), Some(decoded));
    assert_exact_url_token(&text, url);
}

#[test]
fn json_unicode_account_units_keep_complete_raw_manifest_ownership() {
    for (raw, decoded) in [
        (
            r"https://portal.example.invalid/users/\u0061lice",
            "https://portal.example.invalid/users/alice",
        ),
        (
            r"https://portal.example.invalid/users/a\u006cice",
            "https://portal.example.invalid/users/alice",
        ),
        (
            r"https://portal.example.invalid/users/alice\u0031",
            "https://portal.example.invalid/users/alice1",
        ),
        (
            r"https://docs.example.invalid/guide/\u0067etting-started",
            "https://docs.example.invalid/guide/getting-started",
        ),
    ] {
        assert_serialized_url(raw, decoded);
    }
}

#[test]
fn json_unicode_non_ascii_and_surrogate_units_restore_original_spelling() {
    for (raw, decoded) in [
        (
            r"https://portal.example.invalid/users/stra\u00dfe",
            "https://portal.example.invalid/users/straße",
        ),
        (
            r"https://portal.example.invalid/users/\u00DF",
            "https://portal.example.invalid/users/ß",
        ),
        (
            r"https://portal.example.invalid/users/\u6771\u4eac",
            "https://portal.example.invalid/users/東京",
        ),
        (
            r"https://portal.example.invalid/users/\uD83D\ude80",
            "https://portal.example.invalid/users/🚀",
        ),
        (
            r"https://portal.example.invalid/users/é\u00df",
            "https://portal.example.invalid/users/éß",
        ),
    ] {
        assert_serialized_url(raw, decoded);
    }
}

#[test]
fn json_unicode_path_query_and_structural_data_units_are_owned_raw() {
    for (raw, decoded) in [
        (
            r"https://portal.example.invalid/users\u002falice",
            "https://portal.example.invalid/users/alice",
        ),
        (
            r"https://portal.example.invalid/users/alice\u003fowner\u003d\u0061lice\u0026next\u003d\u002Forders\u0023settings",
            "https://portal.example.invalid/users/alice?owner=alice&next=/orders#settings",
        ),
        (
            r"https://portal.example.invalid/?owner=\u0061lice",
            "https://portal.example.invalid/?owner=alice",
        ),
        (
            r"https:\/\/portal.example.invalid\/users\/alice\u002F",
            "https://portal.example.invalid/users/alice/",
        ),
        (
            r"https://portal.example.invalid/users/\u0022\u003C\u003e\u007B\u007d\u005c",
            "https://portal.example.invalid/users/\"<>{}\\",
        ),
    ] {
        assert_serialized_url(raw, decoded);
    }
}

#[test]
fn json_unicode_scheme_slashes_mix_with_literal_and_escaped_slashes() {
    for scheme in [
        r"https://",
        r"https:/\/",
        r"https:/\u002f",
        r"https:/\u002F",
        r"https:\//",
        r"https:\/\/",
        r"https:\/\u002f",
        r"https:\/\u002F",
        r"https:\u002f/",
        r"https:\u002f\/",
        r"https:\u002f\u002f",
        r"https:\u002f\u002F",
        r"https:\u002F/",
        r"https:\u002F\/",
        r"https:\u002F\u002f",
        r"https:\u002F\u002F",
        r"HTTP:\u002f\u002F",
    ] {
        let raw = format!("{scheme}portal.example.invalid/users/\\u0061lice");
        let decoded = if scheme.starts_with("HTTP:") {
            "HTTP://portal.example.invalid/users/alice"
        } else {
            "https://portal.example.invalid/users/alice"
        };
        assert_serialized_url(&raw, decoded);
    }
}

#[test]
fn malformed_unicode_and_unsupported_escapes_remain_raw_boundaries() {
    for tail in [
        r"\u", r"\u0", r"\u00", r"\u006", r"\u00xz", r"\U0061", r"\qtail", r"\", r"\\u0061",
    ] {
        let url = "https://portal.example.invalid/users/alice";
        let text = format!(r#"{{"w":"{url}{tail}","n":42}}"#);
        assert_exact_url_token(&text, url);
    }
    for text in [
        r"https:\u002 is incomplete",
        r"https:\u002x\u002fportal.example.invalid",
        r"https:\U002f\u002fportal.example.invalid",
        r"https:\\u002f\\u002fportal.example.invalid",
        r#"{"w":"portal.example.invalid/users/\u0061lice","n":42}"#,
    ] {
        assert_unchanged(text);
    }
}

#[test]
fn unicode_serialized_regex_evidence_has_original_coordinates_and_no_canonical_form() {
    let detector = configured_url_detector();
    let dictionaries = DictionaryBundle::default();
    let context = gaze_types::DetectContext::new(&[LocaleTag::Global], &dictionaries);
    let url = r"https:\u002F\/portal.example.invalid/users/\uD83D\uDE80\u002f";
    let text = format!(r#"é:{{"w":"{url}","n":81.9}}"#);
    let candidates = gaze_types::Recognizer::detect(&detector, &text, &context).expect("detect");
    assert_eq!(candidates.len(), 1);
    assert_eq!(
        candidates[0].span,
        9..9 + url.len(),
        "UTF-8 source coordinates"
    );
    assert_eq!(&text[candidates[0].span.clone()], url);
    assert_eq!(candidates[0].canonical_form, None);
    assert_eq!(candidates[0].class, url_class());
    assert_eq!(candidates[0].source, "url.anchored");
    assert_eq!(candidates[0].recognizer_id, "url.anchored");
    assert_exact_url_token(&text, url);
}

// Mirror the rulepack construction used by assembly, including capture ownership.
fn configured_url_detector() -> gaze_recognizers::RegexDetector {
    let rulepack = Rulepack::load(RulepackSource::Embedded(embedded("core").expect("core")))
        .expect("core loads");
    let spec = rulepack
        .recognizers
        .iter()
        .find(|r| r.id == "url.anchored")
        .expect("URL rule");
    let gaze::RawMatch::Regex {
        pattern: Some(pattern),
        capture_groups,
        complete_labelled_value,
        ..
    } = &spec.matcher
    else {
        panic!("URL regex");
    };
    assert!(spec.validator.is_none());
    assert!(spec.normalizer.is_none());
    gaze_recognizers::RegexDetector::with_rulepack_fields(
        pattern,
        spec.class.clone(),
        &spec.id,
        spec.locales.clone(),
        spec.scoring.base,
        spec.scoring.priority,
        spec.token.family.as_deref().unwrap_or("counter"),
        capture_groups.clone(),
        spec.context
            .as_ref()
            .map(|c| c.exclusions.clone())
            .unwrap_or_default(),
        None,
        None,
    )
    .expect("actual configured Rust regex compiles")
    .with_rejection_pattern(
        spec.context
            .as_ref()
            .and_then(|c| c.reject_match_regex.as_deref()),
    )
    .expect("rejection pattern")
    .with_complete_labelled_value(*complete_labelled_value)
    .with_locale_basis(spec.locale_basis)
}

fn assert_exact_url_occurrences(text: &str, urls: &[&str]) {
    let detector = configured_url_detector();
    let dictionaries = DictionaryBundle::default();
    let context = gaze_types::DetectContext::new(&[LocaleTag::Global], &dictionaries);
    let candidates = gaze_types::Recognizer::detect(&detector, text, &context).expect("detect");
    assert_eq!(
        candidates.len(),
        urls.len(),
        "every occurrence, no markup candidate"
    );
    let session =
        Session::new(Scope::Conversation("url-html-round-trip".to_string())).expect("session");
    let (clean, manifest, _) = pipeline()
        .clean_with_safety_net_detect_context(
            &session,
            RawDocument::Text(text.to_string()),
            &[LocaleTag::Global],
            &dictionaries,
        )
        .expect("clean");
    let CleanDocument::Text(cleaned) = clean else {
        panic!("expected text");
    };
    let spans: Vec<_> = manifest.iter().collect();
    assert_eq!(spans.len(), urls.len());
    let mut cursor = 0;
    let mut expected = String::new();
    let mut tokens = std::collections::HashMap::new();
    for ((candidate, span), url) in candidates.iter().zip(spans).zip(urls) {
        let start = cursor + text[cursor..].find(url).expect("literal fixture URL");
        let end = start + url.len();
        assert_eq!(candidate.span, start..end, "capture owns only raw URL");
        assert_eq!(&text[candidate.span.clone()], *url);
        assert_eq!(candidate.class, url_class());
        assert_eq!(candidate.source, "url.anchored");
        assert_eq!(candidate.recognizer_id, "url.anchored");
        assert_eq!(candidate.canonical_form, None);
        assert_eq!(span.raw_span, start..end);
        assert_eq!(span.class, url_class());
        assert!(span.origin.is_whole());
        let token = &cleaned[span.clean_span.clone()];
        if let Some(previous) = tokens.insert(*url, token) {
            assert_eq!(previous, token, "repeated raw URL reuses its token");
        }
        expected.push_str(&text[cursor..start]);
        expected.push_str(token);
        cursor = end;
    }
    expected.push_str(&text[cursor..]);
    assert_eq!(
        cleaned, expected,
        "all non-URL HTML bytes survive unchanged"
    );
    let mut entries = session.snapshot_entries();
    assert_eq!(entries.len(), tokens.len());
    for entry in &entries {
        assert_eq!(entry.class, url_class());
        assert_eq!(
            tokens.get(entry.raw.as_str()).copied(),
            Some(entry.token.as_str()),
            "owner raw identity"
        );
    }
    assert_eq!(
        session
            .restore_strict_text(&cleaned)
            .expect("strict restore"),
        text
    );
    let imported = Session::import(session.export().expect("export")).expect("import");
    let mut imported_entries = imported.snapshot_entries();
    // HashMap iteration has no ordering contract; compare complete entries by token.
    entries.sort_by(|left, right| left.token.cmp(&right.token));
    imported_entries.sort_by(|left, right| left.token.cmp(&right.token));
    assert_eq!(imported_entries, entries);
    assert_eq!(
        imported
            .restore_strict_text(&cleaned)
            .expect("imported strict restore"),
        text
    );
}

#[test]
fn html_selfclosing_img_and_link_keep_single_quotes_and_ascii_whitespace() {
    let url = "https://portal.example.invalid/users/alice";
    for (tag, attribute) in [("img", "src"), ("link", "href")] {
        for whitespace in ["", " ", "\t", "\n", "\r", "\u{000B}", "\u{000C}", " \t\r\n"] {
            let text = format!("<{tag} {attribute}='{url}'{whitespace}/>");
            assert_exact_url_occurrences(&text, &[url]);
        }
    }
}

#[test]
fn html_selfclosing_capture_resumes_for_nearby_attributes_and_repeated_urls() {
    let one = "https://portal.example.invalid/users/alice";
    let two = r"https:\u002F\/portal.example.invalid/users/\uD83D\uDE80\u002f";
    let text = format!("é:<img alt='avatar' data-url='{one}' src='{one}'/><link data-state='ready' href='{two}' \t/><img src='{one}'/> units=81.9");
    assert_exact_url_occurrences(&text, &[one, one, two, one]);
}

#[test]
fn html_selfclosing_uses_complete_unicode_units_and_all_scheme_slash_pairs() {
    for left in ["/", r"\/", r"\u002f", r"\u002F"] {
        for right in ["/", r"\/", r"\u002f", r"\u002F"] {
            for suffix in [
                r"/users/\u0061lice",
                r"/\u00DF/é",
                r"/\uD83D\ude80",
                r"/users\u002falice?owner=\u0061lice\u002F",
                r"/\u0022\u003C\u003e\u007B\u007d\u005c",
            ] {
                let url = format!("https:{left}{right}portal.example.invalid{suffix}");
                let text = format!("ß:<img src='{url}'/> count=42");
                assert_exact_url_occurrences(&text, &[&url]);
            }
        }
    }
}

#[test]
fn quoted_selfclosing_lexical_boundary_and_malformed_lookalikes_keep_fallback() {
    let url = "https://portal.example.invalid/users/alice";
    // A complete quoted delimiter is lexical, even outside an HTML document.
    assert_exact_url_occurrences(&format!("note '{url}'/> units=81.9"), &[url]);
    for (suffix, owned_tail) in [
        ("/>", "/"),
        ("' / >", "'"),
        ("'/", "'/"),
        ("'/>later", ""),
        ("'\u{00a0}/>", ""),
        ("'\\t/>", ""),
        ("'//>", "'//"),
    ] {
        let text = format!("<img src='{url}{suffix}");
        let expected = format!("{url}{owned_tail}");
        // Whitespace and unsupported raw backslashes stop the generic fallback.
        let expected = if suffix == "' / >" {
            url.to_string()
        } else {
            expected
        };
        assert_exact_url_occurrences(&text, &[&expected]);
    }
    for text in [
        "<img src='portal.example.invalid/users/alice'/>",
        "<img src='https:'/>",
        "www.",
    ] {
        assert_unchanged(text);
        let dictionaries = DictionaryBundle::default();
        let context = gaze_types::DetectContext::new(&[LocaleTag::Global], &dictionaries);
        assert!(
            gaze_types::Recognizer::detect(&configured_url_detector(), text, &context)
                .expect("detect")
                .is_empty()
        );
    }
}

#[test]
fn html_capture_preserves_literal_and_encoded_apostrophe_uri_data() {
    for url in [
        "https://portal.example.invalid/O'/notes",
        "https://portal.example.invalid/O'Brien?q=O'Brien",
        "https://portal.example.invalid/O%27/notes",
        r"https:\/\/portal.example.invalid/O'/notes",
        r"https://portal.example.invalid/O\u0027/notes",
    ] {
        for text in [
            format!(r#"{{"w":"{url}","n":42}}"#),
            format!(r#"<img src="{url}"/>"#),
            format!("Visit {url} now."),
        ] {
            assert_exact_url_occurrences(&text, &[url]);
        }
        if !url.contains('\'') {
            assert_exact_url_occurrences(&format!("<link href='{url}'/>"), &[url]);
        }
    }
}

#[test]
fn html_preserved_url_keeps_residual_email_ownership_and_round_trip() {
    let url = "https://portal.example.invalid/users/alice@example.invalid";
    let text = format!("é:<img src='{url}'/> contact=bob@example.invalid units=81.9");
    let session =
        Session::new(Scope::Conversation("url-html-residual".to_string())).expect("session");
    let (clean, manifest, _) = pipeline_with_actions(Action::Preserve, Action::Tokenize)
        .clean_with_safety_net_detect_context(
            &session,
            RawDocument::Text(text.clone()),
            &[LocaleTag::Global],
            &DictionaryBundle::default(),
        )
        .expect("clean");
    let CleanDocument::Text(cleaned) = clean else {
        panic!("expected text");
    };
    assert_eq!(
        manifest.len(),
        2,
        "both emails remain protected under URL preserve"
    );
    let mut expected = text.clone();
    for span in manifest.iter() {
        assert_eq!(span.class, PiiClass::Email);
        let raw = &text[span.raw_span.clone()];
        assert!(matches!(
            raw,
            "alice@example.invalid" | "bob@example.invalid"
        ));
        expected = expected.replacen(raw, &cleaned[span.clean_span.clone()], 1);
    }
    assert_eq!(cleaned, expected);
    assert_eq!(
        session.restore_strict_text(&cleaned).expect("restore"),
        text
    );
    let imported = Session::import(session.export().expect("export")).expect("import");
    assert_eq!(
        imported
            .restore_strict_text(&cleaned)
            .expect("imported restore"),
        text
    );
}
