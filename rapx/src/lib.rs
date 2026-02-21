#![feature(rustc_private)]
#![feature(box_patterns)]
#![feature(macro_metavar_expr_concat)]

#[macro_use]
pub mod utils;
pub mod analysis;
pub mod def_id;
pub mod preprocess;
extern crate intervals;
extern crate rustc_abi;
extern crate rustc_ast;
extern crate rustc_data_structures;
extern crate rustc_driver;
extern crate rustc_errors;
extern crate rustc_hir;
extern crate rustc_hir_pretty;
extern crate rustc_index;
extern crate rustc_infer;
extern crate rustc_interface;
extern crate rustc_metadata;
extern crate rustc_middle;
extern crate rustc_public;
extern crate rustc_session;
extern crate rustc_span;
extern crate rustc_target;
extern crate rustc_trait_selection;
extern crate rustc_traits;
extern crate rustc_type_ir;
extern crate thin_vec;
use crate::analysis::{core::alias_analysis::mfp::MfpAliasAnalyzer, scan::ScanAnalysis};
use analysis::{
    Analysis,
    core::{
        alias_analysis::{AliasAnalysis, FnAliasMapWrapper, default::AliasAnalyzer},
        api_dependency::ApiDependencyAnalyzer,
        callgraph::{CallGraphAnalysis, FnCallDisplay, default::CallGraphAnalyzer},
        dataflow::{
            Arg2RetMapWrapper, DataFlowAnalysis, DataFlowGraphMapWrapper, default::DataFlowAnalyzer,
        },
        ownedheap_analysis::{OHAResultMapWrapper, OwnedHeapAnalysis, default::OwnedHeapAnalyzer},
        range_analysis::{
            PathConstraintMapWrapper, RAResultMapWrapper, RangeAnalysis, default::RangeAnalyzer,
        },
        ssa_transform::SSATrans,
    },
    opt::Opt,
    rcanary::rCanary,
    safedrop::SafeDrop,
    senryx::{CheckLevel, SenryxCheck},
    upg::{TargetCrate, UPGAnalysis},
    utils::show_mir::ShowMir,
};
use rustc_ast::ast;
use rustc_driver::{Callbacks, Compilation};
use rustc_interface::{
    Config,
    interface::{self, Compiler},
};
use rustc_middle::{ty::TyCtxt, util::Providers};
use rustc_session::search_paths::PathKind;
use std::path::PathBuf;
use std::sync::Arc;

// Insert rustc arguments at the beginning of the argument list that RAP wants to be
// set per default, for maximal validation power.
pub static RAP_DEFAULT_ARGS: &[&str] = &[
    "-Zalways-encode-mir",
    "-Zmir-opt-level=0",
    "-Zinline-mir-threshold=0",
    "-Zinline-mir-hint-threshold=0",
    "-Zcross-crate-inline-threshold=0",
];

/// This is the data structure to handle rapx options as a rustc callback.

#[derive(Debug, Clone, Hash)]
pub struct RapCallback {
    alias: usize,
    api_dependency: bool,
    callgraph: bool,
    dataflow: usize,
    ownedheap: bool,
    range: usize,
    ssa: bool,
    infer: bool,
    opt: usize,
    rcanary: bool,
    safedrop: bool,
    show_mir: bool,
    show_mir_dot: bool,
    upg: usize,
    verify: bool,
    verify_std: bool,
    scan: bool,
    test_crate: Option<String>,
}

#[allow(clippy::derivable_impls)]
impl Default for RapCallback {
    fn default() -> Self {
        Self {
            alias: 0,
            api_dependency: false,
            callgraph: false,
            dataflow: 0,
            ownedheap: false,
            range: 0,
            ssa: false,
            infer: false,
            opt: usize::MAX,
            rcanary: false,
            safedrop: false,
            show_mir: false,
            show_mir_dot: false,
            upg: 0,
            verify: false,
            verify_std: false,
            scan: false,
            test_crate: None,
        }
    }
}

impl Callbacks for RapCallback {
    fn config(&mut self, config: &mut Config) {
        config.override_queries = Some(|_, providers| {
            providers.extern_queries.used_crate_source = |tcx, cnum| {
                let mut providers = Providers::default();
                rustc_metadata::provide(&mut providers);
                let mut crate_source = (providers.extern_queries.used_crate_source)(tcx, cnum);
                // HACK: rustc will emit "crate ... required to be available in rlib format, but
                // was not found in this form" errors once we use `tcx.dependency_formats()` if
                // there's no rlib provided, so setting a dummy path here to workaround those errors.
                Arc::make_mut(&mut crate_source).rlib = Some((PathBuf::new(), PathKind::All));
                crate_source
            };
        });
    }

