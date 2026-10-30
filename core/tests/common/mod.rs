//! Helpers shared by the FFI test files: C-string plumbing, synthesized
//! fixtures, layer/document accessors, the mirrored `Dr*` constants, the W3C
//! reference blend math, the mask/meta fixtures several files drive, and the
//! layer-style bridges and blur oracle the `style*` files share.
//! Each suite root declares this once with `mod common;` and its mounted files
//! reach it with `use crate::common;`; a standalone binary declares `mod common;`.
//! Helpers only some files use are expected — hence the file-wide
//! `allow(dead_code)`.
#![allow(dead_code)]

use std::ffi::{c_char, c_int, CStr, CString};
use std::path::Path;
use std::ptr;

use doctored_core::doc::{DrDocument, MaskKind};
use doctored_core::ffi::*;
use doctored_core::ffi_doc::*;
use doctored_core::ffi_geometry::dr_doc_geometry_edit;
use doctored_core::ffi_style::*;
use doctored_core::precision_image::REFERENCE_WORKING_BYTES;
use doctored_core::style::LayerStyle;
use doctored_core::DrImage;
use image::{Rgba, RgbaImage};
use serde_json::json;
use tempfile::TempDir;

pub const BLEND_NORMAL: c_int = 0;
pub const BLEND_MULTIPLY: c_int = 1;
pub const BLEND_SCREEN: c_int = 2;
pub const BLEND_OVERLAY: c_int = 3;
pub const BLEND_SOFT_LIGHT: c_int = 4;
pub const BLEND_HARD_LIGHT: c_int = 5;
pub const BLEND_DARKEN: c_int = 6;
pub const BLEND_LIGHTEN: c_int = 7;
pub const BLEND_DIFFERENCE: c_int = 8;
pub const BLEND_EXCLUSION: c_int = 9;
pub const BLEND_COLOR_DODGE: c_int = 10;
pub const BLEND_COLOR_BURN: c_int = 11;
pub const BLEND_ADDITION: c_int = 12;
pub const BLEND_SUBTRACT: c_int = 13;
pub const BLEND_DISSOLVE: c_int = 14;
pub const BLEND_LINEAR_BURN: c_int = 15;
pub const BLEND_DARKER_COLOR: c_int = 16;
pub const BLEND_LIGHTER_COLOR: c_int = 17;
pub const BLEND_VIVID_LIGHT: c_int = 18;
pub const BLEND_LINEAR_LIGHT: c_int = 19;
pub const BLEND_PIN_LIGHT: c_int = 20;
pub const BLEND_HARD_MIX: c_int = 21;
pub const BLEND_DIVIDE: c_int = 22;
pub const BLEND_HUE: c_int = 23;
pub const BLEND_SATURATION: c_int = 24;
pub const BLEND_COLOR: c_int = 25;
pub const BLEND_LUMINOSITY: c_int = 26;
/// The number of modes a RASTER layer may carry: 0..=26. Pass Through (27)
/// is a GROUP's declaration and is refused on a layer, so it is deliberately
/// outside this count and every loop built on it.
pub const BLEND_MODE_COUNT: c_int = 27;
pub const BLEND_PASS_THROUGH: c_int = 27;

pub const COMPOSITE_OVER: c_int = 0;
pub const COMPOSITE_ERASE: c_int = 1;

pub const MASK_REVEAL_ALL: c_int = 0;
pub const MASK_HIDE_ALL: c_int = 1;
pub const MASK_FROM_SELECTION: c_int = 2;

pub const FILTER_NEAREST: c_int = 0;
pub const FILTER_BILINEAR: c_int = 1;
pub const FILTER_CATMULL_ROM: c_int = 2;
pub const FILTER_LANCZOS3: c_int = 3;

/// `DrEditKind`, `DrLockFlags`, `DrArrange` and `DrAlign`, mirrored from the
/// header exactly like the blend and filter tables above.
pub const EDIT_PIXELS: c_int = 0;
pub const EDIT_POSITION: c_int = 1;
pub const EDIT_MASK: c_int = 2;
pub const EDIT_MASK_APPLY: c_int = 3;
pub const EDIT_MERGE: c_int = 4;

pub const LOCK_TRANSPARENCY: u32 = 1;
pub const LOCK_PIXELS: u32 = 2;
pub const LOCK_POSITION: u32 = 4;
pub const LOCK_ALL: u32 = 7;

pub const ARRANGE_FRONT: c_int = 0;
pub const ARRANGE_FORWARD: c_int = 1;
pub const ARRANGE_BACKWARD: c_int = 2;
pub const ARRANGE_BACK: c_int = 3;

pub const ALIGN_LEFT: c_int = 0;
pub const ALIGN_CENTER_X: c_int = 1;
pub const ALIGN_RIGHT: c_int = 2;
pub const ALIGN_TOP: c_int = 3;
pub const ALIGN_CENTER_Y: c_int = 4;
pub const ALIGN_BOTTOM: c_int = 5;

// ---------------------------------------------------------------- helpers --

pub fn cpath(p: &Path) -> CString {
    CString::new(p.to_str().expect("utf-8 path")).expect("no interior NUL")
}

pub fn take_err_string(err: *mut c_char) -> String {
    if err.is_null() {
        return "<no error message>".into();
    }
    let s = unsafe { CStr::from_ptr(err) }
        .to_string_lossy()
        .into_owned();
    unsafe { dr_string_free(err) };
    s
}

/// The live SET transform: `dr_doc_geometry_edit` with
/// `{"op":"affine_set","layers":…,"matrix":…,"filter":"nearest"}`,
/// limit `REFERENCE_WORKING_BYTES`, other_retained 0, NULL job control. Returns the
/// result (NULL on refusal or no-op) and the core's error message, if any;
/// NULL with `None` is the core's "unchanged".
pub fn affine_set(
    doc: *const DrDocument,
    layers: &[usize],
    matrix: [f64; 6],
) -> (*mut DrDocument, Option<String>) {
    let request = CString::new(
        json!({"op":"affine_set","layers":layers,"matrix":matrix,"filter":"nearest"}).to_string(),
    )
    .unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    let out = unsafe {
        dr_doc_geometry_edit(
            doc,
            request.as_ptr(),
            REFERENCE_WORKING_BYTES,
            0,
            ptr::null(),
            &mut err,
        )
    };
    let message = (!err.is_null()).then(|| take_err_string(err));
    (out, message)
}

