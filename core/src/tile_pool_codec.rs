//! The tile pool's key and codec (13C-1, `tile_pool`): [`key`], the one
//! derivation of a payload's content key, the sample types the pool stores
//! ([`PoolSample`]: tag, component size, little-endian layout), and the
//! payload codec. Names no document type.
//!
//! **Key.** SHA-256 over `"DRTP"`, a tag byte (the sample type, or
//! [`TAG_BLOB`]), width u32 LE, height u32 LE, then the samples in
//! little-endian layout ([`PoolSample::put_le`]). The tag and the size are in
//! the key so equal bytes of two sample types, or of one plane cut two ways,
//! never share a payload.
//!
//! **Codec** (one byte per entry: 0 raw, 1 deflate, 2 shuffle-deflate). A u8
//! payload is deflated once; a u16 or f32 payload is deflated plain AND
//! byte-shuffled (each component's k-th bytes together), and the smaller is
//! kept; raw is kept when neither shrinks it. Why both: the spike measured
//! the shuffle winning on noisy high-depth data (RGBA16 with six noisy low
//! bits, 64.2% of raw against 78.5%) and losing on clean data (RGBA16
//! promoted from 8-bit, 48.6% against 36.1%), so neither alone is right. The
//! second deflate is paid only for a tile the pool has not seen. Level 1, the
//! fastest that still compresses: level 6 bought 8 to 12 points at a cost no
//! quiet machine has measured yet (13C-6), and the level is not in the format.
//! Deflate and inflate are `psd_zip`'s, the crate's one zlib seam.

use crate::sha256::Sha256;
use crate::tile_store::Sample;

/// A payload's content key.
pub type Key = [u8; 32];

pub const TAG_GRAY8: u8 = 1;
pub const TAG_GRAY16: u8 = 2;
pub const TAG_RGBA8: u8 = 3;
pub const TAG_RGBA16: u8 = 4;
pub const TAG_CMYKA16: u8 = 5;
pub const TAG_RGBA32F: u8 = 6;
/// A chunk of a blob payload: width = the chunk's length, height = 0.
pub const TAG_BLOB: u8 = 0x80;

/// A blob payload is keyed in chunks of this many bytes, counted from its
/// first byte: the largest tile payload (a 256² RGBA32F tile), so every
/// payload the pool holds is at most 1 MiB.
pub const BLOB_CHUNK_BYTES: usize = 1 << 20;

/// The deflate level (module doc).
const LEVEL: u8 = 1;

pub(crate) const RAW: u8 = 0;
pub(crate) const DEFLATE: u8 = 1;
pub(crate) const SHUFFLE: u8 = 2;

/// The content key of `bytes` (module doc).
pub fn key(tag: u8, width: u32, height: u32, bytes: &[u8]) -> Key {
    let mut hasher = Sha256::new();
    hasher.update(b"DRTP");
    hasher.update(&[tag]);
    hasher.update(&width.to_le_bytes());
    hasher.update(&height.to_le_bytes());
    hasher.update(bytes);
    hasher.finish()
}

/// The first 16 hex digits of a key: how an error names a payload.
pub fn key_text(key: &Key) -> String {
    key[..8].iter().map(|b| format!("{b:02x}")).collect()
}

/// A sample type the pool stores: its tag, its component size (the
/// shuffle's element) and its little-endian layout.
pub trait PoolSample: Sample {
    const TAG: u8;
    const ELEMENT: usize;
    const BYTES: usize;
    /// Appends the samples' little-endian bytes.
    fn put_le(samples: &[Self], out: &mut Vec<u8>);
    /// One sample from its `BYTES` little-endian bytes.
    fn from_le(bytes: &[u8]) -> Self;
}

fn u16_at(bytes: &[u8], i: usize) -> u16 {
    u16::from_le_bytes([bytes[2 * i], bytes[2 * i + 1]])
}

impl PoolSample for u8 {
    const TAG: u8 = TAG_GRAY8;
    const ELEMENT: usize = 1;
    const BYTES: usize = 1;
    fn put_le(samples: &[Self], out: &mut Vec<u8>) {
        out.extend_from_slice(samples);
    }
    fn from_le(bytes: &[u8]) -> Self {
        bytes[0]
    }
}

