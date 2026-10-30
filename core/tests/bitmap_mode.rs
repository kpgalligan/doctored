//! #1 Bitmap phase 1a's conversions through the FFI: 50% Threshold at the
//! boundary, Diffusion Dither (phase 2b's Sierra Lite serpentine) on a
//! hand-worked 4×4 plane and a 16×9 ramp against a test-local Sierra Lite,
//! the Output resolution (resampled by
//! the FFI's own Bicubic resize, an anisotropic document kept, the ppi
//! bounds), Grayscale back at Size Ratio 1 and 2, the hard refusals by
//! sentence, the one-layer rule (`flatten_required`, then `flatten`), every
//! edit and retouch request the gate does not admit refused by the mode's
//! sentence (`bitmap_edits` and `bitmap_tools` test the admitted ones), and
//! the null sweep of
//! the seven exports (phase 1c's three file exports included). Gray inputs
//! are 8-bit gray PNGs (`gray_fixture::png`)
//! opened by `dr_precision_doc_open_gray`.

use std::ffi::CString;
use std::ptr;

use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_bitmap::{
    dr_path_bitmap_source, dr_precision_doc_convert_bitmap_to_gray,
    dr_precision_doc_convert_bitmap_to_gray_plan, dr_precision_doc_convert_to_bitmap,
    dr_precision_doc_convert_to_bitmap_plan, dr_precision_doc_open_bitmap,
    dr_precision_doc_open_bitmap_psd,
};
use doctored_core::ffi_duotone::{dr_precision_doc_set_duotone, dr_precision_doc_set_duotone_plan};
use doctored_core::ffi_gray::{dr_precision_doc_new_gray, dr_precision_doc_open_gray};
use doctored_core::ffi_precision_metadata::dr_precision_doc_set_resolution;
use doctored_core::ffi_retouch::dr_precision_doc_retouch_from;
use doctored_core::precision_document::PrecisionDocument;
use serde_json::{json, Value};
use tempfile::TempDir;

use crate::common::{cpath, take_err_string};
use crate::gray_fixture::{png, srgb_chunk};
use crate::indexed_fixture::{descriptor, free, layer16, paint, render16, rgb_doc, LIMIT};

const FLATTEN: &str = "Bitmap holds one layer: flatten the image.";

/// An 8-bit Grayscale document of `codes` (row-major), opened from a gray
/// PNG tagged sRGB.
pub(crate) fn gray_doc(width: u32, height: u32, codes: &[u8]) -> *mut PrecisionDocument {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("g.png");
    let samples: Vec<u16> = codes.iter().map(|&c| u16::from(c)).collect();
    std::fs::write(
        &path,
        png(width, height, 8, 0, 1, &samples, &[srgb_chunk()]),
    )
    .unwrap();
    let c = cpath(&path);
    let working = CString::new("gray_gamma_22").unwrap();
    let mut err = ptr::null_mut();
    let doc = unsafe {
        dr_precision_doc_open_gray(
            c.as_ptr(),
            working.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!doc.is_null(), "{}", take_err_string(err));
    doc
}

fn json_of(text: *mut std::ffi::c_char, err: *mut std::ffi::c_char) -> Value {
    assert!(!text.is_null(), "{}", take_err_string(err));
    let value =
        serde_json::from_str(unsafe { std::ffi::CStr::from_ptr(text) }.to_str().unwrap()).unwrap();
    unsafe { dr_string_free(text) };
    value
}

fn result(
    out: *mut PrecisionDocument,
    err: *mut std::ffi::c_char,
) -> Result<*mut PrecisionDocument, String> {
    if out.is_null() {
        Err(take_err_string(err))
    } else {
        Ok(out)
    }
}

/// Image ▸ Mode ▸ Bitmap of `doc` under `options`, or its error.
pub(crate) fn to_bitmap(
    doc: *const PrecisionDocument,
    options: &Value,
) -> Result<*mut PrecisionDocument, String> {
    let text = CString::new(options.to_string()).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_convert_to_bitmap(doc, text.as_ptr(), LIMIT, 0, ptr::null(), &mut err)
    };
    result(out, err)
}

