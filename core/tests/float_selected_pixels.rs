//! Move and Free Transform of a layer's selected pixels in a 32-bit
//! float document, black-box through `dr_precision_doc_edit`, its plan and
//! `dr_precision_doc_selected_pixels_bounds`.
//!
//! The documents come from `float_fixture::float_doc` with extended values:
//! RGB above white and below zero, half alpha, hidden RGB nowhere. The move
//! oracle is `common::selected_pixels::oracle_pixel` (the W3C Normal
//! source-over of the float onto the holed pixel, in f64), compared with a
//! relative bound of 1e-6 of the magnitudes mixed; where the landing's
//! `a·c` is 1 over a hole of alpha 0, ½ or 1 the result is the source bit for
//! bit. A filtered transform is pinned by D3 (`float_document`): every visible
//! landed RGB lies in the float's own visible range, and no alpha lies in
//! (0, 2^-24).

use crate::common;
use crate::float_fixture::float_doc;

use common::selected_pixels::{
    bounds16, coverage, edit16, oracle_pixel, plan16, rect, typed_doc, typed_layer, Depth, Plane,
};
use doctored_core::doc_lock::LOCK_PIXELS;
use doctored_core::icc::IccProfile;
use doctored_core::job_control::DrJobControl;
use doctored_core::precision_capability::capability;
use doctored_core::precision_document::PrecisionDocument;
use serde_json::{json, Value};

const CANVAS: (u32, u32) = (16, 12);

/// Extended straight RGBA: above white, negative, and half alpha on a
/// diagonal band.
fn extended(x: u32, y: u32) -> [f32; 4] {
    let alpha = if (x + 2 * y) % 7 == 3 { 0.5 } else { 1.0 };
    [
        -0.75 + 0.625 * x as f32,
        3.5 - 0.375 * y as f32,
        ((x * 7 + y * 3) % 11) as f32 * 0.9 - 1.25,
        alpha,
    ]
}

fn records(canvas: (u32, u32), f: impl Fn(u32, u32) -> [f32; 4]) -> Vec<[f32; 4]> {
    (0..canvas.0 * canvas.1)
        .map(|i| f(i % canvas.0, i / canvas.0))
        .collect()
}

fn doc_of(canvas: (u32, u32), f: impl Fn(u32, u32) -> [f32; 4]) -> PrecisionDocument {
    float_doc(
        canvas.0,
        canvas.1,
        &IccProfile::srgb(),
        &[records(canvas, f)],
    )
}

/// Record 0 at canvas `(x, y)`: transparent zeros off the layer.
fn at(doc: &PrecisionDocument, x: i64, y: i64) -> [f32; 4] {
    let layer = doc.layer(0).unwrap();
    let (ox, oy) = layer.offset();
    let (lx, ly) = (x - i64::from(ox), y - i64::from(oy));
    if lx < 0 || ly < 0 {
        return [0.0; 4];
    }
    layer
        .source()
        .unwrap()
        .sample_at(lx as u32, ly as u32)
        .unwrap_or([0.0; 4])
}

fn moved(dx: i32, dy: i32) -> Value {
    json!({"op":"move_selected","layer":0,"dx":dx,"dy":dy})
}

fn affine(m: [f64; 6], filter: &str) -> Value {
    json!({"op":"transform_selected","layer":0,"matrix":m,"filter":filter})
}

fn run(doc: &PrecisionDocument, request: &Value, cov: &[u16]) -> PrecisionDocument {
    edit16(doc, &request.to_string(), cov, None)
        .unwrap_or_else(|e| panic!("{request}: {e}"))
        .unwrap_or_else(|| panic!("{request} changed nothing"))
}

fn to64(p: [f32; 4]) -> [f64; 4] {
    p.map(f64::from)
}

