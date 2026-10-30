//! #1 phase 2c, the retouch tools on a Grayscale document, through the
//! FFI. Dodge and Burn (Protect Tones on and off), the Blur and Sharpen
//! brushes, Smudge, the Clone Stamp, the Healing Brush and Spot Healing equal
//! the same stroke on the RGB TWIN (the Gray document converted to RGB
//! through its own profile bytes) exactly, and keep R = G = B — at 16 bits,
//! and at 8 and 32 for Dodge, Burn and the Blur brush. The Sponge's oracle is
//! written here from its Grayscale rule: on a uniform document at strength
//! 0.6 under full coverage each pixel is `c + 0.6·(target − c)`, Saturate's
//! target `m + 2·(c − m)` (clamped to 0…1 at 16 bits, unclamped at 32, so a
//! dark 32-bit document goes negative) and Desaturate's `m`, the middle gray
//! (0.5 at 16 bits; at 32 the linear value of code 0.5 through Gray Gamma
//! 2.2's `kTRC`, a 4096-entry `curv` of `round(65535·v^2.2)` interpolated
//! at 0.5, which is `0.5^2.2` to within that table's 1.8e-6), within 1 code /
//! 1e-6; Vibrance
//! changes nothing. The Clone Stamp refuses Hue and runs Multiply; a clone
//! from plates refuses Hue and RGB plates; Remove Red Eye and an RGB History
//! Brush state refuse by sentence; a Gray state runs. Phase 6a-ii: the
//! legacy `clone` and `heal` ops and `erase` under the soft disc equal
//! the twin's exactly and keep R = G = B at 16 bits, and refuse u16-only
//! storage on a 32-bit document by its sentence. Bounds declared before
//! measuring.

use std::ffi::CString;
use std::ptr;

use doctored_core::ffi_retouch::dr_precision_doc_retouch_from;
use doctored_core::ffi_retouch_brush::{
    dr_precision_doc_history_brush, dr_precision_doc_history_brush_plan, dr_precision_doc_smudge,
};
use doctored_core::precision_capability::require_gray;
use doctored_core::precision_document::PrecisionDocument;
use serde_json::{json, Value};

use crate::gray_fixture::srgb_encode;
use crate::gray_ops::{
    assert_neutral, edit, free, gray_doc, layer, plan_json, plan_samples, result, selected, twin,
    uniform_gray, values, LIMIT,
};

const W: u32 = 32;
const H: u32 = 24;
const N: usize = (W * H) as usize;

const HARD: &str =
    r#"{"version":1,"hardness":1.0,"roundness":1.0,"angle_degrees":0.0,"pixel":false}"#;

const RED_EYE: &str =
    "Remove Red Eye is unavailable in a Grayscale document: Doctored offers it on RGB documents only.";
const HISTORY: &str = "The History Brush paints a Grayscale document from a Grayscale state only: this state is from before Image ▸ Mode ▸ Grayscale.";
const PLATES: &str =
    "A Grayscale document clones from a Grayscale document only: convert the source to Grayscale first.";

/// A soft disc of coverage around (16, 12), radius 6.
fn disc() -> Vec<u16> {
    (0..N)
        .map(|i| {
            let (x, y) = ((i as u32 % W) as f64, (i as u32 / W) as f64);
            let d = ((x - 16.0).powi(2) + (y - 12.0).powi(2)).sqrt();
            ((1.0 - (d / 6.0)).clamp(0.0, 1.0) * 65535.0).round() as u16
        })
        .collect()
}

/// The request's planned input: the disc, then a full selection plane when
/// the plan asks for one.
fn input(doc: *const PrecisionDocument, request: &Value, plane: &[u16]) -> Vec<u16> {
    let n = plan_samples(doc, request).unwrap_or_else(|e| panic!("{request}: {e}"));
    let mut out = plane.to_vec();
    out.resize(n, 65535);
    out
}

/// `request` on the Gray document and on its twin: equal and neutral.
fn assert_twin_equal(depth: u32, request: Value, plane: &[u16]) {
    let what = format!("{request} at {depth}");
    let gray = gray_doc(W, H, depth);
    let rgb = twin(gray);
    let samples = input(gray, &request, plane);
    let a = edit(gray, &request, &samples)
        .unwrap_or_else(|e| panic!("{what}: {e}"))
        .unwrap_or_else(|| panic!("{what} changed nothing"));
    let b = edit(rgb, &request, &input(rgb, &request, plane))
        .unwrap_or_else(|e| panic!("{what} twin: {e}"))
        .unwrap();
    assert_neutral(&layer(a, 0, depth), &what);
    assert_eq!(layer(a, 0, depth), layer(b, 0, depth), "{what}");
    assert!(unsafe { &*a }.is_gray(), "{what}");
    for doc in [a, b, rgb, gray] {
        free(doc);
    }
}

