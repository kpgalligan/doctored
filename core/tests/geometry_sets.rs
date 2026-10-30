//! Atomic set geometry over byte and typed postorder records. The projective
//! fixture uses the independent map (x,y)->(x,y)/(1+x/10).

use std::ffi::{c_char, CStr, CString};
use std::ptr;
use std::sync::Arc;

use doctored_core::doc::BlendMode;
use doctored_core::doc::DrDocument;
use doctored_core::doc::LayerRaster;
use doctored_core::doc::MaskKind;
use doctored_core::doc_lock::LOCK_POSITION;
use doctored_core::doc_mask::MaskProperties;
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_geometry::{dr_doc_geometry_edit, dr_doc_geometry_edit_plan};
use doctored_core::icc::IccProfile;
use doctored_core::job_control::DrJobControl;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer};
use doctored_core::precision_geometry::GeometryEdit;
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use doctored_core::precision_mask::{PrecisionMask, PrecisionPlane};
use doctored_core::style::LayerStyle;
use image::{GrayImage, Luma, Rgba, RgbaImage};
use serde_json::json;

use crate::common;
use common::probe::{cancelled_parked_at, entry_count, parked_line};

const LIMIT: u64 = REFERENCE_WORKING_BYTES;

fn styled() -> LayerStyle {
    LayerStyle::from_json(
        r##"{"effects":[{"type":"drop_shadow","use_global_light":false,
        "angle":90,"distance":2,"size":2,"spread":0,"opacity":0.5,
        "color":"#000000"}]}"##,
    )
    .unwrap()
}

fn byte_mask(
    size: (u32, u32),
    offset: (i32, i32),
    linked: bool,
    enabled: bool,
    outside: u8,
) -> (Arc<GrayImage>, MaskProperties, bool) {
    let plane = Arc::new(GrayImage::from_fn(size.0, size.1, |x, y| {
        Luma([((x * 31 + y * 17 + 7) % 256) as u8])
    }));
    let mut properties = MaskProperties::at(offset);
    properties.linked = linked;
    properties.outside_coverage = outside;
    properties.density = 0.75;
    (plane, properties, enabled)
}

fn typed_mask(
    size: (u32, u32),
    offset: (i32, i32),
    linked: bool,
    enabled: bool,
    outside: u8,
) -> PrecisionMask {
    let values = (0..size.0 * size.1).map(|i| i as u16 * 1001 + 3).collect();
    let mut properties = MaskProperties::at(offset);
    properties.linked = linked;
    properties.outside_coverage = outside;
    properties.density = 0.75;
    PrecisionMask::new(
        PrecisionPlane::from_u16(size.0, size.1, values, None).unwrap(),
        properties,
        enabled,
    )
    .unwrap()
}

/// Postorder: A(0); B,C,Inner,G(1..4); D,H(5..6); Fx(7).
/// C links to H; D inside H links to Fx. Selecting G reaches the latter only
/// on the second fixed-point pass. A stays untouched.
fn byte_topology() -> DrDocument {
    let mut doc = DrDocument::from_pixels(RgbaImage::from_fn(8, 6, |x, y| {
        Rgba([x as u8 * 23, y as u8 * 31, 90, 255])
    }));
    doc.layers[0].name = "A".into();
    for (name, color) in [("B", 17), ("C", 73), ("D", 149), ("Fx", 0)] {
        doc = doc
            .adding_image_layer(
                doc.layers.len() - 1,
                RgbaImage::from_fn(3, 2, |x, y| {
                    Rgba([
                        color + x as u8 * 11,
                        31 + y as u8 * 19,
                        217,
                        if x == 0 && y == 0 { 0 } else { 173 },
                    ])
                }),
                name,
            )
            .unwrap();
    }
    let (next, inner, _, _) = doc.group_layers(&[1, 2], "Inner").unwrap();
    assert_eq!(inner, 3);
    let (next, group, _, _) = next.group_layers(&[inner], "G").unwrap();
    assert_eq!(group, 4);
    let (mut doc, through, _, _) = next.group_layers(&[5], "H").unwrap();
    assert_eq!(through, 6);
    doc = doc.with_layer_blend_mode(3, BlendMode::Normal).unwrap();
    doc = doc.with_layer_blend_mode(4, BlendMode::Normal).unwrap();
    for (index, offset) in [(1, (1, 1)), (2, (3, 0)), (5, (-1, 3))] {
        doc.layers[index].offset = offset;
    }
    doc.layers[7].meta = Some(
        json!({"type":"adjust","evaluation":"precision_v1",
        "op":"invert","params":{}})
        .to_string(),
    );
    doc.layers[7].offset = (0, 0);
    doc.layers[7].pixels = LayerRaster::new(RgbaImage::new(8, 6));
    for (index, size, offset, linked, enabled, outside) in [
        (0, (8, 6), (0, 0), true, true, 0),
        (1, (3, 2), (1, 1), true, true, 0),
        (2, (3, 2), (3, 0), false, true, 255),
        (3, (8, 6), (-1, 1), true, false, 0),
        (4, (8, 6), (0, 0), true, true, 0),
        (6, (8, 6), (0, 0), false, true, 0),
        (7, (8, 6), (0, 0), true, true, 0),
    ] {
        let (mask, properties, enabled) = byte_mask(size, offset, linked, enabled, outside);
        doc.layers[index].mask = Some(mask);
        doc.layers[index].mask_properties = properties;
        doc.layers[index].mask_enabled = enabled;
    }
    doc = doc.set_layer_style(1, Some(Arc::new(styled()))).unwrap();
    doc = doc.link_layers(&[2, 6]).unwrap();
    doc.link_layers(&[5, 7]).unwrap()
}

