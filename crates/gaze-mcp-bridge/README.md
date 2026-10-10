# gaze-mcp-bridge

`gaze-mcp-bridge = "0.16.0"` is the optional policy-gated MCP bridge for Gaze.

Fails closed: agents see only pseudonymous tokens,
downstream MCP tools receive restored PII only for explicitly allowed argument
fields, and downstream results are redacted before returning to the agent.
