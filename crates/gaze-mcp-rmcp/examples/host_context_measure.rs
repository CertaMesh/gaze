//! Synthetic measurement host; context is selected by the host process only.
use std::sync::Arc;

use async_trait::async_trait;
use gaze_mcp_core::*;
use gaze_mcp_rmcp::{FixedPrincipalResolver, RmcpFrontend};
use serde_json::json;

struct Auth;
#[async_trait]
impl AuthHook for Auth {
    async fn authorize_agent(&self, _: &Principal, _: &str) -> Result<(), AuthError> {
        Ok(())
    }
    async fn authorize_operator(&self, _: &Principal, _: &str) -> Result<(), AuthError> {
        Err(AuthError::Denied("agent tools only".into()))
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

struct Lookup(ToolDescriptor);
#[async_trait]
impl Tool for Lookup {
    fn descriptor(&self) -> &ToolDescriptor {
        &self.0
    }
    async fn invoke(&self, _: &ToolCtx<'_>) -> Result<ToolResponse, ToolError> {
        Ok(ToolResponse::json(json!({"text": "found Qorvex Zunvela"})))
    }
}

struct Host {
    registry: ToolRegistry,
    pipeline: gaze_assembly::CorePipeline,
    session: gaze::Session,
    selection: HostContextSelection,
}
#[async_trait]
impl DispatchHost for Host {
    async fn dispatch(
        &self,
        principal: &Principal,
        name: &str,
        args: serde_json::Value,
        external_session_id: Option<&str>,
        selection: HostContextSelection,
    ) -> Result<ToolResponse, DispatchError> {
        if !matches!(selection, HostContextSelection::Absent) {
            return Err(HostContextError::Unsupported.into());
        }
        PiiEnvelope::new(
            &self.registry,
            &Auth,
            &Store,
            self.pipeline.pipeline(),
            &self.session,
            self.pipeline.locale_chain().as_slice(),
            &SessionIdPolicy::default_strict(),
        )
        .with_context_policy(self.pipeline.policy(), self.pipeline.rulepacks())
        .with_dictionaries(self.pipeline.dictionaries())
        .dispatch(
            principal,
            name,
            args,
            external_session_id,
            self.selection.clone(),
        )
        .await
    }
    fn list_tools(&self) -> Vec<ToolDescriptor> {
        self.registry.list().into_iter().cloned().collect()
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mode = std::env::args().nth(1).ok_or("supply absent or present")?;
    let selection = match mode.as_str() {
        "absent" => HostContextSelection::Absent,
        "present" => {
            let mut record = serde_json::Map::new();
            let mut field_map = serde_json::Map::new();
            for i in 0..32u8 {
                let value = if i == 0 {
                    "Qorvex Zunvela".to_owned()
                } else {
                    format!(
                        "Qorvex Zunvel{}{}",
                        char::from(b'a' + i / 26),
                        char::from(b'a' + i % 26)
                    )
                };
                record.insert(format!("field{i:02}"), json!(value));
                field_map.insert(format!("/field{i:02}"), json!("Name"));
            }
            HostContextSelection::present(
                json!({"record": record, "field_map": field_map}).to_string(),
            )
        }
        _ => return Err("supply absent or present".into()),
    };
    let fields = (0..32).map(|i| format!("field{i:02}")).collect::<Vec<_>>();
    let input_carriers =
        CarrierDeclaration::text_fields(&fields.iter().map(String::as_str).collect::<Vec<_>>());
    let output_carriers = CarrierDeclaration::text_fields(&["text"]);
    let mut registry = ToolRegistry::new();
    registry.register(Lookup(
        ToolDescriptor::agent("lookup", json!({"type":"object"}))
            .with_carriers(input_carriers, output_carriers),
    ))?;
    let host = Arc::new(Host {
        registry,
        pipeline: gaze_assembly::CorePipelineConfig::new().build()?,
        session: gaze::Session::new(gaze::Scope::Ephemeral)?,
        selection,
    });
    RmcpFrontend::stdio(Arc::new(FixedPrincipalResolver::agent("measurement")))
        .serve(host, ShutdownToken::new())
        .await?;
    Ok(())
}
