use std::{collections::HashMap, fs::File, path::Path};

use serde::Serialize;

use rustc_hir::{
    Block, BlockCheckMode, Safety, UnsafeSource,
    def::DefKind,
    intravisit,
};
use rustc_middle::{
    mir::{AssertKind, Body, Operand, TerminatorKind},
    ty::{Ty, TyCtxt, TyKind},
};

use crate::analysis::{
    core::callgraph::default::CallGraphAnalyzer,
    utils::fn_info::check_safety,
};
use crate::utils::log::{span_to_filename, span_to_line_number};

#[derive(Serialize)]
pub struct BoundsChecksDatabase {
    pub bounds_checks: Vec<BoundsCheckRecord>,
    pub llvm: LlvmReserved,
}

#[derive(Serialize)]
pub struct BoundsCheckRecord {
    pub location: LocationInfo,
    pub symbolic_features: SymbolicFeatures,
    pub function_context: FunctionContext,
    pub call_context: CallContext,
}

#[derive(Serialize)]
pub struct LocationInfo {
    pub file: String,
    pub line: usize,
    pub mir_bb: String,
    pub statement_idx: usize,
}

#[derive(Serialize)]
pub struct SymbolicFeatures {
    pub index_expr: String,
    pub len_expr: String,
    #[serde(rename = "type")]
    pub ty: String,
}

#[derive(Serialize)]
pub struct FunctionContext {
    pub name: String,
    pub is_unsafe_fn: bool,
    pub local_scope_safety: String,
}

#[derive(Serialize)]
pub struct CallContext {
    pub callers: Vec<CallerInfo>,
}

#[derive(Serialize)]
pub struct CallerInfo {
    pub name: String,
    pub caller_is_unsafe: bool,
    pub call_site_in_unsafe_block: bool,
}

#[derive(Serialize)]
pub struct LlvmReserved {
    pub reserved: serde_json::Value,
}

pub fn dump_bounds_assert_database<'tcx>(
    tcx: TyCtxt<'tcx>,
    path: impl AsRef<Path>,
) -> std::io::Result<()> {
    let mut callgraph = CallGraphAnalyzer::new(tcx);
    callgraph.start();
    let callers_map = callgraph.graph.get_callers_map();

    let mut all_records = Vec::new();
    for local_def_id in tcx.iter_local_def_id() {
        let def_id = local_def_id.to_def_id();
        let def_kind = tcx.def_kind(local_def_id);
        if !matches!(def_kind, DefKind::Fn | DefKind::AssocFn) {
            continue;
        }
        let body = tcx.optimized_mir(def_id);
        collect_bounds_checks_in_body(tcx, body, &callers_map, &mut all_records);
    }

    let db = BoundsChecksDatabase {
        bounds_checks: all_records,
        llvm: LlvmReserved {
            reserved: serde_json::json!({}),
        },
    };
    let file = File::create(path)?;
    serde_json::to_writer_pretty(file, &db)?;
    Ok(())
}

fn collect_bounds_checks_in_body<'tcx>(
    tcx: TyCtxt<'tcx>,
    body: &'tcx Body<'tcx>,
    callers_map: &HashMap<rustc_hir::def_id::DefId, Vec<(rustc_hir::def_id::DefId, Option<&'tcx rustc_middle::mir::Terminator<'tcx>>)>>,
    output: &mut Vec<BoundsCheckRecord>,
) {
    let def_id = body.source.def_id();
    let function_name = tcx.def_path_str(def_id);
    let is_unsafe_fn = check_safety(tcx, def_id) == Safety::Unsafe;

    for (bb_idx, bb_data) in body.basic_blocks.iter_enumerated() {
        if let TerminatorKind::Assert { msg, .. } = &bb_data.terminator().kind {
            if let AssertKind::BoundsCheck { len, index } = &**msg {
                let span = bb_data.terminator().source_info.span;
                let file = span_to_filename(span);
                let line = span_to_line_number(span);
                let local_scope_safety = if is_unsafe_fn { "Unsafe" } else { "Safe" }.to_string();
                let callers = build_callers(tcx, callers_map, def_id);
                output.push(BoundsCheckRecord {
                    location: LocationInfo {
                        file,
                        line,
                        mir_bb: format!("{:?}", bb_idx),
                        statement_idx: body.basic_blocks[bb_idx].statements.len(),
                    },
                    symbolic_features: SymbolicFeatures {
                        index_expr: format!("{index:?}"),
                        len_expr: format!("{len:?}"),
                        ty: infer_bounds_type(tcx, body, len, index),
                    },
                    function_context: FunctionContext {
                        name: function_name.clone(),
                        is_unsafe_fn,
                        local_scope_safety,
                    },
                    call_context: CallContext { callers },
                });
            }
        }
    }
}

