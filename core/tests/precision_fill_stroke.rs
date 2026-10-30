//! Edit ▸ Fill… / Stroke… at 16 bits, black-box through the
//! request API: the selection-plane `stroke` kind against the byte
//! `dr_selection_stroke` band and the whole-plane reference, native paint's
//! `preserve_alpha` and transparency-lock clipping, the scalar Fill/Paint
//! `blend_mode` against an independent W3C oracle, and `stroke_layer_edge`
//! against the byte `dr_doc_stroke_layer_edge`.

use std::ffi::{c_char, c_int, CString};
use std::ptr;

use doctored_core::doc::DrDocument;
use doctored_core::doc_lock::{LOCK_PIXELS, LOCK_TRANSPARENCY};
use doctored_core::ffi_fill_stroke::{dr_doc_stroke_layer_edge, dr_selection_stroke};
use doctored_core::ffi_precision_edit::*;
use doctored_core::ffi_selection_u16::dr_selection_u16_plan;
use doctored_core::icc::IccProfile;
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer};
use doctored_core::precision_image::PrecisionImage;
use doctored_core::precision_selection_plane::reference_apply;
use image::{Rgba, RgbaImage};
use serde_json::{json, Value};

use crate::common;
use crate::native_blend_oracle as oracle;
use crate::native_scalar as native;
use native::*;

const LOCATIONS: [(&str, c_int); 3] = [("inside", 0), ("center", 1), ("outside", 2)];
const BLUE: [u8; 4] = [20, 40, 230, 255];

/// A hard rect and a feathered ellipse, as canvas-sized u8 masks.
fn masks(w: u32, h: u32) -> Vec<Vec<u8>> {
    let rect = (0..w * h)
        .map(|i| {
            let (x, y) = (i % w, i / w);
            if (10..28).contains(&x) && (8..22).contains(&y) {
                255
            } else {
                0
            }
        })
        .collect();
    let ellipse = (0..w * h)
        .map(|i| {
            let (x, y) = ((i % w) as f32 + 0.5, (i / w) as f32 + 0.5);
            let d = (((x - 20.) / 11.).powi(2) + ((y - 15.) / 8.).powi(2)).sqrt();
            (((1. - d) * 3. + 0.5).clamp(0., 1.) * 255.).round() as u8
        })
        .collect();
    vec![rect, ellipse]
}

fn promoted(mask: &[u8]) -> Vec<u16> {
    mask.iter().map(|&v| u16::from(v) * 257).collect()
}

fn byte_band(mask: &[u8], (w, h): (u32, u32), px: f32, location: c_int) -> Vec<u8> {
    let mut out = mask.to_vec();
    assert!(unsafe { dr_selection_stroke(out.as_mut_ptr(), w, h, px, location) });
    out
}

fn stroke_kind(px: f64, location: &str) -> Value {
    json!({"kind":"stroke","width":px,"location":location})
}

/// Whether the u16 selection plan accepts `request` (raw text, so a NaN can
/// be spelled).
fn selection_plan_ok(w: u32, h: u32, request: &str) -> bool {
    let t = CString::new(request).unwrap();
    let mut e = ptr::null_mut();
    let p = unsafe { dr_selection_u16_plan(w, h, t.as_ptr(), LIMIT, 0, &mut e) };
    if p.is_null() {
        unsafe { error(e) };
        false
    } else {
        unsafe { doctored_core::ffi::dr_string_free(p) };
        true
    }
}

/// The plan's refusal for `v`, which must refuse.
fn plan_error(d: &PrecisionDocument, v: Value) -> String {
    let t = text(v);
    let mut e = ptr::null_mut();
    let p = unsafe { dr_precision_doc_edit_plan(d, t.as_ptr(), LIMIT, 0, &mut e) };
    assert!(p.is_null(), "the plan was expected to refuse");
    unsafe { error(e) }
}

/// The edit's refusal for `v`, which must refuse after a successful plan.
fn edit_error(d: &PrecisionDocument, v: Value, input: &[u16]) -> String {
    let t = text(v);
    let mut e = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_edit(
            d,
            t.as_ptr(),
            input.as_ptr(),
            input.len(),
            LIMIT,
            0,
            ptr::null(),
            &mut e,
        )
    };
    assert!(out.is_null(), "the edit was expected to refuse");
    unsafe { error(e) }
}

