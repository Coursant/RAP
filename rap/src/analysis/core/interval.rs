pub mod interval_analysis;



use log::{debug, info};
use rustc_hir::def::DefKind;
use rustc_hir::def_id::DefId;
use rustc_middle::ty::TyCtxt;
use rustc_session::Session;
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::rc::Rc;
pub struct IntervalAnalysis<'tcx> {
    /// The central data structure of the compiler
    pub tcx: TyCtxt<'tcx>,


}
impl< 'tcx> IntervalAnalysis<'tcx> {

}