//! #1 Lab phase 6a-ii: the legacy `erase`, `clone` and `heal` paint ops
//! and Edit ▸ Clear (whose layer path is `erase`) on a Lab Color document,
//! through the FFI. Each is the RGB kernel run on the L\*a\*b\* codes, per
//! channel, so the oracle is `lab_geometry`'s RGB TWIN (the identical
//! `from_parts` document without the Lab policy). Bounds, declared before
//! measuring:
//!
//! * (e) `erase` under a soft selection coverage, `clone` (dx, dy,
//!   `source_layer` 1) and `heal` (an explicit window) at 8 and 16 bits
//!   equal the same request on the twin bit for bit, as `lab_geometry` (a)
//!   defines it;
//! * (f) Clear — `erase` at opacity 1 under a hard rectangle — leaves alpha
//!   0 inside the rectangle and every other sample of the layer unchanged.

use doctored_core::precision_document::PrecisionDocument;
use serde_json::{json, Value};

use crate::gray_ops::{edit, free, layer, plan_samples};
use crate::lab_geometry::{assert_twin, pair, DEPTHS, H, W};

/// A soft disc of selection coverage about (12, 8): partial at its rim,
/// above half at its centre (the heal's contour, `precision_heal`).
fn disc() -> Vec<u16> {
    (0..W * H)
        .map(|i| {
            let (x, y) = (f64::from(i % W), f64::from(i / W));
            let d = ((x - 12.0).powi(2) + (y - 8.0).powi(2)).sqrt();
            ((1.0 - d / 6.0).clamp(0.0, 1.0) * 65535.0).round() as u16
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

#[test]
fn the_legacy_erase_clone_and_heal_equal_the_twin() {
    let plane = disc();
    let requests = [
        json!({"op":"erase","layer":0,"opacity":0.5}),
        json!({"op":"clone","layer":0,"source_layer":1,"dx":3,"dy":2,"opacity":1.0}),
        json!({"op":"heal","layer":0,"source_layer":0,"dx":4,"dy":0,"x":6,"y":3,
            "width":12,"height":10,"opacity":1.0}),
    ];
    for bits in DEPTHS {
        for request in &requests {
            let what = format!("{request} at {bits} bits");
            let (lab, rgb) = pair(bits);
            let a = edit(lab, request, &input(lab, request, &plane))
                .unwrap_or_else(|e| panic!("{what}: {e}"))
                .unwrap_or_else(|| panic!("{what} changed nothing"));
            let b = edit(rgb, request, &input(rgb, request, &plane))
                .unwrap_or_else(|e| panic!("{what} twin: {e}"))
                .unwrap_or_else(|| panic!("{what} twin changed nothing"));
            assert_twin(a, b, bits, &what);
            free(lab);
            free(rgb);
        }
    }
}

#[test]
fn clear_leaves_alpha_zero_inside_and_every_other_sample_unchanged() {
    let inside = |i: u32| (4..=11).contains(&(i % W)) && (3..=9).contains(&(i / W));
    let hard: Vec<u16> = (0..W * H)
        .map(|i| if inside(i) { 65535 } else { 0 })
        .collect();
    let request = json!({"op":"erase","layer":0,"opacity":1.0});
    for bits in DEPTHS {
        let (lab, rgb) = pair(bits);
        let before = layer(lab, 0, bits);
        let cleared = edit(lab, &request, &input(lab, &request, &hard))
            .unwrap()
            .expect("Clear changes the layer");
        assert!(unsafe { &*cleared }.is_lab());
        let after = layer(cleared, 0, bits);
        for (i, (got, was)) in after.iter().zip(&before).enumerate() {
            if inside(i as u32) {
                assert_eq!(got[3], 0, "{bits}: pixel {i} is cleared");
            } else {
                assert_eq!(got, was, "{bits}: pixel {i} is untouched");
            }
        }
        for doc in [cleared, lab, rgb] {
            free(doc);
        }
    }
}
