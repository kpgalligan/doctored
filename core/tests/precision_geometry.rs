//! Typed geometry against exact index maps and independent f64 interpolation.
//! Fixtures contain genuine u16 codes, independent masks and latent RGB.
use crate::common;
use crate::group_roots;
use common::probe::{cancelled_parked_at, entry_then_cancel, parked_line};
use doctored_core::doc_mask::MaskProperties;
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_precision_edit::dr_precision_doc_edit;
use doctored_core::icc::IccProfile;
use doctored_core::job_control::DrJobControl;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer as Layer};
use doctored_core::precision_geometry::GeometryEdit;
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use doctored_core::precision_mask::{PrecisionChannel, PrecisionMask, PrecisionPlane};
use doctored_core::style::LayerStyle;
use group_roots::GroupRoots;
use serde_json::{json, Value};
use std::ffi::{c_char, CStr, CString};
use std::sync::Arc;
const LIMIT: u64 = REFERENCE_WORKING_BYTES;
const BOUND: f64 = 2e-6 + 0.5 / 65535.0;
fn edit(value: Value) -> GeometryEdit {
    GeometryEdit::parse(&value.to_string()).unwrap()
}
fn apply(doc: &PrecisionDocument, request: Value) -> PrecisionDocument {
    doc.geometry(&edit(request), LIMIT, 0, None)
        .unwrap()
        .unwrap()
}
fn image(w: u32, h: u32, pixels: Vec<[u16; 4]>, profile: &Arc<IccProfile>) -> PrecisionImage {
    PrecisionImage::from_rgba16(w, h, pixels, profile.clone(), None).unwrap()
}
fn doc(w: u32, h: u32, pixels: Vec<[u16; 4]>) -> PrecisionDocument {
    PrecisionDocument::from_image(
        image(w, h, pixels, &IccProfile::srgb()),
        Metadata::default(),
        Resolution::default(),
    )
    .unwrap()
}
fn source(doc: &PrecisionDocument) -> Vec<[u16; 4]> {
    doc.layer_source(0).unwrap().to_rgba16().unwrap()
}
fn mask(values: Vec<u16>, size: (u32, u32), offset: (i32, i32), linked: bool) -> PrecisionMask {
    let mut properties = MaskProperties::at(offset);
    properties.linked = linked;
    properties.density = 0.625;
    PrecisionMask::new(
        PrecisionPlane::from_u16(size.0, size.1, values, None).unwrap(),
        properties,
        true,
    )
    .unwrap()
}
fn permutation(op: &str, x: usize, y: usize, w: usize, h: usize) -> usize {
    let (x, y) = match op {
        "rotate90" => (y, h - 1 - x),
        "rotate180" => (w - 1 - x, h - 1 - y),
        "rotate270" => (w - 1 - y, x),
        "flip_horizontal" => (w - 1 - x, y),
        "flip_vertical" => (x, h - 1 - y),
        _ => unreachable!(),
    };
    y * w + x
}

