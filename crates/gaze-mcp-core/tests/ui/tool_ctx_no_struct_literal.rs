// Verifies that `ToolCtx`'s fields are `pub(crate)` — external crates
// cannot construct one via struct literal either, even with all the field
// values in hand.

use std::marker::PhantomData;

use serde_json::Value;
use ulid::Ulid;

use gaze_mcp_core::ctx::{SessionHandle, ToolCtx};

fn try_to_fabricate() {
    let _session = SessionHandle::new("audit-id");
    let _ctx = ToolCtx {
        session: _session,
        redacted_args: Value::Null,
        call_id: Ulid::new(),
        tool_name: "tool",
        principal_id: "principal",
        _life: PhantomData,
    };
}

fn main() {}