    fn after_crate_root_parsing(
        &mut self,
        compiler: &interface::Compiler,
        krate: &mut ast::Crate,
    ) -> Compilation {
        let build_std = compiler
            .sess
            .opts
            .crate_name
            .as_deref()
            .map(|s| matches!(s, "core" | "std"))
            .unwrap_or(false);
        preprocess::dummy_fns::create_dummy_fns(krate, build_std);
        preprocess::ssa_preprocess::create_ssa_struct(krate, build_std);
        Compilation::Continue
    }
    fn after_analysis<'tcx>(&mut self, _compiler: &Compiler, tcx: TyCtxt<'tcx>) -> Compilation {
        rap_trace!("Execute after_analysis() of compiler callbacks");

        rustc_public::rustc_internal::run(tcx, || {
            def_id::init(tcx);
            if self.is_building_test_crate() {
                start_analyzer(tcx, self);
            } else {
                let package_name = std::env::var("CARGO_PKG_NAME")
                    .expect("cannot capture env var `CARGO_PKG_NAME`");
                rap_trace!("skip analyzing package `{}`", package_name);
            }
        })
        .expect("Failed to run rustc_public.");
        rap_trace!("analysis done");

        Compilation::Continue
    }
}

impl RapCallback {
    fn is_building_test_crate(&self) -> bool {
        match &self.test_crate {
            None => true,
            Some(test_crate) => {
                let test_crate: &str = test_crate;
                let package_name = std::env::var("CARGO_PKG_NAME")
                    .expect("cannot capture env var `CARGO_PKG_NAME`");
                package_name == test_crate
            }
        }
    }

    /// Enable alias analysis.
    pub fn enable_alias(&mut self, x: usize) {
        self.alias = x;
    }

    pub fn is_alias_enabled(&self) -> usize {
        self.alias
    }

    /// Enable API-dependency graph generation.
    pub fn enable_api_dependency(&mut self) {
        self.api_dependency = true;
    }

    /// Test if API-dependency graph generation is enabled.
    pub fn is_api_dependency_enabled(&self) -> bool {
        self.api_dependency
    }

    /// Enable call-graph analysis.
    pub fn enable_callgraph(&mut self) {
        self.callgraph = true;
    }

    /// Test if call-graph analysis is enabled.
    pub fn is_callgraph_enabled(&self) -> bool {
        self.callgraph
    }

    /// Enable owned heap analysis.
    pub fn enable_ownedheap(&mut self) {
        self.ownedheap = true;
    }

    /// Test if owned-heap analysis is enabled.
    pub fn is_ownedheap_enabled(&self) -> bool {
        self.ownedheap
    }

    /// Enable dataflow analysis.
    pub fn enable_dataflow(&mut self, x: usize) {
        self.dataflow = x;
    }

    /// Test if dataflow analysis is enabled.
    pub fn is_dataflow_enabled(&self) -> usize {
        self.dataflow
    }

    /// Enable range analysis.
    pub fn enable_range_analysis(&mut self, x: usize) {
        self.range = x;
    }

    /// Test if range analysis is enabled.
    pub fn is_range_analysis_enabled(&self) -> bool {
        self.range > 0
    }

    /// Enable ssa transformation
    pub fn enable_ssa_transform(&mut self) {
        self.ssa = true;
    }

    /// Test if ssa transformation is enabled.
    pub fn is_ssa_transform_enabled(&self) -> bool {
        self.ssa
    }

    /// Enable optimization analysis for performance bug detection.
    pub fn enable_opt(&mut self, x: usize) {
        self.opt = x;
    }

    /// Test if optimization analysis is enabled.
    pub fn is_opt_enabled(&self) -> usize {
        self.opt
    }

    /// Enable rcanary for memory leakage detection.
    pub fn enable_rcanary(&mut self) {
        self.rcanary = true;
    }

    /// Test if rcanary is enabled.
    pub fn is_rcanary_enabled(&self) -> bool {
        self.rcanary
    }

    /// Enable safedrop for use-after-free bug detection.
    /// field-sensitive analysis.
    pub fn enable_safedrop(&mut self) {
        self.safedrop = true;
    }

    /// Test if safedrop is enabled.
    pub fn is_safedrop_enabled(&self) -> bool {
        self.safedrop
    }

    /// Enable mir display.
    pub fn enable_show_mir(&mut self) {
        self.show_mir = true;
    }

    /// Test if mir display is enabled.
    pub fn is_show_mir_enabled(&self) -> bool {
        self.show_mir
    }

    pub fn enable_show_mir_dot(&mut self) {
        self.show_mir_dot = true;
    }

    pub fn is_show_mir_dot_enabled(&self) -> bool {
        self.show_mir_dot
    }

    pub fn enable_upg(&mut self, x: usize) {
        self.upg = x;
    }

    pub fn is_upg_enabled(&self) -> usize {
        self.upg
    }

    pub fn enable_verify(&mut self) {
        self.verify = true;
    }

    pub fn is_verify_enabled(&self) -> bool {
        self.verify
    }

    pub fn enable_verify_std(&mut self) {
        self.verify_std = true;
    }

    pub fn is_verify_std_enabled(&self) -> bool {
        self.verify_std
    }

    pub fn enable_infer(&mut self) {
        self.infer = true;
    }

    pub fn is_infer_enabled(&self) -> bool {
        self.infer
    }

    pub fn enable_scan(&mut self) {
        self.scan = true;
    }

