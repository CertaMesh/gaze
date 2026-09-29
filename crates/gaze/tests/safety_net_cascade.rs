//! A safety net primed by the class names of the tokens it reads.
//!
//! Nym reads the scan text with the tokens in it. Probed directly, it flags the
//! `2` in `<hex:Custom:building_number_4>(2)` and nothing in `<hex:Token_4>(2)`:
//! a token's class name primes it to flag the plain number beside it. Each
//! resolve round's tokens could therefore surface one more suspect, the one
//! follow-up round ran out, and the `Redact` fallback deleted a value that was
//! never at risk, so the document stopped restoring exactly.
//!
//! The stand-in below behaves the same way: its first scan flags one number,
//! and every later scan flags the next number only while the scan text still
//! names the `building_number` class. Hiding class names from the net fixed
//! the cascade but cost more recall than it saved (measured), so the
//! `Redact` fallback now tokenizes a residual set it can plan completely
//! instead of deleting it.
use gaze::*;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

struct Primed {
    words: Vec<&'static str>,
    scans: Arc<AtomicUsize>,
}

impl SafetyNet for Primed {
    fn id(&self) -> &str {
        "primed"
    }
    fn supported_locales(&self) -> &[LocaleTag] {
        &[]
    }
    fn check(
        &self,
        text: &str,
        _: SafetyNetContext<'_>,
    ) -> std::result::Result<Vec<LeakSuspect>, SafetyNetError> {
        let scan = self.scans.fetch_add(1, Ordering::SeqCst);
        if scan > 0 && !text.contains("building_number") {
            return Ok(Vec::new());
        }
        let Some(word) = self.words.get(scan) else {
            return Ok(Vec::new());
        };
        // Whole word on exposed text, never a digit inside a token's ordinal.
        let Some(start) = text
            .match_indices(word)
            .map(|(start, _)| start)
            .find(|&start| {
                let before = text[..start].chars().next_back();
                let after = text[start + word.len()..].chars().next();
                !before.is_some_and(|c| c.is_alphanumeric() || c == '_')
                    && !after.is_some_and(char::is_alphanumeric)
            })
        else {
            return Ok(Vec::new());
        };
        Ok(vec![LeakSuspect::new(
            start..start + word.len(),
            PiiClass::custom("building_number").unwrap(),
            "primed",
            Some(0.9),
            LeakKind::Uncovered,
            "BUILDING_NUMBER>=0.5",
            None,
        )])
    }
}

const RAW: &str = "Journal, 15(2), pages 45 to 60.\n";

/// Words of `text` outside tokens and redaction markers.
fn exposed_words(text: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut depth = 0usize;
    let mut word = String::new();
    for ch in text.chars() {
        match ch {
            '<' | '[' => depth += 1,
            '>' | ']' => depth = depth.saturating_sub(1),
            _ if depth == 0 && ch.is_alphanumeric() => {
                word.push(ch);
                continue;
            }
            _ => {}
        }
        if !word.is_empty() {
            words.push(std::mem::take(&mut word));
        }
    }
    words.extend((!word.is_empty()).then_some(word));
    words
}

#[test]
fn a_primed_cascade_is_tokenized_to_the_end_and_restores_exactly() {
    let scans = Arc::new(AtomicUsize::new(0));
    let pipeline = Pipeline::builder()
        .rule(DefaultRule::new(Action::Tokenize))
        .build()
        .unwrap()
        .with_safety_net(Primed {
            words: vec!["45", "15", "2"],
            scans: Arc::clone(&scans),
        });
    let session = Session::new(Scope::Ephemeral).unwrap();
    let (clean, _, _) = pipeline
        .clean_with_safety_net_policy_detect_context(
            &session,
            RawDocument::Text(RAW.into()),
            &[LocaleTag::EnUs],
            &DictionaryBundle::default(),
            SafetyNetPolicy::new(SafetyNetMode::Resolve, SafetyNetFallback::Redact),
        )
        .unwrap();
    let CleanDocument::Text(text) = clean else {
        panic!("text document")
    };
    // The first resolve round takes `45`, the follow-up round `15`, and the fallback finds `2`
    // fully plannable, so it tokenizes it instead of deleting it.
    assert!(!text.contains("[REDACTED"), "{text}");
    for word in ["45", "15", "2"] {
        assert!(
            !exposed_words(&text).iter().any(|w| w == word),
            "{word} in {text}"
        );
    }
    assert_eq!(session.restore_strict_text(&text).unwrap(), RAW);
    // Three scans that each found a link, then the terminal scan that found nothing.
    assert_eq!(scans.load(Ordering::SeqCst), 4);
}
