//! Adversarial edges for the `dashboard-isolation` gate: every check must
//! fail RED on a seeded violation and stay green only on the exact reviewed
//! boundary shape.

#![allow(dead_code)]

#[path = "../src/dashboard_isolation.rs"]
mod dashboard_isolation;

use dashboard_isolation::{
    parse_dashboard_edges, verify_dashboard_gaze_edges, verify_default_cli_graph, DashboardEdges,
};

fn metadata_json(dependencies: &str, member: bool) -> String {
    let members = if member {
        r#"["path+file:///ws/crates/gaze-proxy-dashboard#gaze-proxy-dashboard@0.12.0"]"#
    } else {
        "[]"
    };
    format!(
        r#"{{
  "packages": [
    {{
      "id": "path+file:///ws/crates/gaze-proxy-dashboard#gaze-proxy-dashboard@0.12.0",
      "name": "gaze-proxy-dashboard",
      "dependencies": [{dependencies}]
    }}
  ],
  "workspace_members": {members}
}}"#
    )
}

fn edges(normal: &[&str], non_normal: &[&str]) -> DashboardEdges {
    DashboardEdges {
        normal_gaze: normal.iter().map(|name| (*name).to_owned()).collect(),
        non_normal_gaze: non_normal.iter().map(|name| (*name).to_owned()).collect(),
    }
}

#[test]
fn exact_two_crate_allowlist_passes() {
    verify_dashboard_gaze_edges(&edges(&["gaze-inspection", "gaze-types"], &[]))
        .expect("reviewed edge set must pass");
    verify_dashboard_gaze_edges(&edges(&["gaze-types", "gaze-inspection"], &[]))
        .expect("edge order must not matter");
}

#[test]
fn seeded_gaze_proxy_edge_fails_red() {
    let error = verify_dashboard_gaze_edges(&edges(
        &["gaze-inspection", "gaze-types", "gaze-proxy"],
        &[],
    ))
    .expect_err("gaze-proxy edge must fail");
    assert!(error.to_string().contains("gaze-proxy"));
}

#[test]
fn seeded_core_crate_edge_fails_red() {
    verify_dashboard_gaze_edges(&edges(&["gaze-inspection", "gaze-types", "gaze"], &[]))
        .expect_err("core crate edge must fail");
}

#[test]
fn missing_inspection_edge_fails_red() {
    verify_dashboard_gaze_edges(&edges(&["gaze-types"], &[]))
        .expect_err("missing gaze-inspection edge must fail");
}

#[test]
fn missing_types_edge_fails_red() {
    verify_dashboard_gaze_edges(&edges(&["gaze-inspection"], &[]))
        .expect_err("missing gaze-types edge must fail");
}

#[test]
fn empty_edge_set_fails_red() {
    verify_dashboard_gaze_edges(&edges(&[], &[])).expect_err("empty edge set must fail");
}

#[test]
fn seeded_dev_dependency_gaze_edge_fails_red() {
    let error =
        verify_dashboard_gaze_edges(&edges(&["gaze-inspection", "gaze-types"], &["gaze-proxy"]))
            .expect_err("dev/build Gaze edge must fail");
    assert!(error.to_string().contains("dev/build"));
}

#[test]
fn parse_extracts_normal_and_dev_kinds() {
    let json = metadata_json(
        r#"{"name": "gaze-types", "kind": null},
           {"name": "gaze-inspection", "kind": "normal"},
           {"name": "gaze-proxy", "kind": "dev"},
           {"name": "zeroize", "kind": null}"#,
        true,
    );
    let edges = parse_dashboard_edges(&json).expect("parse");
    assert_eq!(edges.normal_gaze, vec!["gaze-types", "gaze-inspection"]);
    assert_eq!(edges.non_normal_gaze, vec!["gaze-proxy"]);
}

#[test]
fn missing_workspace_package_fails_red_not_silent() {
    let json = metadata_json(r#"{"name": "gaze-types", "kind": null}"#, false);
    let error = parse_dashboard_edges(&json).expect_err("absent workspace member must fail");
    assert!(error.to_string().contains("gaze-proxy-dashboard"));
}

#[test]
fn malformed_metadata_fails_red() {
    parse_dashboard_edges("not json").expect_err("malformed metadata must fail");
}

#[test]
fn default_graph_containing_dashboard_fails_red() {
    let error = verify_default_cli_graph(
        "gaze-cli v0.12.0\n├── gaze-proxy v0.12.0\n├── gaze-proxy-dashboard v0.12.0\n",
    )
    .expect_err("dashboard in default graph must fail");
    assert!(error.to_string().contains("default-off"));
}

#[test]
fn default_graph_without_dashboard_passes() {
    verify_default_cli_graph("gaze-cli v0.12.0\n├── gaze-proxy v0.12.0\n├── gaze-pii v0.12.0\n")
        .expect("dashboard-free default graph must pass");
}