#[test]
fn dodge_burn_and_the_focus_brushes_equal_the_twin() {
    let plane = disc();
    for depth in [8, 16, 32] {
        for (range, burn, protect) in [(1, false, false), (2, true, false), (0, false, true)] {
            assert_twin_equal(
                depth,
                json!({"op":"dodge_burn","layer":0,"exposure":0.6,"range":range,"burn":burn,
                    "protect_tones":protect,"selection":false}),
                &plane,
            );
        }
        assert_twin_equal(
            depth,
            json!({"op":"retouch_brush","layer":0,"kind":"blur","strength":0.7,"selection":false}),
            &plane,
        );
    }
    assert_twin_equal(
        16,
        json!({"op":"retouch_brush","layer":0,"kind":"sharpen","strength":0.7,"selection":false}),
        &plane,
    );
}

#[test]
fn clone_heal_and_spot_heal_equal_the_twin() {
    let plane = disc();
    assert_twin_equal(
        16,
        json!({"op":"retouch_clone","layer":0,"matrix":[1,0,0,1,-6,-2],"opacity":0.8,
            "blend_mode":0}),
        &plane,
    );
    assert_twin_equal(
        16,
        json!({"op":"retouch_clone","layer":0,"matrix":[1,0,0,1,-6,-2],"opacity":0.8,
            "blend_mode":1}),
        &plane,
    );
    assert_twin_equal(
        16,
        json!({"op":"retouch_heal","layer":0,"matrix":[1,0,0,1,-5,-3],"opacity":0.9}),
        &plane,
    );
    let spot: Vec<u16> = (0..N)
        .map(|i| {
            let (x, y) = ((i as u32 % W) as f64, (i as u32 / W) as f64);
            if (x - 16.0).powi(2) + (y - 12.0).powi(2) <= 9.0 {
                65535
            } else {
                0
            }
        })
        .collect();
    assert_twin_equal(
        16,
        json!({"op":"spot_heal","layer":0,"ring":4,"seed":7,"x":13,"y":9,"width":7,"height":7,
            "opacity":1.0}),
        &spot,
    );
}

fn smudge(doc: *const PrecisionDocument) -> *mut PrecisionDocument {
    let dabs: Vec<f64> = (0..12)
        .flat_map(|i| [8.0 + f64::from(i), 9.0 + f64::from(i) * 0.5, 6.0, 1.0])
        .collect();
    let brush = CString::new(HARD).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_smudge(
            doc,
            0,
            brush.as_ptr(),
            dabs.as_ptr(),
            dabs.len() / 4,
            0.8,
            ptr::null(),
            0,
            false,
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    result(out, err)
        .unwrap()
        .expect("the smudge changed nothing")
}

#[test]
fn smudge_equals_the_twin() {
    let gray = gray_doc(W, H, 16);
    let rgb = twin(gray);
    let a = smudge(gray);
    let b = smudge(rgb);
    assert_neutral(&layer(a, 0, 16), "smudge");
    assert_eq!(layer(a, 0, 16), layer(b, 0, 16));
    for doc in [a, b, rgb, gray] {
        free(doc);
    }
}

/// The linear value of code 0.5 in the built-in Gray Gamma 2.2 profile,
/// written from its construction: a 4096-entry `curv` of
/// `round(65535·(i/4095)^2.2)`, and 0.5 = 2047.5/4095 lies midway between
/// entries 2047 and 2048.
fn gamma_22_middle() -> f64 {
    let entry = |i: f64| ((i / 4095.0).powf(2.2) * 65535.0).round();
    (entry(2047.0) + entry(2048.0)) / 2.0 / 65535.0
}

/// The Sponge on a uniform document at strength 0.6 under full coverage.
fn sponge(doc: *const PrecisionDocument, kind: &str, vibrance: bool) -> *mut PrecisionDocument {
    let request = json!({"op":"retouch_brush","layer":0,"kind":kind,"strength":0.6,
        "selection":false,"vibrance":vibrance});
    let samples = input(doc, &request, &vec![65535; N]);
    edit(doc, &request, &samples)
        .unwrap_or_else(|e| panic!("{kind}: {e}"))
        .unwrap_or_else(|| panic!("{kind} changed nothing"))
}

#[test]
fn the_sponge_moves_gray_levels_around_the_middle_gray() {
    // 16 bits: a light gray, so Saturate clamps at white; 32 bits: the sRGB
    // fill whose luminance is about 0.25^2.2 (code 0x4000), so Saturate's
    // unclamped target is negative.
    for (depth, fill) in [(16, 0.85), (32, srgb_encode(0.25f64.powf(2.2)))] {
        let doc = uniform_gray(W, H, depth, fill);
        let c = values(doc, 0, depth)[0][0];
        let (unit, m) = if depth == 32 {
            (c, gamma_22_middle())
        } else {
            (c / 65535.0, 0.5)
        };
        for (kind, target) in [("saturate", m + 2.0 * (unit - m)), ("desaturate", m)] {
            let target = if depth == 32 {
                target
            } else {
                target.clamp(0.0, 1.0)
            };
            let want = unit + 0.6 * (target - unit);
            for vibrance in [false, true] {
                let what = format!("{kind} at {depth}, vibrance {vibrance}");
                let out = sponge(doc, kind, vibrance);
                let got = values(out, 0, depth);
                assert_neutral(&layer(out, 0, depth), &what);
                for p in &got {
                    if depth == 32 {
                        assert!(
                            (p[0] - want).abs() <= 1e-6,
                            "{what}: {} against {want}",
                            p[0]
                        );
                    } else {
                        let want = want * 65535.0;
                        assert!(
                            (p[0] - want).abs() <= 1.0,
                            "{what}: {} against {want}",
                            p[0]
                        );
                    }
                }
                if depth == 32 && kind == "saturate" {
                    assert!(got[0][0] < 0.0, "{what}: {} is not negative", got[0][0]);
                }
                free(out);
            }
        }
        free(doc);
    }
}

fn retouch_from(
    doc: *const PrecisionDocument,
    plates: *const PrecisionDocument,
    request: &Value,
    samples: &[u16],
) -> Result<Option<*mut PrecisionDocument>, String> {
    let text = CString::new(request.to_string()).unwrap();
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
    result(out, err)
}

#[test]
fn the_clone_stamp_refuses_hue_and_rgb_plates() {
    let plane = disc();
    let gray = gray_doc(W, H, 16);
    let hue = json!({"op":"retouch_clone","layer":0,"matrix":[1,0,0,1,-6,-2],"blend_mode":23});
    let sentence = "The Hue blend mode is unavailable in a Grayscale document.";
    assert_eq!(edit(gray, &hue, &plane).unwrap_err(), sentence);
    let plates = gray_doc(W, H, 16);
    assert_eq!(
        retouch_from(gray, plates, &hue, &plane).unwrap_err(),
        sentence
    );
    let normal = json!({"op":"retouch_clone","layer":0,"matrix":[1,0,0,1,-6,-2],"blend_mode":0});
    let samples = input(gray, &normal, &plane);
    let rgb_plates = twin(plates);
    assert_eq!(
        retouch_from(gray, rgb_plates, &normal, &samples).unwrap_err(),
        PLATES
    );
    let cloned = retouch_from(gray, plates, &normal, &samples)
        .unwrap()
        .expect("a clone from Gray plates runs");
    assert_neutral(&layer(cloned, 0, 16), "clone from plates");
    for doc in [cloned, rgb_plates, plates, gray] {
        free(doc);
    }
}

fn history_plan(
    doc: *const PrecisionDocument,
    source: *const PrecisionDocument,
) -> Result<Value, String> {
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_history_brush_plan(doc, 0, source, 0, false, LIMIT, 0, &mut err)
    };
    plan_json(out, err)
}

