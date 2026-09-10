use std::{collections::HashMap, sync::Arc};

use gaze::registry::{
    Canonicalizer as RegistryCanonicalizer, ValidationResult as RegistryValidationResult,
    Validator as RegistryValidator,
};
use gaze::{Canonicalizer, RecognizerRegistryBuilder, ValidationResult, Validator};

struct Extension;

impl Validator for Extension {
    fn id(&self) -> &str {
        "extension"
    }

    fn validate(&self, _raw: &str) -> ValidationResult {
        ValidationResult::Indeterminate
    }
}

impl Canonicalizer for Extension {
    fn canonicalize(&self, raw: &str) -> Option<String> {
        Some(raw.to_owned())
    }
}

#[test]
fn public_extension_paths_and_accessors_remain_compatible() {
    let validator: &dyn RegistryValidator = &Extension;
    let canonicalizer: &dyn RegistryCanonicalizer = &Extension;
    assert_eq!(validator.id(), "extension");
    assert_eq!(
        validator.validate("sample"),
        RegistryValidationResult::Indeterminate
    );
    assert_eq!(canonicalizer.canonicalize("sample"), Some("sample".into()));
    assert_eq!(ValidationResult::Valid, RegistryValidationResult::Valid);
    assert_eq!(ValidationResult::Invalid, RegistryValidationResult::Invalid);

    let registry = RecognizerRegistryBuilder::default().build();
    let validators: &HashMap<String, Arc<dyn Validator>> = registry.validators();
    let canonicalizers: &HashMap<String, Arc<dyn Canonicalizer>> = registry.canonicalizers();
    assert!(validators.is_empty());
    assert!(canonicalizers.is_empty());
}
