//! An independent oracle for Lab files (#1 Lab phase 3a), written from
//! the decided table of `scripts/photoshop-probe/lab_files_take.py` (its
//! take1 RESULT, and the 16-bit PSD a\*/b\* on Photoshop's 15-bit grid that
//! the readback fix measured), Adobe's file format document, TIFF 6.0
//! §23 (CIELab) and TIFF Technote 4 (ICCLab). It never calls the importer it
//! checks:
//!
//! * the file-code formulas as L\*/a\* UNITS ([`psd_units`] and
//!   [`cielab_units`], thin wrappers over `lab_blend_oracle`'s one decode
//!   each, and [`icclab_units`]) and the document's ICC v4 code of a unit
//!   value ([`icc_code`]), plus their inverses for synthetic files;
//! * an uncompressed Lab TIFF reader and writer of either byte order
//!   ([`read_lab_tiff`], [`write_lab_tiff`]);
//! * Lab synthetic builds over `cmyk_psd_fixture`'s generic `plane`, `Rec`,
//!   `layer_info`, `psd` and `layered_tiff`, with the header's colour word 9
//!   and Photometric 8 patched in ([`lab_psd`], [`lab_layered_tiff`]).
//!
//! The ICCLab (Photometric 9) arms are the published spec's, DECLARED, not
//! measured against Photoshop (the take observed Photometric 8 only).
#![allow(dead_code)]

use crate::cmyk_psd_fixture::{self as fx, Comp, PsdSpec, Rec};
use crate::lab_blend_oracle::{cielab_unit, psd_unit, star};

// ------------------------------------------------------------- formulas --

/// L\* (0…100) or a\*/b\* of a unit code.
fn units(channel: usize, u: f64) -> f64 {
    if channel == 0 {
        100.0 * u
    } else {
        star(u)
    }
}

/// A PSD sample's unit value (P8; 16 bits L\* = 100v/65535 and a\* on the
/// 15-bit grid): `lab_blend_oracle::psd_unit`.
pub fn psd_units(bits: u16, channel: usize, v: u16) -> f64 {
    units(channel, psd_unit(bits, channel, v))
}

/// A CIELab TIFF sample's unit value (TIFF 6.0 §23, a\*/b\* signed):
/// `lab_blend_oracle::cielab_unit`.
pub fn cielab_units(bits: u16, channel: usize, raw: u16) -> f64 {
    units(channel, cielab_unit(bits, channel, raw))
}

/// An ICCLab TIFF sample's unit value (TIFF Technote 4; 16 bits the ICC v2
/// legacy encoding). Declared, not measured.
pub fn icclab_units(bits: u16, channel: usize, raw: u16) -> f64 {
    let v = f64::from(raw);
    match (bits, channel) {
        (8, 0) => 100.0 * v / 255.0,
        (8, _) => v - 128.0,
        (_, 0) => 100.0 * v / 65280.0,
        _ => v / 256.0 - 128.0,
    }
}

/// A unit value as the document's ICC v4 16-bit code, unrounded: L\* =
/// 100·c/65535, a\* = 255·c/65535 − 128.
pub fn icc_code(channel: usize, unit: f64) -> f64 {
    if channel == 0 {
        unit / 100.0 * 65535.0
    } else {
        (unit + 128.0) / 255.0 * 65535.0
    }
}

/// [`icc_code`] rounded and clamped.
pub fn code_of(channel: usize, unit: f64) -> u16 {
    icc_code(channel, unit).round().clamp(0.0, 65535.0) as u16
}

/// A document code's unit value.
pub fn units_of(channel: usize, code: u16) -> f64 {
    if channel == 0 {
        100.0 * f64::from(code) / 65535.0
    } else {
        255.0 * f64::from(code) / 65535.0 - 128.0
    }
}

/// The PSD sample of a unit value (the inverse of [`psd_units`]): 16 bits
/// a\* as Photoshop writes its 15-bit value, v = round(i·65535/32768) with
/// i = round(a\*·128) + 16384.
pub fn psd_sample(bits: u16, channel: usize, unit: f64) -> u16 {
    let v = match (bits, channel) {
        (8, 0) => unit * 255.0 / 100.0,
        (8, _) => unit + 128.0,
        (_, 0) => unit * 65535.0 / 100.0,
        _ => ((unit * 128.0).round() + 16384.0).clamp(0.0, 32768.0) * 65535.0 / 32768.0,
    };
    let top = if bits == 8 { 255.0 } else { 65535.0 };
    v.round().clamp(0.0, top) as u16
}

