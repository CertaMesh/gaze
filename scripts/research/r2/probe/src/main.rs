//! Research-only JSONL probe. No clocks, corpus loading or runtime changes.
use gaze::{CleanDocument, Context, Policy, SafetyNetPolicy, Scope, Session};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    io::{self, BufRead},
    num::NonZeroUsize,
    path::Path,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args().nth(1).ok_or("policy path required")?;
    let policy = Policy::load_for_cli(Path::new(&path))?;
    let inputs = gaze_assembly::resolve_policy_inputs(&policy, None, None, None)?;
    let context = Context {
        dictionaries: HashMap::new(),
        class_map: HashMap::new(),
        fields: serde_json::Map::new(),
        record_match_kinds: HashMap::new(),
        record_value_rejections: Vec::new(),
    };
    let mut pipeline = gaze_assembly::build_pipeline_builder(
        &policy,
        &context,
        &inputs.rulepacks,
        &inputs.locale_chain,
        Some(inputs.ner_threshold),
    )?
    .build()?;
    if policy.safety_net.backend == gaze::SafetyNetPolicyBackend::Nym {
        pipeline = gaze_assembly::attach_nym_safety_net(
            pipeline,
            &policy,
            None,
            None,
            NonZeroUsize::new(1),
        )?;
    }
    let mut sessions: HashMap<String, Session> = HashMap::new();
    for line in io::stdin().lock().lines() {
        let request: Value = serde_json::from_str(&line?)?;
        let sid = request["session_id"]
            .as_str()
            .ok_or("session_id required")?;
        let text = request["text"].as_str().ok_or("text required")?;
        if !sessions.contains_key(sid) {
            sessions.insert(sid.to_string(), Session::new(Scope::Ephemeral)?);
        }
        let session = &sessions[sid];
        match pipeline.clean_text_with_safety_net_policy_detect_context_and_protection_evidence(
            session,
            text,
            inputs.locale_chain.as_slice(),
            &inputs.dictionaries,
            SafetyNetPolicy::default(),
        ) {
            Ok((CleanDocument::Text(clean), manifest, _report, _trace, _events)) => {
                let (restored, _) = pipeline.restore_with_telemetry(session, &clean)?;
                let tokens: Vec<Value> = manifest
                    .iter()
                    .map(|entry| {
                        let token = &clean[entry.clean_span.clone()];
                        json!({"token":token, "original":session.restore(token)})
                    })
                    .collect();
                println!(
                    "{}",
                    json!({"session_id":sid, "clean_text":clean,
                    "manifest":manifest, "tokens":tokens, "exact_restore":restored.text == text})
                );
            }
            Ok(_) => return Err("unexpected structured output".into()),
            Err(error) => println!("{}", json!({"session_id":sid, "error":error.to_string()})),
        }
    }
    Ok(())
}
