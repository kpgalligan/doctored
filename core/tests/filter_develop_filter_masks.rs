//! Filters ▸ Develop Filter's masks: `develop_filter` with `masks`
//! behind `dr_image_filter_op` at 8 bits and `selected_edit` at 16. Each
//! expectation is an oracle written here or the filter's own unmasked result
//! (`filter_develop_filter.rs` holds that against its reference chain); there are
//! no golden images.
//!
//! The bounds, declared before the first run and never widened:
//!
//! - FULL. A `full` mask at density 1 with settings S over global G is within
//!   1 code of two sequential `develop_filter` calls (G, then S), at 8 and 16 bits.
//! - RADIAL. A radial mask is within 1 level of the per-pixel lerp of the
//!   source toward the unmasked S result by the smoothstep written here,
//!   quantized to u16 as the mask stores it.
//! - RANGE. A luminance range on a grey ramp changes exactly the pixels whose
//!   CIE L* (written here) is inside the bounds, leaving a 0.5 L* margin at
//!   each edge unjudged; a sampled colour range at 16 bits changes exactly the
//!   sampled region.
//! - MONOTONE. More density never changes a pixel less; more feather spreads
//!   more change outside a hard radial.
//! - ORDER. Two full locals compound in request order: Exposure then Contrast
//!   is within 1 level of the two sequential calls and differs from Contrast
//!   then Exposure by at least 8 levels somewhere.
//! - EMPTY. `masks: []` is bit-identical to no masks at both depths.
//! - DETAIL. A `full` mask's Sharpness +60 is within 1 code of one
//!   `smart_sharpen {amount 90, radius 1, noise 0, remove gaussian}`, −60 of
//!   one `gaussian_blur {radius 1.8}` and Noise Reduction 60 of one
//!   `reduce_noise {radius 0.5, luminance 0.06, detail 0, color 0}`, each at
//!   8 bits (`dr_image_filter_op`; it has no `gaussian_blur`, whose 8-bit
//!   oracle is the 16-bit selected filter of the widened source narrowed to
//!   `round(v/257)`) and 16 (the selected filter); a radial
//!   with Sharpness is within 1 level of the per-pixel lerp toward the full
//!   mask's result by the smoothstep written here; Noise Reduction then
//!   Sharpness on a full mask is within 2 codes of the two filters in turn at
//!   16 bits; a mask whose only detail is 0 changes nothing.
//! - Refusals by sentence.
use doctored_core::ffi::{dr_image_free, dr_image_from_rgba, dr_image_pixels_rgba, dr_string_free};
use doctored_core::ffi_filters::dr_image_filter_op;
use doctored_core::ffi_precision_edit::{dr_precision_doc_edit, dr_precision_doc_edit_plan};
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer};
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use doctored_core::precision_mask::{PrecisionChannel, PrecisionPlane};
use doctored_core::selected_adjustment::SelectedEdit;
use serde_json::{json, Value};
use std::ffi::{c_char, CStr, CString};
use std::ptr;

use crate::develop_filter_reference::s2l;

const FLOAT_REFUSAL: &str = "develop_filter masks are not available on a 32-bit float layer";
const SCALAR_REFUSAL: &str = "develop_filter masks edit layer pixels only";
const UNRESOLVED_REFUSAL: &str =
    "develop_filter subject, people and sky masks are made by the app before the filter runs";
const PROFILE_REFUSAL: &str =
    "calibrated Exposure and White Balance require an RGB matrix/TRC document profile";
const WORK_REFUSAL: &str = "develop_filter exceeds the bounded pixel-work limit";

