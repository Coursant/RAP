#![allow(dead_code)]
#![allow(clippy::len_without_is_empty)]
#![allow(clippy::manual_range_contains)]

//! Minimal runnable range-analysis cases reduced from functions listed in the
//! real-crate source table.  Each module keeps the source crate/function shape
//! where it is useful, but removes unrelated dependencies so this crate can run
//! as a standalone RAP testcase.

pub mod zlib_rs {
    pub mod crc32 {
        pub mod braid {
            // Source: zlib-rs, crc32::braid::crc32_braid
            // Bounds check:
            // - Sites: `bytes[i]`, `bytes[i + 1]`, `bytes[i + 2]`, `bytes[i + 3]`.
            // - Semantically removable: yes.
            // - Pattern: locally guarded or fixed-size in-bounds access.
            // - Reason: `i + 4 <= bytes.len()` proves the unrolled reads are in bounds;
            //   the tail read is guarded by `i < bytes.len()`.
            pub fn crc32_braid(bytes: &[u8], mut crc: u32) -> u32 {
                let mut i = 0;
                while i + 4 <= bytes.len() {
                    crc = crc.rotate_left(5) ^ bytes[i] as u32;
                    crc = crc.rotate_left(5) ^ bytes[i + 1] as u32;
                    crc = crc.rotate_left(5) ^ bytes[i + 2] as u32;
                    crc = crc.rotate_left(5) ^ bytes[i + 3] as u32;
                    i += 4;
                }
                while i < bytes.len() {
                    crc ^= bytes[i] as u32;
                    i += 1;
                }
                crc
            }
        }

        // Source: zlib-rs, crc32::combine::x2nmodp
        pub mod combine {
            // Bounds check:
            // - Sites: none.
            // - Semantically removable: not applicable.
            // - Pattern: no runtime indexing in this function.
            // - Reason: this testcase only uses scalar integer and bit operations.
            pub fn x2nmodp(mut n: u64, mut p: u32) -> u32 {
                while n != 0 {
                    if n & 1 != 0 {
                        p = p.rotate_left(1) ^ 0xedb8_8320;
                    }
                    n >>= 1;
                }
                p
            }
        }

        // Source: zlib-rs, crc32::pclmulqdq::Accumulator::progress
        pub mod pclmulqdq {
            pub struct Accumulator {
                lanes: [u64; 4],
                pos: usize,
            }

            impl Accumulator {
                pub fn new() -> Self {
                    Self {
                        lanes: [0; 4],
                        pos: 0,
                    }
                }

                // Bounds check:
                // - Sites: `self.lanes[lane]`, `input[i]`, and the fixed final
                //   reads `self.lanes[0..=3]`.
                // - Semantically removable: yes.
                // - Pattern: locally guarded or fixed-size in-bounds access.
                // - Reason: `lane = self.pos & 3` is always within the four-lane
                //   array, `i < input.len()` guards the input read, and the final
                //   lane indexes are constants within `[u64; 4]`.
                pub fn progress(&mut self, input: &[u8]) -> u64 {
                    let mut i = 0;
                    while i < input.len() {
                        let lane = self.pos & 3;
                        self.lanes[lane] ^= input[i] as u64;
                        self.pos += 1;
                        i += 1;
                    }
                    self.lanes[0] ^ self.lanes[1] ^ self.lanes[2] ^ self.lanes[3]
                }
            }
        }
    }

    pub mod deflate {
        pub mod algorithm {
            // Source: zlib-rs, deflate::algorithm::medium::fizzle_matches
            pub mod medium {
                // Bounds check:
                // - Sites: `input[pos + len]` and `input[pos]`.
                // - Semantically removable: yes.
                // - Pattern: locally guarded or fixed-size in-bounds access.
                // - Reason:`max = input.len()-pos` `max > 0` implies `pos < input.len()`, and
                //   `len < max` keeps `pos + len` in bounds.
                pub fn fizzle_matches(input: &[u8], pos: usize, limit: usize) -> usize {
                    let mut len = 0;
                    let max = input.len().saturating_sub(pos).min(limit);
                    while len < max && input[pos + len] == input[pos] {
                        len += 1;
                    }
                    len
                }
            }

            // Source: zlib-rs, deflate::algorithm::rle::deflate_rle
            pub mod rle {
                // Bounds check:
                // - Sites: `input[read]`, `input[read + run]`, `out[write]`,
                //   and `out[write + 1]`.
                // - Semantically removable: yes.
                // - Pattern: locally guarded or fixed-size in-bounds access.
                // - Reason: the outer guard bounds `read`, `write`, and `write + 1`;
                //   the inner guard bounds `read + run`.
                pub fn deflate_rle(input: &[u8], out: &mut [u8]) -> usize {
                    let mut read = 0;
                    let mut write = 0;
                    while read < input.len() && write + 1 < out.len() {
                        let byte = input[read];
                        let mut run = 1usize;
                        while read + run < input.len() && run < 255 && input[read + run] == byte {
                            run += 1;
                        }
                        out[write] = run as u8;
                        out[write + 1] = byte;
                        write += 2;
                        read += run;
                    }
                    write
                }
            }

            // Source: zlib-rs, deflate::algorithm::slow::deflate_slow
            pub mod slow {
                // Bounds check:
                // - Sites: `input[probe + len]` and `input[pos + len]`.
                // - Semantically removable: yes.
                // - Pattern: locally guarded or fixed-size in-bounds access.
                // - Reason: `pos < input.len()` and `pos + len < input.len()`
                //   bound the right side; `probe < pos` plus the overlap break
                //   keeps `probe + len` within the already-scanned prefix.
                pub fn deflate_slow(input: &[u8]) -> usize {
                    let mut best = 0;
                    let mut pos = 0;
                    while pos < input.len() {
                        let mut probe = 0;
                        while probe < pos {
                            let mut len = 0;
                            while pos + len < input.len() && input[probe + len] == input[pos + len]
                            {
                                len += 1;
                                if probe + len >= pos {
                                    break;
                                }
                            }
                            best = best.max(len);
                            probe += 1;
                        }
                        pos += 1;
                    }
                    best
                }
            }
        }

        // Source: zlib-rs, deflate::sym_buf::SymBuf::<A>::push
        pub mod sym_buf {
            pub struct SymBuf<const N: usize> {
                data: [u16; N],
                len: usize,
            }

            impl<const N: usize> SymBuf<N> {
                pub fn new() -> Self {
                    Self {
                        data: [0; N],
                        len: 0,
                    }
                }

                // Bounds check:
                // - Sites: `self.data[self.len]`.
                // - Semantically removable: yes.
                // - Pattern: locally guarded or fixed-size in-bounds access.
                // - Reason: the early return proves `self.len < self.data.len()`
                //   before the write.
                pub fn push(&mut self, value: u16) -> bool {
                    if self.len == self.data.len() {
                        return false;
                    }
                    self.data[self.len] = value;
                    self.len += 1;
                    true
                }
            }
        }

        // Source: zlib-rs, deflate::State::<A>::detect_data_type
        pub struct State {
            freqs: [u32; 32],
        }

        impl State {
            pub fn new(freqs: [u32; 32]) -> Self {
                Self { freqs }
            }

            // Bounds check:
            // - Sites: `self.freqs[n]`.
            // - Semantically removable: yes.
            // - Pattern: locally guarded or fixed-size in-bounds access.
            // - Reason: the loop guard `n < self.freqs.len()` proves the index is
            //   within the fixed-size frequency array.
            pub fn detect_data_type(&self) -> u8 {
                let mut n = 0;
                while n < self.freqs.len() {
                    if n < 7 && self.freqs[n] != 0 {
                        return 0;
                    }
                    n += 1;
                }
                1
            }
        }

        // Source: zlib-rs, deflate::build_tree
        // Bounds check:
        // - Sites: `freqs[n]` and `heap[h]`.
        // - Semantically removable: yes.
        // - Pattern: locally guarded or fixed-size in-bounds access.
        // - Reason: the loop guard simultaneously proves `n < freqs.len()` and
        //   `h < heap.len()` before each read/write.
        pub fn build_tree(freqs: &[u16], heap: &mut [usize]) -> usize {
            let mut n = 0;
            let mut h = 0;
            while n < freqs.len() && h < heap.len() {
                if freqs[n] != 0 {
                    heap[h] = n;
                    h += 1;
                }
                n += 1;
            }
            h
        }

        // Source: zlib-rs, deflate::gen_bitlen
        // Bounds check:
        // - Sites: `bitlen[i]` and `freqs[i]`.
        // - Semantically removable: yes.
        // - Pattern: locally guarded or fixed-size in-bounds access.
        // - Reason: the loop guard proves `i` is within both slices before each
        //   read and write.
        pub fn gen_bitlen(freqs: &[u16], bitlen: &mut [u8], max_bits: u8) {
            let mut i = 0;
            while i < freqs.len() && i < bitlen.len() {
                bitlen[i] = if freqs[i] == 0 {
                    0
                } else {
                    ((freqs[i] as u8) & max_bits).max(1)
                };
                i += 1;
            }
        }

        // Source: zlib-rs, deflate::Heap::initialize / deflate::Heap::pqremove
        pub struct Heap {
            data: [usize; 64],
            len: usize,
        }

