//! A comprehensive taxonomy of Bounds Check Elimination (BCE) patterns in Rust.
//! This file is designed for empirical analysis using LLVM IR and Assembly emission.
//!
//! How to verify whether a bounds check is eliminated by LLVM (release only):
//! 1. Build this crate with release optimization and emit LLVM IR:
//!    `cargo rustc --release --lib -- --emit=llvm-ir`
//! 2. Open `target/release/deps/*.ll`, locate the target function (we use
//!    `#[no_mangle]` below to make function names easy to find).
//! 3. If that function body no longer contains a call path to
//!    `panic_bounds_check`/`panic_bounds_check`-related symbols, the bounds check
//!    is eliminated; if such panic path is still present, the check is retained.
//! 4. Always judge based on release artifacts: debug builds intentionally retain
//!    many checks and are not suitable for BCE conclusions.

// ============================================================================
// Category 1: Static Elimination via LLVM Passes (BCE Success)
// ============================================================================

/// Pattern 1.1: Strict Upper-Bound Loop
/// LLVM's Scalar Evolution (SCEV) pass proves that the induction variable `i`
/// never exceeds `slice.len()`. The bounds check is completely eliminated.
#[no_mangle]
pub fn bce_success_strict_loop(slice: &[i32]) -> i32 {
    let mut sum = 0;
    for i in 0..slice.len() {
        sum += slice[i]; // No bounds check
    }
    sum
}

/// Pattern 1.2: Min-Length Aggregation (Zipping)
/// SCEV recognizes that the loop bound is strictly constrained by the minimum 
/// length of both slices. Thus, accesses to both `a` and `b` are proven safe.
#[no_mangle]
pub fn bce_success_min_length_zip(a: &[i32], b: &[i32]) -> i32 {
    let mut sum = 0;
    let len = std::cmp::min(a.len(), b.len());
    for i in 0..len {
        sum += a[i] + b[i]; // No bounds check for either slice
    }
    sum
}

/// Pattern 2.1: Upfront Assertion Dominance
/// The control flow graph (CFG) dictates that if `i >= slice.len()`, the program 
/// will panic at the assert. Because this assert strictly dominates the subsequent 
/// access, LLVM's EarlyCSE removes the implicit bounds check.
#[no_mangle]
pub fn bce_success_assert_dominance(slice: &[i32], i: usize) -> i32 {
    assert!(i < slice.len());
    slice[i] // No bounds check
}

/// Pattern 2.2: Sub-slicing / Slice Down
/// By explicitly downcasting to a fixed-size array slice, the bounds check is 
/// performed exactly once during the creation of `sub`. Subsequent hardcoded 
/// indexing is proven safe.
#[no_mangle]
pub fn bce_success_sub_slicing(slice: &[i32]) -> i32 {
    if slice.len() >= 4 {
        let sub = &slice[0..4];
        sub[0] + sub[1] + sub[2] + sub[3] // No bounds check for these 4 accesses
    } else {
        0
    }
}

/// Pattern 3.1: Modulo Arithmetic Bound
/// LLVM's Correlated Value Propagation (CVP) utilizes mathematical invariants.
/// The modulo operator guarantees that `idx` is strictly less than `slice.len()`.
#[no_mangle]
pub fn bce_success_modulo_bound(slice: &[i32], raw_index: usize) -> i32 {
    if slice.is_empty() { return 0; }
    let idx = raw_index % slice.len();
    slice[idx] // No bounds check
}

/// Pattern 3.2: Bitmask Bound (Power of 2)
/// Common in hash maps and ring buffers. The bitwise AND guarantees the index 
/// stays within the fixed bounds. InstCombine and CVP eliminate the check.
#[no_mangle]
pub fn bce_success_bitmask_bound(raw_index: usize) -> i32 {
    let array = [1, 2, 3, 4, 5, 6, 7, 8]; // Length is 8 (2^3)
    let idx = raw_index & 7; // Guarantees idx is within 0..=7
    array[idx] // No bounds check
}

/// Pattern 4.1: Compile-time Constant Access
/// For fixed-size arrays, if the index is a constant, bounds verification 
/// happens during compilation. No runtime check is emitted.
#[no_mangle]
pub fn bce_success_constant_folding() -> i32 {
    let arr = [10, 20, 30];
    arr[1] // Verified at compile time
}

// ============================================================================
// Category 2: Avoidance via Idiomatic Semantics (No BCE Needed)
// ============================================================================

/// Pattern: Idiomatic Iterator Traversal
/// Does not rely on indexing. It uses internal pointer arithmetic (`ptr` and `end`).
/// Safety is guaranteed by construction, bypassing the bounds checking system entirely.
#[no_mangle]
pub fn bce_avoidance_iterator(slice: &[i32]) -> i32 {
    let mut sum = 0;
    for &val in slice.iter() {
        sum += val; // No index, no bounds check
    }
    sum
}

// ============================================================================
// Category 3: Optimization Barriers (BCE Failure / Anti-Patterns)
// ============================================================================

/// Anti-Pattern 1: Indirect / Data-Dependent Indexing
/// LLVM cannot predict the values stored inside `indices`. Therefore, while the 
/// access to `indices[i]` might be bounds-checked-eliminated, the access to 
/// `data[idx]` absolutely requires a runtime bounds check.
#[no_mangle]
pub fn bce_failure_indirect_indexing(data: &[i32], indices: &[usize]) -> i32 {
    let mut sum = 0;
    for i in 0..indices.len() {
        let idx = indices[i]; // BCE succeeds here
        sum += data[idx];     // BCE FAILS here: bounds check retained
    }
    sum
}

/// Anti-Pattern 2: Mutation and Length Invalidation
/// Because the vector is mutated (`push`) inside the loop, LLVM's alias analysis 
/// must conservatively assume that the length or memory allocation might change. 
/// Consequently, it cannot cache the bounds check verification.
#[no_mangle]
pub fn bce_failure_mutation_invalidation(v: &mut Vec<i32>) {
    let len = v.len();
    for i in 0..len {
        let val = v[i]; // BCE FAILS here: length might have changed
        v.push(val * 2);
    }
}

/// Anti-Pattern 3: Complex / Non-linear Induction Variables
/// While `step_by` has seen improvements in recent Rust versions, a highly dynamic 
/// or complex step variable breaks SCEV's ability to mathematically model the upper 
/// bound. Integer overflow risks force the compiler to retain the check.
#[no_mangle]
pub fn bce_failure_complex_induction(slice: &[i32], dynamic_step: usize) -> i32 {
    if dynamic_step == 0 { return 0; }
    let mut sum = 0;
    for i in (0..slice.len()).step_by(dynamic_step) {
        sum += slice[i]; // BCE likely FAILS: compiler cannot guarantee safety
    }
    sum
}

/// Helper function to simulate an opaque boundary
#[inline(never)]
fn get_opaque_index() -> usize {
    42
}

/// Anti-Pattern 4: Opaque Function Boundaries
/// `get_opaque_index` is explicitly not inlined. The compiler loses local context
/// and treats the returned index as an unknown value. The bounds check is retained.
#[no_mangle]
pub fn bce_failure_opaque_boundary(slice: &[i32]) -> i32 {
    let idx = get_opaque_index();
    slice[idx] // BCE FAILS: index origin is opaque to this compilation unit
}
