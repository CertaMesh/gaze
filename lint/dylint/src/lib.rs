#![feature(rustc_private)]
#![warn(unused_extern_crates)]

extern crate rustc_hir;
extern crate rustc_span;

use std::collections::HashSet;

mod default_config;

use clippy_utils::diagnostics::span_lint_and_then;
use rustc_hir::def::Res;
use rustc_hir::def_id::{DefId, CRATE_DEF_INDEX};
use rustc_hir::{
    AmbigArg, Expr, ExprKind, HirId, ImplItem, Item, ItemKind, Node, Path, PolyTraitRef, QPath, Ty,
    TyKind,
};
use rustc_lint::{LateContext, LateLintPass, LintContext};
use rustc_span::Span;

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
            protected_paths: default_config::PROTECTED_PATHS
                .iter()
                .map(|path| (*path).to_owned())
                .collect(),
            forbidden_crates: default_config::FORBIDDEN_CRATES
                .iter()
                .map(|name| (*name).to_owned())
                .collect(),
            forbidden_items: default_config::FORBIDDEN_ITEMS
                .iter()
                .map(|item| (*item).to_owned())
                .collect(),
        }
    }
}

struct GazeModuleIsolation {
    config: Config,
    reported: HashSet<(String, String)>,
}

impl GazeModuleIsolation {
    fn new() -> Self {
        Self {
            config: dylint_linting::config_or_default(env!("CARGO_PKG_NAME")),
            reported: HashSet::new(),
        }
    }
}

impl<'tcx> LateLintPass<'tcx> for GazeModuleIsolation {
    fn check_item(&mut self, cx: &LateContext<'tcx>, item: &'tcx Item<'tcx>) {
        match item.kind {
            ItemKind::Use(path, _) => {
                self.check_res(cx, path.res.type_ns, item.span, Some(item.hir_id()), "use item");
                self.check_res(cx, path.res.value_ns, item.span, Some(item.hir_id()), "use item");
            }
            ItemKind::ExternCrate(..) => {
                if let Some(crate_num) = cx.tcx.extern_mod_stmt_cnum(item.owner_id.def_id) {
                    let def_id = DefId {
                        krate: crate_num,
                        index: CRATE_DEF_INDEX,
                    };
                    self.check_def_id(
                        cx,
                        def_id,
                        item.span,
                        Some(item.hir_id()),
                        "extern crate item",
                    );
                }
            }
            _ => {}
        }
    }

    fn check_expr(&mut self, cx: &LateContext<'tcx>, expr: &'tcx Expr<'tcx>) {
        match expr.kind {
            ExprKind::Path(qpath) => {
                let res = cx.qpath_res(&qpath, expr.hir_id);
                self.check_res(
                    cx,
                    Some(res),
                    expr.span,
                    Some(expr.hir_id),
                    "expression path",
                );
            }
            ExprKind::Struct(qpath, ..) => {
                let res = cx.qpath_res(qpath, expr.hir_id);
                self.check_res(
                    cx,
                    Some(res),
                    expr.span,
                    Some(expr.hir_id),
                    "struct expression path",
                );
            }
            ExprKind::MethodCall(_, _, _, span) => {
                if let Some(def_id) = cx.typeck_results().type_dependent_def_id(expr.hir_id) {
                    self.check_def_id(
                        cx,
                        def_id,
                        span,
                        Some(expr.hir_id),
                        "method call path",
                    );
                }
            }
            _ => {}
        }
    }

    fn check_ty(&mut self, cx: &LateContext<'tcx>, ty: &'tcx Ty<'tcx, AmbigArg>) {
        if let TyKind::Path(qpath) = ty.kind {
            self.check_qpath(cx, qpath, ty.hir_id, ty.span, "type path");
        }
    }

    fn check_poly_trait_ref(
        &mut self,
        cx: &LateContext<'tcx>,
        trait_ref: &'tcx PolyTraitRef<'tcx>,
    ) {
        self.check_res(
            cx,
            Some(trait_ref.trait_ref.path.res),
            trait_ref.span,
            Some(trait_ref.trait_ref.hir_ref_id),
            "trait bound path",
        );
    }

    fn check_path(&mut self, cx: &LateContext<'tcx>, path: &Path<'tcx>, _hir_id: HirId) {
        self.check_res(cx, Some(path.res), path.span, Some(_hir_id), "HIR path");
    }
}

impl GazeModuleIsolation {
    fn check_qpath(
        &mut self,
        cx: &LateContext<'_>,
        qpath: QPath<'_>,
        hir_id: HirId,
        span: Span,
        usage: &'static str,
    ) {
        let res = cx.qpath_res(&qpath, hir_id);
        self.check_res(cx, Some(res), span, Some(hir_id), usage);
    }