#[test]
fn exact_document_transforms_preserve_every_u16_code_masks_channels_and_hidden_rgb() {
    let pixels: Vec<[u16; 4]> = (0..=65535u16)
        .map(|v| [v, v.wrapping_add(1), 65535 - v, v])
        .collect();
    let plane: Vec<u16> = (0..=65535u16).collect();
    let (width, height) = (512, 128);
    let base = doc(width, height, pixels.clone());
    let masked = base
        .set_layer_mask(
            0,
            Some(mask(plane.clone(), (width, height), (-3, 4), false)),
            LIMIT,
            0,
            None,
        )
        .unwrap()
        .unwrap();
    let channel = PrecisionChannel::new(
        "Native coverage",
        PrecisionPlane::from_u16(width, height, plane.clone(), None).unwrap(),
    )
    .unwrap();
    let original = PrecisionDocument::from_parts_and_channels(
        width,
        height,
        base.profile().clone(),
        Metadata::default(),
        Resolution { x: 300.0, y: 150.0 },
        masked.root_layers().to_vec(),
        vec![channel],
    )
    .unwrap();
    for op in [
        "rotate90",
        "rotate180",
        "rotate270",
        "flip_horizontal",
        "flip_vertical",
    ] {
        let output = apply(&original, json!({"op":op}));
        assert!(Arc::ptr_eq(output.profile(), original.profile()));
        let expected_ppi = if matches!(op, "rotate90" | "rotate270") {
            Resolution { x: 150.0, y: 300.0 }
        } else {
            original.resolution()
        };
        assert_eq!(output.resolution(), expected_ppi);
        let actual = source(&output);
        let actual_mask = output.layer(0).unwrap().mask().unwrap();
        assert!(!actual_mask.properties().linked);
        assert_eq!(actual_mask.properties().density, 0.625);
        let (ow, oh) = output.dimensions();
        assert_eq!(
            (ow, oh),
            if matches!(op, "rotate90" | "rotate270") {
                (height, width)
            } else {
                (width, height)
            }
        );
        for y in 0..oh as usize {
            for x in 0..ow as usize {
                let i = y * ow as usize + x;
                let old = permutation(op, x, y, width as usize, height as usize);
                assert_eq!(actual[i], pixels[old], "{op} at {i}");
                assert_eq!(actual_mask.plane().samples()[i], plane[old]);
                assert_eq!(output.channels()[0].plane().samples()[i], plane[old]);
            }
        }
        // Native persistence must carry the transformed dimensions, mask
        // placement/properties, channel precision and captured color profile.
        let bytes = output.encode_native(LIMIT, 0, None).unwrap();
        let reopened = PrecisionDocument::parse_native(&bytes, LIMIT, 0, None).unwrap();
        assert_eq!(source(&reopened), source(&output));
        assert!(
            reopened.layer(0).unwrap().mask().unwrap().properties() == actual_mask.properties()
        );
        assert_eq!(
            reopened.channels()[0].plane().samples(),
            output.channels()[0].plane().samples()
        );
    }
    assert_eq!(source(&original), pixels);
}

#[test]
fn leaf_translation_obeys_mask_links_and_crop_retains_off_canvas_storage() {
    let profile = IccProfile::srgb();
    let pixels = vec![
        [1001, 2002, 3003, 0],
        [1002, 2003, 3004, 1],
        [1003, 2004, 3005, 65535],
        [7, 8, 9, 32768],
        [10, 11, 12, 65535],
        [13, 14, 15, 256],
    ];
    let layer = Layer::raster(
        "Offset",
        image(3, 2, pixels.clone(), &profile),
        (2, 1),
        1.0,
        true,
    )
    .unwrap();
    let base = PrecisionDocument::from_parts(
        9,
        8,
        profile,
        Metadata::default(),
        Resolution::default(),
        vec![layer],
    )
    .unwrap();
    for linked in [false, true] {
        let original = base
            .set_layer_mask(
                0,
                Some(mask(vec![1, 2, 3, 4, 5, 6, 7, 8], (2, 4), (-1, 2), linked)),
                LIMIT,
                0,
                None,
            )
            .unwrap()
            .unwrap()
            .group_all(0.75)
            .unwrap()
            .unwrap();
        let translated = apply(
            &original,
            json!({"op":"affine","layer":0,"target":"pixels","matrix":[1,0,0,1,3,-2],"filter":"lanczos3"}),
        );
        assert_eq!(translated.layer(0).unwrap().offset(), (5, -1));
        assert!(PrecisionImage::ptr_eq(
            translated.layer_source(0).unwrap(),
            original.layer_source(0).unwrap()
        ));
        let a = translated.layer(0).unwrap().mask().unwrap();
        let b = original.layer(0).unwrap().mask().unwrap();
        assert_eq!(a.properties().offset, if linked { (2, 0) } else { (-1, 2) });
        assert_eq!(a.plane().samples().as_ptr(), b.plane().samples().as_ptr());
        let mask_only = apply(
            &translated,
            json!({"op":"affine","layer":0,"target":"mask","matrix":[1,0,0,1,-2,3],"filter":"bilinear"}),
        );
        assert!(PrecisionImage::ptr_eq(
            mask_only.layer_source(0).unwrap(),
            translated.layer_source(0).unwrap()
        ));
        assert_eq!(
            mask_only.layer(0).unwrap().offset(),
            translated.layer(0).unwrap().offset()
        );
        let cropped = apply(
            &mask_only,
            json!({"op":"crop","x":1,"y":2,"width":5,"height":4}),
        );
        assert_eq!(cropped.dimensions(), (5, 4));
        assert!(PrecisionImage::ptr_eq(
            cropped.layer_source(0).unwrap(),
            mask_only.layer_source(0).unwrap()
        ));
        assert_eq!(cropped.layer(0).unwrap().offset(), (4, -3));
        let old = mask_only.layer(0).unwrap().mask().unwrap();
        let new = cropped.layer(0).unwrap().mask().unwrap();
        assert_eq!(new.plane().dimensions(), old.plane().dimensions());
        assert_eq!(
            new.plane().samples().as_ptr(),
            old.plane().samples().as_ptr()
        );
        assert_eq!(
            new.properties().offset,
            (old.properties().offset.0 - 1, old.properties().offset.1 - 2)
        );
    }
}

