//! The retouch commands on a CMYK document (#1), through the FFI, each
//! against an oracle written here:
//!
//! * the Healing Brush equals, exactly, the shipped RGB16 heal of the two
//!   `L` twin documents reading the twin plates, mapped back;
//! * the Clone Stamp in Normal lands within one code of the RGB16 clone of
//!   the twins, and in Multiply against an f64 W3C step with `B` the
//!   product of `L`s (`cmyk_structure_fixture::over`) over the cloned ink;
//! * Spot Healing and Content-Aware Fill synthesize on the four inks:
//!   nothing outside the coverage box grown by the ring changes, alpha is
//!   never written, and a seed repeats bit for bit;
//! * `new_layer` output is a CMYK raster above the target holding only the
//!   healed pixels; `expand_destination` grows the target with paper;
//! * Red Eye, Pass Through and an RGB plate refuse by name; the source view
//!   is the canvas in Display P3.

use std::ffi::CString;
use std::ptr;

use doctored_core::doc::BlendMode;
use doctored_core::ffi_precision_edit::dr_precision_doc_edit;
use doctored_core::ffi_retouch::{dr_precision_doc_retouch_from, dr_precision_doc_retouch_source};
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer};
use doctored_core::precision_image::PrecisionImage;
use serde_json::{json, Value};

use crate::cmyk_structure_fixture::{cmyk_layers, free, over, then, varied, RgbLayer, LIMIT};
use crate::common::take_err_string;

const W: u32 = 24;
const H: u32 = 20;
const TARGET: usize = 1;

/// An opaque background and an opaque 16 × 12 layer at (4, 4) in a Pass
/// Through group (where a new retouch layer may go).
fn document() -> *mut PrecisionDocument {
    sized(W, H)
}

/// [`document`] on a `w × h` canvas, the layer inset by 4 on every side.
fn sized(w: u32, h: u32) -> *mut PrecisionDocument {
    let opaque = |w, h, seed| -> Vec<[u16; 4]> {
        varied(w, h, seed)
            .into_iter()
            .map(|p| [p[0], p[1], p[2], 65535])
            .collect()
    };
    let doc = cmyk_layers(
        w,
        h,
        vec![
            RgbLayer {
                name: "Background",
                w,
                h,
                offset: (0, 0),
                pixels: opaque(w, h, 2),
            },
            RgbLayer {
                name: "Offset",
                w: w - 8,
                h: h - 8,
                offset: (4, 4),
                pixels: opaque(w - 8, h - 8, 6),
            },
        ],
    );
    then(
        doc,
        r#"{"op":"group_layers","layers":[1],"name":"Group","pass_through":true}"#,
        &[],
    )
}

/// A hard disc of radius 3 at (12, 10).
fn disc() -> Vec<u16> {
    (0..W * H)
        .map(|i| {
            let (x, y) = (f64::from(i % W), f64::from(i / W));
            if (x - 12.0).powi(2) + (y - 10.0).powi(2) <= 9.0 {
                65535
            } else {
                0
            }
        })
        .collect()
}

