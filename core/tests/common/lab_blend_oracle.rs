//! An independent f64 oracle of a Lab Color document's blend modes,
//! written from the declaration in `scripts/photoshop-probe/lab_blend_take.py`
//! (its candidates, its "Before take2" section and take2's RESULT), never
//! from `lab_blend.rs`. The L\* functions and the modes take2 kept on the
//! codes (C0) are `native_blend_oracle`'s W3C equations. Values are the
//! stored unit codes `u = [L̂, â, b̂]`: L\* = 100·L̂, a\* = 255·â − 128.
//!
//! Also the take's file decodes, read in place: a flat CIELab TIFF reader
//! (Photometric 8, uncompressed, II or MM, 8 or 16 bits, chunky or planar)
//! and a Lab PSD sample's unit code (readback fix 2: a 16-bit a\*/b\* sample
//! is Photoshop's 15-bit value i written as round(i·65535/32768)), over the
//! layered reader `cmyk_psd_fixture::read`, which keeps samples as stored.
#![allow(dead_code)]

use crate::native_blend_oracle::{hard_mix, separable};

/// a\* (or b\*) of a unit code.
pub fn star(u: f64) -> f64 {
    255.0 * u - 128.0
}

/// The unit code of a\* (or b\*), clipped to −128…127.
pub fn unit(a: f64) -> f64 {
    (a.clamp(-128.0, 127.0) + 128.0) / 255.0
}

/// C5 (Multiply and Screen): a\* = a\*b + a\*s − |a\*b|·a\*s / 128.
fn c5(b: f64, s: f64) -> f64 {
    let (p, q) = (star(b), star(s));
    unit(p + q - p.abs() * q / 128.0)
}

/// C2b (Linear Burn and Linear Dodge): a\* = a\*b + a\*s.
fn sum(b: f64, s: f64) -> f64 {
    unit(star(b) + star(s))
}

/// Vivid Light's halves without the [0, 1] clamp, the source's ends first.
fn vivid_open(b: f64, s: f64) -> f64 {
    if s == 0.0 {
        0.0
    } else if s == 1.0 {
        1.0
    } else if s <= 0.5 {
        1.0 - (1.0 - b) / (2.0 * s)
    } else {
        b / (2.0 - 2.0 * s)
    }
}

/// C4 (Vivid Light at 16 bits): y = ½ − a\*/256, a\* = −256·(B − ½).
fn c4(b: f64, s: f64) -> f64 {
    let y = |u: f64| 0.5 - star(u) / 256.0;
    unit(-256.0 * (vivid_open(y(b), y(s)) - 0.5))
}

/// C2a for Hard Mix (16 bits): the threshold on y = ½ + a\*/255.
fn c2a_hard_mix(b: f64, s: f64) -> f64 {
    let y = |u: f64| 0.5 + star(u) / 255.0;
    unit(255.0 * (hard_mix(y(b), y(s)) - 0.5))
}

/// VL-INT8 (Vivid Light at 8 bits) on one channel: both inputs rounded to
/// 8-bit codes, the halves evaluated on them and truncated to a code.
fn vivid_codes(b: f64, s: f64) -> f64 {
    let bv = (255.0 * b.clamp(0.0, 1.0)).round();
    let sv = (255.0 * s.clamp(0.0, 1.0)).round();
    let r = if sv == 0.0 {
        0.0
    } else if sv == 255.0 {
        255.0
    } else if sv <= 127.0 {
        (255.0 - (255.0 * (255.0 - bv) / (2.0 * sv)).ceil()).max(0.0)
    } else {
        (255.0 * bv / (510.0 - 2.0 * sv)).floor().min(255.0)
    };
    r / 255.0
}

/// (a\*, b\*, max(|a\*|, |b\*|)); a magnitude under 1e-9 is a neutral code.
fn chebyshev(c: [f64; 3]) -> (f64, f64, f64) {
    let (a, b) = (star(c[1]), star(c[2]));
    let m = a.abs().max(b.abs());
    (a, b, if m < 1e-9 { 0.0 } else { m })
}