/// A byte whole-document geometry request (`straighten`, `rotate_canvas`, …)
/// through `dr_doc_geometry_edit`, in `affine_set`'s call shape: limit
/// `REFERENCE_WORKING_BYTES`, other_retained 0, NULL job control. Returns the
/// result (NULL on refusal or no-op) and the core's error message, if any;
/// NULL with `None` is the core's "unchanged".
pub fn byte_geometry_edit(
    doc: *const DrDocument,
    request: &serde_json::Value,
) -> (*mut DrDocument, Option<String>) {
    let request = CString::new(request.to_string()).unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    let out = unsafe {
        dr_doc_geometry_edit(
            doc,
            request.as_ptr(),
            REFERENCE_WORKING_BYTES,
            0,
            ptr::null(),
            &mut err,
        )
    };
    let message = (!err.is_null()).then(|| take_err_string(err));
    (out, message)
}

/// Materializes a `DrImage` from synthesized pixels by round-tripping a PNG
/// through the FFI (the same trick the existing integration tests use).
pub fn open_image(dir: &TempDir, name: &str, img: &RgbaImage) -> *mut DrImage {
    let path = dir.path().join(name);
    img.save(&path).expect("save pattern png");
    let c = cpath(&path);
    let mut err: *mut c_char = ptr::null_mut();
    let p = unsafe { dr_image_open(c.as_ptr(), &mut err) };
    assert!(!p.is_null(), "open failed: {}", take_err_string(err));
    p
}

pub fn img_dims(img: *const DrImage) -> (u32, u32) {
    unsafe { (dr_image_width(img), dr_image_height(img)) }
}

pub fn img_pixels(img: *const DrImage) -> Vec<u8> {
    let (w, h) = img_dims(img);
    let p = unsafe { dr_image_pixels_rgba(img) };
    assert!(!p.is_null(), "pixels pointer NULL for valid image");
    unsafe { std::slice::from_raw_parts(p, (w * h * 4) as usize) }.to_vec()
}

/// A single-layer document built from synthesized pixels.
pub fn doc_from(dir: &TempDir, name: &str, img: &RgbaImage) -> *mut DrDocument {
    let image = open_image(dir, name, img);
    let doc = unsafe { dr_doc_from_image(image) };
    unsafe { dr_image_free(image) };
    assert!(!doc.is_null());
    doc
}

/// Inserts `img` as a new layer above `idx` (asserting success), returning
/// the NEW document and freeing the old one.
pub fn add_layer(
    dir: &TempDir,
    file: &str,
    doc: *mut DrDocument,
    idx: usize,
    img: &RgbaImage,
    name: &str,
) -> *mut DrDocument {
    let image = open_image(dir, file, img);
    let cname = CString::new(name).unwrap();
    let out = unsafe { dr_doc_adding_image_layer(doc, idx, image, cname.as_ptr()) };
    unsafe { dr_image_free(image) };
    assert!(!out.is_null(), "adding_image_layer({idx}, {name}) failed");
    unsafe { dr_doc_free(doc) };
    out
}

/// Applies a doc -> doc FFI operation, asserting success and freeing the old
/// document.
pub fn apply(
    doc: *mut DrDocument,
    op: impl FnOnce(*const DrDocument) -> *mut DrDocument,
) -> *mut DrDocument {
    let out = op(doc);
    assert!(!out.is_null(), "document operation failed");
    unsafe { dr_doc_free(doc) };
    out
}

/// [`apply`] for a PROPERTY setter a fixture may hand the value the entry
/// already holds — an offset of (0, 0) on a fresh layer, say. The core
/// answers NULL for an op that would change nothing (its purity rule, so a
/// host never registers a phantom undo step), and that is not a failure here:
/// the document already says what the caller asked for, so it is kept.
pub fn apply_or_keep(
    doc: *mut DrDocument,
    op: impl FnOnce(*const DrDocument) -> *mut DrDocument,
) -> *mut DrDocument {
    let out = op(doc);
    if out.is_null() {
        return doc;
    }
    unsafe { dr_doc_free(doc) };
    out
}

pub fn layer_pixels(doc: *const DrDocument, idx: usize) -> Vec<u8> {
    let img = unsafe { dr_doc_layer_image(doc, idx) };
    assert!(!img.is_null(), "layer_image({idx}) NULL");
    let v = img_pixels(img);
    unsafe { dr_image_free(img) };
    v
}

pub fn layer_dims(doc: *const DrDocument, idx: usize) -> (u32, u32) {
    unsafe { (dr_doc_layer_width(doc, idx), dr_doc_layer_height(doc, idx)) }
}

pub fn layer_offset(doc: *const DrDocument, idx: usize) -> (i32, i32) {
    unsafe {
        (
            dr_doc_layer_offset_x(doc, idx),
            dr_doc_layer_offset_y(doc, idx),
        )
    }
}

pub fn layer_name(doc: *const DrDocument, idx: usize) -> String {
    let p = unsafe { dr_doc_layer_name(doc, idx) };
    assert!(!p.is_null(), "layer_name({idx}) NULL");
    let s = unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned();
    unsafe { dr_string_free(p) };
    s
}

pub fn flat_pixels(doc: *const DrDocument) -> Vec<u8> {
    let img = unsafe { dr_doc_flattened(doc, 0, std::ptr::null(), std::ptr::null_mut()) };
    assert!(!img.is_null(), "flattened NULL");
    let v = img_pixels(img);
    unsafe { dr_image_free(img) };
    v
}

pub fn pixel(buf: &[u8], w: u32, x: u32, y: u32) -> [u8; 4] {
    let i = ((y * w + x) * 4) as usize;
    [buf[i], buf[i + 1], buf[i + 2], buf[i + 3]]
}

pub fn opaque_pattern(w: u32, h: u32) -> RgbaImage {
    RgbaImage::from_fn(w, h, |x, y| {
        Rgba([
            (x * 255 / w.max(1)) as u8,
            (y * 255 / h.max(1)) as u8,
            ((x * 7 + y * 13) % 256) as u8,
            255,
        ])
    })
}

pub fn solid(w: u32, h: u32, px: [u8; 4]) -> RgbaImage {
    RgbaImage::from_pixel(w, h, Rgba(px))
}

/// 64x48-style gradient with an alpha ramp (fully opaque top row, fully
/// transparent bottom row).
pub fn test_pattern(w: u32, h: u32) -> RgbaImage {
    RgbaImage::from_fn(w, h, |x, y| {
        let r = (x * 255 / (w - 1).max(1)) as u8;
        let g = (y * 255 / (h - 1).max(1)) as u8;
        let b = ((x + y) % 256) as u8;
        let a = 255 - (y * 255 / (h - 1).max(1)) as u8;
        Rgba([r, g, b, a])
    })
}

