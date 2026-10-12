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

#[test]
#[should_panic(expected = "invalid recognizer registration")]
fn compatibility_build_rejects_orphan_membership() {
    RecognizerRegistryBuilder::default()
        .register_collision(
            "missing",
            gaze::CollisionMembership::new("class_alpha", "alpha", 0, None),
        )
        .build();
}

struct NamedRecognizer {
    id: &'static str,
    source: &'static str,
}

impl gaze::Recognizer for NamedRecognizer {
    fn id(&self) -> &str {
        self.id
    }
    fn supported_class(&self) -> &gaze::PiiClass {
        &gaze::PiiClass::Name
    }
    fn token_family(&self) -> &str {
        "counter"
    }
    fn detect(
        &self,
        input: &str,
        _ctx: &gaze::DetectContext<'_>,
    ) -> Result<Vec<gaze::Candidate>, gaze::DetectError> {
        Ok(vec![gaze::Candidate::new(
            0..input.len(),
            gaze::PiiClass::Name,
            self.id,
            1.0,
            0,
            None,
            "counter",
            self.source,
            gaze::ConflictTier::None,
            Vec::new(),
        )])
    }
}

fn duplicate_batch() -> RecognizerRegistryBuilder {
    RecognizerRegistryBuilder::default()
        .register(NamedRecognizer {
            id: "repeated",
            source: "first",
        })
        .register_arc(Arc::new(NamedRecognizer {
            id: "repeated",
            source: "second",
        }))
}

#[test]
fn fallible_build_rejects_duplicate_scan_and_lookup_identity() {
    assert!(
        duplicate_batch().try_build().is_err(),
        "ambiguous batch must not return a registry"
    );
}

#[test]
#[should_panic(expected = "invalid recognizer registration")]
fn compatibility_build_rejects_duplicate_scan_and_lookup_identity() {
    duplicate_batch().build();
}

#[test]
fn pipeline_construction_returns_registration_errors() {
    assert!(gaze::Pipeline::builder()
        .register_collision(
            "missing",
            gaze::CollisionMembership::new("class_alpha", "alpha", 0, None)
        )
        .build()
        .is_err());
}

struct LegacyDetector(&'static str);

impl gaze::Detector for LegacyDetector {
    fn detect(&self, input: &str) -> Vec<gaze::Detection> {
        vec![gaze::Detection::new(
            0..input.len(),
            gaze::PiiClass::Name,
            self.0,
        )]
    }
}

#[test]
fn repeated_legacy_adapters_have_stable_distinct_ids_and_keep_provenance() {
    for _ in 0..2 {
        let pipeline = gaze::Pipeline::builder()
            .detector(LegacyDetector("first-source"))
            .recognizer(NamedRecognizer {
                id: "named",
                source: "named-source",
            })
            .detector(LegacyDetector("second-source"))
            .build()
            .unwrap();
        let ids = pipeline
            .registry()
            .recognizer_ids()
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(
            ids,
            ["legacy-detector", "legacy-detector/1", "named"]
                .into_iter()
                .collect()
        );
        let dictionaries = gaze::DictionaryBundle::default();
        let candidates = pipeline
            .registry()
            .detect_all("sample", &gaze::DetectContext::new(&[], &dictionaries))
            .unwrap();
        assert_eq!(
            candidates
                .iter()
                .map(|c| c.source.as_str())
                .collect::<Vec<_>>(),
            ["first-source", "named-source", "second-source"]
        );
        assert_eq!(candidates[0].recognizer_id, "first-source");
        assert_eq!(candidates[2].recognizer_id, "second-source");
    }
}

#[test]
fn atomic_registration_and_forward_setters_preserve_order_and_declarations() {
    for fallible in [false, true] {
        let first = gaze::CollisionMembership::new("class_alpha", "alpha", 10, Some("cue".into()));
        let second = gaze::CollisionMembership::new("class_alpha", "beta", 20, None);
        let builder = RecognizerRegistryBuilder::default()
            .register_collision("first", first.clone())
            .register(NamedRecognizer {
                id: "first",
                source: "first-source",
            })
            .register_arc_with_membership(
                Arc::new(NamedRecognizer {
                    id: "second",
                    source: "second-source",
                }),
                Some(second.clone()),
            );
        let registry = if fallible {
            builder.try_build().unwrap()
        } else {
            builder.build()
        };
        assert_eq!(
            registry.recognizer_ids().collect::<Vec<_>>(),
            ["first", "second"]
        );
        assert_eq!(registry.family_policy().membership("first"), Some(&first));
        assert_eq!(registry.family_policy().membership("second"), Some(&second));
        assert_eq!(
            registry.family_policy().compare("first", "second"),
            Some(true)
        );
        let dictionaries = gaze::DictionaryBundle::default();
        let ctx = gaze::DetectContext::new(&[], &dictionaries);
        let scanned = registry.detect_all("sample", &ctx).unwrap();
        for candidate in scanned {
            let looked_up = registry
                .recognizer(&candidate.recognizer_id)
                .unwrap()
                .detect("sample", &ctx)
                .unwrap();
            assert_eq!(looked_up[0].source, candidate.source);
        }
    }
}

#[test]
fn invalid_batches_return_typed_errors_and_never_a_partial_registry() {
    let orphan = || {
        RecognizerRegistryBuilder::default()
            .register(NamedRecognizer {
                id: "admitted",
                source: "source",
            })
            .register_collision(
                "missing",
                gaze::CollisionMembership::new("class_alpha", "alpha", 0, None),
            )
    };
    assert_eq!(
        orphan().try_build().unwrap_err(),
        gaze::RegistryBuildError::OrphanCollisionMembership {
            recognizer_id: "missing".into()
        }
    );
    assert_eq!(
        duplicate_batch().try_build().unwrap_err(),
        gaze::RegistryBuildError::DuplicateRecognizerId {
            recognizer_id: "repeated".into()
        }
    );
    for builder in [orphan(), duplicate_batch()] {
        let panic =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| builder.build())).unwrap_err();
        assert_eq!(
            panic.downcast_ref::<&str>(),
            Some(&"invalid recognizer registration")
        );
    }
}

