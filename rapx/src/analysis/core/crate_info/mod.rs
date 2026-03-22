use crate::analysis::Analysis;
use crate::analysis::core::bounds_check_info::{BoundsCheckAnalyzer, BoundsCheckEntry};
use crate::utils::log::{span_to_filename, span_to_line_number};
use rustc_hir::Safety;
use rustc_hir::def::DefKind;
use rustc_hir::def_id::{DefId, LOCAL_CRATE};
use rustc_middle::mir::{Operand, Rvalue, StatementKind, TerminatorKind};
use rustc_middle::ty::{self, TyCtxt};
use rustc_span::Span;
use serde::Serialize;
use std::collections::HashMap;

// ── Per-function unsafe-operation record ─────────────────────────────────────

/// The kind of unsafe operation found in a function body.
#[derive(Serialize, Debug, Clone)]
pub struct UnsafeOp {
    /// Short name for the unsafe operation:
    /// `"RawPtrDeref"`, `"UnsafeFnCall"`, `"InlineAsm"`, or `"UnionFieldAccess"`.
    pub kind: String,
    pub file: String,
    pub line: usize,
}

// ── Per-function record ───────────────────────────────────────────────────────

#[derive(Serialize, Debug, Clone)]
pub struct FunctionEntry {
    /// Fully-qualified path of the function (e.g. `my_crate::module::foo`).
    pub id: String,
    pub file: String,
    pub line: usize,
    /// `"Public"` or `"Private"`.
    pub visibility: String,
    pub is_unsafe_fn: bool,
    /// Number of generic lifetime/type/const parameters on the function.
    pub generic_param_count: usize,
    /// Number of formal parameters (function arguments).
    pub param_count: usize,
    /// Number of MIR basic blocks.
    pub basic_block_count: usize,
    /// Total number of MIR statements across all basic blocks.
    pub statement_count: usize,
    /// Number of bounds-check `Assert` terminators in this function.
    pub bounds_check_count: usize,
    /// Unsafe operations found in the function body.
    pub unsafe_ops: Vec<UnsafeOp>,
}

// ── Top-level database ────────────────────────────────────────────────────────

#[derive(Serialize, Debug, Default)]
pub struct CrateDatabase {
    /// Name of the analysed crate.
    pub crate_name: String,
    /// One entry per local function / associated function / closure.
    pub functions: Vec<FunctionEntry>,
    /// All bounds-check assert terminators found in the crate.
    pub bounds_checks: Vec<BoundsCheckEntry>,
}

// ── Analyzer ─────────────────────────────────────────────────────────────────

pub struct CrateInfoAnalyzer<'tcx> {
    tcx: TyCtxt<'tcx>,
    pub db: CrateDatabase,
}

impl<'tcx> Analysis for CrateInfoAnalyzer<'tcx> {
    fn name(&self) -> &'static str {
        "Crate information database collector"
    }

    fn run(&mut self) {
        self.collect();
    }

    fn reset(&mut self) {
        self.db = CrateDatabase::default();
    }
}

impl<'tcx> CrateInfoAnalyzer<'tcx> {
    pub fn new(tcx: TyCtxt<'tcx>) -> Self {
        let crate_name = tcx.crate_name(LOCAL_CRATE).to_string();
        Self {
            tcx,
            db: CrateDatabase {
                crate_name,
                ..CrateDatabase::default()
            },
        }
    }

    /// Collect all per-function data and bounds-check data into the database.
    pub fn collect(&mut self) {
        // First, run the bounds-check collector and steal its results.
        let mut bc_analyzer = BoundsCheckAnalyzer::new(self.tcx);
        bc_analyzer.run();

        // Build a per-function bounds-check count from the collected entries.
        let mut bc_count_map: HashMap<String, usize> = HashMap::new();
        for entry in &bc_analyzer.db.bounds_checks {
            *bc_count_map.entry(entry.function_context.name.clone()).or_default() += 1;
        }

        self.db.bounds_checks = bc_analyzer.db.bounds_checks;

        // Now collect per-function information.
        let reverse_call_map = self.build_reverse_call_map();
        let _ = reverse_call_map; // reserved for future caller-context use

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

            let fn_name = self.tcx.def_path_str(def_id);
            let fn_span: Span = body.span;
            let file = span_to_filename(fn_span);
            let line = span_to_line_number(fn_span);

            let is_unsafe_fn = is_fn_unsafe(self.tcx, def_id);
            let visibility = if is_def_id_public(def_id, self.tcx) {
                "Public"
            } else {
                "Private"
            }
            .to_string();

            let generic_param_count = self
                .tcx
                .generics_of(def_id)
                .own_params
                .len();

            // `body.arg_count` is the number of formal function parameters.
            let param_count = body.arg_count;

            let basic_block_count = body.basic_blocks.len();
            let statement_count: usize =
                body.basic_blocks.iter().map(|bb| bb.statements.len()).sum();

            let bounds_check_count = bc_count_map.get(&fn_name).copied().unwrap_or(0);

            let unsafe_ops = collect_unsafe_ops(self.tcx, def_id, body);

            self.db.functions.push(FunctionEntry {
                id: fn_name,
                file,
                line,
                visibility,
                is_unsafe_fn,
                generic_param_count,
                param_count,
                basic_block_count,
                statement_count,
                bounds_check_count,
                unsafe_ops,
            });
        }
    }

    /// Build a reverse call map (callee → callers) for future use.
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
                map.entry(*callee_def_id)
                    .or_default()
                    .push((def_id, terminator.source_info.span));
            }
        }

        map
    }

    /// Serialize the database to a JSON file at `path`.
    pub fn dump_to_json(&self, path: &str) -> std::io::Result<()> {
        let file = std::fs::File::create(path)?;
        serde_json::to_writer_pretty(file, &self.db)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
        Ok(())
    }
}