/// Image ▸ Mode ▸ Bitmap's plan, or its error.
pub(crate) fn plan(doc: *const PrecisionDocument, options: &Value) -> Result<Value, String> {
    let text = CString::new(options.to_string()).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe { dr_precision_doc_convert_to_bitmap_plan(doc, text.as_ptr(), &mut err) };
    if out.is_null() {
        return Err(take_err_string(err));
    }
    Ok(json_of(out, err))
}

/// Image ▸ Mode ▸ Grayscale of a Bitmap `doc` under `options`, or its error.
pub(crate) fn to_gray(
    doc: *const PrecisionDocument,
    options: &Value,
) -> Result<*mut PrecisionDocument, String> {
    let text = CString::new(options.to_string()).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_convert_bitmap_to_gray(doc, text.as_ptr(), LIMIT, 0, ptr::null(), &mut err)
    };
    result(out, err)
}

fn gray_plan(doc: *const PrecisionDocument, options: &Value) -> Value {
    let text = CString::new(options.to_string()).unwrap();
    let mut err = ptr::null_mut();
    json_of(
        unsafe { dr_precision_doc_convert_bitmap_to_gray_plan(doc, text.as_ptr(), &mut err) },
        err,
    )
}

/// The composite as bits, row-major: 1 white, 0 black; any other sample
/// fails.
pub(crate) fn bits(doc: *const PrecisionDocument) -> Vec<u8> {
    render16(doc)
        .into_iter()
        .map(|p| match p {
            [65535, 65535, 65535, 65535] => 1,
            [0, 0, 0, 65535] => 0,
            other => panic!("not a Bitmap sample: {other:?}"),
        })
        .collect()
}

/// A u16 sample's 8-bit code.
fn code8(v: u16) -> u8 {
    ((u32::from(v) + 128) / 257) as u8
}

/// The one layer's 8-bit grays (R; R = G = B asserted), row-major.
fn grays(doc: *const PrecisionDocument) -> Vec<u8> {
    layer16(doc)
        .into_iter()
        .map(|p| {
            assert!(p[0] == p[1] && p[1] == p[2] && p[3] == 65535, "{p:?}");
            code8(p[0])
        })
        .collect()
}

/// A test-local Sierra Lite onto {0, 255}, serpentine (the oracle of phase
/// 2b's shipped Diffusion Dither, written from Frankie Sierra's 1989
/// three-tap kernel, not from the core): even rows left to right, odd rows
/// right to left; `w = g + e`, unclamped, white iff `w ≥ 128`; the error
/// `w − {0, 255}` spread 2/4 to the next pixel along the walk, 1/4 below the
/// previous one and 1/4 below, dropped past the edges.
fn sierra_lite_serpentine(width: usize, height: usize, grays: &[u8]) -> Vec<u8> {
    let mut error = vec![0.0f64; width * height];
    let mut out = vec![0u8; width * height];
    for y in 0..height {
        let backward = y % 2 == 1;
        for step in 0..width {
            let x = if backward { width - 1 - step } else { step };
            let i = y * width + x;
            let w = f64::from(grays[i]) + error[i];
            let white = w >= 128.0;
            out[i] = u8::from(white);
            let e = w - if white { 255.0 } else { 0.0 };
            let (next, previous) = if backward {
                (x.checked_sub(1), Some(x + 1).filter(|&p| p < width))
            } else {
                (Some(x + 1).filter(|&p| p < width), x.checked_sub(1))
            };
            if let Some(n) = next {
                error[y * width + n] += e * 2.0 / 4.0;
            }
            if y + 1 < height {
                if let Some(p) = previous {
                    error[(y + 1) * width + p] += e * 1.0 / 4.0;
                }
                error[(y + 1) * width + x] += e * 1.0 / 4.0;
            }
        }
    }
    out
}

