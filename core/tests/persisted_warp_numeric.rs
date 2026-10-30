//! Independent binary64 field, Newton inverse, kernel and premultiplied oracle.
use crate::warp_fixture as fixture;
use crate::warp_oracle;

use doctored_core::doc::LayerRaster;
use doctored_core::doc_mask::MaskProperties;
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer};
use doctored_core::precision_image::PrecisionImage;
use doctored_core::precision_mask::{PrecisionMask, PrecisionPlane};
use doctored_core::warp::{WarpEdit, WarpField};
use fixture::{byte_document, edit, field, typed_document, LIMIT, OFFSET};
use image::GrayImage;
use serde_json::Value;
use std::sync::Arc;
use warp_oracle::{bounds, inverse, nodes, taps};

fn reference<const MAX: u32>(
    request: &Value,
    source: &[[u32; 4]],
    size: (u32, u32),
    offset: (i32, i32),
) -> ((i32, i32, u32, u32), Vec<[u32; 4]>) {
    let (c, r, n, bicubic) = nodes(request);
    let rect = (offset.0, offset.1, size.0, size.1);
    let dest = bounds(c, r, &n, rect);
    let mut out = Vec::new();
    for y in 0..dest.3 {
        for x in 0..dest.2 {
            let q = [
                f64::from(dest.0) + f64::from(x) + 0.5,
                f64::from(dest.1) + f64::from(y) + 0.5,
            ];
            let p = inverse(c, r, &n, rect, q);
            let xt = taps(p[0] - f64::from(offset.0), bicubic);
            let yt = taps(p[1] - f64::from(offset.1), bicubic);
            let mut acc = [0.0; 4];
            for (sy, wy) in &yt {
                for (sx, wx) in &xt {
                    if *sx < 0 || *sy < 0 || *sx >= i64::from(size.0) || *sy >= i64::from(size.1) {
                        continue;
                    }
                    let px = source[*sy as usize * size.0 as usize + *sx as usize];
                    let w = wy * wx;
                    let a = f64::from(px[3]);
                    for k in 0..3 {
                        acc[k] += w * a * f64::from(px[k]) / f64::from(MAX);
                    }
                    acc[3] += w * a;
                }
            }
            let a = acc[3].clamp(0.0, f64::from(MAX)).round() as u32;
            out.push(if a == 0 {
                [0; 4]
            } else {
                [
                    (acc[0] * f64::from(MAX) / acc[3])
                        .clamp(0.0, f64::from(MAX))
                        .round() as u32,
                    (acc[1] * f64::from(MAX) / acc[3])
                        .clamp(0.0, f64::from(MAX))
                        .round() as u32,
                    (acc[2] * f64::from(MAX) / acc[3])
                        .clamp(0.0, f64::from(MAX))
                        .round() as u32,
                    a,
                ]
            });
        }
    }
    (dest, out)
}
fn field_ref(edit: &WarpEdit) -> &WarpField {
    match edit {
        WarpEdit::Set { field, .. } => field,
        _ => unreachable!(),
    }
}

fn scalar_reference<const MAX: u32>(
    request: &Value,
    source: &[u32],
    size: (u32, u32),
    offset: (i32, i32),
    outside: u32,
) -> ((i32, i32, u32, u32), Vec<u32>) {
    let (c, r, n, bicubic) = nodes(request);
    let rect = (offset.0, offset.1, size.0, size.1);
    let dest = bounds(c, r, &n, rect);
    let mut out = Vec::new();
    for y in 0..dest.3 {
        for x in 0..dest.2 {
            let q = [
                f64::from(dest.0) + f64::from(x) + 0.5,
                f64::from(dest.1) + f64::from(y) + 0.5,
            ];
            let p = inverse(c, r, &n, rect, q);
            let xt = taps(p[0] - f64::from(offset.0), bicubic);
            let yt = taps(p[1] - f64::from(offset.1), bicubic);
            let mut acc = 0.0;
            for (sy, wy) in &yt {
                for (sx, wx) in &xt {
                    if *sx < 0 || *sy < 0 || *sx >= i64::from(size.0) || *sy >= i64::from(size.1) {
                        continue;
                    }
                    acc += wy
                        * wx
                        * (f64::from(source[*sy as usize * size.0 as usize + *sx as usize])
                            - f64::from(outside));
                }
            }
            out.push(
                (acc + f64::from(outside))
                    .clamp(0.0, f64::from(MAX))
                    .round() as u32,
            );
        }
    }
    (dest, out)
}

