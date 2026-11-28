//! Two conversations routed through one `ConversationRegistry` never share
//! mappings, and each refuses the other's tokens on strict restore.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use gaze_mcp_core::*;
use serde_json::{json, Value};

// fixture-cited(crates/gaze-mcp-core/tests/conversation_isolation.rs:conversations_never_share_mappings_or_restore_authority)
const RAW: &str = "alice@example.invalid";

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

struct Auth;

#[async_trait]
impl AuthHook for Auth {
    async fn authorize_agent(&self, _: &Principal, _: &str) -> Result<(), AuthError> {
        Ok(())
    }
    async fn authorize_operator(&self, _: &Principal, _: &str) -> Result<(), AuthError> {
        Err(AuthError::Denied("synthetic".into()))
    }
}

/// A source tool that returns the same private value to every conversation.
struct Lookup(ToolDescriptor);

#[async_trait]
impl Tool for Lookup {
    fn descriptor(&self) -> &ToolDescriptor {
        &self.0
    }
    async fn invoke(&self, _: &ToolCtx<'_>) -> Result<ToolResponse, ToolError> {
        Ok(ToolResponse::json(json!({ "found": RAW })))
    }
}

struct Shared {
    registry: ToolRegistry,
    core: gaze_assembly::CorePipeline,
    policy: SessionIdPolicy,
}

struct Host {
    shared: Arc<Shared>,
    session: gaze::Session,
}

#[async_trait]
impl DispatchHost for Host {
    async fn dispatch(
        &self,
        principal: &Principal,
        tool_name: &str,
        raw_args: Value,
        external_session_id: Option<&str>,
        host_context: HostContextSelection,
    ) -> Result<ToolResponse, DispatchError> {
        let shared = self.shared.as_ref();
        PiiEnvelope::new(
            &shared.registry,
            &Auth,
            &Store,
            shared.core.pipeline(),
            &self.session,
            shared.core.locale_chain().as_slice(),
            &shared.policy,
        )
        .dispatch(
            principal,
            tool_name,
            raw_args,
            external_session_id,
            host_context,
        )
        .await
    }

    fn list_tools(&self) -> Vec<ToolDescriptor> {
        self.shared.registry.list().into_iter().cloned().collect()
    }
}

/// A registry plus every host it built, in build order, for inspection.
fn conversations(limits: ConversationLimits) -> (ConversationRegistry, Arc<Mutex<Vec<Arc<Host>>>>) {
    let mut registry = ToolRegistry::new();
    registry
        .register(Lookup(
            ToolDescriptor::agent("lookup", json!({"type": "object"})).with_carriers(
                CarrierDeclaration::text_fields(&[]),
                CarrierDeclaration::text_fields(&["found"]),
            ),
        ))
        .unwrap();
    let shared = Arc::new(Shared {
        registry,
        core: gaze_assembly::CorePipelineConfig::new().build().unwrap(),
        policy: SessionIdPolicy::default_strict(),
    });
    let built = Arc::new(Mutex::new(Vec::new()));
    let seen = built.clone();
    let registry = ConversationRegistry::new(
        move |_: &ConversationId, _: &Principal| {
            let host = Arc::new(Host {
                shared: shared.clone(),
                session: gaze::Session::new(gaze::Scope::Ephemeral)?,
            });
            seen.lock().unwrap().push(host.clone());
            Ok(host as Arc<dyn DispatchHost>)
        },
        limits,
    );
    (registry, built)
}

async fn lookup(registry: &ConversationRegistry, id: &ConversationId) -> String {
    let response = registry
        .host(&Principal::new("synthetic-agent"), id)
        .unwrap()
        .dispatch(
            &Principal::new("synthetic-agent"),
            "lookup",
            json!({}),
            None,
            HostContextSelection::Absent,
        )
        .await
        .unwrap();
    let clean = response.payload["found"].as_str().unwrap().to_owned();
    assert!(!clean.contains(RAW));
    clean
}

#[tokio::test]
async fn conversations_never_share_mappings_or_restore_authority() {
    let (registry, built) = conversations(ConversationLimits::default());
    let a = ConversationId::new("conversation-a");
    let b = ConversationId::new("conversation-b");

    let token_a = lookup(&registry, &a).await;
    let a_tokens = built.lock().unwrap()[0].session.tokens();
    let token_b = lookup(&registry, &b).await;
    assert_eq!(
        lookup(&registry, &a).await,
        token_a,
        "a conversation reuses its own mapping"
    );

    let hosts = built.lock().unwrap().clone();
    assert_eq!(hosts.len(), 2, "one Session per conversation");
    let (host_a, host_b) = (&hosts[0], &hosts[1]);
    assert_eq!(host_a.session.restore_strict_text(&token_a).unwrap(), RAW);
    assert_eq!(host_b.session.restore_strict_text(&token_b).unwrap(), RAW);
    assert!(host_b.session.restore_strict_text(&token_a).is_err());
    assert!(host_a.session.restore_strict_text(&token_b).is_err());
    assert_eq!(
        host_a.session.tokens(),
        a_tokens,
        "b's calls never touch a's mappings"
    );
    assert!(!host_b.session.tokens().contains(&token_a));
}

#[tokio::test]
async fn a_closed_conversation_reopens_without_its_old_mappings() {
    let (registry, built) = conversations(ConversationLimits::default());
    let a = ConversationId::new("conversation-a");
    let before = lookup(&registry, &a).await;
    assert!(registry.close(&a));
    lookup(&registry, &a).await;
    let hosts = built.lock().unwrap().clone();
    assert_eq!(hosts.len(), 2);
    assert!(hosts[1].session.restore_strict_text(&before).is_err());
}

#[tokio::test]
async fn an_expired_conversation_reopens_without_its_old_mappings() {
    let (registry, built) =
        conversations(ConversationLimits::new(4, Duration::from_millis(20)).unwrap());
    let a = ConversationId::new("conversation-a");
    let before = lookup(&registry, &a).await;
    std::thread::sleep(Duration::from_millis(40));
    lookup(&registry, &a).await;
    let hosts = built.lock().unwrap().clone();
    assert_eq!(hosts.len(), 2);
    assert!(hosts[1].session.restore_strict_text(&before).is_err());
}