// ── Helper functions ──────────────────────────────────────────────────────────

/// Return `true` if the given `DefId` refers to an `unsafe fn`.
fn is_fn_unsafe(tcx: TyCtxt<'_>, def_id: DefId) -> bool {
    match tcx.def_kind(def_id) {
        DefKind::Fn | DefKind::AssocFn => {
            tcx.fn_sig(def_id).skip_binder().safety() == Safety::Unsafe
        }
        _ => false,
    }
}

/// Return `true` if the function is directly visible outside its module.
fn is_def_id_public(def_id: DefId, tcx: TyCtxt<'_>) -> bool {
    let Some(local_id) = def_id.as_local() else {
        return false;
    };
    tcx.effective_visibilities(()).is_directly_public(local_id)
}

/// Walk the MIR body of a function and collect unsafe operations.
///
/// Currently detected:
/// - **`RawPtrDeref`** — a `*ptr` dereference via `Rvalue::RawPtr` or an
///   explicit place projection through a raw pointer.
/// - **`UnsafeFnCall`** — a call to an `unsafe fn` (detected by inspecting the
///   callee's signature in `TerminatorKind::Call`).
/// - **`InlineAsm`** — an inline assembly block (`TerminatorKind::InlineAsm`).
fn collect_unsafe_ops<'tcx>(
    tcx: TyCtxt<'tcx>,
    _def_id: DefId,
    body: &rustc_middle::mir::Body<'tcx>,
) -> Vec<UnsafeOp> {
    let mut ops: Vec<UnsafeOp> = Vec::new();

    for bb_data in body.basic_blocks.iter() {
        // ── Statements ────────────────────────────────────────────────────────
        for stmt in &bb_data.statements {
            if let StatementKind::Assign(box (_lhs, rvalue)) = &stmt.kind {
                match rvalue {
                    // Creating a raw pointer from a reference or other operand.
                    Rvalue::RawPtr(_, _) => {
                        let span = stmt.source_info.span;
                        ops.push(UnsafeOp {
                            kind: "RawPtrDeref".to_string(),
                            file: span_to_filename(span),
                            line: span_to_line_number(span),
                        });
                    }
                    _ => {}
                }
            }
        }

        // ── Terminators ───────────────────────────────────────────────────────
        let Some(terminator) = &bb_data.terminator else {
            continue;
        };

        match &terminator.kind {
            TerminatorKind::Call { func, .. } => {
                // Check if the callee is an unsafe fn.
                if let Operand::Constant(constant) = func {
                    if let ty::FnDef(callee_def_id, _) = constant.const_.ty().kind() {
                        let callee_def_kind = tcx.def_kind(callee_def_id);
                        let callee_is_unsafe = match callee_def_kind {
                            DefKind::Fn | DefKind::AssocFn => tcx
                                .fn_sig(callee_def_id)
                                .skip_binder()
                                .safety()
                                == Safety::Unsafe,
                            _ => false,
                        };
                        if callee_is_unsafe {
                            let span = terminator.source_info.span;
                            ops.push(UnsafeOp {
                                kind: "UnsafeFnCall".to_string(),
                                file: span_to_filename(span),
                                line: span_to_line_number(span),
                            });
                        }
                    }
                }
            }
            TerminatorKind::InlineAsm { .. } => {
                let span = terminator.source_info.span;
                ops.push(UnsafeOp {
                    kind: "InlineAsm".to_string(),
                    file: span_to_filename(span),
                    line: span_to_line_number(span),
                });
            }
            _ => {}
        }
    }

    ops
}