#[test]
fn stroke_kind_matches_the_byte_band() {
    let (w, h) = (40u32, 30u32);
    for mask in masks(w, h) {
        let input = promoted(&mask);
        for px in [1.0f32, 2.5, 4.0] {
            for (name, code) in LOCATIONS {
                let byte = byte_band(&mask, (w, h), px, code);
                let typed = selection(w, h, stroke_kind(f64::from(px), name), &input);
                for (i, (&t, &b)) in typed.iter().zip(&byte).enumerate() {
                    let diff = (i64::from(t) - i64::from(b) * 257).abs();
                    assert!(diff <= 257, "{name} {px} at {i}: {t} vs {b}·257");
                }
            }
        }
    }
}

#[test]
fn windowed_stroke_equals_the_whole_plane_reference() {
    let (w, h) = (40u32, 30u32);
    let mut cases = masks(w, h);
    // A selection touching the canvas edge: the window is clamped there.
    cases.push(
        (0..w * h)
            .map(|i| if i % w < 9 && i / w > 4 { 255 } else { 0 })
            .collect(),
    );
    for mask in cases {
        let input = promoted(&mask);
        for px in [0.5f64, 2.0, 3.7, 7.0] {
            for (name, _) in LOCATIONS {
                let request = stroke_kind(px, name);
                let bounded = selection(w, h, request.clone(), &input);
                let reference = reference_apply(&request.to_string(), &input, w, h).unwrap();
                assert_eq!(bounded, reference, "{name} {px}");
            }
        }
    }
}

#[test]
fn select_all_strokes_a_frame_inside_and_nothing_outside() {
    let (w, h) = (20u32, 16u32);
    let all = vec![65535u16; (w * h) as usize];
    let inside = selection(w, h, stroke_kind(3., "inside"), &all);
    for (i, &v) in inside.iter().enumerate() {
        let (x, y) = (i as u32 % w, i as u32 / w);
        let k = x.min(w - 1 - x).min(y).min(h - 1 - y);
        assert_eq!(v, if k < 3 { 65535 } else { 0 }, "at ({x}, {y})");
    }
    let outside = selection(w, h, stroke_kind(3., "outside"), &all);
    assert!(outside.iter().all(|&v| v == 0));
    for request in [
        r#"{"kind":"stroke","width":NaN,"location":"inside"}"#,
        r#"{"kind":"stroke","width":0,"location":"inside"}"#,
        r#"{"kind":"stroke","width":251,"location":"inside"}"#,
        r#"{"kind":"stroke","width":2,"location":"sideways"}"#,
    ] {
        assert!(!selection_plan_ok(w, h, request), "{request}");
    }
    assert!(selection_plan_ok(
        w,
        h,
        r#"{"kind":"stroke","width":250,"location":"center"}"#
    ));
}

/// One raster: its samples, size and offset.
type Placed = (Vec<[u16; 4]>, (u32, u32), (i32, i32));

/// A typed document of `layers` (bottom first, each `(pixels, size, offset)`)
/// on a `canvas`.
fn typed(canvas: (u32, u32), layers: Vec<Placed>) -> PrecisionDocument {
    let records = layers
        .into_iter()
        .enumerate()
        .map(|(i, (pixels, size, offset))| {
            let image =
                PrecisionImage::from_rgba16(size.0, size.1, pixels, IccProfile::srgb(), None)
                    .unwrap();
            PrecisionDocumentLayer::raster(&format!("Layer {i}"), image, offset, 1., true).unwrap()
        })
        .collect();
    PrecisionDocument::from_parts(
        canvas.0,
        canvas.1,
        IccProfile::srgb(),
        Default::default(),
        Default::default(),
        records,
    )
    .unwrap()
}

fn layer_samples(d: &PrecisionDocument, i: usize) -> Vec<[u16; 4]> {
    d.layer_source(i).unwrap().to_rgba16().unwrap()
}