fn base64(bytes: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(T[((n >> (18 - 6 * i)) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

fn srgb_profile() -> String {
    base64(IccProfile::srgb().bytes())
}

fn message(err: *mut c_char) -> String {
    if err.is_null() {
        return String::new();
    }
    let m = unsafe { CStr::from_ptr(err) }
        .to_string_lossy()
        .into_owned();
    unsafe { dr_string_free(err) };
    m
}

/// `(pixels, message)` of `develop_filter` with `params` at 8 bits.
fn run8(w: u32, h: u32, pixels: &[u8], params: &Value) -> (Option<Vec<u8>>, String) {
    run_op8("develop_filter", w, h, pixels, params)
}

/// `(pixels, message)` of filter `op` with `params` at 8 bits.
fn run_op8(op: &str, w: u32, h: u32, pixels: &[u8], params: &Value) -> (Option<Vec<u8>>, String) {
    let img = unsafe { dr_image_from_rgba(pixels.as_ptr(), w, h) };
    assert!(!img.is_null());
    let op = CString::new(op).unwrap();
    let text = CString::new(params.to_string()).unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    let out = unsafe { dr_image_filter_op(img, op.as_ptr(), text.as_ptr(), ptr::null(), &mut err) };
    let result = (!out.is_null()).then(|| {
        let count = (w * h) as usize * 4;
        let v = unsafe { std::slice::from_raw_parts(dr_image_pixels_rgba(out), count) }.to_vec();
        unsafe { dr_image_free(out) };
        v
    });
    unsafe { dr_image_free(img) };
    (result, message(err))
}

fn filter8(w: u32, h: u32, pixels: &[u8], params: &Value) -> Vec<u8> {
    let (out, message) = run8(w, h, pixels, params);
    assert!(message.is_empty(), "{params}: {message}");
    out.unwrap_or_else(|| panic!("{params}: a bypass"))
}

fn doc16(w: u32, h: u32, pixels: Vec<[u16; 4]>) -> PrecisionDocument {
    PrecisionDocument::from_image(
        PrecisionImage::from_rgba16(w, h, pixels, IccProfile::srgb(), None).unwrap(),
        Metadata::default(),
        Resolution::default(),
    )
    .unwrap()
}

fn edit(params: &Value) -> SelectedEdit {
    edit_op("develop_filter", params)
}

fn edit_op(op: &str, params: &Value) -> SelectedEdit {
    SelectedEdit::parse(&json!({"layer":0,"op":op,"params":params}).to_string())
        .unwrap_or_else(|e| panic!("{op} {params}: {e}"))
}

fn run16(
    w: u32,
    h: u32,
    pixels: &[[u16; 4]],
    params: &Value,
) -> Result<Option<Vec<[u16; 4]>>, String> {
    run_op16("develop_filter", w, h, pixels, params)
}

fn run_op16(
    op: &str,
    w: u32,
    h: u32,
    pixels: &[[u16; 4]],
    params: &Value,
) -> Result<Option<Vec<[u16; 4]>>, String> {
    doc16(w, h, pixels.to_vec())
        .selected_edit(&edit_op(op, params), &[], REFERENCE_WORKING_BYTES, 0, None)
        .map(|d| d.map(|d| d.layer_source(0).unwrap().to_rgba16().unwrap()))
}

/// Filter `op` at 16 bits, which must change the layer.
fn op16(op: &str, w: u32, h: u32, pixels: &[[u16; 4]], params: &Value) -> Vec<[u16; 4]> {
    run_op16(op, w, h, pixels, params)
        .unwrap_or_else(|e| panic!("{op} {params}: {e}"))
        .unwrap_or_else(|| panic!("{op} {params}: no edit"))
}

/// Filter `op` at 8 bits, which must change the image.
fn op8(op: &str, w: u32, h: u32, pixels: &[u8], params: &Value) -> Vec<u8> {
    let (out, message) = run_op8(op, w, h, pixels, params);
    assert!(message.is_empty(), "{op} {params}: {message}");
    out.unwrap_or_else(|| panic!("{op} {params}: a bypass"))
}

fn filter16(w: u32, h: u32, pixels: &[[u16; 4]], params: &Value) -> Vec<[u16; 4]> {
    run16(w, h, pixels, params)
        .unwrap_or_else(|e| panic!("{params}: {e}"))
        .unwrap_or_else(|| panic!("{params}: no edit"))
}

/// Opaque structure: every channel walks the range.
fn plate(w: u32, h: u32) -> Vec<u8> {
    (0..w * h)
        .flat_map(|i| {
            let (x, y) = (i % w, i / w);
            let value = |offset: u32| ((x * 23 + y * 41 + offset + (i % 3) * 17) % 256) as u8;
            [value(0), value(80), value(160), 255]
        })
        .collect()
}

fn widen(pixels: &[u8]) -> Vec<[u16; 4]> {
    pixels
        .chunks_exact(4)
        .map(|p| [0, 1, 2, 3].map(|c| u16::from(p[c]) * 257))
        .collect()
}

fn worst8(a: &[u8], b: &[u8]) -> i32 {
    a.iter()
        .zip(b)
        .map(|(x, y)| (i32::from(*x) - i32::from(*y)).abs())
        .max()
        .unwrap_or(0)
}

fn worst16(a: &[[u16; 4]], b: &[[u16; 4]]) -> i32 {
    a.iter()
        .zip(b)
        .flat_map(|(x, y)| (0..4).map(move |c| (i32::from(x[c]) - i32::from(y[c])).abs()))
        .max()
        .unwrap_or(0)
}

/// `params` with `extra`'s keys added.
fn with(params: &Value, extra: Value) -> Value {
    let mut out = params.as_object().cloned().unwrap_or_default();
    for (k, v) in extra.as_object().unwrap() {
        out.insert(k.clone(), v.clone());
    }
    Value::Object(out)
}

// ---- FULL --------------------------------------------------------------------------

#[test]
fn a_full_mask_equals_two_sequential_calls() {
    let (w, h) = (40_u32, 30_u32);
    let src = plate(w, h);
    let global = json!({"temperature": 20, "whites": 10, "vibrance": 15});
    // The sequential calls round to the depth between the two, and a local
    // whose slope is above 1 magnifies that half code past the bound, so S
    // darkens, flattens and desaturates (slope at most 1) at both depths.
    let local = json!({"exposure": -0.5, "contrast": -20, "saturation": -15});
    let masked = with(
        &global,
        json!({"masks": [with(&local, json!({"shape": {"op": "full"}}))]}),
    );
    let out = filter8(w, h, &src, &masked);
    let first = filter8(w, h, &src, &global);
    let second = filter8(w, h, &first, &local);
    let worst = worst8(&out, &second);
    assert!(
        worst <= 1,
        "8-bit: {worst} levels from the sequential calls"
    );

    let global16 = json!({"temperature": 20, "vibrance": 15});
    let local16 = json!({"exposure": -0.3, "saturation": -15});
    let src16 = widen(&src);
    let masked16 = with(
        &global16,
        json!({"masks": [with(&local16, json!({"shape": {"op": "full"}}))]}),
    );
    let out16 = filter16(w, h, &src16, &masked16);
    let first16 = filter16(w, h, &src16, &global16);
    let second16 = filter16(w, h, &first16, &local16);
    let worst = worst16(&out16, &second16);
    assert!(
        worst <= 1,
        "16-bit: {worst} codes from the sequential calls"
    );
}

// ---- RADIAL ------------------------------------------------------------------------

/// The radial falloff written from the shape's description: 1 inside
/// `hardness`, 0 at and beyond the ellipse, smoothstep between.
fn radial(p: [f64; 2], center: [f64; 2], radius: [f64; 2], hardness: f64) -> f64 {
    let d = ((p[0] - center[0]) / radius[0]).hypot((p[1] - center[1]) / radius[1]);
    if d <= hardness {
        return 1.0;
    }
    if d >= 1.0 {
        return 0.0;
    }
    let t = (d - hardness) / (1.0 - hardness);
    1.0 - t * t * (3.0 - 2.0 * t)
}

#[test]
fn a_radial_mask_lerps_by_its_smoothstep() {
    let (w, h) = (60_u32, 40_u32);
    let src = plate(w, h);
    let local = json!({"exposure": 1.0, "saturation": -30});
    let full = filter8(w, h, &src, &local);
    let (cx, cy, rx, ry, hardness) = (0.45, 0.55, 0.3, 0.4, 0.25);
    let params = json!({"masks": [with(&local, json!({"shape": {"op": "radial",
        "center": [cx, cy], "radius": [rx, ry], "hardness": hardness}}))]});
    let out = filter8(w, h, &src, &params);
    let mut worst = 0.0_f64;
    let mut moved = 0;
    for y in 0..h {
        for x in 0..w {
            let i = (y * w + x) as usize;
            let t = radial(
                [f64::from(x) + 0.5, f64::from(y) + 0.5],
                [cx * f64::from(w), cy * f64::from(h)],
                [rx * f64::from(w), ry * f64::from(h)],
                hardness,
            );
            let t = (t * 65535.0).round() / 65535.0;
            for c in 0..3 {
                let a = f64::from(src[i * 4 + c]);
                let b = f64::from(full[i * 4 + c]);
                let want = (a + (b - a) * t).round();
                worst = worst.max((want - f64::from(out[i * 4 + c])).abs());
            }
            if t == 0.0 {
                assert_eq!(out[i * 4..i * 4 + 4], src[i * 4..i * 4 + 4], "{x},{y}");
            } else {
                moved += 1;
            }
        }
    }
    assert!(worst <= 1.0, "{worst} levels from the lerp");
    assert!(moved > 300, "{moved}");
}

// ---- RANGE -------------------------------------------------------------------------

/// CIE L* of an sRGB grey `v` (0…1): a neutral's Y is its decoded value
/// against the D50 white, so L* = 116·f(Y) − 16.
fn lightness(v: f64) -> f64 {
    let y = s2l(v);
    let f = if y > 216.0 / 24389.0 {
        y.cbrt()
    } else {
        (24389.0 / 27.0 * y + 16.0) / 116.0
    };
    116.0 * f - 16.0
}

#[test]
fn a_luminance_range_follows_cie_lightness() {
    let (w, h) = (256_u32, 2_u32);
    let src: Vec<u8> = (0..w * h)
        .flat_map(|i| {
            let v = (i % w) as u8;
            [v, v, v, 255]
        })
        .collect();
    let local = json!({"exposure": 1.0});
    let full = filter8(w, h, &src, &local);
    let (low, high) = (0.35, 0.65);
    let params = json!({"range_profile": srgb_profile(), "masks": [with(&local, json!({
        "shape": {"op": "full"},
        "range": {"luminance": {"low": low, "high": high, "low_falloff": 0, "high_falloff": 0,
            "invert": false}}}))]});
    let out = filter8(w, h, &src, &params);
    let (mut inside, mut outside) = (0, 0);
    for i in 0..(w * h) as usize {
        let l = lightness(f64::from(src[i * 4]) / 255.0);
        if (l - low * 100.0).abs() < 0.5 || (l - high * 100.0).abs() < 0.5 {
            continue;
        }
        if l > low * 100.0 && l < high * 100.0 {
            inside += 1;
            assert_eq!(out[i * 4..i * 4 + 4], full[i * 4..i * 4 + 4], "grey {i}");
        } else {
            outside += 1;
            assert_eq!(out[i * 4..i * 4 + 4], src[i * 4..i * 4 + 4], "grey {i}");
        }
    }
    assert!(inside > 50 && outside > 100, "{inside} {outside}");
}

#[test]
fn a_sampled_colour_range_selects_the_sampled_region_at_16_bits() {
    let (w, h) = (24_u32, 16_u32);
    // Left half a dark grey, right half a saturated red.
    let src: Vec<[u16; 4]> = (0..w * h)
        .map(|i| {
            if i % w < w / 2 {
                [20000, 20000, 20000, 65535]
            } else {
                [52000, 9000, 7000, 65535]
            }
        })
        .collect();
    let local = json!({"exposure": -0.7});
    let full = filter16(w, h, &src, &local);
    let params = json!({"range_profile": srgb_profile(), "masks": [with(&local, json!({
        "shape": {"op": "full"},
        "range": {"color": {"sample": [0.8, 0.5], "amount": 10, "falloff": 0, "invert": false}}}))]});
    let out = filter16(w, h, &src, &params);
    for i in 0..(w * h) as usize {
        let want = if i % w as usize >= (w / 2) as usize {
            full[i]
        } else {
            src[i]
        };
        assert_eq!(out[i], want, "pixel {i}");
    }
}

// ---- MONOTONE ----------------------------------------------------------------------

#[test]
fn density_and_feather_are_monotone() {
    let (w, h) = (48_u32, 48_u32);
    let src: Vec<u8> = (0..w * h).flat_map(|_| [90, 110, 130, 255]).collect();
    let mask = |density: f64, feather: f64| {
        json!({"masks": [{"shape": {"op": "radial", "center": [0.5, 0.5],
            "radius": [0.25, 0.25], "hardness": 1.0}, "density": density,
            "feather": feather, "exposure": 1.0}]})
    };
    let change = |out: &[u8], i: usize| i32::from(out[i * 4 + 1]) - i32::from(src[i * 4 + 1]);
    let densities: Vec<Vec<u8>> = [0.25, 0.5, 1.0]
        .iter()
        .map(|d| filter8(w, h, &src, &mask(*d, 0.0)))
        .collect();
    for i in 0..(w * h) as usize {
        for pair in densities.windows(2) {
            assert!(change(&pair[0], i) <= change(&pair[1], i), "pixel {i}");
        }
    }
    let outside = |out: &[u8]| -> i64 {
        let mut sum = 0;
        for y in 0..h {
            for x in 0..w {
                let d = (f64::from(x) + 0.5 - 24.0).hypot(f64::from(y) + 0.5 - 24.0);
                if d > 12.5 {
                    sum += i64::from(change(out, (y * w + x) as usize));
                }
            }
        }
        sum
    };
    let spread: Vec<i64> = [0.0, 20.0, 60.0]
        .iter()
        .map(|f| outside(&filter8(w, h, &src, &mask(1.0, *f))))
        .collect();
    assert_eq!(spread[0], 0, "a hard radial changed a pixel outside it");
    assert!(spread[0] < spread[1] && spread[1] < spread[2], "{spread:?}");
}

// ---- ORDER -------------------------------------------------------------------------

#[test]
fn locals_compound_in_request_order() {
    let (w, h) = (256_u32, 2_u32);
    let src: Vec<u8> = (0..w * h)
        .flat_map(|i| {
            let v = (i % w) as u8;
            [v, v, v, 255]
        })
        .collect();
    let exposure = json!({"exposure": 1.0});
    let contrast = json!({"contrast": 50});
    let full = |adjust: &Value| with(adjust, json!({"shape": {"op": "full"}}));
    let ordered = filter8(
        w,
        h,
        &src,
        &json!({"masks": [full(&exposure), full(&contrast)]}),
    );
    let swapped = filter8(
        w,
        h,
        &src,
        &json!({"masks": [full(&contrast), full(&exposure)]}),
    );
    let sequential = filter8(w, h, &filter8(w, h, &src, &exposure), &contrast);
    let worst = worst8(&ordered, &sequential);
    assert!(worst <= 1, "{worst} levels from the sequential calls");
    assert!(worst8(&ordered, &swapped) >= 8, "the order did not matter");
}

// ---- EMPTY -------------------------------------------------------------------------

#[test]
fn no_masks_and_an_empty_array_are_bit_identical() {
    let (w, h) = (36_u32, 28_u32);
    let src = plate(w, h);
    let global = json!({"exposure": 0.5, "contrast": 30, "shadows": 40, "clarity": 30});
    let plain = filter8(w, h, &src, &global);
    let empty = filter8(w, h, &src, &with(&global, json!({"masks": []})));
    assert!(plain == empty, "8-bit: masks [] differs");
    let src16 = widen(&src);
    let plain16 = filter16(w, h, &src16, &global);
    let empty16 = filter16(w, h, &src16, &with(&global, json!({"masks": []})));
    assert!(plain16 == empty16, "16-bit: masks [] differs");
    // An identity local leaves the request the global one.
    let identity = with(
        &global,
        json!({"masks": [{"shape": {"op": "full"}, "density": 0, "exposure": 1}]}),
    );
    assert!(filter8(w, h, &src, &identity) == plain);
    let (out, message) = run8(
        w,
        h,
        &src,
        &json!({"masks": [{"shape": {"op": "full"}, "exposure": 0}]}),
    );
    assert!(
        out.is_none() && message.is_empty(),
        "an identity request: {message}"
    );
}

// ---- DETAIL ------------------------------------------------------------------------

const DETAIL_REFUSAL: &str =
    "develop_filter mask 0: invalid local detail: sharpness is -100 to 100, noise reduction 0 to 100";

/// Each detail setting beside the global filter it is (`local_detail`'s
/// mapping): Sharpness +60, −60 and Noise Reduction 60.
fn detail_cases() -> [(Value, &'static str, Value); 3] {
    [
        (
            json!({"sharpness": 60}),
            "smart_sharpen",
            json!({"amount": 90, "radius": 1, "noise": 0, "remove": "gaussian"}),
        ),
        (
            json!({"sharpness": -60}),
            "gaussian_blur",
            json!({"radius": 1.8}),
        ),
        (
            json!({"noise_reduction": 60}),
            "reduce_noise",
            json!({"radius": 0.5, "luminance": 0.06, "detail": 0, "color": 0}),
        ),
    ]
}

fn full_mask(local: &Value) -> Value {
    json!({"masks": [with(local, json!({"shape": {"op": "full"}}))]})
}

#[test]
fn a_full_mask_detail_is_its_global_filter() {
    let (w, h) = (40_u32, 30_u32);
    let src = plate(w, h);
    let src16 = widen(&src);
    for (local, op, params) in detail_cases() {
        let want = if op == "gaussian_blur" {
            op16(op, w, h, &src16, &params)
                .iter()
                .flat_map(|p| p.map(|v| ((f64::from(v) / 257.0).round()) as u8))
                .collect()
        } else {
            op8(op, w, h, &src, &params)
        };
        assert!(worst8(&want, &src) > 4, "{op}: the oracle barely moved");
        let out = filter8(w, h, &src, &full_mask(&local));
        let worst = worst8(&out, &want);
        assert!(worst <= 1, "8-bit {local}: {worst} levels from {op}");

        let want16 = op16(op, w, h, &src16, &params);
        let out16 = filter16(w, h, &src16, &full_mask(&local));
        let worst = worst16(&out16, &want16);
        assert!(worst <= 1, "16-bit {local}: {worst} codes from {op}");
    }
}

#[test]
fn a_radial_mask_lerps_its_detail_by_its_smoothstep() {
    let (w, h) = (60_u32, 40_u32);
    let src = plate(w, h);
    let local = json!({"sharpness": 70});
    let full = filter8(w, h, &src, &full_mask(&local));
    let (cx, cy, rx, ry, hardness) = (0.5, 0.45, 0.35, 0.4, 0.3);
    let params = json!({"masks": [with(&local, json!({"shape": {"op": "radial",
        "center": [cx, cy], "radius": [rx, ry], "hardness": hardness}}))]});
    let out = filter8(w, h, &src, &params);
    let mut worst = 0.0_f64;
    let mut moved = 0;
    for y in 0..h {
        for x in 0..w {
            let i = (y * w + x) as usize;
            let t = radial(
                [f64::from(x) + 0.5, f64::from(y) + 0.5],
                [cx * f64::from(w), cy * f64::from(h)],
                [rx * f64::from(w), ry * f64::from(h)],
                hardness,
            );
            let t = (t * 65535.0).round() / 65535.0;
            for c in 0..3 {
                let a = f64::from(src[i * 4 + c]);
                let b = f64::from(full[i * 4 + c]);
                let want = (a + (b - a) * t).round();
                worst = worst.max((want - f64::from(out[i * 4 + c])).abs());
            }
            if t == 0.0 {
                assert_eq!(out[i * 4..i * 4 + 4], src[i * 4..i * 4 + 4], "{x},{y}");
            } else {
                moved += 1;
            }
        }
    }
    assert!(worst <= 1.0, "{worst} levels from the lerp");
    assert!(moved > 300, "{moved}");
}

#[test]
fn noise_reduction_then_sharpness_is_the_two_filters_in_turn() {
    let (w, h) = (40_u32, 30_u32);
    let src16 = widen(&plate(w, h));
    let out = filter16(
        w,
        h,
        &src16,
        &full_mask(&json!({"noise_reduction": 60, "sharpness": 60})),
    );
    let [sharpen, _, denoise] = detail_cases();
    let first = op16(denoise.1, w, h, &src16, &denoise.2);
    let second = op16(sharpen.1, w, h, &first, &sharpen.2);
    let worst = worst16(&out, &second);
    assert!(worst <= 2, "{worst} codes from the two filters in turn");
}

#[test]
fn a_mask_whose_only_detail_is_zero_changes_nothing() {
    let (w, h) = (12_u32, 8_u32);
    let src = plate(w, h);
    let params = full_mask(&json!({"sharpness": 0, "noise_reduction": 0}));
    let (out, message) = run8(w, h, &src, &params);
    assert!(out.is_none() && message.is_empty(), "8-bit: {message}");
    assert!(
        run16(w, h, &widen(&src), &params).unwrap().is_none(),
        "16-bit"
    );
}

#[test]
fn detail_refuses_by_sentence() {
    for detail in [
        json!({"sharpness": 101}),
        json!({"sharpness": -101}),
        json!({"noise_reduction": -1}),
        json!({"noise_reduction": 101}),
    ] {
        assert_eq!(
            refused8(8, 6, &full_mask(&detail)),
            DETAIL_REFUSAL,
            "{detail}"
        );
    }
    assert_eq!(
        refused8(8, 6, &full_mask(&json!({"sharpness": "a"}))),
        "develop_filter mask 0: sharpness must be numeric"
    );
}

// ---- Refusals ----------------------------------------------------------------------

fn refused8(w: u32, h: u32, params: &Value) -> String {
    let src = vec![128_u8; (w * h * 4) as usize];
    let (out, message) = run8(w, h, &src, params);
    assert!(out.is_none(), "accepted");
    message
}

#[test]
fn requests_refuse_by_sentence() {
    let full = json!({"shape": {"op": "full"}, "exposure": 1});
    assert_eq!(
        refused8(
            8,
            6,
            &json!({"masks": [{"shape": {"op": "subject"}, "exposure": 1}]})
        ),
        UNRESOLVED_REFUSAL
    );
    assert_eq!(
        refused8(
            8,
            6,
            &json!({"masks": [{"shape": {"op": "full"}, "exposure": 1,
            "range": {"luminance": {"low": 0.2, "high": 0.8, "low_falloff": 0,
            "high_falloff": 0, "invert": false}}}]})
        ),
        PROFILE_REFUSAL
    );
    let seventeen: Vec<Value> = (0..17).map(|_| full.clone()).collect();
    assert_eq!(
        refused8(8, 6, &json!({"masks": seventeen})),
        "develop_filter takes at most 16 masks"
    );
    assert_eq!(
        refused8(
            8,
            6,
            &json!({"masks": [{"shape": {"op": "map", "generated": "sky",
            "width": 1025, "height": 1024, "samples": ""}, "exposure": 1}]})
        ),
        "develop_filter mask 0: a map has 1 to 1048576 samples (1024 × 1024)"
    );
    assert_eq!(
        refused8(
            8,
            6,
            &json!({"masks": [{"shape": {"op": "full"}, "exposure": 1, "foo": 1}]})
        ),
        "develop_filter mask 0: unexpected mask parameter foo"
    );
    assert_eq!(
        refused8(
            8,
            6,
            &json!({"masks": [{"shape": {"op": "full"}, "exposure": 5}]})
        ),
        "develop_filter mask 0: exposure must be finite in [-4,4]"
    );
    assert_eq!(
        refused8(
            8,
            6,
            &json!({"masks": [{"shape": {"op": "full"}, "vibrance": 5}]})
        ),
        "develop_filter mask 0: unexpected mask parameter vibrance"
    );
    let dab = json!({"center": [0.5, 0.5], "radius": 0.1, "hardness": 0.5, "opacity": 1});
    let dabs: Vec<Value> = (0..4097).map(|_| dab.clone()).collect();
    assert_eq!(
        refused8(
            8,
            6,
            &json!({"masks": [{"shape": {"op": "empty"}, "exposure": 1,
            "brush": [{"mode": "add", "dabs": dabs}]}]})
        ),
        "local brush exceeds 4096 dabs; use a shorter stroke"
    );
    // A 2000 × 2000 target: a dab of radius 1 (the longer side) covers all
    // 4,000,000 pixels, so 64 dabs are 256 million evaluated pixels, the
    // per-stroke limit, and 65 are past it.
    let (w, h) = (2000_u32, 2000_u32);
    let big = json!({"center": [0.5, 0.5], "radius": 1.0, "hardness": 0.5, "opacity": 1});
    let stroke = |count: usize| json!({"mode": "add", "dabs": vec![big.clone(); count]});
    assert_eq!(
        refused8(
            w,
            h,
            &json!({"masks": [{"shape": {"op": "empty"}, "exposure": 1,
            "brush": [stroke(65)]}]})
        ),
        "local brush exceeds 256 million evaluated pixels; use a shorter stroke or a smaller brush"
    );
    // 100 strokes at the limit are 25.6 billion evaluated pixels, past the
    // uncontrolled 24 billion work ceiling though each stroke is allowed.
    let strokes: Vec<Value> = (0..100).map(|_| stroke(64)).collect();
    assert_eq!(
        refused8(
            w,
            h,
            &json!({"masks": [{"shape": {"op": "empty"}, "exposure": 1,
            "brush": strokes}]})
        ),
        WORK_REFUSAL
    );
}

#[test]
fn a_float_layer_and_a_scalar_target_refuse_masks_by_name() {
    let (w, h) = (6_u32, 4_u32);
    let masks = json!({"masks": [{"shape": {"op": "full"}, "exposure": 1}]});
    let float = crate::float_fixture::float_doc(
        w,
        h,
        &IccProfile::srgb(),
        &[vec![[0.4, 0.5, 0.6, 1.0]; (w * h) as usize]],
    );
    let refused = float
        .selected_edit(&edit(&masks), &[], REFERENCE_WORKING_BYTES, 0, None)
        .err();
    assert_eq!(refused.as_deref(), Some(FLOAT_REFUSAL));

    let samples: Vec<u16> = (0..w * h).map(|i| (1000 + i * 1031) as u16).collect();
    let channel = PrecisionChannel::new(
        "Alpha 1",
        PrecisionPlane::from_u16(w, h, samples, None).unwrap(),
    )
    .unwrap();
    let layer = PrecisionDocumentLayer::raster(
        "Layer",
        PrecisionImage::from_rgba16(
            w,
            h,
            vec![[0, 0, 0, 65535]; (w * h) as usize],
            IccProfile::srgb(),
            None,
        )
        .unwrap(),
        (0, 0),
        1.0,
        true,
    )
    .unwrap();
    let d = PrecisionDocument::from_parts_and_channels(
        w,
        h,
        IccProfile::srgb(),
        Metadata::default(),
        Resolution::default(),
        vec![layer],
        vec![channel],
    )
    .unwrap();
    let coverage = vec![65535_u16; (w * h) as usize];
    let request = json!({"op": "scalar_edit", "target": "channel", "index": 0, "selection": true,
        "operation": {"kind": "filter", "edit": {"layer": 0, "op": "develop_filter",
        "params": masks}}});
    let text = CString::new(request.to_string()).unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    let planned = unsafe {
        dr_precision_doc_edit_plan(&d, text.as_ptr(), REFERENCE_WORKING_BYTES, 0, &mut err)
    };
    assert!(planned.is_null());
    assert_eq!(message(err), SCALAR_REFUSAL);
    let mut err: *mut c_char = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_edit(
            &d,
            text.as_ptr(),
            coverage.as_ptr(),
            coverage.len(),
            REFERENCE_WORKING_BYTES,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(out.is_null());
    assert_eq!(message(err), SCALAR_REFUSAL);
}

#[test]
fn a_host_map_is_read_bilinearly() {
    // A 2 × 1 map, black then white, over a 4 × 2 target: the pixel centres
    // read the map at x = 0.25, 0.75, 1.25, 1.75, so the coverage is 0,
    // 0.25, 0.75, 1 (clamped at the ends).
    let (w, h) = (4_u32, 2_u32);
    let src: Vec<u8> = (0..w * h).flat_map(|_| [100, 100, 100, 255]).collect();
    let local = json!({"exposure": 1.0});
    let full = filter8(w, h, &src, &local);
    let params = json!({"masks": [with(&local, json!({"shape": {"op": "map",
        "generated": "sky", "width": 2, "height": 1, "samples": base64(&[0, 255])}}))]});
    let out = filter8(w, h, &src, &params);
    for (x, t) in [0.0, 0.25, 0.75, 1.0].iter().enumerate() {
        for y in 0..h as usize {
            let i = y * w as usize + x;
            let (a, b) = (f64::from(src[i * 4]), f64::from(full[i * 4]));
            let want = (a + (b - a) * t).round();
            assert!((f64::from(out[i * 4]) - want).abs() <= 1.0, "{x},{y}");
        }
    }
    let _ = run16(w, h, &widen(&src), &params).unwrap();
}

// ---- The plan ----------------------------------------------------------------------

/// `scripts/develop-filter-admission/measure.py`'s MASK request: a radial with
/// Exposure and Clarity, a 50-dab brush with feather 50 and Dehaze, and a
/// shadow luminance range with Shadows, over no global control.
fn measured_masks() -> Value {
    let dabs: Vec<Value> = (0..50)
        .map(|k| {
            json!({"center": [0.2 + 0.012 * f64::from(k), 0.7], "radius": 0.04,
                "hardness": 0.6, "opacity": 1})
        })
        .collect();
    json!({"range_profile": srgb_profile(), "masks": [
        {"shape": {"op": "radial", "center": [0.4, 0.45], "radius": [0.25, 0.3],
            "hardness": 0.4}, "exposure": 0.6, "clarity": 40},
        {"shape": {"op": "empty"}, "brush": [{"mode": "add", "dabs": dabs}], "feather": 50,
            "dehaze": 40},
        {"shape": {"op": "full"}, "range": {"luminance": {"low": 0, "high": 0.35,
            "low_falloff": 0.1, "high_falloff": 0.1, "invert": false}}, "shadows": 50}
    ]})
}

/// `scripts/develop-filter-admission/measure.py`'s MASKD request: one radial
/// over the whole frame with Clarity 50, Sharpness 60 and Noise Reduction 60.
fn measured_detail() -> Value {
    json!({"masks": [{"shape": {"op": "radial", "center": [0.5, 0.5], "radius": [1.0, 1.0],
        "hardness": 0.5}, "clarity": 50, "sharpness": 60, "noise_reduction": 60}]})
}

/// The MASK kernel plan at 6000×4000 (n = 24,000,000), the brush local's,
/// the largest: `acc` (16n) + the coverage (`LOCAL_BYTES_PER_PIXEL` 8n) + the
/// feather's blur (`LocalSettings` at 50/100 · 0.04 · 6000 = 120 px:
/// `plane_blur::float_plan` of the padded plane, [`FEATHER_120_PLAN`]) + the
/// Dehaze `DevelopFilter` plan (derived below from the unmasked filter's) + two
/// bookkeeping MiB: 2,345,721,452.
const KERNEL_24MP_MASKS: u64 = 2_345_721_452;
const FEATHER_120_PLAN: u64 = 857_411_692;
/// `filter_op_scratch_bytes`: (max(kernel, 20n) + 4 MiB), plus a quarter.
const PLAN_24MP_MASKS: u64 = 2_937_394_695;
/// `selected_edit_plan`: 6 MiB + 8n + max(kernel, 24n) + 2n.
const PLAN_24MP_MASKS_WIDE: u64 = 2_592_012_908;
/// The MASKD kernel plan at 6000×4000: `acc` (16n) + the coverage (8n) +
/// feather 0 (nothing) + the larger of the Clarity `DevelopFilter` plan and the
/// detail stage beside the n×16 value plane it reads, + two bookkeeping MiB.
/// The detail stage (`LocalDetail::plan`) is the larger of Reduce Noise's
/// plan (below 20n) and Smart Sharpen's beside the n×16 denoised plane, + two
/// bookkeeping MiB. Smart Sharpen's (amount 90, radius 1, Gaussian, noise 0)
/// is derived below from the unmasked filter's: [`SHARPEN_24MP`]. So the
/// kernel is 16n + 8n + 16n + 16n + SHARPEN_24MP + 4 MiB = 1,833,060,664.
const KERNEL_24MP_DETAIL: u64 = 1_833_060_664;
const SHARPEN_24MP: u64 = 484_866_360;
const PLAN_24MP_DETAIL: u64 = 2_296_568_710;
const PLAN_24MP_DETAIL_WIDE: u64 = 2_079_352_120;

#[test]
fn the_24_megapixel_masked_plans_are_pinned() {
    use doctored_core::ffi_filters::dr_image_filter_op_scratch_bytes;
    let (w, h) = (6000_u32, 4000_u32);
    let bytes = vec![0_u8; (w * h * 4) as usize];
    let img = unsafe { dr_image_from_rgba(bytes.as_ptr(), w, h) };
    drop(bytes);
    let plan = |params: &Value| {
        let name = CString::new("develop_filter").unwrap();
        let text = CString::new(params.to_string()).unwrap();
        let mut err: *mut c_char = ptr::null_mut();
        let planned = unsafe {
            dr_image_filter_op_scratch_bytes(img, name.as_ptr(), text.as_ptr(), &mut err)
        };
        assert_eq!(message(err), "", "{params}");
        planned
    };
    let masked = plan(&measured_masks());
    let dehaze = plan(&json!({"dehaze": 40}));
    let detailed = plan(&measured_detail());
    let clarity = plan(&json!({"clarity": 50}));
    let sharpen = {
        let name = CString::new("smart_sharpen").unwrap();
        let text = CString::new(
            json!({"amount": 90, "radius": 1, "noise": 0, "remove": "gaussian"}).to_string(),
        )
        .unwrap();
        let mut err: *mut c_char = ptr::null_mut();
        let planned = unsafe {
            dr_image_filter_op_scratch_bytes(img, name.as_ptr(), text.as_ptr(), &mut err)
        };
        assert_eq!(message(err), "");
        planned
    };
    unsafe { dr_image_free(img) };
    let (n, mib) = (24_000_000_u64, 1024 * 1024_u64);
    // The unmasked Dehaze plan is (K + 4 MiB) · 1.25 with K above 20n.
    let dehaze_kernel = dehaze * 4 / 5 - 4 * mib;
    assert!(dehaze_kernel > 20 * n);
    assert_eq!(
        16 * n + 8 * n + FEATHER_120_PLAN + dehaze_kernel + 2 * mib,
        KERNEL_24MP_MASKS,
        "the derivation"
    );
    let base = KERNEL_24MP_MASKS + 4 * mib;
    assert_eq!(base + base.div_ceil(4), PLAN_24MP_MASKS, "the derivation");
    assert_eq!(masked, PLAN_24MP_MASKS);
    let doc = doc16(w, h, vec![[0_u16; 4]; (w * h) as usize]);
    let wide = doc.selected_edit_plan(&edit(&measured_masks())).unwrap()["scratch_bytes"]
        .as_u64()
        .unwrap();
    assert_eq!(
        6 * mib + 8 * n + KERNEL_24MP_MASKS + 2 * n,
        PLAN_24MP_MASKS_WIDE,
        "the derivation"
    );
    assert_eq!(wide, PLAN_24MP_MASKS_WIDE);

    // MASKD: the unmasked Smart Sharpen and Clarity plans are each (K +
    // 4 MiB) · 1.25 with K above 20n.
    let sharpen_kernel = sharpen * 4 / 5 - 4 * mib;
    assert!(sharpen_kernel > 20 * n);
    assert_eq!(sharpen_kernel, SHARPEN_24MP, "the derivation");
    let clarity_kernel = clarity * 4 / 5 - 4 * mib;
    assert!(clarity_kernel > 20 * n);
    let detail = 16 * n + SHARPEN_24MP + 2 * mib;
    assert!(
        clarity_kernel < 16 * n + detail,
        "the detail stage is the larger"
    );
    assert_eq!(
        16 * n + 8 * n + 16 * n + detail + 2 * mib,
        KERNEL_24MP_DETAIL,
        "the derivation"
    );
    let base = KERNEL_24MP_DETAIL + 4 * mib;
    assert_eq!(base + base.div_ceil(4), PLAN_24MP_DETAIL, "the derivation");
    assert_eq!(detailed, PLAN_24MP_DETAIL);
    assert_eq!(
        6 * mib + 8 * n + KERNEL_24MP_DETAIL + 2 * n,
        PLAN_24MP_DETAIL_WIDE,
        "the derivation"
    );
    let wide = doc.selected_edit_plan(&edit(&measured_detail())).unwrap()["scratch_bytes"]
        .as_u64()
        .unwrap();
    assert_eq!(wide, PLAN_24MP_DETAIL_WIDE);
}
