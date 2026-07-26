//! Regression fixtures for the `url.anchored` core recognizer (solo todo #2254).
//!
//! EVERY positive fixture here encodes a structural shape that was MEASURED in the Dataiku EN/DE
//! holdout, not a phrasing invented alongside the implementation. The previous attempt at this
//! bucket (todo #2412) shipped 962 green tests for cue phrasings — "my profile is <url>" — that
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
    }
}

fn url_class() -> PiiClass {
    PiiClass::custom("url")
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
    let rulepack = Rulepack::load(RulepackSource::Embedded(
        embedded("core").expect("core rulepack"),
    ))
    .expect("core loads");
    let mut policy = gaze::Policy::default();
    policy.rules = vec![
        RuleSpec::Class {
            class: url_class(),
            action: Action::Tokenize,
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
// PRECISION OBLIGATION (todo #2254 item 3). This rule DOES tokenize documentation, repository,
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
