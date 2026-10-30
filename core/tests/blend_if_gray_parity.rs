//! Blend If's Gray value is Photoshop's, `0.30 R + 0.59 G + 0.11 B` of
//! the triple as stored, at 8 bits (`dr_doc_set_layer_style` +
//! `dr_doc_flattened`), 16 bits and 32 bits (`dr_precision_doc_edit`
//! `layer_blend_if`).
//!
//! The rule was fitted on Photoshop 26.10.0's receipts and confirmed on a
//! fresh take (`scripts/photoshop-probe/style_v1x_take.py`, member `W30 ·
//! Dstored`); the oracle here is that member restated in f64 — the gray of
//! the 8-bit code triple rounded to a code at 8 bits, of the 16-bit codes
//! rounded to a code at 16, of the stored linear triple unrounded at 32 —
//! through the master ramp (knees `k` at 8 bits, `257 k` above; at 32 bits
//! each zero end one 15-bit code further out), never read off the core.
//! Photoshop's own numbers are held to the residuals the take judged: the A4
//! renders committed in `fixtures/psd_photoshop/blend_if/` and take2's worst
//! patch per case.

use std::ffi::{c_char, CStr, CString};
use std::path::{Path, PathBuf};
use std::ptr;

use doctored_core::doc::DrDocument;
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_doc::dr_doc_free;
use doctored_core::ffi_precision_edit::dr_precision_doc_edit;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer as Layer};
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use image::{Rgba, RgbaImage};
use serde_json::{json, Value};

use crate::common;
use crate::float_fixture::float_doc;
use common::{flat_pixels, styled};

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
const FULL: [u8; 4] = [0, 0, 255, 255];
const SOFT: [u8; 4] = [40, 90, 170, 215];
const HARD: [u8; 4] = [0, 0, 120, 120];
const FLAT: [u8; 3] = [128, 128, 128];

// ------------------------------------------------------------ the oracle --

fn gray(rgb: [f64; 3]) -> f64 {
    0.30 * rgb[0] + 0.59 * rgb[1] + 0.11 * rgb[2]
}

fn ramp(v: f64, knees: [f64; 4]) -> f64 {
    let [lo0, lo1, hi0, hi1] = knees;
    if v < lo0 {
        0.0
    } else if v < lo1 {
        (v - lo0) / (lo1 - lo0)
    } else if v <= hi0 {
        1.0
    } else if v <= hi1 {
        (hi1 - v) / (hi1 - hi0)
    } else {
        0.0
    }
}

/// The ramp `knees` reads on `rgb` (the file's own numbers in [0, 1], or
/// linear at 32 bits) at `bits`, and how far that value sits from a rounding
/// tie (in codes; infinite at 32 bits, which does not round).
fn side(rgb: [f64; 3], knees: [u8; 4], bits: u32) -> (f64, f64) {
    let g = gray(rgb);
    match bits {
        8 => {
            let v = g * 255.0;
            (
                ramp(v.round().clamp(0.0, 255.0), knees.map(f64::from)),
                (v.fract() - 0.5).abs(),
            )
        }
        16 => {
            let v = g * 65535.0;
            let k = knees.map(|k| f64::from(k) * 257.0);
            (
                ramp(v.round().clamp(0.0, 65535.0), k),
                (v.fract() - 0.5).abs(),
            )
        }
        _ => {
            if knees == FULL {
                return (1.0, f64::INFINITY);
            }
            let ext = 65535.0 / 32768.0;
            let [lo0, lo1, hi0, hi1] = knees.map(|k| f64::from(k) * 257.0);
            (
                ramp(g * 65535.0, [lo0 - ext, lo1, hi0, hi1 + ext]),
                f64::INFINITY,
            )
        }
    }
}

/// `back + w (src − back)`, `w` = this ramp(src) × under ramp(back), and the
/// smaller tie margin of the two sides.
fn blended(
    src: [f64; 3],
    back: [f64; 3],
    this: [u8; 4],
    under: [u8; 4],
    bits: u32,
) -> ([f64; 3], f64) {
    let (a, ma) = side(src, this, bits);
    let (b, mb) = side(back, under, bits);
    let w = a * b;
    (
        std::array::from_fn(|c| back[c] + w * (src[c] - back[c])),
        ma.min(mb),
    )
}