/// Every canvas pixel against the move oracle; `hole` false is the duplicate.
/// Returns how many landings were checked bit for bit.
fn check_move(
    before: &PrecisionDocument,
    after: &PrecisionDocument,
    cov: &[u16],
    d: (i64, i64),
    hole: bool,
) -> usize {
    let (w, h) = CANVAS;
    let c_at = |x: i64, y: i64| f64::from(cov[(y * i64::from(w) + x) as usize]) / 65535.0;
    let mut exact = 0;
    for y in 0..i64::from(h) {
        for x in 0..i64::from(w) {
            let orig = to64(at(before, x, y));
            let c = if hole { c_at(x, y) } else { 0.0 };
            let (qx, qy) = (x - d.0, y - d.1);
            let source = (qx >= 0 && qy >= 0 && qx < i64::from(w) && qy < i64::from(h))
                .then(|| (to64(at(before, qx, qy)), c_at(qx, qy)));
            let got = at(after, x, y);
            let Some(want) = oracle_pixel(orig, c, source) else {
                assert_eq!(
                    got.map(f32::to_bits),
                    at(before, x, y).map(f32::to_bits),
                    "({x}, {y}) untouched"
                );
                continue;
            };
            let scale = source.map_or(0.0, |(s, _)| s.iter().map(|v| v.abs()).sum::<f64>());
            for k in 0..4 {
                let bound = 1e-6 * (1.0 + orig[k].abs() + scale);
                assert!(
                    (f64::from(got[k]) - want[k]).abs() <= bound,
                    "({x}, {y})[{k}]: {} vs {}",
                    got[k],
                    want[k]
                );
            }
            if let Some((s, fc)) = source {
                let ab = orig[3] * (1.0 - c);
                if s[3] * fc == 1.0 && [0.0, 0.5, 1.0].contains(&ab) {
                    assert_eq!(
                        got.map(f32::to_bits),
                        at(before, qx, qy).map(f32::to_bits),
                        "({x}, {y}) lands bit for bit"
                    );
                    exact += 1;
                }
            }
        }
    }
    exact
}

#[test]
fn a_hard_move_matches_the_oracle_and_lands_bit_for_bit() {
    let doc = doc_of(CANVAS, extended);
    let cov = coverage(CANVAS, rect(3, 2, 6, 5));
    for (dx, dy) in [(4, 3), (-2, 1), (9, -1)] {
        let out = run(&doc, &moved(dx, dy), &cov);
        assert!(out.is_float());
        let exact = check_move(&doc, &out, &cov, (i64::from(dx), i64::from(dy)), true);
        assert!(exact > 0, "({dx}, {dy}) checks some landings exactly");
    }
}

#[test]
fn a_feathered_move_matches_the_oracle() {
    let doc = doc_of(CANVAS, extended);
    let feather = |x: u32, y: u32| -> u16 {
        let (dx, dy) = (f64::from(x) - 6.3, f64::from(y) - 5.1);
        let r = (dx * dx + dy * dy).sqrt();
        ((1.0 - ((r - 1.0) / 3.5).clamp(0.0, 1.0)) * 65535.0).round() as u16
    };
    let cov = coverage(CANVAS, feather);
    let out = run(&doc, &moved(3, -2), &cov);
    check_move(&doc, &out, &cov, (3, -2), true);
}

#[test]
fn a_duplicate_leaves_no_hole() {
    let doc = doc_of(CANVAS, extended);
    let cov = coverage(CANVAS, rect(2, 2, 5, 4));
    let mut request = moved(5, 4);
    request["duplicate"] = json!(true);
    let out = run(&doc, &request, &cov);
    check_move(&doc, &out, &cov, (5, 4), false);
    assert_eq!(
        at(&out, 2, 2).map(f32::to_bits),
        at(&doc, 2, 2).map(f32::to_bits),
        "the original stays where it was"
    );
}

/// Opaque at x 3..9, y 2..7, transparent elsewhere, so the paste trims the
/// record to that rect.
fn trimmed() -> PrecisionDocument {
    doc_of((20, 14), |x, y| {
        if (3..9).contains(&x) && (2..7).contains(&y) {
            extended(x, y)
        } else {
            [0.0; 4]
        }
    })
}

