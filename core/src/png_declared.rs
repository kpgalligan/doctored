//! A PNG's bare colour declaration: the `gAMA` and `cHRM` chunks of a file
//! that carries neither an `iCCP` nor an `sRGB` chunk, read as the ICC
//! profile they describe.
//!
//! # The rule
//!
//! Precedence is `iCCP`, then `sRGB`, then `gAMA`/`cHRM`: a file with either
//! of the first two is described by it and these chunks are not read
//! ([`declared`] answers `None`). Otherwise each half the file does not
//! declare is sRGB's: no `cHRM` is sRGB's primaries and D65 white, no `gAMA`
//! is the sRGB curve. A gray image reads `gAMA` alone: it has no primaries,
//! so a `cHRM` on it declares nothing and is not read. The samples are never
//! converted at open; the synthesized profile says what they mean, exactly as
//! an `iCCP` holding it would, and every open that reads an `iCCP` reads it in
//! that chunk's place (`precision_codec`, `gray_io`, `indexed_io`, `dr_image`).
//! Only the chunks before the first `IDAT` are read, where the PNG
//! specification puts all four.
//!
//! # Which values are usable
//!
//! `gAMA` is the file's encoding exponent × 100000, so the decode exponent is
//! `100000 / gAMA`. It is usable from 0.1 to 10: zero is the specification's
//! own "no value", and an exponent outside that range describes no display or
//! capture curve, only a writer defect. `cHRM` is the white and the red,
//! green and blue primaries as xy × 100000, usable when every y is positive
//! and the primaries are a basis whose D50 columns an s15Fixed16 holds. A
//! chunk of the wrong length is unusable too. An unusable declaration is an
//! `Err` with one clause: the typed opens refuse with it
//! (`precision_png::png_profile_contract`) and the flat open reads the
//! numbers as sRGB under it (`open_profile::declaration_ignored`).
//!
//! # The profiles
//!
//! RGB ([`rgb_blob`]): a v4.3 `mntr` RGB/XYZ matrix profile with `desc` and
//! `cprt` as `mluc`, `wtpt` D50, the three columns, three TRC tags sharing one
//! curve, and `chad`. The columns are the file's primaries adapted from its
//! white to D50 by Bradford (`float_primaries::rgb_to_xyz` and
//! `adjust_white_balance::bradford`, the crate's copies), or sRGB's own
//! columns and `chad` (`icc_builtin`) with no `cHRM`. The curve is a `para`
//! of function type 0 holding the decode exponent as s15Fixed16, within
//! 1.5e-5 of it, or sRGB's 1024-entry `curv` with no `gAMA`. A `curv` of
//! count 1 would hold the exponent as u8Fixed8 only, 1/0.45455 as 563/256,
//! which is up to about 8 codes of 65535 off at mid-tone. `para` is a v4 tag
//! type, so the header is v4.3, not the built-ins' v2.1.0. Gray
//! ([`gray_blob`]): a v4.3 GRAY/XYZ profile with `desc`, `cprt`, `wtpt` and
//! that same `para` as its `kTRC`, the shape of `icc_builtin::gray_blob`.
//!
//! The name says where the profile came from: "PNG gamma 2.2000" (the decode
//! exponent to four places), with ", file primaries" when a `cHRM` was read,
//! and "PNG sRGB curve, file primaries" for a `cHRM` with no `gAMA`.

use crate::adjust_white_balance::{bradford, mat_mul};
use crate::exr_float::Chromaticities;
use crate::icc_builtin::{assemble, curv_tag, mluc_tag, para_tag, trc_table, xyz_tag, Builtin};
use crate::icc_lut::D50;

/// What a PNG declares about its colour, each half `None` when the file does
/// not declare it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Declared {
    /// The decode exponent, `100000 / gAMA`.
    pub(crate) gamma: Option<f64>,
    /// Red, green, blue and white xy, from `cHRM`.
    pub(crate) chromaticities: Option<Chromaticities>,
}

