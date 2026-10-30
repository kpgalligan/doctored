//! A crop past the canvas with the Crop tool's Fill — the FFI
//! ops the host composes (`CropFillRun`, app/Sources/CropFill.swift), each
//! against an oracle written here:
//!
//! * the turned straighten with the Background (Default) fill to the take's
//!   window (-10, -6, 84 × 60) at 8° bicubic, Delete Cropped Pixels on;
//! * the new area's coverage: the old canvas's polygon mapped by the crop
//!   (rasterized here by supersampling, as CoreGraphics anti-aliases the
//!   host's `.polygon`), inverted, then grown by the bicubic support (2 px,
//!   `CropFillRun.seamRadius`) through `dr_selection_grow`;
//! * Content-Aware Fill on record 0 over that coverage (8 bits, 16 bits,
//!   float, 8-bit CMYK): every sample outside the coverage is bit-exact on
//!   every layer; every pixel whose bicubic taps reach the pad (computed
//!   here) lies inside the coverage, so no pad-tinted seam is left; the upper
//!   layer's new area is transparent; record 0's new area is opaque and not
//!   uniformly the pad colour;
//! * Generative Expand's frame, input and apply over the same coverage with a
//!   solid answer (8 bits, 16 bits, 8-bit CMYK): every composite sample the
//!   coverage leaves at 0 is bit-exact, and the fully covered new area is the
//!   answer's colour (exact at 8 bits, ±1 code at 16; uniform on CMYK, whose
//!   proof of the answer's ink is the press profile's, not the sRGB colour).

use std::ffi::{c_char, CString};
use std::ptr;

use doctored_core::ffi_cmyk::dr_precision_doc_convert_to_cmyk;
use doctored_core::ffi_cmyk_colour::dr_precision_doc_cmyk_profile_edit;
use doctored_core::ffi_content_aware::dr_doc_content_aware_fill_edit;
use doctored_core::ffi_delete_cropped::dr_doc_delete_cropped_pixels;
use doctored_core::ffi_doc::dr_selection_grow;
use doctored_core::ffi_geometry::dr_doc_geometry_edit;
use doctored_core::ffi_precision_edit::dr_precision_doc_edit;
use doctored_core::icc::IccProfile;
use doctored_core::precision_document::PrecisionDocument;
use serde_json::json;

use crate::cmyk_fixture::press_profile;
use crate::common::take_err_string;
use crate::float_fixture::float_doc;
use crate::generative_fill::{frame, input_with, projection};
use crate::generative_variations::{apply, solid, Picture, COLORS, NATIVE};
use crate::sky_replacement::{build, Depth, Doc, LIMIT};

const W: u32 = 64;
const H: u32 = 48;
/// The take's crop window, `(x, y, width, height)`.
const WINDOW: (i32, i32, u32, u32) = (-10, -6, 84, 60);
const ANGLE: f64 = 8.0;
/// Take1's swatch, #C83228.
const RED: [u8; 3] = [200, 50, 40];
/// `CropFillRun.seamRadius` at bicubic: the kernel's support.
const SEAM: f32 = 2.0;
/// The upper layer's patch, `(x, y, width, height)` in old canvas pixels.
const PATCH: (u32, u32, u32, u32) = (20, 10, 16, 12);
const TAG: &str = r#"{"type":"generated","version":1,"feature":"generative_expand","provider":"openai","model":"gpt-image-2.5-sunburst","prompt":""}"#;

// -------------------------------------------------------------- geometry --

/// The Crop tool's matrix: rotate by `−angle` about the window's centre.
fn rotation() -> [f64; 6] {
    let cx = f64::from(WINDOW.0) + f64::from(WINDOW.2) / 2.0;
    let cy = f64::from(WINDOW.1) + f64::from(WINDOW.3) / 2.0;
    let radians = -ANGLE * std::f64::consts::PI / 180.0;
    let (s, c) = (radians.sin(), radians.cos());
    [c, s, -s, c, cx - c * cx + s * cy, cy - s * cx - c * cy]
}

fn apply_affine(m: [f64; 6], x: f64, y: f64) -> (f64, f64) {
    (m[0] * x + m[2] * y + m[4], m[1] * x + m[3] * y + m[5])
}

fn inverse(m: [f64; 6]) -> [f64; 6] {
    let det = m[0] * m[3] - m[1] * m[2];
    [
        m[3] / det,
        -m[1] / det,
        -m[2] / det,
        m[0] / det,
        (m[2] * m[5] - m[3] * m[4]) / det,
        (m[1] * m[4] - m[0] * m[5]) / det,
    ]
}

