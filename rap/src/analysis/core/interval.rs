pub mod interval_analysis;

use rustc_const_eval::const_eval::{throw_machine_stop_str, DummyMachine};
use rustc_const_eval::interpret::{ImmTy, Immediate, InterpCx, OpTy, PlaceTy, Projectable};
use rustc_data_structures::fx::FxHashMap;
use rustc_hir::def::DefKind;
use rustc_middle::bug;
use rustc_middle::mir::interpret::{InterpResult, Scalar};
use rustc_middle::mir::visit::{MutVisitor, PlaceContext, Visitor};
use rustc_middle::mir::*;
use rustc_middle::ty::layout::{HasParamEnv, LayoutOf};
use rustc_middle::ty::{self, Ty, TyCtxt};
use rustc_mir_dataflow::value_analysis::{
    Map, PlaceIndex, State, TrackElem, ValueAnalysis, ValueAnalysisWrapper, ValueOrPlace,
};
use rustc_mir_dataflow::{lattice::FlatSet, Analysis, Results, ResultsVisitor};
use rustc_span::DUMMY_SP;
use rustc_target::abi::{Abi, FieldIdx, Size, VariantIdx, FIRST_VARIANT};

struct IntervalAnalysis<'a, 'tcx> {
    map: Map,
    tcx: TyCtxt<'tcx>,
    local_decls: &'a LocalDecls<'tcx>,
    ecx: InterpCx<'tcx, DummyMachine>,
    param_env: ty::ParamEnv<'tcx>,
}