pub fn open_ok(path: &Path) -> *mut DrImage {
    let c = cpath(path);
    let mut err: *mut c_char = ptr::null_mut();
    let img = unsafe { dr_image_open(c.as_ptr(), &mut err) };
    assert!(
        !img.is_null(),
        "open of {path:?} failed: {}",
        take_err_string(err)
    );
    assert!(err.is_null(), "err_out set on success");
    img
}

/// Writes `img` as a PNG (via the image crate) and opens it back through the
/// FFI; the only way to materialize a DrImage from synthesized pixels.
pub fn open_pattern(dir: &TempDir, name: &str, img: &RgbaImage) -> *mut DrImage {
    let path = dir.path().join(name);
    img.save(&path).expect("save pattern png");
    open_ok(&path)
}

pub fn dims(img: *const DrImage) -> (u32, u32) {
    unsafe { (dr_image_width(img), dr_image_height(img)) }
}

pub fn pixels(img: *const DrImage) -> Vec<u8> {
    let (w, h) = dims(img);
    let p = unsafe { dr_image_pixels_rgba(img) };
    assert!(!p.is_null(), "pixels pointer NULL for valid image");
    unsafe { std::slice::from_raw_parts(p, (w * h * 4) as usize) }.to_vec()
}

pub fn pixel_at(img: *const DrImage, x: u32, y: u32) -> [u8; 4] {
    let (w, h) = dims(img);
    assert!(x < w && y < h);
    let v = pixels(img);
    let i = ((y * w + x) * 4) as usize;
    [v[i], v[i + 1], v[i + 2], v[i + 3]]
}

pub fn free(img: *mut DrImage) {
    unsafe { dr_image_free(img) }
}

// ------------------------------------------------- the byte opacity (#578) --

/// The byte for a unit Opacity or Fill (#578, `opacity_byte`): below
/// 32 bits every composite reads `floor(255 v + 0.5 + 1e-3) / 255`, written
/// here from the integer rule, never from the core.
pub fn byte_unit(v: f64) -> f64 {
    (v.clamp(0.0, 1.0) * 255.0 + 0.5 + 1e-3).floor() / 255.0
}

/// A Fill and an Opacity scaling one contribution in one product (#578,
/// `opacity_byte::joint`): the two bytes' product rounded to a byte, never a
/// tie (255 is odd).
pub fn joint_unit(fill: f64, opacity: f64) -> f64 {
    (byte_unit(fill) * 255.0 * byte_unit(opacity)).round() / 255.0
}

// ---------------------------------------------------- the Classic ramp --

/// One colour component of the Classic gradient (Smoothness 100 %)
/// at `t` over `stops` = `[(position, value)]`, sorted, written from the
/// fit's description (`gradient_identify_673_fit.py`), never from the core:
/// between stops `p1` and `p2` at the segment fraction `f` it is the
/// uniform Catmull-Rom through `p0, p1, p2, p3` (the neighbours, an end stop
/// standing in for its missing one), in its matrix form
/// `½ (2 p1 + (p2 − p0) f + (2 p0 − 5 p1 + 4 p2 − p3) f² + (3 p1 − p0 − 3
/// p2 + p3) f³)`. On two stops `a → b` that is `a + (b − a)(f + 3f² − 2f³) / 2`.
/// The end stop's value at or past an end; a zero-width segment reads its
/// right stop. Opacity is never this: it stays linear.
pub fn classic_ramp(stops: &[(f64, f64)], t: f64) -> f64 {
    let n = stops.len();
    assert!(n > 0, "a ramp needs a stop");
    if t <= stops[0].0 {
        return stops[0].1;
    }
    if t >= stops[n - 1].0 {
        return stops[n - 1].1;
    }
    let k = (0..n - 1)
        .find(|&k| t >= stops[k].0 && t < stops[k + 1].0)
        .expect("t lies inside the stops");
    let width = stops[k + 1].0 - stops[k].0;
    let f = if width > 0.0 {
        (t - stops[k].0) / width
    } else {
        1.0
    };
    let p1 = stops[k].1;
    let p2 = stops[k + 1].1;
    let p0 = if k > 0 { stops[k - 1].1 } else { p1 };
    let p3 = if k + 2 < n { stops[k + 2].1 } else { p2 };
    0.5 * (2.0 * p1
        + (p2 - p0) * f
        + (2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3) * f * f
        + (3.0 * p1 - p0 - 3.0 * p2 + p3) * f * f * f)
}

/// The Smoothness `k` (a fraction, 1 = 100 %) over the same stops,
/// written from the take-2 description (law K1), never from the core:
/// `(1 − k) · linear + k · classic_ramp`, `linear` the straight interpolation
/// between the bracketing stops (the end stop's value at or past an end; a
/// zero-width segment reads its right stop).
pub fn smooth_ramp(stops: &[(f64, f64)], t: f64, k: f64) -> f64 {
    let n = stops.len();
    assert!(n > 0, "a ramp needs a stop");
    let linear = if t <= stops[0].0 {
        stops[0].1
    } else if t >= stops[n - 1].0 {
        stops[n - 1].1
    } else {
        let k = (0..n - 1)
            .find(|&k| t >= stops[k].0 && t < stops[k + 1].0)
            .expect("t lies inside the stops");
        let width = stops[k + 1].0 - stops[k].0;
        let f = if width > 0.0 {
            (t - stops[k].0) / width
        } else {
            1.0
        };
        stops[k].1 + (stops[k + 1].1 - stops[k].1) * f
    };
    (1.0 - k) * linear + k * classic_ramp(stops, t)
}

/// [`smooth_ramp`] with the Classic term held to the two bracketing stops
/// (the FILL CLAMP, `gradient_map_678_take.py`'s BRACKET, written from its
/// declared law, never from the core): `(1 − k) · linear + k · clamp(
/// classic_ramp, min(a, b), max(a, b))`, `a` and `b` the bracketing stops'
/// values. At or past an end stop, both terms are the end stop's value.
pub fn bracket_ramp(stops: &[(f64, f64)], t: f64, k: f64) -> f64 {
    let n = stops.len();
    assert!(n > 0, "a ramp needs a stop");
    if t <= stops[0].0 {
        return stops[0].1;
    }
    if t >= stops[n - 1].0 {
        return stops[n - 1].1;
    }
    let s = (0..n - 1)
        .find(|&s| t >= stops[s].0 && t < stops[s + 1].0)
        .expect("t lies inside the stops");
    let (a, b) = (stops[s].1, stops[s + 1].1);
    let classic = classic_ramp(stops, t).clamp(a.min(b), a.max(b));
    let linear = smooth_ramp(stops, t, 0.0);
    (1.0 - k) * linear + k * classic
}