        impl Heap {
            // Bounds check:
            // - Sites: `weights[i]` and `heap.data[heap.len]`.
            // - Semantically removable: yes.
            // - Pattern: locally guarded or fixed-size in-bounds access.
            // - Reason: the loop guard bounds both the input index and the current
            //   heap length before the write.
            pub fn initialize(weights: &[u16]) -> Self {
                let mut heap = Heap {
                    data: [0; 64],
                    len: 0,
                };
                let mut i = 0;
                while i < weights.len() && heap.len < heap.data.len() {
                    if weights[i] != 0 {
                        heap.data[heap.len] = i;
                        heap.len += 1;
                    }
                    i += 1;
                }
                heap
            }

            // Bounds check:
            // - Sites: `self.data[0]`, `self.data[i - 1]`, and `self.data[i]`.
            // - Semantically removable: yes.
            // - Pattern: locally guarded or fixed-size in-bounds access.
            // - Reason: the empty check proves index 0 exists; the loop starts at
            //   1 and maintains `i < self.len`, so both shift indexes are valid.
            pub fn pqremove(&mut self) -> Option<usize> {
                if self.len == 0 {
                    return None;
                }
                let result = self.data[0];
                let mut i = 1;
                while i < self.len {
                    self.data[i - 1] = self.data[i];
                    i += 1;
                }
                self.len -= 1;
                Some(result)
            }
        }
    }

    pub mod inflate {
        // Source: zlib-rs, inflate::inffast::back / inflate::inffast::inflate_fast_back
        pub mod inffast {
            // Bounds check:
            // - Sites: `out[i]` and `window[start + (i % dist)]`.
            // - Semantically removable: yes.
            // - Pattern: locally guarded or fixed-size in-bounds access.
            // - Reason: `i < out.len()` bounds the output; `dist > 0`,
            //   `dist <= window.len()`, and modulo arithmetic keep the window
            //   index in `start..window.len()`.
            pub fn back(window: &[u8], dist: usize, len: usize, out: &mut [u8]) -> usize {
                if dist == 0 || dist > window.len() {
                    return 0;
                }
                let mut i = 0;
                let start = window.len() - dist;
                while i < len && i < out.len() {
                    out[i] = window[start + (i % dist)];
                    i += 1;
                }
                i
            }

            // Bounds check:
            // - Sites: `input[i]` and `out[w]`.
            // - Semantically removable: yes.
            // - Pattern: locally guarded or fixed-size in-bounds access.
            // - Reason: the outer guard bounds `input[i]`; the inner guard bounds
            //   `out[w]`, and `i` is not changed inside the inner loop.
            pub fn inflate_fast_back(input: &[u8], out: &mut [u8]) -> usize {
                let mut i = 0;
                let mut w = 0;
                while i < input.len() && w < out.len() {
                    let len = input[i] as usize & 7;
                    let mut j = 0;
                    while j < len && w < out.len() {
                        out[w] = input[i];
                        w += 1;
                        j += 1;
                    }
                    i += 1;
                }
                w
            }
        }

        pub struct State {
            table: [u16; 16],
            len: usize,
        }

        impl State {
            // Source: zlib-rs, inflate::State::len_and_friends
            // Bounds check:
            // - Sites: `self.table[self.len]` and `input[i]`.
            // - Semantically removable: yes.
            // - Pattern: locally guarded or fixed-size in-bounds access.
            // - Reason: the loop guard proves both `self.len < self.table.len()`
            //   and `i < input.len()`.
            pub fn len_and_friends(&mut self, input: &[u8]) -> usize {
                let mut i = 0;
                while i < input.len() && self.len < self.table.len() {
                    self.table[self.len] = input[i] as u16;
                    self.len += 1;
                    i += 1;
                }
                self.len
            }

            // Source: zlib-rs, inflate::State::dispatch
            // Bounds check:
            // - Sites: `input[consumed]` and `self.table[self.len]`.
            // - Semantically removable: yes.
            // - Pattern: locally guarded or fixed-size in-bounds access.
            // - Reason: `consumed < input.len()` guards the input read; the write
            //   is guarded by `self.len < self.table.len()`.
            pub fn dispatch(&mut self, input: &[u8]) -> usize {
                let mut consumed = 0;
                while consumed < input.len() {
                    match input[consumed] & 3 {
                        0 => self.len = self.len.saturating_sub(1),
                        1 => {
                            if self.len < self.table.len() {
                                self.table[self.len] = consumed as u16;
                                self.len += 1;
                            }
                        }
                        _ => break,
                    }
                    consumed += 1;
                }
                consumed
            }
        }
    }
}

pub mod fdeflate {
    use core::marker::PhantomData;

    // Source: fdeflate, compress::Compressor::<W>::write_len/write_code
    pub mod compress {
        use super::PhantomData;

        pub struct Compressor<W> {
            bits: u64,
            used: u8,
            sink: Vec<u8>,
            _writer: PhantomData<W>,
        }

        impl<W> Compressor<W> {
            pub fn new() -> Self {
                Self {
                    bits: 0,
                    used: 0,
                    sink: Vec::new(),
                    _writer: PhantomData,
                }
            }

            // Bounds check:
            // - Sites: none.
            // - Semantically removable: not applicable.
            // - Pattern: no runtime indexing in this function.
            // - Reason: `write_len` delegates to `write_code`, which writes to a
            //   `Vec` through `push` rather than indexing.
            pub fn write_len(&mut self, mut len: usize) -> usize {
                let mut written = 0;
                while len > 0 {
                    self.write_code((len & 0xff) as u16, 8);
                    len >>= 8;
                    written += 1;
                }
                written
            }

            // Bounds check:
            // - Sites: none.
            // - Semantically removable: not applicable.
            // - Pattern: no runtime indexing in this function.
            // - Reason: the function updates scalar bit state and appends with
            //   `Vec::push`, so no Rust indexing check is emitted here.
            pub fn write_code(&mut self, code: u16, bits: u8) {
                self.bits |= (code as u64) << self.used;
                self.used += bits;
                while self.used >= 8 {
                    self.sink.push(self.bits as u8);
                    self.bits >>= 8;
                    self.used -= 8;
                }
            }
        }
    }

    // Source: fdeflate, decompress::Decompressor::read_code
    pub mod decompress {
        pub struct Decompressor<'a> {
            input: &'a [u8],
            byte: usize,
            bit: u8,
        }

        impl<'a> Decompressor<'a> {
            pub fn new(input: &'a [u8]) -> Self {
                Self {
                    input,
                    byte: 0,
                    bit: 0,
                }
            }

            // Bounds check:
            // - Sites: `self.input[self.byte]`.
            // - Semantically removable: yes.
            // - Pattern: locally guarded or fixed-size in-bounds access.
            // - Reason: the explicit `self.byte >= self.input.len()` check returns
            //   before every indexed read.
            pub fn read_code(&mut self, bits: u8) -> Option<u16> {
                let mut out = 0u16;
                let mut n = 0;
                while n < bits {
                    if self.byte >= self.input.len() {
                        return None;
                    }
                    out |= (((self.input[self.byte] >> self.bit) & 1) as u16) << n;
                    self.bit += 1;
                    if self.bit == 8 {
                        self.bit = 0;
                        self.byte += 1;
                    }
                    n += 1;
                }
                Some(out)
            }
        }
    }

    // Source: fdeflate, compute_code_lengths / compute_codes
    // Bounds check:
    // - Sites: `lens[i]` and `freqs[i]`.
    // - Semantically removable: yes.
    // - Pattern: locally guarded or fixed-size in-bounds access.
    // - Reason: the loop guard proves `i` is within both slices.
    pub fn compute_code_lengths(freqs: &[u16], lens: &mut [u8]) {
        let mut i = 0;
        while i < freqs.len() && i < lens.len() {
            lens[i] = if freqs[i] == 0 {
                0
            } else {
                1 + (freqs[i] % 15) as u8
            };
            i += 1;
        }
    }

    // Bounds check:
    // - Sites: `codes[i]` and `lens[i]`.
    // - Semantically removable: yes.
    // - Pattern: locally guarded or fixed-size in-bounds access.
    // - Reason: the loop guard proves `i` is within both slices.
    pub fn compute_codes(lens: &[u8], codes: &mut [u16]) {
        let mut code = 0u16;
        let mut i = 0;
        while i < lens.len() && i < codes.len() {
            codes[i] = code;
            if lens[i] != 0 {
                code = code.wrapping_add(1 << (lens[i] - 1).min(15));
            }
            i += 1;
        }
    }
}

pub mod regex_automata {
    // Source: regex-automata, dfa::dense::MatchStates::<T>::validate
    pub mod dfa {
        pub mod dense {
            pub struct MatchStates<T> {
                states: Vec<T>,
                stride: usize,
            }

            impl<T: Copy + PartialEq> MatchStates<T> {
                pub fn new(states: Vec<T>, stride: usize) -> Self {
                    Self { states, stride }
                }

                // Bounds check:
                // - Sites: `self.states[i]`.
                // - Semantically removable: yes.
                // - Pattern: locally guarded or fixed-size in-bounds access.
                // - Reason: `i < self.states.len()` bounds the vector read; note
                //   that `stride != 0` is a separate arithmetic precondition for
                //   the modulo expression, not a bounds-check condition.
                pub fn validate(&self, dead: T) -> bool {
                    let mut i = 0;
                    while i < self.states.len() {
                        if self.states[i] == dead && i % self.stride != 0 {
                            return false;
                        }
                        i += 1;
                    }
                    true
                }
            }
        }

