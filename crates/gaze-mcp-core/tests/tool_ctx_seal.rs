//! Compile-fail fixtures verifying the [`gaze_mcp_core::ToolCtx`] seal:
//! external crates must NOT be able to construct a `ToolCtx` via either
//! [`gaze_mcp_core::ctx::ToolCtx::new`] (private constructor) or a struct
//! literal (private fields + `#[non_exhaustive]`).
//!
//! The fixtures live in `tests/ui/`; this driver runs them via trybuild.

#[test]
fn external_construction_is_compile_fail() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/ui/tool_ctx_*.rs");
}