/// Photoshop's stop midpoints as a move of `t`, written from the
/// take-2 description (`gradient_identify_673_fit.py`'s `pwl.pre`), never
/// from the core: inside the bracket `p_k <= t < p_{k+1}` at the segment
/// fraction `f`, `t` moves to `p_k + g(f, m_k) (p_{k+1} − p_k)`, `m_k` stop
/// `k`'s midpoint as a fraction and `g(f, m) = f / (2m)` up to `m`, `1/2 +
/// (f − m) / (2 (1 − m))` past it. At or past an end stop, or on a zero-width
/// segment, `t` is unchanged. The colour is then `smooth_ramp(stops,
/// midpoint_t(..), k)` and an opacity `smooth_ramp(.., 0.0)` (the linear
/// ramp) over the opacity midpoints.
pub fn midpoint_t(stops: &[(f64, f64)], midpoints: &[f64], t: f64) -> f64 {
    let n = stops.len();
    assert!(n > 0 && midpoints.len() == n, "a midpoint per stop");
    if t <= stops[0].0 || t >= stops[n - 1].0 {
        return t;
    }
    let k = (0..n - 1)
        .find(|&k| t >= stops[k].0 && t < stops[k + 1].0)
        .expect("t lies inside the stops");
    let width = stops[k + 1].0 - stops[k].0;
    if width <= 0.0 {
        return t;
    }
    let f = (t - stops[k].0) / width;
    let m = midpoints[k];
    let g = if f <= m {
        f / (2.0 * m)
    } else {
        0.5 + (f - m) / (2.0 * (1.0 - m))
    };
    stops[k].0 + g * width
}

// --------------------------------------------- reference blend math (W3C) --

pub fn ref_blend(mode: c_int, cb: f32, cs: f32) -> f32 {
    match mode {
        BLEND_NORMAL => cs,
        BLEND_MULTIPLY => cb * cs,
        BLEND_SCREEN => cb + cs - cb * cs,
        BLEND_OVERLAY => ref_blend(BLEND_HARD_LIGHT, cs, cb),
        BLEND_SOFT_LIGHT => {
            let d = if cb <= 0.25 {
                ((16.0 * cb - 12.0) * cb + 4.0) * cb
            } else {
                cb.sqrt()
            };
            if cs <= 0.5 {
                cb - (1.0 - 2.0 * cs) * cb * (1.0 - cb)
            } else {
                cb + (2.0 * cs - 1.0) * (d - cb)
            }
        }
        BLEND_HARD_LIGHT => {
            if cs <= 0.5 {
                2.0 * cb * cs
            } else {
                1.0 - 2.0 * (1.0 - cb) * (1.0 - cs)
            }
        }
        BLEND_DARKEN => cb.min(cs),
        BLEND_LIGHTEN => cb.max(cs),
        BLEND_DIFFERENCE => (cb - cs).abs(),
        BLEND_EXCLUSION => cb + cs - 2.0 * cb * cs,
        BLEND_COLOR_DODGE => {
            if cb == 0.0 {
                0.0
            } else if cs == 1.0 {
                1.0
            } else {
                (cb / (1.0 - cs)).min(1.0)
            }
        }
        BLEND_COLOR_BURN => {
            if cb == 1.0 {
                1.0
            } else if cs == 0.0 {
                0.0
            } else {
                1.0 - ((1.0 - cb) / cs).min(1.0)
            }
        }
        BLEND_ADDITION => (cb + cs).min(1.0),
        BLEND_SUBTRACT => (cb - cs).max(0.0),
        _ => panic!("unknown mode {mode}"),
    }
}

/// Full W3C compositing of one straight-alpha source pixel over a
/// straight-alpha backdrop pixel; all values in [0, 1].
pub fn ref_composite(bg: [f32; 4], src: [f32; 4], opacity: f32, mode: c_int) -> [f32; 4] {
    let sa = src[3] * opacity;
    let ab = bg[3];
    if sa <= 0.0 {
        return bg;
    }
    let ao = sa + ab * (1.0 - sa);
    let mut out = [0.0f32; 4];
    for c in 0..3 {
        out[c] = (sa * (1.0 - ab) * src[c]
            + sa * ab * ref_blend(mode, bg[c], src[c])
            + (1.0 - sa) * ab * bg[c])
            / ao;
    }
    out[3] = ao;
    out
}

pub fn to_unit(px: [u8; 4]) -> [f32; 4] {
    [
        f32::from(px[0]) / 255.0,
        f32::from(px[1]) / 255.0,
        f32::from(px[2]) / 255.0,
        f32::from(px[3]) / 255.0,
    ]
}

// ----------------------------------------------- mask & meta fixtures --

pub const RED: [u8; 4] = [255, 0, 0, 255];
pub const BLUE: [u8; 4] = [0, 0, 255, 255];

/// Opaque red canvas-sized background under an opaque blue layer of size
/// `layer` placed at `offset` (layer index 1).
pub fn mask_fixture(canvas: (u32, u32), layer: (u32, u32), offset: (i32, i32)) -> DrDocument {
    let doc = DrDocument::from_pixels(solid(canvas.0, canvas.1, RED));
    let doc = doc
        .adding_image_layer(0, solid(layer.0, layer.1, BLUE), "Top")
        .expect("add layer");
    // An offset of (0, 0) is the one the layer already has, and a setter
    // handed the stored value answers None (the purity rule). The layer is
    // MADE there: its reference point stays (0, 0).
    let placed = doc
        .with_layer_offset(1, offset.0, offset.1)
        .map(|mut placed| {
            placed.layers[1].reference = (0, 0);
            placed
        });
    placed.unwrap_or(doc)
}

/// Canvas-sized selection buffer from a per-pixel function.
pub fn selection(w: u32, h: u32, f: impl Fn(u32, u32) -> u8) -> Vec<u8> {
    let mut out = Vec::with_capacity((w * h) as usize);
    for y in 0..h {
        for x in 0..w {
            out.push(f(x, y));
        }
    }
    out
}

/// The mask bytes of layer `idx`, row-major.
pub fn mask_bytes(doc: &DrDocument, idx: usize) -> Vec<u8> {
    doc.layers[idx]
        .mask
        .as_ref()
        .expect("layer has a mask")
        .as_raw()
        .clone()
}

pub const GREEN: [u8; 4] = [0, 200, 0, 255];
pub const WHITE: [u8; 4] = [255, 255, 255, 255];
pub const META: &str = "{\"type\":\"text\",\"string\":\"keep me\"}";

/// Asserts the hard invariant on every layer of `doc`.
pub fn assert_mask_invariant(doc: &DrDocument, what: &str) {
    for (i, layer) in doc.layers.iter().enumerate() {
        if let Some(mask) = layer.mask.as_ref() {
            assert_eq!(
                mask.dimensions(),
                layer.pixels.dimensions(),
                "{what}: layer {i}'s mask must be exactly its pixels' size"
            );
        }
    }
}