fn infer_bounds_type<'tcx>(
    tcx: TyCtxt<'tcx>,
    body: &Body<'tcx>,
    len: &Operand<'tcx>,
    index: &Operand<'tcx>,
) -> String {
    let mut kinds = vec![];
    if let Operand::Copy(place) | Operand::Move(place) = len {
        kinds.push(classify_bounds_ty(tcx, body.local_decls[place.local].ty));
    }
    if let Operand::Copy(place) | Operand::Move(place) = index {
        kinds.push(classify_bounds_ty(tcx, body.local_decls[place.local].ty));
    }
    if kinds.iter().any(|k| k == "Slice") {
        "Slice".to_string()
    } else if kinds.iter().any(|k| k == "Vec") {
        "Vec".to_string()
    } else {
        kinds
            .into_iter()
            .next()
            .unwrap_or_else(|| "Unknown".to_string())
    }
}

fn classify_bounds_ty(tcx: TyCtxt<'_>, ty: Ty<'_>) -> String {
    match ty.kind() {
        TyKind::Slice(_) | TyKind::Array(_, _) | TyKind::Str => "Slice".to_string(),
        TyKind::Adt(adt, _) => {
            let path = tcx.def_path_str(adt.did());
            if path == "alloc::vec::Vec" || path == "std::vec::Vec" {
                "Vec".to_string()
            } else {
                ty.to_string()
            }
        }
        _ => ty.to_string(),
    }
}

fn build_callers<'tcx>(
    tcx: TyCtxt<'tcx>,
    callers_map: &HashMap<rustc_hir::def_id::DefId, Vec<(rustc_hir::def_id::DefId, Option<&'tcx rustc_middle::mir::Terminator<'tcx>>)>>,
    def_id: rustc_hir::def_id::DefId,
) -> Vec<CallerInfo> {
    callers_map
        .get(&def_id)
        .map(|v| {
            v.iter()
                .map(|(caller, terminator)| CallerInfo {
                    name: tcx.def_path_str(*caller),
                    caller_is_unsafe: check_safety(tcx, *caller) == Safety::Unsafe,
                    call_site_in_unsafe_block: terminator
                        .map(|term| {
                            is_call_site_in_unsafe_block(tcx, *caller, term.source_info.span)
                        })
                        .unwrap_or(false),
                })
                .collect()
        })
        .unwrap_or_default()
}

struct UnsafeBlockFinder {
    spans: Vec<rustc_span::Span>,
}

impl<'tcx> intravisit::Visitor<'tcx> for UnsafeBlockFinder {
    fn visit_block(&mut self, block: &'tcx Block<'tcx>) {
        if let BlockCheckMode::UnsafeBlock(UnsafeSource::UserProvided) = block.rules {
            self.spans.push(block.span);
        }
        intravisit::walk_block(self, block);
    }
}

fn is_call_site_in_unsafe_block(
    tcx: TyCtxt<'_>,
    caller_def_id: rustc_hir::def_id::DefId,
    call_span: rustc_span::Span,
) -> bool {
    let Some(local_def_id) = caller_def_id.as_local() else {
        return false;
    };
    let body = tcx.hir_body_owned_by(local_def_id);
    let mut finder = UnsafeBlockFinder { spans: vec![] };
    intravisit::walk_body(&mut finder, body);
    finder
        .spans
        .into_iter()
        .any(|unsafe_span| span_contains(unsafe_span, call_span))
}

fn span_contains(outer: rustc_span::Span, inner: rustc_span::Span) -> bool {
    outer.lo() <= inner.lo() && inner.hi() <= outer.hi()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serialize_shape_contains_llvm_placeholder() {
        let db = BoundsChecksDatabase {
            bounds_checks: vec![],
            llvm: LlvmReserved {
                reserved: serde_json::json!({}),
            },
        };
        let v = serde_json::to_value(db).unwrap();
        assert!(v.get("bounds_checks").is_some());
        assert!(v.get("llvm").is_some());
        assert_eq!(v["bounds_checks"], serde_json::json!([]));
    }
}