fn typed_topology() -> PrecisionDocument {
    let profile = IccProfile::srgb();
    let raster = |name: &str, dims: (u32, u32), offset: (i32, i32), base: u16| {
        let image = PrecisionImage::from_rgba16(
            dims.0,
            dims.1,
            (0..dims.0 * dims.1)
                .map(|i| {
                    [
                        base + i as u16 * 101,
                        5001 + i as u16 * 73,
                        63001 - i as u16 * 31,
                        if i == 0 { 0 } else { 32769 },
                    ]
                })
                .collect(),
            profile.clone(),
            None,
        )
        .unwrap();
        PrecisionDocumentLayer::raster(name, image, offset, 1., true).unwrap()
    };
    let a =
        raster("A", (8, 6), (0, 0), 101).with_mask(Some(typed_mask((8, 6), (0, 0), true, true, 0)));
    let b = raster("B", (3, 2), (1, 1), 1001)
        .with_mask(Some(typed_mask((3, 2), (1, 1), true, true, 0)))
        .with_layer_style(Some(&styled()), (8, 6))
        .unwrap();
    let c = raster("C", (3, 2), (3, 0), 3001).with_mask(Some(typed_mask(
        (3, 2),
        (3, 0),
        false,
        true,
        255,
    )));
    let inner = PrecisionDocumentLayer::isolated_normal("Inner", vec![b, c], 1., true)
        .unwrap()
        .with_mask(Some(typed_mask((8, 6), (-1, 1), true, false, 0)));
    let g = PrecisionDocumentLayer::isolated_normal("G", vec![inner], 1., true)
        .unwrap()
        .with_mask(Some(typed_mask((8, 6), (0, 0), true, true, 0)));
    let d = raster("D", (3, 2), (-1, 3), 5001);
    let h = PrecisionDocumentLayer::pass_through("H", vec![d], 1., true)
        .unwrap()
        .with_mask(Some(typed_mask((8, 6), (0, 0), false, true, 0)));
    let doc = PrecisionDocument::from_parts(
        8,
        6,
        profile,
        Metadata::default(),
        Resolution::default(),
        vec![a, g, h],
    )
    .unwrap();
    let description = json!({"type":"adjust","evaluation":"precision_v1",
        "op":"invert","params":{}})
    .to_string();
    let doc = doc
        .add_adjustment("Fx", &description, 1., LIMIT, 0, None)
        .unwrap()
        .unwrap();
    let doc = doc
        .set_layer_mask(
            7,
            Some(typed_mask((8, 6), (0, 0), true, true, 0)),
            LIMIT,
            0,
            None,
        )
        .unwrap()
        .unwrap();
    let doc = doc.link_layers(&[2, 6]).unwrap().unwrap();
    doc.link_layers(&[5, 7]).unwrap().unwrap()
}