fn weight(filter: &str, x: f64) -> f64 {
    let x = x.abs();
    match filter {
        "bilinear" => (1.0 - x).max(0.0),
        "bicubic" if x < 1.0 => 1.0 - 2.5 * x * x + 1.5 * x * x * x,
        "bicubic" if x < 2.0 => 2.0 - 4.0 * x + 2.5 * x * x - 0.5 * x * x * x,
        "bicubic" => 0.0,
        "lanczos3" if x < 1e-6 => 1.0,
        "lanczos3" if x < 3.0 => {
            let p = std::f64::consts::PI * x;
            (p.sin() / p) * ((p / 3.0).sin() / (p / 3.0))
        }
        "lanczos3" => 0.0,
        _ => unreachable!(),
    }
}
fn radius(filter: &str) -> i64 {
    match filter {
        "bilinear" => 1,
        "bicubic" => 2,
        "lanczos3" => 3,
        _ => unreachable!(),
    }
}
fn straight_reference(samples: &[[u16; 4]], taps: impl Iterator<Item = (usize, f64)>) -> [f64; 4] {
    let mut accumulated = [0.0; 4];
    for (i, w) in taps {
        let pixel = samples[i];
        let alpha = f64::from(pixel[3]) / 65535.0;
        for c in 0..3 {
            accumulated[c] += w * alpha * f64::from(pixel[c]) / 65535.0;
        }
        accumulated[3] += w * alpha;
    }
    let alpha = accumulated[3].clamp(0.0, 1.0);
    if (alpha * 65535.0).round() == 0.0 {
        return [0.0; 4];
    }
    [
        (accumulated[0] / accumulated[3]).clamp(0.0, 1.0),
        (accumulated[1] / accumulated[3]).clamp(0.0, 1.0),
        (accumulated[2] / accumulated[3]).clamp(0.0, 1.0),
        alpha,
    ]
}
fn assert_pixel(actual: [u16; 4], expected: [f64; 4]) {
    for (actual, expected) in actual.into_iter().zip(expected) {
        assert!(
            (f64::from(actual) / 65535.0 - expected).abs() <= BOUND,
            "got {actual}, expected {expected}"
        );
    }
}
fn fixture_pixels(w: usize, h: usize) -> Vec<[u16; 4]> {
    (0..w * h)
        .map(|i| {
            if i % 19 == 0 {
                [65535, 3, 60001, 0]
            } else {
                [
                    1001 + ((i * 7919) % 52000) as u16,
                    501 + ((i * 3571) % 54000) as u16,
                    701 + ((i * 2111) % 53000) as u16,
                    30001 + ((i * 1237) % 35000) as u16,
                ]
            }
        })
        .collect()
}
fn transform_reference(
    pixels: &[[u16; 4]],
    size: (usize, usize),
    position: (f64, f64),
    filter: &str,
) -> [f64; 4] {
    let r = radius(filter);
    let (u, v) = position;
    let start_x = (u - 0.5).floor() as i64 - r + 1;
    let start_y = (v - 0.5).floor() as i64 - r + 1;
    let wx: Vec<_> = (0..2 * r)
        .map(|i| weight(filter, u - 0.5 - (start_x + i) as f64))
        .collect();
    let wy: Vec<_> = (0..2 * r)
        .map(|i| weight(filter, v - 0.5 - (start_y + i) as f64))
        .collect();
    let total = wx.iter().sum::<f64>() * wy.iter().sum::<f64>();
    let mut taps = Vec::new();
    for (dy, wy) in wy.into_iter().enumerate() {
        for (dx, wx) in wx.iter().enumerate() {
            let (x, y) = (start_x + dx as i64, start_y + dy as i64);
            if x >= 0 && y >= 0 && x < size.0 as i64 && y < size.1 as i64 {
                taps.push((y as usize * size.0 + x as usize, wy * wx / total));
            }
        }
    }
    straight_reference(pixels, taps.into_iter())
}