/// The decode exponents [`declared`] admits (module doc).
const GAMMA_RANGE: std::ops::RangeInclusive<f64> = 0.1..=10.0;

/// The bare declaration of the PNG `png` (the whole file, or its signature
/// and the chunks before `IDAT`), read as a gray image's when `gray`.
/// `Ok(None)` when an `iCCP` or `sRGB` chunk is present or nothing is
/// declared; `Err` with one clause for an unusable value (module doc).
pub(crate) fn declared(png: &[u8], gray: bool) -> Result<Option<Declared>, String> {
    let (mut gama, mut chrm, mut described) = (None, None, false);
    let mut at = 8_usize;
    while let Some(head) = png.get(at..at.saturating_add(8)) {
        let size = u32::from_be_bytes([head[0], head[1], head[2], head[3]]) as usize;
        let kind = &head[4..8];
        if kind == b"IDAT" || kind == b"IEND" {
            break;
        }
        let Some(data) = at
            .checked_add(8)
            .and_then(|start| png.get(start..start.checked_add(size)?))
        else {
            break;
        };
        match kind {
            b"iCCP" | b"sRGB" => described = true,
            b"gAMA" if gama.is_none() => gama = Some(data),
            b"cHRM" if chrm.is_none() && !gray => chrm = Some(data),
            _ => {}
        }
        at = match at.checked_add(size).and_then(|v| v.checked_add(12)) {
            Some(next) => next,
            None => break,
        };
    }
    if described || (gama.is_none() && chrm.is_none()) {
        return Ok(None);
    }
    let gamma = gama.map(decode_exponent).transpose()?;
    let chromaticities = chrm.map(primaries).transpose()?;
    Ok(Some(Declared {
        gamma,
        chromaticities,
    }))
}

/// The ICC profile a PNG's bare declaration describes ([`declared`]), or
/// `None` when it declares nothing usable for a `gray` or RGB image: no
/// declaration, an `iCCP` or `sRGB` chunk, an unusable value, or a gray
/// image with no `gAMA`.
pub(crate) fn profile(png: &[u8], gray: bool) -> Option<Vec<u8>> {
    let declared = declared(png, gray).ok()??;
    if gray {
        declared.gamma.map(gray_blob)
    } else {
        Some(rgb_blob(&declared))
    }
}

/// `gAMA`'s four bytes as the decode exponent.
fn decode_exponent(data: &[u8]) -> Result<f64, String> {
    let raw: [u8; 4] = data
        .try_into()
        .map_err(|_| "the PNG's gAMA chunk is not 4 bytes long".to_string())?;
    let file = u32::from_be_bytes(raw);
    if file == 0 {
        return Err("the PNG's gAMA chunk is 0, which declares no gamma".into());
    }
    let exponent = 100_000.0 / f64::from(file);
    if !GAMMA_RANGE.contains(&exponent) {
        return Err(format!(
            "the PNG's gAMA chunk declares a decode exponent of {exponent:.4}, outside 0.1 to 10"
        ));
    }
    Ok(exponent)
}

/// `cHRM`'s 32 bytes (white, red, green, blue) as red, green, blue, white.
fn primaries(data: &[u8]) -> Result<Chromaticities, String> {
    const DEGENERATE: &str =
        "the PNG's cHRM chunk does not describe three primaries and a white point";
    if data.len() != 32 {
        return Err("the PNG's cHRM chunk is not 32 bytes long".into());
    }
    let value = |i: usize| {
        let raw = [
            data[4 * i],
            data[4 * i + 1],
            data[4 * i + 2],
            data[4 * i + 3],
        ];
        (f64::from(u32::from_be_bytes(raw)) / 100_000.0) as f32
    };
    let xy = |i: usize| [value(2 * i), value(2 * i + 1)];
    let chromaticities = [xy(1), xy(2), xy(3), xy(0)];
    if chromaticities.iter().any(|pair| pair[1] <= 0.0) {
        return Err(DEGENERATE.into());
    }
    adapted_columns(&chromaticities).ok_or(DEGENERATE)?;
    Ok(chromaticities)
}