#[test]
fn fifty_percent_threshold_makes_128_and_above_white() {
    let gray = gray_doc(5, 1, &[0, 127, 128, 129, 255]);
    let bitmap = to_bitmap(gray, &json!({"method":"threshold"})).unwrap();
    assert_eq!(bits(bitmap), vec![0, 0, 1, 1, 1]);
    let d = descriptor(bitmap);
    assert_eq!(
        (
            d["color_mode"].clone(),
            d["layer_count"].clone(),
            d["channel_count"].clone(),
            d["gray"]["depth"].clone(),
            d["supported_operations"].clone()
        ),
        (
            json!("bitmap"),
            json!(1),
            json!(0),
            json!(8),
            json!([
                "render",
                "sample",
                "convert",
                "export",
                "paint",
                "erase",
                "fill_stroke",
                "geometry_crop",
                "geometry_canvas_size",
                "geometry_rotate",
                "geometry_flip",
                "clipboard_cut",
                "geometry_resize",
                "geometry_straighten",
                "geometry_affine",
                "geometry_perspective",
                "geometry_affine_set",
                "geometry_perspective_set",
                "selected_pixels_move",
                "clipboard_copy",
                "layer_name",
                "layer_label",
                "layer_lock",
                "selection",
                "clone",
                "histogram"
            ])
        )
    );
    let doc = unsafe { &*bitmap };
    assert_eq!(doc.layer(0).unwrap().name(), "Bitmap");
    assert_eq!(
        doc.profile().bytes(),
        unsafe { &*gray }.profile().bytes(),
        "the Gray twin carries"
    );
    free(bitmap);
    free(gray);
}

#[test]
fn diffusion_matches_a_hand_worked_four_by_four() {
    // Rows of 100 then rows of 150, Sierra Lite serpentine (phase 2b): each
    // cell w = g + e (unclamped), white iff w ≥ 128, the error w − {0, 255}
    // spread 2/4 ahead, 1/4 below-behind, 1/4 below; odd rows walk x 3 → 0.
    // y0 (→): 100 → 0 (e 100); 150 → 1 (−105); 47.5 → 0 (47.5);
    //     123.75 → 0 (123.75)
    // y1 (←): x3 130.94 → 1 (−124.06); x2 80.78 → 0 (80.78);
    //     x1 126.02 → 0 (126.02); x0 161.76 → 1 (−93.24)
    // y2 (→): 126.69 → 0 (126.69); 221.54 → 1 (−33.46); 184.97 → 1
    //     (−70.03); 104.16 → 0 (104.16)
    // y3 (←): x3 176.04 → 1 (−78.96); x2 119.05 → 0 (119.05);
    //     x1 183.65 → 1 (−71.35); x0 137.63 → 1
    let mut codes = vec![100u8; 8];
    codes.extend([150u8; 8]);
    let gray = gray_doc(4, 4, &codes);
    let bitmap = to_bitmap(gray, &json!({"method":"diffusion"})).unwrap();
    #[rustfmt::skip]
    let expected = vec![
        0, 1, 0, 0,
        1, 0, 0, 1,
        0, 1, 1, 0,
        1, 1, 0, 1,
    ];
    assert_eq!(bits(bitmap), expected);
    // Diffusion is the default method.
    let default = to_bitmap(gray, &json!({})).unwrap();
    assert_eq!(bits(default), expected);
    for d in [default, bitmap, gray] {
        free(d);
    }
}

#[test]
fn diffusion_matches_sierra_lite_serpentine_on_a_ramp() {
    let (w, h) = (16usize, 9usize);
    let codes: Vec<u8> = (0..h)
        .flat_map(|y| (0..w).map(move |x| (x * 16 + y * 3).min(255) as u8))
        .collect();
    let gray = gray_doc(w as u32, h as u32, &codes);
    let bitmap = to_bitmap(gray, &json!({"method":"diffusion"})).unwrap();
    assert_eq!(bits(bitmap), sierra_lite_serpentine(w, h, &codes));
    free(bitmap);
    free(gray);
}

