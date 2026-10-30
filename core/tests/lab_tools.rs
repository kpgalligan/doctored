//! #1 Lab 2f, third part, through the FFI: Remove, the Art History Brush
//! and Red Eye on a Lab Color layer, at 8 and 16 bits unless said.
//!
//! The oracles are independent of the Lab arms: the CIE formulas of
//! `common/lab_fixture.rs` (the ICC v4 decode, Display P3's published matrix
//! and the IEC curve, ProPhoto's matrix and power), the byte Art History
//! Brush (`dr_doc_art_history_brush_layer`) on an 8-bit RGB document of the
//! same codes, and the RGB Red Eye on a 16-bit ProPhoto RGB document of the
//! oracle's ProPhoto codes.
//!
//! Bounds, declared before measuring:
//! * Remove's model input is the Display P3 view: every unmasked cell within
//!   32/65535 of the oracle's P3 of the decoded codes, clamped (the view's
//!   bound). Its apply converts the model's P3 fill to Lab: a fully covered
//!   pixel within 0.2 L\* and 0.3 a\*/b\* of the oracle's L\*a\*b\* of the
//!   plane's colour, a half-covered one within the same of the half/half mix
//!   of that fill's codes and the codes under it, an uncovered one exact,
//!   and on a new layer the covered alpha is the coverage.
//! * The Art History Brush (8 bits) on R = G = B codes equals the byte
//!   master within 1 code of 255 on every channel; with L\* varying along x
//!   and a\* along y, at tolerance 0 (no stroke skipped on either side) onto
//!   a flat target, the L channel equals the byte master's R on the
//!   `[L, L, L]` document within 1 code of 255 (the strokes follow L\*); its
//!   refusals are exact, and Hue paints (phase 3; its arm is
//!   `lab_blend.rs`'s).
//! * Red Eye: every pixel it changes within 0.2 L\* and 0.3 a\*/b\* of the
//!   oracle's L\*a\*b\* of the ProPhoto twin's result; the wide-gamut green
//!   and every pixel the twin leaves unchanged keep their codes exactly; the
//!   Lab plan charges at least the sRGB document's plan plus 8 B a layer
//!   pixel.

use std::ffi::CString;
use std::ptr;
use std::sync::Arc;

use doctored_core::doc::DrDocument;
use doctored_core::ffi_doc::dr_doc_free;
use doctored_core::ffi_precision_color::dr_precision_doc_add_adjustment;
use doctored_core::ffi_precision_edit::dr_precision_doc_edit_plan;
use doctored_core::ffi_remove_model::{
    dr_precision_doc_remove_model_apply, dr_precision_doc_remove_model_input,
};
use doctored_core::ffi_retouch_brush::{
    dr_doc_art_history_brush_layer, dr_precision_doc_art_history_brush,
    dr_precision_doc_art_history_brush_plan,
};
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer};
use doctored_core::precision_image::PrecisionImage;
use image::{Rgba, RgbaImage};
use serde_json::{json, Value};

use crate::common::{layer_pixels, take_doc, take_err_string};
use crate::gray_ops::{edit, free, plan_json, plan_samples, result, LIMIT};
use crate::lab_fixture::{
    decode, lab_to_prophoto, lab_to_rgb, lab_to_srgb, p3_to_lab, prophoto_to_lab, srgb_to_lab,
    P3_TO_XYZ,
};
use crate::lab_targets::{codes, to_lab};
use crate::lab_wide_gamut::{encode, twin};

const VIEW_BOUND: f64 = 32.0 / 65535.0;
const BOUND_L: f64 = 0.2;
const BOUND_AB: f64 = 0.3;

/// A document of one `w` × `h` layer holding exactly `pixels` in `profile`.
fn doc_in(
    w: u32,
    h: u32,
    pixels: Vec<[u16; 4]>,
    profile: Arc<IccProfile>,
) -> *mut PrecisionDocument {
    let image = PrecisionImage::from_rgba16(w, h, pixels, profile.clone(), None).unwrap();
    let layer = PrecisionDocumentLayer::raster("Layer", image, (0, 0), 1.0, true).unwrap();
    let doc = PrecisionDocument::from_parts(
        w,
        h,
        profile,
        Metadata::default(),
        Resolution::default(),
        vec![layer],
    )
    .unwrap();
    Box::into_raw(Box::new(doc))
}

