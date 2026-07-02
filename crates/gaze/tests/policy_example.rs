use std::path::Path;

use gaze::{Action, PiiClass, Policy, RuleSpec, SessionScope};

#[test]
fn reference_policy_example_loads_under_current_policy_loader() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let policy_path = manifest_dir
        .join("../..")
        .join("docs/reference/policy.example.toml");

    let policy = Policy::load(&policy_path).expect("docs/reference/policy.example.toml must parse");

    assert_eq!(policy.schema_version, "0.1.0");
    assert_eq!(policy.session.scope, SessionScope::Persistent);
    assert_eq!(policy.session.ttl_secs, Some(86400));
    assert_eq!(policy.rulepacks.bundled, ["core"]);
    assert_eq!(policy.detectors.len(), 2);
    assert!(policy.rules.iter().any(|rule| matches!(
        rule,
        RuleSpec::Class {
            class: PiiClass::Email,
            action: Action::Tokenize
        }
    )));
    assert!(policy.rules.iter().any(|rule| matches!(
        rule,
        RuleSpec::Default {
            action: Action::Preserve
        }
    )));
}
