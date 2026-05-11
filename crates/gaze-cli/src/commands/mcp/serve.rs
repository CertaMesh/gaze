use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::UNIX_EPOCH;

use async_trait::async_trait;
use directories_next::ProjectDirs;
use gaze_mcp_core::{
    AuthError, AuthHook, BeginCallContext, CallHandle, DispatchError, DispatchHost, FailureReason,
    Frontend, ManifestError, ManifestStore, PiiEnvelope, Principal, SessionIdPolicy, ShutdownToken,
    SnapshotRef, ToolDescriptor, ToolRegistry, ToolResponse,
};
use gaze_mcp_rmcp::{FixedPrincipalResolver, RmcpFrontend};
use serde::Serialize;

use crate::error::CliError;

use super::ServeArgs;

pub(crate) fn run(args: ServeArgs) -> Result<(), CliError> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|err| CliError::McpDetail(format!("tokio runtime init failed: {err}")))?;
    runtime.block_on(serve_stdio(args))
}

async fn serve_stdio(args: ServeArgs) -> Result<(), CliError> {
    let manifest_dir = args
        .manifest_dir
        .unwrap_or_else(default_manifest_dir)
        .join("calls");
    let manifest = Arc::new(FileManifestStore::new(manifest_dir)?);
    let host = Arc::new(McpHost::new(manifest, args.max_file_size)?);
    let frontend = RmcpFrontend::stdio(Arc::new(FixedPrincipalResolver::agent("gaze-cli")));
    frontend
        .serve(host, ShutdownToken::new())
        .await
        .map_err(|err| CliError::McpDetail(format!("mcp stdio server failed: {err}")))
}

pub(crate) fn default_manifest_dir() -> PathBuf {
    ProjectDirs::from("dev", "Gaze", "gaze")
        .map(|dirs| dirs.data_dir().join("mcp-manifests"))
        .unwrap_or_else(|| PathBuf::from(".gaze").join("mcp-manifests"))
}

struct McpHost {
    registry: ToolRegistry,
    auth: AllowAgentAuth,
    manifest: Arc<FileManifestStore>,
    pipeline: gaze::Pipeline,
    session: gaze::Session,
    session_id_policy: SessionIdPolicy,
}

impl McpHost {
    fn new(manifest: Arc<FileManifestStore>, max_file_size: Option<u64>) -> Result<Self, CliError> {
        let mut registry = ToolRegistry::new();
        let mut opts = gaze_document::mcp::GazeReadOpts::default();
        if let Some(max_file_size) = max_file_size {
            opts.max_file_size = max_file_size;
        }
        gaze_document::mcp::register_tools(&mut registry, opts)
            .map_err(|err| CliError::McpDetail(format!("mcp tool registration failed: {err}")))?;
        let pipeline = gaze::Pipeline::builder()
            .build()
            .map_err(|err| CliError::McpDetail(format!("mcp redaction pipeline failed: {err}")))?;
        let session = gaze::Session::new(gaze::Scope::Ephemeral)
            .map_err(|err| CliError::McpDetail(format!("mcp session init failed: {err}")))?;
        Ok(Self {
            registry,
            auth: AllowAgentAuth,
            manifest,
            pipeline,
            session,
            session_id_policy: SessionIdPolicy::default_strict(),
        })
    }
}

#[async_trait]
impl DispatchHost for McpHost {
    async fn dispatch(
        &self,
        principal: &Principal,
        tool_name: &str,
        raw_args: serde_json::Value,
        external_session_id: Option<&str>,
    ) -> Result<ToolResponse, DispatchError> {
        let envelope = PiiEnvelope::new(
            &self.registry,
            &self.auth,
            self.manifest.as_ref(),
            &self.pipeline,
            &self.session,
            &[gaze::LocaleTag::Global],
            &self.session_id_policy,
        );
        envelope
            .dispatch(principal, tool_name, raw_args, external_session_id)
            .await
    }

