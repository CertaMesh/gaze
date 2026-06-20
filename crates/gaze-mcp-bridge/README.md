# gaze-mcp-bridge

`gaze-mcp-bridge = "0.10.1"` is the optional policy-gated MCP bridge for Gaze.

The bridge is intentionally fail-closed: agents see only pseudonymous tokens,
downstream MCP tools receive restored PII only for explicitly allowed argument
fields, and downstream results are redacted before returning to the agent.
