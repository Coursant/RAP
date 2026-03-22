use std::{collections::HashMap, fs::File, path::Path};

use serde::Serialize;

use rustc_hir::{Safety, def::DefKind};
use rustc_middle::{
    mir::{AssertKind, Body, Operand, TerminatorKind},
    ty::TyCtxt,
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
                        statement_idx: 0,
                    },
                    symbolic_features: SymbolicFeatures {
                        index_expr: format!("{index:?}"),
                        len_expr: format!("{len:?}"),
                        ty: infer_bounds_type(body, len, index),
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

fn infer_bounds_type(body: &Body<'_>, len: &Operand<'_>, index: &Operand<'_>) -> String {
    let ty = match (len, index) {
        (Operand::Copy(place), _) | (Operand::Move(place), _) => {
            body.local_decls[place.local].ty.to_string()
        }
        (_, Operand::Copy(place)) | (_, Operand::Move(place)) => {
            body.local_decls[place.local].ty.to_string()
        }
        _ => return "Unknown".to_string(),
    };
    if ty.contains("slice") || ty.contains('[') {
        "Slice".to_string()
    } else if ty.contains("Vec") {
        "Vec".to_string()
    } else {
        ty
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
                .map(|(caller, _)| CallerInfo {
                    name: tcx.def_path_str(*caller),
                    caller_is_unsafe: check_safety(tcx, *caller) == Safety::Unsafe,
                    call_site_in_unsafe_block: false,
                })
                .collect()
        })
        .unwrap_or_default()
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
