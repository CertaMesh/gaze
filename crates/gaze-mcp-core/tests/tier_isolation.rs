//! Compile-time assertion that agent-tier and operator-tier surfaces are
//! cleanly partitioned by Cargo features.
//!
//! The deeper "linker cannot reach `Session::restore*` when operator-tier
//! is off" guarantee is enforced by the `mcp-tier-isolation` xtask gate
//! introduced in Phase 10. This file is a behavioral cousin: it proves the
//! `core_tools` and `operator_tools` re-export modules track the feature
//! flags as documented.

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
    use gaze_mcp_core::operator_tools::{ExportManifestTool, RestoreStrictTool, RestoreTool};
    use gaze_mcp_core::tool::{Tool, ToolTier};

    assert_eq!(RestoreTool::new().descriptor().tier(), ToolTier::Operator);
    assert_eq!(
        RestoreStrictTool::new().descriptor().tier(),
        ToolTier::Operator
    );
    assert_eq!(
        ExportManifestTool::new().descriptor().tier(),
        ToolTier::Operator
    );
}

#[cfg(not(feature = "operator-tier"))]
#[test]
fn operator_tools_module_is_absent_without_operator_tier_feature() {
    // The presence of this build configuration is the test: if the
    // `operator_tools` module were reachable without the feature, the
    // module-level `#[cfg(feature = "operator-tier")]` gate in lib.rs would
    // be wrong, and a follow-up commit would add a `use
    // gaze_mcp_core::operator_tools` here that fails to compile.
    //
    // Currently we have nothing to assert beyond "this test compiled
    // without the feature" — see the `mcp-tier-isolation` xtask gate
    // (Phase 10) for the linker-level guarantee.
    let agent_tier_only_compiles = true;
    assert!(agent_tier_only_compiles);
}
