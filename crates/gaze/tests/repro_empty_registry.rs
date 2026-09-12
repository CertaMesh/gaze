#![cfg(feature = "bundled-recognizers")]

use gaze::{
    Action, ClassRule, DefaultRule, Detection, Detector, DictionaryBundle, LeakSuspect, LocaleTag,
    PiiClass, Pipeline, ProtectionContext, ProtectionError, SafetyNet, SafetyNetContext,
    SafetyNetError, Scope, Session,
};
use gaze_recognizers::{
    LocaleAwareModel, LocaleAwareModelRegistry, ModelError, ModelHints, ModelInput, ModelSpan,
};
use std::sync::{Arc, Mutex};

const EMAIL: &str = "alice@example.invalid";

#[derive(Clone)]
struct Primary;
impl Detector for Primary {
    fn detect(&self, input: &str) -> Vec<Detection> {
        input
            .match_indices(EMAIL)
            .map(|(start, _)| {
                Detection::new(start..start + EMAIL.len(), PiiClass::Email, "synthetic")
            })
            .collect()
    }
}

#[derive(Clone)]
struct RecordingNet {
    seen: Arc<Mutex<Vec<String>>>,
    locale: LocaleTag,
}
impl SafetyNet for RecordingNet {
    fn id(&self) -> &str {
        "recording"
    }
    fn supported_locales(&self) -> &[LocaleTag] {
        std::slice::from_ref(&self.locale)
    }
    fn check(
        &self,
        text: &str,
        _: SafetyNetContext<'_>,
    ) -> Result<Vec<LeakSuspect>, SafetyNetError> {
        self.seen.lock().unwrap().push(text.to_string());
        Ok(vec![])
    }
}

struct DeOnlyModel;
impl LocaleAwareModel for DeOnlyModel {
    fn name(&self) -> &str {
        "de-only"
    }
    fn native_locales(&self) -> &[LocaleTag] {
        &[LocaleTag::DeDe]
    }
    fn infer(&self, _: ModelInput, _: ModelHints) -> Result<Vec<ModelSpan>, ModelError> {
        Ok(vec![])
    }
}

fn empty_registry() -> LocaleAwareModelRegistry {
    LocaleAwareModelRegistry::new()
}

#[test]
fn some_empty_registry_without_custom_nets_succeeds_via_early_return() {
    let pipeline = Pipeline::builder()
        .detector(Primary)
        .rule(ClassRule::new(PiiClass::Email, Action::Tokenize))
        .rule(DefaultRule::new(Action::Preserve))
        .register_safety_net_registry(empty_registry())
        .build()
        .expect("pipeline");
    let dictionaries = DictionaryBundle::default();
    let context = ProtectionContext::strict(&[LocaleTag::Global], &dictionaries);
    assert_eq!(pipeline.validate_protection_context(context), Ok(()));
    let session = Session::new(Scope::Ephemeral).expect("session");
    let mut tx = session.begin_transaction();
    assert!(pipeline
        .protect_text_transaction(&mut tx, "benign", context)
        .is_ok());
}

#[test]
fn some_empty_registry_with_custom_net_passes_validate_and_runtime() {
    let seen = Arc::new(Mutex::new(vec![]));
    let pipeline = Pipeline::builder()
        .detector(Primary)
        .rule(ClassRule::new(PiiClass::Email, Action::Tokenize))
        .rule(DefaultRule::new(Action::Preserve))
        .register_safety_net(RecordingNet {
            seen: seen.clone(),
            locale: LocaleTag::Global,
        })
        .register_safety_net_registry(empty_registry())
        .build()
        .expect("pipeline");
    let dictionaries = DictionaryBundle::default();
    let context = ProtectionContext::strict(&[LocaleTag::EnUs], &dictionaries);
    assert_eq!(pipeline.validate_protection_context(context), Ok(()));
    let session = Session::new(Scope::Ephemeral).expect("session");
    let mut tx = session.begin_transaction();
    assert!(pipeline
        .protect_text_transaction(&mut tx, "benign", context)
        .is_ok());
    assert_eq!(seen.lock().unwrap().len(), 1);
}