fn edit(
    doc: *const PrecisionDocument,
    request: &Value,
    samples: &[u16],
) -> Result<*mut PrecisionDocument, String> {
    let text = CString::new(request.to_string()).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_edit(
            doc,
            text.as_ptr(),
            samples.as_ptr(),
            samples.len(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    if !err.is_null() {
        return Err(take_err_string(err));
    }
    Ok(out)
}

/// A record's samples, size and offset.
type Record = (Vec<[u16; 5]>, (u32, u32), (i32, i32));

fn record(doc: *const PrecisionDocument, index: usize) -> Record {
    let layer = unsafe { &*doc }.layer(index).unwrap();
    (
        unsafe { &*doc }.cmyk_layer_samples(index).unwrap(),
        layer.cmyk_source().unwrap().dimensions(),
        layer.offset(),
    )
}

/// The `L` pass of CMYK samples.
fn pass(samples: &[[u16; 5]], k: bool) -> Vec<[u16; 4]> {
    samples
        .iter()
        .map(|s| {
            if k {
                [65535 - s[3], 65535 - s[3], 65535 - s[3], s[4]]
            } else {
                [65535 - s[0], 65535 - s[1], 65535 - s[2], s[4]]
            }
        })
        .collect()
}

/// A canvas-sized one-layer RGB16 document of `pixels` at `offset`.
fn rgb(pixels: Vec<[u16; 4]>, (w, h): (u32, u32), offset: (i32, i32)) -> *mut PrecisionDocument {
    let image = PrecisionImage::from_rgba16(w, h, pixels, IccProfile::srgb(), None).unwrap();
    let layer = PrecisionDocumentLayer::raster("twin", image, offset, 1.0, true).unwrap();
    Box::into_raw(Box::new(
        PrecisionDocument::from_parts(
            W,
            H,
            IccProfile::srgb(),
            Metadata::default(),
            Resolution::default(),
            vec![layer],
        )
        .unwrap(),
    ))
}

/// The Current plate of the record (its ink on paper over the canvas).
fn current_plate(doc: *const PrecisionDocument) -> Vec<[u16; 5]> {
    let (samples, (w, h), (ox, oy)) = record(doc, TARGET);
    (0..W * H)
        .map(|i| {
            let (x, y) = (i as i32 % W as i32 - ox, i as i32 / W as i32 - oy);
            if x >= 0 && y >= 0 && (x as u32) < w && (y as u32) < h {
                samples[(y as u32 * w + x as u32) as usize]
            } else {
                [0; 5]
            }
        })
        .collect()
}

/// `dr_precision_doc_retouch_from` on a twin pair: the twin of the record
/// healed (or cloned) from the twin plate.
fn twin_retouch(
    doc: *const PrecisionDocument,
    request: &Value,
    samples: &[u16],
    k: bool,
    blank: bool,
) -> Vec<[u16; 4]> {
    let (source, dims, offset) = record(doc, TARGET);
    let pixels = if blank {
        vec![[65535, 65535, 65535, 0]; source.len()]
    } else {
        pass(&source, k)
    };
    let target = rgb(pixels.clone(), dims, offset);
    let plates = rgb(pass(&current_plate(doc), k), (W, H), (0, 0));
    let mut twin_request = request.clone();
    twin_request["layer"] = json!(0);
    if blank {
        twin_request["opacity"] = json!(1.0);
        twin_request.as_object_mut().unwrap().remove("blend_mode");
    }
    let text = CString::new(twin_request.to_string()).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_retouch_from(
            target,
            plates,
            text.as_ptr(),
            samples.as_ptr(),
            samples.len(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(err.is_null(), "{}", take_err_string(err));
    let from = if out.is_null() { target } else { out };
    let px = unsafe { &*from }
        .layer(0)
        .unwrap()
        .source()
        .unwrap()
        .to_rgba16()
        .unwrap();
    if !out.is_null() {
        free(out);
    }
    free(target);
    free(plates);
    px
}

#[test]
fn the_healing_brush_equals_its_two_rgb16_twins() {
    let doc = document();
    let samples = disc();
    let request =
        json!({"op":"retouch_heal","layer":TARGET,"matrix":[1,0,0,1,-5,-3],"opacity":0.9});
    let out = edit(doc, &request, &samples).unwrap();
    assert!(!out.is_null());
    let (source, ..) = record(doc, TARGET);
    let a = twin_retouch(doc, &request, &samples, false, false);
    let b = twin_retouch(doc, &request, &samples, true, false);
    let want: Vec<[u16; 5]> = source
        .iter()
        .enumerate()
        .map(|(i, s)| {
            [
                65535 - a[i][0],
                65535 - a[i][1],
                65535 - a[i][2],
                65535 - b[i][1],
                s[4],
            ]
        })
        .collect();
    assert_eq!(record(out, TARGET).0, want);
    free(out);
    free(doc);
}

#[test]
fn the_clone_stamp_is_the_twins_clone_in_normal_and_the_f64_step_in_multiply() {
    let doc = document();
    let samples = disc();
    let (source, ..) = record(doc, TARGET);
    for (mode, opacity) in [(0, 1.0), (0, 0.6), (1, 0.7)] {
        let request = json!({"op":"retouch_clone","layer":TARGET,"matrix":[1,0,0,1,-6,-2],
            "opacity":opacity,"blend_mode":mode});
        let out = edit(doc, &request, &samples).unwrap();
        assert!(!out.is_null());
        let (got, ..) = record(out, TARGET);
        // S: the clone of each pass onto a blank twin.
        let a = twin_retouch(doc, &request, &samples, false, true);
        let b = twin_retouch(doc, &request, &samples, true, true);
        for (i, s) in source.iter().enumerate() {
            if a[i][3] == 0 {
                assert_eq!(got[i], *s, "pixel {i} outside the stroke");
                continue;
            }
            let ink = [
                1.0 - f64::from(a[i][0]) / 65535.0,
                1.0 - f64::from(a[i][1]) / 65535.0,
                1.0 - f64::from(a[i][2]) / 65535.0,
                1.0 - f64::from(b[i][1]) / 65535.0,
            ];
            let mut want = s.map(|v| f64::from(v) / 65535.0);
            let mode = if mode == 0 {
                BlendMode::Normal
            } else {
                BlendMode::Multiply
            };
            over(&mut want, ink, f64::from(a[i][3]) / 65535.0 * opacity, mode);
            for c in 0..5 {
                let w = (want[c] * 65535.0).round();
                assert!(
                    (f64::from(got[i][c]) - w).abs() <= 1.0,
                    "{mode:?} pixel {i} plate {c}: {} vs {w}",
                    got[i][c]
                );
            }
        }
        free(out);
    }
    free(doc);
}

#[test]
fn spot_heal_and_content_aware_fill_synthesize_on_the_four_inks() {
    let (cw, ch) = (72u32, 64u32);
    let doc = sized(cw, ch);
    let samples: Vec<u16> = (0..cw * ch)
        .map(|i| {
            let (x, y) = (f64::from(i % cw), f64::from(i / cw));
            if (x - 36.0).powi(2) + (y - 32.0).powi(2) <= 9.0 {
                65535
            } else {
                0
            }
        })
        .collect();
    let ring = 4;
    let (bx, by) = (33, 29);
    for op in ["spot_heal", "content_aware_fill"] {
        let mut request = json!({"op":op,"layer":TARGET,"ring":ring,"seed":7,
            "x":bx,"y":by,"width":7,"height":7});
        if op == "spot_heal" {
            request["opacity"] = json!(1.0);
        }
        let out = edit(doc, &request, &samples).unwrap();
        assert!(!out.is_null(), "{op} changed the record");
        let again = edit(doc, &request, &samples).unwrap();
        assert_eq!(
            record(out, TARGET).0,
            record(again, TARGET).0,
            "{op}: a seed repeats"
        );
        let (before, (w, _), (ox, oy)) = record(doc, TARGET);
        let (after, ..) = record(out, TARGET);
        let reach = ring + 3;
        let mut changed = 0;
        for (i, (b, a)) in before.iter().zip(&after).enumerate() {
            let (x, y) = (i as i32 % w as i32 + ox, i as i32 / w as i32 + oy);
            assert_eq!(a[4], b[4], "{op}: alpha is never written");
            if a != b {
                changed += 1;
                assert!(
                    (bx - reach..bx + 7 + reach).contains(&x)
                        && (by - reach..by + 7 + reach).contains(&y),
                    "{op}: ({x},{y}) is outside the coverage box grown by the ring"
                );
            }
        }
        assert!(changed > 0);
        free(out);
        free(again);
    }
    free(doc);
}

#[test]
fn new_layer_and_expand_write_cmyk_rasters() {
    let doc = document();
    let samples = disc();
    let request = json!({"op":"retouch_heal","layer":TARGET,"matrix":[1,0,0,1,-5,-3],
        "output":"new_layer"});
    let out = edit(doc, &request, &samples).unwrap();
    let next = unsafe { &*out };
    assert_eq!(next.layer_count(), unsafe { &*doc }.layer_count() + 1);
    let added = next.layer(TARGET + 1).unwrap();
    assert_eq!(added.name(), "Retouch");
    assert_eq!(added.cmyk_source().unwrap().dimensions(), (W, H));
    let layer = next.cmyk_layer_samples(TARGET + 1).unwrap();
    for (i, s) in layer.iter().enumerate() {
        if samples[i] < 32768 {
            assert_eq!(*s, [0; 5], "pixel {i} outside the stroke is paper");
        }
    }
    assert!(layer.iter().any(|s| s[4] != 0));
    free(out);
    // Expanded: a clone of the composite where the record has no pixels
    // grows it to its union with the canvas, the old pixels kept in place.
    let reach: Vec<u16> = (0..W * H)
        .map(|i| if i % W < 3 && i / W < 3 { 65535 } else { 0 })
        .collect();
    let request = json!({"op":"retouch_clone","layer":TARGET,"sample":"all",
        "expand_destination":true});
    let out = edit(doc, &request, &reach).unwrap();
    let (grown, dims, offset) = record(out, TARGET);
    assert_eq!(offset, (0, 0));
    assert_eq!(dims, (W, H));
    assert!(grown[0][4] > 0, "the clone landed outside the old extent");
    let (old, ..) = record(doc, TARGET);
    let at = (4 * W + 4) as usize;
    assert_eq!(grown[at..at + 16], old[..16]);
    free(out);
    free(doc);
}

#[test]
fn refusals_name_themselves_and_the_source_is_the_view() {
    let doc = document();
    let samples = disc();
    assert_eq!(
        edit(
            doc,
            &json!({"op":"red_eye","layer":TARGET,"x":2,"y":2,"width":5,"height":5}),
            &samples
        )
        .unwrap_err(),
        "Red Eye is unavailable in a CMYK document"
    );
    assert_eq!(
        edit(
            doc,
            &json!({"op":"retouch_clone","layer":TARGET,"blend_mode":27}),
            &samples
        )
        .unwrap_err(),
        "The Pass Through blend mode is unavailable in a CMYK document"
    );
    assert_eq!(
        edit(doc, &json!({"op":"retouch_heal","layer":2}), &samples).unwrap_err(),
        "retouch requires a raster target"
    );
    let plates = rgb(vec![[0; 4]; (W * H) as usize], (W, H), (0, 0));
    let text = CString::new(json!({"op":"retouch_heal","layer":TARGET}).to_string()).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_retouch_from(
            doc,
            plates,
            text.as_ptr(),
            samples.as_ptr(),
            samples.len(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(out.is_null());
    assert_eq!(
        take_err_string(err),
        "retouch plates must have the document's bit depth"
    );
    free(plates);
    let mut err = ptr::null_mut();
    let view = unsafe {
        dr_precision_doc_retouch_source(doc, TARGET, 2, false, LIMIT, 0, ptr::null(), &mut err)
    };
    assert!(!view.is_null(), "{}", take_err_string(err));
    let view = unsafe { Box::from_raw(view) };
    assert_eq!(view.dimensions(), (W, H));
    assert_eq!(view.profile().name(), unsafe { &*doc }.profile().name());
    free(doc);
}

#[test]
fn the_descriptor_reports_the_retouch_plans() {
    let (cw, ch) = (300u32, 240u32);
    let doc = sized(cw, ch);
    let mut err = ptr::null_mut();
    let text = unsafe { doctored_core::ffi_precision::dr_precision_doc_descriptor(doc, &mut err) };
    assert!(!text.is_null(), "{}", take_err_string(err));
    let descriptor: Value =
        serde_json::from_str(unsafe { std::ffi::CStr::from_ptr(text) }.to_str().unwrap()).unwrap();
    unsafe { doctored_core::ffi::dr_string_free(text) };
    for key in [
        "cmyk_clone_scratch_bytes",
        "cmyk_heal_scratch_bytes",
        "cmyk_spot_heal_scratch_bytes",
        "cmyk_dodge_scratch_bytes",
        "cmyk_smudge_scratch_bytes",
        "cmyk_history_brush_scratch_bytes",
        "cmyk_equalize_scratch_bytes",
    ] {
        assert!(
            descriptor[key].as_u64().is_some_and(|v| v > 0),
            "{key}: {}",
            descriptor[key]
        );
    }
    free(doc);
}