#[test]
fn compatibility_setters_use_the_last_declaration_without_admitting_orphans() {
    let old = gaze::CollisionMembership::new("class_alpha", "alpha", 10, None);
    let last = gaze::CollisionMembership::new("class_alpha", "beta", 20, None);
    let registry = RecognizerRegistryBuilder::default()
        .register_collision("named", old.clone())
        .register_collision("named", last.clone())
        .register(NamedRecognizer {
            id: "named",
            source: "source",
        })
        .try_build()
        .unwrap();
    assert_eq!(registry.family_policy().membership("named"), Some(&last));
    let registry = RecognizerRegistryBuilder::default()
        .register_collision("named", old.clone())
        .register_with_membership(
            NamedRecognizer {
                id: "named",
                source: "source",
            },
            Some(last.clone()),
        )
        .register_collision("named", old.clone())
        .build();
    assert_eq!(registry.family_policy().membership("named"), Some(&old));
}

#[test]
fn registration_error_display_never_includes_adopter_identifiers() {
    let error = RecognizerRegistryBuilder::default()
        .register_collision(
            "alice@example.invalid",
            gaze::CollisionMembership::new("class_alpha", "alpha", 0, None),
        )
        .try_build()
        .unwrap_err();
    assert_eq!(error.to_string(), "orphan collision membership");
}

#[test]
fn atomic_duplicate_and_explicit_legacy_id_collisions_are_rejected() {
    let error = RecognizerRegistryBuilder::default()
        .register_with_membership(
            NamedRecognizer {
                id: "named",
                source: "source",
            },
            Some(gaze::CollisionMembership::new(
                "class_alpha",
                "alpha",
                10,
                None,
            )),
        )
        .register(NamedRecognizer {
            id: "named",
            source: "other",
        })
        .try_build()
        .unwrap_err();
    assert_eq!(
        error,
        gaze::RegistryBuildError::DuplicateRecognizerId {
            recognizer_id: "named".into()
        }
    );
    assert!(matches!(
        gaze::Pipeline::builder()
            .detector(LegacyDetector("source"))
            .recognizer(NamedRecognizer {
                id: "legacy-detector",
                source: "other"
            })
            .build(),
        Err(gaze::Error::RegistryBuild(
            gaze::RegistryBuildError::DuplicateRecognizerId { .. }
        ))
    ));
}
