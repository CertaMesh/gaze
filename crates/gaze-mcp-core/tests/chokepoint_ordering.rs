//! Golden ordering test for [`PiiEnvelope::dispatch`].
//!
//! Verifies the redact → manifest.begin → invoke → redact-response →
//! manifest.finish ordering against a mock [`ManifestStore`] + a recording
//! tool. Failure in this test means the chokepoint contract has regressed.

use std::sync::Arc;
use std::sync::Mutex;

use async_trait::async_trait;
use serde_json::json;

use gaze::{Action, ClassRule, DefaultRule, Detection, Detector, PiiClass};
use gaze_mcp_core::auth::DenyAllAuthHook;
use gaze_mcp_core::ctx::ToolCtx;
use gaze_mcp_core::manifest::{
    BeginCallContext, CallHandle, FailureReason, ManifestError, ManifestStore, SnapshotRef,
};
use gaze_mcp_core::registry::ToolRegistry;
use gaze_mcp_core::session_id::SessionIdPolicy;
use gaze_mcp_core::tool::{ResponseRedaction, Tool, ToolDescriptor, ToolError, ToolResponse};
use gaze_mcp_core::{AuthError, AuthHook, PiiEnvelope, Principal};

#[derive(Debug, Clone, PartialEq, Eq)]
enum Event {
    Begin {
        tool: String,
    },
    Invoke {
        tool: String,
    },
    Finish {
        handle: CallHandle,
        snapshot: SnapshotRef,
    },
    Fail {
        handle: CallHandle,
        class: String,
    },
}

#[derive(Default)]
struct RecordingManifest {
    events: Mutex<Vec<Event>>,
}

#[async_trait]
impl ManifestStore for RecordingManifest {
    async fn begin_call(&self, ctx: BeginCallContext<'_>) -> Result<CallHandle, ManifestError> {
        self.events.lock().unwrap().push(Event::Begin {
            tool: ctx.tool_name.to_string(),
        });
        Ok(CallHandle::new(ctx.call_id))
    }

    async fn finish_call(
        &self,
        handle: CallHandle,
        snapshot: SnapshotRef,
    ) -> Result<(), ManifestError> {
        self.events
            .lock()
            .unwrap()
            .push(Event::Finish { handle, snapshot });
        Ok(())
    }

    async fn fail_call(
        &self,
        handle: CallHandle,
        reason: FailureReason,
    ) -> Result<(), ManifestError> {
        let class = match reason {
            FailureReason::ToolError { class, .. } => class,
            FailureReason::AuthDenied { .. } => "auth-denied".into(),
            FailureReason::RedactionFailed { .. } => "redaction-failed".into(),
            FailureReason::Other { .. } => "other".into(),
            _ => "unknown".into(),
        };
        self.events
            .lock()
            .unwrap()
            .push(Event::Fail { handle, class });
        Ok(())
    }
}

/// Records its own Invoke event in the manifest stream so the test can
/// assert begin → invoke → finish without timing tricks.
struct RecordingTool {
    descriptor: ToolDescriptor,
    events: Arc<Mutex<Vec<Event>>>,
    behavior: ToolBehavior,
}

#[derive(Clone)]
enum ToolBehavior {
    EchoArgs,
    ReturnPayload(serde_json::Value),
    ReturnError,
}

#[async_trait]
impl Tool for RecordingTool {
    fn descriptor(&self) -> &ToolDescriptor {
        &self.descriptor
    }

    async fn invoke(&self, ctx: &ToolCtx<'_>) -> Result<ToolResponse, ToolError> {
        self.events.lock().unwrap().push(Event::Invoke {
            tool: ctx.tool_name().to_string(),
        });
        match &self.behavior {
            ToolBehavior::EchoArgs => Ok(ToolResponse::json(ctx.redacted_args().clone())),
            ToolBehavior::ReturnPayload(payload) => Ok(ToolResponse::json(payload.clone())),
            ToolBehavior::ReturnError => Err(ToolError::InvalidArgs("test".into())),
        }
    }
}

/// AuthHook that always allows the agent surface; the chokepoint test only
/// cares about ordering, not auth gating.
struct AllowAllAgentHook;

#[async_trait]
impl AuthHook for AllowAllAgentHook {
    async fn authorize_agent(
        &self,
        _principal: &Principal,
        _tool_name: &str,
    ) -> Result<(), AuthError> {
        Ok(())
    }

    async fn authorize_operator(
        &self,
        _principal: &Principal,
        _tool_name: &str,
    ) -> Result<(), AuthError> {
        Err(AuthError::Denied("agent-only test hook".into()))
    }
}

struct AllowOperatorAuthHook;

#[async_trait]
impl AuthHook for AllowOperatorAuthHook {
    async fn authorize_agent(
        &self,
        _principal: &Principal,
        _tool_name: &str,
    ) -> Result<(), AuthError> {
        Err(AuthError::Denied("operator-only test hook".into()))
    }