#[test]
fn the_output_resolution_resamples_by_bicubic_then_applies_the_method() {
    let gray = gray_doc(2, 2, &[0, 255, 255, 0]);
    assert_eq!(
        descriptor(gray)["resolution"],
        json!({"x_ppi":72.0,"y_ppi":72.0})
    );
    let p = plan(gray, &json!({"method":"threshold","resolution":144})).unwrap();
    assert_eq!(
        (p["width"].clone(), p["height"].clone()),
        (json!(4), json!(4))
    );
    let bitmap = to_bitmap(gray, &json!({"method":"threshold","resolution":144})).unwrap();
    let d = descriptor(bitmap);
    assert_eq!(
        (
            d["width"].clone(),
            d["height"].clone(),
            d["resolution"].clone()
        ),
        (json!(4), json!(4), json!({"x_ppi":144.0,"y_ppi":144.0}))
    );
    free(bitmap);
    free(gray);

    // An 8×4 ramp at 72 → 108 ppi is 12×6: the FFI's Bicubic resize, then
    // the threshold.
    let codes: Vec<u8> = (0..4)
        .flat_map(|y| (0..8).map(move |x| (x * 36 + y * 5) as u8))
        .collect();
    let gray = gray_doc(8, 4, &codes);
    let resized = paint(
        gray,
        &json!({"op":"resize","width":12,"height":6,"filter":"bicubic"}),
        &[],
    )
    .unwrap()
    .unwrap();
    let expected: Vec<u8> = grays(resized)
        .into_iter()
        .map(|g| u8::from(g >= 128))
        .collect();
    let bitmap = to_bitmap(gray, &json!({"method":"threshold","resolution":108})).unwrap();
    assert_eq!(unsafe { &*bitmap }.dimensions(), (12, 6));
    assert_eq!(bits(bitmap), expected);
    for d in [bitmap, resized, gray] {
        free(d);
    }
}

#[test]
fn an_anisotropic_document_keeps_its_pixels_and_both_resolutions() {
    // 3 × 50, tall enough that 196 → 204 ppi changes the y axis:
    // round(50 · 204 / 196) = round(52.04) = 52.
    let codes: Vec<u8> = (0..150u32).map(|i| (i * 37 % 256) as u8).collect();
    let gray = gray_doc(3, 50, &codes);
    let mut err = ptr::null_mut();
    let skewed = unsafe { dr_precision_doc_set_resolution(gray, 204.0, 196.0, &mut err) };
    assert!(!skewed.is_null(), "{}", take_err_string(err));
    let bitmap = to_bitmap(skewed, &json!({"method":"threshold"})).unwrap();
    let d = descriptor(bitmap);
    assert_eq!(
        (
            d["width"].clone(),
            d["height"].clone(),
            d["resolution"].clone()
        ),
        (json!(3), json!(50), json!({"x_ppi":204.0,"y_ppi":196.0}))
    );
    let expected: Vec<u8> = codes.iter().map(|&g| u8::from(g >= 128)).collect();
    assert_eq!(bits(bitmap), expected);
    // A resolution equal to one axis keeps that axis and resamples the other.
    let p = plan(skewed, &json!({"resolution":204})).unwrap();
    assert_eq!(
        (p["width"].clone(), p["height"].clone()),
        (json!(3), json!(52))
    );
    let resampled = to_bitmap(skewed, &json!({"method":"threshold","resolution":204})).unwrap();
    let d = descriptor(resampled);
    assert_eq!(
        (
            d["width"].clone(),
            d["height"].clone(),
            d["resolution"].clone()
        ),
        (json!(3), json!(52), json!({"x_ppi":204.0,"y_ppi":204.0}))
    );
    for d in [resampled, bitmap, skewed, gray] {
        free(d);
    }
}

#[test]
fn the_output_resolution_takes_one_to_thirty_thousand_ppi() {
    let gray = gray_doc(2, 2, &[0, 255, 255, 0]);
    let p = plan(gray, &json!({"resolution":30000})).unwrap();
    assert_eq!(p["refusal"], json!(null));
    assert_eq!(p["width"], json!(833));
    let refusal = "mode option resolution must be 1 to 30000 pixels per inch";
    assert_eq!(
        plan(gray, &json!({"resolution":30001})).unwrap_err(),
        refusal
    );
    assert_eq!(plan(gray, &json!({"resolution":0.5})).unwrap_err(), refusal);
    assert_eq!(
        to_bitmap(gray, &json!({"resolution":30001})).unwrap_err(),
        refusal
    );
    free(gray);
}