/// `B(cb, cs)` of C code `mode` in a Lab document at 8 bits (`eight`) or 16.
pub fn blend(mode: i32, eight: bool, cb: [f64; 3], cs: [f64; 3]) -> [f64; 3] {
    let per = |f: fn(f64, f64) -> f64| {
        [
            separable(mode, cb[0], cs[0]),
            f(cb[1], cs[1]),
            f(cb[2], cs[2]),
        ]
    };
    match mode {
        1 | 2 => per(c5),
        12 | 15 => per(sum),
        18 if eight => [0, 1, 2].map(|k| vivid_codes(cb[k], cs[k])),
        18 => per(c4),
        21 if eight => [0, 1, 2].map(|k| hard_mix(cb[k], cs[k])),
        21 => [
            hard_mix(cb[0], cs[0]),
            c2a_hard_mix(cb[1], cs[1]),
            c2a_hard_mix(cb[2], cs[2]),
        ],
        16 => {
            if cs[0] < cb[0] {
                cs
            } else {
                cb
            }
        }
        17 => {
            if cs[0] > cb[0] {
                cs
            } else {
                cb
            }
        }
        23 => {
            let (_, _, mb) = chebyshev(cb);
            let (a, b, ms) = chebyshev(cs);
            let k = if ms > 0.0 { mb / ms } else { 0.0 };
            [cb[0], unit(a * k), unit(b * k)]
        }
        24 => {
            let (a, b, mb) = chebyshev(cb);
            let (_, _, ms) = chebyshev(cs);
            let k = if mb > 0.0 { ms / mb } else { 0.0 };
            [cb[0], unit(a * k), unit(b * k)]
        }
        25 => [cb[0], cs[1], cs[2]],
        26 => [cs[0], cb[1], cb[2]],
        _ => [0, 1, 2].map(|k| separable(mode, cb[k], cs[k])),
    }
}

/// True for the modes whose Fill folds the source (Linear Burn, Linear
/// Dodge (Add), Vivid Light).
pub fn folds(mode: i32) -> bool {
    matches!(mode, 12 | 15 | 18)
}

/// The folded source: `n + fill·(cs − n)`, n the neutral colour (L\* 100
/// for Linear Burn, 0 for Linear Dodge, 50 for Vivid Light; a\* = b\* = 0).
pub fn fold(mode: i32, cs: [f64; 3], fill: f64) -> [f64; 3] {
    let level = match mode {
        15 => 1.0,
        12 => 0.0,
        _ => 0.5,
    };
    let n = [level, 128.0 / 255.0, 128.0 / 255.0];
    [0, 1, 2].map(|k| n[k] + fill * (cs[k] - n[k]))
}

/// One opaque composite: the source over an opaque backdrop at `opacity`
/// and Fill `fill`, folded where [`folds`], else the plain `opacity · fill`.
pub fn composite(
    mode: i32,
    eight: bool,
    cb: [f64; 3],
    cs: [f64; 3],
    opacity: f64,
    fill: f64,
) -> [f64; 3] {
    let (source, alpha) = if fill < 1.0 && folds(mode) {
        (fold(mode, cs, fill), opacity)
    } else {
        (cs, opacity * fill)
    };
    let b = blend(mode, eight, cb, source);
    [0, 1, 2].map(|k| cb[k] + alpha * (b[k] - cb[k]))
}

// ------------------------------------------------------- the take's files --

/// A flat CIELab TIFF's planes at the file's depth.
pub struct CielabTiff {
    pub width: u32,
    pub height: u32,
    pub bits: u16,
    pub planes: Vec<Vec<u16>>,
}