/// `CropSession.canvasMap`: old canvas → new canvas.
fn canvas_map(x: f64, y: f64) -> (f64, f64) {
    let (u, v) = apply_affine(rotation(), x, y);
    (u - f64::from(WINDOW.0), v - f64::from(WINDOW.1))
}

/// Whether `(x, y)` lies inside the convex polygon `quad` (either winding).
fn inside(quad: &[(f64, f64); 4], x: f64, y: f64) -> bool {
    let mut sign = 0.0f64;
    for k in 0..4 {
        let (a, b) = (quad[k], quad[(k + 1) % 4]);
        let cross = (b.0 - a.0) * (y - a.1) - (b.1 - a.1) * (x - a.0);
        if cross != 0.0 {
            if sign != 0.0 && sign.signum() != cross.signum() {
                return false;
            }
            sign = cross;
        }
    }
    true
}

/// The host's coverage: the old canvas's polygon mapped into the new canvas,
/// its area coverage (16 × 16 samples a pixel), inverted, grown by `SEAM`.
fn new_area() -> Vec<u8> {
    let (w, h) = (WINDOW.2, WINDOW.3);
    let corners = [
        (0.0, 0.0),
        (f64::from(W), 0.0),
        (f64::from(W), f64::from(H)),
        (0.0, f64::from(H)),
    ];
    let quad = corners.map(|(x, y)| canvas_map(x, y));
    let n = 16;
    let mut mask: Vec<u8> = (0..w * h)
        .map(|i| {
            let (px, py) = (f64::from(i % w), f64::from(i / w));
            let mut hits = 0;
            for sy in 0..n {
                for sx in 0..n {
                    let x = px + (f64::from(sx) + 0.5) / f64::from(n);
                    let y = py + (f64::from(sy) + 0.5) / f64::from(n);
                    if inside(&quad, x, y) {
                        hits += 1;
                    }
                }
            }
            let covered = (255.0 * f64::from(hits) / f64::from(n * n)).round() as u8;
            255 - covered
        })
        .collect();
    assert!(unsafe { dr_selection_grow(mask.as_mut_ptr(), w, h, SEAM) });
    mask
}

/// Catmull-Rom (the Keys cubic at a = -0.5), the geometry walk's bicubic.
fn cubic(t: f64) -> f64 {
    let x = t.abs();
    if x < 1.0 {
        ((1.5 * x - 2.5) * x) * x + 1.0
    } else if x < 2.0 {
        ((-0.5 * x + 2.5) * x - 4.0) * x + 2.0
    } else {
        0.0
    }
}

/// Whether new-canvas pixel `(i, j)`'s bicubic taps (pixel-centre
/// convention, four a side) read a source pixel outside the old canvas with
/// a nonzero weight: a pixel the pad colour reaches.
fn pad_reaches(i: u32, j: u32) -> bool {
    let back = inverse(rotation());
    let (u, v) = apply_affine(
        back,
        f64::from(WINDOW.0) + f64::from(i) + 0.5,
        f64::from(WINDOW.1) + f64::from(j) + 0.5,
    );
    let taps = |s: f64| {
        let p = s - 0.5;
        let base = p.floor();
        (0..4)
            .map(|t| {
                let index = base as i64 - 1 + t;
                (index, cubic(p - (base - 1.0 + t as f64)))
            })
            .collect::<Vec<_>>()
    };
    let (xs, ys) = (taps(u), taps(v));
    ys.iter().any(|&(ty, wy)| {
        xs.iter().any(|&(tx, wx)| {
            wx * wy != 0.0 && (tx < 0 || ty < 0 || tx >= i64::from(W) || ty >= i64::from(H))
        })
    })
}

// ------------------------------------------------------------- documents --

fn unit_gradient() -> Vec<[f64; 4]> {
    (0..W * H)
        .map(|i| {
            let (x, y) = (f64::from(i % W), f64::from(i / W));
            [
                (x * 3.0 + 20.0).round() / 255.0,
                (y * 4.0 + 10.0).round() / 255.0,
                ((x + y) % 11.0 * 17.0 + 30.0).round() / 255.0,
                1.0,
            ]
        })
        .collect()
}

fn unit_patch() -> Vec<[f64; 4]> {
    (0..W * H)
        .map(|i| {
            let (x, y) = (i % W, i / W);
            let on = (PATCH.0..PATCH.0 + PATCH.2).contains(&x)
                && (PATCH.1..PATCH.1 + PATCH.3).contains(&y);
            if on {
                [0.1, 0.8, 0.3, 1.0]
            } else {
                [0.0, 0.0, 0.0, 0.0]
            }
        })
        .collect()
}

