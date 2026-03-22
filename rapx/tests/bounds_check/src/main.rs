pub fn read_slice(data: &[u8], index: usize) -> u8 {
    data[index]
}

pub unsafe fn read_slice_unsafe(data: &[u8], index: usize) -> u8 {
    data[index]
}

pub fn read_array(arr: [i32; 4], idx: usize) -> i32 {
    arr[idx]
}

fn main() {
    let v = vec![1u8, 2u8, 3u8];
    let _ = read_slice(&v, 0);
    let _ = read_array([1, 2, 3, 4], 1);
    let _ = unsafe { read_slice_unsafe(&v, 0) };
}
