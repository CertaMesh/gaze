//! Behavioral proof that the agent-tier and operator-tier surfaces of this
//! crate stay partitioned by Cargo features.
//!
//! ## Where the boundary is actually enforced
//!
//! By rustc, via two independent `cfg` gates:
//!
//! - `#[cfg(feature = "operator-tier")]` on `tools::{export, restore,
//!   restore_strict}` in `src/tools/mod.rs`. `pub mod tools` itself is
//!   ungated, so these three attributes are the whole gate on the deep path.
//! - `#[cfg(feature = "operator-tier")]` on the `operator_tools` re-export
//!   module in `src/lib.rs`.
//!
//! ## What this file does
//!
//! It is the check that the enforcement above *keeps* holding. The
//! `operator_tools_*_present_*` tests pin the tier tagging of each surface
//! when its feature is on. The compile-fail driver pins the inverse: an
//! agent-tier build must not be able to *name* the operator surface at all.
//!
//! That inverse cannot be written as a runtime assertion — "this path does not
//! resolve" is a statement about compilation, so it is asserted the only way
//! the language allows, with compile-fail fixtures. `trybuild` builds each
//! fixture as a real external crate depending on `gaze-mcp-core` with
//! `default-features = false` plus exactly the features this test binary was
//! built with, so the fixtures see the same surface an agent-tier adopter
//! sees, and rustc itself is the enforcer. Un-gate any of the four surfaces
//! above and the corresponding fixture starts compiling, which fails this test
//! by name.

#[cfg(not(feature = "operator-tier"))]
#[path = "support/trybuild_guard.rs"]
mod trybuild_guard;

#[cfg(feature = "core-tools")]
#[test]
fn core_tools_module_is_present_with_core_tools_feature() {
    use gaze_mcp_core::core_tools::{CleanTool, SafetyNetCheckTool, TokenizeFieldTool};
    use gaze_mcp_core::tool::{Tool, ToolTier};

    assert_eq!(CleanTool::new().descriptor().tier(), ToolTier::Agent);
    assert_eq!(
        TokenizeFieldTool::new().descriptor().tier(),
        ToolTier::Agent
    );
    assert_eq!(
        SafetyNetCheckTool::new().descriptor().tier(),
        ToolTier::Agent
    );
}

#[cfg(feature = "operator-tier")]
#[test]
fn operator_tools_module_is_present_with_operator_tier_feature() {
    use gaze_mcp_core::operator_tools::{ExportSessionTokensTool, RestoreStrictTool, RestoreTool};
    use gaze_mcp_core::tool::{Tool, ToolTier};

    assert_eq!(RestoreTool::new().descriptor().tier(), ToolTier::Operator);
    assert_eq!(
        RestoreStrictTool::new().descriptor().tier(),
        ToolTier::Operator
    );
    assert_eq!(
        ExportSessionTokensTool::new().descriptor().tier(),
        ToolTier::Operator
    );
}

/// Directory holding the agent-tier compile-fail fixtures. Kept out of the
/// `tests/ui/*.rs` glob that `tool_ctx_seal.rs` drives, because these fixtures
/// are only meaningful when `operator-tier` is off.
#[cfg(not(feature = "operator-tier"))]
const AGENT_TIER_FIXTURE_DIR: &str = "tests/ui/tier";

/// One fixture per gated operator surface. Listed explicitly rather than left
/// to the glob: `trybuild` reports success for a glob that matches nothing, so
/// an inventory that is only globbed would go quiet the moment a fixture is
/// deleted — the exact failure mode this test exists to prevent.
#[cfg(not(feature = "operator-tier"))]
const AGENT_TIER_FIXTURES: &[&str] = &[
    "agent_tier_must_not_reach_export_session_tokens.rs",
    "agent_tier_must_not_reach_operator_tools_reexport.rs",
    "agent_tier_must_not_reach_restore.rs",
    "agent_tier_must_not_reach_restore_strict.rs",
];

#[cfg(not(feature = "operator-tier"))]
fn assert_agent_tier_fixture_inventory() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(AGENT_TIER_FIXTURE_DIR);
    let entries = std::fs::read_dir(&dir).unwrap_or_else(|error| {
        panic!(
            "tier isolation: cannot read the compile-fail fixture directory {}: {error}",
            dir.display()
        )
    });
    let mut found = entries
        .map(|entry| {
            entry
                .unwrap_or_else(|error| {
                    panic!(
                        "tier isolation: unreadable entry in {}: {error}",
                        dir.display()
                    )
                })
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .filter(|name| name.ends_with(".rs"))
        .collect::<Vec<_>>();
    found.sort();
    let expected = AGENT_TIER_FIXTURES
        .iter()
        .map(|name| (*name).to_owned())
        .collect::<Vec<_>>();
    assert_eq!(
        found,
        expected,
        "tier isolation: the agent-tier compile-fail fixture inventory in {} drifted. \
         Every gated operator surface needs a fixture proving an agent-tier build \
         cannot name it; a fixture removed without removing the surface leaves that \
         surface unguarded.",
        dir.display()
    );
}

/// The operator-tier surface must be unreachable — not merely unused — from an
/// agent-tier build.
///
/// Each fixture in `tests/ui/tier/` names one gated operator surface and must
/// fail to compile. If a `#[cfg(feature = "operator-tier")]` gate is removed,
/// that fixture compiles and `trybuild` fails with
/// `expected test case to fail to compile, but it succeeded`, naming the
/// surface in the fixture filename.
#[cfg(not(feature = "operator-tier"))]
#[test]
fn operator_tier_surface_is_unreachable_from_an_agent_tier_build() {
    trybuild_guard::assert_trybuild_rustc_matches_workspace();
    assert_agent_tier_fixture_inventory();
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/ui/tier/*.rs");
}
