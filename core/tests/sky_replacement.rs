//! Edit ▸ Sky Replacement (M2a) through the FFI: `dr_doc_sky_replace`,
//! `dr_precision_doc_sky_replace`, the preview `dr_image_sky_replace` and the
//! plan `dr_sky_replace_scratch_bytes`, at 8 and 16 bits.
//!
//! # The oracles
//!
//! Every closed form is written here. The scenes use a UNIFORM sky and a
//! BINARY coverage (old sky above the horizon row, foreground below), so the
//! placed sky is the sky colour `S` everywhere, the weighted means are plain
//! colours and each control has a closed form: the cast shift `d = (S −
//! Y(S)) − (O − Y(O))` with Rec.709 luma, `F1 = F + c/100·0.5·d` clamped to
//! 0…1, `F2 = F1 +
//! l/100·(B(F1, S) − F1)` with Multiply `ab` or Screen `a + b − ab`, the
//! band's sky mix `F3 = F2 + e/100·b·(S − F2)`, and the composite. Brightness
//! is the linear-light gain `2^(b/50)` through the sRGB curve; Temperature is
//! checked against Filter ▸ Develop Filter's own export, which the sky
//! adjustment is defined to be. Shift and Fade are checked against
//! `dr_selection_u16`'s `refine`, sample for sample. The placement is checked
//! against the bilinear read of a linear ramp, which is the ramp itself.
//!
//! # Bounds, declared before the first run
//!
//! * ±1 code at 8 and 16 bits for every closed form (one rounding each side);
//!   Temperature at 16 bits is compared with the 8-bit filter at ±1 byte,
//!   the oracle's own resolution.
//! * Shift/Fade: exact codes against `refine`.
//! * A quarter-size sky frames the same within ±2 codes on a smooth sky.
#![allow(clippy::needless_range_loop)]

use std::ffi::CString;
use std::ptr;

use doctored_core::doc::{DrDocument, LayerKind};
use doctored_core::ffi::{dr_image_free, dr_image_from_rgba};
use doctored_core::ffi_cmyk::dr_precision_doc_convert_to_cmyk;
use doctored_core::ffi_doc::{dr_doc_free, dr_doc_layer_canvas_image};
use doctored_core::ffi_filters::dr_image_filter_op;
use doctored_core::ffi_jobs::{dr_job_control_cancel, dr_job_control_create, dr_job_control_free};
use doctored_core::ffi_precision::dr_precision_doc_free;
use doctored_core::ffi_sky::{
    dr_doc_sky_replace, dr_image_sky_replace, dr_precision_doc_sky_replace,
    dr_sky_replace_scratch_bytes,
};
use doctored_core::icc::IccProfile;
use doctored_core::layer_insert::InsertPosition;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer};
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use doctored_core::DrImage;
use image::{Rgba, RgbaImage};
use serde_json::{json, Value};

use crate::cmyk_fixture::press_profile;
use crate::common::{img_pixels, layer_bytes, take_err_string};
use crate::float_fixture::float_doc;
use crate::native_scalar::selection;

pub(crate) const LIMIT: u64 = REFERENCE_WORKING_BYTES;
pub(crate) const W: u32 = 64;
pub(crate) const H: u32 = 48;
/// The horizon row: coverage 65535 above it, 0 from it down.
pub(crate) const HORIZON: u32 = 24;
pub(crate) const SKY: [u8; 4] = [40, 90, 200, 255];
pub(crate) const OLD: [f64; 3] = [0.75, 0.8, 0.85];
pub(crate) const FORE: [f64; 3] = [0.3, 0.45, 0.25];

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Depth {
    Byte,
    Sixteen,
}

impl Depth {
    pub(crate) fn max(self) -> f64 {
        match self {
            Depth::Byte => 255.0,
            Depth::Sixteen => 65535.0,
        }
    }
}

pub(crate) enum Doc {
    Byte(DrDocument),
    Typed(PrecisionDocument),
}

// ---------------------------------------------------------------- builders --

pub(crate) fn byte_image(w: u32, h: u32, pixels: &[[f64; 4]]) -> RgbaImage {
    RgbaImage::from_fn(w, h, |x, y| {
        Rgba(pixels[(y * w + x) as usize].map(|v| (v * 255.0).round() as u8))
    })
}