#[test]
fn nested_topology_matches_each_independent_entry_at_both_depths() {
    let byte = byte_topology();
    let typed = typed_topology();
    for (name, edit) in [
        (
            "affine",
            GeometryEdit::parse(
                &json!({"op":"affine_set","layers":[4],
            "matrix":[1,0,0,1,2,-1],"filter":"nearest"})
                .to_string(),
            )
            .unwrap(),
        ),
        (
            "perspective",
            GeometryEdit::parse(
                &json!({"op":"perspective_set","layers":[4],
            "source_rect":[0,0,8,6],"quad":[0,0,8,0,7.5,6,0,6],
            "filter":"bilinear"})
                .to_string(),
            )
            .unwrap(),
        ),
    ] {
        let moved8 = byte.geometry_edit(&edit, LIMIT, 0, None).unwrap().unwrap();
        let moved16 = typed.geometry(&edit, LIMIT, 0, None).unwrap().unwrap();
        // A is outside the transitive closure; linked Fx is an adjustment
        // whose pixels never move, although its linked mask does.
        assert!(
            LayerRaster::ptr_eq(&byte.layers[0].pixels, &moved8.layers[0].pixels),
            "{name}"
        );
        assert_eq!(byte.layers[0].offset, moved8.layers[0].offset, "{name}");
        assert_eq!(
            typed.layer(0).unwrap().offset(),
            moved16.layer(0).unwrap().offset(),
            "{name}"
        );
        assert!(
            LayerRaster::ptr_eq(&byte.layers[7].pixels, &moved8.layers[7].pixels),
            "{name}"
        );
        assert_eq!(byte.layers[7].offset, moved8.layers[7].offset, "{name}");
        assert!(!moved8.layers[3].mask_enabled, "disabled group mask {name}");
        assert!(
            !moved16.layer(3).unwrap().mask().unwrap().enabled(),
            "disabled typed group mask {name}"
        );
        assert!(moved8.layers[1].style.is_some(), "byte style {name}");
        assert!(
            moved16.layer(1).unwrap().style().is_some(),
            "typed style {name}"
        );
        assert_eq!(
            byte.layers[2].mask_properties.offset, moved8.layers[2].mask_properties.offset,
            "unlinked mask {name}"
        );
        assert_eq!(
            byte.layers[6].mask_properties.offset, moved8.layers[6].mask_properties.offset,
            "unlinked group mask {name}"
        );
        assert_eq!(
            typed.layer(2).unwrap().mask().unwrap().properties().offset,
            moved16
                .layer(2)
                .unwrap()
                .mask()
                .unwrap()
                .properties()
                .offset,
            "{name}"
        );
        assert_eq!(
            typed.layer(6).unwrap().mask().unwrap().properties().offset,
            moved16
                .layer(6)
                .unwrap()
                .mask()
                .unwrap()
                .properties()
                .offset,
            "{name}"
        );
        for index in 1..8 {
            // A one-record set is an independent per-entry oracle. It uses
            // the same global map but cannot borrow a sibling's bounds or
            // mask allocation; group/link expansion does not alter this entry.
            let single = match &edit {
                GeometryEdit::AffineSet { matrix, filter, .. } => GeometryEdit::AffineSet {
                    layers: vec![index],
                    matrix: *matrix,
                    filter: *filter,
                    follow_links: true,
                    interactive: false,
                },
                GeometryEdit::PerspectiveSet {
                    source_rect,
                    quad,
                    filter,
                    ..
                } => GeometryEdit::PerspectiveSet {
                    layers: vec![index],
                    source_rect: *source_rect,
                    quad: *quad,
                    filter: *filter,
                    interactive: false,
                },
                _ => unreachable!(),
            };
            let expected8 = byte
                .geometry_edit(&single, LIMIT, 0, None)
                .unwrap()
                .unwrap();
            let expected16 = typed.geometry(&single, LIMIT, 0, None).unwrap().unwrap();
            let actual8 = &moved8.layers[index];
            let reference8 = &expected8.layers[index];
            assert_eq!(actual8.offset, reference8.offset, "byte {name} {index}");
            assert_eq!(
                actual8.pixels.contiguous_image().as_raw(),
                reference8.pixels.contiguous_image().as_raw(),
                "byte {name} {index}"
            );
            assert_eq!(
                actual8.mask_properties.offset, reference8.mask_properties.offset,
                "byte mask {name} {index}"
            );
            assert_eq!(
                actual8.mask.as_ref().map(|m| m.as_raw()),
                reference8.mask.as_ref().map(|m| m.as_raw()),
                "byte mask {name} {index}"
            );
            let actual16 = moved16.layer(index).unwrap();
            let reference16 = expected16.layer(index).unwrap();
            assert_eq!(
                actual16.offset(),
                reference16.offset(),
                "typed {name} {index}"
            );
            assert_eq!(
                moved16.layer_source(index).map(|p| p.to_rgba16().ok()),
                expected16.layer_source(index).map(|p| p.to_rgba16().ok()),
                "typed pixels {name} {index}"
            );
            assert_eq!(
                actual16.mask().map(|m| m.properties().offset),
                reference16.mask().map(|m| m.properties().offset),
                "typed mask offset {name} {index}"
            );
            assert_eq!(
                actual16.mask().map(|m| m.plane().samples()),
                reference16.mask().map(|m| m.plane().samples()),
                "typed mask samples {name} {index}"
            );
        }
        let overlapping = match &edit {
            GeometryEdit::AffineSet { matrix, filter, .. } => GeometryEdit::AffineSet {
                layers: vec![4, 2],
                matrix: *matrix,
                filter: *filter,
                follow_links: true,
                interactive: false,
            },
            GeometryEdit::PerspectiveSet {
                source_rect,
                quad,
                filter,
                ..
            } => GeometryEdit::PerspectiveSet {
                layers: vec![4, 2],
                source_rect: *source_rect,
                quad: *quad,
                filter: *filter,
                interactive: false,
            },
            _ => unreachable!(),
        };
        let overlap8 = byte
            .geometry_edit(&overlapping, LIMIT, 0, None)
            .unwrap()
            .unwrap();
        let overlap16 = typed
            .geometry(&overlapping, LIMIT, 0, None)
            .unwrap()
            .unwrap();
        for index in 0..8 {
            assert_eq!(
                moved8.layers[index].pixels.contiguous_image().as_raw(),
                overlap8.layers[index].pixels.contiguous_image().as_raw(),
                "overlap byte {name} {index}"
            );
            assert_eq!(moved8.layers[index].offset, overlap8.layers[index].offset);
            assert_eq!(
                moved16.layer(index).unwrap().offset(),
                overlap16.layer(index).unwrap().offset()
            );
            assert_eq!(
                moved16.layer_source(index).map(|p| p.to_rgba16().ok()),
                overlap16.layer_source(index).map(|p| p.to_rgba16().ok())
            );
        }
    }
}