#[test]
fn filtered_affine_uses_premultiplied_float_taps_without_byte_quantization() {
    let pixels = fixture_pixels(10, 8);
    let original = doc(10, 8, pixels.clone());
    for filter in ["bilinear", "bicubic", "lanczos3"] {
        let mut maximum = 0.0_f64;
        let output = apply(
            &original,
            json!({"op":"affine","layer":0,"target":"pixels","matrix":[1.15,0.12,-0.08,0.9,0.3,-0.2],"filter":filter}),
        );
        let layer = output.layer(0).unwrap();
        let (w, h) = layer.source().unwrap().dimensions();
        let offset = layer.offset();
        let determinant = 1.15 * 0.9 - 0.12 * (-0.08);
        for y in 0..h {
            for x in 0..w {
                let dx = f64::from(x) + f64::from(offset.0) + 0.5 - 0.3;
                let dy = f64::from(y) + f64::from(offset.1) + 0.5 + 0.2;
                let position = (
                    (0.9 * dx + 0.08 * dy) / determinant,
                    (-0.12 * dx + 1.15 * dy) / determinant,
                );
                let actual = source(&output)[y as usize * w as usize + x as usize];
                let expected = transform_reference(&pixels, (10, 8), position, filter);
                for c in 0..4 {
                    maximum = maximum.max((f64::from(actual[c]) / 65535.0 - expected[c]).abs());
                    assert!((f64::from(actual[c])/65535.0-expected[c]).abs()<=BOUND,
                    "{filter} ({x},{y}) channel {c}: actual {actual:?}, expected {expected:?}, source {position:?}");
                }
            }
        }
        eprintln!("typed {filter} affine maximum stored error: {maximum:.10}");
    }
    assert_eq!(source(&original), pixels);
}

#[test]
fn projective_trapezoid_matches_an_independently_solved_inverse() {
    let pixels = fixture_pixels(3, 2);
    let profile = IccProfile::srgb();
    let layer = Layer::raster(
        "Quad",
        image(3, 2, pixels.clone(), &profile),
        (2, 1),
        1.0,
        true,
    )
    .unwrap();
    let original = PrecisionDocument::from_parts(
        9,
        8,
        profile,
        Metadata::default(),
        Resolution::default(),
        vec![layer],
    )
    .unwrap();
    let output = apply(
        &original,
        json!({"op":"perspective","layer":0,"target":"pixels","quad":[2,1,5,1,6,4,1,4],"filter":"bilinear"}),
    );
    assert_eq!(output.layer(0).unwrap().offset(), (1, 1));
    assert_eq!(output.layer_source(0).unwrap().dimensions(), (5, 3));
    for y in 0..3 {
        for x in 0..5 {
            let cx = x as f64 + 1.5;
            let cy = y as f64 + 1.5;
            // For this trapezoid, x=(2+3s-1.4t)/(1-.4t),
            // y=(1+1.4t)/(1-.4t), solved here directly for (s,t).
            let t = (cy - 1.0) / (1.4 + 0.4 * cy);
            let s = (cx * (1.0 - 0.4 * t) - 2.0 + 1.4 * t) / 3.0;
            assert_pixel(
                source(&output)[y * 5 + x],
                transform_reference(&pixels, (3, 2), (3.0 * s, 2.0 * t), "bilinear"),
            );
        }
    }
}