pub(crate) fn typed_layer(
    name: &str,
    w: u32,
    h: u32,
    pixels: &[[f64; 4]],
    offset: (i32, i32),
) -> PrecisionDocumentLayer {
    let codes = pixels
        .iter()
        .map(|p| p.map(|v| (v * 65535.0).round() as u16))
        .collect();
    let image = PrecisionImage::from_rgba16(w, h, codes, IccProfile::srgb(), None).unwrap();
    PrecisionDocumentLayer::raster(name, image, offset, 1.0, true).unwrap()
}

/// A document of `layers` canvas-sized straight unit pictures, bottom first.
pub(crate) fn build(depth: Depth, w: u32, h: u32, layers: &[Vec<[f64; 4]>]) -> Doc {
    match depth {
        Depth::Byte => {
            let mut doc = DrDocument::from_pixels(byte_image(w, h, &layers[0]));
            for (k, layer) in layers.iter().enumerate().skip(1) {
                doc = doc
                    .adding_image_layer(k - 1, byte_image(w, h, layer), &format!("L{k}"))
                    .unwrap();
            }
            Doc::Byte(doc)
        }
        Depth::Sixteen => {
            let records = layers
                .iter()
                .enumerate()
                .map(|(k, p)| typed_layer(&format!("L{k}"), w, h, p, (0, 0)))
                .collect();
            Doc::Typed(
                PrecisionDocument::from_parts(
                    w,
                    h,
                    IccProfile::srgb(),
                    Metadata::default(),
                    Resolution::default(),
                    records,
                )
                .unwrap(),
            )
        }
    }
}

pub(crate) fn sky_handle(sky: &RgbaImage) -> *mut DrImage {
    let handle = unsafe { dr_image_from_rgba(sky.as_raw().as_ptr(), sky.width(), sky.height()) };
    assert!(!handle.is_null());
    handle
}

pub(crate) fn params(v: &Value) -> CString {
    CString::new(v.to_string()).unwrap()
}

pub(crate) fn replace_with(
    doc: &Doc,
    layer: usize,
    sky: &RgbaImage,
    coverage: &[u16],
    p: &Value,
    limit: u64,
    control: *const doctored_core::job_control::DrJobControl,
) -> Result<Doc, String> {
    let sky = sky_handle(sky);
    let text = params(p);
    let mut err = ptr::null_mut();
    let out = match doc {
        Doc::Byte(d) => {
            let handle = Box::into_raw(Box::new(d.clone()));
            let out = unsafe {
                dr_doc_sky_replace(
                    handle,
                    layer,
                    sky,
                    coverage.as_ptr(),
                    coverage.len(),
                    text.as_ptr(),
                    limit,
                    0,
                    control,
                    &mut err,
                )
            };
            unsafe { dr_doc_free(handle) };
            (!out.is_null()).then(|| Doc::Byte(*unsafe { Box::from_raw(out) }))
        }
        Doc::Typed(d) => {
            let out = unsafe {
                dr_precision_doc_sky_replace(
                    d,
                    layer,
                    sky,
                    coverage.as_ptr(),
                    coverage.len(),
                    text.as_ptr(),
                    limit,
                    0,
                    control,
                    &mut err,
                )
            };
            (!out.is_null()).then(|| Doc::Typed(*unsafe { Box::from_raw(out) }))
        }
    };
    unsafe { dr_image_free(sky) };
    match out {
        Some(doc) => {
            assert!(err.is_null());
            Ok(doc)
        }
        None => Err(take_err_string(err)),
    }
}

pub(crate) fn replace(
    doc: &Doc,
    layer: usize,
    sky: &RgbaImage,
    coverage: &[u16],
    p: &Value,
) -> Result<Doc, String> {
    replace_with(doc, layer, sky, coverage, p, LIMIT, ptr::null())
}

/// Layer `idx`'s samples as unit floats (each canvas-sized at (0, 0)).
fn unit_layer(doc: &Doc, idx: usize) -> Vec<[f64; 4]> {
    match doc {
        Doc::Byte(d) => layer_bytes(d, idx)
            .chunks(4)
            .map(|p| [0, 1, 2, 3].map(|c| f64::from(p[c]) / 255.0))
            .collect(),
        Doc::Typed(d) => d
            .layer(idx)
            .unwrap()
            .source()
            .unwrap()
            .to_rgba16()
            .unwrap()
            .iter()
            .map(|p| p.map(|v| f64::from(v) / 65535.0))
            .collect(),
    }
}