/// Reads an uncompressed Photometric 8 (CIELab) TIFF (TIFF 6.0 §23).
pub fn read_cielab_tiff(bytes: &[u8]) -> CielabTiff {
    let little = &bytes[..2] == b"II";
    let u16_at = |at: usize| {
        let b = [bytes[at], bytes[at + 1]];
        if little {
            u16::from_le_bytes(b)
        } else {
            u16::from_be_bytes(b)
        }
    };
    let u32_at = |at: usize| {
        let b: [u8; 4] = bytes[at..at + 4].try_into().unwrap();
        if little {
            u32::from_le_bytes(b)
        } else {
            u32::from_be_bytes(b)
        }
    };
    assert_eq!(u16_at(2), 42, "a classic TIFF");
    let directory = u32_at(4) as usize;
    let mut tags = std::collections::HashMap::new();
    for i in 0..usize::from(u16_at(directory)) {
        let at = directory + 2 + i * 12;
        let (tag, kind, n) = (u16_at(at), u16_at(at + 2), u32_at(at + 4) as usize);
        let size = match kind {
            3 => 2,
            4 => 4,
            _ => continue,
        };
        let start = if size * n <= 4 {
            at + 8
        } else {
            u32_at(at + 8) as usize
        };
        let values: Vec<u64> = (0..n)
            .map(|k| {
                if size == 2 {
                    u64::from(u16_at(start + k * 2))
                } else {
                    u64::from(u32_at(start + k * 4))
                }
            })
            .collect();
        tags.insert(tag, values);
    }
    assert_eq!(tags.get(&259).map_or(1, |t| t[0]), 1, "uncompressed");
    assert_eq!(tags[&262][0], 8, "CIELab");
    let width = tags[&256][0] as u32;
    let height = tags[&257][0] as u32;
    let bits = tags[&258][0] as u16;
    let samples = tags[&277][0] as usize;
    let planar = tags.get(&284).map_or(1, |t| t[0]) == 2;
    let mut data = Vec::new();
    for (offset, length) in tags[&273].iter().zip(&tags[&279]) {
        data.extend_from_slice(&bytes[*offset as usize..(*offset + *length) as usize]);
    }
    let values: Vec<u16> = if bits == 8 {
        data.iter().map(|v| u16::from(*v)).collect()
    } else {
        data.chunks_exact(2)
            .map(|s| {
                if little {
                    u16::from_le_bytes([s[0], s[1]])
                } else {
                    u16::from_be_bytes([s[0], s[1]])
                }
            })
            .collect()
    };
    let n = (width * height) as usize;
    let planes = (0..samples)
        .map(|s| {
            if planar {
                values[s * n..(s + 1) * n].to_vec()
            } else {
                values
                    .iter()
                    .skip(s)
                    .step_by(samples)
                    .take(n)
                    .copied()
                    .collect()
            }
        })
        .collect();
    CielabTiff {
        width,
        height,
        bits,
        planes,
    }
}

/// A CIELab TIFF sample as a unit code: L\* unsigned, a\*/b\* signed (8
/// bits the byte, 16 bits /256).
pub fn cielab_unit(bits: u16, channel: usize, raw: u16) -> f64 {
    match (bits, channel) {
        (8, 0) => f64::from(raw) / 255.0,
        (8, _) => (f64::from(raw as u8 as i8) + 128.0) / 255.0,
        (_, 0) => f64::from(raw) / 65535.0,
        _ => (f64::from(raw as i16) / 256.0 + 128.0) / 255.0,
    }
}

/// A Lab PSD sample as a unit code: 8 bits v/255 on every channel; 16 bits
/// L̂ = v/65535 and a\* = (i − 16384)/128 with i = round(v·32768/65535)
/// (readback fix 2).
pub fn psd_unit(bits: u16, channel: usize, v: u16) -> f64 {
    match (bits, channel) {
        (8, _) => f64::from(v) / 255.0,
        (_, 0) => f64::from(v) / 65535.0,
        _ => {
            let i = (f64::from(v) * 32768.0 / 65535.0).round();
            ((i - 16384.0) / 128.0 + 128.0) / 255.0
        }
    }
}