impl PoolSample for u16 {
    const TAG: u8 = TAG_GRAY16;
    const ELEMENT: usize = 2;
    const BYTES: usize = 2;
    fn put_le(samples: &[Self], out: &mut Vec<u8>) {
        samples
            .iter()
            .for_each(|v| out.extend_from_slice(&v.to_le_bytes()));
    }
    fn from_le(bytes: &[u8]) -> Self {
        u16_at(bytes, 0)
    }
}

impl PoolSample for [u8; 4] {
    const TAG: u8 = TAG_RGBA8;
    const ELEMENT: usize = 1;
    const BYTES: usize = 4;
    fn put_le(samples: &[Self], out: &mut Vec<u8>) {
        out.extend_from_slice(samples.as_flattened());
    }
    fn from_le(bytes: &[u8]) -> Self {
        [bytes[0], bytes[1], bytes[2], bytes[3]]
    }
}

impl PoolSample for [u16; 4] {
    const TAG: u8 = TAG_RGBA16;
    const ELEMENT: usize = 2;
    const BYTES: usize = 8;
    fn put_le(samples: &[Self], out: &mut Vec<u8>) {
        for sample in samples {
            sample
                .iter()
                .for_each(|v| out.extend_from_slice(&v.to_le_bytes()));
        }
    }
    fn from_le(bytes: &[u8]) -> Self {
        std::array::from_fn(|i| u16_at(bytes, i))
    }
}

impl PoolSample for [u16; 5] {
    const TAG: u8 = TAG_CMYKA16;
    const ELEMENT: usize = 2;
    const BYTES: usize = 10;
    fn put_le(samples: &[Self], out: &mut Vec<u8>) {
        for sample in samples {
            sample
                .iter()
                .for_each(|v| out.extend_from_slice(&v.to_le_bytes()));
        }
    }
    fn from_le(bytes: &[u8]) -> Self {
        std::array::from_fn(|i| u16_at(bytes, i))
    }
}

impl PoolSample for [f32; 4] {
    const TAG: u8 = TAG_RGBA32F;
    const ELEMENT: usize = 4;
    const BYTES: usize = 16;
    fn put_le(samples: &[Self], out: &mut Vec<u8>) {
        for sample in samples {
            sample
                .iter()
                .for_each(|v| out.extend_from_slice(&v.to_le_bytes()));
        }
    }
    fn from_le(bytes: &[u8]) -> Self {
        std::array::from_fn(|i| {
            f32::from_le_bytes([
                bytes[4 * i],
                bytes[4 * i + 1],
                bytes[4 * i + 2],
                bytes[4 * i + 3],
            ])
        })
    }
}

/// Byte planes: every element's k-th byte together, k = 0, 1, …
fn shuffle(bytes: &[u8], element: usize) -> Vec<u8> {
    let count = bytes.len() / element;
    let mut out = vec![0_u8; bytes.len()];
    for (i, value) in bytes.chunks_exact(element).enumerate() {
        for (k, byte) in value.iter().enumerate() {
            out[k * count + i] = *byte;
        }
    }
    out
}

fn unshuffle(planes: &[u8], element: usize) -> Vec<u8> {
    let count = planes.len() / element;
    let mut out = vec![0_u8; planes.len()];
    for (i, value) in out.chunks_exact_mut(element).enumerate() {
        for (k, byte) in value.iter_mut().enumerate() {
            *byte = planes[k * count + i];
        }
    }
    out
}

/// The stored form of `bytes` and its codec (module doc).
pub(crate) fn encode(bytes: &[u8], element: usize) -> (u8, Vec<u8>) {
    let mut best = (DEFLATE, crate::psd_zip::deflate_zlib(bytes, LEVEL));
    if element > 1 {
        let shuffled = crate::psd_zip::deflate_zlib(&shuffle(bytes, element), LEVEL);
        if shuffled.len() < best.1.len() {
            best = (SHUFFLE, shuffled);
        }
    }
    if best.1.len() >= bytes.len() {
        return (RAW, bytes.to_vec());
    }
    best
}

pub(crate) fn decode(
    codec: u8,
    stored: &[u8],
    element: usize,
    expected: usize,
) -> Result<Vec<u8>, String> {
    match codec {
        RAW if stored.len() == expected => Ok(stored.to_vec()),
        DEFLATE => crate::psd_zip::inflate_zlib_exact(stored, expected),
        SHUFFLE if expected.is_multiple_of(element) => Ok(unshuffle(
            &crate::psd_zip::inflate_zlib_exact(stored, expected)?,
            element,
        )),
        _ => Err("stored payload does not match its codec".into()),
    }
}