#[test]
fn reduction_widens_filter_support_and_nearest_copies_exact_samples() {
    let pixels = fixture_pixels(12, 8);
    let original = doc(12, 8, pixels.clone());
    for filter in ["bilinear", "bicubic", "lanczos3"] {
        let output = apply(
            &original,
            json!({"op":"resize","width":4,"height":3,"filter":filter}),
        );
        for y in 0..3 {
            for x in 0..4 {
                let weights = |source: usize, target: usize, index: usize| -> Vec<(usize, f64)> {
                    let scale = source as f64 / target as f64;
                    let center = (index as f64 + 0.5) * scale;
                    let support = radius(filter) as f64 * scale.max(1.0);
                    let mut values: Vec<_> = (0..source)
                        .filter(|&i| {
                            (i as f64) >= (center - support).floor()
                                && (i as f64) < (center + support).ceil()
                        })
                        .map(|i| {
                            (
                                i,
                                weight(filter, (i as f64 + 0.5 - center) / scale.max(1.0)),
                            )
                        })
                        .collect();
                    let sum = values.iter().map(|v| v.1).sum::<f64>();
                    for v in &mut values {
                        v.1 /= sum;
                    }
                    values
                };
                let wx = weights(12, 4, x);
                let wy = weights(8, 3, y);
                let mut taps = Vec::new();
                for (sy, wy) in wy {
                    for &(sx, wx) in &wx {
                        taps.push((sy * 12 + sx, wy * wx));
                    }
                }
                assert_pixel(
                    source(&output)[y * 4 + x],
                    straight_reference(&pixels, taps.into_iter()),
                );
            }
        }
    }
    let nearest = apply(
        &original,
        json!({"op":"resize","width":7,"height":11,"filter":"nearest"}),
    );
    for y in 0..11 {
        for x in 0..7 {
            assert_eq!(
                source(&nearest)[y * 7 + x],
                pixels[((2 * y + 1) * 8 / (2 * 11)) * 12 + (2 * x + 1) * 12 / (2 * 7)]
            );
        }
    }
}

#[test]
fn admission_strict_requests_and_identity_precede_destination_allocation() {
    for request in [
        r#"{"op":"rotate90","unexpected":true}"#,
        r#"{"op":"resize","width":1,"height":1,"filter":"gaussian"}"#,
        r#"{"op":"crop","x":-1,"y":0,"width":1,"height":1}"#,
    ] {
        assert!(GeometryEdit::parse(request).is_err(), "{request}");
    }
    let original = doc(17, 11, fixture_pixels(17, 11));
    // A near-exact matrix qualifies for the copy path only when its rounded
    // extent also matches that permutation. Otherwise the requested filtered
    // alpha boundary applies, including zero RGB at zero output alpha.
    let tiny = doc(4, 1, vec![[60001, 3, 50001, 0]; 4]);
    let expanded = apply(
        &tiny,
        json!({"op":"affine","layer":0,"target":"pixels","matrix":[1.0000000005,0,0,1,0,0],"filter":"bilinear"}),
    );
    assert_eq!(expanded.layer_source(0).unwrap().dimensions(), (5, 1));
    assert!(source(&expanded).iter().all(|pixel| *pixel == [0; 4]));
    for request in [
        json!({"op":"resize","width":17,"height":11,"filter":"bilinear"}),
        json!({"op":"crop","x":0,"y":0,"width":17,"height":11}),
        json!({"op":"affine","layer":0,"target":"pixels","matrix":[1,0,0,1,0,0],"filter":"lanczos3"}),
    ] {
        assert!(original
            .geometry(&edit(request), LIMIT, 0, None)
            .unwrap()
            .is_none());
    }
    for request in [
        json!({"op":"crop","x":1,"y":0,"width":17,"height":11}),
        json!({"op":"resize","width":0,"height":1,"filter":"nearest"}),
        json!({"op":"affine","layer":0,"target":"pixels","matrix":[0,0,0,1,0,0],"filter":"nearest"}),
        json!({"op":"perspective","layer":0,"target":"pixels","quad":[0,0,10,10,0,10,10,0],"filter":"nearest"}),
    ] {
        assert!(original.geometry_plan(&edit(request)).is_err());
    }
    let grouped = original.group_all(1.0).unwrap().unwrap();
    assert!(grouped.geometry_plan(&edit(json!({"op":"affine","layer":1,"target":"pixels","matrix":[1,0,0,1,1,0],"filter":"nearest"}))).is_err());
    let request = edit(json!({"op":"resize","width":31,"height":23,"filter":"bicubic"}));
    let plan = original.geometry_plan(&request).unwrap();
    let token = DrJobControl::default();
    assert!(original
        .geometry(
            &request,
            plan.retained_bytes + plan.scratch_bytes - 1,
            0,
            Some(&token)
        )
        .is_err());
    assert_eq!(token.scratch_bytes(), 0);
    // A 100000×1 row resized to 1×100000 at Lanczos3 is past the 2·10⁹ work
    // units the plan used to refuse at. A commit runs under a job
    // control, so the plan answers, and a run entered cancelled stops there.
    let wide = doc(100000, 1, vec![[1, 2, 3, 65535]; 100000]);
    let reshape = edit(json!({"op":"resize","width":1,"height":100000,"filter":"lanczos3"}));
    let plan = wide
        .geometry_plan(&reshape)
        .expect("the resize plan answers");
    assert!(plan.work_units > 2_000_000_000, "{}", plan.work_units);
    let cancelled = DrJobControl::default();
    cancelled.cancel();
    assert_eq!(
        wide.geometry(&reshape, LIMIT, 0, Some(&cancelled))
            .err()
            .as_deref(),
        Some("operation cancelled")
    );
    assert_eq!(cancelled.scratch_bytes(), 0);
}