        pub mod accel {
            // Source: regex-automata, dfa::accel::Accels::<A>::needs/len/from_bytes/from_bytes_unchecked
            pub struct Accels<const N: usize> {
                bytes: [u8; N],
                len: usize,
            }

            impl<const N: usize> Accels<N> {
                // Bounds check:
                // - Sites: `self.bytes[i]`.
                // - Semantically removable: conditional.
                // - Pattern: caller or constructor invariant.
                // - Reason: the loop proves `i < self.len`, and constructors keep
                //   `self.len <= N`; direct construction would need that invariant.
                pub fn needs(&self, byte: u8) -> bool {
                    let mut i = 0;
                    while i < self.len {
                        if self.bytes[i] == byte {
                            return true;
                        }
                        i += 1;
                    }
                    false
                }

                // Bounds check:
                // - Sites: none.
                // - Semantically removable: not applicable.
                // - Pattern: no runtime indexing in this function.
                // - Reason: this accessor returns the stored length without indexing.
                pub fn len(&self) -> usize {
                    self.len
                }

                // Bounds check:
                // - Sites: `out.bytes[i]` and `bytes[i]`.
                // - Semantically removable: yes.
                // - Pattern: locally guarded or fixed-size in-bounds access.
                // - Reason: `bytes.len() <= N` is checked before copying, and the
                //   loop guard bounds the source slice.
                pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
                    if bytes.len() > N {
                        return None;
                    }
                    let mut out = Self {
                        bytes: [0; N],
                        len: bytes.len(),
                    };
                    let mut i = 0;
                    while i < bytes.len() {
                        out.bytes[i] = bytes[i];
                        i += 1;
                    }
                    Some(out)
                }

                // Bounds check:
                // - Sites: `out.bytes[i]` and `bytes[i]`.
                // - Semantically removable: yes.
                // - Pattern: locally guarded or fixed-size in-bounds access.
                // - Reason: `out.len` is clamped to `N` and is also no larger than
                //   `bytes.len()`, so both source and destination indexes are valid.
                pub unsafe fn from_bytes_unchecked(bytes: &[u8]) -> Self {
                    let mut out = Self {
                        bytes: [0; N],
                        len: bytes.len().min(N),
                    };
                    let mut i = 0;
                    while i < out.len {
                        out.bytes[i] = bytes[i];
                        i += 1;
                    }
                    out
                }
            }
        }
    }

    // Source: regex-automata, meta::strategy::Pre::<()>::from_prefix
    pub mod meta {
        pub mod strategy {
            pub struct Pre {
                prefix_len: usize,
            }

            impl Pre {
                // Bounds check:
                // - Sites: `haystack[i]` and `prefix[i]`.
                // - Semantically removable: yes.
                // - Pattern: locally guarded or fixed-size in-bounds access.
                // - Reason: the prefix-length check proves `prefix.len() <=
                //   haystack.len()`, and the loop guard proves `i < prefix.len()`.
                pub fn from_prefix(haystack: &[u8], prefix: &[u8]) -> Option<Self> {
                    if prefix.len() > haystack.len() {
                        return None;
                    }
                    let mut i = 0;
                    while i < prefix.len() {
                        if haystack[i] != prefix[i] {
                            return None;
                        }
                        i += 1;
                    }
                    Some(Self {
                        prefix_len: prefix.len(),
                    })
                }
            }
        }
    }

    // Source: regex-automata, util::utf8::is_word_byte / decode_last
    pub mod util {
        pub mod utf8 {
            // Bounds check:
            // - Sites: none.
            // - Semantically removable: not applicable.
            // - Pattern: no runtime indexing in this function.
            // - Reason: this testcase only classifies a scalar byte.
            pub fn is_word_byte(byte: u8) -> bool {
                matches!(byte, b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'_')
            }

            // Bounds check:
            // - Sites: `bytes[i]` and `bytes[i..]`.
            // - Semantically removable: yes.
            // - Pattern: locally guarded or fixed-size in-bounds access.
            // - Reason: `i` starts at `bytes.len()` and is decremented only while
            //   `i > 0`, so the byte read and suffix slice start are in bounds.
            pub fn decode_last(bytes: &[u8]) -> Option<char> {
                let mut i = bytes.len();
                while i > 0 {
                    i -= 1;
                    if bytes[i] < 128 {
                        return Some(bytes[i] as char);
                    }
                    if bytes[i] & 0b1100_0000 == 0b1100_0000 {
                        return core::str::from_utf8(&bytes[i..]).ok()?.chars().next();
                    }
                }
                None
            }
        }
    }
}

pub mod jpeg_decoder {
    pub mod arch {
        pub mod ssse3 {
            // Source: jpeg-decoder, arch::ssse3::idct / transpose8
            // Bounds check:
            // - Sites: `block[base]` and `block[base + col]`.
            // - Semantically removable: yes.
            // - Pattern: locally guarded or fixed-size in-bounds access.
            // - Reason: `row < 8`, `base = row * 8`, and `col < 8` prove all
            //   indexes are below 64.
            pub fn idct(block: &mut [i16; 64]) {
                let mut row = 0;
                while row < 8 {
                    let base = row * 8;
                    let dc = block[base];
                    let mut col = 0;
                    while col < 8 {
                        block[base + col] = block[base + col].wrapping_add(dc / 8);
                        col += 1;
                    }
                    row += 1;
                }
            }

            // Bounds check:
            // - Sites: the two indexes inside `block.swap(y * 8 + x, x * 8 + y)`.
            // - Semantically removable: yes.
            // - Pattern: locally guarded or fixed-size in-bounds access.
            // - Reason: both `x` and `y` stay in `0..8`, so both computed indexes
            //   stay in `0..64`.
            pub fn transpose8(block: &mut [i16; 64]) {
                let mut y = 0;
                while y < 8 {
                    let mut x = y + 1;
                    while x < 8 {
                        block.swap(y * 8 + x, x * 8 + y);
                        x += 1;
                    }
                    y += 1;
                }
            }
        }
    }

    pub mod decoder {
        // Source: jpeg-decoder, decoder::color_convert_line_*
        // Bounds check:
        // - Sites: delegated to `copy_stride`.
        // - Semantically removable: yes.
        // - Pattern: locally guarded or fixed-size in-bounds access.
        // - Reason: this wrapper uses `stride = 3`, and the helper guards all
        //   source and destination accesses.
        pub fn color_convert_line_rgb(input: &[u8], output: &mut [u8]) -> usize {
            copy_stride(input, output, 3)
        }

        // Bounds check:
        // - Sites: delegated to `copy_stride`.
        // - Semantically removable: yes.
        // - Pattern: locally guarded or fixed-size in-bounds access.
        // - Reason: this wrapper uses `stride = 3`, and the helper guards all
        //   source and destination accesses.
        pub fn color_convert_line_ycbcr(input: &[u8], output: &mut [u8]) -> usize {
            copy_stride(input, output, 3)
        }

        // Bounds check:
        // - Sites: delegated to `copy_stride`.
        // - Semantically removable: yes.
        // - Pattern: locally guarded or fixed-size in-bounds access.
        // - Reason: this wrapper uses `stride = 4`, and the helper guards all
        //   source and destination accesses.
        pub fn color_convert_line_ycck(input: &[u8], output: &mut [u8]) -> usize {
            copy_stride(input, output, 4)
        }

        // Bounds check:
        // - Sites: delegated to `copy_stride`.
        // - Semantically removable: yes.
        // - Pattern: locally guarded or fixed-size in-bounds access.
        // - Reason: this wrapper uses `stride = 4`, and the helper guards all
        //   source and destination accesses.
        pub fn color_convert_line_cmyk(input: &[u8], output: &mut [u8]) -> usize {
            copy_stride(input, output, 4)
        }

        // Bounds check:
        // - Sites: `output[o + c]` and `input[i + c]`.
        // - Semantically removable: yes.
        // - Pattern: locally guarded or fixed-size in-bounds access.
        // - Reason: `i + stride <= input.len()`, `o + stride <= output.len()`,
        //   and `c < stride` prove both indexed accesses are in bounds.
        fn copy_stride(input: &[u8], output: &mut [u8], stride: usize) -> usize {
            let mut i = 0;
            let mut o = 0;
            while i + stride <= input.len() && o + stride <= output.len() {
                let mut c = 0;
                while c < stride {
                    output[o + c] = input[i + c];
                    c += 1;
                }
                i += stride;
                o += stride;
            }
            o
        }
    }

    pub mod huffman {
        // Source: jpeg-decoder, huffman::HuffmanTable::new
        pub struct HuffmanTable {
            lookup: [u8; 256],
        }

