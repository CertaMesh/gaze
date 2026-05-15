#![cfg(feature = "bundled-recognizers")]

use gaze::{LocaleTag, PiiClass, Pipeline, Scope, Session};
use gaze_recognizers::{
    LocaleAwareModel, LocaleAwareModelRegistry, ModelError, ModelHints, ModelInput, ModelSpan,
};

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
