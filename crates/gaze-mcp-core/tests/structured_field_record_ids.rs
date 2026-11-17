//! Capture arguments at the MCP tool boundary, respecting numeric carrier refusal.
#[allow(dead_code)]
#[path = "../../gaze-proxy/tests/support/structured_record_ids.rs"]
mod record_ids;

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use gaze::{
    Action, CleanDocument, Context, LocaleChain, Policy, RawDocument, RuleSpec, Rulepack, Scope,
    Session,
};
use gaze_mcp_core::*;
use serde_json::{json, Value};

struct Auth;
#[async_trait]
impl AuthHook for Auth {
    async fn authorize_agent(&self, _: &Principal, _: &str) -> Result<(), AuthError> {
        Ok(())
    }
    async fn authorize_operator(&self, _: &Principal, _: &str) -> Result<(), AuthError> {
        Ok(())
    }
}
struct Store;
#[async_trait]
impl ManifestStore for Store {
    async fn begin_call(&self, ctx: BeginCallContext<'_>) -> Result<CallHandle, ManifestError> {
        Ok(CallHandle::new(ctx.call_id))
    }
    async fn finish_call(&self, _: CallHandle, _: SnapshotRef) -> Result<(), ManifestError> {
        Ok(())
    }
    async fn fail_call(&self, _: CallHandle, _: FailureReason) -> Result<(), ManifestError> {
        Ok(())
    }
}
struct Capture {
    descriptor: ToolDescriptor,
    captured: Arc<Mutex<Vec<Value>>>,
}
#[async_trait]
impl Tool for Capture {
    fn descriptor(&self) -> &ToolDescriptor {
        &self.descriptor
    }
    async fn invoke(&self, ctx: &ToolCtx<'_>) -> Result<ToolResponse, ToolError> {
        self.captured
            .lock()
            .unwrap()
            .push(ctx.redacted_args().clone());
        Ok(ToolResponse::json(Value::Null))
    }
}

fn declare_members(value: &Value, path: &mut CarrierPath, members: &mut Vec<CarrierPath>) {
    match value {
        Value::Object(object) => {
            for (key, child) in object {
                path.push(CarrierSegment::Member(key.clone()));
                members.push(path.clone());
                declare_members(child, path, members);
                path.pop();
            }
        }
        Value::Array(items) => {
            for child in items {
                path.push(CarrierSegment::AnyIndex);
                declare_members(child, path, members);
                path.pop();
            }
        }
        _ => {}
    }
}

#[tokio::test]
async fn mcp_labelled_string_leaves_protect_and_numbers_fail_closed() {
    let mut policy = Policy::default();
    policy.rules = vec![RuleSpec::Default {
        action: Action::Tokenize,
    }];
    let chain = LocaleChain::merge_policy_and_cli(None, None);
    let pipeline = gaze_assembly::build_pipeline(
        &policy,
        &Context {
            dictionaries: Default::default(),
            class_map: Default::default(),
            fields: Default::default(),
            record_match_kinds: Default::default(),
            record_value_rejections: Default::default(),
        },
        // The bundled `core` pack binds labelled fields; the adopter rule calibrates prose.
        &[
            Rulepack::load(gaze::RulepackSource::Embedded(
                gaze_recognizers::embedded("core").unwrap(),
            ))
            .unwrap(),
            Rulepack::parse(record_ids::RULEPACK).unwrap(),
        ],
        &chain,
        None,
    )
    .unwrap();
    for case in record_ids::cases() {
        let document = case.document();
        for (kind, text) in [
            ("prose", case.prose()),
            ("whole-json", document.to_string()),
        ] {
            let session = Session::new(Scope::Ephemeral).unwrap();
            let result = pipeline.pseudonymize_with_context(
                &session,
                RawDocument::Text(text),
                chain.as_slice(),
            );
            if kind == "whole-json" && case.numeric {
                // A labelled JSON number has no reversible text rendering: typed refusal.
                assert!(
                    matches!(
                        result,
                        Err(gaze::Error::Protection(
                            gaze::ProtectionError::Untransformable { .. }
                        ))
                    ),
                    "{case:?}: whole-json labelled number must refuse"
                );
                continue;
            }
            let CleanDocument::Text(clean) = result.unwrap() else {
                panic!("text expected")
            };
            record_ids::check(&case, &format!("MCP/calibration/{kind}"), &clean, false);
        }
        let mut members = vec![];
        declare_members(&document, &mut vec![], &mut members);
        let captured = Arc::new(Mutex::new(vec![]));
        let mut registry = ToolRegistry::new();
        registry
            .register(Capture {
                descriptor: ToolDescriptor::agent("lookup", json!({})).with_carriers(
                    CarrierDeclaration::new(members, vec![]),
                    CarrierDeclaration::default(),
                ),
                captured: captured.clone(),
            })
            .unwrap();
        let session = Session::new(Scope::Ephemeral).unwrap();
        let session_policy = SessionIdPolicy::default_strict();
        let envelope = PiiEnvelope::new(
            &registry,
            &Auth,
            &Store,
            &pipeline,
            &session,
            chain.as_slice(),
            &session_policy,
        );
        let result = envelope
            .dispatch(
                &Principal::new("agent"),
                "lookup",
                document.clone(),
                None,
                gaze_mcp_core::HostContextSelection::Absent,
            )
            .await;
        if case.numeric {
            assert!(
                matches!(
                    result,
                    Err(DispatchError::Carrier(CarrierError::UndeclaredNumber))
                ),
                "{case:?}: expected Carrier(UndeclaredNumber)"
            );
            assert!(captured.lock().unwrap().is_empty());
            println!("RECORD_ID MCP/args {case:?} N/A fail-closed UndeclaredNumber");
        } else {
            result.unwrap_or_else(|_| panic!("MCP/args {case:?}: expected successful dispatch"));
            let sent = captured.lock().unwrap().pop().expect("tool must execute");
            record_ids::check(&case, "MCP/args", &sent.to_string(), false);
        }
    }
}