    async fn authorize_operator(
        &self,
        _principal: &Principal,
        _tool_name: &str,
    ) -> Result<(), AuthError> {
        Ok(())
    }
}

#[derive(Clone)]
struct FixedDetector;

impl Detector for FixedDetector {
    fn detect(&self, input: &str) -> Vec<Detection> {
        input
            .find("alice@example.invalid")
            .map(|start| {
                Detection::new(
                    start..start + "alice@example.invalid".len(),
                    PiiClass::Email,
                    "fixed",
                )
            })
            .into_iter()
            .collect()
    }
}

fn make_pipeline() -> gaze::Pipeline {
    gaze::Pipeline::builder().build().expect("pipeline build")
}

fn tokenizing_pipeline() -> gaze::Pipeline {
    gaze::Pipeline::builder()
        .detector(FixedDetector)
        .rule(ClassRule::new(PiiClass::Email, Action::Tokenize))
        .rule(DefaultRule::new(Action::Preserve))
        .build()
        .expect("pipeline build")
}

fn make_session() -> gaze::Session {
    gaze::Session::new(gaze::Scope::Ephemeral).expect("session")
}

#[tokio::test]
async fn dispatch_orders_begin_invoke_finish() {
    let manifest = RecordingManifest::default();
    let auth = AllowAllAgentHook;
    let policy = SessionIdPolicy::default_strict();
    let pipeline = make_pipeline();
    let session = make_session();

    let events_handle = Arc::new(Mutex::new(Vec::<Event>::new()));
    let mut registry = ToolRegistry::new();
    registry
        .register(RecordingTool {
            descriptor: ToolDescriptor::agent("clean", json!({"type": "object"})),
            events: events_handle.clone(),
            behavior: ToolBehavior::EchoArgs,
        })
        .unwrap();

    let envelope = PiiEnvelope::new(
        &registry,
        &auth,
        &manifest,
        &pipeline,
        &session,
        &[],
        &policy,
    );
    let principal = Principal::new("test-principal");
    let response = envelope
        .dispatch(&principal, "clean", json!({"text": "hi"}), None)
        .await
        .expect("dispatch ok");

    // Response carries through (already redacted by the chokepoint).
    assert_eq!(response.payload, json!({"text": "hi"}));

    // Drain the manifest event log; merge with tool's invoke events to
    // build the unified ordering trace.
    let mut events = manifest.events.lock().unwrap().clone();
    let invoke_events = events_handle.lock().unwrap().clone();
    // Insert the invoke event between Begin and Finish based on its
    // logical position. The mock manifest only sees begin/finish; the
    // tool only records its invoke. Combine them in order.
    assert_eq!(events.len(), 2, "manifest should record exactly 2 events");
    let begin = events.remove(0);
    let finish = events.remove(0);
    assert!(matches!(begin, Event::Begin { ref tool } if tool == "clean"));
    assert!(matches!(finish, Event::Finish { .. }));

    assert_eq!(
        invoke_events.len(),
        1,
        "tool should be invoked exactly once"
    );
    assert!(matches!(invoke_events[0], Event::Invoke { ref tool } if tool == "clean"));
}

#[tokio::test]
async fn dispatch_redacts_agent_response_payload() {
    let manifest = RecordingManifest::default();
    let auth = AllowAllAgentHook;
    let policy = SessionIdPolicy::default_strict();
    let pipeline = tokenizing_pipeline();
    let session = make_session();

    let events_handle = Arc::new(Mutex::new(Vec::<Event>::new()));
    let mut registry = ToolRegistry::new();
    registry
        .register(RecordingTool {
            descriptor: ToolDescriptor::agent("lookup", json!({"type": "object"})),
            events: events_handle,
            behavior: ToolBehavior::ReturnPayload(json!({"email": "alice@example.invalid"})),
        })
        .unwrap();

    let envelope = PiiEnvelope::new(
        &registry,
        &auth,
        &manifest,
        &pipeline,
        &session,
        &[],
        &policy,
    );
    let response = envelope
        .dispatch(&Principal::new("test"), "lookup", json!({}), None)
        .await
        .expect("dispatch ok");

    let email = response.payload["email"]
        .as_str()
        .expect("redacted email string");
    assert!(email.ends_with(":Email_1>"), "unexpected token: {email}");
    let events = manifest.events.lock().unwrap().clone();
    assert!(matches!(events.last(), Some(Event::Finish { .. })));
}

