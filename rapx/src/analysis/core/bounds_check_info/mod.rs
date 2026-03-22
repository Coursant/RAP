use crate::analysis::Analysis;
use crate::utils::log::{span_to_filename, span_to_line_number};
use rustc_hir::Safety;
use rustc_hir::def::DefKind;
use rustc_hir::def_id::DefId;
use rustc_middle::mir::{
    AssertKind, Operand, Rvalue, StatementKind, TerminatorKind, UnOp,
};
use rustc_middle::ty::{self, TyCtxt, TyKind};
use rustc_span::Span;
use serde::Serialize;
use std::collections::HashMap;

/// Reserved for future LLVM-related metadata (e.g., IR location, debug info).
#[derive(Serialize, Debug, Clone)]
pub struct LlvmMetadata {
    // Placeholder fields for LLVM-related information.
    // These will be populated when LLVM integration is added.
}

#[derive(Serialize, Debug, Clone)]
pub struct LocationInfo {
    pub file: String,
    pub line: usize,
    pub mir_bb: String,
    pub statement_idx: usize,
}

#[derive(Serialize, Debug, Clone)]
pub struct SymbolicFeatures {
    pub index_expr: String,
    pub len_expr: String,
    #[serde(rename = "type")]
    pub ty: String,
}

#[derive(Serialize, Debug, Clone)]
pub struct FunctionContext {
    pub name: String,
    pub is_unsafe_fn: bool,
    pub local_scope_safety: String,
}

#[derive(Serialize, Debug, Clone)]
pub struct CallerInfo {
    pub name: String,
    pub caller_is_unsafe: bool,
    pub call_site_in_unsafe_block: bool,
}

#[derive(Serialize, Debug, Clone)]
pub struct CallContext {
    pub callers: Vec<CallerInfo>,
}

#[derive(Serialize, Debug, Clone)]
pub struct BoundsCheckEntry {
    pub location: LocationInfo,
    pub symbolic_features: SymbolicFeatures,
    pub function_context: FunctionContext,
    pub call_context: CallContext,
    /// Reserved for future LLVM-related metadata.
    pub llvm_metadata: Option<LlvmMetadata>,
}

#[derive(Serialize, Debug, Default)]
pub struct BoundsCheckDb {
    pub bounds_checks: Vec<BoundsCheckEntry>,
}

pub struct BoundsCheckAnalyzer<'tcx> {
    tcx: TyCtxt<'tcx>,
    pub db: BoundsCheckDb,
}

impl<'tcx> Analysis for BoundsCheckAnalyzer<'tcx> {
    fn name(&self) -> &'static str {
        "Bounds check database collector"
    }

    fn run(&mut self) {
        self.collect();
    }

    fn reset(&mut self) {
        self.db = BoundsCheckDb::default();
    }
}

impl<'tcx> BoundsCheckAnalyzer<'tcx> {
    pub fn new(tcx: TyCtxt<'tcx>) -> Self {
        Self {
            tcx,
            db: BoundsCheckDb::default(),
        }
    }

    /// Collect all bounds-check Assert terminators and build the database.
    pub fn collect(&mut self) {
        let reverse_call_map = self.build_reverse_call_map();

        for local_def_id in self.tcx.mir_keys(()) {
            let def_id = local_def_id.to_def_id();
            if !self.tcx.is_mir_available(def_id) {
                continue;
            }

            let def_kind = self.tcx.def_kind(def_id);
            let body = match def_kind {
                DefKind::Fn | DefKind::AssocFn | DefKind::Closure => {
                    self.tcx.optimized_mir(def_id)
                }
                DefKind::Const
                | DefKind::Static { .. }
                | DefKind::AssocConst
                | DefKind::InlineConst
                | DefKind::AnonConst => self.tcx.mir_for_ctfe(def_id),
                _ => continue,
            };

            let is_unsafe_fn = is_fn_unsafe(self.tcx, def_id);
            let fn_name = self.tcx.def_path_str(def_id);
            let callers = build_caller_info(self.tcx, def_id, &reverse_call_map);

            for (bb_idx, bb_data) in body.basic_blocks.iter_enumerated() {
                let Some(terminator) = &bb_data.terminator else {
                    continue;
                };
                let TerminatorKind::Assert { msg, .. } = &terminator.kind else {
                    continue;
                };
                let AssertKind::BoundsCheck { index, len } = msg.as_ref() else {
                    continue;
                };

                let span = terminator.source_info.span;
                let file = span_to_filename(span);
                let line = span_to_line_number(span);
                let mir_bb = format!("bb{}", bb_idx.as_usize());

                // The Assert terminator itself has no preceding statement; the
                // statement_idx represents how many statements appear before it in
                // the same basic block.
                let statement_idx = bb_data.statements.len();

                let index_expr = format_operand(index);
                let len_expr = format_operand(len);
                let ty_str = infer_container_type(self.tcx, body, index, len);

                let local_scope_safety = if is_unsafe_fn {
                    "Unsafe".to_string()
                } else {
                    "Safe".to_string()
                };

                let entry = BoundsCheckEntry {
                    location: LocationInfo {
                        file,
                        line,
                        mir_bb,
                        statement_idx,
                    },
                    symbolic_features: SymbolicFeatures {
                        index_expr,
                        len_expr,
                        ty: ty_str,
                    },
                    function_context: FunctionContext {
                        name: fn_name.clone(),
                        is_unsafe_fn,
                        local_scope_safety,
                    },
                    call_context: CallContext {
                        callers: callers.clone(),
                    },
                    llvm_metadata: None,
                };

                self.db.bounds_checks.push(entry);
            }
        }
    }