#[test]
fn cancellation_during_filtered_geometry_releases_scratch_and_keeps_source() {
    let original = doc(1600, 1000, fixture_pixels(1600, 1000));
    let render = |token: usize| {
        let control = unsafe { &*(token as *const DrJobControl) };
        match original.geometry(&edit(json!({"op":"affine","layer":0,"target":"pixels","matrix":[1.07,0.03,-0.02,1.03,0.3,0.4],"filter":"lanczos3"})),LIMIT,0,Some(control)) {
            Ok(_) => (true, String::new()),
            Err(error) => (false, error),
        }
    };
    // k = 1: the sampling polls twice between its reports (the next report is at n0 + 3).
    let (entry, next) = entry_then_cancel(&|progress, _, _| progress > 0.0, render);
    let n0 = entry.checkpoints;
    assert!(
        next.expect("the entry's kernel never reported again")
            .checkpoints
            > n0 + 2,
        "the entry's kernel polls fewer than two checkpoints before its next report"
    );
    let parked = cancelled_parked_at(
        n0 + 2,
        &|progress, stage| stage == entry.stage && progress == entry.progress,
        render,
    );
    eprintln!(
        "typed filtered geometry cancellation: {}",
        parked_line(&parked)
    );
    assert_eq!(source(&original)[1], [8920, 4072, 2812, 31238]);
    let fresh = DrJobControl::default();
    assert!(original.geometry(&edit(json!({"op":"affine","layer":0,"target":"pixels","matrix":[1,0,0,1,1,0],"filter":"nearest"})),LIMIT,0,Some(&fresh)).unwrap().is_some());
    assert_eq!(fresh.scratch_bytes(), 0);
}

