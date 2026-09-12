//! No-op argument-protection dispatch must not spuriously conflict with a
//! concurrently advancing generation.
//!
//! Regression for a dispatcher defect where a dispatch whose args stage no
//! new token mappings still routed through `SessionTransaction::commit`,
//! which unconditionally advances the session generation. A no-op dispatch
//! that captured generation N would then observe a sibling's commit advancing
//! the live generation to N+1, failing its own commit with
//! `SessionTransactionError::GenerationConflict` even though it staged
//! nothing. The fix skips the commit when `protect_json` added no new tokens,
//! restoring the pre-strict-transaction behavior where no-op args never
//! touched the generation.

use async_trait::async_trait;
use gaze::{Action, ClassRule, DefaultRule, Detection, Detector, PiiClass, Scope, Session};
use gaze_mcp_core::*;
use serde_json::json;
use std::sync::{Arc, Mutex};

const EMAIL: &str = "alice@example.invalid";
const CONCURRENT_RAW: &str = "synthetic concurrent";

#[derive(Clone)]
struct Primary;
impl Detector for Primary {
    fn detect(&self, input: &str) -> Vec<Detection> {
        input
            .match_indices(EMAIL)
            .map(|(start, _)| {
                Detection::new(start..start + EMAIL.len(), PiiClass::Email, "synthetic")
            })
            .collect()
    }
}

fn pipeline() -> gaze::Pipeline {
    gaze::Pipeline::builder()
        .detector(Primary)
        .rule(ClassRule::new(PiiClass::Email, Action::Tokenize))
        .rule(DefaultRule::new(Action::Preserve))
        .build()
        .unwrap()
}

struct Auth;
#[async_trait]
impl AuthHook for Auth {
    async fn authorize_agent(&self, _: &Principal, _: &str) -> Result<(), AuthError> {
        Ok(())
    }
    async fn authorize_operator(&self, principal: &Principal, _: &str) -> Result<(), AuthError> {
        if principal.id == "operator" {
            Ok(())
        } else {
            Err(AuthError::Denied("synthetic".into()))
        }
    }
}

/// Manifest store that records every terminal attempt and optionally mutates
/// the shared `Session` during `begin_call`, simulating a concurrently
/// overlapping sibling dispatch committing (and thus advancing the live
/// generation) between this dispatch's `begin_transaction` (capture) and its
/// args `commit`/drop decision.
#[derive(Default)]
struct Store {
    events: Arc<Mutex<Vec<&'static str>>>,
    mutate_begin: Option<Arc<Session>>,
}

#[async_trait]
impl ManifestStore for Store {
    async fn begin_call(&self, ctx: BeginCallContext<'_>) -> Result<CallHandle, ManifestError> {
        self.events.lock().unwrap().push("begin");
        if let Some(session) = &self.mutate_begin {
            session.tokenize(&PiiClass::Name, CONCURRENT_RAW).unwrap();
        }
        Ok(CallHandle::new(ctx.call_id))
    }
    async fn finish_call(&self, _: CallHandle, _: SnapshotRef) -> Result<(), ManifestError> {
        self.events.lock().unwrap().push("finish");
        Ok(())
    }
    async fn fail_call(&self, _: CallHandle, _: FailureReason) -> Result<(), ManifestError> {
        self.events.lock().unwrap().push("fail");
        Ok(())
    }
}

/// Manifest store that synchronizes concurrent dispatches at `begin_call`
/// using a `tokio::sync::Barrier`. `tokio::join!` runs both futures
/// cooperatively on one task, so a blocking barrier would deadlock; an async
/// barrier yields, letting both dispatches capture the generation in
/// `begin_transaction` and run `protect_json` before either is allowed to
/// reach the commit/drop decision.
struct BarrierStore {
    begin: tokio::sync::Barrier,
}

impl BarrierStore {
    fn new(n: usize) -> Self {
        Self {
            begin: tokio::sync::Barrier::new(n),
        }
    }
}

#[async_trait]
impl ManifestStore for BarrierStore {
    async fn begin_call(&self, ctx: BeginCallContext<'_>) -> Result<CallHandle, ManifestError> {
        self.begin.wait().await;
        Ok(CallHandle::new(ctx.call_id))
    }
    async fn finish_call(&self, _: CallHandle, _: SnapshotRef) -> Result<(), ManifestError> {
        Ok(())
    }
    async fn fail_call(&self, _: CallHandle, _: FailureReason) -> Result<(), ManifestError> {
        Ok(())
    }
}

static STRICT_POLICY: std::sync::LazyLock<SessionIdPolicy> =
    std::sync::LazyLock::new(SessionIdPolicy::default_strict);

fn envelope<'a>(
    registry: &'a ToolRegistry,
    manifest: &'a dyn ManifestStore,
    pipeline: &'a gaze::Pipeline,
    session: &'a Session,
) -> PiiEnvelope<'a> {
    PiiEnvelope::new(
        registry,
        &Auth,
        manifest,
        pipeline,
        session,
        &[gaze::LocaleTag::Global],
        &STRICT_POLICY,
    )
}