#[test]
fn size_ratio_two_averages_each_block_by_hand() {
    // 1 0 1 / 0 0 1 / 1 1 0 at ratio 2: four blocks, the right and bottom
    // ones partial. (0,0): 1 white of 4 → (510 + 4) / 8 = 64 (255/4 = 63.75
    // rounded); (1,0): 2 of 2 → 255; (0,1): 2 of 2 → 255; (1,1): 0 of 1 → 0.
    let gray = gray_doc(3, 3, &[255, 0, 255, 0, 0, 255, 255, 255, 0]);
    let bitmap = to_bitmap(gray, &json!({"method":"threshold"})).unwrap();
    assert_eq!(bits(bitmap), vec![1, 0, 1, 0, 0, 1, 1, 1, 0]);
    let p = gray_plan(bitmap, &json!({"size_ratio":2}));
    assert_eq!(
        (
            p["width"].clone(),
            p["height"].clone(),
            p["refusal"].clone()
        ),
        (json!(2), json!(2), json!(null))
    );
    let back = to_gray(bitmap, &json!({"size_ratio":2})).unwrap();
    let d = descriptor(back);
    assert_eq!(
        (
            d["color_mode"].clone(),
            d["width"].clone(),
            d["height"].clone(),
            d["resolution"].clone(),
            d["layer_count"].clone(),
            d["gray"]["depth"].clone()
        ),
        (
            json!("gray"),
            json!(2),
            json!(2),
            json!({"x_ppi":36.0,"y_ppi":36.0}),
            json!(1),
            json!(8)
        )
    );
    assert_eq!(grays(back), vec![64, 255, 255, 0]);
    assert!(to_gray(bitmap, &json!({"size_ratio":17}))
        .unwrap_err()
        .contains("size_ratio"));
    assert!(to_gray(bitmap, &json!({"ratio":2}))
        .unwrap_err()
        .contains("unknown key"));
    for d in [back, bitmap, gray] {
        free(d);
    }
}

#[test]
fn size_ratio_one_is_the_gray_storage_exactly() {
    let gray = gray_doc(4, 2, &[10, 200, 128, 127, 0, 255, 90, 160]);
    let bitmap = to_bitmap(gray, &json!({"method":"threshold"})).unwrap();
    let back = to_gray(bitmap, &json!({})).unwrap();
    let d = descriptor(back);
    assert_eq!(d["color_mode"], json!("gray"));
    assert_eq!(render16(back), render16(bitmap));
    assert_eq!(grays(back), vec![0, 255, 255, 0, 0, 255, 0, 255]);
    for d in [back, bitmap, gray] {
        free(d);
    }
}

fn new_gray16() -> *mut PrecisionDocument {
    let request = CString::new(
        json!({"depth":16,"space":"sgray","fill_srgb":[0.25,0.25,0.25,1.0],
               "layer_name":"Background","ppi":72})
        .to_string(),
    )
    .unwrap();
    let mut err = ptr::null_mut();
    let doc = unsafe {
        dr_precision_doc_new_gray(2, 2, ptr::null(), 0, request.as_ptr(), LIMIT, 0, &mut err)
    };
    assert!(!doc.is_null(), "{}", take_err_string(err));
    doc
}

#[test]
fn the_hard_refusals_name_the_route() {
    let gray = gray_doc(2, 1, &[0, 255]);
    let bitmap = to_bitmap(gray, &json!({})).unwrap();
    assert_eq!(
        to_bitmap(bitmap, &json!({})).unwrap_err(),
        "The document is already in Bitmap mode."
    );
    let rgb = rgb_doc(1, 1, &[[1, 2, 3, 255]]);
    let gray_first = "Bitmap converts an 8-bit Grayscale document; convert to \
                      Grayscale first.";
    assert_eq!(to_bitmap(rgb, &json!({})).unwrap_err(), gray_first);
    assert_eq!(plan(rgb, &json!({})).unwrap()["refusal"], json!(gray_first));
    let gray16 = new_gray16();
    let eight_first = "Bitmap converts an 8-bit Grayscale document; convert to 8 \
                       Bits/Channel first.";
    assert_eq!(to_bitmap(gray16, &json!({})).unwrap_err(), eight_first);
    assert_eq!(
        plan(gray16, &json!({})).unwrap()["refusal"],
        json!(eight_first)
    );
    assert_eq!(
        to_gray(gray, &json!({})).unwrap_err(),
        "the document is not a Bitmap document"
    );
    assert_eq!(
        gray_plan(gray, &json!({}))["refusal"],
        json!("the document is not a Bitmap document")
    );
    assert!(to_bitmap(gray, &json!({"bogus":1}))
        .unwrap_err()
        .contains("unknown key"));
    for d in [gray16, rgb, bitmap, gray] {
        free(d);
    }
}