/// The CIELab TIFF sample (raw bits) of a unit value.
pub fn cielab_sample(bits: u16, channel: usize, unit: f64) -> u16 {
    match (bits, channel) {
        (8, 0) => (unit * 255.0 / 100.0).round().clamp(0.0, 255.0) as u16,
        (8, _) => u16::from(unit.round().clamp(-128.0, 127.0) as i8 as u8),
        (_, 0) => (unit * 65535.0 / 100.0).round().clamp(0.0, 65535.0) as u16,
        _ => (unit * 256.0).round().clamp(-32768.0, 32767.0) as i16 as u16,
    }
}

/// The ICCLab TIFF sample of a unit value (declared encoding).
pub fn icclab_sample(bits: u16, channel: usize, unit: f64) -> u16 {
    let (v, top) = match (bits, channel) {
        (8, 0) => (unit * 255.0 / 100.0, 255.0),
        (8, _) => (unit + 128.0, 255.0),
        (_, 0) => (unit * 65280.0 / 100.0, 65535.0),
        _ => ((unit + 128.0) * 256.0, 65535.0),
    };
    v.round().clamp(0.0, top) as u16
}

// ------------------------------------------------------------ the TIFFs --

/// A flat TIFF's directory and uncompressed samples (one plane a sample, raw
/// unsigned bits at the file's depth).
#[derive(Debug, Clone)]
pub struct LabTiffRead {
    pub width: u32,
    pub height: u32,
    pub photometric: u16,
    pub bits: u16,
    pub samples: usize,
    pub extra_samples: Vec<u16>,
    pub sample_format: Option<Vec<u16>>,
    pub planes: Vec<Vec<u16>>,
}

