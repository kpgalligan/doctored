//! Behind and Clear in a 32-bit float document (`paint_mode`'s rule,
//! `float_paint`'s `# Behind and Clear`), black-box through the request API,
//! against the rule's formulas written here in f64 on extended-range input:
//! negatives, values above white, hidden colour under alpha 0, partial alpha
//! and alpha 1, with linear ink above white. Opaque samples under Behind and
//! every colour component under Clear stay bit for bit; a frozen alpha
//! changes nothing; a colour plane refuses both; the operator table lists
//! them.

use std::ffi::CStr;

use doctored_core::doc_lock::LOCK_TRANSPARENCY;
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_float::dr_float_operators;
use doctored_core::icc::IccProfile;
use doctored_core::precision_document::PrecisionDocument;
use serde_json::{json, Value};

use crate::float_fixture::float_doc;
use crate::native_scalar::{edit, error, plan, text, LIMIT};

const BEHIND: u32 = 28;
const CLEAR: u32 = 29;
const W: u32 = 13;
const H: u32 = 9;
/// Linear ink above white in red.
const INK: [f64; 4] = [3.0, 0.25, 0.5, 1.0];
const OPACITY: f64 = 0.6;

/// Extended-range straight samples (module doc).
fn extended() -> Vec<[f32; 4]> {
    (0..W * H)
        .map(|i| {
            let (x, y) = (i % W, i / W);
            let v = ((x * 37 + y * 101) % 1000) as f32 / 1000.0;
            let a = match (x + 2 * y) % 5 {
                0 => 0.0,
                1 | 2 => 1.0,
                k => k as f32 / 5.0 - 0.1,
            };
            [v * 6.0 - 1.0, 1.0 - v, ((x ^ y) % 9) as f32 / 2.0 - 2.0, a]
        })
        .collect()
}

fn coverage() -> Vec<u16> {
    (0..W * H)
        .map(|i| match i % 7 {
            0 => 0,
            1 => 65535,
            k => (k * 8191 + i * 13) as u16,
        })
        .collect()
}

fn document(samples: &[[f32; 4]]) -> PrecisionDocument {
    let doc = float_doc(W, H, &IccProfile::srgb(), &[samples.to_vec()]);
    let held: Vec<[u32; 4]> = layer32(&doc).iter().map(|p| p.map(f32::to_bits)).collect();
    let wanted: Vec<[u32; 4]> = samples.iter().map(|p| p.map(f32::to_bits)).collect();
    assert_eq!(held, wanted, "the raster holds the samples exactly");
    doc
}

fn layer32(d: &PrecisionDocument) -> Vec<[f32; 4]> {
    d.layer_source(0).unwrap().to_rgba32().unwrap()
}

fn request(mode: u32, extra: Value) -> Value {
    let mut request = json!({"op":"paint_blend","layer":0,"color":INK,"color_space":"linear",
        "opacity":OPACITY,"blend_mode":mode});
    for (key, value) in extra.as_object().unwrap() {
        request[key] = value.clone();
    }
    request
}

/// The rule in binary64.
fn rule(mode: u32, b: [f32; 4], sa: f64) -> [f64; 4] {
    let unit = b.map(f64::from);
    let ab = unit[3];
    if mode == CLEAR {
        return [unit[0], unit[1], unit[2], ab * (1.0 - sa)];
    }
    let ao = ab + sa * (1.0 - ab);
    let mut out = [0.0; 4];
    for c in 0..3 {
        out[c] = (unit[c] * ab + INK[c] * sa * (1.0 - ab)) / ao;
    }
    out[3] = ao;
    out
}

fn close(actual: f32, expected: f64) -> bool {
    (f64::from(actual) - expected).abs() <= 1e-5 * expected.abs().max(1.0)
}

#[test]
fn both_modes_follow_the_rule_on_extended_range_samples() {
    let samples = extended();
    let doc = document(&samples);
    let input = coverage();
    for (mode, name) in [(BEHIND, "Behind"), (CLEAR, "Clear")] {
        let out =
            edit(&doc, request(mode, json!({})), &input).expect("the paint changes the layer");
        let after = layer32(&out);
        let mut moved = 0;
        for (i, (b, a)) in samples.iter().zip(&after).enumerate() {
            let sa = f64::from(input[i]) / 65535.0 * OPACITY;
            let bits = |p: &[f32; 4]| p.map(f32::to_bits);
            if sa <= 0.0 || (mode == BEHIND && b[3] >= 1.0) {
                assert_eq!(bits(a), bits(b), "{name}: sample {i} stays bit for bit");
                continue;
            }
            if mode == CLEAR {
                assert_eq!(bits(a)[..3], bits(b)[..3], "{name}: colour stays at {i}");
            }
            let want = rule(mode, *b, sa);
            for c in 0..4 {
                assert!(
                    close(a[c], want[c]),
                    "{name}: sample {i} {c}: {a:?} vs {want:?}"
                );
            }
            moved += usize::from(bits(a) != bits(b));
        }
        assert!(moved > 20, "{name}: {moved} samples moved");
        // Behind keeps the above-white ink unclamped where nothing was.
        if mode == BEHIND {
            let reached = after
                .iter()
                .zip(&samples)
                .any(|(a, b)| b[3] == 0.0 && a[3] > 0.0 && a[0] > 1.0);
            assert!(reached, "Behind writes the ink above white over alpha 0");
        }
    }
}

#[test]
fn a_frozen_alpha_changes_nothing() {
    let doc = document(&extended());
    let locked = doc.with_layer_locks(0, LOCK_TRANSPARENCY).unwrap().unwrap();
    let input = coverage();
    for mode in [BEHIND, CLEAR] {
        assert!(
            edit(&doc, request(mode, json!({})), &input).is_some(),
            "{mode}: free"
        );
        assert!(edit(&doc, request(mode, json!({"preserve_alpha": true})), &input).is_none());
        assert!(edit(&locked, request(mode, json!({})), &input).is_none());
    }
}

#[test]
fn a_colour_plane_refuses_and_the_table_lists_both() {
    let doc = document(&extended());
    for (mode, name) in [(BEHIND, "Behind"), (CLEAR, "Clear")] {
        let v = request(mode, json!({"plane": "red", "color_space": "document"}));
        let t = text(v);
        let mut e = std::ptr::null_mut();
        let p = unsafe {
            doctored_core::ffi_precision_edit::dr_precision_doc_edit_plan(
                &doc,
                t.as_ptr(),
                LIMIT,
                0,
                &mut e,
            )
        };
        assert!(p.is_null());
        assert_eq!(
            unsafe { error(e) },
            format!(
                "The {name} blend mode paints a layer's transparency; a colour plane has none of \
                 its own"
            )
        );
    }
    // The plan admits both on the layer.
    plan(&doc, request(BEHIND, json!({})));
    let table = unsafe { dr_float_operators() };
    let parsed: Value =
        serde_json::from_str(unsafe { CStr::from_ptr(table) }.to_str().unwrap()).unwrap();
    unsafe { dr_string_free(table) };
    let admitted = parsed["admitted"].as_array().unwrap();
    for row in ["behind_paint", "clear_paint"] {
        assert!(admitted.iter().any(|v| v == row), "{row} is admitted");
    }
}