        impl HuffmanTable {
            // Bounds check:
            // - Sites: `lengths[li]`, `lookup[key]`, and `values[vi]`.
            // - Semantically removable: yes.
            // - Pattern: locally guarded or fixed-size in-bounds access.
            // - Reason: the outer and inner loop guards bound all three indexes
            //   before each read or write.
            pub fn new(lengths: &[u8], values: &[u8]) -> Self {
                let mut lookup = [0u8; 256];
                let mut key = 0usize;
                let mut li = 0;
                let mut vi = 0;
                while li < lengths.len() && key < lookup.len() {
                    let count = lengths[li] as usize;
                    let mut n = 0;
                    while n < count && vi < values.len() && key < lookup.len() {
                        lookup[key] = values[vi];
                        key += 1;
                        vi += 1;
                        n += 1;
                    }
                    li += 1;
                }
                Self { lookup }
            }
        }
    }

    pub mod idct {
        // Source: jpeg-decoder, idct::dequantize_and_idct_block_8x8/4x4/2x2/1x1
        // Bounds check:
        // - Sites: delegated to `dequantize`.
        // - Semantically removable: yes.
        // - Pattern: locally guarded or fixed-size in-bounds access.
        // - Reason: this wrapper passes `n = 64`, which is exactly the array length.
        pub fn dequantize_and_idct_block_8x8(
            input: &[i16; 64],
            quant: &[i16; 64],
            out: &mut [i16; 64],
        ) {
            dequantize(input, quant, out, 64);
        }

        // Bounds check:
        // - Sites: delegated to `dequantize`.
        // - Semantically removable: yes.
        // - Pattern: locally guarded or fixed-size in-bounds access.
        // - Reason: this wrapper passes `n = 16`, below the array length.
        pub fn dequantize_and_idct_block_4x4(
            input: &[i16; 64],
            quant: &[i16; 64],
            out: &mut [i16; 64],
        ) {
            dequantize(input, quant, out, 16);
        }

        // Bounds check:
        // - Sites: delegated to `dequantize`.
        // - Semantically removable: yes.
        // - Pattern: locally guarded or fixed-size in-bounds access.
        // - Reason: this wrapper passes `n = 4`, below the array length.
        pub fn dequantize_and_idct_block_2x2(
            input: &[i16; 64],
            quant: &[i16; 64],
            out: &mut [i16; 64],
        ) {
            dequantize(input, quant, out, 4);
        }

        // Bounds check:
        // - Sites: delegated to `dequantize`.
        // - Semantically removable: yes.
        // - Pattern: locally guarded or fixed-size in-bounds access.
        // - Reason: this wrapper passes `n = 1`, below the array length.
        pub fn dequantize_and_idct_block_1x1(
            input: &[i16; 64],
            quant: &[i16; 64],
            out: &mut [i16; 64],
        ) {
            dequantize(input, quant, out, 1);
        }

        // Bounds check:
        // - Sites: `out[i]`, `input[i]`, and `quant[i]`.
        // - Semantically removable: conditional.
        // - Pattern: caller or constructor invariant.
        // - Reason: this helper is safe when callers pass `n <= 64`; all current
        //   wrappers satisfy that precondition.
        fn dequantize(input: &[i16; 64], quant: &[i16; 64], out: &mut [i16; 64], n: usize) {
            let mut i = 0;
            while i < n {
                out[i] = input[i].saturating_mul(quant[i]);
                i += 1;
            }
        }
    }
}

pub mod arrow_data {
    // Source: arrow-data, data::ArrayData::get_slice_memory_size
    pub mod data {
        pub struct ArrayData<'a> {
            buffers: &'a [&'a [u8]],
            offset: usize,
            len: usize,
        }

        impl<'a> ArrayData<'a> {
            pub fn new(buffers: &'a [&'a [u8]], offset: usize, len: usize) -> Self {
                Self {
                    buffers,
                    offset,
                    len,
                }
            }

            // Bounds check:
            // - Sites: `self.buffers[i]`.
            // - Semantically removable: yes.
            // - Pattern: locally guarded or fixed-size in-bounds access.
            // - Reason: `i < self.buffers.len()` guards each buffer lookup; the
            //   code computes sizes without indexing into the inner byte slices.
            pub fn get_slice_memory_size(&self) -> usize {
                let mut total = 0;
                let mut i = 0;
                while i < self.buffers.len() {
                    let start = self.offset.min(self.buffers[i].len());
                    let end = (start + self.len).min(self.buffers[i].len());
                    total += end - start;
                    i += 1;
                }
                total
            }
        }
    }

    // Source: arrow-data, equal::list::lengths_equal
    pub mod equal {
        pub mod list {
            // Bounds check:
            // - Sites: `left_offsets[i]`, `left_offsets[i - 1]`,
            //   `right_offsets[i]`, and `right_offsets[i - 1]`.
            // - Semantically removable: yes.
            // - Pattern: locally guarded or fixed-size in-bounds access.
            // - Reason: equal lengths are checked up front, and the loop starts at
            //   1 while maintaining `i < left_offsets.len()`.
            pub fn lengths_equal(left_offsets: &[usize], right_offsets: &[usize]) -> bool {
                if left_offsets.len() != right_offsets.len() {
                    return false;
                }
                let mut i = 1;
                while i < left_offsets.len() {
                    if left_offsets[i] - left_offsets[i - 1]
                        != right_offsets[i] - right_offsets[i - 1]
                    {
                        return false;
                    }
                    i += 1;
                }
                true
            }
        }
    }

    // Source: arrow-data, transform::run::get_last_run_end
    pub mod transform {
        pub mod run {
            // Bounds check:
            // - Sites: `runs[i]`.
            // - Semantically removable: yes.
            // - Pattern: locally guarded or fixed-size in-bounds access.
            // - Reason: every read is guarded by `i < runs.len()`.
            pub fn get_last_run_end(runs: &[usize], limit: usize) -> usize {
                let mut end = 0;
                let mut i = 0;
                while i < runs.len() && runs[i] <= limit {
                    end = runs[i];
                    i += 1;
                }
                end
            }
        }
    }
}

pub mod prost_reflect {
    // Source: prost-reflect, descriptor::build::resolve::unsafe_prepare
    pub mod descriptor {
        pub mod build {
            pub mod resolve {
                // Bounds check:
                // - Sites: `values[i]` and `remap[idx]`.
                // - Semantically removable: yes.
                // - Pattern: locally guarded or fixed-size in-bounds access.
                // - Reason: `i < values.len()` guards the values slice; the
                //   explicit `idx < remap.len()` guard bounds the remap lookup.
                pub fn unsafe_prepare(values: &mut [u32], remap: &[u32]) {
                    let mut i = 0;
                    while i < values.len() {
                        let idx = values[i] as usize;
                        if idx < remap.len() {
                            values[i] = remap[idx];
                        }
                        i += 1;
                    }
                }
            }
        }
    }
}

pub mod memchr {
    // Source: memchr, arch::all::twoway::Finder::find_small/large and reverse variants.
    pub mod arch {
        pub mod all {
            pub mod twoway {
                pub struct Finder<'a> {
                    needle: &'a [u8],
                }

                pub struct FinderRev<'a> {
                    needle: &'a [u8],
                }

                impl<'a> Finder<'a> {
                    pub fn new(needle: &'a [u8]) -> Self {
                        Self { needle }
                    }

                    // Bounds check:
                    // - Sites: delegated to `find_forward`.
                    // - Semantically removable: yes.
                    // - Pattern: locally guarded or fixed-size in-bounds access.
                    // - Reason: the helper checks the haystack/needle length
                    //   relation and bounds every comparison.
                    pub fn find_small(&self, haystack: &[u8]) -> Option<usize> {
                        find_forward(haystack, self.needle)
                    }

                    // Bounds check:
                    // - Sites: delegated to `find_forward`.
                    // - Semantically removable: yes.
                    // - Pattern: locally guarded or fixed-size in-bounds access.
                    // - Reason: the helper checks the haystack/needle length
                    //   relation and bounds every comparison.
                    pub fn find_large(&self, haystack: &[u8]) -> Option<usize> {
                        find_forward(haystack, self.needle)
                    }
                }

                impl<'a> FinderRev<'a> {
                    pub fn new(needle: &'a [u8]) -> Self {
                        Self { needle }
                    }

                    // Bounds check:
                    // - Sites: delegated to `find_reverse`.
                    // - Semantically removable: yes.
                    // - Pattern: locally guarded or fixed-size in-bounds access.
                    // - Reason: the helper checks the haystack/needle length
                    //   relation and bounds every comparison.
                    pub fn find_small(&self, haystack: &[u8]) -> Option<usize> {
                        find_reverse(haystack, self.needle)
                    }

                    // Bounds check:
                    // - Sites: delegated to `find_reverse`.
                    // - Semantically removable: yes.
                    // - Pattern: locally guarded or fixed-size in-bounds access.
                    // - Reason: the helper checks the haystack/needle length
                    //   relation and bounds every comparison.
                    pub fn find_large(&self, haystack: &[u8]) -> Option<usize> {
                        find_reverse(haystack, self.needle)
                    }
                }

                // Bounds check:
                // - Sites: `haystack[i + j]` and `needle[j]`.
                // - Semantically removable: yes.
                // - Pattern: locally guarded or fixed-size in-bounds access.
                // - Reason: `needle.len() <= haystack.len()`, `i + needle.len()
                //   <= haystack.len()`, and `j < needle.len()` bound both reads.
                fn find_forward(haystack: &[u8], needle: &[u8]) -> Option<usize> {
                    if needle.is_empty() || needle.len() > haystack.len() {
                        return None;
                    }
                    let mut i = 0;
                    while i + needle.len() <= haystack.len() {
                        let mut j = 0;
                        while j < needle.len() && haystack[i + j] == needle[j] {
                            j += 1;
                        }
                        if j == needle.len() {
                            return Some(i);
                        }
                        i += 1;
                    }
                    None
                }

