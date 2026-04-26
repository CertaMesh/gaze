#![feature(rustc_private)]
#![warn(unused_extern_crates)]

extern crate rustc_hir;
extern crate rustc_span;

use rustc_lint::{LateContext, LateLintPass};

dylint_linting::impl_late_lint! {
    pub GAZE_MODULE_ISOLATION,
    Deny,
    "forbids audit sink symbols inside restore/core protected paths",
    GazeModuleIsolation::new()
}

#[derive(serde::Deserialize)]
struct Config {
    protected_paths: Vec<String>,
    forbidden_crates: Vec<String>,
    forbidden_items: Vec<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            protected_paths: vec![
                "crates/gaze-cli/src/restore".to_owned(),
                "crates/gaze/src".to_owned(),
            ],
            forbidden_crates: vec!["gaze_audit".to_owned()],
            forbidden_items: vec![
                "gaze_audit::SqliteLogger".to_owned(),
                "gaze_audit::AuditFilter".to_owned(),
                "gaze_audit::AuditLogRow".to_owned(),
                "gaze_audit::build_audit_query_sql".to_owned(),
                "gaze_audit::AUDIT_RESTRICTED_COLUMNS".to_owned(),
            ],
        }
    }
}

struct GazeModuleIsolation {
    config: Config,
}

impl GazeModuleIsolation {
    fn new() -> Self {
        Self {
            config: dylint_linting::config_or_default(env!("CARGO_PKG_NAME")),
        }
    }
}

impl<'tcx> LateLintPass<'tcx> for GazeModuleIsolation {
    fn check_item(&mut self, _cx: &LateContext<'tcx>, _item: &'tcx rustc_hir::Item<'tcx>) {}
}