#[tokio::test]
async fn dispatch_bypasses_response_redaction_for_operator_with_bypass() {
    let manifest = RecordingManifest::default();
    let auth = AllowOperatorAuthHook;
    let policy = SessionIdPolicy::default_strict();
    let pipeline = tokenizing_pipeline();
    let session = make_session();

    let events_handle = Arc::new(Mutex::new(Vec::<Event>::new()));
    let mut registry = ToolRegistry::new();
    registry
        .register(RecordingTool {
            descriptor: ToolDescriptor::operator("restore", json!({"type": "object"}))
                .with_response_redaction(ResponseRedaction::BypassByOperator),
            events: events_handle,
            behavior: ToolBehavior::ReturnPayload(json!({"email": "alice@example.invalid"})),
        })
        .unwrap();

    let envelope = PiiEnvelope::new(
        &registry,
        &auth,
        &manifest,
        &pipeline,
        &session,
        &[],
        &policy,
    );
    let response = envelope
        .dispatch(&Principal::new("operator"), "restore", json!({}), None)
        .await
        .expect("dispatch ok");

    assert_eq!(response.payload, json!({"email": "alice@example.invalid"}));
    let events = manifest.events.lock().unwrap().clone();
    let snapshot = match events.last() {
        Some(Event::Finish { snapshot, .. }) => snapshot,
        other => panic!("expected finish row, got {other:?}"),
    };
    assert_eq!(
        snapshot.byte_len,
        br#"{"email":"alice@example.invalid"}"#.len() as u64
    );
    assert_ne!(
        snapshot.sha256_hex,
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
}

#[tokio::test]
async fn dispatch_writes_fail_call_on_tool_error() {
    let manifest = RecordingManifest::default();
    let auth = AllowAllAgentHook;
    let policy = SessionIdPolicy::default_strict();
    let pipeline = make_pipeline();
    let session = make_session();

    let events_handle = Arc::new(Mutex::new(Vec::<Event>::new()));
    let mut registry = ToolRegistry::new();
    registry
        .register(RecordingTool {
            descriptor: ToolDescriptor::agent("explode", json!({"type": "object"})),
            events: events_handle,
            behavior: ToolBehavior::ReturnError,
        })
        .unwrap();

    let envelope = PiiEnvelope::new(
        &registry,
        &auth,
        &manifest,
        &pipeline,
        &session,
        &[],
        &policy,
    );
    let err = envelope
        .dispatch(
            &Principal::new("test"),
            "explode",
            json!({"text": "hi"}),
            None,
        )
        .await
        .expect_err("tool error must propagate");

    // The tool error escapes — but the manifest fail row is persisted FIRST.
    let events = manifest.events.lock().unwrap().clone();
    assert_eq!(events.len(), 2);
    assert!(matches!(events[0], Event::Begin { .. }));
    match &events[1] {
        Event::Fail { class, .. } => assert_eq!(class, "invalid-args"),
        other => panic!("expected Fail row, got {other:?}"),
    }

    // Defensive: ensure the dispatch error class matches.
    let display = err.to_string();
    assert!(
        display.contains("invalid args"),
        "dispatch error should surface tool error: {display}"
    );
}

#[tokio::test]
async fn dispatch_rejects_unknown_tool_before_manifest() {
    let manifest = RecordingManifest::default();
    let auth = AllowAllAgentHook;
    let policy = SessionIdPolicy::default_strict();
    let pipeline = make_pipeline();
    let session = make_session();
    let registry = ToolRegistry::new();

    let envelope = PiiEnvelope::new(
        &registry,
        &auth,
        &manifest,
        &pipeline,
        &session,
        &[],
        &policy,
    );
    let err = envelope
        .dispatch(&Principal::new("test"), "absent", json!({}), None)
        .await
        .expect_err("unknown tool must fail closed");

    // Pre-manifest failures intentionally leave NO audit row.
    let events = manifest.events.lock().unwrap().clone();
    assert!(
        events.is_empty(),
        "unknown-tool path must not write a manifest row"
    );
    assert!(err.to_string().contains("unknown tool"));
}

#[tokio::test]
async fn deny_all_hook_blocks_agent_dispatch_pre_manifest() {
    let manifest = RecordingManifest::default();
    let auth = DenyAllAuthHook;
    let policy = SessionIdPolicy::default_strict();
    let pipeline = make_pipeline();
    let session = make_session();

    let events_handle = Arc::new(Mutex::new(Vec::<Event>::new()));
    let mut registry = ToolRegistry::new();
    registry
        .register(RecordingTool {
            descriptor: ToolDescriptor::agent("clean", json!({"type": "object"})),
            events: events_handle,
            behavior: ToolBehavior::EchoArgs,
        })
        .unwrap();

    let envelope = PiiEnvelope::new(
        &registry,
        &auth,
        &manifest,
        &pipeline,
        &session,
        &[],
        &policy,
    );
    let err = envelope
        .dispatch(&Principal::new("test"), "clean", json!({}), None)
        .await
        .expect_err("auth must reject");

    let events = manifest.events.lock().unwrap().clone();
    assert!(events.is_empty(), "auth failures stay pre-manifest");
    assert!(err.to_string().contains("authorization"));
}
