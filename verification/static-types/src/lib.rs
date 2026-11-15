#![feature(rustc_private)]
extern crate rustc_errors;
extern crate rustc_hir;
extern crate rustc_middle;
extern crate rustc_span;

use rustc_hir::{Body, Expr, FnDecl, def_id::LocalDefId};
use rustc_lint::{LateContext, LateLintPass, LintContext};
use rustc_middle::ty::{self, Ty};
use rustc_span::Span;

dylint_linting::declare_late_lint! {
    pub STATIC_TYPES,
    Forbid,
    "authored Rust must use static dispatch and concrete ownership"
}

fn check(cx: &LateContext<'_>, value: Ty<'_>, span: Span) {
    if span.from_expansion()
        && !span
            .ctxt()
            .outer_expn_data()
            .macro_def_id
            .is_some_and(|id| id.is_local())
    {
        return;
    }
    for argument in value.walk() {
        let Some(value) = argument.as_type() else {
            continue;
        };
        let forbidden = match value.kind() {
            ty::Dynamic(..) => true,
            ty::Adt(definition, _) => matches!(
                cx.tcx.def_path_str(definition.did()).as_str(),
                "alloc::boxed::Box"
                    | "std::boxed::Box"
                    | "alloc::sync::Arc"
                    | "std::sync::Arc"
                    | "alloc::rc::Rc"
                    | "std::rc::Rc"
                    | "core::cell::RefCell"
                    | "std::cell::RefCell"
                    | "core::cell::Cell"
                    | "std::cell::Cell"
                    | "std::sync::poison::mutex::Mutex"
                    | "std::sync::poison::rwlock::RwLock"
                    | "tokio::sync::mutex::Mutex"
                    | "tokio::sync::rwlock::RwLock"
            ),
            _ => false,
        };
        if forbidden {
            cx.emit_span_lint(
                STATIC_TYPES,
                span,
                rustc_errors::DiagDecorator(|diagnostic| {
                    diagnostic.primary_message(
                        "static_types: dynamic dispatch or shared ownership is prohibited",
                    );
                    diagnostic
                        .note("use concrete ownership, borrowing, generics, or an exhaustive enum");
                }),
            );
            return;
        }
    }
}

impl<'tcx> LateLintPass<'tcx> for StaticTypes {
    fn check_item(&mut self, cx: &LateContext<'tcx>, item: &'tcx rustc_hir::Item<'tcx>) {
        if matches!(
            item.kind,
            rustc_hir::ItemKind::TyAlias(..)
                | rustc_hir::ItemKind::Static(..)
                | rustc_hir::ItemKind::Const(..)
        ) {
            check(
                cx,
                cx.tcx
                    .type_of(item.owner_id)
                    .instantiate_identity()
                    .skip_norm_wip(),
                item.span,
            );
        }
    }

    fn check_trait_item(&mut self, cx: &LateContext<'tcx>, item: &'tcx rustc_hir::TraitItem<'tcx>) {
        if matches!(item.kind, rustc_hir::TraitItemKind::Fn(..)) {
            let signature = cx
                .tcx
                .fn_sig(item.owner_id)
                .instantiate_identity()
                .skip_binder();
            for value in signature.inputs_and_output {
                check(cx, value, item.span);
            }
        }
    }

    fn check_impl_item(&mut self, cx: &LateContext<'tcx>, item: &'tcx rustc_hir::ImplItem<'tcx>) {
        if matches!(item.kind, rustc_hir::ImplItemKind::Type(..)) {
            check(
                cx,
                cx.tcx
                    .type_of(item.owner_id)
                    .instantiate_identity()
                    .skip_norm_wip(),
                item.span,
            );
        }
    }
    fn check_expr(&mut self, cx: &LateContext<'tcx>, expression: &'tcx Expr<'tcx>) {
        check(cx, cx.typeck_results().expr_ty(expression), expression.span);
    }

    fn check_pat(&mut self, cx: &LateContext<'tcx>, pattern: &'tcx rustc_hir::Pat<'tcx>) {
        check(cx, cx.typeck_results().pat_ty(pattern), pattern.span);
    }

    fn check_fn(
        &mut self,
        cx: &LateContext<'tcx>,
        _: rustc_hir::intravisit::FnKind<'tcx>,
        _: &'tcx FnDecl<'tcx>,
        _: &'tcx Body<'tcx>,
        span: Span,
        id: LocalDefId,
    ) {
        if cx.tcx.def_kind(id) == rustc_hir::def::DefKind::Closure {
            return;
        }
        let signature = cx.tcx.fn_sig(id).instantiate_identity().skip_binder();
        for value in signature.inputs_and_output {
            check(cx, value, span);
        }
    }

    fn check_field_def(&mut self, cx: &LateContext<'tcx>, field: &'tcx rustc_hir::FieldDef<'tcx>) {
        check(
            cx,
            cx.tcx
                .type_of(field.def_id)
                .instantiate_identity()
                .skip_norm_wip(),
            field.span,
        );
    }
}
