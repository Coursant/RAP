fn foo1(x: i32) -> i32 {
    let a = x + 1;
    let y = x;
    let mut result ;
    let _b = a - y; // _25.0 [1,1] can be inferred before range analysis
    if a >= y {    // always true
        result =  a;
    } else {
        result =  y;
    }
    return result;  // result is always a, but its upper/lower bound 
                    // symbexpr is hard to be inferred without range analysis
}

fn foo2(x: i32) {
    let mut k = 0;
    while k < x {
        let mut i = 0;
        let mut j = k;
        while i < j {
            i += 1;
            j -= 1;
        }
        k += 1;
    }
}
fn main(){
    let y = 2;
    let x = y;
    // foo1(2);
}