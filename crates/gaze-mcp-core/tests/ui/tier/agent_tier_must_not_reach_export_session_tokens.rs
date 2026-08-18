// Tier boundary: `tools::export` is gated by
// `#[cfg(feature = "operator-tier")]` in `src/tools/mod.rs`, while
// `pub mod tools` itself is ungated in `lib.rs`. Compiled as an external crate
// against an agent-tier feature graph, naming the module must not resolve.
use gaze_mcp_core::tools::export::ExportSessionTokensTool;

fn main() {}
