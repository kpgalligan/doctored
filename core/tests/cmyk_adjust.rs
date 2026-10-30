//! Levels and Curves on ink (#1 phase 2), destructive and as adjustment
//! records, each against an independent oracle written here: Levels in
//! closed form (the stage formula of `adjust_levels`' module doc) on
//! `L = 1 − ink`, Curves through a two-point line and a three-point natural
//! spline whose second derivative this file solves itself; an adjustment
//! record at an opacity and through a mask against the test's own lerp; a
//! destructive apply within a selection plane; the `.rz` round trip of a
//! painted, adjusted, masked document at word 83 (and a plain one at 82);
//! drawn (map) curves against the byte maps on every 8-bit code and
//! against the 15-bit stage tables written here from `adjust_curves`' module
//! doc at 16 bits, as a record at the root and inside a group; every new
//! refusal by its sentence. No test here sets or clears the process working CMYK profile.

use std::ffi::{CStr, CString};
use std::ptr;

use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_cmyk::{
    dr_precision_doc_cmyk_sample, dr_precision_doc_convert_cmyk_to_rgb,
    dr_precision_doc_convert_cmyk_to_rgb_plan, dr_precision_doc_convert_to_cmyk,
};
use doctored_core::ffi_insert::dr_precision_doc_add_adjustment_at;
use doctored_core::ffi_precision::{
    dr_precision_doc_descriptor, dr_precision_doc_free, dr_precision_doc_info,
    dr_precision_doc_open, dr_precision_doc_render, dr_precision_doc_save_native,
    dr_precision_image_free,
};
use doctored_core::ffi_precision_color::{
    dr_precision_doc_add_adjustment, dr_precision_doc_apply_adjustment,
    dr_precision_doc_set_adjustment,
};
use doctored_core::ffi_precision_edit::dr_precision_doc_edit;
use doctored_core::ffi_precision_inspect::dr_precision_doc_adjustment_description;
use doctored_core::ffi_selected_adjustment::dr_precision_doc_selected_edit;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::PrecisionImage;
use serde_json::Value;
use tempfile::TempDir;

use crate::cmyk_fixture::press_profile;
use crate::cmyk_structure_fixture::then;
use crate::cmyk_tiff_builder::cmyk_tiff;
use crate::common::{cpath, take_err_string};

const LIMIT: u64 = 2 * 1024 * 1024 * 1024;

fn free(document: *mut PrecisionDocument) {
    unsafe { dr_precision_doc_free(document) };
}

fn json(text: *mut std::ffi::c_char, err: *mut std::ffi::c_char) -> Value {
    assert!(!text.is_null(), "{}", take_err_string(err));
    let value = serde_json::from_str(unsafe { CStr::from_ptr(text) }.to_str().unwrap()).unwrap();
    unsafe { dr_string_free(text) };
    value
}