#[cfg(feature = "operator-tier")]
fn export_registry() -> ToolRegistry {
    let mut registry = ToolRegistry::new();
    registry
        .register(gaze_mcp_core::operator_tools::ExportSessionTokensTool::new())
        .unwrap();
    registry
}

/// A no-op args dispatch (empty `{}` for `export_session_tokens`) MUST NOT
/// fail when a concurrent sibling advances the live generation during
/// `begin_call`. Pre-fix the unconditional `args_transaction.commit()` saw
/// the captured generation fall behind the live one and returned
/// `GenerationConflict`, spuriously failing the call and persisting a
/// mislabeled `RedactionFailed` audit row. The fix skips the commit when no
/// new tokens were staged, so no generation is published and no conflict is
/// possible.
#[cfg(feature = "operator-tier")]
#[tokio::test]
async fn noop_args_dispatch_succeeds_when_sibling_advances_generation() {
    let session = Arc::new(Session::new(Scope::Ephemeral).unwrap());
    let store = Store {
        mutate_begin: Some(session.clone()),
        ..Default::default()
    };
    let pipeline = pipeline();
    let registry = export_registry();
    let env = envelope(&registry, &store, &pipeline, &session);
    let principal = Principal::new("operator");

    let result = env
        .dispatch(&principal, "export_session_tokens", json!({}), None)
        .await;

    assert!(
        result.is_ok(),
        "no-op args dispatch must not fail on a concurrent generation advance"
    );
    assert_eq!(
        *store.events.lock().unwrap(),
        ["begin", "finish"],
        "a successful dispatch records begin then finish, never fail"
    );
    assert_eq!(
        session.tokens().len(),
        1,
        "only the sibling's staged token should exist; the no-op args staged nothing"
    );
    assert!(
        session
            .snapshot_entries()
            .iter()
            .all(|entry| entry.raw == CONCURRENT_RAW),
        "no email token from the no-op args should have been committed"
    );
}

/// A dispatch that stages real PII in its args MUST still commit and therefore
/// still fail closed when a concurrent sibling advances the live generation.
/// This guards that the no-op skip does not weaken the strict fail-closed
/// boundary for genuine concurrent mutations.
#[tokio::test]
async fn real_args_dispatch_still_fails_closed_when_sibling_advances_generation() {
    struct Echo {
        descriptor: ToolDescriptor,
    }
    #[async_trait]
    impl Tool for Echo {
        fn descriptor(&self) -> &ToolDescriptor {
            &self.descriptor
        }
        async fn invoke(&self, ctx: &ToolCtx<'_>) -> Result<ToolResponse, ToolError> {
            Ok(ToolResponse::json(ctx.redacted_args().clone()))
        }
    }
    fn echo_registry() -> ToolRegistry {
        let mut registry = ToolRegistry::new();
        registry
            .register(Echo {
                descriptor: ToolDescriptor::agent("echo", json!({}))
                    .with_carriers(CarrierDeclaration::default(), CarrierDeclaration::default()),
            })
            .unwrap();
        registry
    }

    let session = Arc::new(Session::new(Scope::Ephemeral).unwrap());
    let store = Store {
        mutate_begin: Some(session.clone()),
        ..Default::default()
    };
    let pipeline = pipeline();
    let registry = echo_registry();
    let env = envelope(&registry, &store, &pipeline, &session);
    let principal = Principal::new("agent");

    let result = env.dispatch(&principal, "echo", json!(EMAIL), None).await;

    assert!(
        matches!(result, Err(DispatchError::Transaction(_))),
        "real args mutation must fail closed on a concurrent generation advance"
    );
    assert_eq!(
        *store.events.lock().unwrap(),
        ["begin", "fail"],
        "a transaction-conflict loss records begin then fail"
    );
    assert!(
        session
            .snapshot_entries()
            .iter()
            .all(|entry| entry.raw != EMAIL),
        "the losing args token must not be committed"
    );
}

/// Two genuinely overlapping no-op args dispatches on one shared `Session`
/// (synchronized at `begin_call` so both capture the same generation before
/// either reaches the commit/drop site) MUST both succeed. Pre-fix exactly
/// one failed with `DispatchError::Transaction`; the fix lets both skip the
/// no-op commit and complete independently.
#[cfg(feature = "operator-tier")]
#[tokio::test]
async fn concurrent_noop_args_dispatches_both_succeed() {
    let session = Session::new(Scope::Ephemeral).unwrap();
    let manifest = BarrierStore::new(2);
    let pipeline = pipeline();
    let registry = export_registry();
    let env = envelope(&registry, &manifest, &pipeline, &session);
    let principal = Principal::new("operator");

    let (r1, r2) = tokio::join!(
        env.dispatch(&principal, "export_session_tokens", json!({}), None),
        env.dispatch(&principal, "export_session_tokens", json!({}), None),
    );

    assert!(
        r1.is_ok(),
        "first overlapping no-op dispatch should succeed: {r1:?}"
    );
    assert!(
        r2.is_ok(),
        "second overlapping no-op dispatch should succeed: {r2:?}"
    );
    assert!(
        session.tokens().is_empty(),
        "no-op dispatches must stage no tokens"
    );
}