    fn list_tools(&self) -> Vec<ToolDescriptor> {
        self.registry.list().into_iter().cloned().collect()
    }
}

#[derive(Debug, Default)]
struct AllowAgentAuth;

#[async_trait]
impl AuthHook for AllowAgentAuth {
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
        Err(AuthError::Denied(
            "gaze-cli mcp serve exposes only agent-tier document tools".to_string(),
        ))
    }
}

#[derive(Debug)]
struct FileManifestStore {
    dir: PathBuf,
    handles: Mutex<HashSet<CallHandle>>,
}

impl FileManifestStore {
    fn new(dir: PathBuf) -> Result<Self, CliError> {
        std::fs::create_dir_all(&dir).map_err(|err| {
            CliError::McpDetail(format!(
                "cannot create manifest directory `{}`: {err}",
                dir.display()
            ))
        })?;
        Ok(Self {
            dir,
            handles: Mutex::new(HashSet::new()),
        })
    }

    fn path_for(&self, handle: CallHandle) -> PathBuf {
        self.dir.join(format!("{}.json", handle.id()))
    }

    fn write_record(&self, path: &Path, record: &ManifestRecord) -> Result<(), ManifestError> {
        let bytes = serde_json::to_vec_pretty(record).map_err(ManifestError::backend)?;
        std::fs::write(path, bytes).map_err(ManifestError::backend)
    }
}

#[async_trait]
impl ManifestStore for FileManifestStore {
    async fn begin_call(&self, ctx: BeginCallContext<'_>) -> Result<CallHandle, ManifestError> {
        let handle = CallHandle::new(ctx.call_id);
        {
            let mut handles = self.handles.lock().map_err(|_| {
                ManifestError::Validation("manifest handle mutex poisoned".to_string())
            })?;
            if !handles.insert(handle) {
                return Err(ManifestError::DuplicateCallId(handle));
            }
        }
        let record = ManifestRecord::Started {
            call_id: ctx.call_id.to_string(),
            external_session_id: ctx.external_session_id.map(ToOwned::to_owned),
            principal_id: ctx.principal_id.to_string(),
            tool_name: ctx.tool_name.to_string(),
            redacted_args: ctx.redacted_args.clone(),
            started_at_unix_ms: unix_ms(ctx.started_at),
        };
        self.write_record(&self.path_for(handle), &record)?;
        Ok(handle)
    }

    async fn finish_call(
        &self,
        handle: CallHandle,
        snapshot: SnapshotRef,
    ) -> Result<(), ManifestError> {
        self.finish_handle(handle)?;
        self.write_record(
            &self.path_for(handle),
            &ManifestRecord::Finished {
                call_id: handle.id().to_string(),
                snapshot,
            },
        )
    }

    async fn fail_call(
        &self,
        handle: CallHandle,
        reason: FailureReason,
    ) -> Result<(), ManifestError> {
        self.finish_handle(handle)?;
        self.write_record(
            &self.path_for(handle),
            &ManifestRecord::Failed {
                call_id: handle.id().to_string(),
                reason,
            },
        )
    }
}

impl FileManifestStore {
    fn finish_handle(&self, handle: CallHandle) -> Result<(), ManifestError> {
        let mut handles = self
            .handles
            .lock()
            .map_err(|_| ManifestError::Validation("manifest handle mutex poisoned".to_string()))?;
        if handles.remove(&handle) {
            Ok(())
        } else {
            Err(ManifestError::UnknownHandle(handle))
        }
    }
}

#[derive(Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
enum ManifestRecord {
    Started {
        call_id: String,
        external_session_id: Option<String>,
        principal_id: String,
        tool_name: String,
        redacted_args: serde_json::Value,
        started_at_unix_ms: u128,
    },
    Finished {
        call_id: String,
        snapshot: SnapshotRef,
    },
    Failed {
        call_id: String,
        reason: FailureReason,
    },
}

fn unix_ms(time: std::time::SystemTime) -> u128 {
    time.duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default()
}
