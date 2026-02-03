#[inline(never)]
#[allow(dead_code)]

fn two_slices_mut<'a>(x: &'a mut [usize], y: &'a mut [usize]) {
    unsafe {
    for i in 0..y.len() {
        x[i] += 1;
        y[i] += 1;
    }}
}
fn main() {
    let mut x = [0; 10];
    let mut y = [0; 10];
    // two_slices_mut(&mut x[0..5], &mut y[0..4]);
    let mut z = &mut x[1..9];;


    unsafe {
    for i in 0..z.len() {
        x[i] += 1;
        y[i] += 1;
    }
}



    // let slice_index = 5;
    // let slice = &mut x[1..slice_index];
    // for i in 0..slice.len() {
    //     slice[i] = i+1 ;
    // }
}
