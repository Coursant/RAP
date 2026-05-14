/// Brings elements in `bytes` forward until `\n` (inclusive) or end of `source`.
///
/// `read_idx` must be greater than or equal to `write_idx`.
const fn copy_forward_until_eol(
    bytes: &mut [u8],
    mut read_idx: usize,
    mut write_idx: usize,
) -> (usize, usize) {
    assert!(read_idx >= write_idx);/// assert
    while read_idx < bytes.len() {
        let value = bytes[read_idx];
        bytes[write_idx] = value;
        read_idx += 1;
        write_idx += 1;
        if value == b'\n' {
            break;
        }
    }
    (read_idx, write_idx)
}