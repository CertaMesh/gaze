//! Capture arguments at the MCP tool boundary, respecting numeric carrier refusal.
#[allow(dead_code)]
#[path = "../../gaze-proxy/tests/support/structured_record_ids.rs"]
mod record_ids;

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use gaze::{Action, Context, LocaleChain, PiiClass, Policy, RuleSpec, Rulepack, Scope, Session};
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
async fn mcp_labelled_string_leaves_known_failure_but_numbers_fail_closed() {
    let mut policy = Policy::default();
    policy.rules = vec![
        RuleSpec::Class {
            class: PiiClass::Custom("probe_record_id".into()),
            action: Action::Tokenize,
        },
        RuleSpec::Default {
            action: Action::Preserve,
        },
    ];
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
        &[Rulepack::parse(record_ids::RULEPACK).unwrap()],
        &chain,
        None,
    )
    .unwrap();
    for case in record_ids::cases() {
        let document = case.document();
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
            .dispatch(&Principal::new("agent"), "lookup", document.clone(), None)
            .await;
        if case.numeric {
            assert!(
                matches!(
                    result,
                    Err(DispatchError::Carrier(CarrierError::UndeclaredNumber))
                ),
                "{case:?}: {result:?}"
            );
            assert!(captured.lock().unwrap().is_empty());
            println!(
                "RECORD_ID MCP/args {case:?} N/A fail-closed UndeclaredNumber; input={document}"
            );
        } else {
            result.unwrap();
            let sent = captured.lock().unwrap().pop().expect("tool must execute");
            record_ids::check(&case, "MCP/args", &sent.to_string(), true);
        }
    }
}