/// `mask_fixture` with a checkerboard mask and meta on layer 1: revealed
/// where the CANVAS coordinate sum is even. A one-pixel misalignment inverts
/// the pattern, so a stale mask cannot survive the projection comparisons.
pub fn checkerboard_masked(
    canvas: (u32, u32),
    layer: (u32, u32),
    offset: (i32, i32),
) -> DrDocument {
    let doc = mask_fixture(canvas, layer, offset);
    let sel = selection(
        canvas.0,
        canvas.1,
        |x, y| if (x + y) % 2 == 0 { 255 } else { 0 },
    );
    let mut doc = doc
        .add_mask(1, MaskKind::FromSelection(&sel))
        .expect("mask from selection");
    doc.layers[1].meta = Some(META.to_string());
    doc
}

/// The FFI twin of `mask_fixture`: an opaque red canvas-sized background under
/// an opaque blue layer of size `layer` placed at `offset` (layer index 1).
pub fn ffi_mask_fixture(
    dir: &TempDir,
    tag: &str,
    canvas: (u32, u32),
    layer: (u32, u32),
    offset: (i32, i32),
) -> *mut DrDocument {
    let doc = doc_from(
        dir,
        &format!("{tag}-bg.png"),
        &solid(canvas.0, canvas.1, RED),
    );
    let doc = add_layer(
        dir,
        &format!("{tag}-top.png"),
        doc,
        0,
        &solid(layer.0, layer.1, BLUE),
        "Top",
    );
    apply_or_keep(doc, |d| unsafe {
        dr_doc_with_layer_offset(d, 1, offset.0, offset.1)
    })
}

/// Layer `idx`'s coverage read back through `dr_doc_layer_mask_image`, which
/// must be an opaque grayscale image at the LAYER's size.
pub fn ffi_mask_bytes(doc: *const DrDocument, idx: usize) -> Vec<u8> {
    let img = unsafe { dr_doc_layer_mask_image(doc, idx) };
    assert!(!img.is_null(), "layer_mask_image({idx}) NULL");
    assert_eq!(
        img_dims(img),
        layer_dims(doc, idx),
        "the mask image is the layer's size, not the canvas'"
    );
    let px = img_pixels(img);
    unsafe { dr_image_free(img) };
    let mut coverage = Vec::with_capacity(px.len() / 4);
    for c in px.chunks_exact(4) {
        assert_eq!(
            [c[1], c[2], c[3]],
            [c[0], c[0], 255],
            "the mask image must be opaque grayscale"
        );
        coverage.push(c[0]);
    }
    coverage
}

/// The two mask queries as a pair, for terse before/after assertions.
pub fn ffi_mask_flags(doc: *const DrDocument, idx: usize) -> (bool, bool) {
    unsafe {
        (
            dr_doc_layer_has_mask(doc, idx),
            dr_doc_layer_mask_enabled(doc, idx),
        )
    }
}

/// A realistic text-layer payload with non-ASCII content, so "came back
/// unchanged" means byte-for-byte rather than merely non-empty.
pub const TEXT_META: &str = concat!(
    "{\"type\":\"text\",\"string\":\"héllo 层 — ✎\",",
    "\"font\":\"Helvetica Neue\",\"size\":24.5,",
    "\"color\":\"#ff8800\",\"alignment\":\"center\"}"
);

/// Layer `idx`'s metadata through `dr_doc_layer_meta`; None when the call
/// returns NULL (no metadata, or an out-of-range index).
pub fn ffi_meta(doc: *const DrDocument, idx: usize) -> Option<String> {
    let p = unsafe { dr_doc_layer_meta(doc, idx) };
    if p.is_null() {
        return None;
    }
    let s = unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned();
    unsafe { dr_string_free(p) };
    Some(s)
}

/// Sets layer `idx`'s metadata, asserting success and freeing the old handle.
pub fn set_meta(doc: *mut DrDocument, idx: usize, meta: &str) -> *mut DrDocument {
    let c = CString::new(meta).expect("no interior NUL");
    apply(doc, |d| unsafe {
        dr_doc_with_layer_meta(d, idx, c.as_ptr())
    })
}

pub const MAGENTA: [u8; 4] = [255, 0, 255, 255];

/// Meta blob for adjustment op `op` with a `params` JSON object literal.
pub fn adjust_meta(op: &str, params: &str) -> String {
    format!("{{\"type\":\"adjust\",\"op\":\"{op}\",\"params\":{params}}}")
}

// ------------------------------------------------------- layer styles --

/// A drop shadow + stroke style in a non-canonical spelling (unsorted keys,
/// integer numbers, an uppercase colour), so "unchanged" proves the core
/// re-serializes rather than echoes, and "survived" means the effects did.
/// It names no unknown key: the setters refuse one by name.
pub const STYLE_JSON: &str = concat!(
    "{\"fill_opacity\":0.5,",
    "\"effects\":[{\"type\":\"drop_shadow\",\"distance\":3,\"size\":2},",
    "{\"type\":\"stroke\",\"size\":2,\"color\":\"#FF0000\"}]}"
);

/// Layer `idx`'s canonical style JSON through `dr_doc_layer_style`; None
/// when the call returns NULL (no style, or an out-of-range index).
pub fn ffi_style(doc: *const DrDocument, idx: usize) -> Option<String> {
    let p = unsafe { dr_doc_layer_style(doc, idx) };
    if p.is_null() {
        return None;
    }
    let s = unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned();
    unsafe { dr_string_free(p) };
    Some(s)
}

/// Sets layer `idx`'s style, asserting success (no error message) and
/// freeing the old handle.
pub fn set_style(doc: *mut DrDocument, idx: usize, json: &str) -> *mut DrDocument {
    let c = CString::new(json).expect("no interior NUL");
    let mut err: *mut c_char = ptr::null_mut();
    let out = unsafe { dr_doc_set_layer_style(doc, idx, c.as_ptr(), &mut err) };
    assert!(
        !out.is_null(),
        "set_layer_style({idx}) failed: {}",
        take_err_string(err)
    );
    assert!(err.is_null(), "err_out set on success");
    unsafe { dr_doc_free(doc) };
    out
}