#[test]
fn noncontiguous_roots_move_only_their_link_closures() {
    let byte = byte_topology();
    let typed = typed_topology();
    let together = GeometryEdit::parse(
        &json!({"op":"affine_set","layers":[1,5],
        "matrix":[1,0,0,1,3,-2],"filter":"nearest"})
        .to_string(),
    )
    .unwrap();
    let first = GeometryEdit::parse(
        &json!({"op":"affine_set","layers":[1],
        "matrix":[1,0,0,1,3,-2],"filter":"nearest"})
        .to_string(),
    )
    .unwrap();
    let second = GeometryEdit::parse(
        &json!({"op":"affine_set","layers":[5],
        "matrix":[1,0,0,1,3,-2],"filter":"nearest"})
        .to_string(),
    )
    .unwrap();
    let output8 = byte
        .geometry_edit(&together, LIMIT, 0, None)
        .unwrap()
        .unwrap();
    let first8 = byte.geometry_edit(&first, LIMIT, 0, None).unwrap().unwrap();
    let second8 = byte
        .geometry_edit(&second, LIMIT, 0, None)
        .unwrap()
        .unwrap();
    let output16 = typed.geometry(&together, LIMIT, 0, None).unwrap().unwrap();
    let first16 = typed.geometry(&first, LIMIT, 0, None).unwrap().unwrap();
    let second16 = typed.geometry(&second, LIMIT, 0, None).unwrap().unwrap();
    for index in 0..8 {
        let reference8 = if index == 1 {
            &first8
        } else if index == 5 || index == 7 {
            &second8
        } else {
            &byte
        };
        let reference16 = if index == 1 {
            &first16
        } else if index == 5 || index == 7 {
            &second16
        } else {
            &typed
        };
        assert_eq!(
            output8.layers[index].offset, reference8.layers[index].offset,
            "byte noncontiguous index {index}"
        );
        assert_eq!(
            output8.layers[index].pixels.contiguous_image().as_raw(),
            reference8.layers[index].pixels.contiguous_image().as_raw()
        );
        assert_eq!(
            output8.layers[index].mask_properties.offset,
            reference8.layers[index].mask_properties.offset
        );
        assert_eq!(
            output16.layer(index).unwrap().offset(),
            reference16.layer(index).unwrap().offset(),
            "typed noncontiguous {index}"
        );
        assert_eq!(
            output16.layer_source(index).map(|p| p.to_rgba16().ok()),
            reference16.layer_source(index).map(|p| p.to_rgba16().ok())
        );
        assert_eq!(
            output16
                .layer(index)
                .unwrap()
                .mask()
                .map(|m| m.properties().offset),
            reference16
                .layer(index)
                .unwrap()
                .mask()
                .map(|m| m.properties().offset)
        );
    }
}

#[test]
fn set_refusals_are_atomic_for_invalid_last_member_and_transitive_lock() {
    let byte = byte_topology();
    let typed = typed_topology();
    for operation in ["affine_set", "perspective_set"] {
        let request = |layers: Vec<usize>| edit_set(operation, layers, "nearest");
        let invalid = request(vec![4, 999]);
        assert!(byte.geometry_edit(&invalid, LIMIT, 0, None).is_err());
        assert!(typed.geometry(&invalid, LIMIT, 0, None).is_err());
        let locked8 = byte.with_layer_locks(7, LOCK_POSITION).unwrap();
        let locked16 = typed.with_layer_locks(7, LOCK_POSITION).unwrap().unwrap();
        let original8 = locked8.layers[1].pixels.clone();
        let original16 = locked16.layer_source(1).unwrap().clone();
        let valid = request(vec![4]);
        assert!(locked8.geometry_edit(&valid, LIMIT, 0, None).is_err());
        assert!(locked16.geometry(&valid, LIMIT, 0, None).is_err());
        assert!(LayerRaster::ptr_eq(&locked8.layers[1].pixels, &original8));
        assert_eq!(locked8.layers[1].offset, byte.layers[1].offset);
        assert!(PrecisionImage::ptr_eq(
            locked16.layer_source(1).unwrap(),
            &original16
        ));
        assert_eq!(
            locked16.layer(1).unwrap().offset(),
            typed.layer(1).unwrap().offset()
        );
        let cancelled8 = DrJobControl::default();
        cancelled8.cancel();
        assert!(byte
            .geometry_edit(&valid, LIMIT, 0, Some(&cancelled8))
            .is_err());
        assert_eq!(cancelled8.scratch_bytes(), 0);
        let cancelled16 = DrJobControl::default();
        cancelled16.cancel();
        assert!(typed
            .geometry(&valid, LIMIT, 0, Some(&cancelled16))
            .is_err());
        assert_eq!(cancelled16.scratch_bytes(), 0);
        assert_eq!(locked8.layers[7].link, byte.layers[7].link);
    }
}

fn edit_set(operation: &str, layers: Vec<usize>, filter: &str) -> GeometryEdit {
    let value = if operation == "affine_set" {
        json!({"op":operation,"layers":layers,
            "matrix":[1.03,0.01,-0.02,1.02,0.3,0.4],"filter":filter})
    } else {
        json!({"op":operation,"layers":layers,"source_rect":[0,0,1200,800],
            "quad":[0,0,1180,0,1170,790,0,800],"filter":filter})
    };
    GeometryEdit::parse(&value.to_string()).unwrap()
}