/// The three D50 columns and the `chad` of `chromaticities` as raw
/// s15Fixed16; `None` when they are not a basis or a value does not fit.
fn adapted_columns(chromaticities: &Chromaticities) -> Option<([[i32; 3]; 3], [i32; 9])> {
    let (matrix, white) = crate::float_primaries::rgb_to_xyz(chromaticities)?;
    let adaptation = bradford(white, D50);
    let adapted = mat_mul(adaptation, matrix);
    let fixed = |v: f64| {
        let raw = (v * 65536.0).round();
        (raw.is_finite() && raw.abs() < f64::from(i32::MAX)).then_some(raw as i32)
    };
    let mut columns = [[0_i32; 3]; 3];
    for (k, column) in columns.iter_mut().enumerate() {
        for (i, slot) in column.iter_mut().enumerate() {
            *slot = fixed(adapted[i][k])?;
        }
    }
    let mut chad = [0_i32; 9];
    for (slot, v) in chad.iter_mut().zip(adaptation.iter().flatten()) {
        *slot = fixed(*v)?;
    }
    Some((columns, chad))
}

/// The profile's name (module doc).
fn name(declared: &Declared) -> String {
    let curve = match declared.gamma {
        Some(gamma) => format!("PNG gamma {gamma:.4}"),
        None => "PNG sRGB curve".to_string(),
    };
    match declared.chromaticities {
        Some(_) => format!("{curve}, file primaries"),
        None => curve,
    }
}

/// The decode exponent as a `para` function type 0.
fn gamma_tag(gamma: f64) -> Vec<u8> {
    para_tag((gamma * 65536.0).round() as i32)
}

/// The RGB matrix profile of `declared` (module doc). A `cHRM` that
/// [`declared`] admitted always has columns; were it not to, sRGB's stand.
pub(crate) fn rgb_blob(declared: &Declared) -> Vec<u8> {
    let srgb = (Builtin::Srgb.columns(), Builtin::Srgb.chad());
    let (columns, chad) = declared
        .chromaticities
        .and_then(|c| adapted_columns(&c))
        .unwrap_or(srgb);
    let curve = match declared.gamma {
        Some(gamma) => gamma_tag(gamma),
        None => curv_tag(&trc_table()),
    };
    let chad_tag = crate::icc_builtin::sf32_tag(&chad);
    let tags: [(&[u8; 4], Vec<u8>); 10] = [
        (b"desc", mluc_tag(&name(declared))),
        (b"cprt", mluc_tag("Public Domain")),
        (b"wtpt", xyz_tag(crate::icc_builtin::D50)),
        (b"rXYZ", xyz_tag(columns[0])),
        (b"gXYZ", xyz_tag(columns[1])),
        (b"bXYZ", xyz_tag(columns[2])),
        (b"rTRC", curve.clone()),
        (b"gTRC", curve.clone()),
        (b"bTRC", curve),
        (b"chad", chad_tag),
    ];
    assemble(&tags, V4_3, b"RGB ", b"XYZ ")
}

/// The Gray profile of a gray PNG's decode exponent `gamma` (module doc).
pub(crate) fn gray_blob(gamma: f64) -> Vec<u8> {
    let declared = Declared {
        gamma: Some(gamma),
        chromaticities: None,
    };
    let tags: [(&[u8; 4], Vec<u8>); 4] = [
        (b"desc", mluc_tag(&name(&declared))),
        (b"cprt", mluc_tag("Public Domain")),
        (b"wtpt", xyz_tag(crate::icc_builtin::D50)),
        (b"kTRC", gamma_tag(gamma)),
    ];
    assemble(&tags, V4_3, b"GRAY", b"XYZ ")
}

/// ICC version 4.3.0, the header version both profiles carry.
const V4_3: [u8; 4] = [0x04, 0x30, 0x00, 0x00];
