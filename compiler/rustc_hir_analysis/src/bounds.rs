//! Bounds are restrictions applied to some types after they've been lowered from the HIR to the
//! [`rustc_middle::ty`] form.

use rustc_data_structures::fx::FxIndexMap;
use rustc_hir::def::DefKind;
use rustc_hir::LangItem;
use rustc_middle::ty::fold::FnMutDelegate;
use rustc_middle::ty::{self, Ty, TyCtxt, Upcast};
use rustc_span::def_id::DefId;
use rustc_span::Span;

/// Collects together a list of type bounds. These lists of bounds occur in many places
/// in Rust's syntax:
///
/// ```text
/// trait Foo: Bar + Baz { }
///            ^^^^^^^^^ supertrait list bounding the `Self` type parameter
///
/// fn foo<T: Bar + Baz>() { }
///           ^^^^^^^^^ bounding the type parameter `T`
///
/// impl dyn Bar + Baz
///          ^^^^^^^^^ bounding the type-erased dynamic type
/// ```
///
/// Our representation is a bit mixed here -- in some cases, we
/// include the self type (e.g., `trait_bounds`) but in others we do not
#[derive(Default, PartialEq, Eq, Clone, Debug)]
pub(crate) struct Bounds<'tcx> {
    clauses: Vec<(ty::Clause<'tcx>, Span)>,
    effects_min_tys: FxIndexMap<Ty<'tcx>, Span>,
}

impl<'tcx> Bounds<'tcx> {
    pub(crate) fn push_region_bound(
        &mut self,
        tcx: TyCtxt<'tcx>,
        region: ty::PolyTypeOutlivesPredicate<'tcx>,
        span: Span,
    ) {
        self.clauses
            .push((region.map_bound(|p| ty::ClauseKind::TypeOutlives(p)).upcast(tcx), span));
    }

