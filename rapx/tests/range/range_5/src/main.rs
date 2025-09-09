
use std::slice;

fn main() {

    let mut arr = Box::new([0; 10]);
    for i in 0..arr.len() {
        arr[i] = (i * i) as i32;
    }

    let slice: &[i32] = &arr[2..7];
    for val in slice.iter() {
        let _ = val; // use the value (placeholder)
    }
}

