// Tier boundary: `tools::restore` is gated by `#[cfg(feature = "operator-tier")]`
// in `src/tools/mod.rs`, while `pub mod tools` itself is ungated in `lib.rs`.
// This fixture is compiled as an external crate against an agent-tier feature
// graph, so naming the module must not resolve. If this file ever compiles,
// the operator-tier restore surface is reachable from an agent-tier build.
use gaze_mcp_core::tools::restore::RestoreTool;

fn main() {}