/// Bitmap and Duotone are both Gray storage underneath: the direct Bitmap
/// conversion refuses a Duotone document (Duotone → Bitmap is
/// `dr_precision_doc_convert_duotone_to_bitmap`, `duotone_close.rs`), and
/// Duotone refuses a Bitmap document.
#[test]
fn bitmap_and_duotone_do_not_convert_into_each_other() {
    let gray = gray_doc(2, 1, &[0, 255]);
    let options = CString::new(
        json!({"inks":[{"name":"Black","lab":[0,0,0],
                        "curve":[0,null,null,null,null,null,null,null,null,null,null,null,100]}]})
        .to_string(),
    )
    .unwrap();
    let mut err = ptr::null_mut();
    let duotone = unsafe { dr_precision_doc_set_duotone(gray, options.as_ptr(), &mut err) };
    assert!(!duotone.is_null(), "{}", take_err_string(err));
    let gray_first = "Bitmap converts an 8-bit Grayscale document; convert to \
                      Grayscale first.";
    assert_eq!(to_bitmap(duotone, &json!({})).unwrap_err(), gray_first);
    assert_eq!(
        plan(duotone, &json!({})).unwrap()["refusal"],
        json!(gray_first)
    );

    let bitmap = to_bitmap(gray, &json!({})).unwrap();
    let duotone_first = "Duotone converts an 8- or 16-bit Grayscale document; \
                         convert to Grayscale first.";
    let mut err = ptr::null_mut();
    let out = unsafe { dr_precision_doc_set_duotone(bitmap, options.as_ptr(), &mut err) };
    assert!(out.is_null());
    assert_eq!(take_err_string(err), duotone_first);
    let mut err = ptr::null_mut();
    let reply = json_of(
        unsafe { dr_precision_doc_set_duotone_plan(bitmap, options.as_ptr(), &mut err) },
        err,
    );
    assert_eq!(reply["refusal"], json!(duotone_first));
    for d in [bitmap, duotone, gray] {
        free(d);
    }
}

#[test]
fn more_than_one_plain_layer_must_flatten() {
    let gray = gray_doc(2, 2, &[0, 200, 60, 255]);
    let layered = paint(
        gray,
        &json!({"op":"add_layer","name":"Top","anchor":0,"position":"above"}),
        &[],
    )
    .unwrap()
    .unwrap();
    let options = json!({"method":"threshold"});
    let p = plan(layered, &options).unwrap();
    assert_eq!(
        (
            p["flatten_required"].clone(),
            p["refusal"].clone(),
            p["flattens"].clone()
        ),
        (json!(true), json!(FLATTEN), json!(false))
    );
    assert_eq!(to_bitmap(layered, &options).unwrap_err(), FLATTEN);
    let flatten = json!({"method":"threshold","flatten":true});
    let p = plan(layered, &flatten).unwrap();
    assert_eq!(
        (p["refusal"].clone(), p["flattens"].clone()),
        (json!(null), json!(true))
    );
    let bitmap = to_bitmap(layered, &flatten).unwrap();
    assert_eq!(descriptor(bitmap)["layer_count"], json!(1));
    // The new layer is empty, so the flattened composite is the gray.
    assert_eq!(bits(bitmap), vec![0, 1, 0, 1]);
    for d in [bitmap, layered, gray] {
        free(d);
    }
}