/// A CMYK document converted from u16 sRGB `pixels` into the press profile.
fn ink_doc(w: u32, h: u32, pixels: Vec<[u16; 4]>) -> *mut PrecisionDocument {
    let image = PrecisionImage::from_rgba16(w, h, pixels, IccProfile::srgb(), None).unwrap();
    let rgb = Box::into_raw(Box::new(
        PrecisionDocument::from_image(image, Metadata::default(), Resolution::default()).unwrap(),
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

fn picture(w: u32, h: u32, alpha: bool) -> Vec<[u16; 4]> {
    (0..w * h)
        .map(|i| {
            [
                (i * 7919 % 65536) as u16,
                (i * 104_729 % 65536) as u16,
                (i * 31_337 % 65536) as u16,
                if alpha {
                    [65535, 30000, 0][(i % 3) as usize]
                } else {
                    65535
                },
            ]
        })
        .collect()
}

fn samples(doc: *const PrecisionDocument, index: usize) -> Vec<[u16; 5]> {
    unsafe { &*doc }
        .cmyk_layer_samples(index)
        .expect("a CMYK record")
}

fn code(v: f64) -> u16 {
    (v.clamp(0.0, 1.0) * 65535.0).round() as u16
}

fn description(op: &str, params: &str) -> String {
    format!(r#"{{"type":"adjust","evaluation":"precision_v1","op":"{op}","params":{params}}}"#)
}

fn apply(
    doc: *const PrecisionDocument,
    index: usize,
    desc: &str,
) -> Result<*mut PrecisionDocument, String> {
    let text = CString::new(desc).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_apply_adjustment(
            doc,
            index,
            text.as_ptr(),
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

fn add(
    doc: *const PrecisionDocument,
    desc: &str,
    opacity: f32,
) -> Result<*mut PrecisionDocument, String> {
    let text = CString::new(desc).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_add_adjustment(
            doc,
            c"Tone".as_ptr(),
            text.as_ptr(),
            opacity,
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    if out.is_null() {
        Err(take_err_string(err))
    } else {
        Ok(out)
    }
}

fn edit(doc: *const PrecisionDocument, request: &str, input: &[u16]) -> *mut PrecisionDocument {
    let text = CString::new(request).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_edit(
            doc,
            text.as_ptr(),
            if input.is_empty() {
                ptr::null()
            } else {
                input.as_ptr()
            },
            input.len(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!out.is_null(), "{request}: {}", take_err_string(err));
    out
}

/// One Levels stage on `v` (`adjust_levels`' module doc).
#[derive(Clone, Copy)]
struct Stage {
    black: f64,
    white: f64,
    gamma: f64,
    out_black: f64,
    out_white: f64,
}

const IDENTITY: Stage = Stage {
    black: 0.0,
    white: 1.0,
    gamma: 1.0,
    out_black: 0.0,
    out_white: 1.0,
};

impl Stage {
    fn at(&self, v: f64) -> f64 {
        let t = ((v - self.black) / (self.white - self.black))
            .clamp(0.0, 1.0)
            .powf(1.0 / self.gamma);
        self.out_black + (self.out_white - self.out_black) * t
    }
}

#[test]
fn levels_on_ink_is_the_closed_form_on_l() {
    let (w, h) = (8u32, 6u32);
    let doc = ink_doc(w, h, picture(w, h, true));
    let before = samples(doc, 0);
    let params = r#"{"black":0.1,"white":0.9,"gamma":1.4,"output_black":0.05,"output_white":0.95,"c":{"black":0.2,"gamma":0.8},"k":{"white":0.7,"output_black":0.1}}"#;
    let master = Stage {
        black: 0.1,
        white: 0.9,
        gamma: 1.4,
        out_black: 0.05,
        out_white: 0.95,
    };
    let inks = [
        Stage {
            black: 0.2,
            gamma: 0.8,
            ..IDENTITY
        },
        IDENTITY,
        IDENTITY,
        Stage {
            white: 0.7,
            out_black: 0.1,
            ..IDENTITY
        },
    ];
    let out = apply(doc, 0, &description("levels", params)).unwrap();
    for (i, (s, g)) in before.iter().zip(samples(out, 0)).enumerate() {
        if s[4] == 0 {
            assert_eq!(*s, g, "pixel {i} at alpha 0 is untouched");
            continue;
        }
        for c in 0..4 {
            let l = 1.0 - f64::from(s[c]) / 65535.0;
            let want = code(1.0 - master.at(inks[c].at(l)));
            assert!(
                g[c].abs_diff(want) <= 1,
                "pixel {i} ink {c}: {} vs {want}",
                g[c]
            );
        }
        assert_eq!(g[4], s[4], "alpha is kept");
    }
    free(out);
    free(doc);
}

/// The natural cubic spline through `points` (sorted, distinct x), solved
/// here: the tridiagonal system for the interior second derivatives with
/// `M = 0` at both ends; flat beyond the ends, clamped to 0–255 inside.
fn natural(points: &[(f64, f64)], x: f64) -> f64 {
    let n = points.len();
    if x <= points[0].0 {
        return points[0].1;
    }
    if x >= points[n - 1].0 {
        return points[n - 1].1;
    }
    let h: Vec<f64> = (0..n - 1).map(|k| points[k + 1].0 - points[k].0).collect();
    let d: Vec<f64> = (0..n - 1)
        .map(|k| (points[k + 1].1 - points[k].1) / h[k])
        .collect();
    let mut m = vec![0.0; n];
    if n == 3 {
        // One interior unknown: 2(h0 + h1)·M1 = 6(d1 − d0).
        m[1] = 6.0 * (d[1] - d[0]) / (2.0 * (h[0] + h[1]));
    }
    assert!(n <= 3, "this oracle solves two- and three-point curves");
    let k = (0..n - 1).find(|&k| x <= points[k + 1].0).unwrap();
    let (x0, y0) = points[k];
    let (x1, y1) = points[k + 1];
    let a = (x1 - x) / h[k];
    let b = (x - x0) / h[k];
    (a * y0 + b * y1 + ((a.powi(3) - a) * m[k] + (b.powi(3) - b) * m[k + 1]) * h[k] * h[k] / 6.0)
        .clamp(0.0, 255.0)
}

#[test]
fn curves_on_ink_follow_a_line_and_a_natural_spline() {
    let (w, h) = (9u32, 5u32);
    let doc = ink_doc(w, h, picture(w, h, false));
    let before = samples(doc, 0);
    let composite = [(0.0, 20.0), (255.0, 230.0)];
    let cyan = [(0.0, 0.0), (100.0, 160.0), (255.0, 255.0)];
    let yellow = [(40.0, 10.0), (200.0, 250.0)];
    let params =
        r#"{"cmyk":[[0,20],[255,230]],"c":[[0,0],[100,160],[255,255]],"y":[[40,10],[200,250]]}"#;
    let out = apply(doc, 0, &description("curves", params)).unwrap();
    for (i, (s, g)) in before.iter().zip(samples(out, 0)).enumerate() {
        for c in 0..4 {
            let x = f64::from(s[c]) / 65535.0 * 255.0;
            let x = match c {
                0 => natural(&cyan, x),
                2 => natural(&yellow, x),
                _ => x,
            };
            let want = code(natural(&composite, x) / 255.0);
            assert!(
                g[c].abs_diff(want) <= 1,
                "pixel {i} ink {c}: {} vs {want}",
                g[c]
            );
        }
    }
    // The identity curve is an exact no-op.
    let identity = description("curves", r#"{"cmyk":[[0,0],[128,128],[255,255]]}"#);
    assert!(apply(doc, 0, &identity).unwrap().is_null());
    free(out);
    free(doc);
}

fn sample_ink(doc: *const PrecisionDocument, x: i32, y: i32) -> [u16; 5] {
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

#[test]
fn an_adjustment_record_lerps_the_composite_by_opacity_and_mask() {
    let (w, h) = (6u32, 4u32);
    let doc = ink_doc(w, h, picture(w, h, true));
    let low = samples(doc, 0);
    let curve = [(0.0, 40.0), (255.0, 200.0)];
    let desc = description("curves", r#"{"cmyk":[[0,40],[255,200]]}"#);
    let layered = add(doc, &desc, 0.6).unwrap();
    let mut err = ptr::null_mut();
    let info = json(unsafe { dr_precision_doc_info(layered, &mut err) }, err);
    assert_eq!(info["layers"][1]["kind"], "adjustment");
    assert_eq!(info["layers"][1]["adjustment"]["operation"], "curves");
    let plane: Vec<u16> = (0..w * h).map(|i| (i * 2731 % 65536) as u16).collect();
    let set = format!(r#"{{"op":"set_mask","layer":1,"width":{w},"height":{h}}}"#);
    let masked = edit(layered, &set, &plane);
    for y in 0..h {
        for x in 0..w {
            let i = (y * w + x) as usize;
            let got = sample_ink(masked, x as i32, y as i32);
            if low[i][4] == 0 {
                assert_eq!(got, [0; 5], "({x},{y}): nothing to adjust");
                continue;
            }
            let k = 0.6 * f64::from(plane[i]) / 65535.0;
            for c in 0..4 {
                let ink = f64::from(low[i][c]) / 65535.0;
                let f = natural(&curve, ink * 255.0) / 255.0;
                let want = code(ink + (f - ink) * k);
                assert!(
                    got[c].abs_diff(want) <= 1,
                    "({x},{y}) ink {c}: {} vs {want}",
                    got[c]
                );
            }
            assert!(got[4].abs_diff(low[i][4]) <= 1, "alpha is kept");
        }
    }
    // Set: a new description replaces it; the same one is a no-op.
    let levels = description("levels", r#"{"gamma":2}"#);
    let text = CString::new(levels.as_str()).unwrap();
    let set = unsafe {
        dr_precision_doc_set_adjustment(masked, 1, text.as_ptr(), LIMIT, 0, ptr::null(), &mut err)
    };
    assert!(!set.is_null(), "{}", take_err_string(err));
    let read = json(
        unsafe { dr_precision_doc_adjustment_description(set, 1, &mut err) },
        err,
    );
    assert_eq!(read["op"], "levels");
    let again = unsafe {
        dr_precision_doc_set_adjustment(set, 1, text.as_ptr(), LIMIT, 0, ptr::null(), &mut err)
    };
    assert!(again.is_null() && err.is_null());
    // Added at a position: above the raster, below the curves record.
    let at = unsafe {
        dr_precision_doc_add_adjustment_at(
            masked,
            c"Levels".as_ptr(),
            text.as_ptr(),
            1.0,
            0,
            0,
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!at.is_null(), "{}", take_err_string(err));
    assert_eq!(unsafe { &*at }.layer(1).unwrap().name(), "Levels");
    for d in [layered, masked, set, at, doc] {
        free(d);
    }
}

#[test]
fn a_destructive_apply_lerps_by_the_selection() {
    let (w, h) = (7u32, 5u32);
    let n = (w * h) as usize;
    let doc = ink_doc(w, h, picture(w, h, true));
    let before = samples(doc, 0);
    let desc = description("levels", r#"{"output_white":0.5,"m":{"gamma":2.0}}"#);
    let selection: Vec<u16> = (0..n).map(|i| [0, 65535, 20000, 51000][i % 4]).collect();
    let request = serde_json::json!({"layer":0,"op":"adjustment","description":desc}).to_string();
    let text = CString::new(request).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_selected_edit(
            doc,
            text.as_ptr(),
            selection.as_ptr(),
            selection.len(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!out.is_null(), "{}", take_err_string(err));
    let master = Stage {
        out_white: 0.5,
        ..IDENTITY
    };
    let magenta = Stage {
        gamma: 2.0,
        ..IDENTITY
    };
    for (i, (s, g)) in before.iter().zip(samples(out, 0)).enumerate() {
        if s[4] == 0 || selection[i] == 0 {
            assert_eq!(*s, g, "pixel {i} is untouched");
            continue;
        }
        let t = f64::from(selection[i]) / 65535.0;
        for c in 0..4 {
            let ink = f64::from(s[c]) / 65535.0;
            let l = 1.0 - ink;
            let l = if c == 1 { magenta.at(l) } else { l };
            let f = 1.0 - master.at(l);
            let want = code(ink + (f - ink) * t);
            assert!(
                g[c].abs_diff(want) <= 1,
                "pixel {i} ink {c}: {} vs {want}",
                g[c]
            );
        }
    }
    // A filter a CMYK document does not offer refuses by name.
    let blur = CString::new(
        r#"{"layer":0,"op":"depth_blur","params":{"depth":{"width":2,"height":1,"samples":[0,1],"frame":[0,0,2,2]}}}"#,
    )
    .unwrap();
    let refused = unsafe {
        dr_precision_doc_selected_edit(
            doc,
            blur.as_ptr(),
            ptr::null(),
            0,
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(refused.is_null());
    assert_eq!(
        take_err_string(err),
        "Depth Blur is unavailable in a CMYK document: Doctored offers it on RGB documents only"
    );
    free(out);
    free(doc);
}

#[test]
fn every_tone_refusal_names_itself() {
    let doc = ink_doc(2, 2, picture(2, 2, false));
    let cases = [
        (
            description("levels", r#"{"r":{"gamma":2}}"#),
            "The RGB channel key \"r\" is unavailable in a CMYK document",
        ),
        (
            description("curves", r#"{"rgb":[[0,0],[255,255]]}"#),
            "The RGB channel key \"rgb\" is unavailable in a CMYK document",
        ),
        (
            description(
                "curves",
                &format!(r#"{{"rgb_map":{:?}}}"#, (0..256).collect::<Vec<u32>>()),
            ),
            "The RGB channel key \"rgb_map\" is unavailable in a CMYK document",
        ),
        (
            description(
                "curves",
                &format!(
                    r#"{{"k":[[0,0],[255,255]],"cmyk_map":{:?}}}"#,
                    (0..256).collect::<Vec<u32>>()
                ),
            ),
            "invalid CMYK curves parameters: a map key beside a point key",
        ),
        (
            description("curves", r#"{"c_map":[0,1,2]}"#),
            "invalid CMYK curves c_map",
        ),
        (
            description("brightness_contrast", r#"{"brightness":10}"#),
            "The \"brightness_contrast\" adjustment is unavailable in a CMYK document",
        ),
        (
            description("levels", r#"{"gamma":40}"#),
            "invalid CMYK levels composite parameters",
        ),
        (
            description("levels", r#"{"dither":1}"#),
            "unknown CMYK levels parameter dither",
        ),
    ];
    for (desc, sentence) in &cases {
        assert_eq!(add(doc, desc, 1.0).unwrap_err(), *sentence, "{desc}");
    }
    // Setting needs an adjustment record.
    let levels = CString::new(description("levels", r#"{"gamma":2}"#)).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_set_adjustment(doc, 0, levels.as_ptr(), LIMIT, 0, ptr::null(), &mut err)
    };
    assert!(out.is_null());
    assert_eq!(
        take_err_string(err),
        "setting a CMYK adjustment requires an adjustment layer"
    );
    // RGB Color needs a flatten past a CMYK adjustment record.
    let layered = add(doc, &description("levels", r#"{"gamma":2}"#), 1.0).unwrap();
    let blocker = "“Tone” is a CMYK adjustment layer, which RGB Color cannot carry; flatten the image to convert it";
    let plan = json(
        unsafe {
            dr_precision_doc_convert_cmyk_to_rgb_plan(
                layered,
                ptr::null(),
                0,
                c"{}".as_ptr(),
                &mut err,
            )
        },
        err,
    );
    assert_eq!(plan["flatten_required"], true);
    assert_eq!(plan["refusal"], blocker);
    let refused = unsafe {
        dr_precision_doc_convert_cmyk_to_rgb(
            layered,
            ptr::null(),
            0,
            c"{}".as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(refused.is_null());
    assert_eq!(take_err_string(err), blocker);
    let flattened = unsafe {
        dr_precision_doc_convert_cmyk_to_rgb(
            layered,
            ptr::null(),
            0,
            c"{\"flatten\":true}".as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!flattened.is_null(), "{}", take_err_string(err));
    assert_eq!(unsafe { &*flattened }.layer_count(), 1);
    for d in [layered, flattened, doc] {
        free(d);
    }
}

fn save(doc: *const PrecisionDocument, path: &std::path::Path) -> Vec<u8> {
    let c = cpath(path);
    let mut err = ptr::null_mut();
    assert!(
        unsafe { dr_precision_doc_save_native(doc, c.as_ptr(), LIMIT, 0, ptr::null(), &mut err) },
        "{}",
        take_err_string(err)
    );
    std::fs::read(path).unwrap()
}

fn open(path: &std::path::Path) -> *mut PrecisionDocument {
    let c = cpath(path);
    let mut err = ptr::null_mut();
    let doc = unsafe { dr_precision_doc_open(c.as_ptr(), LIMIT, 0, ptr::null(), &mut err) };
    assert!(!doc.is_null(), "open {path:?}: {}", take_err_string(err));
    doc
}

/// The version words after DRDC and 46, up to the body version (< 40).
fn words(bytes: &[u8]) -> Vec<u32> {
    let mut out = Vec::new();
    let mut at = 8;
    loop {
        let word = u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
        if word < 40 {
            return out;
        }
        out.push(word);
        at += 4;
    }
}

fn render(doc: *const PrecisionDocument) -> Vec<[u16; 4]> {
    let mut err = ptr::null_mut();
    let image = unsafe { dr_precision_doc_render(doc, LIMIT, 0, ptr::null(), &mut err) };
    assert!(!image.is_null(), "{}", take_err_string(err));
    let pixels = unsafe { &*image }.to_rgba16().unwrap();
    unsafe { dr_precision_image_free(image) };
    pixels
}

#[test]
fn the_native_save_carries_masks_and_tone_records_at_word_83() {
    let dir = TempDir::new().unwrap();
    let (w, h) = (6u32, 5u32);
    let n = (w * h) as usize;
    let doc = ink_doc(w, h, picture(w, h, true));
    // A plain CMYK document still writes word 82.
    assert_eq!(words(&save(doc, &dir.path().join("plain.rz")))[0], 82);
    // Paint, a second layer, Levels (output range and a cyan stage), Curves,
    // and masks on the second raster (hide-all outside) and on the Curves.
    let stroke: Vec<u16> = (0..n).map(|i| (i * 2003 % 65536) as u16).collect();
    let painted = edit(
        doc,
        r#"{"op":"paint_blend","layer":0,"ink":[0.3,0.1,0.8,0.2,0.9],"opacity":0.8}"#,
        &stroke,
    );
    let added = edit(
        painted,
        r#"{"op":"add_layer","name":"Two","anchor":0,"position":"above"}"#,
        &[],
    );
    let second = edit(
        added,
        r#"{"op":"paint_blend","layer":1,"ink":[0.9,0.6,0.0,0.4,1],"opacity":1}"#,
        &stroke,
    );
    let mask: Vec<u16> = (0..12).map(|i| (i * 5000) as u16).collect();
    let masked = edit(
        second,
        r#"{"op":"set_mask","layer":1,"width":4,"height":3,"x":1,"y":1,"outside_coverage":255}"#,
        &mask,
    );
    let levels = add(
        masked,
        &description(
            "levels",
            r#"{"output_black":0.1,"output_white":0.8,"c":{"gamma":1.6,"black":0.05}}"#,
        ),
        0.7,
    )
    .unwrap();
    let curves = add(
        levels,
        &description("curves", r#"{"k":[[0,0],[90,150],[255,255]]}"#),
        1.0,
    )
    .unwrap();
    let tone_mask: Vec<u16> = (0..n).map(|i| (i * 977 % 65536) as u16).collect();
    let set = format!(r#"{{"op":"set_mask","layer":3,"width":{w},"height":{h}}}"#);
    let full = edit(curves, &set, &tone_mask);
    let bytes = save(full, &dir.path().join("edited.rz"));
    let header = words(&bytes);
    assert_eq!(header[0], 83, "{header:?}");
    assert!(!header.contains(&62) && !header.contains(&64), "{header:?}");
    let again = open(&dir.path().join("edited.rz"));
    let (a, b) = (unsafe { &*full }, unsafe { &*again });
    assert_eq!(a.layer_count(), b.layer_count());
    for i in 0..a.layer_count() {
        let (la, lb) = (a.layer(i).unwrap(), b.layer(i).unwrap());
        assert_eq!(la.name(), lb.name());
        assert_eq!(la.opacity(), lb.opacity());
        assert_eq!(
            a.cmyk_layer_samples(i),
            b.cmyk_layer_samples(i),
            "record {i} samples"
        );
        assert_eq!(
            a.adjustment_description(i),
            b.adjustment_description(i),
            "record {i}"
        );
        match (la.mask(), lb.mask()) {
            (None, None) => {}
            (Some(ma), Some(mb)) => {
                assert_eq!(
                    ma.plane().samples(),
                    mb.plane().samples(),
                    "record {i} mask"
                );
                assert_eq!(ma.properties().offset, mb.properties().offset);
                assert_eq!(
                    ma.properties().outside_coverage,
                    mb.properties().outside_coverage
                );
            }
            _ => panic!("record {i}: the mask did not round trip"),
        }
    }
    assert_eq!(
        render(full),
        render(again),
        "the composite reopens bit-exact"
    );
    let mut err = ptr::null_mut();
    let descriptor = json(unsafe { dr_precision_doc_descriptor(again, &mut err) }, err);
    for key in [
        "cmyk_paint_scratch_bytes",
        "cmyk_gradient_scratch_bytes",
        "cmyk_bucket_scratch_bytes",
        "cmyk_adjust_apply_scratch_bytes",
        "cmyk_adjust_add_scratch_bytes",
        "cmyk_add_layer_scratch_bytes",
    ] {
        assert!(
            descriptor[key].as_u64().is_some_and(|v| v > 0),
            "{key}: {descriptor}"
        );
    }
    for d in [
        doc, painted, added, second, masked, levels, curves, full, again,
    ] {
        free(d);
    }
}

// ------------------------------------------------------------ drawn curves --

/// `R(n, d)`, the half-up integer ratio `adjust_curves`' module doc reads
/// Photoshop's 15-bit stage tables with.
fn ratio(n: i64, d: i64) -> i64 {
    (2 * n + d).div_euclid(2 * d)
}

/// One map's 15-bit stage table, written from `adjust_curves`' module doc.
fn stage(map: &[u8; 256]) -> Vec<i64> {
    let q: Vec<i64> = map
        .iter()
        .map(|&m| ratio(32768 * i64::from(m), 255))
        .collect();
    (0..=4096i64)
        .map(|j| {
            let i = (255 * j / 4096).min(254);
            let at = i as usize;
            q[at] + ratio((q[at + 1] - q[at]) * (255 * j - 4096 * i), 4096)
        })
        .collect()
}

/// `read(A, x)` on a 4097-node table (module doc).
fn read(table: &[i64], x: i64) -> i64 {
    let x = x.min(32768);
    let i = (x / 8).min(4095);
    let at = i as usize;
    table[at] + ratio((table[at + 1] - table[at]) * (x - 8 * i), 8)
}

/// A drawn curve on one 16-bit code: the composed table at the code's
/// 15-bit index, written back to 16 bits.
fn mapped16(master: &[u8; 256], channel: &[u8; 256], code: u16) -> u16 {
    let (m, c) = (stage(master), stage(channel));
    let composed: Vec<i64> = c.iter().map(|&v| read(&m, v)).collect();
    let k = ratio(32768 * i64::from(code), 65535);
    ratio(65535 * read(&composed, k), 32768) as u16
}

/// A lone pencil press (one entry moved, the rest the identity), a random
/// map and a zig-zag one: the sharp maps a drawn curve can hold.
fn drawn_maps() -> [[u8; 256]; 5] {
    let identity: [u8; 256] = std::array::from_fn(|i| i as u8);
    let mut spike = identity;
    spike[100] = 255;
    let mut seed = 0x2545_f491u32;
    let mut random = [0u8; 256];
    for v in &mut random {
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        *v = (seed >> 24) as u8;
    }
    let zigzag: [u8; 256] = std::array::from_fn(|i| (i + 40 * ((i / 8) % 2)).min(255) as u8);
    let mut master = identity;
    master[200] = 30;
    master[17] = 240;
    [master, spike, random, zigzag, identity]
}

fn map_params(maps: &[[u8; 256]; 5]) -> String {
    let list = |m: &[u8; 256]| m.iter().map(u8::to_string).collect::<Vec<_>>().join(",");
    format!(
        r#"{{"cmyk_map":[{}],"c_map":[{}],"m_map":[{}],"y_map":[{}],"k_map":[{}]}}"#,
        list(&maps[0]),
        list(&maps[1]),
        list(&maps[2]),
        list(&maps[3]),
        list(&maps[4])
    )
}

/// A CMYK document at `bits` from a TIFF of `samples` in the press profile.
fn tiff_doc(dir: &TempDir, w: u32, samples: &[[u16; 4]], bits: u16) -> *mut PrecisionDocument {
    let h = samples.len() as u32 / w;
    let data: Vec<u8> = match bits {
        8 => samples.iter().flatten().map(|v| (v / 257) as u8).collect(),
        _ => samples
            .iter()
            .flatten()
            .flat_map(|v| v.to_le_bytes())
            .collect(),
    };
    let path = dir.path().join(format!("maps-{bits}.tif"));
    let profile = press_profile("Press Maps");
    std::fs::write(&path, cmyk_tiff(w, h, bits, 4, 0, &data, Some(&profile))).unwrap();
    open(&path)
}

#[test]
fn a_drawn_curve_reads_the_byte_maps_on_every_code_at_8_bits() {
    let dir = TempDir::new().unwrap();
    // Every plate meets every 8-bit code once (7 and 13 are prime to 256).
    let samples: Vec<[u16; 4]> = (0..256u32)
        .map(|i| [i, i * 7 % 256, i * 13 % 256, 255 - i].map(|v| v as u16 * 257))
        .collect();
    let doc = tiff_doc(&dir, 16, &samples, 8);
    let maps = drawn_maps();
    let out = apply(doc, 0, &description("curves", &map_params(&maps))).unwrap();
    assert!(!out.is_null());
    for (i, (s, g)) in samples.iter().zip(samples_of(out)).enumerate() {
        for p in 0..4 {
            let v = usize::from(s[p] / 257);
            let want = u16::from(maps[0][usize::from(maps[1 + p][v])]) * 257;
            assert_eq!(g[p], want, "pixel {i} plate {p} at code {v}");
        }
    }
    free(out);
    free(doc);
}

fn samples_of(doc: *const PrecisionDocument) -> Vec<[u16; 5]> {
    samples(doc, 0)
}

#[test]
fn a_drawn_curve_reads_the_composed_stage_tables_at_16_bits() {
    let dir = TempDir::new().unwrap();
    let samples: Vec<[u16; 4]> = (0..512u32)
        .map(|i| {
            [
                i * 128 + 3,
                i * 7919 % 65536,
                65535 - i * 97,
                i * 31_337 % 65536,
            ]
            .map(|v| v as u16)
        })
        .collect();
    let doc = tiff_doc(&dir, 32, &samples, 16);
    let maps = drawn_maps();
    let out = apply(doc, 0, &description("curves", &map_params(&maps))).unwrap();
    assert!(!out.is_null());
    for (i, (s, g)) in samples.iter().zip(samples_of(out)).enumerate() {
        for p in 0..4 {
            let want = mapped16(&maps[0], &maps[1 + p], s[p]);
            assert_eq!(g[p], want, "pixel {i} plate {p} at code {}", s[p]);
        }
    }
    free(out);
    free(doc);
}

#[test]
fn a_drawn_curve_record_renders_its_result_at_the_root_and_in_a_group() {
    let dir = TempDir::new().unwrap();
    let samples: Vec<[u16; 4]> = (0..48u32)
        .map(|i| [i * 5, i * 3 + 7, 200 - i * 4, i * 2].map(|v| v as u16 * 257))
        .collect();
    let doc = tiff_doc(&dir, 8, &samples, 8);
    let maps = drawn_maps();
    let desc = description("curves", &map_params(&maps));
    let destructive = apply(doc, 0, &desc).unwrap();
    let after = samples_of(destructive);
    let root = add(doc, &desc, 1.0).unwrap();
    let mut err = ptr::null_mut();
    let info = json(unsafe { dr_precision_doc_info(root, &mut err) }, err);
    assert_eq!(info["layers"][1]["adjustment"]["operation"], "curves");
    let grouped = then(
        add(doc, &desc, 1.0).unwrap(),
        r#"{"op":"group_layers","layers":[0,1],"name":"Group","pass_through":false}"#,
        &[],
    );
    for (i, d) in after.iter().enumerate() {
        let (x, y) = ((i % 8) as i32, (i / 8) as i32);
        for doc in [root, grouped] {
            let got = sample_ink(doc, x, y);
            assert_eq!(&got[..4], &d[..4], "({x},{y})");
        }
    }
    // The description reopens verbatim at word 83: no format change.
    let path = dir.path().join("maps.rz");
    assert_eq!(words(&save(root, &path))[0], 83);
    let again = open(&path);
    assert_eq!(
        unsafe { &*again }.adjustment_description(1),
        unsafe { &*root }.adjustment_description(1)
    );
    assert_eq!(render(again), render(root));
    // The identity maps are an exact no-op.
    let identity: [u8; 256] = std::array::from_fn(|i| i as u8);
    let none = description("curves", &map_params(&[identity; 5]));
    assert!(apply(doc, 0, &none).unwrap().is_null());
    for d in [destructive, root, grouped, again, doc] {
        free(d);
    }
}
