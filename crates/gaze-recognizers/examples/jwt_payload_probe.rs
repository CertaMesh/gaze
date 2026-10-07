//! Inactive model-free supplemental probe. Run only in an assigned native window.
//! Input has no gold spans. Output is native evidence, never PII gate credit.

use std::collections::HashSet;
use std::io::{self, BufRead, Write};
use std::path::Path;

use gaze::{
    CleanDocument, Context, DictionaryBundle, LocaleChain, LocaleTag, Policy, Rulepack,
    RulepackSource, SafetyNetFallback, SafetyNetMode, SafetyNetPolicy, Scope, SensitiveSnapshot,
    Session,
};
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

const POLICY: &[u8] = include_bytes!("../../../scripts/bench/fixtures/jwt_payload/policy.toml");
const PROTOCOL: &str = "jwt-payload-supplement-v1";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    id: String,
    text: String,
}

fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 2 {
        return Err("expected exactly one pinned opt-in policy path".into());
    }
    let commit = option_env!("GAZE_JWT_BUILD_COMMIT").ok_or("missing build commit pin")?;
    if commit.len() != 40
        || !commit
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    {
        return Err("invalid build commit pin".into());
    }
    let bytes = std::fs::read(&args[1])?;
    if bytes != POLICY {
        return Err("opt-in policy bytes differ from compiled policy pin".into());
    }
    let policy = Policy::load(Path::new(&args[1]))?;
    let context = Context {
        dictionaries: Default::default(),
        class_map: Default::default(),
        fields: Default::default(),
        record_match_kinds: Default::default(),
        record_value_rejections: Default::default(),
    };
    let packs = ["core", "secrets"]
        .map(|name| {
            Rulepack::load(RulepackSource::Embedded(
                gaze_recognizers::embedded(name).expect("bundle"),
            ))
        })
        .into_iter()
        .collect::<Result<Vec<_>, _>>()?;
    let chain =
        LocaleChain::merge_cli_policy_rulepack_default(None, None, Some(&[LocaleTag::Global]));
    let pipeline = gaze_assembly::build_pipeline(&policy, &context, &packs, &chain, None)?;
    let mut out = io::BufWriter::new(io::stdout().lock());
    serde_json::to_writer(
        &mut out,
        &json!({
            "protocol": PROTOCOL, "build_commit": commit,
            "policy_sha256": hash(&bytes), "model_free": true,
        }),
    )?;
    out.write_all(b"\n")?;
    out.flush()?;
    let mut seen = HashSet::new();
    let mut executed = 0usize;
    for line in io::stdin().lock().lines() {
        let request: Request = serde_json::from_str(&line?)?;
        if request.id.is_empty() || request.text.is_empty() || !seen.insert(request.id.clone()) {
            return Err("empty or duplicate request".into());
        }
        let session = Session::new(Scope::Conversation(request.id.clone()))?;
        let cleaned = pipeline
            .clean_text_with_safety_net_policy_detect_context_and_protection_trace(
                &session,
                &request.text,
                &[LocaleTag::Global],
                &DictionaryBundle::default(),
                SafetyNetPolicy::new(SafetyNetMode::Strict, SafetyNetFallback::Strict),
            );
        let record = match cleaned {
            Err(error) => json!({
                "id": request.id, "input_sha256": hash(request.text.as_bytes()),
                "policy_sha256": hash(&bytes), "refused": true, "error": error.to_string(),
                "clean_text": null, "restored": null, "imported_restored": null,
                "manifest": [], "trace": [],
            }),
            Ok((doc, spans, _, trace)) => {
                let CleanDocument::Text(clean) = doc else {
                    return Err("nontext result".into());
                };
                let manifest = spans
                    .iter()
                    .map(|span| {
                        let token = clean
                            .get(span.clean_span.clone())
                            .ok_or("invalid clean span")?;
                        Ok(json!({
                            "raw_span": [span.raw_span.start, span.raw_span.end],
                            "clean_span": [span.clean_span.start, span.clean_span.end],
                            "class": span.class.to_canonical_str(),
                            "token_restore": session.restore(token),
                        }))
                    })
                    .collect::<Result<Vec<Value>, &str>>()?;
                let trace = trace
                    .iter()
                    .map(|item| {
                        json!({
                            "raw_span": [item.raw_start(), item.raw_end()],
                            "class": item.class().to_canonical_str(), "sources": item.source_ids(),
                        })
                    })
                    .collect::<Vec<_>>();
                let restored = pipeline.restore_strict_text(&session, &clean).ok();
                let snapshot = session.export()?.into_bytes();
                let imported = Session::import(SensitiveSnapshot::from(snapshot))?;
                let imported_restored = pipeline.restore_strict_text(&imported, &clean).ok();
                json!({
                    "id": request.id, "input_sha256": hash(request.text.as_bytes()),
                    "policy_sha256": hash(&bytes), "refused": false, "error": null,
                    "clean_text": clean, "restored": restored, "imported_restored": imported_restored,
                    "manifest": manifest, "trace": trace,
                })
            }
        };
        serde_json::to_writer(&mut out, &record)?;
        out.write_all(b"\n")?;
        out.flush()?;
        executed += 1;
    }
    if executed == 0 {
        return Err("zero executed requests".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pinned_policy_loads_as_model_free_credential_only_opt_in() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../scripts/bench/fixtures/jwt_payload/policy.toml");
        let loaded = Policy::load(&path);
        assert!(loaded.is_ok(), "pinned policy must load: {loaded:?}");
        let policy = loaded.unwrap();
        assert_eq!(policy.rulepacks.bundled, ["core", "secrets"]);
        assert!(!policy.rulepacks.auto_activate_locale_gated);
        assert!(policy.detectors.is_empty());
        assert!(policy.ner.is_none());
        assert!(policy.dob_judge.is_none());
        assert_eq!(
            policy.safety_net.backend,
            gaze::SafetyNetPolicyBackend::None
        );
        assert_eq!(policy.rules.len(), 2);
        assert!(
            matches!(&policy.rules[0], gaze::RuleSpec::Class { class, action }
            if class.to_canonical_str() == "custom:security_token" && *action == gaze::Action::Tokenize)
        );
        assert!(
            matches!(&policy.rules[1], gaze::RuleSpec::Default { action }
            if *action == gaze::Action::Preserve)
        );
    }
}