    pub fn is_scan_enabled(&self) -> bool {
        self.scan
    }

    pub fn set_test_crate(&mut self, crate_name: impl ToString) {
        self.test_crate = Some(crate_name.to_string())
    }
}

/// Start the analysis with the features enabled.
pub fn start_analyzer(tcx: TyCtxt, callback: &RapCallback) {
    match callback.is_alias_enabled() {
        1 => {
            let mut analyzer = AliasAnalyzer::new(tcx);
            analyzer.run();
            let alias = analyzer.get_local_fn_alias();
            rap_info!("{}", FnAliasMapWrapper(alias));
        }
        2 => {
            let mut analyzer = MfpAliasAnalyzer::new(tcx);
            analyzer.run();
            let alias = analyzer.get_local_fn_alias();
            rap_info!("{}", FnAliasMapWrapper(alias));
        }
        _ => {}
    }

    if callback.is_api_dependency_enabled() {
        let mut analyzer = ApiDependencyAnalyzer::new(
            tcx,
            analysis::core::api_dependency::Config {
                pub_only: true,
                resolve_generic: true,
                ignore_const_generic: true,
            },
        );
        analyzer.run();
    }

    if callback.is_callgraph_enabled() {
        let mut analyzer = CallGraphAnalyzer::new(tcx);
        analyzer.run();
        let callgraph = analyzer.get_fn_calls();
        rap_info!(
            "{}",
            FnCallDisplay {
                fn_calls: &callgraph,
                tcx
            }
        );
        //analyzer.display();
    }

    match callback.is_dataflow_enabled() {
        1 => {
            let mut analyzer = DataFlowAnalyzer::new(tcx, false);
            analyzer.run();
            let result = analyzer.get_all_arg2ret();
            rap_info!("{}", Arg2RetMapWrapper(result));
        }
        2 => {
            let mut analyzer = DataFlowAnalyzer::new(tcx, true);
            analyzer.run();
            let result = analyzer.get_all_dataflow();
            rap_info!("{}", DataFlowGraphMapWrapper(result));
        }
        _ => {}
    }

    if callback.is_ownedheap_enabled() {
        let mut analyzer = OwnedHeapAnalyzer::new(tcx);
        analyzer.run();
        let result = analyzer.get_all_items();
        rap_info!("{}", OHAResultMapWrapper(result));
    }

    if callback.is_range_analysis_enabled() {
        match callback.range {
            1 => {
                let mut analyzer = RangeAnalyzer::<i64>::new(tcx, false);
                analyzer.run();
                let result = analyzer.get_all_fn_ranges();
                rap_info!("{}", RAResultMapWrapper(result));
            }
            2 => {
                let mut analyzer = RangeAnalyzer::<i64>::new(tcx, true);
                analyzer.run();
                let result = analyzer.get_all_fn_ranges();
                rap_info!("{}", RAResultMapWrapper(result));
            }
            3 => {
                let mut analyzer = RangeAnalyzer::<i64>::new(tcx, false);
                analyzer.start_path_constraints_analysis();
                let result = analyzer.get_all_path_constraints();
                rap_info!("{}", PathConstraintMapWrapper(result));
            }
            _ => {}
        }
    }

    match callback.is_opt_enabled() {
        0 => Opt::new(tcx, 0).start(),
        1 => Opt::new(tcx, 1).start(),
        2 => Opt::new(tcx, 2).start(),
        _ => {}
    }

    let _rcanary: Option<rCanary> = if callback.is_rcanary_enabled() {
        let mut heap = OwnedHeapAnalyzer::new(tcx);
        heap.run();
        let adt_owner = heap.get_all_items();
        let mut rcx = rCanary::new(tcx, adt_owner);
        rcx.start();
        Some(rcx)
    } else {
        None
    };

    if callback.is_safedrop_enabled() {
        SafeDrop::new(tcx).start();
    }

    if callback.is_show_mir_enabled() {
        ShowMir::new(tcx).start();
    }

    if callback.is_show_mir_dot_enabled() {
        ShowMir::new(tcx).start_generate_dot();
    }

    if callback.is_ssa_transform_enabled() {
        SSATrans::new(tcx, false).start();
    }

    let x = callback.is_upg_enabled();
    match x {
        1 => UPGAnalysis::new(tcx).start(TargetCrate::Other),
        2 => UPGAnalysis::new(tcx).start(TargetCrate::Std),
        _ => {}
    }

    if callback.is_verify_enabled() {
        let check_level = CheckLevel::Medium;
        SenryxCheck::new(tcx, 2).start(check_level, true);
    }

    if callback.is_verify_std_enabled() {
        SenryxCheck::new(tcx, 2).start_analyze_std_func();
        // SenryxCheck::new(tcx, 2).generate_uig_by_def_id();
    }

    if callback.is_infer_enabled() {
        let check_level = CheckLevel::Medium;
        SenryxCheck::new(tcx, 2).start(check_level, false);
    }

    if callback.is_scan_enabled() {
        ScanAnalysis::new(tcx).run();
    }
}
