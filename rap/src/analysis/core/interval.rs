pub mod interval_analysis;


pub mod domain;
use log::{debug, info};
use rustc_hir::def::DefKind;
use rustc_hir::def_id::DefId;
use rustc_middle::ty::TyCtxt;
use rustc_session::Session;
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::rc::Rc;
pub struct IntervalAnalysis<'a, 'tcx> {
    /// The central data structure of the compiler
    pub tcx: TyCtxt<'tcx>,

    /// Represents the data associated with a compilation session for a single crate

    /// The entry function of the analysis
    pub entry_point: DefId,

    /// Stores the DefIds that have been already checked, to avoid redundant check
    pub checked_def_ids: HashSet<DefId>,

    /// Stores the Heaps that have been already dropped, to detect double-free, use-after-free, etc.
    pub dropped_heaps: HashSet<Rc<SymbolicValue>>,

    /// Cache for the Weak Topological Ordering
    pub wto_cache: WtoCache<'tcx>,

    /// Cache for the name of each DefId
    pub function_name_cache: HashMap<DefId, Rc<String>>,

    /// Customized options that may change the behavior of the analysis
    pub analysis_options: AnalysisOption,

    /// Generated diagnostic messages for each DefId
    pub diagnostics_for: DiagnosticsForDefId<'compiler>,
}
impl IntervalAnalysis<'tcx, '_> {
    pub fn new<'a, 'tcx>(
        tcx: TyCtxt<'tcx>
    ) -> Self {

            }
}