#[test]
fn whole_tree_resize_and_crop_include_group_masks_adjustments_and_channels() {
    let base = doc(6, 4, fixture_pixels(6, 4));
    let grouped = base.group_all(0.75).unwrap().unwrap();
    let description =
        json!({"type":"adjust","evaluation":"precision_v1","op":"invert","params":{}}).to_string();
    let adjusted = grouped
        .add_adjustment("Invert", &description, 0.5, LIMIT, 0, None)
        .unwrap()
        .unwrap();
    let original = adjusted
        .set_layer_mask(
            1,
            Some(mask(vec![1, 2, 32768, 65535], (2, 2), (-2, 1), false)),
            LIMIT,
            0,
            None,
        )
        .unwrap()
        .unwrap()
        .set_layer_mask(
            2,
            Some(mask(vec![3, 10001, 60001], (3, 1), (2, 0), true)),
            LIMIT,
            0,
            None,
        )
        .unwrap()
        .unwrap();
    let channel = PrecisionChannel::new(
        "Canvas plane",
        PrecisionPlane::from_u16(6, 4, (0..24).map(|i| i * 2711).collect(), None).unwrap(),
    )
    .unwrap();
    let original = PrecisionDocument::from_parts_and_channels(
        6,
        4,
        original.profile().clone(),
        original.metadata().clone(),
        original.resolution(),
        original.root_layers().to_vec(),
        vec![channel],
    )
    .unwrap();
    let output = apply(
        &original,
        json!({"op":"resize","width":12,"height":8,"filter":"nearest"}),
    );
    assert_eq!(output.layer_count(), original.layer_count());
    assert_eq!(
        output.adjustment_description(2),
        original.adjustment_description(2)
    );
    assert_eq!(output.layer(1).unwrap().opacity(), 0.75);
    for index in [1, 2] {
        let before = original.layer(index).unwrap().mask().unwrap();
        let after = output.layer(index).unwrap().mask().unwrap();
        let (w, h) = before.plane().dimensions();
        assert_eq!(after.plane().dimensions(), (w * 2, h * 2));
        assert_eq!(
            after.properties().offset,
            (
                before.properties().offset.0 * 2,
                before.properties().offset.1 * 2
            )
        );
        assert_eq!(after.properties().linked, before.properties().linked);
        for y in 0..h * 2 {
            for x in 0..w * 2 {
                assert_eq!(
                    after.plane().samples()[(y * w * 2 + x) as usize],
                    before.plane().samples()[((y / 2) * w + x / 2) as usize]
                );
            }
        }
    }
    for y in 0..8 {
        for x in 0..12 {
            assert_eq!(
                output.channels()[0].plane().samples()[y * 12 + x],
                original.channels()[0].plane().samples()[(y / 2) * 6 + x / 2]
            );
        }
    }
    let crop = apply(
        &output,
        json!({"op":"crop","x":1,"y":2,"width":8,"height":5}),
    );
    assert_eq!(crop.channels()[0].id(), output.channels()[0].id());
    for y in 0..5 {
        for x in 0..8 {
            assert_eq!(
                crop.channels()[0].plane().samples()[y * 8 + x],
                output.channels()[0].plane().samples()[(y + 2) * 12 + x + 1]
            );
        }
    }
    for index in [1, 2] {
        let before = output.layer(index).unwrap().mask().unwrap();
        let after = crop.layer(index).unwrap().mask().unwrap();
        assert_eq!(
            after.plane().samples().as_ptr(),
            before.plane().samples().as_ptr()
        );
        assert_eq!(
            after.properties().offset,
            (
                before.properties().offset.0 - 1,
                before.properties().offset.1 - 2
            )
        );
    }
}

/// A 1×1 canvas holding a background and an isolated group "Styled" whose
/// inner shadow (distance 310, size 0, choke 0) pads its plane by 312. A
/// group's planned plane is the NEW canvas, so Canvas Size alone moves it
/// across the limit with the style factor at exactly 1.
fn styled_group() -> PrecisionDocument {
    let profile = IccProfile::srgb();
    let raster = |name: &str| {
        Layer::raster(
            name,
            image(1, 1, vec![[40_000, 20_000, 9_000, 65_535]], &profile),
            (0, 0),
            1.0,
            true,
        )
        .unwrap()
    };
    let style = LayerStyle::from_json(
        r#"{"effects":[{"type":"inner_shadow","distance":310,"size":0,"choke":0}]}"#,
    )
    .unwrap();
    assert_eq!(style.pad(), 312);
    let group = Layer::isolated_normal("Styled", vec![raster("Child")], 1.0, true)
        .unwrap()
        .with_layer_style(Some(&style), (1, 1))
        .unwrap();
    PrecisionDocument::from_parts(
        1,
        1,
        profile.clone(),
        Metadata::default(),
        Resolution::default(),
        vec![raster("Background"), group],
    )
    .unwrap()
}

