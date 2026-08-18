//! Compile-fail fixtures verifying the [`gaze_mcp_core::ToolCtx`] seal:
//! external crates must NOT be able to construct a `ToolCtx` via either
//! [`gaze_mcp_core::ctx::ToolCtx::new_with_resources`] (private constructor) or a struct
//! literal (private fields + `#[non_exhaustive]`).
//!
//! The fixtures live in `tests/ui/`; this driver runs them via trybuild.
//! `tests/ui/tier/` is deliberately NOT matched here — those fixtures belong to
//! the feature-gated tier driver in `tier_isolation.rs`.

#[path = "support/trybuild_guard.rs"]
mod trybuild_guard;

use trybuild_guard::assert_trybuild_rustc_matches_workspace;

#[test]
fn external_construction_is_compile_fail() {
    assert_trybuild_rustc_matches_workspace();
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/ui/*.rs");
}