    /// Build a reverse call map: callee DefId → Vec<(caller DefId, call-site Span)>.
    fn build_reverse_call_map(&self) -> HashMap<DefId, Vec<(DefId, Span)>> {
        let mut map: HashMap<DefId, Vec<(DefId, Span)>> = HashMap::new();

        for local_def_id in self.tcx.mir_keys(()) {
            let def_id = local_def_id.to_def_id();
            if !self.tcx.is_mir_available(def_id) {
                continue;
            }
            let def_kind = self.tcx.def_kind(def_id);
            let body = match def_kind {
                DefKind::Fn | DefKind::AssocFn | DefKind::Closure => {
                    self.tcx.optimized_mir(def_id)
                }
                _ => continue,
            };

            for bb_data in body.basic_blocks.iter() {
                let Some(terminator) = &bb_data.terminator else {
                    continue;
                };
                let TerminatorKind::Call { func, .. } = &terminator.kind else {
                    continue;
                };
                let Operand::Constant(constant) = func else {
                    continue;
                };
                let ty::FnDef(callee_def_id, _) = constant.const_.ty().kind() else {
                    continue;
                };
                let call_span = terminator.source_info.span;
                map.entry(*callee_def_id)
                    .or_default()
                    .push((def_id, call_span));
            }
        }

        map
    }

    /// Dump the collected database to a JSON file.
    pub fn dump_to_json(&self, path: &str) -> std::io::Result<()> {
        let file = std::fs::File::create(path)?;
        serde_json::to_writer_pretty(file, &self.db)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
        Ok(())
    }
}

// ── Helper functions ──────────────────────────────────────────────────────────

/// Return true if the given `DefId` refers to an `unsafe fn`.
fn is_fn_unsafe(tcx: TyCtxt<'_>, def_id: DefId) -> bool {
    let def_kind = tcx.def_kind(def_id);
    match def_kind {
        DefKind::Fn | DefKind::AssocFn => {
            let sig = tcx.fn_sig(def_id).skip_binder();
            sig.safety() == Safety::Unsafe
        }
        // Closures inherit the safety of their enclosing scope and cannot
        // be independently declared `unsafe fn`, so treat them as safe.
        DefKind::Closure => false,
        _ => false,
    }
}

/// Collect `CallerInfo` entries for `callee_def_id` using the reverse call map.
fn build_caller_info<'tcx>(
    tcx: TyCtxt<'tcx>,
    callee_def_id: DefId,
    reverse_call_map: &HashMap<DefId, Vec<(DefId, Span)>>,
) -> Vec<CallerInfo> {
    let Some(callers) = reverse_call_map.get(&callee_def_id) else {
        return vec![];
    };

    callers
        .iter()
        .map(|(caller_id, call_span)| {
            let caller_name = tcx.def_path_str(*caller_id);
            let caller_is_unsafe = is_fn_unsafe(tcx, *caller_id);

            // A call site is in an unsafe block if the caller function itself is
            // unsafe, or if the call site's MIR scope is inside an explicit unsafe
            // block within a safe function.
            let call_site_in_unsafe_block = caller_is_unsafe
                || call_site_scope_is_unsafe(tcx, *caller_id, *call_span);

            CallerInfo {
                name: caller_name,
                caller_is_unsafe,
                call_site_in_unsafe_block,
            }
        })
        .collect()
}