pub(crate) fn layer_count(doc: &Doc) -> usize {
    match doc {
        Doc::Byte(d) => d.layers.len(),
        Doc::Typed(d) => d.layer_count(),
    }
}

pub(crate) fn two_band(w: u32, h: u32, top: [f64; 3], bottom: [f64; 3]) -> Vec<[f64; 4]> {
    (0..w * h)
        .map(|i| {
            let c = if i / w < HORIZON { top } else { bottom };
            [c[0], c[1], c[2], 1.0]
        })
        .collect()
}

pub(crate) fn binary_coverage(w: u32, h: u32) -> Vec<u16> {
    (0..w * h)
        .map(|i| if i / w < HORIZON { 65535 } else { 0 })
        .collect()
}

pub(crate) fn uniform_sky(c: [u8; 4]) -> RgbaImage {
    RgbaImage::from_pixel(32, 16, Rgba(c))
}

/// The value a depth stores for a unit value, back in unit terms.
fn stored(depth: Depth, v: f64) -> f64 {
    (v.clamp(0.0, 1.0) * depth.max()).round() / depth.max()
}

fn luma(c: [f64; 3]) -> f64 {
    0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2]
}

fn assert_within(depth: Depth, got: f64, want: f64, codes: f64, what: &str) {
    let diff = (got - want).abs() * depth.max();
    assert!(
        diff <= codes + 1e-6,
        "{what} at {depth:?}: got {got}, want {want} ({diff} codes)"
    );
}

/// The foreground's closed form (module doc) for the stored foreground `f`,
/// stored old sky `o` and sky `s`.
fn harmonized(f: [f64; 3], o: [f64; 3], s: [f64; 3], p: &Value) -> [f64; 3] {
    // Each slider at its byte: the New Layers output carries both as
    // layer opacities (#2, `sky_harmonize`'s step 6).
    let lighting = crate::common::byte_unit(p["lighting"].as_f64().unwrap() / 100.0);
    let color = crate::common::byte_unit(p["color_adjustment"].as_f64().unwrap() / 100.0);
    let screen = p["lighting_mode"] == "screen";
    let (ys, yo) = (luma(s), luma(o));
    let mut out = [0.0; 3];
    for c in 0..3 {
        let d = (s[c] - ys) - (o[c] - yo);
        let f1 = (f[c] + color * 0.5 * d).clamp(0.0, 1.0);
        let b = if screen {
            f1 + s[c] - f1 * s[c]
        } else {
            f1 * s[c]
        };
        out[c] = (f1 + lighting * (b - f1)).clamp(0.0, 1.0);
    }
    out
}

fn base(p: Value) -> Value {
    let mut v = json!({"fade_edge": 0, "shift_edge": 0});
    for (k, value) in p.as_object().unwrap() {
        v[k] = value.clone();
    }
    v
}

// --------------------------------------------------------- closed forms --

#[test]
fn the_foreground_harmonization_matches_its_closed_form() {
    let sky = uniform_sky(SKY);
    let s = [SKY[0], SKY[1], SKY[2]].map(|v| f64::from(v) / 255.0);
    let cases = [
        json!({"lighting": 0, "color_adjustment": 0, "lighting_mode": "multiply"}),
        json!({"lighting": 50, "color_adjustment": 50, "lighting_mode": "multiply"}),
        json!({"lighting": 100, "color_adjustment": 0, "lighting_mode": "screen"}),
        json!({"lighting": 0, "color_adjustment": 100, "lighting_mode": "multiply"}),
        json!({"lighting": 75, "color_adjustment": 25, "lighting_mode": "screen"}),
    ];
    for depth in [Depth::Byte, Depth::Sixteen] {
        let doc = build(depth, W, H, &[two_band(W, H, OLD, FORE)]);
        let o = OLD.map(|v| stored(depth, v));
        let f = FORE.map(|v| stored(depth, v));
        for case in &cases {
            let p = base(case.clone());
            let out = replace(&doc, 0, &sky, &binary_coverage(W, H), &p).unwrap();
            let pixels = unit_layer(&out, 1);
            let want_fore = harmonized(f, o, s, &p);
            for (i, px) in pixels.iter().enumerate() {
                let want = if (i as u32) / W < HORIZON {
                    s
                } else {
                    want_fore
                };
                for c in 0..3 {
                    assert_within(depth, px[c], want[c], 1.0, &format!("{case} channel {c}"));
                }
                assert_within(depth, px[3], 1.0, 0.0, "alpha");
            }
        }
    }
}

