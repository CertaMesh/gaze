use gaze_recognizers::{
    LocaleAwareModel, LocaleAwareModelRegistry, ModelError, ModelHints, ModelInput, ModelSpan,
    ModelStage,
};
use gaze_types::LocaleTag;

#[derive(Debug)]
struct TestModel {
    name: &'static str,
    locales: Vec<LocaleTag>,
}

impl TestModel {
    fn new(name: &'static str, locales: Vec<LocaleTag>) -> Self {
        Self { name, locales }
    }
}

impl LocaleAwareModel for TestModel {
    fn name(&self) -> &str {
        self.name
    }

    fn native_locales(&self) -> &[LocaleTag] {
        &self.locales
    }

    fn infer(&self, _input: ModelInput, _hints: ModelHints) -> Result<Vec<ModelSpan>, ModelError> {
        Ok(Vec::new())
    }
}

#[test]
fn resolves_exact_locale_before_parent_and_global() {
    let registry = LocaleAwareModelRegistry::from_backends(vec![
        Box::new(TestModel::new("global", vec![LocaleTag::Global])),
        Box::new(TestModel::new(
            "de-parent",
            vec![LocaleTag::Other("de".to_string())],
        )),
        Box::new(TestModel::new("de-de", vec![LocaleTag::DeDe])),
    ]);

    let models = registry
        .resolve(&LocaleTag::DeDe, ModelStage::Pass3SafetyNet)
        .unwrap();

    assert_eq!(models.len(), 1);
    assert_eq!(models[0].name(), "de-de");
}

#[test]
fn resolves_parent_language_when_exact_is_absent() {
    let registry = LocaleAwareModelRegistry::from_backends(vec![
        Box::new(TestModel::new("global", vec![LocaleTag::Global])),
        Box::new(TestModel::new(
            "de-parent",
            vec![LocaleTag::Other("de".to_string())],
        )),
    ]);

    let models = registry
        .resolve(&LocaleTag::DeAt, ModelStage::Pass2Ner)
        .unwrap();

    assert_eq!(models.len(), 1);
    assert_eq!(models[0].name(), "de-parent");
}

#[test]
fn resolves_multilingual_global_when_locale_specific_models_are_absent() {
    let registry = LocaleAwareModelRegistry::from_backends(vec![Box::new(TestModel::new(
        "global",
        vec![LocaleTag::Global],
    ))]);

    let models = registry
        .resolve(&LocaleTag::EnGb, ModelStage::Pass3SafetyNet)
        .unwrap();

    assert_eq!(models.len(), 1);
    assert_eq!(models[0].name(), "global");
}

#[test]
fn fails_closed_when_no_locale_model_covers_request() {
    let registry = LocaleAwareModelRegistry::new();

    let error = match registry.resolve(&LocaleTag::EnUs, ModelStage::Pass2Ner) {
        Ok(models) => panic!("expected fail-closed error, got {} models", models.len()),
        Err(error) => error,
    };

    assert_eq!(
        error,
        ModelError::NoLocaleModelCoverage {
            locale: LocaleTag::EnUs,
            stage: ModelStage::Pass2Ner,
        }
    );
}
