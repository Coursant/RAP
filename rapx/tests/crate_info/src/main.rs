/// A public safe function that performs a bounds-checked slice read.
pub fn read_elem(data: &[u32], idx: usize) -> u32 {
    data[idx]
}

/// A private safe function with no unsafe operations.
fn add(a: i32, b: i32) -> i32 {
    a + b
}

/// A public unsafe function that dereferences a raw pointer.
pub unsafe fn read_raw(ptr: *const u32) -> u32 {
    *ptr
}

/// A safe function that calls an unsafe function internally.
pub fn safe_wrapper(x: u32) -> u32 {
    let val = x;
    unsafe { read_raw(&val as *const u32) }
}

fn main() {
    let data = [1u32, 2, 3, 4];
    let _ = read_elem(&data, 1);
    let _ = add(1, 2);
    let _ = safe_wrapper(42);
}