#[test]
fn midwork_set_cancellation_releases_scratch_and_preserves_inputs() {
    let pixels = RgbaImage::from_fn(1200, 800, |x, y| {
        Rgba([(x % 251) as u8, (y % 241) as u8, 31, 255])
    });
    let byte = Arc::new(
        DrDocument::from_pixels(pixels)
            .adding_image_layer(0, RgbaImage::from_pixel(2, 2, Rgba([17, 19, 23, 255])), "B")
            .unwrap(),
    );
    let profile = IccProfile::srgb();
    let image = PrecisionImage::from_rgba16(
        1200,
        800,
        vec![[3001, 5003, 7001, 65535]; 1200 * 800],
        profile.clone(),
        None,
    )
    .unwrap();
    let small =
        PrecisionImage::from_rgba16(2, 2, vec![[11, 13, 17, 65535]; 4], profile.clone(), None)
            .unwrap();
    let typed = Arc::new(
        PrecisionDocument::from_parts(
            1200,
            800,
            profile,
            Metadata::default(),
            Resolution::default(),
            vec![
                PrecisionDocumentLayer::raster("A", image, (0, 0), 1., true).unwrap(),
                PrecisionDocumentLayer::raster("B", small, (0, 0), 1., true).unwrap(),
            ],
        )
        .unwrap(),
    );
    let request = edit_set("perspective_set", vec![0, 1], "lanczos3");
    for depth in [8, 16] {
        let render = || {
            let (byte, typed, request) = (&byte, &typed, &request);
            move |token: usize| {
                let control = unsafe { &*(token as *const DrJobControl) };
                let out = if depth == 8 {
                    byte.geometry_edit(request, LIMIT, 0, Some(control))
                        .map(|_| ())
                } else {
                    typed.geometry(request, LIMIT, 0, Some(control)).map(|_| ())
                };
                match out {
                    Ok(()) => (true, String::new()),
                    Err(e) => (false, e),
                }
            }
        };
        // Park one checkpoint into the resample (the first stage-6 report of
        // a fresh twin's log), past the reservation.
        let n0 = entry_count(&|_, stage, _| stage == 6, render());
        let parked = cancelled_parked_at(n0 + 1, &|_, stage| stage == 6, render());
        eprintln!("{depth}-bit: {}", parked_line(&parked));
        assert!(
            parked.scratch > 0,
            "{depth}-bit worker must be active with reservation"
        );
    }
    assert_eq!(byte.layers[0].offset, (0, 0));
    assert_eq!(
        byte.layers[0].pixels.contiguous_image().get_pixel(0, 0).0,
        [0, 0, 31, 255]
    );
    assert_eq!(typed.layer(0).unwrap().offset(), (0, 0));
    assert_eq!(
        typed.layer_source(0).unwrap().to_rgba16().ok().unwrap()[0],
        [3001, 5003, 7001, 65535]
    );
}

fn byte_document() -> DrDocument {
    let first = DrDocument::from_pixels(RgbaImage::from_pixel(2, 2, Rgba([7, 31, 91, 217])));
    let mut doc = first
        .adding_image_layer(0, RgbaImage::from_pixel(1, 2, Rgba([181, 3, 51, 255])), "B")
        .unwrap();
    doc.layers[1].offset = (3, 0);
    doc
}

fn typed_document() -> PrecisionDocument {
    let profile = IccProfile::srgb();
    let image =
        |samples| PrecisionImage::from_rgba16(1, 1, vec![samples], profile.clone(), None).unwrap();
    let left = PrecisionDocumentLayer::raster("A", image([1, 257, 65535, 32769]), (0, 0), 1., true)
        .unwrap();
    let right =
        PrecisionDocumentLayer::raster("B", image([65534, 3, 5, 65535]), (3, 0), 1., true).unwrap();
    PrecisionDocument::from_parts(
        4,
        2,
        profile,
        Metadata::default(),
        Resolution::default(),
        vec![left, right],
    )
    .unwrap()
}

#[test]
fn byte_set_affine_is_one_edit_and_rejects_duplicate_or_invalid_members() {
    let doc = byte_document();
    let request =
        json!({"op":"affine_set","layers":[0,1],"matrix":[1,0,0,1,2,-3],"filter":"nearest"});
    let text = CString::new(request.to_string()).unwrap();
    let mut error: *mut c_char = ptr::null_mut();
    let plan = unsafe { dr_doc_geometry_edit_plan(&doc, text.as_ptr(), &mut error) };
    assert!(!plan.is_null());
    assert!(error.is_null());
    unsafe { dr_string_free(plan) };
    let pointer =
        unsafe { dr_doc_geometry_edit(&doc, text.as_ptr(), LIMIT, 0, ptr::null(), &mut error) };
    assert!(!pointer.is_null(), "{}", unsafe {
        error
            .as_ref()
            .map(|_| CStr::from_ptr(error).to_string_lossy().into_owned())
            .unwrap_or_default()
    });
    let moved = unsafe { Box::from_raw(pointer) };
    assert_eq!(moved.layers[0].offset, (2, -3));
    assert_eq!(moved.layers[1].offset, (5, -3));
    assert_eq!(doc.layers[0].offset, (0, 0));
    assert_eq!(doc.layers[1].offset, (3, 0));
    assert_eq!(
        *moved.layers[0].pixels.contiguous_image(),
        *doc.layers[0].pixels.contiguous_image()
    );
    let duplicate = CString::new(
        json!({"op":"affine_set","layers":[1,1],"matrix":[1,0,0,1,2,0],"filter":"nearest"})
            .to_string(),
    )
    .unwrap();
    let refused = unsafe { dr_doc_geometry_edit_plan(&doc, duplicate.as_ptr(), &mut error) };
    assert!(refused.is_null());
    assert!(unsafe { CStr::from_ptr(error) }
        .to_string_lossy()
        .contains("duplicate"));
    unsafe { dr_string_free(error) };
    let identity = GeometryEdit::parse(
        &json!({"op":"affine_set","layers":[0,1],"matrix":[1,0,0,1,0,0],"filter":"nearest"})
            .to_string(),
    )
    .unwrap();
    assert!(doc
        .geometry_edit(&identity, LIMIT, 0, None)
        .unwrap()
        .is_none());
}

