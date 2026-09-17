//! Library-level proof that the `secrets` bundle is opt-in (todo #3646).
//!
//! `CorePipelineConfig::new()` is the default library adopter: it loads `core` and
//! tokenizes every class the loaded rulepacks declare. Re-adding `password.field` or
//! `security_token.anchored` to `embedded/core.toml` turns the default case RED.

use gaze::{CleanDocument, LocaleTag, RawDocument, Scope, Session};
use gaze_assembly::CorePipelineConfig;

const AWS_KEY: &str = "AKIAIOSFODNN7EXAMPLE";
const PASSWORD: &str = "synthetic-credential-value";

fn input() -> String {
    format!("rotate {AWS_KEY} today\npassword: {PASSWORD}\n")
}

fn clean(config: CorePipelineConfig) -> String {
    let core = config.build().expect("pipeline");
    let session = Session::new(Scope::Ephemeral).expect("session");
    let (clean, _, _) = core
        .pipeline()
        .clean_with_safety_net(&session, RawDocument::Text(input()), &[LocaleTag::Global])
        .expect("clean");
    let CleanDocument::Text(clean) = clean else {
        panic!("expected text")
    };
    clean
}

#[test]
fn default_core_pipeline_emits_no_credential_tokens() {
    assert_eq!(clean(CorePipelineConfig::new()), input());
}

#[test]
fn secrets_opt_in_tokenizes_both_credentials() {
    let cleaned = clean(CorePipelineConfig::new().with_bundled_rulepack("secrets"));
    assert!(!cleaned.contains(AWS_KEY), "{cleaned:?}");
    assert!(!cleaned.contains(PASSWORD), "{cleaned:?}");
    assert!(cleaned.contains(":security_token_"), "{cleaned:?}");
    assert!(cleaned.contains(":password_"), "{cleaned:?}");
}
