//! Edit ▸ Puppet Warp at 16 bits and 32-bit float, and the
//! selection's float form at 8, 16 and 32 bits. The oracles are
//! `puppet_warp.rs`'s (the test's own Catmull-Rom resample, point location and
//! exact poses): within 1 code at 16 bits and 1e-5 at float, where the
//! oracle finishes with D2/D3 (an alpha below 2^-24 is transparent, RGB
//! clamped to the layer's own visible range).
#![allow(clippy::needless_range_loop)]

use crate::common;
use crate::float_fixture::float_doc;
use crate::puppet_warp::{
    check_identity, check_rotation, check_translation, describe_pose, ellipse, free_mesh,
    free_pose, solve, Samples, TestDoc, CANVAS, LIMIT, OFFSET, SIZE,
};

use common::selected_pixels::{edit16, edit8, typed_doc, typed_layer, Depth, Plane};
use doctored_core::doc_mask::MaskProperties;
use doctored_core::icc::IccProfile;
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_mask::{PrecisionMask, PrecisionPlane};
use serde_json::json;

fn typed16() -> PrecisionDocument {
    typed_doc(
        CANVAS,
        vec![typed_layer(&Plane::of(Depth::D16, OFFSET, SIZE, ellipse))],
    )
}

/// The ellipse at float: codes / 255, every other column's red lifted past
/// 1 (an extended value D3 must keep), hidden RGB under alpha 0.
fn typed32() -> PrecisionDocument {
    let record: Vec<[f32; 4]> = (0..CANVAS.0 * CANVAS.1)
        .map(|i| {
            let (cx, cy) = ((i % CANVAS.0) as i32, (i / CANVAS.0) as i32);
            let (x, y) = (cx - OFFSET.0, cy - OFFSET.1);
            if x < 0 || y < 0 || x >= SIZE.0 as i32 || y >= SIZE.1 as i32 {
                return [0.0; 4];
            }
            let p = ellipse(x as u32, y as u32).map(|v| f32::from(v) / 255.0);
            let lift = if x % 2 == 0 { 1.4 } else { 1.0 };
            [p[0] * lift, p[1], p[2], p[3]]
        })
        .collect();
    float_doc(CANVAS.0, CANVAS.1, &IccProfile::srgb(), &[record])
}

#[test]
fn unmoved_pins_change_nothing_at_16_bits_and_float() {
    check_identity(&TestDoc::Typed(typed16()));
    check_identity(&TestDoc::Typed(typed32()));
}

#[test]
fn a_translation_is_the_affine_resample_at_16_bits_and_float() {
    check_translation(&TestDoc::Typed(typed16()), 1.0);
    check_translation(&TestDoc::Typed(typed32()), 1e-5);
}

#[test]
fn a_rotation_rotates_the_mesh_and_the_pixels_at_16_bits_and_float() {
    check_rotation(&TestDoc::Typed(typed16()), 1.0);
    check_rotation(&TestDoc::Typed(typed32()), 1e-5);
}

#[test]
fn a_linked_mask_warps_along_and_an_unlinked_mask_stays_at_16_bits() {
    let gradient = |x: u32, y: u32| ((x * 1301 + y * 977) % 65536) as u16;
    for linked in [true, false] {
        let plane = Plane::of(Depth::D16, OFFSET, SIZE, ellipse);
        let samples: Vec<u16> = (0..SIZE.1)
            .flat_map(|y| (0..SIZE.0).map(move |x| gradient(x, y)))
            .collect();
        let mut properties = MaskProperties::at(OFFSET);
        properties.linked = linked;
        let mask = PrecisionMask::new(
            PrecisionPlane::from_u16(SIZE.0, SIZE.1, samples.clone(), None).unwrap(),
            properties,
            true,
        )
        .unwrap();
        let doc = typed_doc(CANVAS, vec![typed_layer(&plane).with_mask(Some(mask))]);
        let request = json!({"op":"puppet_warp","layer":0,"pins":[
            {"from":[28.0,23.0],"to":[34.0,21.0]},{"from":[18.0,23.0],"to":[24.0,21.0]}]});
        let Some(TestDoc::Typed(out)) = TestDoc::Typed(doc).warp(&request).unwrap() else {
            panic!("the warp changes the layer");
        };
        let layer = out.layer(0).unwrap();
        let after = layer.mask().unwrap();
        if !linked {
            assert_eq!(after.properties().offset, OFFSET);
            assert_eq!(after.plane().samples(), samples.as_slice());
            continue;
        }
        let origin = layer.offset();
        assert_eq!(after.properties().offset, origin);
        let (w, h) = after.plane().dimensions();
        let image = layer.source().unwrap();
        let mut checked = 0;
        for y in 0..h {
            for x in 0..w {
                let (cx, cy) = (origin.0 + x as i32, origin.1 + y as i32);
                let (sx, sy) = (cx - 6 - OFFSET.0, cy + 2 - OFFSET.1);
                let before = if sx >= 0 && sy >= 0 && (sx as u32) < SIZE.0 && (sy as u32) < SIZE.1 {
                    gradient(sx as u32, sy as u32)
                } else {
                    0
                };
                let got = after.plane().samples()[(y * w + x) as usize];
                if image.sample_at(x, y).unwrap()[3] > 0.0 {
                    assert_eq!(got, before, "mask at ({cx}, {cy})");
                    checked += 1;
                } else {
                    assert!(got == before || got == 0, "mask at ({cx}, {cy})");
                }
            }
        }
        assert!(checked > 100);
    }
}