/// A Lab document of `depth` holding `pixels` (twin codes), through Mode ▸
/// Lab Color (the twin into itself; 8 bits rounds to the ×257 grid).
fn lab_grid(w: u32, h: u32, pixels: Vec<[u16; 4]>, depth: u32) -> *mut PrecisionDocument {
    let rgb = doc_in(w, h, pixels, twin());
    let lab = to_lab(rgb, &format!(r#"{{"depth":{depth}}}"#)).unwrap();
    free(rgb);
    assert!(unsafe { &*lab }.is_lab());
    lab
}

/// L\*a\*b\* as unit codes (the ICC v4 16-bit encoding, unrounded).
fn unit(lab: [f64; 3]) -> [f64; 3] {
    [
        lab[0] / 100.0,
        (lab[1] + 128.0) / 255.0,
        (lab[2] + 128.0) / 255.0,
    ]
}

/// Unit codes as L\*a\*b\*.
fn from_unit(u: [f64; 3]) -> [f64; 3] {
    [u[0] * 100.0, u[1] * 255.0 - 128.0, u[2] * 255.0 - 128.0]
}

fn assert_lab_near(got: [f64; 3], want: [f64; 3], what: &str) {
    assert!(
        (got[0] - want[0]).abs() <= BOUND_L
            && (got[1] - want[1]).abs() <= BOUND_AB
            && (got[2] - want[2]).abs() <= BOUND_AB,
        "{what}: {got:?} against {want:?}"
    );
}

// ---- Remove ---------------------------------------------------------------

const SIDE: u32 = 64;
const RS: u32 = 64;

/// A varied opaque L\*a\*b\* field over the `RS` square.
fn remove_field() -> Vec<[u16; 4]> {
    (0..RS * RS)
        .map(|i| {
            let (x, y) = (f64::from(i % RS), f64::from(i / RS));
            encode([
                15.0 + 70.0 * x / 63.0,
                -50.0 + 100.0 * y / 63.0,
                40.0 - 80.0 * (x + y) / 126.0,
            ])
        })
        .collect()
}

/// Full coverage within radius 7 of (32, 32), half to radius 9.
fn remove_disc() -> Vec<u16> {
    (0..RS * RS)
        .map(|i| {
            let (x, y) = (f64::from(i % RS) + 0.5, f64::from(i / RS) + 0.5);
            let d = (x - 32.0).powi(2) + (y - 32.0).powi(2);
            if d <= 49.0 {
                65535
            } else if d <= 81.0 {
                32768
            } else {
                0
            }
        })
        .collect()
}

#[test]
fn remove_feeds_the_model_the_display_p3_view() {
    let coverage = remove_disc();
    let cells = (SIDE * SIDE) as usize;
    for depth in [8, 16] {
        let lab = lab_grid(RS, RS, remove_field(), depth);
        let stored = codes(lab, 0);
        let sample = CString::new("current").unwrap();
        let mut crop = [0u32; 4];
        let mut image = vec![0f32; 3 * cells];
        let mut mask = vec![0f32; cells];
        let mut err = ptr::null_mut();
        let ok = unsafe {
            dr_precision_doc_remove_model_input(
                lab,
                0,
                sample.as_ptr(),
                false,
                coverage.as_ptr(),
                coverage.len(),
                SIDE,
                crop.as_mut_ptr(),
                image.as_mut_ptr(),
                image.len(),
                mask.as_mut_ptr(),
                mask.len(),
                LIMIT,
                0,
                ptr::null(),
                &mut err,
            )
        };
        assert!(ok, "remove input at {depth}: {}", take_err_string(err));
        assert_eq!(crop, [0, 0, RS, RS], "a 1:1 crop");
        let mut unmasked = 0;
        for i in 0..cells {
            if mask[i] != 0.0 {
                continue;
            }
            unmasked += 1;
            let want = lab_to_rgb(decode(stored[i]), P3_TO_XYZ).map(|v| v.clamp(0.0, 1.0));
            for c in 0..3 {
                let got = f64::from(image[c * cells + i]);
                assert!(
                    (got - want[c]).abs() <= VIEW_BOUND,
                    "remove input at {depth}, cell {i} channel {c}: {got} against {}",
                    want[c]
                );
            }
        }
        assert!(unmasked > cells / 2, "remove input at {depth}: {unmasked}");
        free(lab);
    }
}

/// The model's plane: one Display P3 colour.
const PLANE: [f64; 3] = [0.75, 0.35, 0.2];

fn remove_apply(doc: *const PrecisionDocument, new_layer: bool) -> *mut PrecisionDocument {
    let coverage = remove_disc();
    let cells = (SIDE * SIDE) as usize;
    let plane: Vec<f32> = (0..3 * cells).map(|k| PLANE[k / cells] as f32).collect();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_remove_model_apply(
            doc,
            0,
            coverage.as_ptr(),
            coverage.len(),
            SIDE,
            plane.as_ptr(),
            plane.len(),
            new_layer,
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!out.is_null(), "remove apply: {}", take_err_string(err));
    out
}

#[test]
fn remove_converts_the_models_fill_to_lab_and_mixes_codes_by_coverage() {
    let coverage = remove_disc();
    let fill = p3_to_lab(PLANE);
    let fill_unit = unit(fill);
    for depth in [8, 16] {
        let lab = lab_grid(RS, RS, remove_field(), depth);
        let under = codes(lab, 0);
        let out = remove_apply(lab, false);
        assert!(unsafe { &*out }.is_lab());
        let got = codes(out, 0);
        let (mut full, mut half) = (0, 0);
        for (i, (p, q)) in got.iter().zip(&under).enumerate() {
            let what = format!("remove at {depth}, pixel {i} coverage {}", coverage[i]);
            match coverage[i] {
                0 => assert_eq!(p, q, "{what}"),
                65535 => {
                    full += 1;
                    assert_lab_near(decode(*p), fill, &what);
                    assert_eq!(p[3], 65535, "{what}");
                }
                _ => {
                    half += 1;
                    let mix: [f64; 3] = std::array::from_fn(|c| {
                        0.5 * fill_unit[c] + 0.5 * f64::from(q[c]) / 65535.0
                    });
                    assert_lab_near(decode(*p), from_unit(mix), &what);
                    assert_eq!(p[3], 65535, "{what}");
                }
            }
        }
        assert!(
            full > 0 && half > 0,
            "remove at {depth}: {full} full, {half} half"
        );
        let added = remove_apply(lab, true);
        let layer = codes(added, 1);
        assert_eq!(codes(added, 0), under, "remove at {depth}: the layer below");
        for (i, p) in layer.iter().enumerate() {
            let what = format!("remove new layer at {depth}, pixel {i}");
            assert_eq!(p[3], coverage[i], "{what}");
            if coverage[i] == 65535 {
                assert_lab_near(decode(*p), fill, &what);
            }
        }
        for doc in [added, out, lab] {
            free(doc);
        }
    }
}

// ---- Art History ----------------------------------------------------------

const AS: u32 = 64;
const HARD: &str =
    r#"{"version":1,"hardness":1.0,"roundness":1.0,"angle_degrees":0.0,"pixel":false}"#;
const DABS: [f64; 8] = [32., 32., 5., 1., 40., 36., 5., 1.];
const DARKEN: u32 = 6;
const HUE: u32 = 23;

fn art_request(mode: u32) -> String {
    format!(
        r#"{{"version":1,"style":"tight_long","area":30,"tolerance":0,"opacity":1,"blend_mode":{mode},"seed":4}}"#
    )
}

fn art_plan(
    doc: *const PrecisionDocument,
    target: usize,
    source: *const PrecisionDocument,
    request: &str,
) -> Result<Value, String> {
    let text = CString::new(request).unwrap();
    let brush = CString::new(HARD).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_art_history_brush_plan(
            doc,
            target,
            source,
            0,
            text.as_ptr(),
            brush.as_ptr(),
            DABS.as_ptr(),
            DABS.len() / 4,
            false,
            LIMIT,
            0,
            &mut err,
        )
    };
    plan_json(out, err)
}

fn art_typed(
    doc: *const PrecisionDocument,
    source: *const PrecisionDocument,
    request: &str,
) -> Result<Option<*mut PrecisionDocument>, String> {
    let text = CString::new(request).unwrap();
    let brush = CString::new(HARD).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_art_history_brush(
            doc,
            0,
            source,
            0,
            text.as_ptr(),
            brush.as_ptr(),
            DABS.as_ptr(),
            DABS.len() / 4,
            ptr::null(),
            0,
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    result(out, err)
}

/// The byte master on 8-bit documents of `current` and `source` (bytes).
fn byte_art(current: Vec<[u8; 4]>, source: Vec<[u8; 4]>, request: &str) -> Vec<u8> {
    let make = |pixels: Vec<[u8; 4]>| {
        let image = RgbaImage::from_fn(AS, AS, |x, y| Rgba(pixels[(y * AS + x) as usize]));
        Box::into_raw(Box::new(DrDocument::from_pixels(image)))
    };
    let (d, s) = (make(current), make(source));
    let text = CString::new(request).unwrap();
    let brush = CString::new(HARD).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_doc_art_history_brush_layer(
            d,
            0,
            s,
            0,
            text.as_ptr(),
            brush.as_ptr(),
            DABS.as_ptr(),
            DABS.len() / 4,
            ptr::null(),
            0,
            &mut err,
        )
    };
    unsafe {
        dr_doc_free(d);
        dr_doc_free(s);
    }
    let (doc, message) = take_doc(out, err);
    let handle = Box::into_raw(Box::new(
        doc.unwrap_or_else(|| panic!("byte art history: {message:?}")),
    ));
    let pixels = layer_pixels(handle, 0);
    unsafe { dr_doc_free(handle) };
    pixels
}

/// 8-bit codes as bytes.
fn bytes_of(codes: &[[u16; 4]]) -> Vec<[u8; 4]> {
    codes.iter().map(|p| p.map(|v| (v / 257) as u8)).collect()
}

fn flat(code: [u16; 4]) -> Vec<[u16; 4]> {
    vec![code; (AS * AS) as usize]
}

#[test]
fn art_history_on_equal_codes_equals_the_byte_master() {
    let source = lab_grid(
        AS,
        AS,
        (0..AS * AS)
            .map(|i| {
                let v = (((i % AS) * 4 + (i / AS) * 2) % 256 * 257) as u16;
                [v, v, v, 65535]
            })
            .collect(),
        8,
    );
    let current = lab_grid(AS, AS, flat([0x8080, 0x8080, 0x8080, 65535]), 8);
    let request = art_request(0);
    let planned = art_plan(current, 0, source, &request).unwrap();
    assert_eq!(planned["input_samples"], 0);
    let out = art_typed(current, source, &request)
        .unwrap()
        .expect("painted");
    assert!(unsafe { &*out }.is_lab());
    let painted = codes(out, 0);
    let before = codes(current, 0);
    let master = byte_art(bytes_of(&before), bytes_of(&codes(source, 0)), &request);
    let mut changed = 0;
    for (i, (p, q)) in painted.iter().zip(master.chunks_exact(4)).enumerate() {
        changed += usize::from(*p != before[i]);
        for c in 0..4 {
            let got = f64::from(p[c]) / 257.0;
            assert!(
                (got - f64::from(q[c])).abs() <= 1.0,
                "pixel {i} channel {c}: {p:?} against {q:?}"
            );
        }
    }
    assert!(changed > 0, "the Art History Brush changed nothing");
    for doc in [out, current, source] {
        free(doc);
    }
}

#[test]
fn art_history_strokes_follow_lightness() {
    let lightness = |i: u32| (40 + 3 * (i % AS)) as u16;
    let source = lab_grid(
        AS,
        AS,
        (0..AS * AS)
            .map(|i| {
                [
                    lightness(i) * 257,
                    (60 + 2 * (i / AS)) as u16 * 257,
                    0x8080,
                    65535,
                ]
            })
            .collect(),
        8,
    );
    let current = lab_grid(AS, AS, flat([0x4000, 0x8000, 0x8000, 65535]), 8);
    let request = art_request(0);
    let out = art_typed(current, source, &request)
        .unwrap()
        .expect("painted");
    let painted = codes(out, 0);
    let before = codes(current, 0);
    let master = byte_art(
        vec![[0x40, 0x40, 0x40, 255]; (AS * AS) as usize],
        (0..AS * AS)
            .map(|i| {
                let l = lightness(i) as u8;
                [l, l, l, 255]
            })
            .collect(),
        &request,
    );
    let mut changed = 0;
    for (i, (p, q)) in painted.iter().zip(master.chunks_exact(4)).enumerate() {
        changed += usize::from(*p != before[i]);
        let got = f64::from(p[0]) / 257.0;
        assert!(
            (got - f64::from(q[0])).abs() <= 1.0,
            "pixel {i}: L {got} against the master's R {}",
            q[0]
        );
    }
    assert!(changed > 0, "the Art History Brush changed nothing");
    for doc in [out, current, source] {
        free(doc);
    }
}

fn levels_layer(doc: *const PrecisionDocument) -> *mut PrecisionDocument {
    let name = CString::new("Levels").unwrap();
    let meta = CString::new(
        json!({"type":"adjust","evaluation":"precision_v1","op":"levels",
            "params":{"gamma":1.4}})
        .to_string(),
    )
    .unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_add_adjustment(
            doc,
            name.as_ptr(),
            meta.as_ptr(),
            1.0,
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    result(out, err).unwrap().unwrap()
}

#[test]
fn art_history_refuses_sixteen_bits_modes_sources_and_targets() {
    let request = art_request(0);
    let sixteen = lab_grid(AS, AS, flat([0x8000, 0x8000, 0x8000, 65535]), 16);
    let depth = "The Art History Brush is unavailable in a 16-bit document.";
    assert_eq!(art_plan(sixteen, 0, sixteen, &request).unwrap_err(), depth);
    assert_eq!(art_typed(sixteen, sixteen, &request).unwrap_err(), depth);
    let lab = lab_grid(AS, AS, flat([0x8000, 0x8000, 0x8000, 65535]), 8);
    let darken = art_request(DARKEN);
    let sentence = "The Darken blend mode is unavailable in a Lab Color document.";
    assert_eq!(art_plan(lab, 0, lab, &darken).unwrap_err(), sentence);
    assert_eq!(art_typed(lab, lab, &darken).unwrap_err(), sentence);
    // Phase 3: Hue is admitted.
    assert!(art_plan(lab, 0, lab, &art_request(HUE)).is_ok());
    let rgb = doc_in(
        AS,
        AS,
        flat([0x8000, 0x8000, 0x8000, 65535]),
        IccProfile::srgb(),
    );
    let source = "The Art History Brush's source state is not a Lab Color raster";
    assert_eq!(art_plan(lab, 0, rgb, &request).unwrap_err(), source);
    assert_eq!(art_typed(lab, rgb, &request).unwrap_err(), source);
    let layered = levels_layer(lab);
    assert_eq!(
        art_plan(layered, 1, layered, &request).unwrap_err(),
        "The Art History Brush paints only a Lab Color raster layer"
    );
    for doc in [layered, rgb, lab, sixteen] {
        free(doc);
    }
}

// ---- Red Eye --------------------------------------------------------------

const EW: u32 = 24;
const EH: u32 = 16;
/// The wide-gamut green: outside sRGB, inside ProPhoto RGB.
const GREEN: [f64; 3] = [60.0, -100.0, 60.0];
const GREEN_AT: [(u32, u32); 2] = [(16, 12), (7, 12)];

fn red_eye_request() -> Value {
    json!({"op":"red_eye","layer":0,"x":6,"y":2,"width":12,"height":12,
        "pupil_size":1.0,"darken":1.0})
}

/// Gray L\* 50 with a hard sRGB (220, 30, 30) disc of radius 3 at (12, 8)
/// and the green at `GREEN_AT`, all inside the rect.
fn red_eye_field() -> Vec<[u16; 4]> {
    let red = srgb_to_lab([220.0 / 255.0, 30.0 / 255.0, 30.0 / 255.0]);
    (0..EW * EH)
        .map(|i| {
            let (x, y) = (i % EW, i / EW);
            let d = (f64::from(x) - 12.0).powi(2) + (f64::from(y) - 8.0).powi(2);
            if GREEN_AT.contains(&(x, y)) {
                encode(GREEN)
            } else if d <= 9.0 {
                encode(red)
            } else {
                encode([50.0, 0.0, 0.0])
            }
        })
        .collect()
}

fn edit_plan(doc: *const PrecisionDocument, request: &Value) -> Value {
    let text = CString::new(request.to_string()).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe { dr_precision_doc_edit_plan(doc, text.as_ptr(), LIMIT, 0, &mut err) };
    plan_json(out, err).unwrap()
}

fn run(doc: *const PrecisionDocument, request: &Value) -> *mut PrecisionDocument {
    let n = plan_samples(doc, request).unwrap_or_else(|e| panic!("{request}: {e}"));
    edit(doc, request, &vec![65535; n])
        .unwrap_or_else(|e| panic!("{request}: {e}"))
        .unwrap_or_else(|| panic!("{request} changed nothing"))
}

#[test]
fn red_eye_corrects_the_prophoto_colour_and_keeps_the_rest() {
    let prophoto = lab_to_prophoto(GREEN);
    assert!(
        prophoto.iter().all(|v| (0.0..=1.0).contains(v)),
        "{prophoto:?}"
    );
    assert!(lab_to_srgb(GREEN).iter().any(|v| !(0.0..=1.0).contains(v)));
    let request = red_eye_request();
    for depth in [8, 16] {
        let lab = lab_grid(EW, EH, red_eye_field(), depth);
        let before = codes(lab, 0);
        let twin_codes: Vec<[u16; 4]> = before
            .iter()
            .map(|p| {
                let rgb = lab_to_prophoto(decode(*p));
                let code = |v: f64| (v.clamp(0.0, 1.0) * 65535.0).round() as u16;
                [code(rgb[0]), code(rgb[1]), code(rgb[2]), p[3]]
            })
            .collect();
        let rgb = doc_in(EW, EH, twin_codes.clone(), IccProfile::prophoto_rgb());
        let out = run(lab, &request);
        let twin_result = run(rgb, &request);
        let twin_out = codes(twin_result, 0);
        let got = codes(out, 0);
        let mut changed = 0;
        for (i, p) in got.iter().enumerate() {
            let what = format!("red eye at {depth}, pixel {i}");
            let twin_changed = twin_out[i] != twin_codes[i];
            if GREEN_AT.contains(&(i as u32 % EW, i as u32 / EW)) || !twin_changed {
                assert_eq!(*p, before[i], "{what}");
                continue;
            }
            if *p == before[i] {
                continue;
            }
            changed += 1;
            let unit = [0, 1, 2].map(|c| f64::from(twin_out[i][c]) / 65535.0);
            assert_lab_near(decode(*p), prophoto_to_lab(unit), &what);
            assert_eq!(p[3], before[i][3], "{what}");
        }
        assert!(
            changed >= 20,
            "red eye at {depth}: {changed} pixels changed"
        );
        let srgb = doc_in(EW, EH, before.clone(), IccProfile::srgb());
        let planned = edit_plan(lab, &request)["scratch_bytes"].as_u64().unwrap();
        let rgb_plan = edit_plan(srgb, &request)["scratch_bytes"].as_u64().unwrap();
        assert!(
            planned >= rgb_plan + u64::from(EW * EH) * 8,
            "red eye plan at {depth}: {planned} against {rgb_plan}"
        );
        for doc in [srgb, out, twin_result, rgb, lab] {
            free(doc);
        }
    }
}