/// Calls `dr_doc_set_layer_style` expecting a NULL result: `Some(message)`
/// when the core reported an error, `None` for a silent refusal. A
/// non-NULL result is freed and reported as a failure.
pub fn style_error(doc: *const DrDocument, idx: usize, json: &str) -> Option<String> {
    let c = CString::new(json).expect("no interior NUL");
    let mut err: *mut c_char = ptr::null_mut();
    let out = unsafe { dr_doc_set_layer_style(doc, idx, c.as_ptr(), &mut err) };
    if !out.is_null() {
        unsafe { dr_doc_free(out) };
        panic!("style_error: the call succeeded for {json}");
    }
    if err.is_null() {
        None
    } else {
        Some(take_err_string(err))
    }
}

/// Drives `dr_doc_set_layer_style` on a copy of the safe-API `doc`:
/// `Ok(Some(new))` on success, `Ok(None)` on a silent refusal (out of range
/// or unchanged), `Err(message)` when the core reports one. `None` for
/// `json` clears.
pub fn try_styled(
    doc: &DrDocument,
    idx: usize,
    json: Option<&str>,
) -> Result<Option<DrDocument>, String> {
    let handle = Box::into_raw(Box::new(doc.clone()));
    let c = json.map(|j| CString::new(j).expect("no interior NUL"));
    let mut err: *mut c_char = ptr::null_mut();
    let out = unsafe {
        dr_doc_set_layer_style(
            handle,
            idx,
            c.as_ref().map_or(ptr::null(), |c| c.as_ptr()),
            &mut err,
        )
    };
    unsafe { dr_doc_free(handle) };
    if out.is_null() {
        if err.is_null() {
            Ok(None)
        } else {
            Err(take_err_string(err))
        }
    } else {
        assert!(err.is_null(), "err_out set on success");
        Ok(Some(*unsafe { Box::from_raw(out) }))
    }
}

/// [`try_styled`], panicking with the core's message on failure — the
/// helper every effect test starts from.
pub fn styled(doc: &DrDocument, idx: usize, json: &str) -> DrDocument {
    match try_styled(doc, idx, Some(json)) {
        Ok(Some(d)) => d,
        Ok(None) => panic!("set_layer_style refused {json}"),
        Err(e) => panic!("set_layer_style failed: {e}"),
    }
}

/// A strict [`LayerStyle::from_json`] that panics with the core's message:
/// the model tests' parser, and — wrapped in a fresh `Arc` — the empty-cache
/// oracle every cached projection is held to.
pub fn parse(json: &str) -> LayerStyle {
    LayerStyle::from_json(json).unwrap_or_else(|e| panic!("{json}: {e}"))
}

/// `dr_doc_set_global_light` on a copy of `doc`, asserting success.
pub fn with_light(doc: &DrDocument, angle: f32, altitude: f32) -> DrDocument {
    let handle = Box::into_raw(Box::new(doc.clone()));
    let out = unsafe { dr_doc_set_global_light(handle, angle, altitude) };
    unsafe { dr_doc_free(handle) };
    assert!(
        !out.is_null(),
        "set_global_light({angle}, {altitude}) refused"
    );
    *unsafe { Box::from_raw(out) }
}

/// ±1 per colour channel, alpha exact (the tolerance for two paths that
/// quantize at different points).
pub fn assert_close(actual: &[u8], expected: &[u8], what: &str) {
    assert_eq!(actual.len(), expected.len(), "{what}: buffer sizes differ");
    for (i, (&a, &e)) in actual.iter().zip(expected).enumerate() {
        let tol = if i % 4 == 3 { 0 } else { 1 };
        assert!(
            (i32::from(a) - i32::from(e)).abs() <= tol,
            "{what}: byte {i} (pixel {}, channel {}) is {a}, expected {e} (±{tol})",
            i / 4,
            i % 4
        );
    }
}

/// The fixture every effect oracle starts from: an opaque WHITE canvas of
/// size `canvas` under one opaque `color` layer of `rect` = (x, y, w, h) at
/// index 1.
pub fn rect_layer_doc(
    canvas: (u32, u32),
    rect: (i32, i32, u32, u32),
    color: [u8; 4],
) -> DrDocument {
    let doc = DrDocument::from_pixels(solid(canvas.0, canvas.1, WHITE));
    let doc = doc
        .adding_image_layer(0, solid(rect.2, rect.3, color), "Rect")
        .expect("add rect layer");
    // The layer is MADE at the rect: its reference point stays (0, 0).
    let placed = doc.with_layer_offset(1, rect.0, rect.1).map(|mut placed| {
        placed.layers[1].reference = (0, 0);
        placed
    });
    placed.unwrap_or(doc)
}

/// The documented feather kernel (doctored_core.h, dr_selection_feather):
/// `taps = nearest_odd(2r + 1)` floored at 3 (ties to the lower odd),
/// `sigma = 0.3 (r - 1) + 0.8`, normalized — rebuilt here from the contract
/// so the oracle shares no code with the core.
pub fn feather_kernel(radius: f32) -> (Vec<f32>, i64) {
    let nearest_odd = |x: f32| -> u32 {
        let n = x.round().max(1.0) as u32;
        if n % 2 == 1 {
            n
        } else if x - (n - 1) as f32 <= (n + 1) as f32 - x {
            n - 1
        } else {
            n + 1
        }
    };
    let taps = nearest_odd(radius * 2.0 + 1.0).max(3);
    let half = (taps / 2) as i64;
    let sigma = 0.3 * (radius - 1.0) + 0.8;
    let mut k: Vec<f32> = (0..taps)
        .map(|i| (-0.5 * ((i as i64 - half) as f32 / sigma).powi(2)).exp())
        .collect();
    let sum: f32 = k.iter().sum();
    for v in &mut k {
        *v /= sum;
    }
    (k, half)
}

/// The independent separable-Gaussian oracle: `plane` (row-major `w * h`
/// coverage bytes) feathered by `radius` per the header contract —
/// horizontal pass into f32, vertical pass, clamp-to-edge sampling, one
/// rounding at the end. Compare with ±1.
pub fn feathered_plane(plane: &[u8], w: u32, h: u32, radius: f32) -> Vec<u8> {
    feathered_field(plane, w, h, radius)
        .into_iter()
        .map(|v| v.clamp(0.0, 255.0).round() as u8)
        .collect()
}