/// 96 chromatic colours from a fixed LCG: every hue and lightness, so Rec.
/// 709 and Photoshop's Gray part on most of them.
fn colours() -> Vec<[u8; 3]> {
    let mut state = 0x2545_f491_u32;
    let mut next = || {
        state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        (state >> 24) as u8
    };
    (0..96).map(|_| [next(), next(), next()]).collect()
}

fn unit(c: [u8; 3]) -> [f64; 3] {
    c.map(|v| f64::from(v) / 255.0)
}

/// IEC 61966-2.1 decode, written here.
fn decode(v: f64) -> f64 {
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}

// ------------------------------------------------------------- plumbing --

fn blend_if_json(this: [u8; 4], under: [u8; 4]) -> Value {
    json!({"channel":"gray","this_layer":this,"underlying":under})
}

/// The 8-bit projection of `source` over `backdrop` (both `w × 1`) with a
/// Gray Blend If.
fn flat8(backdrop: &[[u8; 3]], source: &[[u8; 3]], this: [u8; 4], under: [u8; 4]) -> Vec<u8> {
    let w = backdrop.len() as u32;
    let px = |c: [u8; 3]| Rgba([c[0], c[1], c[2], 255]);
    let doc = DrDocument::from_pixels(RgbaImage::from_fn(w, 1, |x, _| px(backdrop[x as usize])))
        .adding_image_layer(
            0,
            RgbaImage::from_fn(w, 1, |x, _| px(source[x as usize])),
            "Top",
        )
        .expect("the top layer");
    let style = json!({"blend_if": blend_if_json(this, under)}).to_string();
    let handle = Box::into_raw(Box::new(styled(&doc, 1, &style)));
    let out = flat_pixels(handle);
    unsafe { dr_doc_free(handle) };
    out
}

fn take(err: *mut c_char) -> String {
    let text = unsafe { CStr::from_ptr(err) }
        .to_string_lossy()
        .into_owned();
    unsafe { dr_string_free(err) };
    text
}