fn geometry(d: &PrecisionDocument, i: usize) -> ((u32, u32), (i32, i32)) {
    let layer = d.layer(i).unwrap();
    (layer.source().unwrap().dimensions(), layer.offset())
}

/// A 10×10 trimmed layer: transparent `[0; 4]` on the left half, mixed alpha
/// on the right.
fn trimmed_pixels() -> Vec<[u16; 4]> {
    (0..100u32)
        .map(|i| {
            let (x, y) = (i % 10, i / 10);
            if x < 5 {
                [0; 4]
            } else {
                [
                    (x * 5000) as u16,
                    (y * 6000) as u16,
                    30000,
                    (8000 + x * 5000 + y * 1000) as u16,
                ]
            }
        })
        .collect()
}

fn paint_blend(preserve: bool) -> Value {
    json!({"op":"paint_blend","layer":0,"color":[0.9,0.2,0.1,1.0],"color_space":"document",
        "opacity":0.75,"blend_mode":0,"preserve_alpha":preserve})
}

#[test]
fn preserve_alpha_keeps_alpha_extent_and_transparent_pixels() {
    let canvas = (20u32, 20u32);
    let d = typed(canvas, vec![(trimmed_pixels(), (10, 10), (5, 5))]);
    let whole = vec![65535u16; 400];
    let out = edit(&d, paint_blend(true), &whole).expect("the fill changes colour");
    assert_eq!(geometry(&out, 0), ((10, 10), (5, 5)));
    let (before, after) = (layer_samples(&d, 0), layer_samples(&out, 0));
    for (b, a) in before.iter().zip(&after) {
        assert_eq!(a[3], b[3], "alpha is bit-identical");
        if *b == [0; 4] {
            assert_eq!(*a, [0; 4]);
        }
    }
    assert_ne!(before, after);
    // Without preserve the same fill grows the trimmed layer to the canvas.
    let grown = edit(&d, paint_blend(false), &whole).unwrap();
    assert_eq!(geometry(&grown, 0), ((20, 20), (0, 0)));
    // Coverage over the transparent half only: nothing to keep.
    let left: Vec<u16> = (0..400u32)
        .map(|i| {
            let (x, y) = (i % 20, i / 20);
            if (5..10).contains(&x) && (5..15).contains(&y) {
                65535
            } else {
                0
            }
        })
        .collect();
    assert!(edit(&d, paint_blend(true), &left).is_none());
}

#[test]
fn a_transparency_locked_trimmed_layer_paints_clipped() {
    let d = typed((20, 20), vec![(trimmed_pixels(), (10, 10), (5, 5))]);
    let locked = d.with_layer_locks(0, LOCK_TRANSPARENCY).unwrap().unwrap();
    let whole = vec![65535u16; 400];
    let out = edit(&locked, paint_blend(false), &whole).expect("clipped paint");
    assert_eq!(geometry(&out, 0), ((10, 10), (5, 5)));
    for (b, a) in layer_samples(&d, 0).iter().zip(layer_samples(&out, 0)) {
        assert_eq!(a[3], b[3]);
    }
}

#[test]
fn scalar_blend_mode_matches_the_w3c_oracle() {
    let pixels: Vec<[u16; 4]> = (0..64u32)
        .map(|i| [(i * 1000) as u16, 7, 9, 65535])
        .collect();
    let d = doc(8, 8, pixels.clone());
    let value = 40000u16;
    for kind in ["fill", "paint"] {
        let operation = json!({"kind":kind,"value":value,"opacity":0.5,"blend_mode":1});
        let input = if kind == "paint" {
            vec![65535u16; 64]
        } else {
            vec![]
        };
        let out = scalar(&d, "red", operation, false, &input).unwrap();
        let red: Vec<u16> = layer_samples(&out, 0).iter().map(|p| p[0]).collect();
        for (i, (&r, p)) in red.iter().zip(&pixels).enumerate() {
            let cb = f64::from(p[0]) / 65535.;
            let cs = f64::from(value) / 65535.;
            let expected = cb * 0.5 + 0.5 * oracle::separable(1, cb, cs);
            let diff = (i64::from(r) - i64::from(q(expected * 65535.))).abs();
            assert!(diff <= 1, "{kind} at {i}: {r} vs {expected}");
        }
    }
    // Normal (0) stays the lerp, byte-identical to an absent blend_mode.
    let normal = scalar(
        &d,
        "red",
        json!({"kind":"fill","value":value,"opacity":0.5,"blend_mode":0}),
        false,
        &[],
    )
    .unwrap();
    let absent = scalar(
        &d,
        "red",
        json!({"kind":"fill","value":value,"opacity":0.5}),
        false,
        &[],
    )
    .unwrap();
    assert_eq!(layer_samples(&normal, 0), layer_samples(&absent, 0));
    let hue = plan_error(
        &d,
        json!({"op":"scalar_edit","target":"red","index":0,
            "operation":{"kind":"fill","value":value,"blend_mode":23}}),
    );
    assert!(hue.contains("HSL"), "{hue}");
}