    fn check_res(
        &mut self,
        cx: &LateContext<'_>,
        res: Option<Res>,
        span: Span,
        hir_id: Option<HirId>,
        usage: &'static str,
    ) {
        let Some(Res::Def(_, def_id)) = res else {
            return;
        };
        self.check_def_id(cx, def_id, span, hir_id, usage);
    }

    fn check_def_id(
        &mut self,
        cx: &LateContext<'_>,
        def_id: DefId,
        span: Span,
        hir_id: Option<HirId>,
        usage: &'static str,
    ) {
        let Some(protected_path) = self.protected_context(cx, span, hir_id) else {
            return;
        };

        let crate_name = cx.tcx.crate_name(def_id.krate).to_string();
        let resolved_item = cx.tcx.def_path_str(def_id);
        let reason = if self
            .config
            .forbidden_crates
            .iter()
            .any(|name| name == &crate_name)
        {
            format!("crate `{crate_name}` is forbidden in protected paths")
        } else if self
            .config
            .forbidden_items
            .iter()
            .any(|item| item == &resolved_item)
        {
            format!("item `{resolved_item}` is forbidden in protected paths")
        } else {
            return;
        };

        let key = (span_debug_key(span), resolved_item.clone());
        if !self.reported.insert(key) {
            return;
        }

        let source_span = source_span_note(cx, span);
        span_lint_and_then(
            cx,
            GAZE_MODULE_ISOLATION,
            span,
            format!("forbidden audit dependency `{crate_name}` used inside protected path"),
            |diag| {
                diag.note(format!("protected path: {protected_path}"));
                diag.note(format!("source span: {source_span}"));
                diag.note(format!("resolved DefId: {def_id:?}"));
                diag.note(format!("resolved item: {resolved_item}"));
                diag.note(format!("why forbidden: {reason}"));
                diag.note(format!("usage: {usage}"));
            },
        );
    }

    fn protected_context(
        &self,
        cx: &LateContext<'_>,
        span: Span,
        hir_id: Option<HirId>,
    ) -> Option<String> {
        self.protected_context_for_span(cx, span)
            .or_else(|| hir_id.and_then(|hir_id| self.protected_context_for_hir_id(cx, hir_id)))
    }

    fn protected_context_for_span(&self, cx: &LateContext<'_>, span: Span) -> Option<String> {
        let mut cursor = Some(span);
        while let Some(current) = cursor {
            if let Some(protected_path) = self.protected_path_for_direct_span(cx, current) {
                return Some(protected_path);
            }
            let callsite = current.source_callsite();
            if callsite != current {
                if let Some(protected_path) = self.protected_path_for_direct_span(cx, callsite) {
                    return Some(protected_path);
                }
            }
            cursor = current.parent_callsite();
        }
        None
    }

    fn protected_context_for_hir_id(&self, cx: &LateContext<'_>, hir_id: HirId) -> Option<String> {
        for (_, node) in cx.tcx.hir_parent_iter(hir_id) {
            let span = match node {
                Node::Item(item) => Some(item.span),
                Node::ImplItem(ImplItem { span, .. }) => Some(*span),
                Node::TraitItem(item) => Some(item.span),
                Node::ForeignItem(item) => Some(item.span),
                Node::Variant(variant) => Some(variant.span),
                Node::Field(field) => Some(field.span),
                Node::Expr(expr) => Some(expr.span),
                Node::Ty(ty) => Some(ty.span),
                Node::Block(block) => Some(block.span),
                Node::LetStmt(local) => Some(local.span),
                Node::Stmt(stmt) => Some(stmt.span),
                Node::Crate(module) => Some(module.spans.inner_span),
                Node::Err(span) => Some(span),
                _ => None,
            };
            if let Some(span) = span
                && let Some(protected_path) = self.protected_context_for_span(cx, span)
            {
                return Some(protected_path);
            }
        }
        None
    }

    fn protected_path_for_direct_span(
        &self,
        cx: &LateContext<'_>,
        span: Span,
    ) -> Option<String> {
        let source_map = cx.sess().source_map();
        let filename = source_map.span_to_filename(span);
        let filename = filename.prefer_local().to_string();

        self.config
            .protected_paths
            .iter()
            .find(|protected_path| filename.contains(protected_path.as_str()))
            .cloned()
    }
}

fn source_span_note(cx: &LateContext<'_>, span: Span) -> String {
    let source_map = cx.sess().source_map();
    let start = source_map.lookup_char_pos(span.lo());
    let end = source_map.lookup_char_pos(span.hi());
    format!(
        "{}:{}:{}-{}:{}",
        start.file.name.prefer_local(),
        start.line,
        start.col_display + 1,
        end.line,
        end.col_display + 1
    )
}

fn span_debug_key(span: Span) -> String {
    let data = span.data();
    format!("{}:{}:{:?}", data.lo.0, data.hi.0, data.ctxt)
}
