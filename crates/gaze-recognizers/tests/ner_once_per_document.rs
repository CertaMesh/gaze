//! NER inference runs once per document, however long the locale chain.
//!
//! NER is a document-basis recognizer active for `global`, so the registry offers it every
//! locale-chain step. Its output ignores the step locale, so every step after the first recomputed
//! the same spans and then dropped them as already claimed; under the 15-step `gaze setup` chain
//! that was 15 inferences per document. The registry now reuses the first result, and these tests
//! pin both the call count and that per-span locale fall-through gives byte-identical output.

#![cfg(feature = "test-support")]

use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use gaze::{
    Action, CleanDocument, Context, DictionaryBundle, LocaleChain, LocaleTag, PiiClass,
    RawDocument, RuleSpec, Rulepack, RulepackSource, Scope, Session,
};
use gaze_recognizers::{embedded, NerOptions, NerRecognizer};
use gaze_types::{
    Candidate, DetectContext, DetectError, LocaleBasis, Recognizer, RedactionEntry,
    RedactionLogError, RedactionLogger,
};

#[path = "support/token_assertions.rs"]
mod token_assertions;
use token_assertions::without_tokens;

/// Forwards to the test-support NER recognizer and counts `detect` calls. `force_per_step`
/// reports the recognizer as locale-dependent, which is the pre-fix detection path.
struct CountingNer {
    inner: NerRecognizer,
    calls: Arc<AtomicUsize>,
    force_per_step: bool,
}

impl Recognizer for CountingNer {
    fn id(&self) -> &str {
        self.inner.id()
    }
    fn supported_class(&self) -> &PiiClass {
        self.inner.supported_class()
    }
    fn possible_classes(&self) -> Vec<PiiClass> {
        self.inner.possible_classes()
    }
    fn detect(&self, input: &str, ctx: &DetectContext<'_>) -> Result<Vec<Candidate>, DetectError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.inner.detect(input, ctx)
    }
    fn token_family(&self) -> &str {
        self.inner.token_family()
    }
    fn locales(&self) -> &[LocaleTag] {
        self.inner.locales()
    }
    fn locale_basis(&self) -> LocaleBasis {
        self.inner.locale_basis()
    }
    fn detect_is_locale_invariant(&self) -> bool {
        !self.force_per_step && self.inner.detect_is_locale_invariant()
    }
}

#[derive(Clone, Default)]
struct MemoryLogger(Arc<Mutex<Vec<RedactionEntry>>>);

impl RedactionLogger for MemoryLogger {
    fn log(&self, entry: &RedactionEntry) -> Result<(), RedactionLogError> {
        self.0.lock().expect("entries").push(entry.clone());
        Ok(())
    }
}

/// The `gaze setup` chain shape: 14 locales plus `global`.
fn fifteen_step_chain() -> Vec<LocaleTag> {
    let mut chain = vec![
        LocaleTag::EnUs,
        LocaleTag::DeDe,
        LocaleTag::DeAt,
        LocaleTag::DeCh,
        LocaleTag::EnGb,
        LocaleTag::EnIe,
        LocaleTag::EnAu,
        LocaleTag::EnCa,
    ];
    chain.extend(
        ["en-IN", "hi-IN", "fr-FR", "es-ES", "nl-NL", "pt-BR"]
            .map(|tag| LocaleTag::Other(tag.to_string())),
    );
    chain.push(LocaleTag::Global);
    chain
}

fn postal_class() -> PiiClass {
    PiiClass::custom("postal_code").expect("valid custom class")
}

/// Everything one clean produces, with the per-run audit fields cleared.
#[derive(Debug, PartialEq)]
struct Outcome {
    clean: String,
    emitted: String,
    manifest: String,
    audit: Vec<RedactionEntry>,
    ner_calls: usize,
}

