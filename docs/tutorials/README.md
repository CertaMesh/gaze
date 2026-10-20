# Tutorials

CI [executes these tutorials](../reference/tutorial-gate.md) and compares their
documented output. Account-bound steps are marked for manual replay before release.

- [Your first pseudonymization](first-pseudonymization.md) takes about five
  minutes with no model download: clean a fictional email, inspect its token,
  restore the exact original bytes, and restore a simulated model reply.
- [Protect a model call from Rust](getting-started.md) builds a complete mock
  model loop with a private session snapshot and strict restore. About ten minutes.
- [Protect a coding assistant's document read](mcp-coding-assistant.md) connects
  Claude Code to Gaze's MCP tool and checks its tokenized result. About ten minutes.
- [Route an SDK call through the proxy](proxy-sdk.md) sends an OpenAI SDK request
  to a local mock and proves owner-side restore. About fifteen minutes.
- [Reuse tokens across daemon turns](daemon-conversation.md) checks token reuse
  within one conversation and isolation from another. About ten minutes.
- [Protect an order ID](custom-order-id.md) adds a custom regex and checks a
  byte-exact round trip. About ten minutes.
- [Clean a scanned document](scanned-document.md) turns a fictional invoice
  image into a SafeBundle with separate agent and owner files. About fifteen minutes.

For specific tasks, use the [how-to guides](../how-to/README.md).
For exact behavior, use the [reference](../reference/README.md).
For design reasons, use [explanation](../explanation/README.md).
