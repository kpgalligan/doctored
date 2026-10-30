//! #1 Lab phase 2e, the retouch tools on a Lab Color document, through the
//! FFI, at 8 and 16 bits. The per-channel tools — the Blur and Sharpen
//! brushes, Smudge, the Clone Stamp (Normal; its other modes composite by
//! the Lab arms since phase 3, `lab_blend.rs`), the Healing Brush,
//! Spot Healing and Content-Aware Fill — equal the same request on a 16-bit
//! sRGB document holding identical codes (`lab_targets::srgb_doc`) exactly.
//! Dodge (Midtones, exposure 0.6) is written here from its documented curve
//! (`doc_retouch::shaped`, `v^(1/(1+e))`, evaluated on the f32 unit sample
//! the brush reads): the L\* code is `round(c(1−k) + 65535·shaped(c)·k)`, k
//! the coverage, and a\*/b\* keep their codes; Protect Tones gives the
//! identical L\* code, a pixel whose a\* code exceeds its L\* code included.
//! The Sponge on a uniform document at strength 0.6 under full coverage is
//! `c + 0.6·(t − c)` on a\* and b\* within 1 code, t Saturate's
//! `n + 2·(c − n)` (clamped to 0…1) or Desaturate's `n`, n the a\* = b\* = 0
//! code 0x8080, L\* exact, Vibrance changing nothing. A Darken clone refuses
//! and a Hue one runs (phase 3); the History Brush refuses an
//! RGB state on a Lab document and a Lab state on an RGB one, and runs Lab →
//! Lab; a clone from plates runs from Lab plates (from the document itself,
//! equal to the plain clone) and refuses RGB plates (Remove Red Eye runs since
//! 2f's third part: `lab_tools.rs`). Bounds declared before measuring.

use std::ffi::CString;
use std::ptr;

use doctored_core::ffi_retouch::dr_precision_doc_retouch_from;
use doctored_core::ffi_retouch_brush::{
    dr_precision_doc_history_brush, dr_precision_doc_history_brush_plan, dr_precision_doc_smudge,
};
use doctored_core::precision_document::PrecisionDocument;
use serde_json::{json, Value};

use crate::gray_ops::{edit, free, plan_json, plan_samples, result, selected, LIMIT};
use crate::lab_targets::{codes, pattern, srgb_doc, to_lab, H, W};

const N: usize = (W * H) as usize;

const HARD: &str =
    r#"{"version":1,"hardness":1.0,"roundness":1.0,"angle_degrees":0.0,"pixel":false}"#;

const HISTORY_SOURCE: &str = "The History Brush paints a Lab Color document from a Lab Color state only: this state is from before Image ▸ Mode ▸ Lab Color.";
const HISTORY_LAB_STATE: &str = "The History Brush cannot paint from a Lab Color state: this state is from before Image ▸ Mode ▸ RGB Color.";
const PLATES: &str =
    "A Lab Color document clones from a Lab Color document only: convert the source to Lab Color first.";

/// The a\* = b\* = 0 code as a unit sample.
const NEUTRAL: f64 = 32896.0 / 65535.0;

