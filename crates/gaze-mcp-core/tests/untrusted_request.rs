//! Request execution and audit are separate; every response still crosses core.
use async_trait::async_trait;
use gaze::{Action, ClassRule, DefaultRule, Detection, Detector, PiiClass};
use gaze_mcp_core::*;
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};

const REQUEST: &str = "request-only@example.invalid";
const RESPONSE: &str = "response-only@example.invalid";

struct Primary(Arc<Mutex<Vec<String>>>);
impl Detector for Primary {
    fn detect(&self, input: &str) -> Vec<Detection> {
        self.0.lock().unwrap().push(input.into());
        [REQUEST, RESPONSE]
            .into_iter()
            .flat_map(|raw| {
                input.match_indices(raw).map(move |(start, _)| {
                    Detection::new(start..start + raw.len(), PiiClass::Email, "synthetic")
                })
            })
            .collect()
    }
}
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
#[derive(Default)]
struct Store {
    events: Mutex<Vec<&'static str>>,
    audit: Mutex<Vec<String>>,
    fail: Option<&'static str>,
}
#[async_trait]
impl ManifestStore for Store {
    async fn begin_call(&self, ctx: BeginCallContext<'_>) -> Result<CallHandle, ManifestError> {
        self.events.lock().unwrap().push("begin");
        assert_eq!(ctx.redacted_args, &Value::Null);
        assert_eq!(
            ctx.args_audit.unwrap(),
            &json!({"gaze_request_audit":1,"mode":"metadata_only","arguments":"omitted"})
        );
        self.audit.lock().unwrap().push(format!("{ctx:?}"));
        if self.fail == Some("begin") {
            return Err(ManifestError::Validation("synthetic".into()));
        }
        Ok(CallHandle::new(ctx.call_id))
    }
    async fn finish_call(&self, _: CallHandle, _: SnapshotRef) -> Result<(), ManifestError> {
        self.events.lock().unwrap().push("finish");
        if self.fail == Some("finish") {
            return Err(ManifestError::Validation("synthetic".into()));
        }
        Ok(())
    }
    async fn fail_call(&self, _: CallHandle, _: FailureReason) -> Result<(), ManifestError> {
        self.events.lock().unwrap().push("fail");
        Ok(())
    }
}
struct Producer {
    descriptor: ToolDescriptor,
    seen: Arc<Mutex<Vec<Value>>>,
    output: Value,
    fail: bool,
}
#[async_trait]
impl Tool for Producer {
    fn descriptor(&self) -> &ToolDescriptor {
        &self.descriptor
    }
    async fn invoke(&self, ctx: &ToolCtx<'_>) -> Result<ToolResponse, ToolError> {
        assert_eq!(ctx.redacted_args(), &Value::Null);
        assert!(!format!("{ctx:?}").contains(REQUEST));
        self.seen
            .lock()
            .unwrap()
            .push(ctx.invocation_args().unwrap().as_value().clone());
        if self.fail {
            return Err(ToolError::InvalidArgs("request rejected".into()));
        }
        Ok(ToolResponse::json(self.output.clone()))
    }
}
async fn run(
    mode: RequestMode,
    new_entry: bool,
    bypass: bool,
    fail: Option<&str>,
    output: Value,
) -> (
    bool,
    Vec<&'static str>,
    Vec<Value>,
    Vec<String>,
    Vec<String>,
) {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let detections = Arc::new(Mutex::new(Vec::new()));
    let pipeline = gaze::Pipeline::builder()
        .detector(Primary(detections.clone()))
        .rule(ClassRule::new(PiiClass::Email, Action::Tokenize))
        .rule(DefaultRule::new(Action::Preserve))
        .build()
        .unwrap();
    let session = gaze::Session::new(gaze::Scope::Ephemeral).unwrap();
    let store = Store {
        fail: match fail {
            Some("begin") => Some("begin"),
            Some("finish") => Some("finish"),
            _ => None,
        },
        ..Default::default()
    };
    let mut descriptor = ToolDescriptor::operator("test", json!({})).with_request_mode(mode);
    if bypass {
        descriptor = descriptor.with_response_redaction(ResponseRedaction::BypassByOperator);
    }
    let mut registry = ToolRegistry::new();
    registry
        .register(Producer {
            descriptor,
            seen: seen.clone(),
            output,
            fail: fail == Some("tool"),
        })
        .unwrap();
    let policy = SessionIdPolicy::default_strict();
    let envelope = PiiEnvelope::new(&registry, &Auth, &store, &pipeline, &session, &[], &policy);
    let input = json!({"unrestricted_request":REQUEST,"number":123});
    let result = if new_entry {
        envelope
            .dispatch_request(
                &Principal::new("test"),
                "test",
                UntrustedInvocationArgs::new(input.clone()),
                None,
            )
            .await
    } else {
        envelope
            .dispatch(&Principal::new("test"), "test", input.clone(), None)
            .await
    };
    if let Ok(response) = &result {
        assert!(!response.payload.to_string().contains(RESPONSE));
        assert_eq!(seen.lock().unwrap()[0], input);
        assert_eq!(
            session
                .restore_strict_text(response.payload.as_str().unwrap())
                .unwrap(),
            RESPONSE
        );
    }
    assert!(!format!("{:?}", store.audit.lock().unwrap()).contains(REQUEST));
    let raw: Vec<_> = session
        .snapshot_entries()
        .iter()
        .map(|entry| entry.raw.clone())
        .collect();
    assert!(!raw.iter().any(|v| v.contains(REQUEST)));
    let success = result.is_ok();
    let events = store.events.lock().unwrap().clone();
    let observed = seen.lock().unwrap().clone();
    let scanned = detections.lock().unwrap().clone();
    (success, events, observed, scanned, raw)
}
#[tokio::test]
async fn both_entry_points_reject_the_wrong_descriptor_before_audit_or_detection() {
    for (mode, entry) in [
        (RequestMode::Protected, true),
        (RequestMode::UntrustedInvocation, false),
    ] {
        let (ok, events, seen, scanned, _) = run(mode, entry, false, None, json!(RESPONSE)).await;
        assert!(!ok);
        assert!(events.is_empty());
        assert!(seen.is_empty());
        assert!(scanned.is_empty());
    }
}
#[tokio::test]
async fn untrusted_operator_bypass_is_rejected_before_audit() {
    let (ok, events, seen, scanned, _) = run(
        RequestMode::UntrustedInvocation,
        true,
        true,
        None,
        json!(RESPONSE),
    )
    .await;
    assert!(!ok);
    assert!(events.is_empty());
    assert!(seen.is_empty());
    assert!(scanned.is_empty());
}
#[tokio::test]
async fn exact_requests_are_not_scanned_but_response_is_protected_and_committed() {
    let (ok, events, _, scanned, raw) = run(
        RequestMode::UntrustedInvocation,
        true,
        false,
        None,
        json!(RESPONSE),
    )
    .await;
    assert!(ok);
    assert_eq!(events, ["begin", "finish"]);
    assert!(!scanned.iter().any(|s| s.contains(REQUEST)));
    assert!(scanned.iter().any(|s| s.contains(RESPONSE)));
    assert!(raw.contains(&RESPONSE.to_string()));
}
#[tokio::test]
async fn failures_preserve_single_terminal_attempt_and_no_request_mapping() {
    for (failure, expected) in [
        ("begin", vec!["begin"]),
        ("tool", vec!["begin", "fail"]),
        ("finish", vec!["begin", "finish"]),
    ] {
        let (ok, events, _, scanned, raw) = run(
            RequestMode::UntrustedInvocation,
            true,
            false,
            Some(failure),
            json!(RESPONSE),
        )
        .await;
        assert!(!ok);
        assert_eq!(events, expected);
        assert!(!scanned.iter().any(|s| s.contains(REQUEST)));
        if failure == "finish" {
            assert!(raw.contains(&RESPONSE.to_string()));
        }
    }
    let (ok, events, _, _, _) = run(
        RequestMode::UntrustedInvocation,
        true,
        false,
        None,
        json!({"undeclared": RESPONSE}),
    )
    .await;
    assert!(!ok);
    assert_eq!(events, ["begin", "fail"]);
}
#[test]
fn descriptor_mode_is_private_metadata_and_defaults_to_protected() {
    let descriptor = ToolDescriptor::agent("test", json!({}));
    let wire = serde_json::to_value(&descriptor).unwrap();
    assert_eq!(
        wire,
        serde_json::to_value(descriptor.with_request_mode(RequestMode::UntrustedInvocation))
            .unwrap()
    );
    let roundtrip: ToolDescriptor = serde_json::from_value(wire).unwrap();
    assert_eq!(roundtrip.request_mode(), RequestMode::Protected);
}