#[test]
fn binary64_oracle_matches_byte_and_native_u16_for_both_lattices_and_filters() {
    for (columns, rows) in [(3, 3), (4, 2)] {
        for filter in ["bilinear", "bicubic"] {
            let request = field(columns, rows, filter);
            let parsed = edit(request.clone());
            field_ref(&parsed).validate().unwrap();
            let byte = byte_document();
            let b = byte.warp_edit(&parsed, LIMIT, 0, None).unwrap().unwrap();
            let input: Vec<[u32; 4]> = byte.layers[0]
                .pixels
                .contiguous_image()
                .as_raw()
                .chunks_exact(4)
                .map(|v| [v[0] as u32, v[1] as u32, v[2] as u32, v[3] as u32])
                .collect();
            let (bounds, expected) =
                reference::<255>(&request, &input, (fixture::W, fixture::H), OFFSET);
            assert_eq!(
                (
                    b.layers[0].offset.0,
                    b.layers[0].offset.1,
                    b.layers[0].pixels.width(),
                    b.layers[0].pixels.height()
                ),
                bounds
            );
            let mut max_error = 0u32;
            let mut changed = 0usize;
            for (i, actual) in b.layers[0]
                .pixels
                .contiguous_image()
                .as_raw()
                .chunks_exact(4)
                .enumerate()
            {
                for k in 0..4 {
                    let delta = actual[k].abs_diff(expected[i][k] as u8) as u32;
                    max_error = max_error.max(delta);
                    assert!(
                        delta <= 1,
                        "byte {columns}x{rows} {filter} pixel {i} channel {k}: {} vs {}",
                        actual[k],
                        expected[i][k]
                    );
                }
                if expected[i][3] == 0 {
                    assert_eq!(actual, [0; 4]);
                }
                if i < input.len()
                    && actual
                        .iter()
                        .zip(input[i])
                        .any(|(&a, b)| u32::from(a).abs_diff(b) >= 8)
                {
                    changed += 1;
                }
            }
            assert!(changed > 0, "byte non-vacuity");
            let typed = typed_document();
            let t = typed.warp_edit(&parsed, LIMIT, 0, None).unwrap().unwrap();
            let raw = typed.layer_source(0).unwrap().to_rgba16().ok().unwrap();
            let input: Vec<[u32; 4]> = raw.iter().map(|p| p.map(u32::from)).collect();
            let (bounds, expected) =
                reference::<65535>(&request, &input, (fixture::W, fixture::H), OFFSET);
            let layer = t.layer(0).unwrap();
            assert_eq!(
                (
                    layer.offset().0,
                    layer.offset().1,
                    layer.source().unwrap().dimensions().0,
                    layer.source().unwrap().dimensions().1
                ),
                bounds
            );
            let mut max_typed = 0u32;
            let mut changed = 0usize;
            for (i, actual) in layer
                .source()
                .unwrap()
                .to_rgba16()
                .ok()
                .unwrap()
                .iter()
                .enumerate()
            {
                for k in 0..4 {
                    let delta = u32::from(actual[k]).abs_diff(expected[i][k]);
                    max_typed = max_typed.max(delta);
                    assert!(
                        delta <= 1,
                        "u16 {columns}x{rows} {filter} pixel {i} channel {k}: {} vs {}",
                        actual[k],
                        expected[i][k]
                    );
                }
                if expected[i][3] == 0 {
                    assert_eq!(*actual, [0; 4]);
                }
                if i < input.len()
                    && actual
                        .iter()
                        .zip(input[i])
                        .any(|(&a, b)| u32::from(a).abs_diff(b) >= 2048)
                {
                    changed += 1;
                }
            }
            assert!(changed > 0, "typed non-vacuity");
            eprintln!(
                "warp numeric {columns}x{rows} {filter}: max byte={max_error}, max u16={max_typed}"
            );
        }
    }
}

