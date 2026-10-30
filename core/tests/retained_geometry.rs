//! Whole canvas retained-source geometry keeps the original master and exact
//! allocations on window moves while filtered changes re-evaluate that master.

use doctored_core::doc::{DrDocument, LayerRaster};
use doctored_core::doc_comps::CompRequest;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_geometry::GeometryEdit;
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use doctored_core::precision_sampling::GeometryFilter;
use doctored_core::retained_source::{SourceEdit, SourceInput};
use doctored_core::source_transform::SourceTransform;
use image::{Rgba, RgbaImage};
use serde_json::json;
use std::sync::Arc;

const LIMIT: u64 = REFERENCE_WORKING_BYTES;

fn edit(value: serde_json::Value) -> GeometryEdit {
    GeometryEdit::parse(&value.to_string()).unwrap()
}

fn place() -> SourceEdit {
    SourceEdit::parse(
        &json!({"op":"place","source_id":"whole-master",
        "revision_id":"first","name":"Source","x":1,"y":1})
        .to_string(),
    )
    .unwrap()
}

fn byte() -> DrDocument {
    let base = DrDocument::from_pixels(RgbaImage::from_pixel(8, 8, Rgba([2, 4, 6, 255])));
    let original = DrDocument::from_pixels(RgbaImage::from_fn(4, 4, |x, y| {
        Rgba([
            (x * 31 + y * 7) as u8,
            (x * 17 + y * 19) as u8,
            221,
            if x == 0 { 0 } else { 255 },
        ])
    }));
    base.source_edit(Some(&original), &place(), LIMIT, 0, None)
        .unwrap()
        .unwrap()
}

fn typed() -> PrecisionDocument {
    let profile = IccProfile::srgb();
    let base = PrecisionDocument::from_image(
        PrecisionImage::from_rgba16(8, 8, vec![[2, 4, 6, 65535]; 64], profile.clone(), None)
            .unwrap(),
        Metadata::default(),
        Resolution::default(),
    )
    .unwrap();
    let original = PrecisionDocument::from_image(
        PrecisionImage::from_rgba16(
            4,
            4,
            (0..16)
                .map(|v| {
                    [
                        v * 101 + 1,
                        v * 67 + 3,
                        65535 - v * 71,
                        if v % 4 == 0 { 0 } else { 32769 },
                    ]
                })
                .collect(),
            profile,
            None,
        )
        .unwrap(),
        Metadata::default(),
        Resolution::default(),
    )
    .unwrap();
    base.source_edit(Some(&original), &place(), LIMIT, 0, None)
        .unwrap()
        .unwrap()
}

#[test]
fn crop_keeps_exact_fallback_allocation_and_master_at_both_depths() {
    let request = edit(json!({"op":"crop","x":1,"y":1,"width":6,"height":6}));
    let byte = byte();
    let output = byte
        .geometry_edit(&request, LIMIT, 0, None)
        .unwrap()
        .unwrap();
    assert!(LayerRaster::ptr_eq(
        &byte.layers[1].pixels,
        &output.layers[1].pixels
    ));
    assert!(Arc::ptr_eq(
        &byte.layers[1].retained_source.as_ref().unwrap().asset,
        &output.layers[1]
            .effective_source(&output.profile)
            .unwrap()
            .asset
    ));
    assert_eq!(output.layers[1].offset, (0, 0));
    let typed = typed();
    let old = typed.layer(1).unwrap().effective_source().unwrap();
    let old_pixels = typed.layer_source(1).unwrap().clone();
    let output = typed.geometry(&request, LIMIT, 0, None).unwrap().unwrap();
    assert!(PrecisionImage::ptr_eq(
        output.layer_source(1).unwrap(),
        &old_pixels
    ));
    assert!(Arc::ptr_eq(
        &old.asset,
        &output.layer(1).unwrap().effective_source().unwrap().asset
    ));
    assert_eq!(output.layer(1).unwrap().offset(), (0, 0));
}