fn with_blend_if(doc: &PrecisionDocument, this: [u8; 4], under: [u8; 4]) -> PrecisionDocument {
    let request = json!({"op":"layer_blend_if","version":1,"layer":1,
                         "blend_if":blend_if_json(this, under)});
    let text = CString::new(request.to_string()).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_edit(
            doc,
            text.as_ptr(),
            ptr::null(),
            0,
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(err.is_null(), "{request}: {}", take(err));
    assert!(!out.is_null(), "{request} changed nothing");
    *unsafe { Box::from_raw(out) }
}

/// The 16-bit render of `source` over `backdrop` (codes, `w × 1`).
fn render16(
    backdrop: &[[u16; 3]],
    source: &[[u16; 3]],
    this: [u8; 4],
    under: [u8; 4],
) -> Vec<[u16; 4]> {
    let w = backdrop.len() as u32;
    let image = |row: &[[u16; 3]]| {
        let pixels = row.iter().map(|c| [c[0], c[1], c[2], 65535]).collect();
        PrecisionImage::from_rgba16(w, 1, pixels, IccProfile::srgb(), None).unwrap()
    };
    let doc = PrecisionDocument::from_parts(
        w,
        1,
        IccProfile::srgb(),
        Metadata::default(),
        Resolution::default(),
        vec![
            Layer::raster("Back", image(backdrop), (0, 0), 1.0, true).unwrap(),
            Layer::raster("Top", image(source), (0, 0), 1.0, true).unwrap(),
        ],
    )
    .unwrap();
    with_blend_if(&doc, this, under)
        .render(LIMIT, 0, None)
        .unwrap()
        .to_rgba16()
        .unwrap()
}

/// The 32-bit render of `source` over `backdrop` (linear, `w × 1`).
fn render32(
    backdrop: &[[f32; 3]],
    source: &[[f32; 3]],
    this: [u8; 4],
    under: [u8; 4],
) -> Vec<[f32; 4]> {
    let w = backdrop.len() as u32;
    let records =
        [backdrop, source].map(|row| row.iter().map(|c| [c[0], c[1], c[2], 1.0]).collect());
    let doc = float_doc(w, 1, &IccProfile::srgb(), &records);
    with_blend_if(&doc, this, under)
        .render(LIMIT, 0, None)
        .unwrap()
        .to_rgba32()
        .unwrap()
}

// --------------------------------------------- the member, every depth --

#[test]
fn gray_reads_the_30_59_11_weights_at_8_bits() {
    let cs = colours();
    let flat = vec![FLAT; cs.len()];
    for (this, under, src, back) in [(SOFT, FULL, &cs, &flat), (FULL, HARD, &flat, &cs)] {
        let out = flat8(back, src, this, under);
        let mut differ_from_709 = 0;
        for x in 0..cs.len() {
            let (want, margin) = blended(unit(src[x]), unit(back[x]), this, under, 8);
            if margin < 0.01 {
                continue; // a rounding tie of the f64 and f32 sums
            }
            let got = common::pixel(&out, cs.len() as u32, x as u32, 0);
            for c in 0..3 {
                assert!(
                    (f64::from(got[c]) - want[c] * 255.0).abs() <= 0.51,
                    "{this:?}/{under:?} colour {:?}: got {got:?}, want {want:?}",
                    src[x]
                );
            }
            let luma709 = |c: [u8; 3]| {
                let v = unit(c);
                (0.2126 * v[0] + 0.7152 * v[1] + 0.0722 * v[2]) * 255.0
            };
            let probe = if this == FULL { back[x] } else { src[x] };
            let knees = if this == FULL { under } else { this };
            if ramp(luma709(probe).round(), knees.map(f64::from))
                != ramp((gray(unit(probe)) * 255.0).round(), knees.map(f64::from))
            {
                differ_from_709 += 1;
            }
        }
        assert!(
            differ_from_709 >= 5,
            "the colours tell the two lumas apart ({differ_from_709})"
        );
    }
}

#[test]
fn gray_reads_the_30_59_11_weights_at_16_bits() {
    let cs: Vec<[u16; 3]> = colours()
        .iter()
        .map(|c| c.map(|v| u16::from(v) * 257 + 3))
        .collect();
    let flat = vec![FLAT.map(|v| u16::from(v) * 257); cs.len()];
    let unit16 = |c: [u16; 3]| c.map(|v| f64::from(v) / 65535.0);
    for (this, under, src, back) in [(SOFT, FULL, &cs, &flat), (FULL, HARD, &flat, &cs)] {
        let out = render16(back, src, this, under);
        for x in 0..cs.len() {
            let (want, margin) = blended(unit16(src[x]), unit16(back[x]), this, under, 16);
            if margin < 1.0 / 32.0 {
                continue; // inside the rounding band (`native_blend_oracle`)
            }
            for c in 0..3 {
                let got = f64::from(out[x][c]) / 65535.0;
                assert!(
                    (got - want[c]).abs() * 65535.0 <= 0.65,
                    "{this:?}/{under:?} colour {:?}: got {:?}, want {want:?}",
                    src[x],
                    out[x]
                );
            }
        }
    }
}

#[test]
fn gray_reads_the_stored_linear_triple_at_32_bits() {
    let cs: Vec<[f32; 3]> = colours()
        .iter()
        .map(|c| unit(*c).map(|v| decode(v) as f32))
        .collect();
    let flat = vec![unit(FLAT).map(|v| decode(v) as f32); cs.len()];
    let f64s = |c: [f32; 3]| c.map(f64::from);
    for (this, under, src, back) in [(SOFT, FULL, &cs, &flat), (FULL, HARD, &flat, &cs)] {
        let out = render32(back, src, this, under);
        for x in 0..cs.len() {
            let (want, _) = blended(f64s(src[x]), f64s(back[x]), this, under, 32);
            for c in 0..3 {
                assert!(
                    (f64::from(out[x][c]) - want[c]).abs() <= 2e-5,
                    "{this:?}/{under:?} colour {:?}: got {:?}, want {want:?}",
                    src[x],
                    out[x]
                );
            }
        }
    }
}

// ----------------------------------------------- Photoshop's own numbers --

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/psd_photoshop/blend_if")
        .join(name)
}

/// Photoshop's own 8 → 16 promotion (`scripts/verify_phase10_photoshop_appearance.py`
/// `ps16`, measured over 240 values in its Run 3): how the A4 16-bit base
/// was written.
fn ps16(v: u8) -> u16 {
    let k = (f64::from(v) * 32768.0 / 255.0).round();
    (k * 65535.0 / 32768.0).round() as u16
}

/// max / mean / differing fraction, in 8-bit codes, of a render against
/// Photoshop's.
fn residual(got: &[[f64; 3]], photoshop: &[[f64; 3]]) -> (f64, f64, f64) {
    let (mut max, mut sum, mut differ) = (0f64, 0f64, 0usize);
    for (g, p) in got.iter().zip(photoshop) {
        let e = (0..3)
            .map(|c| (g[c] - p[c]).abs() * 255.0)
            .fold(0.0, f64::max);
        max = max.max(e);
        sum += (0..3).map(|c| (g[c] - p[c]).abs() * 255.0).sum::<f64>();
        differ += usize::from(e > 0.5);
    }
    let n = got.len() as f64;
    (max, sum / (3.0 * n), differ as f64 / n)
}