fn srgb_decode(v: f64) -> f64 {
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}

fn srgb_encode(v: f64) -> f64 {
    if v <= 0.0031308 {
        v * 12.92
    } else {
        1.055 * v.powf(1.0 / 2.4) - 0.055
    }
}

#[test]
fn brightness_is_a_linear_light_gain_of_the_sky() {
    let sky = uniform_sky(SKY);
    let p = base(json!({"brightness": -50, "lighting": 0, "color_adjustment": 0}));
    for depth in [Depth::Byte, Depth::Sixteen] {
        let doc = build(depth, W, H, &[two_band(W, H, OLD, FORE)]);
        let out = replace(&doc, 0, &sky, &binary_coverage(W, H), &p).unwrap();
        let px = unit_layer(&out, 1)[0];
        for c in 0..3 {
            let want = srgb_encode(srgb_decode(f64::from(SKY[c]) / 255.0) * 0.5);
            assert_within(depth, px[c], want, 1.0, "brightness -50");
        }
    }
}

#[test]
fn temperature_is_develop_filter_filters_temperature() {
    let sky = uniform_sky(SKY);
    let handle = sky_handle(&sky);
    let filter_params = CString::new(json!({"temperature": 40}).to_string()).unwrap();
    let mut err = ptr::null_mut();
    let filtered = unsafe {
        dr_image_filter_op(
            handle,
            c"develop_filter".as_ptr(),
            filter_params.as_ptr(),
            ptr::null(),
            &mut err,
        )
    };
    assert!(!filtered.is_null(), "{}", take_err_string(err));
    let want: Vec<f64> = img_pixels(filtered)[..3]
        .iter()
        .map(|&v| f64::from(v) / 255.0)
        .collect();
    unsafe {
        dr_image_free(filtered);
        dr_image_free(handle);
    }
    let p = base(json!({"temperature": 40, "lighting": 0, "color_adjustment": 0}));
    for depth in [Depth::Byte, Depth::Sixteen] {
        let doc = build(depth, W, H, &[two_band(W, H, OLD, FORE)]);
        let out = replace(&doc, 0, &sky, &binary_coverage(W, H), &p).unwrap();
        let px = unit_layer(&out, 1)[0];
        for c in 0..3 {
            let diff = (px[c] - want[c]).abs() * 255.0;
            assert!(diff <= 1.0 + 1e-6, "temperature at {depth:?}: {diff} bytes");
        }
    }
}

#[test]
fn edge_lighting_mixes_the_sky_into_the_band_inside_the_horizon() {
    let sky = uniform_sky(SKY);
    let s = [SKY[0], SKY[1], SKY[2]].map(|v| f64::from(v) / 255.0);
    let coverage = binary_coverage(W, H);
    // R_e = clamp(64/32, 2, 64) = 2 canvas pixels on a 64-pixel canvas.
    let e = selection(W, H, json!({"kind": "refine", "feather": 2.0}), &coverage);
    let p = base(json!({"edge_lighting": 100, "lighting": 0, "color_adjustment": 0}));
    for depth in [Depth::Byte, Depth::Sixteen] {
        let doc = build(depth, W, H, &[two_band(W, H, OLD, FORE)]);
        let f = FORE.map(|v| stored(depth, v));
        let out = replace(&doc, 0, &sky, &coverage, &p).unwrap();
        let pixels = unit_layer(&out, 1);
        let mut banded = 0;
        for (i, px) in pixels.iter().enumerate() {
            if (i as u32) / W < HORIZON {
                continue;
            }
            let b = f64::from(e[i]) / 65535.0;
            banded += usize::from(b > 0.0);
            for c in 0..3 {
                let want = f[c] + b * (s[c] - f[c]);
                assert_within(depth, px[c], want, 1.0, "edge lighting");
            }
        }
        assert!(banded > 0, "the band reaches below the horizon");
    }
}

