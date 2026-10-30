//! Whole-canvas layer-mask Invert at 16 bits: the `scalar_edit` Invert
//! filter on `target: mask`, through `dr_precision_doc_edit`.
//!
//! The oracle is the canvas-wide mask plane (`read_plane`, `canvas: true`)
//! before and after: with no selection every canvas sample reads
//! `65535 − before`; with a selection every canvas sample reads the f64
//! interpolation of `before` and `65535 − before` by its coverage.
use crate::native_scalar::{doc, edit, plan, plane, LIMIT};
use doctored_core::doc_mask::MaskProperties;
use doctored_core::icc::IccProfile;
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer};
use doctored_core::precision_image::PrecisionImage;
use doctored_core::precision_mask::{PrecisionMask, PrecisionPlane};
use serde_json::{json, Value};

const W: u32 = 9;
const H: u32 = 7;

fn mask(w: u32, h: u32, offset: (i32, i32), outside: u8) -> PrecisionMask {
    let values: Vec<u16> = (0..w * h).map(|i| (i * 9973 + 4111) as u16).collect();
    let mut properties = MaskProperties::at(offset);
    properties.outside_coverage = outside;
    PrecisionMask::new(
        PrecisionPlane::from_u16(w, h, values, None).unwrap(),
        properties,
        true,
    )
    .unwrap()
}

/// A 9×7 16-bit document whose one layer carries `mask`.
fn masked(mask: PrecisionMask) -> PrecisionDocument {
    doc(W, H, vec![[13001, 22002, 31003, 65535]; (W * H) as usize])
        .set_layer_mask(0, Some(mask), LIMIT, 0, None)
        .unwrap()
        .unwrap()
}

/// The request the host sends for Image ▸ Adjustments ▸ Invert on a mask.
fn request(index: usize, selection: bool) -> Value {
    let description =
        json!({"type":"adjust","op":"invert","params":{},"evaluation":"precision_v1"}).to_string();
    json!({"op":"scalar_edit","target":"mask","index":index,"selection":selection,
        "operation":{"kind":"filter","edit":{"layer":0,"op":"adjustment","description":description}}})
}

fn selection() -> Vec<u16> {
    (0..W * H)
        .map(|i| [0u16, 65535, 32768, 65535, 1, 16384, 0, 65534][(i % 8) as usize])
        .collect()
}

/// `before` and `65535 − before` interpolated by coverage `q`, in f64.
fn scoped(before: u16, q: u16) -> f64 {
    let t = f64::from(q) / 65535.;
    f64::from(before) * (1. - t) + f64::from(65535 - before) * t
}

#[test]
fn no_selection_inverts_inside_and_outside_the_stored_rectangle() {
    for outside in [0u8, 255] {
        let d = masked(mask(3, 2, (4, 3), outside));
        let before = plane(&d, "mask", true);
        let out = edit(&d, request(0, false), &[]).expect("an inverted mask");
        let after = plane(&out, "mask", true);
        assert_eq!(after.len(), (W * H) as usize);
        for (i, (&b, &a)) in before.iter().zip(&after).enumerate() {
            assert_eq!(a, 65535 - b, "canvas sample {i}, outside {outside}");
        }
        let m = out.layer(0).unwrap().mask().unwrap();
        assert_eq!(m.plane().dimensions(), (3, 2));
        assert_eq!(m.properties().offset, (4, 3));
        assert_eq!(m.properties().outside_coverage, 255 - outside);
        // Twice is the original plane and outside constant.
        let back = edit(&out, request(0, false), &[]).expect("a second inversion");
        assert_eq!(plane(&back, "mask", true), before);
        let m = back.layer(0).unwrap().mask().unwrap();
        assert_eq!(m.properties().outside_coverage, outside);
        assert_eq!(
            m.plane().samples(),
            d.layer(0).unwrap().mask().unwrap().plane().samples()
        );
    }
}