/// `pattern()` as a Lab document at `depth`.
fn lab_doc(depth: u32) -> *mut PrecisionDocument {
    let rgb = srgb_doc(pattern());
    let lab = to_lab(rgb, &format!(r#"{{"depth":{depth}}}"#)).unwrap();
    free(rgb);
    lab
}

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

/// The request's planned input: `plane`, then full coverage for any further
/// plane the plan asks for.
fn input(doc: *const PrecisionDocument, request: &Value, plane: &[u16]) -> Vec<u16> {
    let n = plan_samples(doc, request).unwrap_or_else(|e| panic!("{request}: {e}"));
    let mut out = plane.to_vec();
    out.resize(n, 65535);
    out
}

fn run(doc: *const PrecisionDocument, request: &Value, plane: &[u16]) -> *mut PrecisionDocument {
    edit(doc, request, &input(doc, request, plane))
        .unwrap_or_else(|e| panic!("{request}: {e}"))
        .unwrap_or_else(|| panic!("{request} changed nothing"))
}

/// `request` on the Lab document at `depth` and on its sRGB twin: equal.
fn assert_twin_equal(depth: u32, request: Value, plane: &[u16]) {
    let what = format!("{request} at {depth}");
    let lab = lab_doc(depth);
    let twin = srgb_doc(codes(lab, 0));
    let a = run(lab, &request, plane);
    let b = run(twin, &request, plane);
    assert_eq!(codes(a, 0), codes(b, 0), "{what}");
    assert!(unsafe { &*a }.is_lab(), "{what}");
    for doc in [a, b, twin, lab] {
        free(doc);
    }
}

/// Coverage 65535 inside `(x, y, w, h)`.
fn rect(r: (u32, u32, u32, u32)) -> Vec<u16> {
    (0..W * H)
        .map(|i| {
            let (x, y) = (i % W, i / W);
            let inside = x >= r.0 && x < r.0 + r.2 && y >= r.1 && y < r.1 + r.3;
            if inside {
                65535
            } else {
                0
            }
        })
        .collect()
}

#[test]
fn the_per_channel_tools_equal_the_srgb_twin() {
    let plane = disc();
    for depth in [8, 16] {
        for kind in ["blur", "sharpen"] {
            assert_twin_equal(
                depth,
                json!({"op":"retouch_brush","layer":0,"kind":kind,"strength":0.7,
                    "selection":false}),
                &plane,
            );
        }
        // Normal only: every other mode composites by its Lab arm,
        // checked against `lab_blend_oracle` in `lab_blend.rs`.
        assert_twin_equal(
            depth,
            json!({"op":"retouch_clone","layer":0,"matrix":[1,0,0,1,-6,-2],"opacity":0.8,
                "blend_mode":0}),
            &plane,
        );
        assert_twin_equal(
            depth,
            json!({"op":"retouch_heal","layer":0,"matrix":[1,0,0,1,-6,-2],"opacity":0.9}),
            &plane,
        );
        assert_twin_equal(
            depth,
            json!({"op":"spot_heal","layer":0,"ring":4,"seed":7,"x":13,"y":9,"width":7,
                "height":7,"opacity":1.0}),
            &rect((14, 10, 4, 4)),
        );
        assert_twin_equal(
            depth,
            json!({"op":"content_aware_fill","layer":0,"x":12,"y":9,"width":6,"height":5,
                "seed":7}),
            &rect((12, 9, 6, 5)),
        );
    }
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
fn smudge_equals_the_srgb_twin() {
    for depth in [8, 16] {
        let lab = lab_doc(depth);
        let twin = srgb_doc(codes(lab, 0));
        let a = smudge(lab);
        let b = smudge(twin);
        assert_eq!(codes(a, 0), codes(b, 0), "smudge at {depth}");
        assert!(unsafe { &*a }.is_lab());
        for doc in [a, b, twin, lab] {
            free(doc);
        }
    }
}

/// Dodge's Midtones curve at exposure `e` (`doc_retouch::shaped`, range 1):
/// `v^(1/(1+e))`, on the f32 unit sample the brush reads.
fn midtones(code: u16, e: f32) -> f64 {
    f64::from((f32::from(code) / 65535.0).powf(1.0 / (1.0 + e)))
}

#[test]
fn dodge_shapes_lightness_alone_and_protect_tones_is_its_plain_curve() {
    let plane = disc();
    let request = |protect: bool| {
        json!({"op":"dodge_burn","layer":0,"exposure":0.6,"range":1,"burn":false,
            "protect_tones":protect,"selection":false})
    };
    for depth in [8, 16] {
        let lab = lab_doc(depth);
        let before = codes(lab, 0);
        // The Protect Tones case below needs a covered pixel whose a* code
        // exceeds its L* code (a dark one): on [L, a, b] the group's maximum
        // would be a*'s.
        assert!(
            before
                .iter()
                .zip(&plane)
                .any(|(p, &k)| k > 0 && p[1] > p[0]),
            "no covered pixel with a* code above L* code at {depth}"
        );
        let plain = run(lab, &request(false), &plane);
        let got = codes(plain, 0);
        for (i, (p, q)) in before.iter().zip(&got).enumerate() {
            let k = f64::from(plane[i]) / 65535.0;
            let want = (f64::from(p[0]) * (1.0 - k) + 65535.0 * midtones(p[0], 0.6) * k)
                .clamp(0.0, 65535.0)
                .round() as u16;
            assert_eq!(q[0], want, "L* at {i}, depth {depth}: {p:?} → {q:?}");
            assert_eq!(q[1..], p[1..], "a*, b*, alpha at {i}, depth {depth}");
        }
        assert_ne!(got, before, "dodge moved nothing at {depth}");
        let protected = run(lab, &request(true), &plane);
        assert_eq!(codes(protected, 0), got, "Protect Tones at {depth}");
        for doc in [protected, plain, lab] {
            free(doc);
        }
    }
}

#[test]
fn the_sponge_doubles_or_zeroes_chroma_and_keeps_lightness() {
    let code = |v: f64| (v * 65535.0).round() as u16;
    let fill = [code(0.8), code(0.3), code(0.2), 65535];
    for depth in [8, 16] {
        let rgb = srgb_doc(vec![fill; N]);
        let lab = to_lab(rgb, &format!(r#"{{"depth":{depth}}}"#)).unwrap();
        free(rgb);
        let c = codes(lab, 0)[0];
        for kind in ["saturate", "desaturate"] {
            for vibrance in [false, true] {
                let what = format!("{kind} at {depth}, vibrance {vibrance}");
                let request = json!({"op":"retouch_brush","layer":0,"kind":kind,
                    "strength":0.6,"selection":false,"vibrance":vibrance});
                let out = run(lab, &request, &vec![65535; N]);
                for p in codes(out, 0) {
                    assert_eq!(p[0], c[0], "{what}: L*");
                    assert_eq!(p[3], c[3], "{what}: alpha");
                    for ch in 1..3 {
                        let unit = f64::from(c[ch]) / 65535.0;
                        let target = if kind == "saturate" {
                            (NEUTRAL + 2.0 * (unit - NEUTRAL)).clamp(0.0, 1.0)
                        } else {
                            NEUTRAL
                        };
                        let want = (unit + 0.6 * (target - unit)) * 65535.0;
                        assert!(
                            (f64::from(p[ch]) - want).abs() <= 1.0,
                            "{what}: channel {ch} {} against {want}",
                            p[ch]
                        );
                    }
                }
                free(out);
            }
        }
        free(lab);
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
fn the_clone_stamp_refuses_darken_admits_hue_and_clones_from_lab_plates_only() {
    let plane = disc();
    let lab = lab_doc(16);
    let clone = |mode: i32| json!({"op":"retouch_clone","layer":0,"matrix":[1,0,0,1,-6,-2],"blend_mode":mode});
    let darken = "The Darken blend mode is unavailable in a Lab Color document.";
    assert_eq!(edit(lab, &clone(6), &plane).unwrap_err(), darken);
    assert_eq!(
        retouch_from(lab, lab, &clone(6), &plane).unwrap_err(),
        darken
    );
    // Phase 3: Hue composites by its Lab arm.
    let hue = run(lab, &clone(23), &plane);
    assert!(unsafe { &*hue }.is_lab());
    free(hue);
    let normal = clone(0);
    let samples = input(lab, &normal, &plane);
    let rgb_plates = srgb_doc(codes(lab, 0));
    assert_eq!(
        retouch_from(lab, rgb_plates, &normal, &samples).unwrap_err(),
        PLATES
    );
    let plates = lab_doc(16);
    let from_plates = retouch_from(lab, plates, &normal, &samples)
        .unwrap()
        .expect("a clone from Lab plates runs");
    assert!(unsafe { &*from_plates }.is_lab());
    let plain = run(lab, &normal, &plane);
    assert_eq!(codes(from_plates, 0), codes(plain, 0));
    for doc in [plain, from_plates, plates, rgb_plates, lab] {
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
fn the_history_brush_needs_a_lab_state() {
    let plane = disc();
    let lab = lab_doc(16);
    let blurred = selected(
        lab,
        &json!({"layer":0,"op":"blur","params":{"sigma":2}}),
        &[],
    )
    .unwrap()
    .unwrap();
    let rgb_state = srgb_doc(codes(lab, 0));
    assert_eq!(
        history_plan(blurred, rgb_state).unwrap_err(),
        HISTORY_SOURCE
    );
    assert_eq!(
        history(blurred, rgb_state, &plane).unwrap_err(),
        HISTORY_SOURCE
    );
    assert_eq!(history_plan(rgb_state, lab).unwrap_err(), HISTORY_LAB_STATE);
    assert_eq!(
        history(rgb_state, lab, &plane).unwrap_err(),
        HISTORY_LAB_STATE
    );
    assert!(history_plan(blurred, lab).is_ok());
    let restored = history(blurred, lab, &plane)
        .unwrap()
        .expect("the History Brush changed the blur");
    assert!(unsafe { &*restored }.is_lab());
    assert_ne!(codes(restored, 0), codes(blurred, 0));
    for doc in [restored, rgb_state, blurred, lab] {
        free(doc);
    }
}
