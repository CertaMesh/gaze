use gaze_mcp_bridge::config::{SessionCfg, SessionMode};
use gaze_mcp_bridge::{BridgeError, BridgeSessionStore};

#[tokio::test]
async fn public_constructor_rejects_zero_session_cap() {
    let config = SessionCfg {
        mode: SessionMode::Ephemeral,
        dir: None,
        key_env: None,
        max_sessions: 0,
    };

    match BridgeSessionStore::from_config(&config) {
        Err(BridgeError::Config(_)) => {}
        Err(_) => panic!("zero cap must return a typed configuration error"),
        Ok(store) => {
            // A bounded demonstration of the disabled cap, without exhausting memory.
            for id in [
                "01HRT7K6P6X5Q9M0V8YQ4N7T01",
                "01HRT7K6P6X5Q9M0V8YQ4N7T02",
                "01HRT7K6P6X5Q9M0V8YQ4N7T03",
            ] {
                drop(
                    store
                        .get(id)
                        .await
                        .expect("zero cap admits another session"),
                );
            }
            let admitted = store.len().await;
            assert_eq!(admitted, 3);
            panic!("public constructor accepted max_sessions=0 and admitted {admitted} sessions");
        }
    }
}