fn clean(chain: &[LocaleTag], text: &str, force_per_step: bool) -> Outcome {
    let rulepack = Rulepack::load(RulepackSource::Embedded(
        embedded("core").expect("core rulepack"),
    ))
    .expect("core loads");
    let mut policy = gaze::Policy::default();
    policy.rules = vec![
        RuleSpec::Class {
            class: PiiClass::Name,
            action: Action::Tokenize,
        },
        RuleSpec::Class {
            class: postal_class(),
            action: Action::Tokenize,
        },
        RuleSpec::Default {
            action: Action::Preserve,
        },
    ];
    policy.rulepacks.bundled = vec!["core".to_string()];
    policy.rulepacks.auto_activate_locale_gated = false;
    let context = Context {
        dictionaries: std::collections::HashMap::new(),
        class_map: std::collections::HashMap::new(),
        fields: serde_json::Map::new(),
    };
    let locale_chain = LocaleChain::merge_cli_policy_rulepack_default(None, None, Some(chain));
    let calls = Arc::new(AtomicUsize::new(0));
    let ner = CountingNer {
        inner: NerRecognizer::load_with_options(
            Path::new("__gaze_test_fixed_ner"),
            NerOptions::default(),
        )
        .expect("test support recognizer"),
        calls: Arc::clone(&calls),
        force_per_step,
    };
    let logger = MemoryLogger::default();
    let pipeline =
        gaze_assembly::build_pipeline_builder(&policy, &context, &[rulepack], &locale_chain, None)
            .expect("pipeline builder")
            .recognizer(ner)
            .redaction_logger(logger.clone())
            .build()
            .expect("pipeline");
    let session =
        Session::new_with_session_hex_for_tests(Scope::Ephemeral, [0x5e, 0x55, 0x10, 0x4e])
            .expect("fixed session");
    let (clean, emitted, _) = pipeline
        .clean_with_safety_net_detect_context(
            &session,
            RawDocument::Text(text.to_string()),
            chain,
            &DictionaryBundle::default(),
        )
        .expect("clean");
    let CleanDocument::Text(clean) = clean else {
        panic!("expected text");
    };
    let audit = logger
        .0
        .lock()
        .expect("entries")
        .iter()
        .cloned()
        .map(|mut entry| {
            entry.created_at = 0;
            entry.session_id = None;
            entry
        })
        .collect();
    // The session lists entries in hash-map order; sort so equal manifests compare equal.
    let mut manifest = session.snapshot_entries();
    manifest.sort_by(|a, b| a.token.cmp(&b.token));
    Outcome {
        clean,
        emitted: format!("{emitted:?}"),
        manifest: format!("{manifest:?}"),
        audit,
        ner_calls: calls.load(Ordering::SeqCst),
    }
}

const NAME_DOC: &str = "Please forward this to Alice Example today.";

#[test]
fn ner_detect_runs_once_per_document_under_a_fifteen_step_chain() {
    let outcome = clean(&fifteen_step_chain(), NAME_DOC, false);

    assert_eq!(outcome.ner_calls, 1);
    assert!(!without_tokens(&outcome.clean).contains("Alice"));
}

// Control for the test above: the same chain really offers NER all 15 steps, so a count of one
// comes from the reuse and not from NER dropping out of the chain.
#[test]
fn per_step_detection_calls_ner_at_every_chain_step() {
    let outcome = clean(&fifteen_step_chain(), NAME_DOC, true);

    assert_eq!(outcome.ner_calls, 15);
}

/// Out-of-corpus probe: locale-specific postal rules that fall through per span (`postal.at_ch`
/// under de-AT, `postal.de` under de-DE), cue-anchored Name rules that share NER's class, and an
/// NER name in one document. Reusing NER's candidates must give the same clean text, emitted
/// spans, manifest and audit rows as calling it at every step.
#[test]
fn mixed_locale_document_matches_per_step_detection() {
    let doc = "Von: Max Beispiel\nWien: 4020 Musterstadt. Berlin: 10115 Musterberg.\n\
               From: Jane Sample\nForwarded for Alice Example, Springfield, IL 90210.";
    let chains = [
        vec![LocaleTag::DeAt, LocaleTag::DeDe],
        vec![LocaleTag::DeDe, LocaleTag::DeAt],
        vec![LocaleTag::Global, LocaleTag::DeAt, LocaleTag::DeDe],
        fifteen_step_chain(),
    ];
    for chain in chains {
        let reused = clean(&chain, doc, false);
        let per_step = clean(&chain, doc, true);

        assert_eq!(reused.ner_calls, 1, "{chain:?}");
        // The chain gains a trailing `global` step when it lacks one.
        let steps = chain.len() + usize::from(!chain.contains(&LocaleTag::Global));
        assert_eq!(per_step.ner_calls, steps, "{chain:?}");
        assert_eq!(
            Outcome {
                ner_calls: 0,
                ..reused
            },
            Outcome {
                ner_calls: 0,
                ..per_step
            },
            "{chain:?}"
        );
    }

    let both = clean(&[LocaleTag::DeAt, LocaleTag::DeDe], doc, false);
    let visible = without_tokens(&both.clean);
    assert!(!visible.contains("4020"), "{visible}");
    assert!(!visible.contains("10115"), "{visible}");
    assert!(!visible.contains("Alice"), "{visible}");
}
