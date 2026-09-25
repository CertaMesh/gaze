#![cfg(feature = "bundled-recognizers")]

use gaze::{LocaleTag, PiiClass, Pipeline, Scope, Session};
use gaze_recognizers::{
    LocaleAwareModel, LocaleAwareModelRegistry, ModelError, ModelHints, ModelInput, ModelSpan,
};
use std::sync::{Arc, Mutex};

struct TestModel {
    name: &'static str,
    locales: Vec<LocaleTag>,
    class: PiiClass,
    span: std::ops::Range<usize>,
}

impl TestModel {
    fn new(
        name: &'static str,
        locales: Vec<LocaleTag>,
        class: PiiClass,
        span: std::ops::Range<usize>,
    ) -> Self {
        Self {
            name,
            locales,
            class,
            span,
        }
    }
}

impl LocaleAwareModel for TestModel {
    fn name(&self) -> &str {
        self.name
    }

    fn native_locales(&self) -> &[LocaleTag] {
        &self.locales
    }

    fn infer(&self, input: ModelInput, _hints: ModelHints) -> Result<Vec<ModelSpan>, ModelError> {
        Ok(vec![ModelSpan {
            text: input.text[self.span.clone()].to_string(),
            byte_range: self.span.clone(),
            class: self.class.clone(),
            confidence: Some(0.99),
            model_name: self.name.to_string(),
        }])
    }
}

#[test]
fn safety_net_registry_routes_by_active_locale() {
    let registry = LocaleAwareModelRegistry::from_backends(vec![
        Box::new(TestModel::new(
            "backend-a",
            vec![LocaleTag::EnUs],
            PiiClass::Name,
            0..5,
        )),
        Box::new(TestModel::new(
            "backend-b",
            vec![LocaleTag::DeDe],
            PiiClass::Location,
            6..10,
        )),
    ]);
    let pipeline = Pipeline::builder()
        .build()
        .expect("pipeline")
        .with_safety_net_registry(registry);
    let session = Session::new(Scope::Ephemeral).expect("session");

    let en = pipeline
        .scan_safety_nets(&session, "alpha beta", &[LocaleTag::EnUs])
        .expect("en scan")
        .report;
    assert_eq!(en.suspects.len(), 1);
    assert_eq!(en.suspects[0].safety_net_id, "backend-a");
    assert_eq!(en.suspects[0].class, PiiClass::Name);

    let de = pipeline
        .scan_safety_nets(&session, "alpha beta", &[LocaleTag::DeDe])
        .expect("de scan")
        .report;
    assert_eq!(de.suspects.len(), 1);
    assert_eq!(de.suspects[0].safety_net_id, "backend-b");
    assert_eq!(de.suspects[0].class, PiiClass::Location);
}

struct CaptureModel(Arc<Mutex<Vec<String>>>);

impl LocaleAwareModel for CaptureModel {
    fn name(&self) -> &str {
        "capture-model"
    }

    fn native_locales(&self) -> &[LocaleTag] {
        &[LocaleTag::EnUs]
    }

    fn infer(&self, input: ModelInput, _hints: ModelHints) -> Result<Vec<ModelSpan>, ModelError> {
        self.0.lock().unwrap().push(input.text.clone());
        let marker = "Dr. Schmidt";
        let start = input.text.find(marker).expect("synthetic marker");
        Ok(vec![ModelSpan {
            text: marker.to_string(),
            byte_range: start..start + marker.len(),
            class: PiiClass::Name,
            confidence: Some(0.99),
            model_name: self.name().to_string(),
        }])
    }
}

#[test]
fn registry_scan_uses_stable_owned_tokens_and_real_clean_offsets() {
    let inputs = Arc::new(Mutex::new(Vec::new()));
    let registry =
        LocaleAwareModelRegistry::from_backends(vec![Box::new(CaptureModel(inputs.clone()))]);
    let pipeline = Pipeline::builder()
        .build()
        .unwrap()
        .with_safety_net_registry(registry);
    let mut clean_texts = Vec::new();
    let mut suspects = Vec::new();
    for _ in 0..2 {
        let session = Session::new(Scope::Ephemeral).unwrap();
        let token = session
            .tokenize(&PiiClass::Email, "alice@example.invalid")
            .unwrap();
        let clean_text = format!("🦊 {token} Dr. Schmidt");
        let report = pipeline
            .scan_safety_nets(&session, &clean_text, &[LocaleTag::EnUs])
            .unwrap()
            .report;
        let start = clean_text.find("Dr. Schmidt").unwrap();
        assert_eq!(report.suspects[0].span, start..start + "Dr. Schmidt".len());
        clean_texts.push(clean_text);
        suspects.push(report.suspects);
    }
    assert_ne!(clean_texts[0], clean_texts[1]);
    assert_eq!(suspects[0], suspects[1]);
    let captured = inputs.lock().unwrap();
    assert_eq!(captured.len(), 2);
    assert_eq!(captured[0], captured[1]);
    assert!(captured[0].contains("<00000000:Email_1>"));
}