                // Bounds check:
                // - Sites: `haystack[i + j]` and `needle[j]`.
                // - Semantically removable: yes.
                // - Pattern: locally guarded or fixed-size in-bounds access.
                // - Reason: after the length check, `i` starts at the last valid
                //   match position and only decreases; `j < needle.len()` bounds
                //   the compared bytes.
                fn find_reverse(haystack: &[u8], needle: &[u8]) -> Option<usize> {
                    if needle.is_empty() || needle.len() > haystack.len() {
                        return None;
                    }
                    let mut i = haystack.len() - needle.len();
                    loop {
                        let mut j = 0;
                        while j < needle.len() && haystack[i + j] == needle[j] {
                            j += 1;
                        }
                        if j == needle.len() {
                            return Some(i);
                        }
                        if i == 0 {
                            break;
                        }
                        i -= 1;
                    }
                    None
                }
            }

            pub mod substring {
                // Source: memchr, arch::all::substring::Suffix::reverse
                pub struct Suffix<'a> {
                    bytes: &'a [u8],
                }

                impl<'a> Suffix<'a> {
                    pub fn new(bytes: &'a [u8]) -> Self {
                        Self { bytes }
                    }

                    // Bounds check:
                    // - Sites: `self.bytes[self.bytes.len() - 1 - i]` and
                    //   `haystack[haystack.len() - 1 - i]`.
                    // - Semantically removable: yes.
                    // - Pattern: locally guarded or fixed-size in-bounds access.
                    // - Reason: the prefix length check proves the haystack is at
                    //   least as long as the suffix, and `i < self.bytes.len()`
                    //   keeps both reverse indexes valid.
                    pub fn reverse(&self, haystack: &[u8]) -> bool {
                        if self.bytes.len() > haystack.len() {
                            return false;
                        }
                        let mut i = 0;
                        while i < self.bytes.len() {
                            if self.bytes[self.bytes.len() - 1 - i]
                                != haystack[haystack.len() - 1 - i]
                            {
                                return false;
                            }
                            i += 1;
                        }
                        true
                    }
                }
            }
        }
    }
}

pub mod arrow_row {
    // Source: arrow-row, compute_list_view_bounds
    // Bounds check:
    // - Sites: `offsets[i]` and `sizes[i]`.
    // - Semantically removable: yes.
    // - Pattern: locally guarded or fixed-size in-bounds access.
    // - Reason: equal lengths are checked first, and `i < offsets.len()` bounds
    //   both slices.
    pub fn compute_list_view_bounds(offsets: &[usize], sizes: &[usize]) -> Option<(usize, usize)> {
        if offsets.len() != sizes.len() {
            return None;
        }
        let mut min = usize::MAX;
        let mut max = 0usize;
        let mut i = 0;
        while i < offsets.len() {
            min = min.min(offsets[i]);
            max = max.max(offsets[i].saturating_add(sizes[i]));
            i += 1;
        }
        Some((min, max))
    }
}

pub mod base64 {
    // Source: base64, engine::general_purpose::decode_suffix
    pub mod engine {
        pub mod general_purpose {
            // Bounds check:
            // - Sites: `input[i]` and `out[o]`.
            // - Semantically removable: yes.
            // - Pattern: locally guarded or fixed-size in-bounds access.
            // - Reason: the loop guard proves `i < input.len()` and `o < out.len()`.
            pub fn decode_suffix(input: &[u8], out: &mut [u8]) -> usize {
                let mut i = 0;
                let mut o = 0;
                while i < input.len() && input[i] != b'=' && o < out.len() {
                    out[o] = input[i] & 0x3f;
                    i += 1;
                    o += 1;
                }
                o
            }
        }
    }

    // Source: base64, alphabet::Alphabet::from_str_unchecked
    pub mod alphabet {
        pub struct Alphabet {
            bytes: [u8; 64],
        }

        impl Alphabet {
            // Bounds check:
            // - Sites: `bytes[i]` and `raw[i]`.
            // - Semantically removable: yes.
            // - Pattern: locally guarded or fixed-size in-bounds access.
            // - Reason: the loop guard proves `i < raw.len()` and `i < bytes.len()`.
            pub unsafe fn from_str_unchecked(s: &str) -> Self {
                let mut bytes = [0; 64];
                let raw = s.as_bytes();
                let mut i = 0;
                while i < raw.len() && i < bytes.len() {
                    bytes[i] = raw[i];
                    i += 1;
                }
                Self { bytes }
            }
        }
    }
}

pub mod arrow_cast {
    // Source: arrow-cast, parse::string_to_datetime/string_to_time/parse_date
    pub mod parse {
        // Bounds check:
        // - Sites: `s.get(0..10)` and `s.get(11..)`.
        // - Semantically removable: yes.
        // - Pattern: locally guarded or fixed-size in-bounds access.
        // - Reason: `str::get` performs checked slicing and returns `None` instead
        //   of panicking; subsequent parsing is delegated to guarded helpers.
        pub fn string_to_datetime(s: &str) -> Option<(i32, u32, u32, u32, u32, u32)> {
            let date = parse_date(s.get(0..10)?)?;
            let time = string_to_time(s.get(11..)?)?;
            Some((date.0, date.1, date.2, time.0, time.1, time.2))
        }

        // Bounds check:
        // - Sites: `b[2]`, `b[5]`, and delegated `two(b, pos)` calls.
        // - Semantically removable: yes.
        // - Pattern: locally guarded or fixed-size in-bounds access.
        // - Reason: `b.len() < 8` returns before the delimiter checks, and the
        //   fixed `two` positions read only indexes below 8.
        pub fn string_to_time(s: &str) -> Option<(u32, u32, u32)> {
            let b = s.as_bytes();
            if b.len() < 8 || b[2] != b':' || b[5] != b':' {
                return None;
            }
            Some((two(b, 0)?, two(b, 3)?, two(b, 6)?))
        }

        // Bounds check:
        // - Sites: `b[4]`, `b[7]`, `b[i]`, and delegated `two(b, pos)` calls.
        // - Semantically removable: yes.
        // - Pattern: locally guarded or fixed-size in-bounds access.
        // - Reason: `b.len() == 10` is required before fixed delimiter, year, and
        //   two-digit component accesses.
        pub fn parse_date(s: &str) -> Option<(i32, u32, u32)> {
            let b = s.as_bytes();
            if b.len() != 10 || b[4] != b'-' || b[7] != b'-' {
                return None;
            }
            let mut year = 0i32;
            let mut i = 0;
            while i < 4 {
                year = year * 10 + digit(b[i])? as i32;
                i += 1;
            }
            Some((year, two(b, 5)?, two(b, 8)?))
        }

        // Bounds check:
        // - Sites: `b[pos]` and `b[pos + 1]`.
        // - Semantically removable: conditional.
        // - Pattern: caller or constructor invariant.
        // - Reason: this helper requires callers to pass `pos + 1 < b.len()`; the
        //   current callers satisfy that with fixed positions after length checks.
        fn two(b: &[u8], pos: usize) -> Option<u32> {
            Some(digit(b[pos])? * 10 + digit(b[pos + 1])?)
        }

        fn digit(b: u8) -> Option<u32> {
            if b.is_ascii_digit() {
                Some((b - b'0') as u32)
            } else {
                None
            }
        }
    }
}

pub mod curve25519_dalek {
    // Source: curve25519-dalek, scalar::Scalar::from_canonical_bytes/non_adjacent_form/as_radix_16/as_radix_2w/clamp_integer
    pub mod scalar {
        #[derive(Clone, Copy)]
        pub struct Scalar {
            bytes: [u8; 32],
        }

        impl Scalar {
            // Bounds check:
            // - Sites: `bytes[31]`.
            // - Semantically removable: yes.
            // - Pattern: locally guarded or fixed-size in-bounds access.
            // - Reason: `bytes` is a fixed `[u8; 32]`, so index 31 is always valid.
            pub fn from_canonical_bytes(bytes: [u8; 32]) -> Option<Self> {
                if bytes[31] & 0b1110_0000 == 0 {
                    Some(Self { bytes })
                } else {
                    None
                }
            }

            // Bounds check:
            // - Sites: `self.bytes[i]` and `naf[i]`.
            // - Semantically removable: yes.
            // - Pattern: locally guarded or fixed-size in-bounds access.
            // - Reason: `i < self.bytes.len()` and `naf` is longer than
            //   `self.bytes`, so both indexes are valid.
            pub fn non_adjacent_form(&self, width: usize) -> [i8; 256] {
                let mut naf = [0i8; 256];
                let mut carry = 0i16;
                let mut i = 0;
                while i < self.bytes.len() {
                    let mut val = self.bytes[i] as i16 + carry;
                    carry = val >> width.min(7);
                    val -= carry << width.min(7);
                    naf[i] = val as i8;
                    i += 1;
                }
                naf
            }