#[test]
fn the_placement_is_the_bilinear_read_of_the_sky_at_canvas_fractions() {
    // A horizontal ramp, code = column: its bilinear read at `u` is
    // clamp(u − 0.5, 0, 255), the ramp itself.
    let sky = RgbaImage::from_fn(256, 16, |x, _| Rgba([x as u8, 0, 0, 255]));
    let coverage = vec![65535u16; (W * H) as usize];
    for flip in [false, true] {
        let p = json!({"fade_edge": 0, "scale": 200, "offset": [0.1, 0.2], "flip": flip,
                       "lighting": 0, "color_adjustment": 0});
        for depth in [Depth::Byte, Depth::Sixteen] {
            let doc = build(depth, W, H, &[two_band(W, H, OLD, FORE)]);
            let out = replace(&doc, 0, &sky, &coverage, &p).unwrap();
            let pixels = unit_layer(&out, 1);
            let s = (f64::from(W) / 256.0).max(f64::from(H) / 16.0) * 2.0;
            for x in 0..W {
                let mut u =
                    (f64::from(x) + 0.5 - f64::from(W) / 2.0 - 0.1 * f64::from(W)) / s + 128.0;
                if flip {
                    u = 256.0 - u;
                }
                let want = (u - 0.5).clamp(0.0, 255.0) / 255.0;
                for y in [0, H / 2, H - 1] {
                    let px = pixels[(y * W + x) as usize];
                    assert_within(depth, px[0], want, 1.0, &format!("ramp x={x} flip={flip}"));
                }
            }
        }
    }
}

#[test]
fn shift_and_fade_are_refine_sample_for_sample() {
    // 512 px canvas: R = clamp(512/64, 2, 64) = 8 px.
    let (w, h) = (512u32, 256u32);
    let coverage: Vec<u16> = (0..w * h)
        .map(|i| {
            let (x, y) = (i % w, i / w);
            if x < 300 && y < 120 {
                65535
            } else if x < 310 && y < 130 {
                30000
            } else {
                0
            }
        })
        .collect();
    // Transparent except one opaque pixel inside the sky, so the output alpha
    // is the refined mask everywhere else.
    let mut picture = vec![[0.5, 0.5, 0.5, 0.0]; (w * h) as usize];
    picture[0][3] = 1.0;
    let sky = uniform_sky(SKY);
    for (shift, fade) in [(50.0, 50.0), (-50.0, 25.0), (0.0, 100.0)] {
        let want = selection(
            w,
            h,
            json!({"kind": "refine", "shift": shift / 100.0 * 8.0, "feather": fade / 100.0 * 8.0}),
            &coverage,
        );
        let p = json!({"shift_edge": shift, "fade_edge": fade});
        for depth in [Depth::Byte, Depth::Sixteen] {
            let doc = build(depth, w, h, &[picture.clone()]);
            let out = replace(&doc, 0, &sky, &coverage, &p).unwrap();
            let pixels = unit_layer(&out, 1);
            for i in 1..pixels.len() {
                // The kernel's own single quantization, in f32.
                let m = f32::from(want[i]) / 65535.0;
                let code = f64::from((m * depth.max() as f32).round());
                assert_eq!(
                    (pixels[i][3] * depth.max()).round(),
                    code,
                    "alpha at {i} for shift {shift} fade {fade} ({depth:?})"
                );
            }
        }
    }
}

// ------------------------------------------------------------ the preview --

