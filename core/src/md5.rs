//! RFC 1321 MD5, written out here like `metadata::crc32` rather than pulled
//! in as a dependency, because the crate needs it for exactly one thing: the
//! GUID that names a JPEG's ExtendedXMP packet. Adobe's XMP Specification
//! Part 3 defines that GUID as the 128-bit MD5 digest of the extended
//! packet, spelled as 32 uppercase hexadecimal digits, so a writer that
//! wants Photoshop, Lightroom and exiftool to recognize its chunks has to
//! compute it.
//!
//! It is a FORMAT IDENTIFIER here, not an integrity or security check. MD5
//! has been broken for collision resistance since 2004 and nothing in this
//! crate relies on it for anything a forged digest could exploit: the reader
//! (`metadata_xmp_extended`) only uses the GUID to tell which chunks belong
//! to the standard packet's `xmpNote:HasExtendedXMP`, and deliberately does
//! not verify it — a mismatched digest is exactly what a tool that edited
//! the packet without recomputing it leaves behind.
//!
//! `md5` (`#[doc(hidden)] pub`) is the GUID, a format identifier only.

/// The per-round left-rotation amounts, RFC 1321 §3.4.
const SHIFTS: [u32; 64] = [
    7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 5, 9, 14, 20, 5, 9, 14, 20, 5, 9,
    14, 20, 5, 9, 14, 20, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 6, 10, 15,
    21, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21,
];

/// `floor(abs(sin(i + 1)) * 2^32)`, RFC 1321 §3.4, as the table the RFC
/// prints rather than computed at run time, so no floating-point library's
/// rounding can change a digest.
const SINES: [u32; 64] = [
    0xd76a_a478,
    0xe8c7_b756,
    0x2420_70db,
    0xc1bd_ceee,
    0xf57c_0faf,
    0x4787_c62a,
    0xa830_4613,
    0xfd46_9501,
    0x6980_98d8,
    0x8b44_f7af,
    0xffff_5bb1,
    0x895c_d7be,
    0x6b90_1122,
    0xfd98_7193,
    0xa679_438e,
    0x49b4_0821,
    0xf61e_2562,
    0xc040_b340,
    0x265e_5a51,
    0xe9b6_c7aa,
    0xd62f_105d,
    0x0244_1453,
    0xd8a1_e681,
    0xe7d3_fbc8,
    0x21e1_cde6,
    0xc337_07d6,
    0xf4d5_0d87,
    0x455a_14ed,
    0xa9e3_e905,
    0xfcef_a3f8,
    0x676f_02d9,
    0x8d2a_4c8a,
    0xfffa_3942,
    0x8771_f681,
    0x6d9d_6122,
    0xfde5_380c,
    0xa4be_ea44,
    0x4bde_cfa9,
    0xf6bb_4b60,
    0xbebf_bc70,
    0x289b_7ec6,
    0xeaa1_27fa,
    0xd4ef_3085,
    0x0488_1d05,
    0xd9d4_d039,
    0xe6db_99e5,
    0x1fa2_7cf8,
    0xc4ac_5665,
    0xf429_2244,
    0x432a_ff97,
    0xab94_23a7,
    0xfc93_a039,
    0x655b_59c3,
    0x8f0c_cc92,
    0xffef_f47d,
    0x8584_5dd1,
    0x6fa8_7e4f,
    0xfe2c_e6e0,
    0xa301_4314,
    0x4e08_11a1,
    0xf753_7e82,
    0xbd3a_f235,
    0x2ad7_d2bb,
    0xeb86_d391,
];

/// The MD5 digest of `bytes`, RFC 1321. The message is padded with one 1
/// bit, then zero bits to 56 bytes modulo 64 — every one of the 64 residues
/// of the input length is a different amount of padding, including the one
/// that needs a whole extra block — then its length in BITS as a
/// little-endian u64.
#[doc(hidden)]
pub fn md5(bytes: &[u8]) -> [u8; 16] {
    let mut state: [u32; 4] = [0x6745_2301, 0xefcd_ab89, 0x98ba_dcfe, 0x1032_5476];
    let mut blocks = bytes.chunks_exact(64);
    for block in blocks.by_ref() {
        compress(&mut state, block);
    }
    // The tail: the leftover bytes, the 0x80 marker, zeros, the bit length.
    // One block when the leftover is at most 55 bytes, otherwise two.
    let rest = blocks.remainder();
    let mut tail = [0u8; 128];
    tail[..rest.len()].copy_from_slice(rest);
    tail[rest.len()] = 0x80;
    let used = if rest.len() < 56 { 64 } else { 128 };
    let bits = (bytes.len() as u64).wrapping_mul(8);
    tail[used - 8..used].copy_from_slice(&bits.to_le_bytes());
    for block in tail[..used].chunks_exact(64) {
        compress(&mut state, block);
    }
    let mut out = [0u8; 16];
    for (i, word) in state.iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&word.to_le_bytes());
    }
    out
}

/// One 64-byte block, RFC 1321 §3.4's four rounds of sixteen operations.
fn compress(state: &mut [u32; 4], block: &[u8]) {
    let mut m = [0u32; 16];
    for (i, word) in block.chunks_exact(4).enumerate() {
        m[i] = u32::from_le_bytes([word[0], word[1], word[2], word[3]]);
    }
    let [mut a, mut b, mut c, mut d] = *state;
    for i in 0..64 {
        let (f, g) = match i / 16 {
            0 => ((b & c) | (!b & d), i),
            1 => ((d & b) | (!d & c), (5 * i + 1) % 16),
            2 => (b ^ c ^ d, (3 * i + 5) % 16),
            _ => (c ^ (b | !d), (7 * i) % 16),
        };
        let rotated = a
            .wrapping_add(f)
            .wrapping_add(SINES[i])
            .wrapping_add(m[g])
            .rotate_left(SHIFTS[i]);
        a = d;
        d = c;
        c = b;
        b = b.wrapping_add(rotated);
    }
    state[0] = state[0].wrapping_add(a);
    state[1] = state[1].wrapping_add(b);
    state[2] = state[2].wrapping_add(c);
    state[3] = state[3].wrapping_add(d);
}
