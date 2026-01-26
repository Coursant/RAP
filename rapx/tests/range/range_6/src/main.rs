
pub fn main(){
    let pieces : [u8; 8]= [42; 8];
    let mut i = 0;
    let len = pieces.len();
    while i < len {
        let val = pieces[i]; 
        if val > 128 {
            i += 2;
        } else {
            i += 1;
        }
    }}