#[test]
fn one_projective_map_moves_two_separated_rasters_at_both_depths() {
    let quad = [0., 0., 20. / 7., 0., 20. / 7., 20. / 7., 0., 4.];
    let request = json!({"op":"perspective_set","layers":[0,1],
        "source_rect":[0,0,4,4],"quad":quad,"filter":"nearest"});
    let edit = GeometryEdit::parse(&request.to_string()).unwrap();
    let byte = byte_document()
        .geometry_edit(&edit, LIMIT, 0, None)
        .unwrap()
        .unwrap();
    let typed = typed_document()
        .geometry(&edit, LIMIT, 0, None)
        .unwrap()
        .unwrap();
    // The second member starts at x=3. The independent map's right edge x=4
    // becomes 4/1.4=20/7; mapping each member's OWN rect to the supplied
    // quad would instead put it at x=0 and fail both assertions.
    assert!(byte.layers[1].offset.0 >= 2);
    assert!(typed.layer(1).unwrap().offset().0 >= 2);
    assert_ne!(byte.layers[0].offset, byte.layers[1].offset);
    assert_ne!(
        typed.layer(0).unwrap().offset(),
        typed.layer(1).unwrap().offset()
    );
}

#[test]
fn new_byte_exports_tolerate_null_and_malformed_json() {
    let mut error: *mut c_char = ptr::null_mut();
    let plan = unsafe { dr_doc_geometry_edit_plan(ptr::null(), ptr::null(), &mut error) };
    assert!(plan.is_null());
    assert!(!error.is_null());
    unsafe { dr_string_free(error) };
    let output = unsafe {
        dr_doc_geometry_edit(
            ptr::null(),
            ptr::null(),
            LIMIT,
            0,
            ptr::null(),
            ptr::null_mut(),
        )
    };
    assert!(output.is_null());
    let malformed = CString::new("{\"op\":\"affine_set\",\"layers\":[0],\"matrix\":[1]}").unwrap();
    error = ptr::null_mut();
    let result =
        unsafe { dr_doc_geometry_edit_plan(&byte_document(), malformed.as_ptr(), &mut error) };
    assert!(result.is_null());
    assert!(!error.is_null());
    unsafe { dr_string_free(error) };
}

#[test]
fn enlarged_set_destinations_are_admitted_before_any_allocation() {
    let doc = byte_document();
    let original = doc.layers[0].pixels.clone();
    for request in [
        json!({"op":"affine_set","layers":[0,1],
            "matrix":[500,0,0,500,0,0],"filter":"nearest"}),
        json!({"op":"perspective_set","layers":[0,1],
            "source_rect":[0,0,4,2],
            "quad":[0,0,2000,0,1900,1000,0,1000],"filter":"nearest"}),
    ] {
        let edit = GeometryEdit::parse(&request.to_string()).unwrap();
        let plan = doc.geometry_edit_plan(&edit).unwrap();
        assert!(
            plan.scratch_bytes > 8 * 1024 * 1024,
            "enlarged destination missing from plan: {request}"
        );
        let error = match doc.geometry_edit(&edit, 8 * 1024 * 1024, 0, None) {
            Err(error) => error,
            Ok(_) => panic!("enlarged edit bypassed its low budget: {request}"),
        };
        assert!(
            error.contains("limit") || error.contains("budget"),
            "{error}"
        );
        assert!(LayerRaster::ptr_eq(&doc.layers[0].pixels, &original));
        assert_eq!(doc.layers[0].offset, (0, 0));
    }
}

// The reference is intentionally separate from the production homography
// solver and f32 sampling walk. This fixture's analytic inverse is
// (X,Y) -> (X,Y)/(1-X/10), evaluated in f64 at destination pixel centres.
fn weight(kind: &str, distance: f64) -> f64 {
    let x = distance.abs();
    match kind {
        "bilinear" => (1. - x).max(0.),
        "bicubic" if x < 1. => ((1.5 * x - 2.5) * x) * x + 1.,
        "bicubic" if x < 2. => ((-0.5 * x + 2.5) * x - 4.) * x + 2.,
        "bicubic" => 0.,
        "lanczos3" if x < 1e-6 => 1.,
        "lanczos3" if x < 3. => {
            let p = std::f64::consts::PI * x;
            3. * p.sin() * (p / 3.).sin() / (p * p)
        }
        "lanczos3" => 0.,
        _ => unreachable!(),
    }
}

