// Verifies that `ToolCtx::new` is `pub(crate)` — external crates cannot call
// it. This is the primary chokepoint guarantee: no fabrication of contexts
// from outside the dispatcher.

use serde_json::Value;
use ulid::Ulid;

use gaze_mcp_core::ctx::{SessionHandle, ToolCtx};

fn try_to_fabricate() {
    let _session = SessionHandle::new("audit-id");
    let _ctx = ToolCtx::new(_session, Value::Null, Ulid::new(), "tool", "principal");
}

fn main() {}
