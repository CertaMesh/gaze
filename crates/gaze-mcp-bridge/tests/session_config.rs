use std::collections::BTreeMap;
use std::sync::Arc;

use gaze_mcp_bridge::audit::FileBridgeAuditSink;
use gaze_mcp_bridge::config::{BridgeConfig, SessionCfg, SessionMode};
use gaze_mcp_bridge::{BridgeError, BridgeHost, BridgeSessionStore};

#[tokio::test]
async fn direct_file_config_rejects_zero_and_positive_configs_enforce_capacity() {
    let dir = tempfile::TempDir::new().unwrap();
    let key_env = "GAZE_BRIDGE_DIRECT_SESSION_CONFIG_KEY";
    std::env::set_var(key_env, "77".repeat(32));
    let mut config = SessionCfg {
        mode: SessionMode::File,
        dir: Some(dir.path().to_path_buf()),
        key_env: Some(key_env.to_string()),
        max_sessions: 0,
    };
    assert!(matches!(
        BridgeSessionStore::from_config(&config),
        Err(BridgeError::Config(message))
            if message == "session.max_sessions must be greater than 0"
    ));
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);

    config.max_sessions = 1;
    for mode in [SessionMode::File, SessionMode::Ephemeral] {
        config.mode = mode;
        let store = BridgeSessionStore::from_config(&config).expect("positive cap accepted");
        let first = store.get("session-a").await.unwrap();
        assert!(matches!(
            store.get("session-b").await,
            Err(BridgeError::LimitExceeded(_))
        ));
        assert_eq!(store.len().await, 1);
        assert!(Arc::ptr_eq(&first, &store.get("session-a").await.unwrap()));
    }
}

#[tokio::test]
async fn host_rejects_zero_before_spawning_downstream() {
    let dir = tempfile::TempDir::new().unwrap();
    // If host construction starts the child first, the missing executable returns
    // a downstream error instead of the promised configuration error.
    let servers = BTreeMap::from([(
        "fixture".to_string(),
        gaze_mcp_bridge::config::ServerSpec {
            command: dir.path().join("missing-child").display().to_string(),
            args: Vec::new(),
            env: BTreeMap::new(),
            cwd: None,
        },
    )]);
    for mode in [SessionMode::Ephemeral, SessionMode::File] {
        let config = BridgeConfig {
            session: SessionCfg {
                mode,
                dir: None,
                key_env: None,
                max_sessions: 0,
            },
            limits: Default::default(),
            servers: servers.clone(),
            policy: Default::default(),
        };
        let audit = Arc::new(FileBridgeAuditSink::new(dir.path().join("audit.jsonl")));
        assert!(matches!(
            BridgeHost::from_config(config, audit).await,
            Err(BridgeError::Config(message))
                if message == "session.max_sessions must be greater than 0"
        ));
    }
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
}