            // Bounds check:
            // - Sites: `self.bytes[i]`, `out[2 * i]`, and `out[2 * i + 1]`.
            // - Semantically removable: yes.
            // - Pattern: locally guarded or fixed-size in-bounds access.
            // - Reason: `i < 32` implies both output indexes are below 64.
            pub fn as_radix_16(&self) -> [i8; 64] {
                let mut out = [0i8; 64];
                let mut i = 0;
                while i < self.bytes.len() {
                    out[2 * i] = (self.bytes[i] & 15) as i8;
                    out[2 * i + 1] = (self.bytes[i] >> 4) as i8;
                    i += 1;
                }
                out
            }

            // Bounds check:
            // - Sites: `self.bytes[i]`.
            // - Semantically removable: yes.
            // - Pattern: locally guarded or fixed-size in-bounds access.
            // - Reason: the loop guard `i < self.bytes.len()` bounds the read;
            //   the output uses `Vec::push` rather than indexing.
            pub fn as_radix_2w(&self, w: usize) -> Vec<i8> {
                let mask = (1u16 << w.min(7)) - 1;
                let mut out = Vec::new();
                let mut i = 0;
                while i < self.bytes.len() {
                    out.push((self.bytes[i] as u16 & mask) as i8);
                    i += 1;
                }
                out
            }

            // Bounds check:
            // - Sites: `bytes[0]` and `bytes[31]`.
            // - Semantically removable: yes.
            // - Pattern: locally guarded or fixed-size in-bounds access.
            // - Reason: `bytes` is a fixed `[u8; 32]`, so both constant indexes
            //   are always valid.
            pub fn clamp_integer(mut bytes: [u8; 32]) -> [u8; 32] {
                bytes[0] &= 248;
                bytes[31] &= 127;
                bytes[31] |= 64;
                bytes
            }
        }
    }

    // Source: curve25519-dalek, backend::serial::u64::* FieldElement/Scalar52 helpers
    pub mod backend {
        pub mod serial {
            pub mod u64 {
                pub mod field {
                    pub struct FieldElement51(pub [u64; 5]);

                    impl FieldElement51 {
                        // Bounds check:
                        // - Sites: `limbs[i]` and `bytes[i * 6]`.
                        // - Semantically removable: yes.
                        // - Pattern: locally guarded or fixed-size in-bounds access.
                        // - Reason: `i < 5` bounds `limbs`, and `i * 6` is at
                        //   most 24, which is below the 32-byte input length.
                        pub fn from_bytes(bytes: &[u8; 32]) -> Self {
                            let mut limbs = [0u64; 5];
                            let mut i = 0;
                            while i < limbs.len() {
                                limbs[i] = bytes[i * 6] as u64;
                                i += 1;
                            }
                            Self(limbs)
                        }
                    }
                }

                pub mod scalar {
                    pub struct Scalar52(pub [u64; 5]);

                    impl Scalar52 {
                        // Bounds check:
                        // - Sites: `limbs[i]` and `bytes[i * 6]`.
                        // - Semantically removable: yes.
                        // - Pattern: locally guarded or fixed-size in-bounds access.
                        // - Reason: `i < 5` bounds `limbs`, and `i * 6` is at
                        //   most 24, which is below the 32-byte input length.
                        pub fn from_bytes(bytes: &[u8; 32]) -> Self {
                            let mut limbs = [0u64; 5];
                            let mut i = 0;
                            while i < limbs.len() {
                                limbs[i] = bytes[i * 6] as u64 & ((1 << 52) - 1);
                                i += 1;
                            }
                            Self(limbs)
                        }

                        // Bounds check:
                        // - Sites: `out[i]` and `self.0[i]`.
                        // - Semantically removable: yes.
                        // - Pattern: locally guarded or fixed-size in-bounds access.
                        // - Reason: the loop guard `i < out.len()` also bounds
                        //   `self.0` because both arrays have length 5.
                        pub fn square(&self) -> Self {
                            let mut out = [0u64; 5];
                            let mut i = 0;
                            while i < out.len() {
                                out[i] = self.0[i].wrapping_mul(self.0[i]);
                                i += 1;
                            }
                            Self(out)
                        }
                    }
                }
            }
        }
    }
}

pub mod png {
    // Source: png, adam7::expand_pass
    pub mod adam7 {
        // Bounds check:
        // - Sites: `out[x]` and `pass[i]`.
        // - Semantically removable: yes.
        // - Pattern: locally guarded or fixed-size in-bounds access.
        // - Reason: the loop guard proves `i < pass.len()` and `x < out.len()`.
        pub fn expand_pass(pass: &[u8], out: &mut [u8], x_start: usize, x_step: usize) -> usize {
            let mut i = 0;
            let mut x = x_start;
            while i < pass.len() && x < out.len() {
                out[x] = pass[i];
                i += 1;
                x += x_step;
            }
            i
        }
    }

    // Source: png, decoder::transform::palette::* and row transform
    pub mod decoder {
        pub mod transform {
            pub mod palette {
                // Bounds check:
                // - Sites: `out[i]`, `palette[i]`, and `trns[i]`.
                // - Semantically removable: yes.
                // - Pattern: locally guarded or fixed-size in-bounds access.
                // - Reason: the loop guard bounds `out` and `palette`; `trns[i]`
                //   is guarded by `i < trns.len()`.
                pub fn create_rgba(palette: &[[u8; 3]], trns: &[u8], out: &mut [[u8; 4]]) -> usize {
                    let mut i = 0;
                    while i < palette.len() && i < out.len() {
                        out[i] = [
                            palette[i][0],
                            palette[i][1],
                            palette[i][2],
                            if i < trns.len() { trns[i] } else { 255 },
                        ];
                        i += 1;
                    }
                    i
                }

                // Bounds check:
                // - Sites: `indexes[i]`, `rgba[idx]`, and `out[o..o + 4]`.
                // - Semantically removable: yes.
                // - Pattern: locally guarded or fixed-size in-bounds access.
                // - Reason: the loop bounds `indexes` and the output slice range;
                //   the explicit `idx >= rgba.len()` check guards the palette read.
                pub fn expand_8bit(indexes: &[u8], rgba: &[[u8; 4]], out: &mut [u8]) -> usize {
                    let mut i = 0;
                    let mut o = 0;
                    while i < indexes.len() && o + 4 <= out.len() {
                        let idx = indexes[i] as usize;
                        if idx >= rgba.len() {
                            break;
                        }
                        out[o..o + 4].copy_from_slice(&rgba[idx]);
                        i += 1;
                        o += 4;
                    }
                    o
                }
            }

            // Bounds check:
            // - Sites: `row[i]` and `row[i - bpp]`.
            // - Semantically removable: yes.
            // - Pattern: locally guarded or fixed-size in-bounds access.
            // - Reason: `i` starts at `bpp`, so `i - bpp <= i`; the loop guard
            //   `i < row.len()` bounds both accesses. `bpp > 0` is only needed
            //   for loop progress, not for bounds safety.
            pub fn transform_row_sub(row: &mut [u8], bpp: usize) {
                let mut i = bpp;
                while i < row.len() {
                    row[i] = row[i].wrapping_add(row[i - bpp]);
                    i += 1;
                }
            }
        }
    }
}

pub mod chrono {
    // Source: chrono, format::parse::digit
    pub mod format {
        pub mod parse {
            // Bounds check:
            // - Sites: none.
            // - Semantically removable: not applicable.
            // - Pattern: no runtime indexing in this function.
            // - Reason: this testcase only classifies a scalar byte.
            pub fn digit(byte: u8) -> Option<u32> {
                if byte.is_ascii_digit() {
                    Some((byte - b'0') as u32)
                } else {
                    None
                }
            }
        }

        pub mod scan {
            // Source: chrono, format::scan::nanosecond_fixed/short_or_long_month0
            // Bounds check:
            // - Sites: `bytes[i]`.
            // - Semantically removable: yes.
            // - Pattern: locally guarded or fixed-size in-bounds access.
            // - Reason: the loop guard requires both `i < bytes.len()` and `i < 9`
            //   before each read.
            pub fn nanosecond_fixed(bytes: &[u8]) -> Option<u32> {
                let mut value = 0u32;
                let mut i = 0;
                while i < bytes.len() && i < 9 {
                    value = value * 10 + super::parse::digit(bytes[i])?;
                    i += 1;
                }
                while i < 9 {
                    value *= 10;
                    i += 1;
                }
                Some(value)
            }

            // Bounds check:
            // - Sites: `MONTHS[m]`, `lower[i]`, and `name[i]`.
            // - Semantically removable: yes.
            // - Pattern: locally guarded or fixed-size in-bounds access.
            // - Reason: `m < MONTHS.len()` bounds the month table; the inner loop
            //   checks `i` against both byte slices before reading.
            pub fn short_or_long_month0(s: &str) -> Option<u32> {
                const MONTHS: [&str; 12] = [
                    "jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov",
                    "dec",
                ];
                let lower = s.as_bytes();
                let mut m = 0;
                while m < MONTHS.len() {
                    let name = MONTHS[m].as_bytes();
                    let mut i = 0;
                    while i < lower.len()
                        && i < name.len()
                        && lower[i].to_ascii_lowercase() == name[i]
                    {
                        i += 1;
                    }
                    if i == name.len() {
                        return Some(m as u32);
                    }
                    m += 1;
                }
                None
            }
        }
    }