/// The float form: pixels outside the selection and the landing are
/// unchanged, and the hole left inside the selection is `a·(1−c)` with the
/// RGB kept.
fn check_float_form(doc: TestDoc, tolerance: f64) {
    let coverage: Vec<u16> = (0..CANVAS.0 * CANVAS.1)
        .map(|i| {
            let (x, y) = (i % CANVAS.0, i / CANVAS.0);
            match (x, y) {
                (14..=26, 10..=33) => 65535,
                (27..=39, 10..=33) => 40000,
                _ => 0,
            }
        })
        .collect();
    let pins = json!([{"from":[28.0,23.0],"to":[48.0,37.0]},{"from":[20.0,20.0],"to":[40.0,34.0]}]);
    let mesh = doc.mesh_at(0, &json!({}), Some(&coverage), LIMIT).unwrap();
    let pose = solve(mesh, &json!({"mode":"rigid","pins":pins})).unwrap();
    let posed = describe_pose(mesh, pose);
    free_pose(pose);
    free_mesh(mesh);
    let low = posed
        .positions
        .iter()
        .fold([f64::INFINITY; 2], |a, p| [a[0].min(p[0]), a[1].min(p[1])]);
    let high = posed.positions.iter().fold([f64::NEG_INFINITY; 2], |a, p| {
        [a[0].max(p[0]), a[1].max(p[1])]
    });
    let landing = |x: i64, y: i64| {
        x >= low[0].floor() as i64
            && y >= low[1].floor() as i64
            && x < high[0].ceil() as i64
            && y < high[1].ceil() as i64
    };
    let text = json!({"op":"puppet_selected","layer":0,"mode":"rigid","pins":pins}).to_string();
    let after: Samples = match &doc {
        TestDoc::Byte(d) => {
            TestDoc::Byte(edit8(d, &text, &coverage, None).unwrap().unwrap()).layer(0)
        }
        TestDoc::Typed(d) => {
            TestDoc::Typed(edit16(d, &text, &coverage, None).unwrap().unwrap()).layer(0)
        }
    };
    let before = doc.layer(0);
    let (mut holes, mut kept) = (0, 0);
    for y in 0..CANVAS.1 as i64 {
        for x in 0..CANVAS.0 as i64 {
            if landing(x, y) {
                continue;
            }
            let old = before.at(x, y).unwrap_or([0.0; 4]);
            let new = after.at(x, y).unwrap_or([0.0; 4]);
            let c = f64::from(coverage[(y as u32 * CANVAS.0 + x as u32) as usize]) / 65535.0;
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
    }
    assert!(holes > 10 && kept > 100, "holes {holes}, kept {kept}");
}

#[test]
fn the_float_form_lands_and_leaves_its_hole_at_every_depth() {
    check_float_form(TestDoc::Byte(crate::puppet_warp::byte_doc()), 1.0);
    check_float_form(TestDoc::Typed(typed16()), 1.0);
    check_float_form(TestDoc::Typed(typed32()), 1e-6);
}
