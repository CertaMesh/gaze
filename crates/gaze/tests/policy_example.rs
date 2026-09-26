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
            action: Action::Tokenize
        }
    )));
}

/// docs/reference/policy.md "Minimal working example" shipped a policy that
/// failed to load from v0.14.0 on (its generic email regex shadowed Gaze's own
/// email-shaped tokens). Load the exact block so the page cannot rot again.
#[test]
fn policy_md_minimal_working_example_loads() {
    let page = include_str!("../../../docs/reference/policy.md");
    let section = page
        .split_once("## Minimal working example\n")
        .expect("policy.md has a Minimal working example section")
        .1;
    let body = section
        .split_once("```toml\n")
        .expect("minimal example opens a toml fence")
        .1
        .split_once("\n```")
        .expect("minimal example closes its toml fence")
        .0;

    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("minimal.toml");
    std::fs::write(&path, body).expect("write minimal.toml");

    let policy = Policy::load_for_cli(&path).expect("policy.md minimal example must load");
    assert_eq!(policy.session.scope, SessionScope::Persistent);
    // The page says the omitted `[policy.rulepacks]` falls back to bundled `core`.
    assert_eq!(policy.rulepacks.bundled, ["core"]);
    assert!(policy.rules.iter().any(|rule| matches!(
        rule,
        RuleSpec::Class {
            class: PiiClass::Email,
            action: Action::Tokenize
        }
    )));
}