/// [`feathered_plane`] before its one rounding: the feathered values in
/// codes, for an oracle that maps them before rounding (the outer glow's
/// matte stretch, #578).
pub fn feathered_field(plane: &[u8], w: u32, h: u32, radius: f32) -> Vec<f32> {
    let (w, h) = (w as usize, h as usize);
    assert_eq!(plane.len(), w * h);
    if radius <= 0.0 {
        return plane.iter().map(|&v| f32::from(v)).collect();
    }
    let (k, half) = feather_kernel(radius);
    let mut tmp = vec![0f32; w * h];
    for y in 0..h {
        for x in 0..w {
            let mut acc = 0.0;
            for (i, kv) in k.iter().enumerate() {
                let sx = (x as i64 + i as i64 - half).clamp(0, w as i64 - 1) as usize;
                acc += kv * f32::from(plane[y * w + sx]);
            }
            tmp[y * w + x] = acc;
        }
    }
    let mut out = vec![0f32; w * h];
    for y in 0..h {
        for x in 0..w {
            let mut acc = 0.0;
            for (i, kv) in k.iter().enumerate() {
                let sy = (y as i64 + i as i64 - half).clamp(0, h as i64 - 1) as usize;
                acc += kv * tmp[sy * w + x];
            }
            out[y * w + x] = acc;
        }
    }
    out
}

// ------------------------------------------- the content-aware fixtures --
//
// Shared by `content_aware_tests` (what the fill puts on screen) and
// `content_aware_cost_tests` (what it costs and when it refuses), which are
// two files for the same reason every area is: one of them is about pixels
// and the other about caps, and neither should carry the other's fixtures.

/// The `content_aware_fill` request JSON for a canvas-sized u8 `mask`:
/// `{"op":"content_aware_fill","layer","x","y","width","height","ring","seed","sample"}`,
/// the box the app's `ContentAwareFillWork` builds — the bounding box of the
/// nonzero bytes, or the whole canvas when there are none — sampling All
/// when `sample_all`, else Current, in place.
pub fn fill_request(
    idx: usize,
    mask: &[u8],
    (w, h): (u32, u32),
    (ring, seed): (u32, u64),
    sample_all: bool,
) -> CString {
    assert_eq!(mask.len(), w as usize * h as usize, "a canvas-sized mask");
    let mut bounds: Option<(u32, u32, u32, u32)> = None;
    for (i, c) in mask.iter().enumerate() {
        if *c == 0 {
            continue;
        }
        let (x, y) = ((i % w as usize) as u32, (i / w as usize) as u32);
        bounds = Some(match bounds {
            None => (x, y, x, y),
            Some((x0, y0, x1, y1)) => (x0.min(x), y0.min(y), x1.max(x), y1.max(y)),
        });
    }
    let (x, y, width, height) = match bounds {
        Some((x0, y0, x1, y1)) => (x0, y0, x1 - x0 + 1, y1 - y0 + 1),
        None => (0, 0, w, h),
    };
    CString::new(
        json!({
            "op": "content_aware_fill",
            "layer": idx,
            "x": x,
            "y": y,
            "width": width,
            "height": height,
            "ring": ring,
            "seed": seed,
            "sample": if sample_all { "all" } else { "current" },
        })
        .to_string(),
    )
    .unwrap()
}

/// Runs `dr_doc_content_aware_fill_edit` on `doc` (which must be `w` x `h`)
/// with [`fill_request`]'s request and `control` (NULL allowed): the new
/// document (NULL = a refusal) and the error message, if any.
pub fn fill_edit(
    doc: *const DrDocument,
    idx: usize,
    mask: &[u8],
    (w, h): (u32, u32),
    (ring, seed): (u32, u64),
    sample_all: bool,
    control: *const doctored_core::job_control::DrJobControl,
) -> (Option<DrDocument>, Option<String>) {
    let document = unsafe { &*doc };
    assert_eq!(
        (document.width, document.height),
        (w, h),
        "a w x h document"
    );
    let request = fill_request(idx, mask, (w, h), (ring, seed), sample_all);
    let mut err: *mut c_char = ptr::null_mut();
    let out = unsafe {
        doctored_core::ffi_content_aware::dr_doc_content_aware_fill_edit(
            doc,
            request.as_ptr(),
            mask.as_ptr(),
            control,
            &mut err,
        )
    };
    take_doc(out, err)
}

/// [`fill_edit`] on a copy of `doc` with no job control.
pub fn fill(
    doc: &DrDocument,
    idx: usize,
    mask: &[u8],
    dims: (u32, u32),
    rs: (u32, u64),
    sample_all: bool,
) -> (Option<DrDocument>, Option<String>) {
    let handle = Box::into_raw(Box::new(doc.clone()));
    let result = fill_edit(handle, idx, mask, dims, rs, sample_all, ptr::null());
    unsafe { dr_doc_free(handle) };
    result
}

/// Runs `dr_doc_spot_heal_layer_job` — the one spot-heal export — on `doc`
/// under a fresh job control (limit 0 = unlimited), freed before return: the
/// new document (NULL = a refusal) and the error message, if any.
pub fn spot_heal_job(
    doc: *const DrDocument,
    idx: usize,
    overlay: &[u8],
    (w, h): (u32, u32),
    strength: f32,
    (ring, seed, sample): (u32, u64, u32),
    ignore_adjustments: bool,
) -> (Option<DrDocument>, Option<String>) {
    let control = unsafe { doctored_core::ffi_jobs::dr_job_control_create() };
    assert!(!control.is_null(), "a job control");
    let mut err: *mut c_char = ptr::null_mut();
    let out = unsafe {
        doctored_core::ffi_jobs::dr_doc_spot_heal_layer_job(
            doc,
            idx,
            overlay.as_ptr(),
            w,
            h,
            strength,
            ring,
            seed,
            0,
            sample,
            ignore_adjustments,
            control,
            &mut err,
        )
    };
    unsafe { doctored_core::ffi_jobs::dr_job_control_free(control) };
    take_doc(out, err)
}

/// Unpacks an FFI document/error pair into owned Rust values.
pub fn take_doc(out: *mut DrDocument, err: *mut c_char) -> (Option<DrDocument>, Option<String>) {
    let message = if err.is_null() {
        None
    } else {
        Some(take_err_string(err))
    };
    let document = if out.is_null() {
        None
    } else {
        Some(*unsafe { Box::from_raw(out) })
    };
    (document, message)
}

/// The layer's own pixel bytes, read back through the FFI.
pub fn layer_bytes(doc: &DrDocument, idx: usize) -> Vec<u8> {
    let handle = Box::into_raw(Box::new(doc.clone()));
    let out = layer_pixels(handle, idx);
    unsafe { dr_doc_free(handle) };
    out
}

/// A single-layer opaque document whose pixels come from `f`.
pub fn doc_of(w: u32, h: u32, f: impl Fn(u32, u32) -> [u8; 3]) -> DrDocument {
    DrDocument::from_pixels(RgbaImage::from_fn(w, h, |x, y| {
        let c = f(x, y);
        Rgba([c[0], c[1], c[2], 255])
    }))
}

