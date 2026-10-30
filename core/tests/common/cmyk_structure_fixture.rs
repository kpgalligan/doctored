//! The CMYK structure files' shared fixtures and f64 oracle (#547 phase
//! 4a: `cmyk_blend.rs`, `cmyk_structure.rs`, `cmyk_merge.rs`).
//!
//! The documents are built through the FFI: an RGB document of layers (any
//! offsets, any alpha) converted into the synthetic press profile
//! (`common/cmyk_fixture.rs`), then edited in ink. The ORACLE never reads the
//! implementation: it reads each document's own records through the public
//! layer API (stored samples, offsets, opacity, mode, clipping, Fill, masks)
//! and composites them per pixel in f64, written here from the W3C
//! Compositing and Blending spec on the stored domain `L = 1 − ink`, with
//! the K and Darker/Lighter Color rules of `cmyk_composite`'s module doc,
//! Photoshop's group semantics (an isolated group's own buffer, a Pass
//! Through group gated back by opacity × mask) and its clip-unit rule (the
//! base drawn at Normal with its Fill, footprint alpha × mask, members
//! clamped to it, the unit composited by the base's mode and opacity).

#![allow(dead_code)]

use std::ffi::{CStr, CString};
use std::ptr;
use std::sync::Arc;

use doctored_core::doc::BlendMode;
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_cmyk::{dr_precision_doc_cmyk_sample, dr_precision_doc_convert_to_cmyk};
use doctored_core::ffi_precision::{
    dr_precision_doc_free, dr_precision_doc_open, dr_precision_doc_save_native,
    dr_precision_doc_set_layer_opacity,
};
use doctored_core::ffi_precision_color::dr_precision_doc_add_adjustment;
use doctored_core::ffi_precision_edit::{dr_precision_doc_edit, dr_precision_doc_edit_plan};
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer};
use doctored_core::precision_image::PrecisionImage;
use serde_json::Value;

use crate::cmyk_fixture::press_profile;
use crate::common::{cpath, take_err_string};

pub const LIMIT: u64 = 2 * 1024 * 1024 * 1024;

/// A straight CMYKA sample in [0, 1].
pub type Ink = [f64; 5];

pub fn free(document: *mut PrecisionDocument) {
    unsafe { dr_precision_doc_free(document) };
}

pub fn json(text: *mut std::ffi::c_char, err: *mut std::ffi::c_char) -> Value {
    assert!(!text.is_null(), "{}", take_err_string(err));
    let value = serde_json::from_str(unsafe { CStr::from_ptr(text) }.to_str().unwrap()).unwrap();
    unsafe { dr_string_free(text) };
    value
}

/// One RGB layer of the source document: `pixels` (u16 sRGB, row-major)
/// `w × h` at `offset`.
pub struct RgbLayer {
    pub name: &'static str,
    pub w: u32,
    pub h: u32,
    pub offset: (i32, i32),
    pub pixels: Vec<[u16; 4]>,
}

/// A varied picture with every alpha class (opaque, partial, zero), seeded
/// by `seed`.
pub fn varied(w: u32, h: u32, seed: u32) -> Vec<[u16; 4]> {
    (0..w * h)
        .map(|i| {
            let j = i
                .wrapping_mul(2_654_435_761)
                .wrapping_add(seed.wrapping_mul(40_503));
            let alpha = match (i + seed) % 5 {
                0 | 1 => 65535,
                2 => 40000,
                3 => 0,
                _ => 12000,
            };
            [
                (j % 65536) as u16,
                (j.rotate_left(11) % 65536) as u16,
                (j.rotate_left(23) % 65536) as u16,
                alpha,
            ]
        })
        .collect()
}

