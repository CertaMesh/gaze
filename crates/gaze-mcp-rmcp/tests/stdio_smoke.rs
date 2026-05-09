use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use async_trait::async_trait;
use gaze_mcp_core::{DispatchError, DispatchHost, Principal, ToolDescriptor, ToolResponse};
use gaze_mcp_rmcp::{FixedPrincipalResolver, RmcpFrontend};
use rmcp::ServiceExt;
use rmcp::model::CallToolRequestParam;
use serde_json::json;
use tokio::io::duplex;

struct SmokeHost {
    calls: AtomicUsize,
}

#[async_trait]
impl DispatchHost for SmokeHost {
    async fn dispatch(
        &self,
        principal: &Principal,
        tool_name: &str,
        raw_args: serde_json::Value,
        external_session_id: Option<&str>,
    ) -> Result<ToolResponse, DispatchError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        assert_eq!(principal.id, "stdio-test");
        assert_eq!(tool_name, "clean");
        assert_eq!(external_session_id, Some("01ARZ3NDEKTSV4RRFFQ69G5FAV"));
        assert_eq!(raw_args, json!({ "text": "hello" }));
        Ok(ToolResponse::text("redacted: hello"))
    }

    fn list_tools(&self) -> Vec<ToolDescriptor> {
        vec![
            ToolDescriptor::agent(
                "clean",
                json!({
                    "type": "object",
                    "properties": {
                        "text": { "type": "string" }
                    }
                }),
            )
            .with_description("Clean text through Gaze"),
        ]
    }
}

#[tokio::test]
async fn stdio_dispatch_lists_and_calls_tools() {
    let host = Arc::new(SmokeHost {
        calls: AtomicUsize::new(0),
    });
    let frontend = RmcpFrontend::stdio(Arc::new(FixedPrincipalResolver::agent("stdio-test")));
    let handler = frontend.into_server_handler(host.clone());
    let (client_stream, server_stream) = duplex(16 * 1024);

    let server_task = tokio::spawn(async move {
        let running = rmcp::serve_server(handler, server_stream)
            .await
            .expect("server initializes");
        running.waiting().await.expect("server task joins")
    });

    let client = ().serve(client_stream).await.expect("client initializes");
    let tools = client
        .list_tools(Default::default())
        .await
        .expect("list tools");
    assert_eq!(tools.tools.len(), 1);
    assert_eq!(tools.tools[0].name, "clean");

    let result = client
        .call_tool(CallToolRequestParam {
            name: "clean".into(),
            arguments: json!({
                "text": "hello",
                "_session_id": "01ARZ3NDEKTSV4RRFFQ69G5FAV"
            })
            .as_object()
            .cloned(),
        })
        .await
        .expect("tool call succeeds");

    assert_eq!(result.is_error, Some(false));
    assert_eq!(
        result.content[0].raw.as_text().unwrap().text,
        "redacted: hello"
    );
    assert_eq!(host.calls.load(Ordering::SeqCst), 1);

    client.cancel().await.expect("client cancels");
    server_task.await.expect("server finishes");
}