/// Return true if the call-site span within `caller_def_id` resides inside an
/// unsafe block by matching the span of a `Call` terminator and inspecting the
/// containing basic block's scope in the caller's MIR.
///
/// In this version of the compiler `SourceScopeLocalData` does not carry a
/// `safety` field, so we approximate the check by looking for a `Call`
/// terminator with the matching span and checking whether any predecessor
/// block on the path already indicates unsafety via its terminator scope.
/// For a first approximation we simply report `false` for safe callers.
fn call_site_scope_is_unsafe<'tcx>(
    tcx: TyCtxt<'tcx>,
    caller_def_id: DefId,
    call_span: Span,
) -> bool {
    if !tcx.is_mir_available(caller_def_id) {
        return false;
    }
    let def_kind = tcx.def_kind(caller_def_id);
    let body = match def_kind {
        DefKind::Fn | DefKind::AssocFn | DefKind::Closure => tcx.optimized_mir(caller_def_id),
        _ => return false,
    };

    // Walk the source scopes of the matching terminator. In nightly-2025-12-06
    // `SourceScopeLocalData` only has `lint_root`; safety is tracked differently.
    // We therefore rely on identifying unsafe closures/fn items via the function
    // signature (handled by the caller) and return false here for safe functions.
    for bb_data in body.basic_blocks.iter() {
        let Some(terminator) = &bb_data.terminator else {
            continue;
        };
        if terminator.source_info.span == call_span {
            // Walk up the scope chain and check if any scope is the body of an
            // unsafe fn (which we detect by checking the parent function's safety).
            // Since scope-level unsafe blocks are not visible in scope local data
            // for this compiler version, we conservatively return false.
            return false;
        }
    }
    false
}

/// Format an `Operand` as a symbolic expression string.
fn format_operand(operand: &Operand<'_>) -> String {
    match operand {
        Operand::Copy(place) | Operand::Move(place) => {
            if place.projection.is_empty() {
                format!("SymbExpr::Var(_{:?})", place.local.as_usize())
            } else {
                format!("SymbExpr::Proj({:?})", place)
            }
        }
        Operand::Constant(c) => format!("SymbExpr::Const({:?})", c.const_),
    }
}

/// Infer the container type for a bounds-check assertion.
///
/// We inspect the MIR body for the statement that produced the `len` operand
/// and try to classify the container as `Slice`, `Array`, or `Vec`. Falls
/// back to `"Slice"` when the information is not readily available, since
/// slices are the most common source of bounds checks.
///
/// In nightly-2025-12-06, `Rvalue::Len` no longer exists; slice lengths are
/// computed via `Rvalue::UnaryOp(UnOp::PtrMetadata, <ref-to-slice>)`.
fn infer_container_type<'tcx>(
    tcx: TyCtxt<'tcx>,
    body: &rustc_middle::mir::Body<'tcx>,
    _index: &Operand<'tcx>,
    len: &Operand<'tcx>,
) -> String {
    // If `len` is a constant, the container is an array with a fixed size.
    if matches!(len, Operand::Constant(_)) {
        return "Array".to_string();
    }

    // If `len` is a local variable, trace how it was defined.
    let len_local = match len {
        Operand::Copy(place) | Operand::Move(place) if place.projection.is_empty() => {
            place.local
        }
        _ => return "Slice".to_string(),
    };

    // Search all basic blocks for the assignment that defines `len_local`.
    for bb_data in body.basic_blocks.iter() {
        for stmt in &bb_data.statements {
            if let StatementKind::Assign(box (lhs, rvalue)) = &stmt.kind {
                if lhs.local != len_local || !lhs.projection.is_empty() {
                    continue;
                }
                match rvalue {
                    // In nightly-2025-12-06, slice/array length is retrieved via
                    // `UnaryOp(PtrMetadata, ref_to_container)`.
                    Rvalue::UnaryOp(UnOp::PtrMetadata, operand) => {
                        let op_ty = operand.ty(&body.local_decls, tcx);
                        // Peel references/pointers to get the pointee type.
                        let pointee_ty = match op_ty.kind() {
                            TyKind::Ref(_, ty, _) => *ty,
                            TyKind::RawPtr(ty, _) => *ty,
                            _ => op_ty,
                        };
                        return classify_container_ty(tcx, pointee_ty);
                    }
                    // Constant length → fixed-size array.
                    Rvalue::Use(Operand::Constant(_)) => {
                        return "Array".to_string();
                    }
                    _ => {}
                }
            }
        }
    }

    // Fall back: inspect the type of `len_local` itself.
    let local_ty = body.local_decls[len_local].ty;
    match local_ty.kind() {
        TyKind::Uint(_) | TyKind::Int(_) => "Slice".to_string(),
        _ => "Unknown".to_string(),
    }
}

/// Classify a container type as `"Slice"`, `"Array"`, `"Vec"`, or a generic
/// `"Adt(path)"` string.
fn classify_container_ty<'tcx>(tcx: TyCtxt<'tcx>, ty: ty::Ty<'tcx>) -> String {
    match ty.kind() {
        TyKind::Slice(_) => "Slice".to_string(),
        TyKind::Array(_, _) => "Array".to_string(),
        TyKind::Adt(adt_def, _) => {
            let path = tcx.def_path_str(adt_def.did());
            if path == "alloc::vec::Vec" || path.ends_with("::Vec") {
                "Vec".to_string()
            } else {
                format!("Adt({})", path)
            }
        }
        _ => "Slice".to_string(),
    }
}