/// A `w × h` CMYK document converted (into the press profile) from an RGB
/// document of `layers`, bottom first, each Normal, opaque, visible.
pub fn cmyk_layers(w: u32, h: u32, layers: Vec<RgbLayer>) -> *mut PrecisionDocument {
    let srgb = IccProfile::srgb();
    let records = layers
        .into_iter()
        .map(|layer| {
            let image =
                PrecisionImage::from_rgba16(layer.w, layer.h, layer.pixels, srgb.clone(), None)
                    .unwrap();
            PrecisionDocumentLayer::raster(layer.name, image, layer.offset, 1.0, true).unwrap()
        })
        .collect();
    let rgb = Box::into_raw(Box::new(
        PrecisionDocument::from_parts(
            w,
            h,
            srgb,
            Metadata::default(),
            Resolution::default(),
            records,
        )
        .unwrap(),
    ));
    let profile = press_profile("Press Test");
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_convert_to_cmyk(
            rgb,
            profile.as_ptr(),
            profile.len(),
            c"{}".as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!out.is_null(), "to CMYK: {}", take_err_string(err));
    free(rgb);
    out
}

/// `dr_precision_doc_edit`: `Ok(null)` for an exact no-op.
pub fn try_edit(
    doc: *const PrecisionDocument,
    request: &str,
    samples: &[u16],
) -> Result<*mut PrecisionDocument, String> {
    let text = CString::new(request).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_edit(
            doc,
            text.as_ptr(),
            if samples.is_empty() {
                ptr::null()
            } else {
                samples.as_ptr()
            },
            samples.len(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    if out.is_null() && !err.is_null() {
        Err(take_err_string(err))
    } else {
        Ok(out)
    }
}

/// An edit that must produce a new document; the input is FREED, so a
/// chain of edits reads as one value.
pub fn then(doc: *mut PrecisionDocument, request: &str, samples: &[u16]) -> *mut PrecisionDocument {
    let out = try_edit(doc, request, samples).unwrap_or_else(|e| panic!("{request}: {e}"));
    assert!(!out.is_null(), "{request} changed nothing");
    free(doc);
    out
}

/// The edit's refusal sentence.
pub fn refusal(doc: *const PrecisionDocument, request: &str, samples: &[u16]) -> String {
    match try_edit(doc, request, samples) {
        Err(e) => e,
        Ok(out) => {
            if !out.is_null() {
                free(out);
            }
            panic!("{request} was not refused")
        }
    }
}

/// `dr_precision_doc_edit_plan`.
pub fn plan(doc: *const PrecisionDocument, request: &str) -> Result<Value, String> {
    let text = CString::new(request).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe { dr_precision_doc_edit_plan(doc, text.as_ptr(), LIMIT, 0, &mut err) };
    if out.is_null() {
        return Err(take_err_string(err));
    }
    Ok(json(out, err))
}

/// Layer `index`'s opacity, the input freed.
pub fn with_opacity(
    doc: *mut PrecisionDocument,
    index: usize,
    opacity: f32,
) -> *mut PrecisionDocument {
    let mut err = ptr::null_mut();
    let out = unsafe { dr_precision_doc_set_layer_opacity(doc, index, opacity, &mut err) };
    assert!(!out.is_null(), "opacity: {}", take_err_string(err));
    free(doc);
    out
}

/// Layer `index`'s blend mode (`DrBlendMode`), the input freed.
pub fn with_mode(doc: *mut PrecisionDocument, index: usize, mode: i32) -> *mut PrecisionDocument {
    then(
        doc,
        &format!(r#"{{"op":"layer_blend","version":1,"layer":{index},"blend_mode":{mode}}}"#),
        &[],
    )
}

/// Layer `index`'s Fill Opacity, the input freed.
pub fn with_fill(doc: *mut PrecisionDocument, index: usize, fill: f64) -> *mut PrecisionDocument {
    then(
        doc,
        &format!(
            r#"{{"op":"layer_style","version":1,"layer":{index},"style":{{"fill_opacity":{fill}}}}}"#
        ),
        &[],
    )
}

/// A canvas-sized layer mask of `samples` on `index`, the input freed.
pub fn with_mask(
    doc: *mut PrecisionDocument,
    index: usize,
    samples: &[u16],
) -> *mut PrecisionDocument {
    let (w, h) = unsafe { &*doc }.dimensions();
    then(
        doc,
        &format!(r#"{{"op":"set_mask","layer":{index},"width":{w},"height":{h}}}"#),
        samples,
    )
}

/// A mask ramp over the canvas, seeded by `seed`.
pub fn ramp(w: u32, h: u32, seed: u32) -> Vec<u16> {
    (0..w * h)
        .map(|i| ((i * 9973 + seed * 17_389) % 65536) as u16)
        .collect()
}

/// The Levels description the tests' adjustment records carry: the output
/// range alone, `L' = 0.1 + 0.7·L` on `L = 1 − ink`.
pub const LEVELS: &str = r#"{"type":"adjust","evaluation":"precision_v1","op":"levels","params":{"output_black":0.1,"output_white":0.8}}"#;

/// [`LEVELS`] on one ink (the oracle of the tests' adjustment records).
pub fn levels(ink: [f64; 4]) -> [f64; 4] {
    ink.map(|v| 1.0 - (0.1 + 0.7 * (1.0 - v)))
}

/// A [`LEVELS`] record added on top at `opacity`, the input freed.
pub fn with_levels(doc: *mut PrecisionDocument, opacity: f32) -> *mut PrecisionDocument {
    let text = CString::new(LEVELS).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_add_adjustment(
            doc,
            c"Levels".as_ptr(),
            text.as_ptr(),
            opacity,
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!out.is_null(), "levels: {}", take_err_string(err));
    free(doc);
    out
}

pub fn save(doc: *const PrecisionDocument, path: &std::path::Path) -> Vec<u8> {
    let c = cpath(path);
    let mut err = ptr::null_mut();
    assert!(
        unsafe { dr_precision_doc_save_native(doc, c.as_ptr(), LIMIT, 0, ptr::null(), &mut err) },
        "{}",
        take_err_string(err)
    );
    std::fs::read(path).unwrap()
}

pub fn open(path: &std::path::Path) -> Result<*mut PrecisionDocument, String> {
    let c = cpath(path);
    let mut err = ptr::null_mut();
    let doc = unsafe { dr_precision_doc_open(c.as_ptr(), LIMIT, 0, ptr::null(), &mut err) };
    if doc.is_null() {
        Err(take_err_string(err))
    } else {
        Ok(doc)
    }
}

/// The CMYK word: the first word after `DRDC` and 46.
pub fn cmyk_word(bytes: &[u8]) -> u32 {
    assert_eq!(u32::from_le_bytes(bytes[4..8].try_into().unwrap()), 46);
    u32::from_le_bytes(bytes[8..12].try_into().unwrap())
}

/// The straight composite at `(x, y)` through `dr_precision_doc_cmyk_sample`
/// (reach 0): ink16 then alpha16.
pub fn composite(doc: *const PrecisionDocument, x: i32, y: i32) -> [u16; 5] {
    let mut err = ptr::null_mut();
    let value = json(
        unsafe { dr_precision_doc_cmyk_sample(doc, x, y, 0, LIMIT, 0, &mut err) },
        err,
    );
    let ink: Vec<u16> = value["ink16"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_u64().unwrap() as u16)
        .collect();
    [
        ink[0],
        ink[1],
        ink[2],
        ink[3],
        value["alpha16"].as_u64().unwrap() as u16,
    ]
}

pub fn unit(s: [u16; 5]) -> Ink {
    s.map(|v| f64::from(v) / 65535.0)
}

pub fn code(v: f64) -> u16 {
    (v.clamp(0.0, 1.0) * 65535.0).round() as u16
}

/// `got` (straight codes) against the oracle's straight `want` FLATTENED
/// onto paper — each plate times alpha — within `bound` per plate, and
/// alpha within `bound`: a transparent sample's ink means nothing.
pub fn assert_flat(got: [u16; 5], want: Ink, bound: u16, what: &str) {
    let g = unit(got);
    for c in 0..4 {
        let (a, b) = (code(g[c] * g[4]), code(want[c] * want[4]));
        assert!(
            a.abs_diff(b) <= bound,
            "{what}: plate {c}: {got:?} vs {want:?} ({a} vs {b} on paper)"
        );
    }
    assert!(
        got[4].abs_diff(code(want[4])) <= bound,
        "{what}: alpha: {got:?} vs {want:?}"
    );
}

// ----------------------------------------------------------- the oracle --
// W3C Compositing and Blending Level 1, section 10 (separable) and 11
// (non-separable), on `L` in [0, 1]; Photoshop's extra separable modes by
// their published formulas.

fn hard_light(cb: f64, cs: f64) -> f64 {
    if cs <= 0.5 {
        cb * 2.0 * cs
    } else {
        let s = 2.0 * cs - 1.0;
        cb + s - cb * s
    }
}

fn soft_light(cb: f64, cs: f64) -> f64 {
    if cs <= 0.5 {
        cb - (1.0 - 2.0 * cs) * cb * (1.0 - cb)
    } else {
        let d = if cb <= 0.25 {
            ((16.0 * cb - 12.0) * cb + 4.0) * cb
        } else {
            cb.sqrt()
        };
        cb + (2.0 * cs - 1.0) * (d - cb)
    }
}

fn dodge(cb: f64, cs: f64) -> f64 {
    if cb == 0.0 {
        0.0
    } else if cs == 1.0 {
        1.0
    } else {
        (cb / (1.0 - cs)).min(1.0)
    }
}

fn burn(cb: f64, cs: f64) -> f64 {
    if cb == 1.0 {
        1.0
    } else if cs == 0.0 {
        0.0
    } else {
        1.0 - ((1.0 - cb) / cs).min(1.0)
    }
}

/// `B(cb, cs)` of a separable mode; `None` for the rest.
fn separable(mode: BlendMode, cb: f64, cs: f64) -> Option<f64> {
    Some(match mode {
        BlendMode::Normal => cs,
        BlendMode::Multiply => cb * cs,
        BlendMode::Screen => cb + cs - cb * cs,
        BlendMode::Overlay => hard_light(cs, cb),
        BlendMode::SoftLight => soft_light(cb, cs),
        BlendMode::HardLight => hard_light(cb, cs),
        BlendMode::Darken => cb.min(cs),
        BlendMode::Lighten => cb.max(cs),
        BlendMode::Difference => (cb - cs).abs(),
        BlendMode::Exclusion => cb + cs - 2.0 * cb * cs,
        BlendMode::ColorDodge => dodge(cb, cs),
        BlendMode::ColorBurn => burn(cb, cs),
        BlendMode::Addition => (cb + cs).min(1.0),
        BlendMode::Subtract => (cb - cs).max(0.0),
        BlendMode::LinearBurn => (cb + cs - 1.0).max(0.0),
        // Edge conventions measured against Photoshop (`blend_edge_take.py`
        // take1): Vivid Light's halves take a black source to 0 and a white one
        // to 1 first, and Divide keeps 0 at 0 / 0.
        BlendMode::VividLight => {
            if cs == 0.0 {
                0.0
            } else if cs == 1.0 {
                1.0
            } else if cs <= 0.5 {
                burn(cb, 2.0 * cs)
            } else {
                dodge(cb, 2.0 * cs - 1.0)
            }
        }
        BlendMode::LinearLight => (cb + 2.0 * cs - 1.0).clamp(0.0, 1.0),
        BlendMode::PinLight => {
            if cs <= 0.5 {
                cb.min(2.0 * cs)
            } else {
                cb.max(2.0 * cs - 1.0)
            }
        }
        // The exact tie by order, as RGB's: 1 iff `cb − cs > 1/32768`
        // (`gray_luminance_take.py` take1, HM-ORD on CMYK's L).
        BlendMode::HardMix if (cb + cs - 1.0).abs() <= 1.0 / 4194304.0 => {
            if cb - cs > 1.0 / 32768.0 {
                1.0
            } else {
                0.0
            }
        }
        BlendMode::HardMix => {
            if cb + cs >= 1.0 {
                1.0
            } else {
                0.0
            }
        }
        BlendMode::Divide => {
            if cs == 0.0 && cb == 0.0 {
                0.0
            } else if cs == 0.0 {
                1.0
            } else {
                (cb / cs).min(1.0)
            }
        }
        _ => return None,
    })
}

fn lum(c: [f64; 3]) -> f64 {
    0.3 * c[0] + 0.59 * c[1] + 0.11 * c[2]
}

fn clip_color(c: [f64; 3]) -> [f64; 3] {
    let l = lum(c);
    let n = c[0].min(c[1]).min(c[2]);
    let x = c[0].max(c[1]).max(c[2]);
    let mut c = c;
    if n < 0.0 {
        c = c.map(|v| l + (v - l) * l / (l - n));
    }
    if x > 1.0 && x > l {
        c = c.map(|v| l + (v - l) * (1.0 - l) / (x - l));
    }
    c
}

fn set_lum(c: [f64; 3], l: f64) -> [f64; 3] {
    let d = l - lum(c);
    clip_color(c.map(|v| v + d))
}

fn sat(c: [f64; 3]) -> f64 {
    c[0].max(c[1]).max(c[2]) - c[0].min(c[1]).min(c[2])
}

fn set_sat(c: [f64; 3], s: f64) -> [f64; 3] {
    let mut order = [0usize, 1, 2];
    order.sort_by(|&a, &b| c[a].partial_cmp(&c[b]).unwrap());
    let [lo, mid, hi] = order;
    let mut out = [0.0; 3];
    if c[hi] > c[lo] {
        out[mid] = (c[mid] - c[lo]) * s / (c[hi] - c[lo]);
        out[hi] = s;
    }
    out
}

/// `B(Cb, Cs)` on INK (module doc): the blended ink.
pub fn blend_ink(mode: BlendMode, cb: [f64; 4], cs: [f64; 4]) -> [f64; 4] {
    let (lb, ls) = (cb.map(|v| 1.0 - v), cs.map(|v| 1.0 - v));
    if separable(mode, 0.0, 0.0).is_some() {
        return std::array::from_fn(|c| 1.0 - separable(mode, lb[c], ls[c]).unwrap());
    }
    let (b3, s3) = ([lb[0], lb[1], lb[2]], [ls[0], ls[1], ls[2]]);
    let triple = |out: [f64; 3], k: f64| [1.0 - out[0], 1.0 - out[1], 1.0 - out[2], k];
    match mode {
        BlendMode::Hue => triple(set_lum(set_sat(s3, sat(b3)), lum(b3)), cb[3]),
        BlendMode::Saturation => triple(set_lum(set_sat(b3, sat(s3)), lum(b3)), cb[3]),
        BlendMode::Color => triple(set_lum(s3, lum(b3)), cb[3]),
        BlendMode::Luminosity => triple(set_lum(b3, lum(s3)), cs[3]),
        BlendMode::DarkerColor | BlendMode::LighterColor => {
            let weight = |l: [f64; 4]| lum([l[0], l[1], l[2]]) * l[3];
            // A tie (within 1e-6, so rounding never breaks an exact one —
            // a CMY grey against the K of the same value) keeps the backdrop.
            let (wb, ws) = (weight(lb), weight(ls));
            let source = if mode == BlendMode::DarkerColor {
                ws < wb - 1e-6
            } else {
                ws > wb + 1e-6
            };
            if source {
                cs
            } else {
                cb
            }
        }
        _ => panic!("no blend function for {mode:?}"),
    }
}

/// The W3C source-over step of `src` at effective alpha `sa` through
/// `mode`, on `L` (the module doc's rule); Dissolve is not an oracle.
pub fn over(dst: &mut Ink, src: [f64; 4], sa: f64, mode: BlendMode) {
    if sa <= 0.0 {
        return;
    }
    let ab = dst[4];
    let ao = sa + ab * (1.0 - sa);
    let blended = blend_ink(mode, [dst[0], dst[1], dst[2], dst[3]], src);
    for c in 0..4 {
        let (lb, ls, b) = (1.0 - dst[c], 1.0 - src[c], 1.0 - blended[c]);
        let lo = (sa * (1.0 - ab) * ls + sa * ab * b + (1.0 - sa) * ab * lb) / ao;
        dst[c] = 1.0 - lo;
    }
    dst[4] = ao;
}

/// A Pass Through group's gate: premultiplied interpolation toward
/// `evaluated` by `t`.
pub fn gate(dst: &mut Ink, evaluated: Ink, t: f64) {
    if t <= 0.0 {
        return;
    }
    let a = dst[4] + (evaluated[4] - dst[4]) * t;
    if a <= 0.0 {
        *dst = [0.0; 5];
        return;
    }
    for c in 0..4 {
        let (p0, p1) = (dst[c] * dst[4], evaluated[c] * evaluated[4]);
        dst[c] = (p0 + (p1 - p0) * t) / a;
    }
    dst[4] = a;
}

/// An adjustment record's step: the adjusted ink through `mode` as the
/// source, mixed back by `k`, alpha kept.
pub fn tone(dst: &mut Ink, adjusted: [f64; 4], k: f64, mode: BlendMode) {
    if dst[4] <= 0.0 || k <= 0.0 {
        return;
    }
    let cb = [dst[0], dst[1], dst[2], dst[3]];
    let effective = blend_ink(mode, cb, adjusted);
    for c in 0..4 {
        dst[c] += (effective[c] - dst[c]) * k;
    }
}

/// The byte for a unit Opacity or Fill (#578): `floor(255 v + 0.5 +
/// 1e-3) / 255` on the stored f32 widened to f64; a CMYK composite reads it
/// at every depth.
pub fn byte_unit(v: f32) -> f64 {
    crate::common::byte_unit(f64::from(v))
}

fn fill_of(layer: &PrecisionDocumentLayer) -> f64 {
    layer.style().map_or(1.0, |s| byte_unit(s.fill_opacity))
}

fn mask_at(layer: &PrecisionDocumentLayer, x: i64, y: i64) -> f64 {
    let Some(mask) = layer.mask() else {
        return 1.0;
    };
    if !mask.enabled() {
        return 1.0;
    }
    let (ox, oy) = mask.properties().offset;
    let (w, h) = mask.plane().dimensions();
    let (px, py) = (x - i64::from(ox), y - i64::from(oy));
    if px < 0 || py < 0 || px >= i64::from(w) || py >= i64::from(h) {
        return f64::from(mask.properties().outside_coverage) / 255.0;
    }
    f64::from(mask.plane().samples()[(py * i64::from(w) + px) as usize]) / 65535.0
}

/// A CMYK raster record's stored straight sample at canvas `(x, y)`.
fn pixel_at(layer: &PrecisionDocumentLayer, x: i64, y: i64) -> Option<Ink> {
    let image = layer.cmyk_source()?;
    let (ox, oy) = layer.offset();
    let (px, py) = (x - i64::from(ox), y - i64::from(oy));
    let (w, h) = image.dimensions();
    if px < 0 || py < 0 || px >= i64::from(w) || py >= i64::from(h) {
        return None;
    }
    Some(unit(image.sample_at(px as u32, py as u32)?))
}

fn ink4(ink: Ink) -> [f64; 4] {
    [ink[0], ink[1], ink[2], ink[3]]
}

/// One record composited onto `acc` at `(x, y)`; `isolate` composites a
/// Pass Through group as an isolated one (a clip unit's member).
fn record(layer: &PrecisionDocumentLayer, isolate: bool, x: i64, y: i64, acc: &mut Ink) {
    let m = mask_at(layer, x, y);
    let opacity = byte_unit(layer.opacity());
    if let Some(children) = layer.children() {
        if layer.is_pass_through() && !isolate {
            let mut evaluated = *acc;
            level(children, x, y, &mut evaluated);
            gate(acc, evaluated, opacity * m);
            return;
        }
        let mut own = [0.0; 5];
        level(children, x, y, &mut own);
        let mode = if layer.is_pass_through() {
            BlendMode::Normal
        } else {
            layer.blend_mode()
        };
        over(
            acc,
            ink4(own),
            own[4] * crate::common::joint_unit(fill_of(layer), opacity) * m,
            mode,
        );
        return;
    }
    match pixel_at(layer, x, y) {
        Some(src) => over(
            acc,
            ink4(src),
            src[4] * crate::common::joint_unit(fill_of(layer), opacity) * m,
            layer.blend_mode(),
        ),
        None if layer.cmyk_source().is_none() => {
            // The tests' one adjustment record kind: `LEVELS`.
            let adjusted = levels(ink4(*acc));
            tone(acc, adjusted, opacity * m, layer.blend_mode());
        }
        None => {}
    }
}

/// One level of records composited onto `acc` at `(x, y)`, bottom first,
/// with its clip units (module doc).
pub fn level(layers: &[PrecisionDocumentLayer], x: i64, y: i64, acc: &mut Ink) {
    let mut i = 0;
    while i < layers.len() {
        let base = &layers[i];
        if base.clipped() {
            if base.visible() {
                record(base, false, x, y, acc);
            }
            i += 1;
            continue;
        }
        let mut end = i + 1;
        while end < layers.len() && layers[end].clipped() {
            end += 1;
        }
        let members: Vec<&PrecisionDocumentLayer> =
            layers[i + 1..end].iter().filter(|m| m.visible()).collect();
        if base.visible() {
            if members.is_empty() {
                record(base, false, x, y, acc);
            } else {
                let m = mask_at(base, x, y);
                // The base's own pixels and footprint (no Fill in it).
                let (ink, footprint) = if let Some(children) = base.children() {
                    let mut own = [0.0; 5];
                    level(children, x, y, &mut own);
                    (ink4(own), own[4] * m)
                } else {
                    match pixel_at(base, x, y) {
                        Some(src) => (ink4(src), src[4] * m),
                        None => ([0.0; 4], 0.0),
                    }
                };
                // Photoshop's clip rule (#578): the members land on the base
                // with its alpha frozen RELATIVE to the footprint — the base
                // at its Fill share of it, each member over that, the result
                // scaled back onto the footprint.
                let mut buf = [0.0; 5];
                over(&mut buf, ink, footprint * fill_of(base), BlendMode::Normal);
                buf[4] = if footprint > 0.0 {
                    (buf[4] / footprint).min(1.0)
                } else {
                    0.0
                };
                for member in members {
                    record(member, true, x, y, &mut buf);
                    if footprint <= 0.0 {
                        buf[4] = 0.0;
                    }
                }
                buf[4] *= footprint;
                let mode = if base.is_pass_through() {
                    BlendMode::Normal
                } else {
                    base.blend_mode()
                };
                over(acc, ink4(buf), buf[4] * byte_unit(base.opacity()), mode);
            }
        }
        i = end;
    }
}

/// The document's composite at canvas `(x, y)` by the oracle.
pub fn oracle(doc: *const PrecisionDocument, x: i64, y: i64) -> Ink {
    let mut acc = [0.0; 5];
    level(unsafe { &*doc }.root_layers(), x, y, &mut acc);
    acc
}

/// Every canvas pixel's composite against the oracle within `bound`.
pub fn assert_composite(doc: *const PrecisionDocument, bound: u16, what: &str) {
    let (w, h) = unsafe { &*doc }.dimensions();
    for y in 0..h as i32 {
        for x in 0..w as i32 {
            assert_flat(
                composite(doc, x, y),
                oracle(doc, i64::from(x), i64::from(y)),
                bound,
                &format!("{what} at ({x}, {y})"),
            );
        }
    }
}

/// The shared profile handle, for a caller that builds its own RGB layers.
pub fn srgb() -> Arc<IccProfile> {
    IccProfile::srgb()
}