/// A 30×24 soft-edged disc of radius 13 (the byte suite's `soft_disc`).
fn soft_disc() -> RgbaImage {
    RgbaImage::from_fn(30, 24, |x, y| {
        let d = ((x as f32 + 0.5 - 15.0).powi(2) + (y as f32 + 0.5 - 12.0).powi(2)).sqrt();
        let a = ((13.0 - d + 0.5).clamp(0.0, 1.0) * 255.0).round() as u8;
        Rgba([(x * 8) as u8, (y * 10) as u8, 120, a])
    })
}

fn widen(image: &RgbaImage) -> Vec<[u16; 4]> {
    image
        .pixels()
        .map(|p| p.0.map(|v| u16::from(v) * 257))
        .collect()
}

/// The byte document and its typed promotion: an opaque background with
/// `layer` above it at `offset`.
fn pair(
    canvas: (u32, u32),
    layer: &RgbaImage,
    offset: (i32, i32),
) -> (DrDocument, PrecisionDocument) {
    let background = common::opaque_pattern(canvas.0, canvas.1);
    let mut byte = DrDocument::from_pixels(background.clone())
        .adding_image_layer(0, layer.clone(), "Layer")
        .unwrap();
    byte.layers[1].offset = offset;
    let typed = typed(
        canvas,
        vec![
            (widen(&background), canvas, (0, 0)),
            (widen(layer), layer.dimensions(), offset),
        ],
    );
    (byte, typed)
}

fn edge_request(width: f64, location: &str, preserve: bool) -> Value {
    let color: Vec<f64> = BLUE.iter().map(|&v| f64::from(v) / 255.).collect();
    json!({"op":"stroke_layer_edge","layer":1,"width":width,"location":location,"color":color,
        "color_space":"document","opacity":1.0,"blend_mode":0,"preserve_alpha":preserve})
}

fn byte_edge(doc: &DrDocument, width: f32, location: c_int, preserve: bool) -> DrDocument {
    let mut err: *mut c_char = ptr::null_mut();
    let out = unsafe {
        dr_doc_stroke_layer_edge(
            doc,
            1,
            width,
            location,
            BLUE.as_ptr(),
            0,
            1.0,
            preserve,
            ptr::null(),
            &mut err,
        )
    };
    let (out, err) = common::take_doc(out, err);
    assert!(err.is_none(), "{err:?}");
    out.expect("the byte stroke changed the layer")
}

