//! Edit ▸ Perspective Warp at 16 bits and 32-bit float, and the
//! selection's float form at 8, 16 and 32 bits. The oracles are
//! `perspective_warp.rs`'s (the test's own homography and Catmull-Rom
//! resample): within 1 code at 16 bits and 1e-5 at float, where the oracle
//! finishes with D2/D3 (an alpha below 2^-24 is transparent, RGB clamped to
//! the layer's own visible range).
use crate::float_fixture::float_doc;
use crate::perspective_warp::{
    byte_doc, check_identity, check_keystone, quad, texture, CANVAS, OFFSET, SIZE,
};
use crate::puppet_warp::{Samples, TestDoc};

use crate::common::selected_pixels::{edit16, edit8, typed_doc, typed_layer, Depth, Plane};
use doctored_core::icc::IccProfile;
use doctored_core::perspective_warp::probe;
use doctored_core::precision_document::PrecisionDocument;
use serde_json::json;

fn typed16() -> PrecisionDocument {
    typed_doc(
        CANVAS,
        vec![typed_layer(&Plane::of(Depth::D16, OFFSET, SIZE, texture))],
    )
}

/// The texture at float: codes / 255, every other column's red lifted past
/// 1 (an extended value D3 must keep).
fn typed32() -> PrecisionDocument {
    let record: Vec<[f32; 4]> = (0..CANVAS.0 * CANVAS.1)
        .map(|i| {
            let (cx, cy) = ((i % CANVAS.0) as i32, (i / CANVAS.0) as i32);
            let (x, y) = (cx - OFFSET.0, cy - OFFSET.1);
            if x < 0 || y < 0 || x >= SIZE.0 as i32 || y >= SIZE.1 as i32 {
                return [0.0; 4];
            }
            let p = texture(x as u32, y as u32).map(|v| f32::from(v) / 255.0);
            let lift = if x % 2 == 0 { 1.4 } else { 1.0 };
            [p[0] * lift, p[1], p[2], p[3]]
        })
        .collect();
    float_doc(CANVAS.0, CANVAS.1, &IccProfile::srgb(), &[record])
}

#[test]
fn unmoved_corners_change_nothing_at_16_bits_and_float() {
    check_identity(&TestDoc::Typed(typed16()));
    check_identity(&TestDoc::Typed(typed32()));
}

#[test]
fn one_keystone_quad_is_the_homography_resample_at_16_bits_and_float() {
    check_keystone(&TestDoc::Typed(typed16()), 1.0);
    check_keystone(&TestDoc::Typed(typed32()), 1e-5);
}

/// The float form: pixels whose source lies well away from the float are
/// what the walk leaves: unchanged outside the selection, the hole
/// `a·(1−c)` with the RGB kept inside it.
fn check_float_form(doc: TestDoc, tolerance: f64) {
    let coverage: Vec<u16> = (0..CANVAS.0 * CANVAS.1)
        .map(|i| {
            let (x, y) = (i % CANVAS.0, i / CANVAS.0);
            match (x, y) {
                (30..=59, 30..=59) => 65535,
                (60..=69, 30..=59) => 40000,
                _ => 0,
            }
        })
        .collect();
    let to = [[68.0, 45.0], [118.0, 44.0], [119.0, 96.0], [66.0, 95.0]];
    let from = [[60.0, 40.0], [110.0, 40.0], [110.0, 90.0], [60.0, 90.0]];
    let quads = json!([quad(from, to)]);
    let text = json!({"op":"perspective_selected","layer":0,"quads":quads}).to_string();
    let after: Samples = match &doc {
        TestDoc::Byte(d) => {
            TestDoc::Byte(edit8(d, &text, &coverage, None).unwrap().unwrap()).layer(0)
        }
        TestDoc::Typed(d) => {
            TestDoc::Typed(edit16(d, &text, &coverage, None).unwrap().unwrap()).layer(0)
        }
    };
    let before = doc.layer(0);
    let points: Vec<[f64; 2]> = (0..CANVAS.0 * CANVAS.1)
        .map(|i| [f64::from(i % CANVAS.0) + 0.5, f64::from(i / CANVAS.0) + 0.5])
        .collect();
    let subject = (i64::from(OFFSET.0), i64::from(OFFSET.1), SIZE.0, SIZE.1);
    let mapped = probe(&json!({"quads":quads}).to_string(), subject, &points).unwrap();
    let (mut holes, mut kept) = (0, 0);
    for (i, source) in mapped.iter().enumerate() {
        let (x, y) = (
            i64::from(i as u32 % CANVAS.0),
            i64::from(i as u32 / CANVAS.0),
        );
        let near = source.is_some_and(|(_, s)| {
            s[0] > 30.0 - 3.0 && s[0] < 70.0 + 3.0 && s[1] > 30.0 - 3.0 && s[1] < 60.0 + 3.0
        });
        if near {
            continue;
        }
        let old = before.at(x, y).unwrap_or([0.0; 4]);
        let new = after.at(x, y).unwrap_or([0.0; 4]);
        let c = f64::from(coverage[i]) / 65535.0;
        if c == 0.0 {
            assert_eq!(new, old, "({x}, {y}) lies outside the selection");
            kept += 1;
            continue;
        }
        let want = old[3] * (1.0 - c);
        assert!(
            (new[3] - want).abs() <= tolerance,
            "hole alpha at ({x}, {y}): {} for {want}",
            new[3]
        );
        if new[3] > 0.0 {
            assert_eq!(&new[..3], &old[..3], "the hole keeps the RGB at ({x}, {y})");
            holes += 1;
        }
    }
    assert!(holes > 10 && kept > 100, "holes {holes}, kept {kept}");
}

#[test]
fn the_float_form_lands_and_leaves_its_hole_at_every_depth() {
    check_float_form(TestDoc::Byte(byte_doc()), 1.0);
    check_float_form(TestDoc::Typed(typed16()), 1.0);
    check_float_form(TestDoc::Typed(typed32()), 1e-6);
}