#[test]
fn none_registry_with_custom_net_runs_custom_net() {
    let seen = Arc::new(Mutex::new(vec![]));
    let pipeline = Pipeline::builder()
        .detector(Primary)
        .rule(ClassRule::new(PiiClass::Email, Action::Tokenize))
        .rule(DefaultRule::new(Action::Preserve))
        .register_safety_net(RecordingNet {
            seen: seen.clone(),
            locale: LocaleTag::Global,
        })
        .build()
        .expect("pipeline");
    let dictionaries = DictionaryBundle::default();
    let context = ProtectionContext::strict(&[LocaleTag::EnUs], &dictionaries);
    assert_eq!(pipeline.validate_protection_context(context), Ok(()));
    let session = Session::new(Scope::Ephemeral).expect("session");
    let mut tx = session.begin_transaction();
    assert!(pipeline
        .protect_text_transaction(&mut tx, "benign", context)
        .is_ok());
    assert_eq!(seen.lock().unwrap().len(), 1);
}

#[test]
fn nonempty_registry_without_coverage_still_fails_closed() {
    let registry = LocaleAwareModelRegistry::from_backends(vec![
        Box::new(DeOnlyModel) as Box<dyn LocaleAwareModel>
    ]);
    let pipeline = Pipeline::builder()
        .detector(Primary)
        .rule(ClassRule::new(PiiClass::Email, Action::Tokenize))
        .rule(DefaultRule::new(Action::Preserve))
        .register_safety_net_registry(registry)
        .build()
        .expect("pipeline");
    let dictionaries = DictionaryBundle::default();
    let context = ProtectionContext::strict(&[LocaleTag::EnUs], &dictionaries);
    assert_eq!(
        pipeline.validate_protection_context(context),
        Err(ProtectionError::UnsupportedCoverage)
    );
    let session = Session::new(Scope::Ephemeral).expect("session");
    let mut tx = session.begin_transaction();
    assert_eq!(
        pipeline.protect_text_transaction(&mut tx, "benign", context),
        Err(ProtectionError::UnsupportedCoverage)
    );
}

#[test]
fn some_empty_registry_with_custom_net_observer_scan_succeeds() {
    let seen = Arc::new(Mutex::new(vec![]));
    let pipeline = Pipeline::builder()
        .detector(Primary)
        .rule(ClassRule::new(PiiClass::Email, Action::Tokenize))
        .rule(DefaultRule::new(Action::Preserve))
        .register_safety_net(RecordingNet {
            seen: seen.clone(),
            locale: LocaleTag::Global,
        })
        .register_safety_net_registry(empty_registry())
        .build()
        .expect("pipeline");
    let session = Session::new(Scope::Ephemeral).expect("session");
    let result = pipeline.scan_safety_nets(&session, "benign", &[LocaleTag::EnUs]);
    assert!(
        result.is_ok(),
        "observer scan over an empty registry should skip, not fail: {result:?}"
    );
    assert_eq!(seen.lock().unwrap().len(), 1);
}

#[test]
fn some_empty_registry_with_custom_net_observer_scan_with_dictionaries_succeeds() {
    let seen = Arc::new(Mutex::new(vec![]));
    let pipeline = Pipeline::builder()
        .detector(Primary)
        .rule(ClassRule::new(PiiClass::Email, Action::Tokenize))
        .rule(DefaultRule::new(Action::Preserve))
        .register_safety_net(RecordingNet {
            seen: seen.clone(),
            locale: LocaleTag::Global,
        })
        .register_safety_net_registry(empty_registry())
        .build()
        .expect("pipeline");
    let session = Session::new(Scope::Ephemeral).expect("session");
    let dictionaries = DictionaryBundle::default();
    let result = pipeline.scan_safety_nets_with_dictionaries(
        &session,
        "benign",
        &[LocaleTag::EnUs],
        &dictionaries,
    );
    assert!(
        result.is_ok(),
        "observer scan_with_dictionaries over an empty registry should skip, not fail: {result:?}"
    );
    assert_eq!(seen.lock().unwrap().len(), 1);
}