fn stock_pipeline() -> (gaze::Pipeline, LocaleChain) {
    let mut policy = Policy::default();
    policy.rules = vec![RuleSpec::Default {
        action: Action::Tokenize,
    }];
    let chain = LocaleChain::merge_policy_and_cli(None, None);
    let pipeline = gaze_assembly::build_pipeline(
        &policy,
        &Context {
            dictionaries: Default::default(),
            class_map: Default::default(),
            fields: Default::default(),
            record_match_kinds: Default::default(),
            record_value_rejections: Default::default(),
        },
        // The shipped `core` pack alone: no adopter calibration rule.
        &[Rulepack::load(gaze::RulepackSource::Embedded(
            gaze_recognizers::embedded("core").unwrap(),
        ))
        .unwrap()],
        &chain,
        None,
    )
    .unwrap();
    (pipeline, chain)
}

async fn dispatch_capture(
    pipeline: &gaze::Pipeline,
    chain: &LocaleChain,
    session: &Session,
    args: Value,
) -> Result<Value, DispatchError> {
    let mut members = vec![];
    declare_members(&args, &mut vec![], &mut members);
    let captured = Arc::new(Mutex::new(vec![]));
    let mut registry = ToolRegistry::new();
    registry
        .register(Capture {
            descriptor: ToolDescriptor::agent("lookup", json!({})).with_carriers(
                CarrierDeclaration::new(members, vec![]),
                CarrierDeclaration::default(),
            ),
            captured: captured.clone(),
        })
        .unwrap();
    let session_policy = SessionIdPolicy::default_strict();
    let envelope = PiiEnvelope::new(
        &registry,
        &Auth,
        &Store,
        pipeline,
        session,
        chain.as_slice(),
        &session_policy,
    );
    envelope
        .dispatch(
            &Principal::new("agent"),
            "lookup",
            args,
            None,
            gaze_mcp_core::HostContextSelection::Absent,
        )
        .await?;
    let sent = captured.lock().unwrap().pop().expect("tool must execute");
    Ok(sent)
}

/// MCP leaf path with the stock `core` activation: a session-owned token inside a labelled
/// argument never exposes the rest of the identifier, and a string argument whose whole text
/// is a JSON document binds its own labelled fields.
#[tokio::test]
async fn mcp_owned_tokens_and_json_text_arguments_bind_with_stock_core() {
    let (pipeline, chain) = stock_pipeline();
    let session = Session::new(Scope::Conversation("synthetic-mcp-json".into())).unwrap();
    let owned = session
        .tokenize(&gaze::PiiClass::Email, "synthetic@example.invalid")
        .unwrap();
    let sent = dispatch_capture(
        &pipeline,
        &chain,
        &session,
        json!({"customer_id": format!("{owned} C-731284")}),
    )
    .await
    .unwrap_or_else(|error| panic!("owned-token leaf: {error:?}"));
    let leaf = sent["customer_id"].as_str().unwrap();
    assert!(!leaf.contains("73128"), "{leaf}");
    assert!(
        leaf.contains(&owned),
        "owned token keeps its authority: {leaf}"
    );

    // Fresh sessions: no earlier bound value may protect these through the repeat sweep.
    let sent = dispatch_capture(
        &pipeline,
        &chain,
        &Session::new(Scope::Ephemeral).unwrap(),
        json!({"payload": r#"{"customer_id":"C-731299","note":"ok"}"#}),
    )
    .await
    .unwrap_or_else(|error| panic!("JSON-text argument: {error:?}"));
    let payload = sent["payload"].as_str().unwrap();
    assert!(!payload.contains("731299"), "{payload}");
    let inner: Value = serde_json::from_str(payload).expect("payload stays JSON");
    assert_eq!(inner["note"], "ok");
    // A labelled number inside the JSON text refuses typed instead of becoming a token.
    let refused = dispatch_capture(
        &pipeline,
        &chain,
        &Session::new(Scope::Ephemeral).unwrap(),
        json!({"payload": r#"{"customer_id":731299}"#}),
    )
    .await;
    assert!(
        matches!(
            &refused,
            Err(DispatchError::Protection(
                gaze::ProtectionError::Untransformable { .. }
            ))
        ),
        "typed refusal expected, got {refused:?}"
    );

    // Prose stays out of scope: no free-text label guessing. A fresh session, so no repeat
    // of an earlier bound value applies.
    let fresh = Session::new(Scope::Ephemeral).unwrap();
    let sent = dispatch_capture(
        &pipeline,
        &chain,
        &fresh,
        json!({"payload": "customer_id: C-731284"}),
    )
    .await
    .unwrap();
    assert!(sent["payload"].as_str().unwrap().contains("C-731284"));
}