#[test]
fn a_selection_inverts_by_coverage_across_the_whole_canvas() {
    for outside in [0u8, 255] {
        // The stored plane hangs off the canvas' left and bottom edges.
        let d = masked(mask(3, 3, (-1, 5), outside));
        let before = plane(&d, "mask", true);
        let stored = d
            .layer(0)
            .unwrap()
            .mask()
            .unwrap()
            .plane()
            .samples()
            .to_vec();
        let selection = selection();
        // The plan and the edit agree on the grown plane.
        let planned = plan(&d, request(0, true));
        assert_eq!(planned["width"], 10);
        assert_eq!(planned["height"], 8);
        assert_eq!(planned["x"], -1);
        assert_eq!(planned["y"], 0);
        let out = edit(&d, request(0, true), &selection).expect("a scoped inversion");
        let after = plane(&out, "mask", true);
        for i in 0..(W * H) as usize {
            let want = scoped(before[i], selection[i]);
            // Coverage 0 and 65535 are exact; a partial weight rounds once
            // from the filter's f32 evaluation.
            let slack = if matches!(selection[i], 0 | 65535) {
                0.
            } else {
                1.
            };
            assert!(
                (f64::from(after[i]) - want).abs() <= 0.5 + slack,
                "canvas sample {i}, outside {outside}: {} against {want}",
                after[i]
            );
        }
        let m = out.layer(0).unwrap().mask().unwrap();
        assert_eq!(m.plane().dimensions(), (10, 8));
        assert_eq!(m.properties().offset, (-1, 0));
        assert_eq!(m.properties().outside_coverage, outside);
        // Stored samples outside the canvas are not selected: unchanged.
        for (i, &value) in stored.iter().enumerate() {
            let (x, y) = (i as i32 % 3 - 1, i as i32 / 3 + 5);
            if x >= 0 && y < H as i32 {
                continue;
            }
            let at = y as usize * 10 + (x + 1) as usize;
            assert_eq!(m.plane().samples()[at], value, "stored sample {i}");
        }
    }
}

#[test]
fn a_selection_that_selects_nothing_is_a_no_op() {
    let d = masked(mask(3, 2, (4, 3), 255));
    assert!(edit(&d, request(0, true), &vec![0u16; (W * H) as usize]).is_none());
}

#[test]
fn a_group_mask_keeps_its_rectangle() {
    let image = PrecisionImage::from_rgba16(
        W,
        H,
        vec![[500, 600, 700, 65535]; (W * H) as usize],
        IccProfile::srgb(),
        None,
    )
    .unwrap();
    let child = PrecisionDocumentLayer::raster("Child", image, (0, 0), 1.0, true).unwrap();
    let group = PrecisionDocumentLayer::isolated_normal("Group", vec![child], 1.0, true)
        .unwrap()
        .with_mask(Some(mask(4, 3, (7, 5), 0)));
    let d = PrecisionDocument::from_parts(
        W,
        H,
        IccProfile::srgb(),
        Default::default(),
        Default::default(),
        vec![group],
    )
    .unwrap();
    // Postorder: the child is record 0, the group record 1.
    let stored = d
        .layer(1)
        .unwrap()
        .mask()
        .unwrap()
        .plane()
        .samples()
        .to_vec();
    let planned = plan(&d, request(1, true));
    assert_eq!(planned["width"], 4);
    assert_eq!(planned["height"], 3);
    let selection = vec![65535u16; (W * H) as usize];
    let out = edit(&d, request(1, true), &selection).expect("a scoped group inversion");
    let m = out.layer(1).unwrap().mask().unwrap();
    assert_eq!(m.plane().dimensions(), (4, 3));
    assert_eq!(m.properties().offset, (7, 5));
    assert_eq!(m.properties().outside_coverage, 0);
    for (i, &value) in stored.iter().enumerate() {
        let (cx, cy) = (i as u32 % 4 + 7, i as u32 / 4 + 5);
        let want = if cx < W && cy < H {
            65535 - value
        } else {
            value
        };
        assert_eq!(m.plane().samples()[i], want, "stored sample {i}");
    }
}

#[test]
fn another_filter_on_a_mask_keeps_the_stored_rectangle_and_outside() {
    // Only Invert follows the whole-canvas rule: a pixelate edit still writes
    // the stored samples alone.
    let d = masked(mask(3, 2, (4, 3), 255));
    let pixelate = json!({"op":"scalar_edit","target":"mask","index":0,"selection":true,
        "operation":{"kind":"filter","edit":{"layer":0,"op":"pixelate","params":{"block":2}}}});
    let planned = plan(&d, pixelate.clone());
    assert_eq!(planned["width"], 3);
    assert_eq!(planned["height"], 2);
    let out = edit(&d, pixelate, &vec![65535u16; (W * H) as usize]).expect("a pixelate edit");
    let m = out.layer(0).unwrap().mask().unwrap();
    assert_eq!(m.plane().dimensions(), (3, 2));
    assert_eq!(m.properties().offset, (4, 3));
    assert_eq!(m.properties().outside_coverage, 255);
}
