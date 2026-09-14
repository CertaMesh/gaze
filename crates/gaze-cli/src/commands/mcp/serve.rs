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
    pipeline: gaze_assembly::CorePipeline,
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
        let pipeline = gaze_assembly::CorePipelineConfig::new()
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
            self.pipeline.pipeline(),
            &self.session,
            self.pipeline.locale_chain().as_slice(),
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

#[cfg(test)]
mod strict_host_tests {
    use super::*;

    #[tokio::test]
    async fn shipped_host_has_real_primary_detection_and_document_round_trip() {
        let directory = std::env::temp_dir().join(format!(
            "gaze-mcp-host-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        let manifest = Arc::new(FileManifestStore::new(directory.clone()).expect("manifest"));
        let host = McpHost::new(manifest, None).expect("host");
        // fixture-cited(crates/gaze-cli/src/commands/mcp/serve.rs:commands::mcp::serve::strict_host_tests::shipped_host_has_real_primary_detection_and_document_round_trip)
        let input = "alice@example.invalid";
        let response = host
            .dispatch(
                &Principal::new("synthetic"),
                "gaze_read_text",
                serde_json::json!({"text":input}),
                None,
            )
            .await
            .expect("document text succeeds");
        let clean = response.payload["clean_markdown"]
            .as_str()
            .expect("markdown");
        assert!(!clean.contains(input));
        assert!(host
            .session
            .restore_strict_text(clean)
            .expect("restore")
            .contains(input));
        std::fs::remove_dir_all(directory).expect("remove synthetic manifest");
    }
}

#[cfg(test)]
mod manifest_store_tests {
    use super::*;
    use serde_json::{json, Value};

    struct FailingText(ToolDescriptor);

    #[async_trait]
    impl gaze_mcp_core::Tool for FailingText {
        fn descriptor(&self) -> &ToolDescriptor {
            &self.0
        }

        async fn invoke(
            &self,
            _ctx: &gaze_mcp_core::ToolCtx<'_>,
        ) -> Result<ToolResponse, gaze_mcp_core::ToolError> {
            Err(gaze_mcp_core::ToolError::InvalidArgs(
                "synthetic tool failure".into(),
            ))
        }
    }

    // Observe the real dispatcher context without adding a public context constructor.
    struct Probe {
        store: FileManifestStore,
        started: Mutex<Option<(CallHandle, Vec<u8>)>>,
        block_begin: bool,
        block_terminal: bool,
    }

    #[async_trait]
    impl ManifestStore for Probe {
        async fn begin_call(&self, ctx: BeginCallContext<'_>) -> Result<CallHandle, ManifestError> {
            let handle = CallHandle::new(ctx.call_id);
            let path = self.store.path_for(handle);
            if self.block_begin {
                std::fs::create_dir(&path).unwrap();
            }
            let result = self.store.begin_call(ctx).await;
            assert!(matches!(
                self.store.begin_call(ctx).await,
                Err(ManifestError::DuplicateCallId(h)) if h == handle
            ));
            let handle = result?;
            let bytes = std::fs::read(path).unwrap();
            let record: Value = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(
                record,
                json!({
                    "status": "started",
                    "call_id": ctx.call_id.to_string(),
                    "external_session_id": ctx.external_session_id,
                    "principal_id": ctx.principal_id,
                    "tool_name": ctx.tool_name,
                    "redacted_args": ctx.redacted_args,
                    "started_at_unix_ms": unix_ms(ctx.started_at),
                })
            );
            *self.started.lock().unwrap() = Some((handle, bytes));
            if self.block_terminal {
                std::fs::create_dir(
                    self.store
                        .dir
                        .join(format!("{}.terminal.json", handle.id())),
                )
                .unwrap();
            }
            Ok(handle)
        }

        async fn finish_call(
            &self,
            handle: CallHandle,
            snapshot: SnapshotRef,
        ) -> Result<(), ManifestError> {
            self.store.finish_call(handle, snapshot).await
        }

        async fn fail_call(
            &self,
            handle: CallHandle,
            reason: FailureReason,
        ) -> Result<(), ManifestError> {
            self.store.fail_call(handle, reason).await
        }
    }

    fn reason() -> FailureReason {
        FailureReason::Other {
            message: "synthetic failure".into(),
        }
    }

    async fn assert_consumed(store: &FileManifestStore, handle: CallHandle) {
        assert!(matches!(
            store.finish_call(handle, SnapshotRef::new("synthetic", "00", 0)).await,
            Err(ManifestError::UnknownHandle(h)) if h == handle
        ));
        assert!(matches!(
            store.fail_call(handle, reason()).await,
            Err(ManifestError::UnknownHandle(h)) if h == handle
        ));
    }

    async fn exercise(failure: bool, block_begin: bool, block_terminal: bool) {
        let directory = tempfile::tempdir().unwrap();
        let probe = Probe {
            store: FileManifestStore::new(directory.path().to_owned()).unwrap(),
            started: Mutex::new(None),
            block_begin,
            block_terminal,
        };
        let host = McpHost::new(
            Arc::new(FileManifestStore::new(directory.path().to_owned()).unwrap()),
            None,
        )
        .unwrap();
        let mut failing_registry = ToolRegistry::new();
        if failure {
            let descriptor = host
                .registry
                .list()
                .into_iter()
                .find(|tool| tool.name() == "gaze_read_text")
                .unwrap()
                .clone();
            failing_registry.register(FailingText(descriptor)).unwrap();
        }
        let envelope = PiiEnvelope::new(
            if failure {
                &failing_registry
            } else {
                &host.registry
            },
            &host.auth,
            &probe,
            host.pipeline.pipeline(),
            &host.session,
            host.pipeline.locale_chain().as_slice(),
            &host.session_id_policy,
        );
        // fixture-cited(crates/gaze-cli/src/commands/mcp/serve.rs:commands::mcp::serve::manifest_store_tests::success_preserves_audit_context_on_disk)
        // fixture-cited(crates/gaze-cli/src/commands/mcp/serve.rs:commands::mcp::serve::manifest_store_tests::failure_preserves_audit_context_on_disk)
        let raw = "alice@example.invalid";
        let args = json!({"text": raw});
        let result = envelope
            .dispatch(
                &Principal::new("synthetic-agent"),
                "gaze_read_text",
                args.clone(),
                Some("01ARZ3NDEKTSV4RRFFQ69G5FAV"),
            )
            .await;
        if block_begin || block_terminal {
            assert!(matches!(
                result,
                Err(DispatchError::Manifest(ManifestError::Backend(_)))
            ));
        } else if failure {
            assert!(matches!(result, Err(DispatchError::ToolError(_))));
        } else {
            result.unwrap();
        }
        if block_begin {
            assert!(probe.started.lock().unwrap().is_none());
            return;
        }
        let (handle, before) = probe.started.lock().unwrap().take().unwrap();
        assert_consumed(&probe.store, handle).await;
        drop(envelope);
        drop(host);
        drop(probe);
        let reopened = FileManifestStore::new(directory.path().to_owned()).unwrap();
        assert_eq!(
            std::fs::read(reopened.path_for(handle)).unwrap(),
            before,
            "terminal persistence must preserve the complete begin record byte-for-byte"
        );
        let start: Value = serde_json::from_slice(&before).unwrap();
        assert_eq!(start["principal_id"], "synthetic-agent");
        assert_eq!(start["tool_name"], "gaze_read_text");
        assert_eq!(start["external_session_id"], "01ARZ3NDEKTSV4RRFFQ69G5FAV");
        assert_ne!(start["redacted_args"], args);
        assert!(!String::from_utf8(before).unwrap().contains(raw));
        assert!(start["started_at_unix_ms"].as_u64().unwrap() > 0);
        let terminal_path = directory
            .path()
            .join(format!("{}.terminal.json", handle.id()));
        if block_terminal {
            assert!(terminal_path.is_dir());
        } else {
            let terminal: Value =
                serde_json::from_slice(&std::fs::read(terminal_path).unwrap()).unwrap();
            assert_eq!(terminal["call_id"], start["call_id"]);
            assert_eq!(
                terminal["status"],
                if failure { "failed" } else { "finished" }
            );
            assert!(terminal
                .get(if failure { "reason" } else { "snapshot" })
                .is_some());
            assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 2);
        }
        // Reopening does not restore in-flight handles or permit terminal retries.
        assert_consumed(&reopened, handle).await;
    }

    #[tokio::test]
    async fn success_preserves_audit_context_on_disk() {
        exercise(false, false, false).await;
    }

    #[tokio::test]
    async fn failure_preserves_audit_context_on_disk() {
        exercise(true, false, false).await;
    }

    #[tokio::test]
    async fn terminal_io_failure_preserves_start_and_consumes_handle() {
        exercise(false, false, true).await;
        exercise(true, false, true).await;
    }

    #[tokio::test]
    async fn begin_io_failure_rejects_duplicate_retry() {
        exercise(false, true, false).await;
    }

    #[tokio::test]
    async fn unknown_handles_never_write_records() {
        let directory = tempfile::tempdir().unwrap();
        let store = FileManifestStore::new(directory.path().to_owned()).unwrap();
        let handle = CallHandle::new("01ARZ3NDEKTSV4RRFFQ69G5FAV".parse().unwrap());
        assert_consumed(&store, handle).await;
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 0);
    }
}
