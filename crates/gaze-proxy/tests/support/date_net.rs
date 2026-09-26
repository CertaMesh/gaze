//! Deterministic stand-in for the Nym date finding under the `gaze setup` policy (todo 3847).
//!
//! Flags dates the primary rules do not cover, as `Custom("date")`, the class Nym maps its
//! `DATE_OF_BIRTH` label to. It sees only the scan text, where owned tokens are already
//! replaced, so it never flags token bytes.
use gaze::{
    LeakKind, LeakSuspect, LocaleTag, PiiClass, SafetyNet, SafetyNetContext, SafetyNetError,
};
use regex::Regex;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc, OnceLock,
};

pub const ISO_DATE: &str = "1971-05-30";
pub const WRITTEN_DATE: &str = "30 May 1971";

fn dates() -> &'static Regex {
    static DATES: OnceLock<Regex> = OnceLock::new();
    DATES.get_or_init(|| {
        Regex::new(
            r"\b\d{4}-\d{2}-\d{2}\b|\b\d{1,2}\.\d{1,2}\.\d{4}\b|\b\d{1,2}/\d{1,2}/\d{4}\b|\b\d{1,2} (?:January|February|March|April|May|June|July|August|September|October|November|December) \d{4}\b|\b(?:January|February|March|April|May|June|July|August|September|October|November|December) \d{1,2}, \d{4}\b|\b\d{8}\b",
        )
        .unwrap()
    })
}

pub struct DateNet {
    pub hits: Arc<AtomicUsize>,
}

impl SafetyNet for DateNet {
    fn id(&self) -> &str {
        "synthetic-date-net"
    }
    fn supported_locales(&self) -> &[LocaleTag] {
        &[LocaleTag::Global]
    }
    fn check(
        &self,
        text: &str,
        _: SafetyNetContext<'_>,
    ) -> Result<Vec<LeakSuspect>, SafetyNetError> {
        self.hits.fetch_add(1, Ordering::SeqCst);
        Ok(dates()
            .find_iter(text)
            .map(|found| {
                LeakSuspect::new(
                    found.range(),
                    PiiClass::Custom("date".to_string()),
                    self.id(),
                    Some(0.95),
                    LeakKind::Uncovered,
                    "DATE_OF_BIRTH>=0.9",
                    None,
                )
            })
            .collect())
    }
}

/// Dated prompts in several shapes, with cue words and a primary-detected email beside them.
pub const PARITY_INPUTS: &[&str] = &[
    "Invoice date 1971-05-30.",
    "born on 30 May 1971.",
    "DOB: 30.05.1971, contact alice@example.invalid",
    "Date of birth 05/30/1971 and again May 30, 1971.",
    "geboren am 30.05.1971",
    "ref 19710530 issued 1971-05-30",
    "no date here",
];

/// `text` with every token's session hex replaced, so two sessions compare by class and place.
pub fn without_session_hex(text: &str) -> String {
    static HEX: OnceLock<Regex> = OnceLock::new();
    HEX.get_or_init(|| Regex::new(r"<[0-9a-f]{8}:").unwrap())
        .replace_all(text, "<SESSION:")
        .into_owned()
}

/// What `gaze clean` and `gaze daemon` produce for `text`: both call this library entry point
/// with the default Resolve policy.
pub fn clean_reference(pipeline: &gaze::Pipeline, text: &str) -> String {
    let session = gaze::Session::new(gaze::Scope::Ephemeral).unwrap();
    let (clean, _, _) = pipeline
        .clean_with_safety_net_policy_detect_context(
            &session,
            gaze::RawDocument::Text(text.to_string()),
            &[LocaleTag::Global],
            &gaze::DictionaryBundle::default(),
            gaze::SafetyNetPolicy::default(),
        )
        .unwrap();
    let gaze::CleanDocument::Text(clean) = clean else {
        panic!("text in, text out");
    };
    without_session_hex(&clean)
}
