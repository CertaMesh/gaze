// Verifies that `ToolCtx::new_with_resources` is `pub(crate)` — external crates
// cannot call it. This is the primary chokepoint guarantee: no fabrication of
// contexts from outside the dispatcher.

use serde_json::Value;
use ulid::Ulid;

use gaze_mcp_core::ctx::{SessionHandle, ToolCtx, ToolResources};

fn fake_session<'a>() -> SessionHandle<'a> {
    unimplemented!()
}

fn fake_resources<'a>() -> ToolResources<'a> {
    unimplemented!()
}

fn try_to_fabricate() {
    let _ctx = ToolCtx::new_with_resources(
        fake_session(),
        fake_resources(),
        Value::Null,
        Ulid::new(),
        "tool",
        "principal",
    );
}

fn main() {}