fn history(
    doc: *const PrecisionDocument,
    source: *const PrecisionDocument,
    samples: &[u16],
) -> Result<Option<*mut PrecisionDocument>, String> {
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_history_brush(
            doc,
            0,
            source,
            0,
            0.75,
            samples.as_ptr(),
            samples.len(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    result(out, err)
}

#[test]
fn red_eye_and_an_rgb_history_state_refuse_and_a_gray_state_runs() {
    let plane = disc();
    let gray = gray_doc(W, H, 16);
    assert_eq!(
        edit(
            gray,
            &json!({"op":"red_eye","layer":0,"x":10,"y":8,"width":8,"height":8}),
            &plane
        )
        .unwrap_err(),
        RED_EYE
    );
    let blurred = selected(
        gray,
        &json!({"layer":0,"op":"blur","params":{"sigma":2}}),
        &[],
    )
    .unwrap()
    .unwrap();
    let rgb_state = twin(gray);
    assert_eq!(history_plan(blurred, rgb_state).unwrap_err(), HISTORY);
    assert_eq!(history(blurred, rgb_state, &plane).unwrap_err(), HISTORY);
    assert!(history_plan(blurred, gray).is_ok());
    let restored = history(blurred, gray, &plane)
        .unwrap()
        .expect("the History Brush changed the blur");
    assert_neutral(&layer(restored, 0, 16), "history brush");
    assert!(unsafe { &*restored }.is_gray());
    for family in ["clone", "heal"] {
        assert!(require_gray(family, "raster").is_ok(), "{family}");
    }
    for doc in [restored, rgb_state, blurred, gray] {
        free(doc);
    }
}

#[test]
fn the_legacy_clone_heal_and_erase_equal_the_twin_and_refuse_float() {
    // The soft disc: partial coverage at its rim, and above half (the
    // heal's contour, `precision_heal`) at its centre.
    let plane = disc();
    let requests = [
        json!({"op":"clone","layer":0,"source_layer":0,"dx":3,"dy":0,"opacity":1.0}),
        json!({"op":"heal","layer":0,"source_layer":0,"dx":4,"dy":0,"x":10,"y":7,
            "width":12,"height":10,"opacity":1.0}),
        json!({"op":"erase","layer":0,"opacity":0.5}),
    ];
    for request in &requests {
        assert_twin_equal(16, request.clone(), &plane);
    }
    let float = gray_doc(W, H, 32);
    for request in &requests {
        let err = edit(float, request, &plane).unwrap_err();
        assert_eq!(err, "high-depth paint requires u16 storage", "{request}");
    }
    free(float);
}