fn byte_doc() -> Doc {
    build(Depth::Byte, W, H, &[unit_gradient(), unit_patch()])
}

fn sixteen_doc() -> Doc {
    build(Depth::Sixteen, W, H, &[unit_gradient(), unit_patch()])
}

fn float_document() -> Doc {
    let f = |pixels: Vec<[f64; 4]>| pixels.iter().map(|p| p.map(|v| v as f32)).collect();
    Doc::Typed(float_doc(
        W,
        H,
        &IccProfile::srgb(),
        &[f(unit_gradient()), f(unit_patch())],
    ))
}

fn typed_ptr(doc: &Doc) -> &PrecisionDocument {
    match doc {
        Doc::Typed(d) => d,
        Doc::Byte(_) => panic!("a typed document"),
    }
}

fn cmyk8_doc() -> Doc {
    let rgb = sixteen_doc();
    let profile = press_profile("Press Test");
    let mut err: *mut c_char = ptr::null_mut();
    let cmyk = unsafe {
        dr_precision_doc_convert_to_cmyk(
            typed_ptr(&rgb),
            profile.as_ptr(),
            profile.len(),
            c"{}".as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!cmyk.is_null(), "to CMYK: {}", take_err_string(err));
    let text = CString::new(r#"{"op":"depth","depth":8}"#).unwrap();
    let eight = unsafe {
        dr_precision_doc_cmyk_profile_edit(
            cmyk,
            ptr::null(),
            0,
            text.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!eight.is_null(), "to 8 bits: {}", take_err_string(err));
    drop(unsafe { Box::from_raw(cmyk) });
    Doc::Typed(*unsafe { Box::from_raw(eight) })
}

// ----------------------------------------------------------------- calls --

/// The crop with the Background (Default) fill, Delete Cropped Pixels on —
/// what `CropFillRun.cropped` publishes nothing of, the base of the fill.
fn cropped(doc: &Doc, cmyk: bool) -> Doc {
    let background = if cmyk {
        json!({"ink": [0.1, 0.7, 0.6, 0.05]})
    } else {
        json!({"color": RED.map(|v| f64::from(v) / 255.0), "color_space": "document"})
    };
    let request = json!({"op":"straighten","matrix":rotation(),"filter":"bicubic",
        "x":WINDOW.0,"y":WINDOW.1,"width":WINDOW.2,"height":WINDOW.3,
        "delete_cropped":true,"background":background});
    let text = CString::new(request.to_string()).unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    match doc {
        Doc::Byte(d) => {
            let out =
                unsafe { dr_doc_geometry_edit(d, text.as_ptr(), LIMIT, 0, ptr::null(), &mut err) };
            assert!(!out.is_null(), "{request}: {}", take_err_string(err));
            let out = unsafe { Box::from_raw(out) };
            let trimmed = unsafe { dr_doc_delete_cropped_pixels(&*out) };
            if trimmed.is_null() {
                Doc::Byte(*out)
            } else {
                Doc::Byte(*unsafe { Box::from_raw(trimmed) })
            }
        }
        Doc::Typed(d) => {
            let out = unsafe {
                dr_precision_doc_edit(
                    d,
                    text.as_ptr(),
                    ptr::null(),
                    0,
                    LIMIT,
                    0,
                    ptr::null(),
                    &mut err,
                )
            };
            assert!(!out.is_null(), "{request}: {}", take_err_string(err));
            Doc::Typed(*unsafe { Box::from_raw(out) })
        }
    }
}

/// `ContentAwareFillWork` with the default settings on record 0.
fn content_aware(doc: &Doc, mask: &[u8]) -> Doc {
    let (w, h) = (WINDOW.2, WINDOW.3);
    let (mut x0, mut y0, mut x1, mut y1) = (w, h, 0, 0);
    for (k, &m) in mask.iter().enumerate() {
        if m > 0 {
            let (x, y) = (k as u32 % w, k as u32 / w);
            (x0, y0, x1, y1) = (x0.min(x), y0.min(y), x1.max(x), y1.max(y));
        }
    }
    let request = json!({"op":"content_aware_fill","layer":0,"ring":0,"seed":0,
        "sample":"current","ignore_adjustments":false,"output":"layer",
        "x":x0,"y":y0,"width":x1 - x0 + 1,"height":y1 - y0 + 1});
    let text = CString::new(request.to_string()).unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    match doc {
        Doc::Byte(d) => {
            let out = unsafe {
                dr_doc_content_aware_fill_edit(
                    d,
                    text.as_ptr(),
                    mask.as_ptr(),
                    ptr::null(),
                    &mut err,
                )
            };
            assert!(!out.is_null(), "byte fill: {}", take_err_string(err));
            Doc::Byte(*unsafe { Box::from_raw(out) })
        }
        Doc::Typed(d) => {
            let samples: Vec<u16> = mask.iter().map(|&v| u16::from(v) * 257).collect();
            let out = unsafe {
                dr_precision_doc_edit(
                    d,
                    text.as_ptr(),
                    samples.as_ptr(),
                    samples.len(),
                    LIMIT,
                    0,
                    ptr::null(),
                    &mut err,
                )
            };
            assert!(!out.is_null(), "typed fill: {}", take_err_string(err));
            Doc::Typed(*unsafe { Box::from_raw(out) })
        }
    }
}

// ----------------------------------------------------------------- reads --

/// One stored sample, as raw codes or f32 bits; `opaque` from its own alpha.
#[derive(Clone, Copy, PartialEq, Debug)]
struct Sample {
    raw: [u32; 5],
    opaque: bool,
    transparent: bool,
}

const CLEAR: Sample = Sample {
    raw: [0; 5],
    opaque: false,
    transparent: true,
};

/// Layer `idx` at every new-canvas pixel, transparent off its storage.
fn layer_at(doc: &Doc, idx: usize) -> Vec<Sample> {
    let (w, h) = (WINDOW.2, WINDOW.3);
    let mut out = vec![CLEAR; (w * h) as usize];
    let mut put = |offset: (i32, i32), dims: (u32, u32), read: &dyn Fn(usize) -> Sample| {
        for j in 0..h as i32 {
            for i in 0..w as i32 {
                let (lx, ly) = (i - offset.0, j - offset.1);
                if lx >= 0 && ly >= 0 && (lx as u32) < dims.0 && (ly as u32) < dims.1 {
                    out[(j * w as i32 + i) as usize] =
                        read(ly as usize * dims.0 as usize + lx as usize);
                }
            }
        }
    };
    match doc {
        Doc::Byte(d) => {
            let l = &d.layers[idx];
            let dims = (l.pixels.width(), l.pixels.height());
            put(l.offset, dims, &|k| {
                let p = l
                    .pixels
                    .sample_at(k as u32 % dims.0, k as u32 / dims.0)
                    .unwrap_or([0; 4]);
                Sample {
                    raw: [p[0], p[1], p[2], p[3], 0].map(u32::from),
                    opaque: p[3] == 255,
                    transparent: p[3] == 0,
                }
            });
        }
        Doc::Typed(d) => {
            let layer = d.layer(idx).expect("a record");
            if let Some(image) = layer.cmyk_source() {
                let samples = d.cmyk_layer_samples(idx).unwrap();
                put(layer.offset(), image.dimensions(), &|k| {
                    let p = samples[k];
                    Sample {
                        raw: p.map(u32::from),
                        opaque: p[4] == 65535,
                        transparent: p[4] == 0,
                    }
                });
            } else {
                let image = layer.source().expect("a raster");
                let dims = image.dimensions();
                match image.to_rgba16() {
                    Ok(samples) => put(layer.offset(), dims, &|k| {
                        let p = samples[k];
                        Sample {
                            raw: [p[0], p[1], p[2], p[3], 0].map(u32::from),
                            opaque: p[3] == 65535,
                            transparent: p[3] == 0,
                        }
                    }),
                    Err(_) => {
                        let samples = image.to_rgba32().unwrap();
                        put(layer.offset(), dims, &|k| {
                            let p = samples[k];
                            Sample {
                                raw: [p[0], p[1], p[2], p[3], 0.0].map(f32::to_bits),
                                // A turned bicubic resample of an opaque
                                // float plane sums its weights to within an
                                // ulp of 1 (0x3F7FFFFF), still opaque.
                                opaque: p[3] >= 1.0 - 1e-6,
                                transparent: p[3] == 0.0,
                            }
                        })
                    }
                }
            }
        }
    }
    out
}

fn layer_count(doc: &Doc) -> usize {
    match doc {
        Doc::Byte(d) => d.layers.len(),
        Doc::Typed(d) => d.layer_count(),
    }
}

/// Whether new-canvas pixel `k` lies outside the old canvas's polygon.
fn in_new_area(k: usize) -> bool {
    let w = WINDOW.2 as usize;
    let back = inverse(rotation());
    let (u, v) = apply_affine(
        back,
        f64::from(WINDOW.0) + (k % w) as f64 + 0.5,
        f64::from(WINDOW.1) + (k / w) as f64 + 0.5,
    );
    u < 0.0 || v < 0.0 || u >= f64::from(W) || v >= f64::from(H)
}

// ----------------------------------------------------------------- tests --

#[test]
fn the_grown_new_area_covers_every_pixel_the_pad_reaches() {
    let mask = new_area();
    let (w, h) = (WINDOW.2, WINDOW.3);
    let mut reached = 0;
    for j in 0..h {
        for i in 0..w {
            if pad_reaches(i, j) {
                reached += 1;
                let k = (j * w + i) as usize;
                assert!(
                    mask[k] > 0,
                    "({i}, {j}) reads the pad but lies outside the coverage"
                );
            }
        }
    }
    assert!(
        reached > 0 && mask.contains(&0),
        "a real seam and a kept interior"
    );
}

#[test]
fn content_aware_fills_only_the_grown_new_area_of_record_zero() {
    let mask = new_area();
    for (what, doc, cmyk) in [
        ("8", byte_doc(), false),
        ("16", sixteen_doc(), false),
        ("float", float_document(), false),
        ("cmyk8", cmyk8_doc(), true),
    ] {
        let base = cropped(&doc, cmyk);
        let filled = content_aware(&base, &mask);
        assert_eq!(layer_count(&filled), layer_count(&base), "{what}");
        for idx in 0..layer_count(&base) {
            let (before, after) = (layer_at(&base, idx), layer_at(&filled, idx));
            for (k, m) in mask.iter().enumerate() {
                if *m == 0 {
                    assert_eq!(
                        after[k], before[k],
                        "{what} layer {idx}: {k} outside the coverage"
                    );
                }
            }
        }
        let upper = layer_at(&filled, 1);
        let record0 = layer_at(&filled, 0);
        let pad = layer_at(&base, 0);
        let mut differs = false;
        for k in (0..mask.len()).filter(|&k| in_new_area(k)) {
            assert!(
                upper[k].transparent,
                "{what}: the upper layer's new area at {k}"
            );
            assert!(
                record0[k].opaque,
                "{what}: record 0's new area at {k} is opaque: {:?} (pad {:?})",
                record0[k], pad[k]
            );
            differs |= record0[k].raw != pad[k].raw;
        }
        assert!(
            differs,
            "{what}: the new area is not uniformly the pad colour"
        );
    }
}

#[test]
fn generative_expand_fills_the_grown_new_area_and_keeps_the_picture() {
    let mask = new_area();
    let coverage: Vec<u16> = mask.iter().map(|&v| u16::from(v) * 257).collect();
    let (w, h) = (WINDOW.2, WINDOW.3);
    for (what, doc, cmyk, tolerance) in [
        ("8", byte_doc(), false, 0.0),
        ("16", sixteen_doc(), false, 1.0),
        ("cmyk8", cmyk8_doc(), true, f64::NAN),
    ] {
        let base = cropped(&doc, cmyk);
        let before = projection(&base);
        let framed = frame(&coverage, w, h, &[NATIVE]).unwrap();
        assert_eq!((framed[4], framed[5]), NATIVE, "{what}");
        let input = input_with(&base, 0, &coverage, NATIVE, LIMIT, ptr::null()).unwrap();
        assert_eq!(input.crop[..], framed[..4], "{what}");
        let results: Vec<Picture> = COLORS[..3].iter().map(|&c| solid(NATIVE, c)).collect();
        let (out, index) = apply(&base, &coverage, &results, TAG).unwrap();
        assert_eq!(index, 1, "{what}: the layer lands above record 0");
        let after = projection(&out);
        let max = if matches!(base, Doc::Byte(_)) {
            255.0
        } else {
            65535.0
        };
        let mut solid_area: Option<[f64; 3]> = None;
        for (k, &c) in coverage.iter().enumerate() {
            let at = 4 * k;
            if c == 0 {
                assert_eq!(after[at..at + 4], before[at..at + 4], "{what}: {k} kept");
                continue;
            }
            if c < 65535 {
                continue;
            }
            assert_eq!(after[at + 3], 1.0, "{what}: {k} opaque");
            let got = [after[at], after[at + 1], after[at + 2]];
            if cmyk {
                let first = *solid_area.get_or_insert(got);
                assert_eq!(got, first, "{what}: the answer's ink is uniform at {k}");
                continue;
            }
            for (ch, g) in got.iter().enumerate() {
                let want = f64::from(COLORS[0][ch]) / 255.0;
                assert!(
                    (g - want).abs() * max <= tolerance + 1e-6,
                    "{what}: {k} channel {ch} {g} vs {want}"
                );
            }
        }
    }
}
