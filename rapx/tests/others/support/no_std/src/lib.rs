#![no_std]

// no_std crates without using alloc will never find dealloc:
// 13:57:47|RAP|WARN|: Intrinsic functions is incompletely retrieved.
// 1 fn ids are not found: [
//     [
//         "std::alloc::dealloc",
//         "alloc::alloc::dealloc",
//     ],
// ]
// extern crate alloc;