pub(crate) fn preview(
    foreground: &RgbaImage,
    sky: &RgbaImage,
    coverage: &[u16],
    grid: (u32, u32),
    p: &Value,
) -> Result<Vec<u8>, String> {
    let fg = sky_handle(foreground);
    let sky = sky_handle(sky);
    let text = params(p);
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_image_sky_replace(
            fg,
            sky,
            coverage.as_ptr(),
            coverage.len(),
            grid.0,
            grid.1,
            text.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    unsafe {
        dr_image_free(fg);
        dr_image_free(sky);
    }
    if out.is_null() {
        return Err(take_err_string(err));
    }
    let bytes = img_pixels(out);
    unsafe { dr_image_free(out) };
    Ok(bytes)
}

#[test]
fn the_byte_doc_op_is_the_image_op_on_the_canvas_image() {
    let (w, h) = (80u32, 60u32);
    let plasma: Vec<[f64; 4]> = (0..w * h)
        .map(|i| {
            let p = crate::common::plasma(i % w, i / w).map(|v| f64::from(v) / 255.0);
            [p[0], p[1], p[2], 1.0]
        })
        .collect();
    let mut doc = DrDocument::from_pixels(byte_image(w, h, &plasma));
    // A smaller second layer at an offset, partly off the canvas.
    let small = RgbaImage::from_fn(50, 40, |x, y| Rgba([(x * 5) as u8, (y * 6) as u8, 90, 255]));
    doc = doc.adding_image_layer(0, small, "Small").unwrap();
    doc.layers[1].offset = (40, -10);
    let coverage: Vec<u16> = (0..w * h)
        .map(|i| {
            if i / w < 20 {
                65535
            } else {
                (i % 7) as u16 * 900
            }
        })
        .collect();
    let sky = RgbaImage::from_fn(120, 50, |x, y| {
        Rgba([(x * 2) as u8, (y * 5) as u8, 200, 255])
    });
    let p = json!({"shift_edge": 20, "fade_edge": 40, "brightness": 10, "temperature": -15,
                   "scale": 130, "flip": true, "offset": [0.05, -0.1], "lighting_mode": "screen",
                   "lighting": 30, "color_adjustment": 70, "edge_lighting": 40, "name": "Sky"});
    let out = replace(&Doc::Byte(doc.clone()), 1, &sky, &coverage, &p).unwrap();
    let Doc::Byte(out) = out else { unreachable!() };
    let handle = Box::into_raw(Box::new(doc));
    let canvas = unsafe { dr_doc_layer_canvas_image(handle, 1) };
    assert!(!canvas.is_null());
    let canvas_bytes = RgbaImage::from_raw(w, h, img_pixels(canvas)).unwrap();
    unsafe {
        dr_image_free(canvas);
        dr_doc_free(handle);
    }
    let previewed = preview(&canvas_bytes, &sky, &coverage, (w, h), &p).unwrap();
    assert_eq!(layer_bytes(&out, 2), previewed, "byte for byte");
}

#[test]
fn a_reduced_preview_takes_the_canvas_edge_widths() {
    // An 8192-wide coverage: R = 64 canvas px; on a 1024-wide foreground that
    // is 64·1024/8192 = 8 px, not the 16 the foreground's own size gives.
    let (cw, ch) = (8192u32, 64u32);
    let coverage: Vec<u16> = (0..cw * ch)
        .map(|i| if i % cw < 4096 { 65535 } else { 0 })
        .collect();
    let (w, h) = (1024u32, 8u32);
    // The bilinear read at each centre: column x reads at 8x + 4, so the step
    // stays a step at 512.
    let read: Vec<u16> = (0..w * h)
        .map(|i| if i % w < 512 { 65535 } else { 0 })
        .collect();
    let mut fg = RgbaImage::new(w, h);
    fg.put_pixel(0, 0, Rgba([10, 10, 10, 255]));
    let sky = uniform_sky(SKY);
    let p = json!({"fade_edge": 100});
    let got = preview(&fg, &sky, &coverage, (cw, ch), &p).unwrap();
    let at8 = selection(w, h, json!({"kind": "refine", "feather": 8.0}), &read);
    let at16 = selection(w, h, json!({"kind": "refine", "feather": 16.0}), &read);
    assert_ne!(at8, at16);
    for i in 1..(w * h) as usize {
        let want = (f32::from(at8[i]) / 65535.0 * 255.0).round() as u8;
        assert_eq!(got[i * 4 + 3], want, "alpha at {i}");
    }
}

#[test]
fn a_quarter_size_sky_frames_the_same() {
    let sky = RgbaImage::from_fn(1024, 512, |x, y| {
        Rgba([
            (x * 255 / 1023) as u8,
            (y * 255 / 511) as u8,
            ((x + y) * 255 / 1535) as u8,
            255,
        ])
    });
    let quarter = image::imageops::resize(&sky, 256, 128, image::imageops::FilterType::Triangle);
    let (w, h) = (96u32, 64u32);
    let fg = RgbaImage::from_pixel(w, h, Rgba([100, 120, 140, 255]));
    let coverage = vec![65535u16; (w * h) as usize];
    let p = json!({"fade_edge": 0, "scale": 150, "offset": [0.1, -0.05], "flip": true,
                   "lighting": 0, "color_adjustment": 0});
    let full = preview(&fg, &sky, &coverage, (w, h), &p).unwrap();
    let reduced = preview(&fg, &quarter, &coverage, (w, h), &p).unwrap();
    for (i, (a, b)) in full.iter().zip(&reduced).enumerate() {
        assert!(a.abs_diff(*b) <= 2, "byte {i}: {a} vs {b}");
    }
}

// -------------------------------------------------------------- structure --

#[test]
fn the_new_layer_is_one_canvas_layer_directly_above_the_source() {
    let sky = uniform_sky(SKY);
    let p = json!({"name": "New Sky"});
    for depth in [Depth::Byte, Depth::Sixteen] {
        let doc = build(
            depth,
            W,
            H,
            &[two_band(W, H, OLD, FORE), two_band(W, H, FORE, OLD)],
        );
        let before = unit_layer(&doc, 0);
        let out = replace(&doc, 0, &sky, &binary_coverage(W, H), &p).unwrap();
        assert_eq!(layer_count(&out), 3);
        assert_eq!(unit_layer(&out, 0), before, "the source is unchanged");
        match &out {
            Doc::Byte(d) => {
                let l = &d.layers[1];
                assert_eq!(l.name, "New Sky");
                assert_eq!((l.opacity, l.offset, l.visible), (1.0, (0, 0), true));
                assert_eq!(l.pixels.dimensions(), (W, H));
                assert_eq!(l.blend, doctored_core::doc::BlendMode::Normal);
                assert_eq!(d.layers[2].name, "L1");
            }
            Doc::Typed(d) => {
                let l = d.layer(1).unwrap();
                assert_eq!(l.name(), "New Sky");
                assert_eq!((l.opacity(), l.offset(), l.visible()), (1.0, (0, 0), true));
                assert_eq!(l.source().unwrap().dimensions(), (W, H));
                assert_eq!(d.layer(2).unwrap().name(), "L1");
            }
        }
    }
    // The default name.
    let doc = build(Depth::Byte, W, H, &[two_band(W, H, OLD, FORE)]);
    let Doc::Byte(out) = replace(&doc, 0, &sky, &binary_coverage(W, H), &json!({})).unwrap() else {
        unreachable!()
    };
    assert_eq!(out.layers[1].name, "Sky Replacement");
}

// --------------------------------------------------------------- refusals --

fn refused(doc: &Doc, layer: usize, coverage: &[u16], p: &Value) -> String {
    replace(doc, layer, &uniform_sky(SKY), coverage, p)
        .err()
        .expect("a refusal")
}

#[test]
fn refusals_name_their_reason() {
    let cov = binary_coverage(W, H);
    for depth in [Depth::Byte, Depth::Sixteen] {
        let doc = build(
            depth,
            W,
            H,
            &[two_band(W, H, OLD, FORE), two_band(W, H, FORE, OLD)],
        );
        assert!(refused(&doc, 0, &cov, &json!({"foo": 1})).contains("foo"));
        assert!(refused(&doc, 0, &cov, &json!({"scale": 10})).contains("scale"));
        assert!(
            refused(&doc, 0, &cov, &json!({"lighting_mode": "overlay"})).contains("lighting_mode")
        );
        assert!(refused(&doc, 0, &cov, &json!({"offset": [2, 0]})).contains("offset"));
        assert_eq!(
            refused(&doc, 5, &cov, &json!({})),
            "sky replacement layer 5 is out of range"
        );
        assert_eq!(
            refused(&doc, 0, &cov[1..], &json!({})),
            "invalid sky replacement coverage buffer"
        );
        assert_eq!(
            refused(&doc, 0, &vec![0; cov.len()], &json!({})),
            "sky replacement found no sky on layer 0"
        );
        let grouped = match &doc {
            Doc::Byte(d) => {
                let (g, ..) = d.group_layers(&[0], "G").unwrap();
                let at = g
                    .layers
                    .iter()
                    .position(|l| l.kind == LayerKind::Group)
                    .unwrap();
                (Doc::Byte(g), at)
            }
            Doc::Typed(d) => {
                let (g, _) = d.group_layers(&[0], "G", false).unwrap().unwrap();
                let at = (0..g.layer_count())
                    .find(|&i| g.layer(i).unwrap().children().is_some())
                    .unwrap();
                (Doc::Typed(g), at)
            }
        };
        assert_eq!(
            refused(&grouped.0, grouped.1, &cov, &json!({})),
            format!(
                "sky replacement needs a raster layer; layer {} is not one",
                grouped.1
            )
        );
        let adjusted = match &doc {
            Doc::Byte(d) => {
                let mut a = d.clone();
                a.layers[1].meta = Some(r#"{"type":"adjust","op":"invert"}"#.into());
                Doc::Byte(a)
            }
            Doc::Typed(d) => {
                let description = json!({"type":"adjust","evaluation":"precision_v1",
                                         "op":"exposure","params":{"exposure":0.25}});
                let a = d
                    .add_adjustment_at(
                        "Adj",
                        &description.to_string(),
                        1.0,
                        1,
                        InsertPosition::Top,
                        LIMIT,
                        0,
                        None,
                    )
                    .unwrap()
                    .unwrap();
                Doc::Typed(a)
            }
        };
        let adjustment = layer_count(&adjusted) - 1;
        assert_eq!(
            refused(&adjusted, adjustment, &cov, &json!({})),
            format!("sky replacement needs a raster layer; layer {adjustment} is not one")
        );
    }
    // 32-bit: refused by name.
    let n = (W * H) as usize;
    let float = Doc::Typed(float_doc(
        W,
        H,
        &IccProfile::srgb(),
        &[vec![[0.5, 0.5, 0.5, 1.0]; n]],
    ));
    assert_eq!(
        refused(&float, 0, &cov, &json!({})),
        "sky replacement is not available in a 32-bit document"
    );
    // CMYK: refused by name.
    let rgb = Box::into_raw(Box::new(
        match build(Depth::Sixteen, W, H, &[two_band(W, H, OLD, FORE)]) {
            Doc::Typed(d) => d,
            Doc::Byte(_) => unreachable!(),
        },
    ));
    let profile = press_profile("Press Sky");
    let mut err = ptr::null_mut();
    let cmyk = unsafe {
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
    assert!(!cmyk.is_null(), "{}", take_err_string(err));
    let sky = sky_handle(&uniform_sky(SKY));
    let text = params(&json!({}));
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_sky_replace(
            cmyk,
            0,
            sky,
            cov.as_ptr(),
            cov.len(),
            text.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(out.is_null());
    assert_eq!(
        take_err_string(err),
        "sky replacement is not available in a CMYK document"
    );
    unsafe {
        dr_image_free(sky);
        dr_precision_doc_free(cmyk);
        dr_precision_doc_free(rgb);
    }
}

#[test]
fn a_cancelled_control_answers_operation_cancelled() {
    for depth in [Depth::Byte, Depth::Sixteen] {
        let doc = build(depth, W, H, &[two_band(W, H, OLD, FORE)]);
        let control = unsafe { dr_job_control_create() };
        unsafe { dr_job_control_cancel(control) };
        let refused = replace_with(
            &doc,
            0,
            &uniform_sky(SKY),
            &binary_coverage(W, H),
            &json!({}),
            LIMIT,
            control,
        );
        unsafe { dr_job_control_free(control) };
        assert_eq!(refused.err().as_deref(), Some("operation cancelled"));
    }
}

#[test]
fn the_plan_is_its_formula_and_is_admitted_against_the_limit() {
    let mib4 = 4 * 1024 * 1024;
    let plan = |bits| unsafe { dr_sky_replace_scratch_bytes(100, 50, 20, 10, bits, 0, false) };
    assert_eq!(plan(8), 5000 * 61 + 200 * 36 + mib4);
    assert_eq!(plan(16), 5000 * 65 + 200 * 36 + mib4);
    assert_eq!(plan(32), 5000 * 73 + 200 * 36 + mib4);
    assert_eq!(
        unsafe { dr_sky_replace_scratch_bytes(0, 50, 20, 10, 8, 0, false) },
        0
    );
    let scratch = unsafe { dr_sky_replace_scratch_bytes(W, H, 32, 16, 8, 0, false) };
    let doc = build(Depth::Byte, W, H, &[two_band(W, H, OLD, FORE)]);
    let refused = replace_with(
        &doc,
        0,
        &uniform_sky(SKY),
        &binary_coverage(W, H),
        &json!({}),
        scratch,
        ptr::null(),
    );
    assert!(refused.err().unwrap().contains("working limit"));
}