/// IFD 0 of an uncompressed chunky or planar TIFF of either byte order.
pub fn read_lab_tiff(bytes: &[u8]) -> LabTiffRead {
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
    let directory = u32_at(4) as usize;
    let count = usize::from(u16_at(directory));
    let mut tags = std::collections::HashMap::new();
    for i in 0..count {
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
    let width = tags[&256][0] as u32;
    let height = tags[&257][0] as u32;
    let bits = tags[&258][0] as u16;
    let samples = tags.get(&277).map_or(1, |t| t[0]) as usize;
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
    let pixels = width as usize * height as usize;
    let planes = (0..samples)
        .map(|c| {
            (0..pixels)
                .map(|i| {
                    if planar {
                        values[c * pixels + i]
                    } else {
                        values[i * samples + c]
                    }
                })
                .collect()
        })
        .collect();
    LabTiffRead {
        width,
        height,
        photometric: tags[&262][0] as u16,
        bits,
        samples,
        extra_samples: tags
            .get(&338)
            .map_or(Vec::new(), |t| t.iter().map(|v| *v as u16).collect()),
        sample_format: tags
            .get(&339)
            .map(|t| t.iter().map(|v| *v as u16).collect()),
        planes,
    }
}

/// What [`write_lab_tiff`] writes beyond the samples.
pub struct TiffShape {
    pub photometric: u16,
    pub bits: u16,
    pub big: bool,
    /// ExtraSamples (a fourth sample when `Some`).
    pub extra: Option<u16>,
    pub sample_format: Option<Vec<u16>>,
    pub planar: bool,
}

/// One uncompressed strip of `samples` (raw bits, interleaved, 3 or 4 a
/// pixel) as a TIFF of `shape` (a planar one keeps ONE strip of every plane
/// in turn: only the source test reads it, and refuses it).
pub fn write_lab_tiff(width: u32, height: u32, samples: &[u16], shape: &TiffShape) -> Vec<u8> {
    let per = 3 + usize::from(shape.extra.is_some());
    let pixels = width as usize * height as usize;
    assert_eq!(samples.len(), pixels * per);
    let ordered: Vec<u16> = if shape.planar {
        (0..per)
            .flat_map(|c| (0..pixels).map(move |i| samples[i * per + c]))
            .collect()
    } else {
        samples.to_vec()
    };
    let put16 = |v: u16| {
        if shape.big {
            v.to_be_bytes()
        } else {
            v.to_le_bytes()
        }
    };
    let put32 = |v: u32| {
        if shape.big {
            v.to_be_bytes()
        } else {
            v.to_le_bytes()
        }
    };
    let mut data = Vec::new();
    for v in ordered {
        if shape.bits == 8 {
            data.push(v as u8);
        } else {
            data.extend(put16(v));
        }
    }
    let mut out = if shape.big {
        b"MM\0*".to_vec()
    } else {
        b"II*\0".to_vec()
    };
    out.extend([0; 4]);
    let strip_at = out.len() as u32;
    out.extend(&data);
    if out.len() % 2 == 1 {
        out.push(0);
    }
    let shorts = |values: &[u16], out: &mut Vec<u8>| -> [u8; 4] {
        if values.len() <= 2 {
            let mut field = [0u8; 4];
            for (i, v) in values.iter().enumerate() {
                field[i * 2..i * 2 + 2].copy_from_slice(&put16(*v));
            }
            field
        } else {
            let at = out.len() as u32;
            for v in values {
                out.extend(put16(*v));
            }
            put32(at)
        }
    };
    let bits = shorts(&vec![shape.bits; per], &mut out);
    let formats = shape
        .sample_format
        .as_ref()
        .map(|f| (f.len(), shorts(f, &mut out)));
    let directory = out.len() as u32;
    let short = |v: u16| {
        let mut field = [0u8; 4];
        field[..2].copy_from_slice(&put16(v));
        field
    };
    let mut entries: Vec<(u16, u16, u32, [u8; 4])> = vec![
        (256, 4, 1, put32(width)),
        (257, 4, 1, put32(height)),
        (258, 3, per as u32, bits),
        (259, 3, 1, short(1)),
        (262, 3, 1, short(shape.photometric)),
        (273, 4, 1, put32(strip_at)),
        (277, 3, 1, short(per as u16)),
        (278, 4, 1, put32(height)),
        (279, 4, 1, put32(data.len() as u32)),
        (284, 3, 1, short(if shape.planar { 2 } else { 1 })),
    ];
    if let Some(extra) = shape.extra {
        entries.push((338, 3, 1, short(extra)));
    }
    if let Some((n, field)) = formats {
        entries.push((339, 3, n as u32, field));
    }
    out.extend(put16(entries.len() as u16));
    for (tag, kind, count, value) in entries {
        out.extend(put16(tag));
        out.extend(put16(kind));
        out.extend(put32(count));
        out.extend(value);
    }
    out.extend(put32(0));
    let head = put32(directory);
    out[4..8].copy_from_slice(&head);
    out
}

// -------------------------------------------------------- the PSD files --

/// A raster's channels from PSD-stored Lab samples `[L, a, b, alpha]` at
/// `depth` (already the file's codes): `-1` (when `alpha`) then `0`–`2`.
pub fn lab_channels(
    samples: &[[u16; 4]],
    width: usize,
    depth: u16,
    alpha: bool,
    comp: Comp,
    large: bool,
) -> Vec<(i16, Vec<u8>)> {
    let height = samples.len() / width.max(1);
    let mut out = Vec::new();
    if alpha {
        let values: Vec<u16> = samples.iter().map(|s| s[3]).collect();
        out.push((-1, fx::plane(&values, width, height, depth, comp, large)));
    }
    for c in 0..3 {
        let values: Vec<u16> = samples.iter().map(|s| s[c]).collect();
        out.push((
            c as i16,
            fx::plane(&values, width, height, depth, comp, large),
        ));
    }
    out
}

/// A pixel-less Lab record (four placeholder channels).
pub fn lab_empty(name: &str) -> Rec {
    Rec::raster(
        name,
        [0; 4],
        [-1i16, 0, 1, 2]
            .into_iter()
            .map(|id| (id, vec![0, 0]))
            .collect(),
    )
}

/// A Lab folder record.
pub fn lab_folder(name: &str, divider: u32, blend: [u8; 4]) -> Rec {
    Rec {
        divider,
        blend,
        flags: 0x18,
        ..lab_empty(name)
    }
}

/// A colour-mode-9 PSD (or PSB): `cmyk_psd_fixture::psd` with the header's
/// colour word patched to 9 (the composite planes are STORED Lab samples).
pub fn lab_psd(spec: &PsdSpec<'_>, records: &[Rec]) -> Vec<u8> {
    let mut bytes = fx::psd(spec, records);
    bytes[24..26].copy_from_slice(&9u16.to_be_bytes());
    bytes
}

/// A big-endian layered Lab TIFF: `cmyk_psd_fixture::layered_tiff` (its
/// flat strip five samples) with Photometric patched to 8 (CIELab).
pub fn lab_layered_tiff(
    canvas: (u32, u32),
    depth: u16,
    strip: &[[u16; 5]],
    records: &[Rec],
) -> Vec<u8> {
    let mut bytes = fx::layered_tiff(canvas, depth, None, strip, records);
    let directory = u32::from_be_bytes(bytes[4..8].try_into().unwrap()) as usize;
    let count = usize::from(u16::from_be_bytes([bytes[directory], bytes[directory + 1]]));
    for i in 0..count {
        let at = directory + 2 + i * 12;
        if u16::from_be_bytes([bytes[at], bytes[at + 1]]) == 262 {
            bytes[at + 8..at + 10].copy_from_slice(&8u16.to_be_bytes());
        }
    }
    bytes
}