fn taps(kind: &str, center: f64) -> Vec<(i64, f64)> {
    let radius = match kind {
        "bilinear" => 1,
        "bicubic" => 2,
        _ => 3,
    };
    let first = (center - 0.5).floor() as i64 - radius + 1;
    let mut values: Vec<_> = (0..2 * radius)
        .map(|step| {
            let index = first + step;
            (index, weight(kind, center - 0.5 - index as f64))
        })
        .collect();
    let sum: f64 = values.iter().map(|(_, value)| value).sum();
    if sum != 0. {
        for (_, value) in &mut values {
            *value /= sum;
        }
    }
    values
}

fn oracle(
    samples: &[[u16; 4]],
    coverage: &[u16],
    kind: &str,
    x: u32,
    y: u32,
    maximum: f64,
) -> ([u16; 4], u16) {
    let x = f64::from(x) + 0.5;
    let y = f64::from(y) + 0.5;
    let denominator = 1. - x / 10.;
    let (u, v) = (x / denominator, y / denominator);
    if kind == "nearest" {
        let (sx, sy) = (u.floor() as usize, v.floor() as usize);
        if sx >= 3 || sy >= 3 {
            return ([0; 4], 0);
        }
        return (samples[sy * 3 + sx], coverage[sy * 3 + sx]);
    }
    let mut rgba = [0.; 4];
    let mut mask = 0.;
    for (sy, wy) in taps(kind, v) {
        if !(0..3).contains(&sy) {
            continue;
        }
        for (sx, wx) in taps(kind, u) {
            if !(0..3).contains(&sx) {
                continue;
            }
            let index = sy as usize * 3 + sx as usize;
            let sample = samples[index];
            let w = wx * wy;
            let alpha = f64::from(sample[3]);
            for channel in 0..3 {
                rgba[channel] += w * alpha * f64::from(sample[channel]) / maximum;
            }
            rgba[3] += w * alpha;
            mask += w * f64::from(coverage[index]);
        }
    }
    let alpha = rgba[3].clamp(0., maximum).round() as u16;
    let mut output = [0; 4];
    output[3] = alpha;
    if alpha != 0 {
        for channel in 0..3 {
            output[channel] = (rgba[channel] * maximum / rgba[3])
                .clamp(0., maximum)
                .round() as u16;
        }
    }
    (output, mask.clamp(0., maximum).round() as u16)
}

#[test]
fn projective_set_matches_independent_f64_rgba_and_mask_oracle() {
    let samples = [
        [62000, 1000, 300, 0],
        [52000, 3000, 10000, 32769],
        [500, 12000, 60000, 65535],
        [10000, 51000, 9000, 44001],
        [49000, 1000, 28000, 65535],
        [7000, 40000, 55000, 19001],
        [51000, 32000, 4000, 65535],
        [1000, 59000, 32000, 33001],
        [42000, 9000, 22000, 65535],
    ];
    let coverage = [65535, 1001, 32769, 50000, 65000, 12345, 0, 23456, 60000];
    let profile = IccProfile::srgb();
    let typed = PrecisionDocument::from_image(
        PrecisionImage::from_rgba16(3, 3, samples.to_vec(), profile, None).unwrap(),
        Metadata::default(),
        Resolution::default(),
    )
    .unwrap();
    let typed = typed
        .set_layer_mask(
            0,
            Some(
                PrecisionMask::new(
                    PrecisionPlane::from_u16(3, 3, coverage.to_vec(), None).unwrap(),
                    MaskProperties::at((0, 0)),
                    true,
                )
                .unwrap(),
            ),
            LIMIT,
            0,
            None,
        )
        .unwrap()
        .unwrap();
    let bytes: Vec<[u8; 4]> = samples
        .iter()
        .map(|s| s.map(|v| (f64::from(v) / 257.).round() as u8))
        .collect();
    let byte_image = RgbaImage::from_fn(3, 3, |x, y| Rgba(bytes[(y * 3 + x) as usize]));
    let byte = DrDocument::from_pixels(byte_image);
    let mask = GrayImage::from_fn(3, 3, |x, y| {
        Luma([(f64::from(coverage[(y * 3 + x) as usize]) / 257.).round() as u8])
    });
    let byte = byte
        .add_mask(0, MaskKind::FromSelection(mask.as_raw()))
        .unwrap();
    for kind in ["nearest", "bilinear", "bicubic", "lanczos3"] {
        let edit = GeometryEdit::parse(
            &json!({"op":"perspective_set","layers":[0],
            "source_rect":[0,0,3,3],
            "quad":[0,0,30.0/13.0,0,30.0/13.0,30.0/13.0,0,3],
            "filter":kind})
            .to_string(),
        )
        .unwrap();
        let output16 = typed.geometry(&edit, LIMIT, 0, None).unwrap().unwrap();
        let output8 = byte.geometry_edit(&edit, LIMIT, 0, None).unwrap().unwrap();
        let layer16 = output16.layer(0).unwrap();
        let layer8 = &output8.layers[0];
        assert_eq!(layer16.offset(), (0, 0), "{kind}");
        assert_eq!(layer8.offset, (0, 0), "{kind}");
        assert_eq!(
            output16.layer_source(0).unwrap().dimensions(),
            (3, 3),
            "{kind}"
        );
        assert_eq!(layer8.pixels.dimensions(), (3, 3), "{kind}");
        let typed_pixels = output16.layer_source(0).unwrap().to_rgba16().ok().unwrap();
        let typed_mask = layer16.mask().unwrap().plane().samples();
        let byte_mask = layer8.mask.as_ref().unwrap();
        let byte_samples: Vec<[u16; 4]> = bytes.iter().map(|s| s.map(u16::from)).collect();
        let byte_coverage: Vec<u16> = mask.as_raw().iter().copied().map(u16::from).collect();
        let byte_image = layer8.pixels.contiguous_image();
        for y in 0..3 {
            for x in 0..3 {
                let at = (y * 3 + x) as usize;
                let (expected16, mask16) = oracle(&samples, &coverage, kind, x, y, 65535.);
                let (expected8, mask8) = oracle(&byte_samples, &byte_coverage, kind, x, y, 255.);
                for channel in 0..4 {
                    assert!(
                        typed_pixels[at][channel].abs_diff(expected16[channel]) <= 1,
                        "u16 {kind} ({x},{y}) channel {channel}: {} vs {}",
                        typed_pixels[at][channel],
                        expected16[channel]
                    );
                    assert!(
                        u16::from(byte_image.get_pixel(x, y)[channel]).abs_diff(expected8[channel])
                            <= 1,
                        "u8 {kind} ({x},{y}) channel {channel}"
                    );
                }
                assert!(
                    typed_mask[at].abs_diff(mask16) <= 1,
                    "u16 mask {kind} ({x},{y})"
                );
                assert!(
                    u16::from(byte_mask.get_pixel(x, y)[0]).abs_diff(mask8) <= 1,
                    "u8 mask {kind} ({x},{y})"
                );
            }
        }
    }
}