#[test]
fn a_landing_outside_a_trimmed_record_grows_it() {
    let doc = trimmed();
    let layer = doc.layer(0).unwrap();
    let (ox, oy) = layer.offset();
    let (lw, lh) = layer.source().unwrap().dimensions();
    let canvas = (20u32, 14u32);
    let cov = coverage(canvas, rect(3, 2, 6, 5));
    let out = run(&doc, &moved(9, 6), &cov);
    // layer ∪ ((F + d) ∩ canvas): F is x 3..9, y 2..7, so the landing is
    // x 12..18, y 8..13.
    let grown = out.layer(0).unwrap();
    let (gx, gy) = grown.offset();
    let (gw, gh) = grown.source().unwrap().dimensions();
    let x1 = (ox + lw as i32).max(18);
    let y1 = (oy + lh as i32).max(13);
    assert_eq!((gx, gy), (ox.min(12), oy.min(8)));
    assert_eq!((gw as i32, gh as i32), (x1 - gx, y1 - gy));
    for y in 0..14i64 {
        for x in 0..20i64 {
            let got = at(&out, x, y);
            let source = (12..18).contains(&x) && (8..13).contains(&y);
            if source {
                assert_eq!(
                    got.map(f32::to_bits),
                    at(&doc, x - 9, y - 6).map(f32::to_bits),
                    "({x}, {y}) landed"
                );
            } else {
                assert_eq!(got[3], 0.0, "({x}, {y}) is the hole or empty");
            }
        }
    }
}

#[test]
fn a_growth_that_moves_the_origin_keeps_the_comp() {
    let doc = trimmed();
    let save = json!({"op":"layer_comp","action":"save","name":"Here","position":true});
    let saved = run(&doc, &save, &[]);
    let cov = coverage((20, 14), rect(3, 2, 6, 5));
    let grown = run(&saved, &moved(-3, -2), &cov);
    assert_eq!(grown.layer(0).unwrap().offset(), (0, 0), "the grown origin");
    let shifted = run(
        &grown,
        &json!({"op":"affine_set","layers":[0],"matrix":[1,0,0,1,9,4],"filter":"nearest"}),
        &[],
    );
    let applied = run(
        &shifted,
        &json!({"op":"layer_comp","action":"apply","comp":0}),
        &[],
    );
    assert_eq!(
        applied.layer(0).unwrap().offset(),
        (0, 0),
        "the comp puts the grown content back where the growth left it"
    );
    for y in 0..14 {
        for x in 0..20 {
            assert_eq!(
                at(&applied, x, y).map(f32::to_bits),
                at(&grown, x, y).map(f32::to_bits),
                "({x}, {y})"
            );
        }
    }
}

#[test]
fn an_integer_translation_is_the_move_bit_for_bit() {
    let doc = doc_of(CANVAS, extended);
    let cov = coverage(CANVAS, rect(2, 3, 7, 4));
    let by_move = run(&doc, &moved(5, -2), &cov);
    for filter in ["bilinear", "lanczos3"] {
        let by_transform = run(&doc, &affine([1.0, 0.0, 0.0, 1.0, 5.0, -2.0], filter), &cov);
        assert_eq!(
            by_transform.layer(0).unwrap().offset(),
            by_move.layer(0).unwrap().offset()
        );
        for y in 0..12 {
            for x in 0..16 {
                assert_eq!(
                    at(&by_transform, x, y).map(f32::to_bits),
                    at(&by_move, x, y).map(f32::to_bits),
                    "{filter} ({x}, {y})"
                );
            }
        }
    }
}