#[test]
fn independent_masks_follow_only_when_linked_and_mask_only_preserves_pixels() {
    let mut max_byte_mask_error = 0u32;
    let mut max_u16_mask_error = 0u32;
    let mask_size = (11, 19);
    let mask_offset = (1, -4);
    let byte_values: Vec<u8> = (0..mask_size.0 * mask_size.1)
        .map(|i| ((i * 67 + 41) % 256) as u8)
        .collect();
    let typed_values: Vec<u16> = (0..mask_size.0 * mask_size.1)
        .map(|i| ((i * 3901 + 179) % 65536) as u16)
        .collect();
    for (columns, rows) in [(3, 3), (4, 2)] {
        for filter in ["bilinear", "bicubic"] {
            for outside in [0, 255] {
                for linked in [false, true] {
                    for enabled in [false, true] {
                        let mut request = field(columns, rows, filter);
                        let mut byte = byte_document();
                        byte.layers[0].mask = Some(Arc::new(
                            GrayImage::from_raw(mask_size.0, mask_size.1, byte_values.clone())
                                .unwrap(),
                        ));
                        let mut props = MaskProperties::at(mask_offset);
                        props.linked = linked;
                        props.outside_coverage = outside;
                        props.density = 0.63;
                        props.feather = 1.25;
                        byte.layers[0].mask_properties = props.clone();
                        byte.layers[0].mask_enabled = enabled;
                        let byte_mask_ptr = Arc::as_ptr(byte.layers[0].mask.as_ref().unwrap());
                        let pixel = byte
                            .warp_edit(&edit(request.clone()), LIMIT, 0, None)
                            .unwrap()
                            .unwrap();
                        assert_eq!(pixel.layers[0].mask_enabled, enabled);
                        assert_eq!(pixel.layers[0].mask_properties.density, 0.63);
                        assert_eq!(pixel.layers[0].mask_properties.feather, 1.25);
                        if linked {
                            let input: Vec<u32> =
                                byte_values.iter().map(|&v| u32::from(v)).collect();
                            let (bounds, expected) = scalar_reference::<255>(
                                &request,
                                &input,
                                mask_size,
                                mask_offset,
                                u32::from(outside),
                            );
                            assert_eq!(
                                (
                                    pixel.layers[0].mask_properties.offset.0,
                                    pixel.layers[0].mask_properties.offset.1,
                                    pixel.layers[0].mask.as_ref().unwrap().width(),
                                    pixel.layers[0].mask.as_ref().unwrap().height()
                                ),
                                bounds
                            );
                            for (a, b) in pixel.layers[0]
                                .mask
                                .as_ref()
                                .unwrap()
                                .as_raw()
                                .iter()
                                .zip(expected)
                            {
                                let error = u32::from(*a).abs_diff(b);
                                max_byte_mask_error = max_byte_mask_error.max(error);
                                assert!(error <= 1);
                            }
                        } else {
                            assert_eq!(
                                Arc::as_ptr(pixel.layers[0].mask.as_ref().unwrap()),
                                byte_mask_ptr
                            );
                            assert_eq!(pixel.layers[0].mask_properties.offset, mask_offset);
                        }
                        request["target"] = Value::String("mask".into());
                        let mask_only = byte
                            .warp_edit(&edit(request.clone()), LIMIT, 0, None)
                            .unwrap()
                            .unwrap();
                        assert!(LayerRaster::ptr_eq(
                            &mask_only.layers[0].pixels,
                            &byte.layers[0].pixels
                        ));
                        assert_eq!(mask_only.layers[0].offset, byte.layers[0].offset);

                        let base = typed_document();
                        let profile = base.profile().clone();
                        let image = base.layer_source(0).unwrap().clone();
                        let mask = PrecisionMask::new(
                            PrecisionPlane::from_u16(
                                mask_size.0,
                                mask_size.1,
                                typed_values.clone(),
                                None,
                            )
                            .unwrap(),
                            props,
                            enabled,
                        )
                        .unwrap();
                        let layer =
                            PrecisionDocumentLayer::raster("masked", image, OFFSET, 1.0, true)
                                .unwrap()
                                .with_mask(Some(mask));
                        let typed = PrecisionDocument::from_parts(
                            fixture::W,
                            fixture::H,
                            profile,
                            doctored_core::metadata::Metadata::default(),
                            doctored_core::metadata::Resolution::default(),
                            vec![layer],
                        )
                        .unwrap();
                        let source_ptr = typed.layer_source(0).unwrap().clone();
                        let mask_ptr = typed
                            .layer(0)
                            .unwrap()
                            .mask()
                            .unwrap()
                            .plane()
                            .samples()
                            .as_ptr();
                        request["target"] = Value::String("pixels".into());
                        let pixel = typed
                            .warp_edit(&edit(request.clone()), LIMIT, 0, None)
                            .unwrap()
                            .unwrap();
                        if linked {
                            let input: Vec<u32> =
                                typed_values.iter().map(|&v| u32::from(v)).collect();
                            let (bounds, expected) = scalar_reference::<65535>(
                                &request,
                                &input,
                                mask_size,
                                mask_offset,
                                u32::from(outside) * 257,
                            );
                            let output = pixel.layer(0).unwrap().mask().unwrap();
                            assert_eq!(
                                (
                                    output.properties().offset.0,
                                    output.properties().offset.1,
                                    output.plane().dimensions().0,
                                    output.plane().dimensions().1
                                ),
                                bounds
                            );
                            for (a, b) in output.plane().samples().iter().zip(expected) {
                                let error = u32::from(*a).abs_diff(b);
                                max_u16_mask_error = max_u16_mask_error.max(error);
                                assert!(error <= 1);
                            }
                        } else {
                            assert_eq!(
                                pixel
                                    .layer(0)
                                    .unwrap()
                                    .mask()
                                    .unwrap()
                                    .plane()
                                    .samples()
                                    .as_ptr(),
                                mask_ptr
                            );
                        }
                        request["target"] = Value::String("mask".into());
                        let only = typed
                            .warp_edit(&edit(request), LIMIT, 0, None)
                            .unwrap()
                            .unwrap();
                        assert!(PrecisionImage::ptr_eq(
                            only.layer_source(0).unwrap(),
                            &source_ptr
                        ));
                    }
                }
            }
        }
    }
    eprintln!("warp mask numeric: max byte={max_byte_mask_error}, max u16={max_u16_mask_error}");
}