    pub(crate) fn push_trait_bound(
        &mut self,
        tcx: TyCtxt<'tcx>,
        defining_def_id: DefId,
        bound_trait_ref: ty::PolyTraitRef<'tcx>,
        span: Span,
        polarity: ty::PredicatePolarity,
        constness: ty::BoundConstness,
    ) {
        let clause = (
            bound_trait_ref
                .map_bound(|trait_ref| {
                    ty::ClauseKind::Trait(ty::TraitPredicate { trait_ref, polarity })
                })
                .upcast(tcx),
            span,
        );
        // FIXME(-Znext-solver): We can likely remove this hack once the new trait solver lands.
        if tcx.is_lang_item(bound_trait_ref.def_id(), LangItem::Sized) {
            self.clauses.insert(0, clause);
        } else {
            self.clauses.push(clause);
        }

        if !tcx.features().effects {
            return;
        }
        // For `T: ~const Tr` or `T: const Tr`, we need to add an additional bound on the
        // associated type of `<T as Tr>` and make sure that the effect is compatible.
        let compat_val = match (tcx.def_kind(defining_def_id), constness) {
            // FIXME(effects): revisit the correctness of this
            (_, ty::BoundConstness::Const) => tcx.consts.false_,
            // body owners that can have trait bounds
            (DefKind::Const | DefKind::Fn | DefKind::AssocFn, ty::BoundConstness::ConstIfConst) => {
                tcx.expected_host_effect_param_for_body(defining_def_id)
            }

            (_, ty::BoundConstness::NotConst) => {
                if !tcx.is_const_trait(bound_trait_ref.def_id()) {
                    return;
                }
                tcx.consts.true_
            }

            (
                DefKind::Trait | DefKind::Impl { of_trait: true },
                ty::BoundConstness::ConstIfConst,
            ) => {
                // this is either a where clause on an impl/trait header or on a trait.
                // push `<T as Tr>::Effects` into the set for the `Min` bound.
                let Some(assoc) = tcx.associated_type_for_effects(bound_trait_ref.def_id()) else {
                    tcx.dcx().span_delayed_bug(span, "`~const` on trait without Effects assoc");
                    return;
                };

                let ty = bound_trait_ref
                    .map_bound(|trait_ref| Ty::new_projection(tcx, assoc, trait_ref.args));

                // When the user has written `for<'a, T> X<'a, T>: ~const Foo`, replace the
                // binders to dummy ones i.e. `X<'static, ()>` so they can be referenced in
                // the `Min` associated type properly (which doesn't allow using `for<>`)
                // This should work for any bound variables as long as they don't have any
                // bounds e.g. `for<T: Trait>`.
                // FIXME(effects) reconsider this approach to allow compatibility with `for<T: Tr>`
                let ty = tcx.replace_bound_vars_uncached(
                    ty,
                    FnMutDelegate {
                        regions: &mut |_| tcx.lifetimes.re_static,
                        types: &mut |_| tcx.types.unit,
                        consts: &mut |_| unimplemented!("`~const` does not support const binders"),
                    },
                );

                self.effects_min_tys.insert(ty, span);
                return;
            }
            // for
            // ```
            // trait Foo { type Bar: ~const Trait }
            // ```
            // ensure that `<Self::Bar as Trait>::Effects: TyCompat<Self::Effects>`.
            //
            // FIXME(effects) this is equality for now, which wouldn't be helpful for a non-const implementor
            // that uses a `Bar` that implements `Trait` with `Maybe` effects.
            (DefKind::AssocTy, ty::BoundConstness::ConstIfConst) => {
                // FIXME(effects): implement this
                return;
            }
            // probably illegal in this position.
            (_, ty::BoundConstness::ConstIfConst) => {
                tcx.dcx().span_delayed_bug(span, "invalid `~const` encountered");
                return;
            }
        };
        // create a new projection type `<T as Tr>::Effects`
        let Some(assoc) = tcx.associated_type_for_effects(bound_trait_ref.def_id()) else {
            tcx.dcx().span_delayed_bug(
                span,
                "`~const` trait bound has no effect assoc yet no errors encountered?",
            );
            return;
        };
        let self_ty = Ty::new_projection(tcx, assoc, bound_trait_ref.skip_binder().args);
        // make `<T as Tr>::Effects: Compat<runtime>`
        let new_trait_ref = ty::TraitRef::new(
            tcx,
            tcx.require_lang_item(LangItem::EffectsCompat, Some(span)),
            [ty::GenericArg::from(self_ty), compat_val.into()],
        );
        self.clauses.push((bound_trait_ref.rebind(new_trait_ref).upcast(tcx), span));
    }

    pub(crate) fn push_projection_bound(
        &mut self,
        tcx: TyCtxt<'tcx>,
        projection: ty::PolyProjectionPredicate<'tcx>,
        span: Span,
    ) {
        self.clauses.push((
            projection.map_bound(|proj| ty::ClauseKind::Projection(proj)).upcast(tcx),
            span,
        ));
    }

    pub(crate) fn push_sized(&mut self, tcx: TyCtxt<'tcx>, ty: Ty<'tcx>, span: Span) {
        let sized_def_id = tcx.require_lang_item(LangItem::Sized, Some(span));
        let trait_ref = ty::TraitRef::new(tcx, sized_def_id, [ty]);
        // Preferable to put this obligation first, since we report better errors for sized ambiguity.
        self.clauses.insert(0, (trait_ref.upcast(tcx), span));
    }

    pub(crate) fn clauses(
        &self,
        // FIXME(effects): remove tcx
        _tcx: TyCtxt<'tcx>,
    ) -> impl Iterator<Item = (ty::Clause<'tcx>, Span)> + '_ {
        self.clauses.iter().cloned()
    }

    pub(crate) fn effects_min_tys(&self) -> impl Iterator<Item = Ty<'tcx>> + '_ {
        self.effects_min_tys.keys().copied()
    }
}