/// A canvas-sized coverage mask from a predicate: 255 inside, 0 outside.
pub fn mask_of(w: u32, h: u32, f: impl Fn(u32, u32) -> bool) -> Vec<u8> {
    (0..w * h)
        .map(|i| if f(i % w, i / w) { 255u8 } else { 0 })
        .collect()
}

/// A deterministic, structured, NON-periodic field: smooth ridges plus a
/// fixed hash of the pixel, so neither a constant nor a tiling can pass a
/// test written against it.
pub fn plasma(x: u32, y: u32) -> [u8; 3] {
    let fx = x as f32 * 0.11;
    let fy = y as f32 * 0.13;
    let base = 128.0 + 60.0 * (fx.sin() + (fy * 0.7).cos());
    let hash = ((x.wrapping_mul(2654435761) ^ y.wrapping_mul(40503)) >> 8) & 15;
    let v = (base + hash as f32 - 8.0).clamp(0.0, 255.0) as u8;
    [v, v.saturating_sub(7), v / 2 + 40]
}

/// Overwrites the masked pixels with a flat colour: the "damage" a fill is
/// asked to repair.
pub fn punch(doc: &DrDocument, mask: &[u8], colour: [u8; 3]) -> DrDocument {
    let (w, h) = (doc.width, doc.height);
    let mut pixels = doc.layer_canvas_image(0).expect("layer 0");
    for y in 0..h {
        for x in 0..w {
            if mask[(y * w + x) as usize] < 128 {
                continue;
            }
            let px = pixels.get_pixel_mut(x, y);
            px.0[..3].copy_from_slice(&colour);
        }
    }
    doc.with_layer_pixels(0, pixels).expect("layer 0")
}

/// `payload` with every `"id":{id}` whose number ENDS there (the next byte
/// is `,` or `}`) replaced by `replacement`. Layer ids come from one
/// process-wide counter the parallel tests share, so a bare `str::replace`
/// of `"id":1` would also rewrite `"id":12`.
pub fn replace_comp_id(payload: &str, id: u64, replacement: &str) -> String {
    let needle = format!(r#""id":{id}"#);
    let mut out = String::with_capacity(payload.len());
    let mut rest = payload;
    while let Some(at) = rest.find(&needle) {
        let end = at + needle.len();
        let whole = matches!(rest.as_bytes().get(end), Some(b',' | b'}'));
        out.push_str(&rest[..at]);
        out.push_str(if whole { replacement } else { &rest[at..end] });
        rest = &rest[end..];
    }
    out.push_str(rest);
    out
}

/// The tiles a rect `(x0, y0, x1, y1)` (exclusive, non-empty) visits: the
/// checkpoints one job-polled tile read or write of it makes.
pub fn rect_tiles(rect: (u32, u32, u32, u32)) -> u64 {
    let e = doctored_core::tile_store::TILE_EDGE;
    let columns = (rect.2 - 1) / e - rect.0 / e + 1;
    let rows = (rect.3 - 1) / e - rect.1 / e + 1;
    u64::from(columns * rows)
}

/// The checkpoints a new-layer retouch's deposit makes on a canvas of
/// `canvas_pixels`, whose domain's bounding box is `rect` (exclusive): the
/// domain pass every 4096 canvas pixels, the reservation, the healed rect
/// read and the write (one per tile each; the transparent new layer is never
/// read), the lift every 4096 rect pixels.
pub fn deposit_checkpoints(canvas_pixels: u64, rect: (u32, u32, u32, u32)) -> u64 {
    let area = u64::from(rect.2 - rect.0) * u64::from(rect.3 - rect.1);
    canvas_pixels.div_ceil(4096) + 1 + 2 * rect_tiles(rect) + area.div_ceil(4096)
}

pub mod extended_xmp_fixture;
pub mod icc_chain;
pub mod metadata_fixtures;
pub mod p3_oracle;
pub mod probe;
pub mod psd_photoshop;
pub mod selected_pixels;
pub mod sha256;
pub mod white_balance_oracle;

// ------------------------------------------- the 12E-1 identity wrapper --

/// Phase 12E-1 made the OUTERMOST `.rz` version word the ALWAYS-written
/// identity wrapper — 45 at byte depth, 46 at typed depth — and appended one
/// `LIDS` table (tag, u32 schema, u32 count, one u64 per record) after every
/// prior tail. A test that cares about the BODY's version, or about the bytes
/// the PRIOR writer made, says so through these three rather than repeating
/// the arithmetic.
///
/// `tests/common/identity.rs` holds a TWIN of `identity_tail`,
/// `without_identity` and `drdc_word`, and both copies exist on purpose: this
/// one serves the files that use `common` and take the whole helper
/// module, while the twin is a standalone `#[path]` module a test file can
/// pull in ALONE — which is what a format test that wants nothing else from
/// `common` needs, since `mod common;` drags in every fixture helper and its
/// dependencies. Change one, change the other.
pub const IDENTITY_BYTE_VERSION: u32 = 45;
pub const IDENTITY_PRECISION_VERSION: u32 = 46;

/// The `LIDS` tail's size for a document of `records` records.
pub fn identity_tail(records: usize) -> usize {
    12 + 8 * records
}

/// `bytes` with the identity wrapper removed — the outermost word and the
/// `LIDS` tail cut away — which is byte-for-byte what the prior writer made.
pub fn without_identity(bytes: &[u8], records: usize) -> Vec<u8> {
    let tail = identity_tail(records);
    assert!(
        bytes.len() > 8 + tail && &bytes[bytes.len() - tail..bytes.len() - tail + 4] == b"LIDS",
        "the identity table closes every file this build writes"
    );
    let mut out = bytes[..4].to_vec();
    out.extend_from_slice(&bytes[8..bytes.len() - tail]);
    out
}

/// The `.rz` version word at `index`, counting the outermost one as 0.
pub fn drdc_word(bytes: &[u8], index: usize) -> u32 {
    let at = 4 + index * 4;
    u32::from_le_bytes(bytes[at..at + 4].try_into().expect("4 bytes"))
}

/// A file of the Blend If Photoshop take (`blend_if_multichannel_take.py`,
/// take1's `jobs/<job>/<name>`), kept outside the repo: every CMYK file of
/// the take embeds the 557 KB SWOP profile, past the repo fixture folder's
/// 64 KB cap (`cmyk_psd_styles.rs`'s rule). `None` when the take folder is
/// absent.
pub fn take1_558(job: &str, name: &str) -> Option<std::path::PathBuf> {
    let home = std::env::var("HOME").ok()?;
    let path = std::path::PathBuf::from(home)
        .join("devel/doctored-fixtures/photoshop/blend-if-multichannel-558/take1/jobs")
        .join(job)
        .join(name);
    path.is_file().then_some(path)
}