#[test]
fn a_nearest_quarter_turn_of_an_opaque_selection_is_a_permutation() {
    let opaque = |x: u32, y: u32| {
        let mut p = extended(x, y);
        p[3] = 1.0;
        p
    };
    let doc = doc_of(CANVAS, opaque);
    // The box (4, 4, 6, 4) turned 90° about its centre (7, 6): x' = 13 − y,
    // y' = x − 1, so a destination centre maps back to source pixel
    // (Y + 1, 12 − X).
    let cov = coverage(CANVAS, rect(4, 4, 6, 4));
    let out = run(
        &doc,
        &affine([0.0, 1.0, -1.0, 0.0, 13.0, -1.0], "nearest"),
        &cov,
    );
    let in_box = |x: i64, y: i64| (4..10).contains(&x) && (4..8).contains(&y);
    for y in 0..12i64 {
        for x in 0..16i64 {
            let (sx, sy) = (y + 1, 12 - x);
            let got = at(&out, x, y).map(f32::to_bits);
            if in_box(sx, sy) {
                assert_eq!(got, at(&doc, sx, sy).map(f32::to_bits), "({x}, {y})");
            } else if in_box(x, y) {
                let mut hole = at(&doc, x, y);
                hole[3] = 0.0;
                assert_eq!(got, hole.map(f32::to_bits), "({x}, {y}) hole");
            } else {
                assert_eq!(got, at(&doc, x, y).map(f32::to_bits), "({x}, {y})");
            }
        }
    }
}

#[test]
fn a_filtered_rotation_lands_inside_the_floats_range() {
    // A hard checker of -1.5 and 6 (and a negative green) rings under every
    // kernel; outside the selection the layer holds values far past it.
    let checker = |x: u32, y: u32| {
        if (4..12).contains(&x) && (3..9).contains(&y) {
            let hi = (x + y).is_multiple_of(2);
            [
                if hi { 6.0 } else { -1.5 },
                if hi { -0.5 } else { 2.0 },
                0.25,
                1.0,
            ]
        } else {
            [40.0, -30.0, 25.0, 1.0]
        }
    };
    let doc = doc_of(CANVAS, checker);
    let cov = coverage(CANVAS, rect(4, 3, 8, 6));
    let (lo, hi) = ([-1.5f32, -0.5, 0.25], [6.0f32, 2.0, 0.25]);
    let (c, s) = (30f64.to_radians().cos(), 30f64.to_radians().sin());
    let (cx, cy) = (8.0, 6.0);
    let m = [c, s, -s, c, cx - c * cx + s * cy, cy - s * cx - c * cy];
    for filter in ["bilinear", "bicubic", "lanczos3"] {
        let out = run(&doc, &affine(m, filter), &cov);
        let mut landed = 0;
        for y in 0..12i64 {
            for x in 0..16i64 {
                let p = at(&out, x, y);
                assert!(
                    p[3] == 0.0 || p[3] >= 1.0 / 16_777_216.0,
                    "{filter} ({x}, {y}): alpha {} below 2^-24",
                    p[3]
                );
                // Inside the hole the backdrop is transparent, so a visible
                // pixel there is the landed sample alone.
                let hole = (4..12).contains(&x) && (3..9).contains(&y);
                if !hole || p[3] == 0.0 {
                    continue;
                }
                landed += 1;
                for k in 0..3 {
                    let slack = 1e-6 * (1.0 + hi[k].abs().max(lo[k].abs()));
                    assert!(
                        p[k] >= lo[k] - slack && p[k] <= hi[k] + slack,
                        "{filter} ({x}, {y})[{k}] = {} leaves [{}, {}]",
                        p[k],
                        lo[k],
                        hi[k]
                    );
                }
            }
        }
        assert!(landed > 0, "{filter} lands inside the hole");
    }
}

fn scratch(plan: &Value) -> u64 {
    plan["scratch_bytes"].as_u64().unwrap()
}