#[test]
fn parallelogram_set_keeps_exact_hidden_rgb_and_mask_allocations() {
    let original = RgbaImage::from_fn(2, 2, |x, y| {
        Rgba(match (x, y) {
            (0, 0) => [251, 3, 7, 0],
            (1, 0) => [9, 211, 31, 127],
            (0, 1) => [41, 59, 233, 255],
            _ => [71, 83, 97, 19],
        })
    });
    let byte = DrDocument::from_pixels(original)
        .add_mask(0, MaskKind::FromSelection(&[0, 31, 127, 255]))
        .unwrap();
    let typed = PrecisionDocument::from_image(
        PrecisionImage::from_rgba16(
            2,
            2,
            vec![
                [65001, 3, 7, 0],
                [9, 54001, 31, 32769],
                [41, 59, 62001, 65535],
                [71, 83, 97, 19],
            ],
            IccProfile::srgb(),
            None,
        )
        .unwrap(),
        Metadata::default(),
        Resolution::default(),
    )
    .unwrap();
    let typed = typed
        .set_layer_mask(
            0,
            Some(
                PrecisionMask::new(
                    PrecisionPlane::from_u16(2, 2, vec![0, 3001, 32769, 65535], None).unwrap(),
                    MaskProperties::at((0, 0)),
                    true,
                )
                .unwrap(),
            ),
            LIMIT,
            0,
            None,
        )
        .unwrap()
        .unwrap();
    let edit = GeometryEdit::parse(
        &json!({"op":"perspective_set","layers":[0],
        "source_rect":[0,0,2,2],"quad":[2,-1,4,-1,4,1,2,1],
        "filter":"lanczos3"})
        .to_string(),
    )
    .unwrap();
    let moved8 = byte.geometry_edit(&edit, LIMIT, 0, None).unwrap().unwrap();
    let moved16 = typed.geometry(&edit, LIMIT, 0, None).unwrap().unwrap();
    assert_eq!(moved8.layers[0].offset, (2, -1));
    assert!(LayerRaster::ptr_eq(
        &byte.layers[0].pixels,
        &moved8.layers[0].pixels
    ));
    assert!(std::sync::Arc::ptr_eq(
        byte.layers[0].mask.as_ref().unwrap(),
        moved8.layers[0].mask.as_ref().unwrap()
    ));
    assert_eq!(
        moved8.layers[0].pixels.contiguous_image().get_pixel(0, 0).0,
        [251, 3, 7, 0]
    );
    assert_eq!(moved16.layer(0).unwrap().offset(), (2, -1));
    assert!(PrecisionImage::ptr_eq(
        typed.layer_source(0).unwrap(),
        moved16.layer_source(0).unwrap()
    ));
    assert_eq!(
        typed
            .layer(0)
            .unwrap()
            .mask()
            .unwrap()
            .plane()
            .samples()
            .as_ptr(),
        moved16
            .layer(0)
            .unwrap()
            .mask()
            .unwrap()
            .plane()
            .samples()
            .as_ptr()
    );
    assert_eq!(
        moved16.layer_source(0).unwrap().to_rgba16().ok().unwrap()[0],
        [65001, 3, 7, 0]
    );
}