#[test]
fn identity_straighten_over_a_window_is_the_crop_and_keeps_the_binding() {
    // This once answered "byte straighten could not transform all
    // layers": the retained layer's identity source edit is `None`.
    let byte = byte();
    let straightened = byte
        .geometry_edit(
            &edit(
                json!({"op":"straighten","matrix":[1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
                "filter":"bicubic","x":1,"y":1,"width":6,"height":6}),
            ),
            LIMIT,
            0,
            None,
        )
        .unwrap()
        .expect("an identity over an inner window crops");
    let cropped = byte
        .geometry_edit(
            &edit(json!({"op":"crop","x":1,"y":1,"width":6,"height":6})),
            LIMIT,
            0,
            None,
        )
        .unwrap()
        .unwrap();
    assert_eq!(
        (straightened.width, straightened.height),
        (cropped.width, cropped.height)
    );
    assert_eq!(straightened.layers.len(), cropped.layers.len());
    for (a, b) in straightened.layers.iter().zip(&cropped.layers) {
        assert!(LayerRaster::ptr_eq(&a.pixels, &b.pixels));
        assert_eq!(a.offset, b.offset);
    }
    assert!(Arc::ptr_eq(
        &byte.layers[1].retained_source.as_ref().unwrap().asset,
        &straightened.layers[1]
            .retained_source
            .as_ref()
            .expect("layer 1 keeps its retained source")
            .asset
    ));
}

#[test]
fn filtered_down_up_uses_original_master_at_both_depths() {
    let down = edit(json!({"op":"resize","width":4,"height":4,"filter":"lanczos3"}));
    let up = edit(json!({"op":"resize","width":8,"height":8,"filter":"lanczos3"}));
    let byte = byte();
    let original = byte.layers[1].pixels.clone();
    let smaller = byte.geometry_edit(&down, LIMIT, 0, None).unwrap().unwrap();
    let restored = smaller.geometry_edit(&up, LIMIT, 0, None).unwrap().unwrap();
    assert_eq!(
        restored.layers[1].pixels.contiguous_image().as_raw(),
        original.contiguous_image().as_raw()
    );
    assert!(Arc::ptr_eq(
        &byte.layers[1].retained_source.as_ref().unwrap().asset,
        &restored.layers[1]
            .effective_source(&restored.profile)
            .unwrap()
            .asset
    ));

    let typed = typed();
    let original = typed.layer_source(1).unwrap().to_rgba16().unwrap();
    let old_master = typed.layer(1).unwrap().effective_source().unwrap().asset;
    let smaller = typed.geometry(&down, LIMIT, 0, None).unwrap().unwrap();
    let restored = smaller.geometry(&up, LIMIT, 0, None).unwrap().unwrap();
    assert_eq!(
        restored.layer_source(1).unwrap().to_rgba16().ok().unwrap(),
        original.as_slice()
    );
    assert!(Arc::ptr_eq(
        &old_master,
        &restored.layer(1).unwrap().effective_source().unwrap().asset
    ));
}

fn master8() -> DrDocument {
    DrDocument::from_pixels(RgbaImage::from_fn(4, 4, |x, y| {
        Rgba([
            (x * 31 + y * 7 + 1) as u8,
            (x * 17 + y * 19 + 3) as u8,
            221,
            if x == 0 { 0 } else { 255 },
        ])
    }))
}

fn master16() -> PrecisionDocument {
    PrecisionDocument::from_image(
        PrecisionImage::from_rgba16(
            4,
            4,
            (0..16)
                .map(|v| {
                    [
                        v * 101 + 1,
                        v * 67 + 3,
                        65535 - v * 71,
                        if v % 4 == 0 { 0 } else { 32769 },
                    ]
                })
                .collect(),
            IccProfile::srgb(),
            None,
        )
        .unwrap(),
        Metadata::default(),
        Resolution::default(),
    )
    .unwrap()
}

fn whole_edits() -> Vec<(&'static str, GeometryEdit)> {
    vec![
        (
            "resize",
            edit(json!({"op":"resize","width":11,"height":9,"filter":"bilinear"})),
        ),
        (
            "crop",
            edit(json!({"op":"crop","x":1,"y":1,"width":6,"height":6})),
        ),
        (
            "canvas_size",
            edit(json!({"op":"canvas_size","width":10,"height":9,"x":1,"y":2})),
        ),
        (
            "straighten",
            edit(json!({"op":"straighten","matrix":[1,0.05,-0.05,1,0,0],
            "filter":"bilinear","x":1,"y":1,"width":6,"height":6})),
        ),
        (
            "rotate_canvas",
            edit(json!({"op":"rotate_canvas","degrees":7.5,"filter":"bilinear"})),
        ),
        ("rotate90", GeometryEdit::Rotate90),
        ("rotate180", GeometryEdit::Rotate180),
        ("rotate270", GeometryEdit::Rotate270),
        ("flip_horizontal", GeometryEdit::FlipHorizontal),
        ("flip_vertical", GeometryEdit::FlipVertical),
    ]
}

fn linked_place() -> SourceEdit {
    SourceEdit::parse(
        &json!({"op":"place","source_id":"matrix-master",
        "revision_id":"first","name":"Linked source","x":1,"y":1,
        "depth_conversion":"convert_fallback",
        "link":{"path":"/fixture/source.png","sha256":"a".repeat(64)}})
        .to_string(),
    )
    .unwrap()
}

fn replacement() -> SourceEdit {
    SourceEdit::parse(
        &json!({"op":"replace","layer":1,"revision_id":"fresh",
        "depth_conversion":"convert_fallback"})
        .to_string(),
    )
    .unwrap()
}

#[test]
fn grouped_and_linked_set_geometry_retain_original_masters_at_both_fallback_depths() {
    let source8 = master8();
    let source16 = master16();
    for master_depth in [8, 16] {
        let input = if master_depth == 8 {
            SourceInput::Byte(&source8)
        } else {
            SourceInput::Typed(&source16)
        };
        for fallback_depth in [8, 16] {
            for (map, operation) in [
                (
                    "affine",
                    json!({"op":"affine_set","matrix":[1.1,0.03,-0.02,1.08,0.4,0.2],
                    "filter":"bilinear"}),
                ),
                (
                    "projective",
                    json!({"op":"perspective_set","source_rect":[0,0,8,8],
                    "quad":[0,0,8,0,7,8,0,8],"filter":"bilinear"}),
                ),
            ] {
                for (root_name, root) in [("group", 2), ("linked", 3)] {
                    let mut request = operation.clone();
                    request["layers"] = json!([root]);
                    let geometry = edit(request);
                    if fallback_depth == 8 {
                        let base = DrDocument::from_pixels(RgbaImage::from_pixel(
                            8,
                            8,
                            Rgba([3, 5, 7, 255]),
                        ));
                        let placed = base
                            .source_edit_input(Some(input), &linked_place(), LIMIT, 0, None)
                            .unwrap()
                            .unwrap();
                        let duplicated = placed.duplicate_layers(&[1]).unwrap();
                        let (grouped, group, _, _) =
                            duplicated.group_layers(&[1], "Retained G").unwrap();
                        assert_eq!(group, 2);
                        let grouped = grouped.link_layers(&[1, 3]).unwrap();
                        let old = grouped.layers[1]
                            .effective_source(&grouped.profile)
                            .unwrap();
                        let old_info = grouped.source_info(1).unwrap();
                        let master = old.asset.clone();
                        let link = old.link.as_ref().unwrap().clone();
                        let output = grouped
                            .geometry_edit(&geometry, LIMIT, 0, None)
                            .unwrap()
                            .unwrap();
                        let carried = output.layers[1].effective_source(&output.profile).unwrap();
                        assert!(
                            Arc::ptr_eq(&master, &carried.asset),
                            "{master_depth} to u8 {map} {root_name}"
                        );
                        assert!(Arc::ptr_eq(&link, carried.link.as_ref().unwrap()));
                        let info = output.source_info(1).unwrap();
                        assert_ne!(info["transform"], old_info["transform"]);
                        assert_ne!(
                            output.source_info(3).unwrap()["transform"],
                            grouped.source_info(3).unwrap()["transform"]
                        );
                        for key in [
                            "source_id",
                            "revision_id",
                            "link",
                            "depth",
                            "master_depth",
                            "fallback_depth",
                            "profile",
                            "profile_bytes",
                            "source_attachment",
                        ] {
                            assert_eq!(
                                info[key], old_info[key],
                                "byte metadata {key} {map} {root_name}"
                            );
                        }
                        assert_eq!(info["transform_kind"], map);
                        let fresh = output
                            .source_edit_input(Some(input), &replacement(), LIMIT, 0, None)
                            .unwrap()
                            .unwrap();
                        assert_eq!(
                            output.layers[1].pixels.contiguous_image().as_raw(),
                            fresh.layers[1].pixels.contiguous_image().as_raw(),
                            "byte fresh rerender {map} {root_name}"
                        );
                        assert_eq!(output.layers[1].offset, fresh.layers[1].offset);
                    } else {
                        let base = PrecisionDocument::from_image(
                            PrecisionImage::from_rgba16(
                                8,
                                8,
                                vec![[3, 5, 7, 65535]; 64],
                                IccProfile::srgb(),
                                None,
                            )
                            .unwrap(),
                            Metadata::default(),
                            Resolution::default(),
                        )
                        .unwrap();
                        let placed = base
                            .source_edit_input(Some(input), &linked_place(), LIMIT, 0, None)
                            .unwrap()
                            .unwrap();
                        let duplicated = placed.duplicate_layer(1).unwrap().unwrap();
                        let (grouped, report) = duplicated
                            .group_layers(&[1], "Retained G", false)
                            .unwrap()
                            .unwrap();
                        assert_eq!(report.group, 2);
                        let grouped = grouped.link_layers(&[1, 3]).unwrap().unwrap();
                        let old = grouped.layer(1).unwrap().effective_source().unwrap();
                        let old_info = grouped.source_info(1).unwrap();
                        let master = old.asset.clone();
                        let link = old.link.as_ref().unwrap().clone();
                        let output = grouped
                            .geometry(&geometry, LIMIT, 0, None)
                            .unwrap()
                            .unwrap();
                        let carried = output.layer(1).unwrap().effective_source().unwrap();
                        assert!(
                            Arc::ptr_eq(&master, &carried.asset),
                            "{master_depth} to u16 {map} {root_name}"
                        );
                        assert!(Arc::ptr_eq(&link, carried.link.as_ref().unwrap()));
                        let info = output.source_info(1).unwrap();
                        assert_ne!(info["transform"], old_info["transform"]);
                        assert_ne!(
                            output.source_info(3).unwrap()["transform"],
                            grouped.source_info(3).unwrap()["transform"]
                        );
                        for key in [
                            "source_id",
                            "revision_id",
                            "link",
                            "depth",
                            "master_depth",
                            "fallback_depth",
                            "profile",
                            "profile_bytes",
                            "source_attachment",
                        ] {
                            assert_eq!(
                                info[key], old_info[key],
                                "typed metadata {key} {map} {root_name}"
                            );
                        }
                        assert_eq!(info["transform_kind"], map);
                        let fresh = output
                            .source_edit_input(Some(input), &replacement(), LIMIT, 0, None)
                            .unwrap()
                            .unwrap();
                        assert_eq!(
                            output.layer_source(1).unwrap().to_rgba16().ok().unwrap(),
                            fresh.layer_source(1).unwrap().to_rgba16().ok().unwrap(),
                            "typed fresh rerender {map} {root_name}"
                        );
                        assert_eq!(
                            output.layer(1).unwrap().offset(),
                            fresh.layer(1).unwrap().offset()
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn all_whole_geometry_preserves_linked_master_across_source_and_fallback_depths() {
    let source8 = master8();
    let source16 = master16();
    for master_depth in [8, 16] {
        for fallback_depth in [8, 16] {
            if fallback_depth == 8 {
                let base =
                    DrDocument::from_pixels(RgbaImage::from_pixel(8, 8, Rgba([3, 5, 7, 255])));
                let input = if master_depth == 8 {
                    SourceInput::Byte(&source8)
                } else {
                    SourceInput::Typed(&source16)
                };
                let placed = base
                    .source_edit_input(Some(input), &linked_place(), LIMIT, 0, None)
                    .unwrap()
                    .unwrap();
                let old = placed.layers[1].effective_source(&placed.profile).unwrap();
                assert_eq!(old.asset.depth(), master_depth);
                assert!(old.link.is_some());
                for (name, operation) in whole_edits() {
                    let result = placed
                        .geometry_edit(&operation, LIMIT, 0, None)
                        .unwrap()
                        .unwrap();
                    let carried = result.layers[1].effective_source(&result.profile).unwrap();
                    assert!(
                        Arc::ptr_eq(&old.asset, &carried.asset),
                        "{name}: master {master_depth}, fallback {fallback_depth}"
                    );
                    assert!(Arc::ptr_eq(
                        old.link.as_ref().unwrap(),
                        carried.link.as_ref().unwrap()
                    ));
                    assert_eq!(result.source_info(1).unwrap()["valid"], true, "{name}");
                    let fresh = result
                        .source_edit_input(Some(input), &replacement(), LIMIT, 0, None)
                        .unwrap()
                        .unwrap();
                    assert_eq!(result.layers[1].pixels.contiguous_image().as_raw(), fresh.layers[1].pixels.contiguous_image().as_raw(),
                        "{name}: fallback was not rendered from the original {master_depth}-bit master");
                    assert_eq!(result.layers[1].offset, fresh.layers[1].offset, "{name}");
                }
            } else {
                let base = PrecisionDocument::from_image(
                    PrecisionImage::from_rgba16(
                        8,
                        8,
                        vec![[3, 5, 7, 65535]; 64],
                        IccProfile::srgb(),
                        None,
                    )
                    .unwrap(),
                    Metadata::default(),
                    Resolution::default(),
                )
                .unwrap();
                let input = if master_depth == 8 {
                    SourceInput::Byte(&source8)
                } else {
                    SourceInput::Typed(&source16)
                };
                let placed = base
                    .source_edit_input(Some(input), &linked_place(), LIMIT, 0, None)
                    .unwrap()
                    .unwrap();
                let old = placed.layer(1).unwrap().effective_source().unwrap();
                assert_eq!(old.asset.depth(), master_depth);
                assert!(old.link.is_some());
                for (name, operation) in whole_edits() {
                    let result = placed
                        .geometry(&operation, LIMIT, 0, None)
                        .unwrap()
                        .unwrap();
                    let carried = result.layer(1).unwrap().effective_source().unwrap();
                    assert!(
                        Arc::ptr_eq(&old.asset, &carried.asset),
                        "{name}: master {master_depth}, fallback {fallback_depth}"
                    );
                    assert!(Arc::ptr_eq(
                        old.link.as_ref().unwrap(),
                        carried.link.as_ref().unwrap()
                    ));
                    assert_eq!(result.source_info(1).unwrap()["valid"], true, "{name}");
                    let fresh = result
                        .source_edit_input(Some(input), &replacement(), LIMIT, 0, None)
                        .unwrap()
                        .unwrap();
                    assert_eq!(result.layer_source(1).unwrap().to_rgba16().ok().unwrap(),
                        fresh.layer_source(1).unwrap().to_rgba16().ok().unwrap(),
                        "{name}: fallback was not rendered from the original {master_depth}-bit master");
                    assert_eq!(
                        result.layer(1).unwrap().offset(),
                        fresh.layer(1).unwrap().offset(),
                        "{name}"
                    );
                }
            }
        }
    }
}

#[test]
fn a_projective_source_survives_profile_conversion_and_master_replacement() {
    let source8 = master8();
    let source16 = master16();
    let perspective = edit(json!({"op":"perspective_set","layers":[1],
        "source_rect":[0,0,8,8],"quad":[0,0,7,0,6,7,0,8],"filter":"bilinear"}));
    let destination_profile = IccProfile::display_p3();
    for master_depth in [8, 16] {
        let input = if master_depth == 8 {
            SourceInput::Byte(&source8)
        } else {
            SourceInput::Typed(&source16)
        };
        let byte_base = DrDocument::from_pixels(RgbaImage::from_pixel(8, 8, Rgba([3, 5, 7, 255])));
        let byte = byte_base
            .source_edit_input(Some(input), &linked_place(), LIMIT, 0, None)
            .unwrap()
            .unwrap();
        let warped = byte
            .geometry_edit(&perspective, LIMIT, 0, None)
            .unwrap()
            .unwrap();
        let converted = warped
            .convert_to_profile(&destination_profile)
            .unwrap()
            .unwrap();
        let retained = converted.layers[1]
            .effective_source(&converted.profile)
            .unwrap();
        assert_eq!(
            retained.asset.profile().bytes(),
            if master_depth == 8 {
                source8.profile.bytes()
            } else {
                source16.profile().bytes()
            }
        );
        assert_eq!(
            converted.source_info(1).unwrap()["transform_kind"],
            "projective"
        );
        let fresh = converted
            .source_edit_input(Some(input), &replacement(), LIMIT, 0, None)
            .unwrap()
            .unwrap();
        assert_eq!(
            converted.layers[1].pixels.contiguous_image().as_raw(),
            fresh.layers[1].pixels.contiguous_image().as_raw()
        );

        let typed_base = PrecisionDocument::from_image(
            PrecisionImage::from_rgba16(8, 8, vec![[3, 5, 7, 65535]; 64], IccProfile::srgb(), None)
                .unwrap(),
            Metadata::default(),
            Resolution::default(),
        )
        .unwrap();
        let typed = typed_base
            .source_edit_input(Some(input), &linked_place(), LIMIT, 0, None)
            .unwrap()
            .unwrap();
        let warped = typed
            .geometry(&perspective, LIMIT, 0, None)
            .unwrap()
            .unwrap();
        let converted = warped
            .convert_to_profile(&destination_profile, LIMIT, 0, None)
            .unwrap()
            .unwrap();
        let retained = converted.layer(1).unwrap().effective_source().unwrap();
        assert_eq!(
            retained.asset.profile().bytes(),
            if master_depth == 8 {
                source8.profile.bytes()
            } else {
                source16.profile().bytes()
            }
        );
        assert_eq!(
            converted.source_info(1).unwrap()["transform_kind"],
            "projective"
        );
        let fresh = converted
            .source_edit_input(Some(input), &replacement(), LIMIT, 0, None)
            .unwrap()
            .unwrap();
        assert_eq!(
            converted.layer_source(1).unwrap().to_rgba16().ok().unwrap(),
            fresh.layer_source(1).unwrap().to_rgba16().ok().unwrap()
        );
    }
}

#[test]
fn byte_set_plan_charges_large_master_conversion_before_low_limit_edit() {
    let source = PrecisionDocument::from_image(
        PrecisionImage::from_rgba16(
            256,
            256,
            vec![[1101, 2203, 3307, 65535]; 256 * 256],
            IccProfile::srgb(),
            None,
        )
        .unwrap(),
        Metadata::default(),
        Resolution::default(),
    )
    .unwrap();
    let mut base = DrDocument::from_pixels(RgbaImage::from_pixel(4, 4, Rgba([3, 5, 7, 255])));
    base.profile = IccProfile::display_p3();
    let place = SourceEdit::parse(
        &json!({"op":"place","source_id":"large-master",
        "revision_id":"first","name":"Large","x":0,"y":0,
        "depth_conversion":"convert_fallback",
        "source_transform":[0.03125,0,0,0.03125,0,0]})
        .to_string(),
    )
    .unwrap();
    let placed = base
        .source_edit_input(Some(SourceInput::Typed(&source)), &place, LIMIT, 0, None)
        .unwrap()
        .unwrap();
    let master = placed.layers[1]
        .effective_source(&placed.profile)
        .unwrap()
        .asset;
    let operation = edit(json!({"op":"affine_set","layers":[1],
        "matrix":[1.25,0,0,1.25,0,0],"filter":"bilinear"}));
    let plan = placed.geometry_edit_plan(&operation).unwrap();
    let input_bytes = placed
        .layers
        .iter()
        .map(|layer| layer.pixels.contiguous_image().as_raw().len() as u64)
        .sum::<u64>();
    let old_input_estimate =
        input_bytes * 6 + u64::from(placed.width) * u64::from(placed.height) * 8 + 4 * 1024 * 1024;
    assert!(
        plan.scratch_bytes > old_input_estimate + 256 * 256 * 8,
        "retained high-depth master color work was not planned"
    );
    let low_limit = plan.retained_bytes + old_input_estimate + 1024;
    assert!(placed
        .geometry_edit(&operation, low_limit, 0, None)
        .is_err());
    assert!(Arc::ptr_eq(
        &master,
        &placed.layers[1]
            .effective_source(&placed.profile)
            .unwrap()
            .asset
    ));
}

/// An exact integer translation (typed Move, Align, Distribute) keeps
/// the binding's own filter, as the byte move (`translate_entry`) does, so a
/// later re-render resamples the master with the filter it was bound with.
#[test]
fn integer_move_keeps_a_bicubic_binding_at_both_depths() {
    let bicubic = SourceEdit::parse(
        &json!({"op":"transform","layer":1,"matrix":[1.5,0,0,1.5,1,1],"filter":"bicubic"})
            .to_string(),
    )
    .unwrap();

    let byte = byte()
        .source_edit(None, &bicubic, LIMIT, 0, None)
        .unwrap()
        .unwrap();
    let before = byte.layers[1].retained_source.as_ref().unwrap();
    assert!(matches!(before.filter, GeometryFilter::Bicubic));
    let moved = byte.move_layers(&[1], 2, 1).unwrap();
    let after = moved.layers[1].retained_source.as_ref().unwrap();
    assert!(matches!(after.filter, GeometryFilter::Bicubic));

    let typed = typed()
        .source_edit(None, &bicubic, LIMIT, 0, None)
        .unwrap()
        .unwrap();
    let before = typed.layer(1).unwrap().effective_source().unwrap();
    assert!(matches!(before.filter, GeometryFilter::Bicubic));
    let SourceTransform::Affine([a, b, c, d, tx, ty]) = before.transform else {
        panic!("an affine binding");
    };
    let moved = typed
        .geometry(
            &edit(json!({"op":"affine_set","layers":[1],
                "matrix":[1,0,0,1,2,1],"filter":"nearest"})),
            LIMIT,
            0,
            None,
        )
        .unwrap()
        .unwrap();
    let after = moved.layer(1).unwrap().effective_source().unwrap();
    assert!(
        matches!(after.filter, GeometryFilter::Bicubic),
        "{:?}",
        after.filter
    );
    assert_eq!(
        after.transform,
        SourceTransform::Affine([a, b, c, d, tx + 2.0, ty + 1.0])
    );
    let (x, y) = typed.layer(1).unwrap().offset();
    assert_eq!(moved.layer(1).unwrap().offset(), (x + 2, y + 1));
    assert_eq!(
        moved.layer_source(1).unwrap().to_rgba16().unwrap(),
        typed.layer_source(1).unwrap().to_rgba16().unwrap()
    );
}

fn comp_request(request: serde_json::Value) -> CompRequest {
    CompRequest::parse(&request.to_string()).unwrap()
}

fn byte_applied(document: DrDocument, apply: &CompRequest) -> DrDocument {
    match document.edit_layer_comps(apply).unwrap() {
        Some(next) => next,
        None => document,
    }
}

fn typed_applied(document: PrecisionDocument, apply: &CompRequest) -> PrecisionDocument {
    match document.edit_layer_comps(apply).unwrap() {
        Some(next) => next,
        None => document,
    }
}

/// A comp entry on a layer with a RETAINED SOURCE follows every
/// whole-canvas edit EXACTLY at both depths. The comp is saved with the
/// sourced layer at (2, 2) and the layer is then moved back to (1, 1), so
/// applying the comp after the edit must land the layer exactly where the
/// edit puts it when the comp is applied first. The resize row is the one
/// the plain rule gets wrong: 8x8 -> 11x9 takes (2, 2) to round(2 * 11/8,
/// 2 * 9/8) = (3, 2), while the re-rendered source's bounds start at
/// floor(2.75, 2.25) = (2, 2).
#[test]
fn a_sourced_comp_entry_follows_every_whole_edit_exactly_at_both_depths() {
    let save = comp_request(json!({"op":"layer_comp","action":"save","name":"At 2,2"}));
    let apply = comp_request(json!({"op":"layer_comp","action":"apply","comp":0}));

    let saved = byte()
        .with_layer_offset(1, 2, 2)
        .unwrap()
        .edit_layer_comps(&save)
        .unwrap()
        .unwrap();
    let moved = saved.with_layer_offset(1, 1, 1).unwrap();
    assert!(moved.layers[1].effective_source(&moved.profile).is_some());
    for (name, operation) in whole_edits() {
        let done = moved
            .geometry_edit(&operation, LIMIT, 0, None)
            .unwrap()
            .unwrap();
        let entry = done.layer_comps()[0].entries[1].offset;
        let op_then_apply = byte_applied(done, &apply);
        let apply_then_op = byte_applied(moved.clone(), &apply)
            .geometry_edit(&operation, LIMIT, 0, None)
            .unwrap()
            .unwrap();
        assert!(
            apply_then_op.layers[1]
                .effective_source(&apply_then_op.profile)
                .is_some(),
            "byte {name}: the layer is still sourced"
        );
        assert_eq!(entry, Some(apply_then_op.layers[1].offset), "byte {name}");
        for index in 0..apply_then_op.layers.len() {
            assert_eq!(
                op_then_apply.layers[index].offset, apply_then_op.layers[index].offset,
                "byte {name}: layer {index}"
            );
        }
        if name == "resize" {
            assert_eq!(entry, Some((2, 2)), "the source bounds, not (3, 2)");
        }
    }

    let translate = |dx: i32, dy: i32| {
        edit(json!({"op":"affine_set","layers":[1],
            "matrix":[1,0,0,1,dx,dy],"filter":"nearest"}))
    };
    let saved = typed()
        .geometry(&translate(1, 1), LIMIT, 0, None)
        .unwrap()
        .unwrap()
        .edit_layer_comps(&save)
        .unwrap()
        .unwrap();
    assert_eq!(saved.layer(1).unwrap().offset(), (2, 2));
    let moved = saved
        .geometry(&translate(-1, -1), LIMIT, 0, None)
        .unwrap()
        .unwrap();
    assert_eq!(moved.layer(1).unwrap().offset(), (1, 1));
    assert!(moved.layer(1).unwrap().effective_source().is_some());
    for (name, operation) in whole_edits() {
        let done = moved.geometry(&operation, LIMIT, 0, None).unwrap().unwrap();
        let entry = done.layer_comps()[0].entries[1].offset;
        let op_then_apply = typed_applied(done, &apply);
        let apply_then_op = typed_applied(moved.clone(), &apply)
            .geometry(&operation, LIMIT, 0, None)
            .unwrap()
            .unwrap();
        assert!(
            apply_then_op.layer(1).unwrap().effective_source().is_some(),
            "typed {name}: the layer is still sourced"
        );
        assert_eq!(
            entry,
            Some(apply_then_op.layer(1).unwrap().offset()),
            "typed {name}"
        );
        for index in 0..apply_then_op.layer_count() {
            assert_eq!(
                op_then_apply.layer(index).unwrap().offset(),
                apply_then_op.layer(index).unwrap().offset(),
                "typed {name}: layer {index}"
            );
        }
        if name == "resize" {
            assert_eq!(entry, Some((2, 2)), "the source bounds, not (3, 2)");
        }
    }
}

/// A sourced layer's re-render inside a filtered whole-canvas turn
/// reserves `max(own plan, floor)` against the control's limit, and the turn
/// plan contains the re-render's own plan, so a control limited to the plan
/// completes and equals the uncontrolled edit.
#[test]
fn a_sourced_canvas_turn_completes_under_its_own_plan_limit() {
    use doctored_core::ffi_jobs::*;
    let base = DrDocument::from_pixels(RgbaImage::from_pixel(600, 400, Rgba([2, 4, 6, 255])));
    let original = DrDocument::from_pixels(RgbaImage::from_fn(600, 400, |x, y| {
        Rgba([(x % 251) as u8, (y % 241) as u8, ((x + y) % 239) as u8, 255])
    }));
    let large = base
        .source_edit(Some(&original), &place(), LIMIT, 0, None)
        .unwrap()
        .unwrap();
    for doc in [byte(), large] {
        for (name, operation) in whole_edits() {
            if name != "straighten" && name != "rotate_canvas" {
                continue;
            }
            let plan = doc.geometry_edit_plan(&operation).unwrap();
            let expected = doc
                .geometry_edit(&operation, LIMIT, 0, None)
                .unwrap()
                .unwrap();
            let control = unsafe { dr_job_control_create() };
            assert!(unsafe { dr_job_control_set_scratch_limit(control, plan.scratch_bytes) });
            let done = doc
                .geometry_edit(&operation, LIMIT, 0, Some(unsafe { &*control }))
                .unwrap()
                .unwrap_or_else(|| panic!("{name}: the controlled turn completes"));
            assert_eq!(
                unsafe { dr_job_control_scratch_bytes(control) },
                0,
                "{name}"
            );
            unsafe { dr_job_control_free(control) };
            // A 1-byte limit refuses: the turn reserves through the control.
            let control = unsafe { dr_job_control_create() };
            assert!(unsafe { dr_job_control_set_scratch_limit(control, 1) });
            let refused = doc.geometry_edit(&operation, LIMIT, 0, Some(unsafe { &*control }));
            assert!(
                !matches!(refused, Ok(Some(_))),
                "{name}: a 1-byte control limit refuses the turn"
            );
            unsafe { dr_job_control_free(control) };
            assert_eq!(done.layers.len(), expected.layers.len(), "{name}");
            for (a, b) in done.layers.iter().zip(&expected.layers) {
                assert!(a.pixels.same_bytes(&b.pixels), "{name}");
                assert_eq!(a.offset, b.offset, "{name}");
            }
        }
    }
}