#[test]
fn every_edit_the_gate_does_not_admit_refuses_by_the_modes_sentence() {
    let gray = gray_doc(2, 2, &[0, 200, 60, 255]);
    let bitmap = to_bitmap(gray, &json!({})).unwrap();
    let refused = |request: Value| paint(bitmap, &request, &[]).unwrap_err();
    let unoffered = |op: &str| {
        format!(
            "{op} is unavailable in a Bitmap document: Doctored offers it on Grayscale and \
             colour documents only; convert to Grayscale first."
        )
    };
    assert_eq!(
        refused(json!({"op":"add_layer","name":"New","anchor":0,"position":"above"})),
        "add_layer is not available in Bitmap mode."
    );
    // Phase 3: the one-layer ops refuse by the one-layer reason.
    assert_eq!(
        refused(json!({"op":"flatten"})),
        "flatten needs more than one layer, and a Bitmap document holds one."
    );
    // The controller's ruling on phase 2b's walk: the filters refuse for
    // good (Lens Blur, the one Filter leaf enabled, is not built here).
    assert_eq!(
        refused(json!({"op":"filter","layer":0})),
        "filter is not available in Bitmap mode."
    );
    // Phase 2b's walk: Assign and Convert to Profile are disabled in Bitmap.
    assert_eq!(
        refused(json!({"op":"color_convert","layer":0})),
        "color_convert is not available in Bitmap mode."
    );
    assert_eq!(
        refused(json!({"op":"retouch_brush","layer":0})),
        unoffered("retouch_brush")
    );
    assert!(paint(bitmap, &json!("not an object"), &[]).is_err());
    // The retouch-from-plates export asks the same gate first: the gate
    // admits the Clone Stamp (phase 3), and a retouch from another state is
    // Content-Aware Move's, which a Bitmap document is not offered.
    let text = CString::new(json!({"op":"retouch_clone","layer":0}).to_string()).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_retouch_from(
            bitmap,
            bitmap,
            text.as_ptr(),
            ptr::null(),
            0,
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(out.is_null());
    assert_eq!(take_err_string(err), unoffered("content_aware_move"));
    free(bitmap);
    free(gray);
}

#[test]
fn every_bitmap_export_tolerates_null() {
    unsafe {
        let mut err = ptr::null_mut();
        assert!(
            dr_precision_doc_convert_to_bitmap_plan(ptr::null(), ptr::null(), &mut err).is_null()
        );
        assert!(!err.is_null());
        dr_string_free(err);
        assert!(
            dr_precision_doc_convert_to_bitmap_plan(ptr::null(), ptr::null(), ptr::null_mut())
                .is_null()
        );
        err = ptr::null_mut();
        assert!(dr_precision_doc_convert_to_bitmap(
            ptr::null(),
            ptr::null(),
            LIMIT,
            0,
            ptr::null(),
            &mut err
        )
        .is_null());
        assert!(!err.is_null());
        dr_string_free(err);
        assert!(dr_precision_doc_convert_to_bitmap(
            ptr::null(),
            ptr::null(),
            LIMIT,
            0,
            ptr::null(),
            ptr::null_mut()
        )
        .is_null());
        err = ptr::null_mut();
        assert!(
            dr_precision_doc_convert_bitmap_to_gray_plan(ptr::null(), ptr::null(), &mut err)
                .is_null()
        );
        assert!(!err.is_null());
        dr_string_free(err);
        assert!(dr_precision_doc_convert_bitmap_to_gray_plan(
            ptr::null(),
            ptr::null(),
            ptr::null_mut()
        )
        .is_null());
        err = ptr::null_mut();
        assert!(dr_precision_doc_convert_bitmap_to_gray(
            ptr::null(),
            ptr::null(),
            LIMIT,
            0,
            ptr::null(),
            &mut err
        )
        .is_null());
        assert!(!err.is_null());
        dr_string_free(err);
        assert!(dr_precision_doc_convert_bitmap_to_gray(
            ptr::null(),
            ptr::null(),
            LIMIT,
            0,
            ptr::null(),
            ptr::null_mut()
        )
        .is_null());
        // Phase 1c's three: the source test answers 0, the opens NULL.
        assert_eq!(dr_path_bitmap_source(ptr::null()), 0);
        err = ptr::null_mut();
        assert!(dr_precision_doc_open_bitmap(
            ptr::null(),
            ptr::null(),
            LIMIT,
            0,
            ptr::null(),
            &mut err
        )
        .is_null());
        assert!(!err.is_null());
        dr_string_free(err);
        assert!(dr_precision_doc_open_bitmap(
            ptr::null(),
            ptr::null(),
            LIMIT,
            0,
            ptr::null(),
            ptr::null_mut()
        )
        .is_null());
        err = ptr::null_mut();
        assert!(dr_precision_doc_open_bitmap_psd(
            ptr::null(),
            ptr::null(),
            ptr::null(),
            LIMIT,
            0,
            ptr::null(),
            &mut err
        )
        .is_null());
        assert!(!err.is_null());
        dr_string_free(err);
        assert!(dr_precision_doc_open_bitmap_psd(
            ptr::null(),
            ptr::null(),
            ptr::null(),
            LIMIT,
            0,
            ptr::null(),
            ptr::null_mut()
        )
        .is_null());
    }
}
