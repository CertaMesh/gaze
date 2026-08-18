// Tier boundary: the `operator_tools` re-export module is gated by
// `#[cfg(feature = "operator-tier")]` in `lib.rs`. This is the second,
// independent gate on the operator surface — un-gating either it or the
// `tools::*` modules it re-exports is a tier violation, so both are probed.
use gaze_mcp_core::operator_tools::RestoreTool;

fn main() {}
