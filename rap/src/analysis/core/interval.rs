pub mod interval_analysis;
// impl rustc_driver::Callbacks for CustomCompilerCalls {
//     fn after_expansion<'tcx>(
//         &mut self,
//         _compiler: &rustc_interface::interface::Compiler,
//         queries: &'tcx Queries<'tcx>,
//     ) -> Compilation {
//         queries.global_ctxt().unwrap().enter(|tcx| {
//             for body_id in tcx.hir().body_owners() {
//                 let mir = tcx.mir_built(body_id);
//                 let mir = mir.borrow();

//                 // The set of locals in a MIR body that do not have `StorageLive`/`StorageDead` annotations.
//                 // These locals have fixed storage for the duration of the body.
//                 let always_live_locals = always_storage_live_locals(&mir);
//                 eprintln!("always live: {always_live_locals:?}");
//                 // Compute `MaybeStorageDead` dataflow to check that we only replace when the pointee is
//                 // definitely live.
//                 let mut maybe_dead = MaybeStorageDead::new(always_live_locals)
//                     .into_engine(tcx, &mir)
//                     .iterate_to_fixpoint()
//                     .into_results_cursor(&mir);

//                 for (bb, _block) in mir.basic_blocks.iter_enumerated() {
//                     maybe_dead.seek_before_primary_effect(Location {
//                         block: bb,
//                         statement_index: 0,
//                     });
//                     let input_state = maybe_dead.get();
//                     println!("{bb:?} (input): {:?}", input_state);
//                     maybe_dead.seek_after_primary_effect(mir.terminator_loc(bb));
//                     let output_state = maybe_dead.get();
//                     println!("{bb:?} (output): {:?}", output_state);
//                 }
//             }
//         });

//         Compilation::Stop
//     }
// }

// fn main() -> rustc_interface::interface::Result<()> {
//     let args = env::args().collect::<Vec<_>>();
    
//     rustc_driver::RunCompiler::new(&args, &mut CustomCompilerCalls).run()
// }