/// The A4 receipts (`doctored-fixtures/photoshop/blend-if/photoshop-blendif{8,16}`,
/// copied here): Photoshop 26.10.0's render of Base plus `this_gray_soft`
/// (the inverted picture, This Layer gray `[32, 96, 160, 224]`) and
/// `under_gray_hard` (#40607c, Underlying gray `[100, 100, 255, 255]`).
/// sha256: this_gray_soft_8 `c0bdafb4…3445667`, under_gray_hard_8
/// `c7a967b2…8c45e`, this_gray_soft_16 `ee2611be…f436f`, under_gray_hard_16
/// `41692c57…d68d`. Held to the take's judged receipt residuals: 8 bits soft
/// 7.14 / 0.549 (Photoshop's 8-bit ramp, which the Red channel shares), hard
/// no pixel; 16 bits within 0.06.
#[test]
fn photoshops_a4_gray_renders() {
    let base = image::open(fixture("base.png")).unwrap().to_rgba8();
    let inverted = image::open(fixture("inverted.png")).unwrap().to_rgba8();
    let solid = image::open(fixture("solid.png")).unwrap().to_rgba8();
    let rows8 =
        |img: &RgbaImage| -> Vec<[u8; 3]> { img.pixels().map(|p| [p[0], p[1], p[2]]).collect() };
    let (w, h) = base.dimensions();
    for (name, layer, this, under, hard) in [
        ("this_gray_soft", &inverted, [32, 96, 160, 224], FULL, false),
        ("under_gray_hard", &solid, FULL, [100, 100, 255, 255], true),
    ] {
        // 8 bits: the whole 64 x 64 picture through the FFI, row by row.
        let ps8 = image::open(fixture(&format!("{name}_8.png")))
            .unwrap()
            .to_rgba8();
        let (b8, l8) = (rows8(&base), rows8(layer));
        let mut got = Vec::new();
        for y in 0..h as usize {
            let row = y * w as usize..(y + 1) * w as usize;
            let out = flat8(&b8[row.clone()], &l8[row], this, under);
            got.extend(
                out.chunks(4)
                    .map(|p| [p[0], p[1], p[2]].map(|v| f64::from(v) / 255.0)),
            );
        }
        let ps: Vec<[f64; 3]> = ps8
            .pixels()
            .map(|p| [p[0], p[1], p[2]].map(|v| f64::from(v) / 255.0))
            .collect();
        let (max, mean, differ) = residual(&got, &ps);
        if hard {
            assert!(
                differ <= 0.005 && mean <= 0.5,
                "{name} 8: {max} / {mean} / {differ}"
            );
        } else {
            assert!(max <= 7.2 && mean <= 0.6, "{name} 8: {max} / {mean}");
        }
        // 16 bits: Photoshop's promotion of the base, Doctored's inversion
        // (65535 − code) and #40607c at 257·v — how A4 authored the file.
        let ps16img = image::open(fixture(&format!("{name}_16.png")))
            .unwrap()
            .to_rgba16();
        let b16: Vec<[u16; 3]> = b8.iter().map(|c| c.map(ps16)).collect();
        let l16: Vec<[u16; 3]> = if hard {
            l8.iter().map(|c| c.map(|v| u16::from(v) * 257)).collect()
        } else {
            b16.iter().map(|c| c.map(|v| 65535 - v)).collect()
        };
        let mut got = Vec::new();
        for y in 0..h as usize {
            let row = y * w as usize..(y + 1) * w as usize;
            let out = render16(&b16[row.clone()], &l16[row], this, under);
            got.extend(
                out.iter()
                    .map(|p| [p[0], p[1], p[2]].map(|v| f64::from(v) / 65535.0)),
            );
        }
        let ps: Vec<[f64; 3]> = ps16img
            .pixels()
            .map(|p| [p[0], p[1], p[2]].map(|v| f64::from(v) / 65535.0))
            .collect();
        let (max, mean, _) = residual(&got, &ps);
        assert!(max <= 0.07 && mean <= 0.01, "{name} 16: {max} / {mean}");
    }
}