    pub mod naive {
        // Source: chrono, naive::date::cycle_to_yo / yo_to_cycle
//     const fn cycle_to_yo(cycle: u32) -> (u32, u32) {
//     let mut year_mod_400 = cycle / 365;
//     let mut ordinal0 = cycle % 365;
//     let delta = YEAR_DELTAS[year_mod_400 as usize] as u32;
//     if ordinal0 < delta {
//         year_mod_400 -= 1;
//         ordinal0 += 365 - YEAR_DELTAS[year_mod_400 as usize] as u32;
//     } else {
//         ordinal0 -= delta;
//     }
//     (year_mod_400, ordinal0 + 1)
// }
        pub mod date {
            // Bounds check:
            // - Sites: none.
            // - Semantically removable: not applicable.
            // - Pattern: no runtime indexing in this function.
            // - Reason: this reduced testcase uses only scalar arithmetic.
            pub fn cycle_to_yo(cycle: i32, day: u32) -> (i32, u32) {
                let mut year = cycle * 400;
                let mut rest = day;
                while rest >= 365 {
                    rest -= 365;
                    year += 1;
                }
                (year, rest)
            }

            // Bounds check:
            // - Sites: none.
            // - Semantically removable: not applicable.
            // - Pattern: no runtime indexing in this function.
            // - Reason: this reduced testcase uses only scalar arithmetic.
            pub fn yo_to_cycle(year: i32, ordinal: u32) -> (i32, u32) {
                let cycle = year.div_euclid(400);
                let y = year.rem_euclid(400) as u32;
                (cycle, y * 365 + ordinal)
            }
        }

        pub mod internals {
            // Source: chrono, naive::internals::YearFlags::from_year / Mdf::from_ol / Mdf::ordinal_and_flags
            pub struct YearFlags(u8);

            impl YearFlags {
                // Bounds check:
                // - Sites: none.
                // - Semantically removable: not applicable.
                // - Pattern: no runtime indexing in this function.
                // - Reason: this testcase only computes leap-year flags from a scalar.
                pub fn from_year(year: i32) -> Self {
                    let leap = (year % 4 == 0 && year % 100 != 0) || year % 400 == 0;
                    Self(if leap { 1 } else { 0 })
                }
            }

            pub struct Mdf {
                month: u32,
                day: u32,
                flags: YearFlags,
            }

            impl Mdf {
                // Bounds check:
                // - Sites: `month_len[month]`.
                // - Semantically removable: yes.
                // - Pattern: locally guarded or fixed-size in-bounds access.
                // - Reason: the loop guard `month < month_len.len()` bounds every
                //   access to the fixed month-length table.
                pub fn from_ol(ordinal: u32, flags: YearFlags) -> Option<Self> {
                    let month_len = [
                        31,
                        28 + flags.0 as u32,
                        31,
                        30,
                        31,
                        30,
                        31,
                        31,
                        30,
                        31,
                        30,
                        31,
                    ];
                    let mut remaining = ordinal;
                    let mut month = 0;
                    while month < month_len.len() {
                        if remaining <= month_len[month] {
                            return Some(Self {
                                month: month as u32 + 1,
                                day: remaining,
                                flags,
                            });
                        }
                        remaining -= month_len[month];
                        month += 1;
                    }
                    None
                }

                // Bounds check:
                // - Sites: `month_len[i]`.
                // - Semantically removable: conditional.
                // - Pattern: caller or constructor invariant.
                // - Reason: `i + 1 < self.month` does not by itself prove
                //   `self.month <= 12`; the constructor preserves that invariant,
                //   but direct construction would need the same precondition.
                pub fn ordinal_and_flags(&self) -> (u32, u8) {
                    let month_len = [
                        31,
                        28 + self.flags.0 as u32,
                        31,
                        30,
                        31,
                        30,
                        31,
                        31,
                        30,
                        31,
                        30,
                        31,
                    ];
                    let mut ord = self.day;
                    let mut i = 0;
                    while i + 1 < self.month as usize {
                        ord += month_len[i];
                        i += 1;
                    }
                    (ord, self.flags.0)
                }
            }
        }
    }

    pub mod offset {
        pub mod local {
            pub mod tz_info {
                pub mod rule {
                    // Source: chrono, offset::local::tz_info::rule::RuleDay::* and days_since
                    pub enum RuleDay {
                        Julian1(u16),
                        Julian0(u16),
                        MonthWeekDay { month: u8, week: u8, day: u8 },
                    }

                    impl RuleDay {
                        // Bounds check:
                        // - Sites: `month_offsets[(month - 1) as usize]`.
                        // - Semantically removable: conditional.
                        // - Pattern: caller or constructor invariant.
                        // - Reason: the month-week-day variant needs
                        //   `1 <= month <= 12` from the caller or parser.
                        pub fn transition_day(&self, leap: bool) -> u16 {
                            match *self {
                                RuleDay::Julian1(n) => n + u16::from(leap && n >= 60),
                                RuleDay::Julian0(n) => n,
                                RuleDay::MonthWeekDay { month, week, day } => {
                                    let month_offsets =
                                        [0u16, 31, 59, 90, 120, 151, 181, 212, 243, 273, 304, 334];
                                    month_offsets[(month - 1) as usize]
                                        + (week as u16 - 1) * 7
                                        + day as u16
                                }
                            }
                        }
                    }

                    // Bounds check:
                    // - Sites: `MONTHS[(m - 1) as usize]`.
                    // - Semantically removable: conditional.
                    // - Pattern: caller or constructor invariant.
                    // - Reason: the loop indexes the month table from the caller
                    //   supplied `month`; it is safe when `1 <= month <= 12`.
                    pub fn days_since(year: i32, month: u8, day: u8) -> i64 {
                        let mut days = 0i64;
                        let mut y = 1970;
                        while y < year {
                            days += if (y % 4 == 0 && y % 100 != 0) || y % 400 == 0 {
                                366
                            } else {
                                365
                            };
                            y += 1;
                        }
                        const MONTHS: [i64; 12] = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
                        let mut m = 1;
                        while m < month {
                            days += MONTHS[(m - 1) as usize];
                            m += 1;
                        }
                        days + day as i64 - 1
                    }
                }
            }
        }
    }
}

pub mod regex_lite {
    // Source: regex-lite, hir::Class::canonicalize / hir::Look::is_match
    pub mod hir {
        pub struct Class {
            ranges: Vec<(char, char)>,
        }

        impl Class {
            pub fn new(ranges: Vec<(char, char)>) -> Self {
                Self { ranges }
            }

            // Bounds check:
            // - Sites: `self.ranges[read]`, `self.ranges[write - 1]`, and
            //   `self.ranges[write]`.
            // - Semantically removable: yes.
            // - Pattern: locally guarded or fixed-size in-bounds access.
            // - Reason: `read < self.ranges.len()` bounds reads; `write` never
            //   exceeds `read`, and `write > 0` guards `write - 1`.
            pub fn canonicalize(&mut self) {
                self.ranges.sort_by_key(|r| r.0);
                let mut write = 0;
                let mut read = 0;
                while read < self.ranges.len() {
                    if write > 0 && self.ranges[read].0 <= self.ranges[write - 1].1 {
                        self.ranges[write - 1].1 =
                            self.ranges[write - 1].1.max(self.ranges[read].1);
                    } else {
                        self.ranges[write] = self.ranges[read];
                        write += 1;
                    }
                    read += 1;
                }
                self.ranges.truncate(write);
            }
        }

        pub enum Look {
            Start,
            End,
            Word,
        }

        impl Look {
            // Bounds check:
            // - Sites: `haystack[at]`.
            // - Semantically removable: yes.
            // - Pattern: locally guarded or fixed-size in-bounds access.
            // - Reason: the `Look::Word` branch checks `at < haystack.len()` before
            //   indexing; the other branches do not index.
            pub fn is_match(&self, haystack: &[u8], at: usize) -> bool {
                match self {
                    Look::Start => at == 0,
                    Look::End => at == haystack.len(),
                    Look::Word => at < haystack.len() && haystack[at].is_ascii_alphanumeric(),
                }
            }
        }
    }
}

pub mod similar {
    // Source: similar, algorithms::hunt::lower_bound
    pub mod algorithms {
        pub mod hunt {
            // Bounds check:
            // - Sites: `slice[mid]`.
            // - Semantically removable: yes.
            // - Pattern: locally guarded or fixed-size in-bounds access.
            // - Reason: binary search maintains `left <= mid < right <= slice.len()`.
            pub fn lower_bound(slice: &[usize], value: usize) -> usize {
                let mut left = 0;
                let mut right = slice.len();
                while left < right {
                    let mid = left + (right - left) / 2;
                    if slice[mid] < value {
                        left = mid + 1;
                    } else {
                        right = mid;
                    }
                }
                left
            }
        }
    }
}