#[test]
fn the_plan_charges_forty_eight_bytes_a_pixel() {
    let doc = doc_of((12, 9), extended);
    const BOOKKEEPING: u64 = 4 * 1024 * 1024;
    // 16·|L ∩ C| + 48·|bbox(L ∪ C)| + bookkeeping.
    let plan = plan16(&doc, &moved(1, 0).to_string()).unwrap();
    assert_eq!(scratch(&plan), 108 * 16 + 108 * 48 + BOOKKEEPING);
    assert_eq!(plan["input_samples"], 108);
    // Bounds (2, 2, 3, 3): 16·|B| + 48·|bbox(B ∪ (B + d))| + bookkeeping.
    let mut bounded = moved(1, 0);
    bounded["bounds"] = json!([2, 2, 3, 3]);
    let plan = plan16(&doc, &bounded.to_string()).unwrap();
    assert_eq!(scratch(&plan), 9 * 16 + 12 * 48 + BOOKKEEPING);
    // A transform's landing term is the storage-independent 16 bytes a
    // pixel of its landing box, the same at 16 bits and at 32.
    let d16 = typed_doc(
        (12, 9),
        vec![typed_layer(&Plane::of(
            Depth::D16,
            (0, 0),
            (12, 9),
            |x, y| [x as u8, y as u8, 7, 255],
        ))],
    );
    let turn = affine([0.8660254, 0.5, -0.5, 0.8660254, 4.0, -3.0], "bilinear").to_string();
    let float_landing = scratch(&plan16(&doc, &turn).unwrap())
        - scratch(&plan16(&doc, &moved(1, 0).to_string()).unwrap());
    let d16_landing = scratch(&plan16(&d16, &turn).unwrap())
        - scratch(&plan16(&d16, &moved(1, 0).to_string()).unwrap());
    assert!(float_landing > 0);
    assert_eq!(float_landing, d16_landing);
}

#[test]
fn the_float_bounds_query_answers_the_floats_rect() {
    let doc = trimmed();
    let cov = coverage((20, 14), rect(0, 0, 20, 14));
    assert_eq!(bounds16(&doc, 0, &cov, None).unwrap(), Some([3, 2, 6, 5]));
    let cov = coverage((20, 14), rect(12, 9, 3, 3));
    assert_eq!(bounds16(&doc, 0, &cov, None).unwrap(), None);
}

#[test]
fn lock_pixels_refuses() {
    let doc = doc_of(CANVAS, extended)
        .with_layer_locks(0, LOCK_PIXELS)
        .unwrap()
        .unwrap();
    let cov = coverage(CANVAS, rect(2, 2, 3, 3));
    for request in [
        moved(2, 1),
        affine([0.0, 1.0, -1.0, 0.0, 7.0, 0.0], "bilinear"),
    ] {
        let err = edit16(&doc, &request.to_string(), &cov, None)
            .err()
            .unwrap();
        assert!(err.contains("locked"), "{request}: {err}");
    }
}

#[test]
fn a_cancelled_control_answers_operation_cancelled() {
    let doc = doc_of(CANVAS, extended);
    let cov = coverage(CANVAS, rect(2, 2, 5, 5));
    for request in [
        moved(2, 1),
        affine([0.0, 1.0, -1.0, 0.0, 9.0, 0.0], "bicubic"),
    ] {
        let control = DrJobControl::default();
        control.cancel();
        assert_eq!(
            edit16(&doc, &request.to_string(), &cov, Some(&control))
                .err()
                .unwrap(),
            "operation cancelled"
        );
    }
}

#[test]
fn the_float_capability_admits_the_layer_and_its_planes() {
    for family in ["selected_pixels_move", "selected_pixels_transform"] {
        for (target, supported) in [
            ("raster", true),
            ("mask", true),
            ("channel", true),
            ("red", true),
            ("green", true),
            ("blue", true),
            ("alpha", true),
            ("group", false),
        ] {
            assert_eq!(
                capability(family, target, 32)["supported"],
                supported,
                "{family} {target}@32"
            );
        }
    }
}