/// take2's worst patch per case (`doctored-fixtures/photoshop/
/// style-parity-v1x/take2/job/gray_00/`): the MIX colour, the side its ramp
/// reads, and Photoshop's patch mean, as 8-bit codes (8 bits), 16-bit codes
/// ×65535 (16) or linear (32). The source is that colour over a flat
/// (128, 128, 128) backdrop for This Layer, the colour under a flat source for
/// Underlying. sha256 of the TIFFs: gray_this_soft_8 `eff738f9c2556fb4…`,
/// gray_under_soft_8 `eb00920766196478…`, gray_under_soft_16
/// `961bbe35f1613782…`, gray_this_soft_16 `31e4cf9f2ed1c3ed…`,
/// gray_under_hard_16 `57c858c5d502efe3…`, gray_this_soft_32
/// `c65d815db8c7814c…`. Judged: 2.67 code at 8 bits (the soft ramp's shared
/// 8-bit residual), 0.06 at 16, 0.00 at 32.
/// `(case, colour, this side?, ramp, bits, Photoshop's patch mean)`.
type GrayRow = (&'static str, [u8; 3], bool, [u8; 4], u32, [f64; 3]);

const TAKE2_ROWS: [GrayRow; 6] = [
    (
        "this_soft_8",
        [180, 233, 208],
        true,
        SOFT,
        8,
        [130.0, 133.0, 131.0],
    ),
    (
        "under_soft_8",
        [180, 233, 208],
        false,
        SOFT,
        8,
        [178.0, 228.0, 205.0],
    ),
    (
        "this_soft_16",
        [165, 13, 148],
        true,
        SOFT,
        16,
        [0.598_962_386_5, 0.200_350_957_5, 0.554_406_042_6],
    ),
    (
        "under_soft_16",
        [191, 211, 255],
        false,
        SOFT,
        16,
        [0.720_546_273_0, 0.789_974_822_6, 0.942_656_595_7],
    ),
    (
        "under_hard_16",
        [56, 185, 52],
        false,
        HARD,
        16,
        [0.219_607_843_1, 0.725_429_160_0, 0.203_921_568_6],
    ),
    (
        "this_soft_32",
        [215, 236, 253],
        true,
        SOFT,
        32,
        [0.311_397_194_9, 0.344_210_296_9, 0.373_766_928_9],
    ),
];

#[test]
fn photoshops_take2_worst_patches() {
    for (name, colour, this_side, knees, bits, photoshop) in TAKE2_ROWS {
        let (this, under) = if this_side {
            (knees, FULL)
        } else {
            (FULL, knees)
        };
        let (src, back) = if this_side {
            (colour, FLAT)
        } else {
            (FLAT, colour)
        };
        let (got, bound): ([f64; 3], f64) = match bits {
            8 => {
                let out = flat8(&[back], &[src], this, under);
                ([out[0], out[1], out[2]].map(f64::from), 2.67 + 1.0)
            }
            16 => {
                let c16 = |c: [u8; 3]| c.map(|v| u16::from(v) * 257);
                let out = render16(&[c16(back)], &[c16(src)], this, under)[0];
                (
                    [out[0], out[1], out[2]].map(|v| f64::from(v) / 65535.0 * 255.0),
                    0.06 + 0.01,
                )
            }
            _ => {
                let lin = |c: [u8; 3]| unit(c).map(|v| decode(v) as f32);
                let out = render32(&[lin(back)], &[lin(src)], this, under)[0];
                // Compared on the encoded value × 255, the take's measure.
                let enc = |v: f64| {
                    let v = v.clamp(0.0, 1.0);
                    if v <= 0.003_130_8 {
                        12.92 * v
                    } else {
                        1.055 * v.powf(1.0 / 2.4) - 0.055
                    }
                };
                (
                    [out[0], out[1], out[2]].map(|v| enc(f64::from(v)) * 255.0),
                    0.01,
                )
            }
        };
        let want: [f64; 3] = match bits {
            8 => photoshop,
            16 => photoshop.map(|v| v * 255.0),
            _ => photoshop.map(|v| {
                let v = v.clamp(0.0, 1.0);
                (if v <= 0.003_130_8 {
                    12.92 * v
                } else {
                    1.055 * v.powf(1.0 / 2.4) - 0.055
                }) * 255.0
            }),
        };
        for c in 0..3 {
            assert!(
                (got[c] - want[c]).abs() <= bound,
                "{name} channel {c}: got {}, Photoshop {} (bound {bound})",
                got[c],
                want[c]
            );
        }
    }
}