#[test]
fn layer_edge_stroke_grows_past_the_canvas_like_the_byte_stroke() {
    let disc = soft_disc();
    let (byte, typed_doc) = pair((40, 30), &disc, (-8, 12));
    for (name, code) in LOCATIONS {
        let p = plan(&typed_doc, edge_request(3., name, false));
        assert_eq!(p["outline"], true);
        assert_eq!(p["input_samples"], 0);
        let out = edit(&typed_doc, edge_request(3., name, false), &[]).expect("a stroke");
        let ((tw, th), (tx, ty)) = geometry(&out, 1);
        if name != "inside" {
            // The disc touches its rect's top and bottom, and the bottom is
            // already below the canvas: the band grows the layer there.
            assert!(
                ty < 12 && ty + th as i32 > 36,
                "{name} grows past the canvas"
            );
        }
        if name == "outside" {
            assert!(tx < -8, "outside grows past the canvas's left edge");
        }
        let reference = byte_edge(&byte, 3., code, false);
        let layer = &reference.layers[1];
        let ((bw, bh), (bx, by)) = (layer.pixels.dimensions(), layer.offset);
        let bytes = common::layer_bytes(&reference, 1);
        let samples = layer_samples(&out, 1);
        let x0 = tx.min(bx);
        let y0 = ty.min(by);
        let x1 = (tx + tw as i32).max(bx + bw as i32);
        let y1 = (ty + th as i32).max(by + bh as i32);
        for y in y0..y1 {
            for x in x0..x1 {
                let t = if (tx..tx + tw as i32).contains(&x) && (ty..ty + th as i32).contains(&y) {
                    samples[((y - ty) as u32 * tw + (x - tx) as u32) as usize]
                } else {
                    [0; 4]
                };
                let b: [u8; 4] =
                    if (bx..bx + bw as i32).contains(&x) && (by..by + bh as i32).contains(&y) {
                        let i = ((y - by) as u32 * bw + (x - bx) as u32) as usize * 4;
                        [bytes[i], bytes[i + 1], bytes[i + 2], bytes[i + 3]]
                    } else {
                        [0; 4]
                    };
                let alpha = (i64::from(t[3]) - i64::from(b[3]) * 257).abs();
                assert!(alpha <= 257, "{name} alpha at ({x}, {y}): {t:?} vs {b:?}");
                // Colour where the byte straight RGB is meaningful. The byte
                // stroke rounds twice — its u8 band, then its u8 composite —
                // so a pixel blending the ink over the disc's colour can sit
                // up to a byte and a half from the typed one; 2 codes covers it.
                if b[3] >= 128 && t[3] >= 128 * 257 {
                    for c in 0..3 {
                        let diff = (i64::from(t[c]) - i64::from(b[c]) * 257).abs();
                        assert!(
                            diff <= 2 * 257,
                            "{name} channel {c} at ({x}, {y}): {t:?} vs {b:?}"
                        );
                    }
                }
            }
        }
    }
    // Preserve Transparency never grows the layer.
    let kept = edit(&typed_doc, edge_request(3., "outside", true), &[]);
    if let Some(kept) = kept {
        assert_eq!(geometry(&kept, 1), ((30, 24), (-8, 12)));
        for (b, a) in layer_samples(&typed_doc, 1)
            .iter()
            .zip(layer_samples(&kept, 1))
        {
            assert_eq!(a[3], b[3]);
        }
    }
    let centred = edit(&typed_doc, edge_request(3., "center", true), &[]).expect("a stroke");
    assert_eq!(geometry(&centred, 1), ((30, 24), (-8, 12)));
}

#[test]
fn layer_edge_stroke_refusals_and_empty_layers() {
    let empty = RgbaImage::new(12, 10);
    let (_, d) = pair((20, 20), &empty, (3, 4));
    let p = plan(&d, edge_request(2., "outside", false));
    assert_eq!(p["outline"], false);
    assert!(edit(&d, edge_request(2., "outside", false), &[]).is_none());

    let (_, d) = pair((40, 30), &soft_disc(), (5, 3));
    let locked = d.with_layer_locks(1, LOCK_PIXELS).unwrap().unwrap();
    let message = plan_error(&locked, edge_request(2., "outside", false));
    assert!(message.contains("locked"), "{message}");
    let message = edit_error(&locked, edge_request(2., "outside", false), &[]);
    assert!(message.contains("locked"), "{message}");
    let message = plan_error(&d, edge_request(0., "outside", false));
    assert!(message.contains("width"), "{message}");
    let mut pass_through = edge_request(2., "outside", false);
    pass_through["blend_mode"] = json!(27);
    let message = plan_error(&d, pass_through);
    assert!(message.contains("Pass Through"), "{message}");
    let mut background = edge_request(2., "outside", false);
    background["layer"] = json!(7);
    plan_error(&d, background);
}