pub mod arrow_select {
    // Source: arrow-select, concat::binary_capacity
    pub mod concat {
        // Bounds check:
        // - Sites: `offsets[i]` and `offsets[i - 1]`.
        // - Semantically removable: yes.
        // - Pattern: locally guarded or fixed-size in-bounds access.
        // - Reason: the loop starts at 1 and maintains `i < offsets.len()`.
        pub fn binary_capacity(offsets: &[usize]) -> usize {
            let mut cap = 0;
            let mut i = 1;
            while i < offsets.len() {
                cap += offsets[i] - offsets[i - 1];
                i += 1;
            }
            cap
        }
    }
}

pub mod unicode_bidi {
    // Source: unicode-bidi, assign_levels_to_removed_chars / Utf16Char helpers
    // Bounds check:
    // - Sites: `removed[i]` and `levels[idx]`.
    // - Semantically removable: yes.
    // - Pattern: locally guarded or fixed-size in-bounds access.
    // - Reason: `i < removed.len()` guards the removed-index read, and
    //   `idx < levels.len()` guards the levels write.
    pub fn assign_levels_to_removed_chars(levels: &mut [u8], removed: &[usize], level: u8) {
        let mut i = 0;
        while i < removed.len() {
            let idx = removed[i];
            if idx < levels.len() {
                levels[idx] = level;
            }
            i += 1;
        }
    }

    pub struct Utf16Char<T> {
        units: T,
    }

    impl Utf16Char<u16> {
        // Bounds check:
        // - Sites: none.
        // - Semantically removable: not applicable.
        // - Pattern: no runtime indexing in this function.
        // - Reason: this constructor stores one scalar UTF-16 code unit.
        pub fn from_u16(unit: u16) -> Self {
            Self { units: unit }
        }

        // Bounds check:
        // - Sites: none.
        // - Semantically removable: not applicable.
        // - Pattern: no runtime indexing in this function.
        // - Reason: this testcase only checks a scalar range.
        pub fn is_surrogate(&self) -> bool {
            (0xd800..=0xdfff).contains(&self.units)
        }
    }

    impl Utf16Char<[u16; 2]> {
        // Bounds check:
        // - Sites: none.
        // - Semantically removable: not applicable.
        // - Pattern: no runtime indexing in this function.
        // - Reason: this constructor builds a fixed two-element array without
        //   indexing.
        pub fn from_pair(high: u16, low: u16) -> Option<Self> {
            if (0xd800..=0xdbff).contains(&high) && (0xdc00..=0xdfff).contains(&low) {
                Some(Self { units: [high, low] })
            } else {
                None
            }
        }
    }
}

pub mod chacha20 {
    // Source: chacha20, quarter_round
    // Bounds check:
    // - Sites: `state[a]`, `state[b]`, `state[c]`, and `state[d]`.
    // - Semantically removable: conditional.
    // - Pattern: caller or constructor invariant.
    // - Reason: the ChaCha quarter-round semantics require caller-supplied lane
    //   indexes in `0..16`; this reduced function does not validate them locally.
    pub fn quarter_round(state: &mut [u32; 16], a: usize, b: usize, c: usize, d: usize) {
        state[a] = state[a].wrapping_add(state[b]);
        state[d] ^= state[a];
        state[d] = state[d].rotate_left(16);
        state[c] = state[c].wrapping_add(state[d]);
        state[b] ^= state[c];
        state[b] = state[b].rotate_left(12);
    }
}

pub mod ipnet {
    // Source: ipnet, parser::Parser::<A>::read_char
    pub mod parser {
        pub struct Parser<'a> {
            bytes: &'a [u8],
            pos: usize,
        }

        impl<'a> Parser<'a> {
            pub fn new(s: &'a str) -> Self {
                Self {
                    bytes: s.as_bytes(),
                    pos: 0,
                }
            }

            // Bounds check:
            // - Sites: `self.bytes[self.pos]`.
            // - Semantically removable: yes.
            // - Pattern: locally guarded or fixed-size in-bounds access.
            // - Reason: the function returns before indexing when
            //   `self.pos >= self.bytes.len()`.
            pub fn read_char(&mut self) -> Option<u8> {
                if self.pos >= self.bytes.len() {
                    return None;
                }
                let b = self.bytes[self.pos];
                self.pos += 1;
                Some(b)
            }
        }
    }
}

pub mod iri_string {
    // Source: iri-string, normalize::pct_case::into_char_trusted and parser helpers
    pub mod normalize {
        pub mod pct_case {
            // Bounds check:
            // - Sites: none.
            // - Semantically removable: not applicable.
            // - Pattern: no runtime indexing in this function.
            // - Reason: this testcase converts scalar bytes through a helper
            //   without indexing.
            pub fn into_char_trusted(hi: u8, lo: u8) -> char {
                let value = (hex(hi) << 4) | hex(lo);
                value as char
            }

            fn hex(b: u8) -> u8 {
                match b {
                    b'0'..=b'9' => b - b'0',
                    b'a'..=b'f' => b - b'a' + 10,
                    b'A'..=b'F' => b - b'A' + 10,
                    _ => 0,
                }
            }
        }
    }

    pub mod parser {
        // Bounds check:
        // - Sites: `bytes[i]`.
        // - Semantically removable: yes.
        // - Pattern: locally guarded or fixed-size in-bounds access.
        // - Reason: the loop guard `i < bytes.len()` bounds every read.
        pub fn is_ascii_iri(s: &str) -> bool {
            let bytes = s.as_bytes();
            let mut i = 0;
            while i < bytes.len() {
                if bytes[i] > 0x7f || bytes[i] == b' ' {
                    return false;
                }
                i += 1;
            }
            true
        }

        pub mod trusted {
            // Bounds check:
            // - Sites: `b[i]`.
            // - Semantically removable: yes.
            // - Pattern: locally guarded or fixed-size in-bounds access.
            // - Reason: the loop guard `i < b.len()` bounds every host-byte read.
            pub fn is_ascii_only_host(host: &str) -> bool {
                let b = host.as_bytes();
                let mut i = 0;
                while i < b.len() {
                    if !(b[i].is_ascii_alphanumeric() || b[i] == b'-' || b[i] == b'.') {
                        return false;
                    }
                    i += 1;
                }
                true
            }
        }
    }

    pub mod template {
        pub mod components {
            pub struct ExprBody<'a> {
                raw: &'a str,
            }

            impl<'a> ExprBody<'a> {
                pub fn new(raw: &'a str) -> Self {
                    Self { raw }
                }

                // Bounds check:
                // - Sites: `b[i]`.
                // - Semantically removable: yes.
                // - Pattern: locally guarded or fixed-size in-bounds access.
                // - Reason: each byte read is guarded by `i < b.len()`.
                pub fn operator_end(&self) -> usize {
                    let b = self.raw.as_bytes();
                    let mut i = 0;
                    while i < b.len() && b[i] != b':' && b[i] != b',' {
                        i += 1;
                    }
                    i
                }
            }
        }
    }
}

pub mod pyo3 {
    // Source: pyo3, impl_::unindent::copy_forward_until_eol
    pub mod impl_ {
        pub mod unindent {
            // Bounds check:
            // - Sites: `bytes[read_idx]` and `bytes[write_idx]`.
            // - Semantically removable: yes.
            // - Pattern: locally guarded or fixed-size in-bounds access.
            // - Reason: `read_idx < bytes.len()` guards the read and
            //   `read_idx >= write_idx` implies `write_idx < bytes.len()`; the
            //   invariant is preserved because both indexes advance together.
            pub  fn copy_forward_until_eol(
                bytes: &mut [u8],
                mut read_idx: usize,
                mut write_idx: usize,
            ) -> (usize, usize) {
                assert!(read_idx >= write_idx);
                while read_idx < bytes.len() {
                    let value = bytes[read_idx]; // bounds check eliminated 
                    bytes[write_idx] = value;// bounds check retained
                    read_idx += 1;
                    write_idx += 1;
                    if value == b'\n' {
                        break;
                    }
                }
                (read_idx, write_idx)
            }
        }
    }
}

pub mod arrow_string {
    // Source: arrow-string, traits::merge_nonascii_unchecked / utf16_char::Utf16Char::from_str_start
    pub mod traits {
        // Bounds check:
        // - Sites: `l[i]` and `r[j]`.
        // - Semantically removable: yes.
        // - Pattern: locally guarded or fixed-size in-bounds access.
        // - Reason: each loop guard bounds the corresponding byte slice before
        //   reading; output appends through `String::push`.
        pub unsafe fn merge_nonascii_unchecked(left: &str, right: &str, out: &mut String) {
            let mut i = 0;
            let l = left.as_bytes();
            while i < l.len() {
                out.push(l[i] as char);
                i += 1;
            }
            let mut j = 0;
            let r = right.as_bytes();
            while j < r.len() {
                out.push(r[j] as char);
                j += 1;
            }
        }
    }

    pub mod utf16_char {
        pub struct Utf16Char {
            units: [u16; 2],
            len: usize,
        }

        impl Utf16Char {
            // Bounds check:
            // - Sites: none.
            // - Semantically removable: not applicable.
            // - Pattern: no runtime indexing in this function.
            // - Reason: `chars().next()` and `encode_utf16` avoid explicit indexing.
            pub fn from_str_start(s: &str) -> Option<Self> {
                let ch = s.chars().next()?;
                let mut units = [0u16; 2];
                let len = ch.encode_utf16(&mut units).len();
                Some(Self { units, len })
            }
        }
    }
}
