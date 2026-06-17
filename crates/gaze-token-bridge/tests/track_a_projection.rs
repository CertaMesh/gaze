use gaze::PiiClass;
use gaze_token_bridge::registry::IndexDomainRegistry;
use gaze_token_bridge::traits::{DomainProjector, KeyManager};
use gaze_token_bridge::{AuthGrant, BridgeError, CanonicalEntity, IndexedEntityRef, PolicyOutcome};

const POLICY_JSON: &str = include_str!("../fixtures/policy.json");
const CUSTOMER_DOMAIN: &str = "tenant_demo/customer_docs/v1";
const LEGAL_DOMAIN: &str = "tenant_demo/legal_docs/v1";

fn registry() -> IndexDomainRegistry {
    IndexDomainRegistry::from_json(POLICY_JSON).expect("fixture policy loads")
}

#[test]
fn projection_is_deterministic_and_domain_key_scoped() {
    let registry = registry();
    let projector = gaze_token_bridge::projection::HmacDomainProjector::new(&registry);
    let entity = CanonicalEntity::from_raw(PiiClass::Email, "alice@example.invalid");
    let customer_domain = registry.domain(CUSTOMER_DOMAIN).unwrap();
    let legal_domain = registry.domain(LEGAL_DOMAIN).unwrap();

    let first = projector.project(customer_domain, &entity).unwrap();
    let second = projector.project(customer_domain, &entity).unwrap();
    let legal = projector.project(legal_domain, &entity).unwrap();

    let mut old_key_customer_domain = customer_domain.clone();
    old_key_customer_domain.projection_key_id = "customer-v1".to_string();
    let old_key = projector
        .project(&old_key_customer_domain, &entity)
        .unwrap();

    assert_eq!(first, second);
    assert_eq!(first.domain_id, CUSTOMER_DOMAIN);
    assert_eq!(first.key_id, "customer-v2");
    assert_ne!(first.fingerprint_hex, legal.fingerprint_hex);
    assert_ne!(first.fingerprint_hex, old_key.fingerprint_hex);
    assert_eq!(old_key.key_id, "customer-v1");
}

#[test]
fn key_rotation_retains_old_key_material_for_reads() {
    let registry = registry();
    let active_domain = registry.domain(CUSTOMER_DOMAIN).unwrap();

    assert_eq!(active_domain.projection_key_id, "customer-v2");
    assert!(KeyManager::key(&registry, "customer-v1").is_some());
    assert!(KeyManager::key(&registry, "customer-v2").is_some());
    assert!(registry
        .domains()
        .all(|domain| domain.projection_key_id != "customer-v1"));
}

#[test]
fn missing_projection_key_fails_closed() {
    let registry = registry();
    let projector = gaze_token_bridge::projection::HmacDomainProjector::new(&registry);
    let mut domain = registry.domain(CUSTOMER_DOMAIN).unwrap().clone();
    domain.projection_key_id = "missing-key".to_string();
    let entity = CanonicalEntity::from_raw(PiiClass::Name, "Dr. Schmidt");

    let err = projector.project(&domain, &entity).unwrap_err();

    assert!(
        matches!(err, BridgeError::Policy(message) if message.contains("missing projection key"))
    );
    assert!(KeyManager::key(&registry, "missing-key").is_none());
}

#[test]
fn unknown_pii_class_in_policy_fails_closed() {
    let bad_policy = POLICY_JSON.replace("\"name\"", "\"not_a_real_class\"");

    let err = IndexDomainRegistry::from_json(&bad_policy).unwrap_err();

    assert!(matches!(err, BridgeError::Policy(message) if message.contains("unknown pii class")));
}

#[test]
fn registry_exposes_domain_rules_and_keys() {
    let registry = registry();

    assert!(registry.domain(CUSTOMER_DOMAIN).is_some());
    assert_eq!(registry.rules().len(), 2);
    assert_eq!(registry.rules_for_domain(CUSTOMER_DOMAIN).count(), 1);
    assert!(registry.projection_key("legal-v1").is_some());
    assert_eq!(registry.projection_keys().len(), 3);
}

#[test]
fn r1_policy_outcome_grant_is_constructible() {
    let entity_ref = IndexedEntityRef {
        domain_id: CUSTOMER_DOMAIN.to_string(),
        key_id: "customer-v2".to_string(),
        entity_class: PiiClass::Email,
        fingerprint_hex: "ab".repeat(32),
    };

    let outcome = PolicyOutcome::Grant(AuthGrant {
        entity_ref: entity_ref.clone(),
        entity_class: PiiClass::Email,
        effective_purpose: "support_lookup".to_string(),
        max_results: 20,
        allowed_filters: vec!["customer_status".to_string()],
        elevated_audit: false,
        raw_sha256: "cd".repeat(32),
    });

    match outcome {
        PolicyOutcome::Grant(grant) => {
            assert_eq!(grant.entity_ref, entity_ref);
            assert_eq!(grant.effective_purpose, "support_lookup");
            assert_eq!(grant.max_results, 20);
            assert!(!grant.elevated_audit);
        }
        PolicyOutcome::Deny { .. } => panic!("expected grant"),
    }
}