/// One typed geometry request through the FFI: the new handle, or the
/// sentence it refused with.
fn ffi_geometry(doc: &PrecisionDocument, request: Value) -> Result<Box<PrecisionDocument>, String> {
    let request = CString::new(request.to_string()).unwrap();
    let mut error: *mut c_char = std::ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_edit(
            doc,
            request.as_ptr(),
            std::ptr::null(),
            0,
            LIMIT,
            0,
            std::ptr::null(),
            &mut error,
        )
    };
    if out.is_null() {
        assert!(!error.is_null(), "a refusal carries a sentence");
        let message = unsafe { CStr::from_ptr(error) }
            .to_str()
            .unwrap()
            .to_owned();
        unsafe { dr_string_free(error) };
        return Err(message);
    }
    Ok(unsafe { Box::from_raw(out) })
}

#[test]
fn a_style_plane_past_the_pixel_limit_after_geometry_refuses_and_the_limit_itself_passes() {
    let doc = styled_group();
    let style = doc.layer(2).unwrap().style().unwrap().clone();
    let before = doc.render(LIMIT, 0, None).unwrap().to_rgba16().unwrap();

    // (1 + 2·312)·(159_377 + 2·312) = 100_000_625 > MAX_PIXELS (100 MP).
    let refusal = ffi_geometry(
        &doc,
        json!({"op":"canvas_size","width":1,"height":159_377,"x":0,"y":0}),
    )
    .err()
    .expect("one pixel row past the limit refuses");
    assert_eq!(
        refusal,
        "the layer style plane of “Styled” would exceed the pixel limit after this geometry; reduce the effect sizes or clear its style first"
    );
    assert_eq!(doc.dimensions(), (1, 1));
    assert!(Arc::ptr_eq(doc.layer(2).unwrap().style().unwrap(), &style));
    assert_eq!(
        doc.render(LIMIT, 0, None).unwrap().to_rgba16().unwrap(),
        before
    );

    // (1 + 624)·(159_376 + 624) = 100_000_000 exactly: the gate is `>`, so
    // the limit itself is admitted and the style rides along. Never render
    // this one — it would build the whole 100 MP plane.
    let grown = ffi_geometry(
        &doc,
        json!({"op":"canvas_size","width":1,"height":159_376,"x":0,"y":0}),
    )
    .expect("the at-limit canvas is admitted");
    assert_eq!(grown.dimensions(), (1, 159_376));
    assert!(Arc::ptr_eq(
        grown.layer(2).unwrap().style().unwrap(),
        &style
    ));
}

mod bench {
    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    #[ignore = "make cancel-bench: a wall-clock verdict"]
    fn cancellation_during_filtered_geometry_releases_scratch_and_keeps_source() {
        let original = Arc::new(doc(1600, 1000, fixture_pixels(1600, 1000)));
        let token = Arc::new(DrJobControl::default());
        let input = original.clone();
        let control = token.clone();
        let worker = std::thread::spawn(move || {
            input.geometry(&edit(json!({"op":"affine","layer":0,"target":"pixels","matrix":[1.07,0.03,-0.02,1.03,0.3,0.4],"filter":"lanczos3"})),LIMIT,0,Some(&control))
        });
        let deadline = Instant::now() + Duration::from_secs(10);
        while token.progress() == 0.0 && !worker.is_finished() && Instant::now() < deadline {
            std::thread::yield_now();
        }
        assert!(
            token.progress() > 0.0 && !worker.is_finished(),
            "must cancel during numerical sampling"
        );
        let start = Instant::now();
        token.cancel();
        let result = worker.join().unwrap();
        let elapsed = start.elapsed();
        assert!(result.err().unwrap().contains("cancelled"));
        assert_eq!(token.scratch_bytes(), 0);
        eprintln!(
            "typed filtered geometry cancellation: {:.3} ms",
            elapsed.as_secs_f64() * 1000.0
        );
        assert!(elapsed < Duration::from_secs(2));
        assert_eq!(source(&original)[1], [8920, 4072, 2812, 31238]);
        let fresh = DrJobControl::default();
        assert!(original.geometry(&edit(json!({"op":"affine","layer":0,"target":"pixels","matrix":[1,0,0,1,1,0],"filter":"nearest"})),LIMIT,0,Some(&fresh)).unwrap().is_some());
        assert_eq!(fresh.scratch_bytes(), 0);
    }
}
