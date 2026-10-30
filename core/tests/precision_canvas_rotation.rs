//! Typed arbitrary canvas rotation: strict requests, shared extents, exact
//! delegation and filtered u16 sampling.

use std::ffi::{c_char, CStr, CString};
use std::ptr;
use std::sync::Arc;

use doctored_core::doc_evaluation::Evaluation;
use doctored_core::doc_guide::GuideOrientation;
use doctored_core::doc_mask::MaskProperties;
use doctored_core::doc_rotate::{rotated_canvas, rotation_kind, RotationKind};
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_precision_edit::{dr_precision_doc_edit, dr_precision_doc_edit_plan};
use doctored_core::icc::IccProfile;
use doctored_core::job_control::DrJobControl;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer as Layer};
use doctored_core::precision_geometry::GeometryEdit;
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use doctored_core::precision_mask::{PrecisionChannel, PrecisionMask, PrecisionPlane};
use doctored_core::retained_source::{SourceEdit, SourceInput};
use doctored_core::style::LayerStyle;
use serde_json::{json, Value};

use crate::inverse_mapping;

const LIMIT: u64 = REFERENCE_WORKING_BYTES;

fn image(w: u32, h: u32) -> PrecisionImage {
    let pixels = (0..w * h)
        .map(|i| {
            let alpha = if i % 7 == 0 {
                0
            } else {
                11_000 + (i * 997) as u16
            };
            [
                300 + (i * 211) as u16,
                900 + (i * 157) as u16,
                65_000 - (i * 173) as u16,
                alpha,
            ]
        })
        .collect();
    PrecisionImage::from_rgba16(w, h, pixels, IccProfile::srgb(), None).unwrap()
}

fn fixture(w: u32, h: u32) -> PrecisionDocument {
    PrecisionDocument::from_parts(
        w,
        h,
        IccProfile::srgb(),
        Metadata::default(),
        Resolution { x: 300.0, y: 150.0 },
        vec![Layer::raster("Pixels", image(w, h), (0, 0), 1.0, true).unwrap()],
    )
    .unwrap()
}

fn plane(w: u32, h: u32, seed: u32) -> Vec<u16> {
    (0..w * h)
        .map(|i| ((i * 1901 + seed * 313) % 65_521) as u16)
        .collect()
}

fn mask(w: u32, h: u32, offset: (i32, i32), enabled: bool, outside: u8) -> PrecisionMask {
    let mut properties = MaskProperties::at(offset);
    properties.linked = false;
    properties.density = 0.625;
    properties.outside_coverage = outside;
    PrecisionMask::new(
        PrecisionPlane::from_u16(w, h, plane(w, h, 7), None).unwrap(),
        properties,
        enabled,
    )
    .unwrap()
}

fn decorated_fixture() -> PrecisionDocument {
    let style = LayerStyle::from_json(
        r##"{"effects":[{"type":"drop_shadow","use_global_light":false,"angle":90,
        "distance":3,"size":4,"spread":0,"opacity":0.5,"color":"#000000"}]}"##,
    )
    .unwrap();
    let base = Layer::raster("Base", image(9, 7), (0, 0), 1.0, true)
        .unwrap()
        .with_mask(Some(mask(9, 7, (1, -1), true, 255)))
        .with_layer_style(Some(&style), (11, 9))
        .unwrap();
    let small = Layer::raster("Small", image(5, 4), (3, 2), 1.0, false)
        .unwrap()
        .with_mask(Some(mask(4, 3, (2, 1), false, 0)));
    let channel = PrecisionChannel::new(
        "Saved",
        PrecisionPlane::from_u16(11, 9, plane(11, 9, 19), None).unwrap(),
    )
    .unwrap();
    let mut doc = PrecisionDocument::from_parts_and_channels(
        11,
        9,
        IccProfile::srgb(),
        Metadata::default(),
        Resolution { x: 300.0, y: 150.0 },
        vec![base, small],
        vec![channel],
    )
    .unwrap();
    doc = doc
        .add_guide(GuideOrientation::Vertical, 1.25)
        .unwrap()
        .add_guide(GuideOrientation::Horizontal, 7.5)
        .unwrap()
        .set_ruler_origin(2.0, 3.0)
        .unwrap();

    let description = CString::new(r#"{"type":"shape","version":3,"probe":"rotation"}"#).unwrap();
    let name = CString::new("Base").unwrap();
    let mut error: *mut c_char = ptr::null_mut();
    let source = doc.layer_source(0).unwrap();
    let described = unsafe {
        doctored_core::ffi_artwork::dr_precision_doc_artwork(
            &doc,
            0,
            true,
            source,
            0,
            0,
            name.as_ptr(),
            description.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut error,
        )
    };
    assert!(error.is_null() && !described.is_null());
    let doc = *unsafe { Box::from_raw(described) };
    let lock = CString::new(json!({"op":"layer_lock","layer":0,"locks":15}).to_string()).unwrap();
    error = ptr::null_mut();
    let locked = unsafe {
        dr_precision_doc_edit(
            &doc,
            lock.as_ptr(),
            ptr::null(),
            0,
            LIMIT,
            0,
            ptr::null(),
            &mut error,
        )
    };
    assert!(error.is_null() && !locked.is_null());
    *unsafe { Box::from_raw(locked) }
}

fn request(degrees: f64, filter: &str) -> Value {
    json!({"op":"rotate_canvas","degrees":degrees,"filter":filter})
}

fn parse(value: &Value) -> GeometryEdit {
    GeometryEdit::parse(&value.to_string()).unwrap()
}

fn direct(doc: &PrecisionDocument, value: Value) -> PrecisionDocument {
    doc.geometry(&parse(&value), LIMIT, 0, None)
        .unwrap()
        .unwrap()
}

fn assert_same_document(got: &PrecisionDocument, want: &PrecisionDocument, what: &str) {
    assert_eq!(got.dimensions(), want.dimensions(), "{what}: canvas");
    assert_eq!(got.resolution(), want.resolution(), "{what}: ppi");
    assert_eq!(got.guides(), want.guides(), "{what}: guides");
    assert_eq!(got.ruler_origin(), want.ruler_origin(), "{what}: ruler");
    assert_eq!(got.layer_count(), want.layer_count(), "{what}: records");
    for i in 0..got.layer_count() {
        let (a, b) = (got.layer(i).unwrap(), want.layer(i).unwrap());
        assert_eq!(a.offset(), b.offset(), "{what}: layer {i} offset");
        assert_eq!(
            got.layer_source(i).and_then(|i| i.to_rgba16().ok()),
            want.layer_source(i).and_then(|i| i.to_rgba16().ok()),
            "{what}: layer {i} samples"
        );
        assert_eq!(got.artwork_description(i), want.artwork_description(i));
        match (a.mask(), b.mask()) {
            (Some(a), Some(b)) => {
                assert_eq!(a.plane().dimensions(), b.plane().dimensions());
                assert_eq!(a.plane().samples(), b.plane().samples());
                assert!(
                    a.properties() == b.properties(),
                    "{what}: layer {i} mask properties"
                );
                assert_eq!(a.enabled(), b.enabled());
            }
            (None, None) => {}
            _ => panic!("{what}: layer {i} mask presence"),
        }
        match (a.style(), b.style()) {
            (Some(a), Some(b)) => assert!(Arc::ptr_eq(a, b), "{what}: layer {i} style"),
            (None, None) => {}
            _ => panic!("{what}: layer {i} style presence"),
        }
    }
    assert_eq!(got.channels().len(), want.channels().len());
    for (a, b) in got.channels().iter().zip(want.channels()) {
        assert_eq!(a.plane().samples(), b.plane().samples(), "{what}: channel");
    }
}

fn error_text(error: *mut c_char) -> String {
    if error.is_null() {
        return String::new();
    }
    let text = unsafe { CStr::from_ptr(error) }
        .to_string_lossy()
        .into_owned();
    unsafe { dr_string_free(error) };
    text
}

fn assert_ffi_plan_and_edit_accept(doc: &PrecisionDocument, value: &Value) {
    let text = CString::new(value.to_string()).unwrap();
    let mut error: *mut c_char = ptr::null_mut();
    let plan = unsafe { dr_precision_doc_edit_plan(doc, text.as_ptr(), LIMIT, 0, &mut error) };
    assert!(
        !plan.is_null(),
        "plan refused {value}: {}",
        error_text(error)
    );
    unsafe { dr_string_free(plan) };

    error = ptr::null_mut();
    let output = unsafe {
        dr_precision_doc_edit(
            doc,
            text.as_ptr(),
            ptr::null(),
            0,
            LIMIT,
            0,
            ptr::null(),
            &mut error,
        )
    };
    assert!(
        !output.is_null(),
        "edit refused {value}: {}",
        error_text(error)
    );
    unsafe { drop(Box::from_raw(output)) };
}

fn retained_fixture() -> PrecisionDocument {
    let doc = decorated_fixture();
    let master = fixture(4, 3);
    let place = SourceEdit::parse(
        &json!({"op":"place","source_id":"rotation-master","revision_id":"v1",
                "name":"Placed","x":1,"y":1})
        .to_string(),
    )
    .unwrap();
    doc.source_edit_input(Some(SourceInput::Typed(&master)), &place, LIMIT, 0, None)
        .unwrap()
        .unwrap()
}

#[test]
fn request_is_strict_and_capability_admits_16_and_32_bit() {
    parse(&request(30.0, "bicubic"));
    for key in ["degrees", "filter"] {
        let mut value = request(30.0, "bicubic");
        value.as_object_mut().unwrap().remove(key);
        assert!(
            GeometryEdit::parse(&value.to_string()).is_err(),
            "missing {key}"
        );
    }
    let mut extra = request(30.0, "bicubic");
    extra["extra"] = json!(true);
    assert!(GeometryEdit::parse(&extra.to_string()).is_err());
    assert!(
        doctored_core::precision_capability::require("geometry_rotate_canvas", "document", 16)
            .is_ok()
    );
    // The filtered rotation is admitted at 32-bit float too.
    assert!(
        doctored_core::precision_capability::require("geometry_rotate_canvas", "document", 32)
            .is_ok()
    );
}

#[test]
fn independent_extent_cases_and_near_quarters_match_the_shared_plan() {
    let cases = [
        // The raw ceil-box plan sizes the exact quarter turn 48x63 but places
        // it half a pixel off on an odd W+H; execution delegates this exact
        // angle to rotate90 instead. 89.999 is ceil(48.0011) x ceil(63.0008).
        (63, 48, 90.0, RotationKind::Rotate90, (48, 63)),
        (63, 48, 89.999, RotationKind::Filtered, (49, 64)),
        (100, 10, 80.0, RotationKind::Filtered, (28, 101)),
        (17, 12, 0.0, RotationKind::Identity, (17, 12)),
    ];
    for (w, h, degrees, kind, expected) in cases {
        assert_eq!(rotation_kind(degrees).unwrap().0, kind);
        let plan = rotated_canvas(w, h, degrees).unwrap();
        assert_eq!((plan.width, plan.height), expected);
    }
}

#[test]
fn identity_is_none_and_quarters_delegate_with_ppi() {
    let doc = fixture(7, 5);
    assert!(doc
        .geometry(&parse(&request(360.0, "bilinear")), LIMIT, 0, None)
        .unwrap()
        .is_none());
    for (degrees, exact) in [
        (
            90.0,
            doc.geometry(&GeometryEdit::Rotate90, LIMIT, 0, None)
                .unwrap(),
        ),
        (
            180.0,
            doc.geometry(&GeometryEdit::Rotate180, LIMIT, 0, None)
                .unwrap(),
        ),
        (
            270.0,
            doc.geometry(&GeometryEdit::Rotate270, LIMIT, 0, None)
                .unwrap(),
        ),
    ] {
        let rotated = doc
            .geometry(&parse(&request(degrees, "lanczos3")), LIMIT, 0, None)
            .unwrap()
            .unwrap();
        let exact = exact.unwrap();
        assert_eq!(rotated.dimensions(), exact.dimensions());
        assert_eq!(rotated.resolution(), exact.resolution());
        assert_eq!(
            rotated.layer_source(0).unwrap().to_rgba16().ok().unwrap(),
            exact.layer_source(0).unwrap().to_rgba16().ok().unwrap()
        );
    }

    // The same delegation over independent offsets, masks (including a
    // disabled one), a saved channel, chrome, style, descriptions and PPI.
    let decorated = decorated_fixture();
    for (degrees, exact) in [
        (90.0, GeometryEdit::Rotate90),
        (180.0, GeometryEdit::Rotate180),
        (270.0, GeometryEdit::Rotate270),
    ] {
        let rotated = direct(&decorated, request(degrees, "lanczos3"));
        let exact = decorated.geometry(&exact, LIMIT, 0, None).unwrap().unwrap();
        assert_same_document(&rotated, &exact, &format!("{degrees} degrees"));
    }
}

#[test]
fn bilinear_and_bicubic_match_the_independent_u16_oracle() {
    let doc = fixture(7, 5);
    let source = doc.layer_source(0).unwrap().to_rgba16().ok().unwrap();
    let source32: Vec<[u32; 4]> = source.iter().map(|p| p.map(u32::from)).collect();
    for filter in ["bilinear", "bicubic"] {
        let plan = rotated_canvas(7, 5, 31.0).unwrap();
        let out = doc
            .geometry(&parse(&request(31.0, filter)), LIMIT, 0, None)
            .unwrap()
            .unwrap();
        assert_eq!(out.dimensions(), (plan.width, plan.height));
        // The layer's own extent: the canvas-sized layer's rotated box, which
        // under R2's floor placement need not be the new canvas.
        let destination = inverse_mapping::bounds((0, 0), (7, 5), plan.matrix);
        let got_image = out.layer_source(0).unwrap();
        let got_offset = out.layer(0).unwrap().offset();
        assert_eq!(
            (
                got_offset.0,
                got_offset.1,
                got_image.dimensions().0,
                got_image.dimensions().1
            ),
            destination,
            "{filter}: the layer's extent"
        );
        let got: Vec<[u32; 4]> = got_image
            .to_rgba16()
            .ok()
            .unwrap()
            .iter()
            .map(|p| p.map(u32::from))
            .collect();
        let want = inverse_mapping::reference_rgba(
            &source32,
            (7, 5),
            (0, 0),
            destination,
            inverse_mapping::invert(plan.matrix),
            filter,
            65_535,
        );
        inverse_mapping::assert_rgba_within_one(&got, &want, filter);
    }
}

#[test]
fn every_plane_and_document_state_rides_filtered_rotation() {
    let doc = decorated_fixture();
    let plan = rotated_canvas(11, 9, 31.0).unwrap();
    let inverse = inverse_mapping::invert(plan.matrix);
    for filter in ["bilinear", "bicubic"] {
        let out = direct(&doc, request(31.0, filter));
        assert_eq!(out.dimensions(), (plan.width, plan.height));
        assert_eq!(out.resolution(), doc.resolution(), "filtered PPI stays put");
        assert_eq!(out.evaluation(), Evaluation::Encoded);
        assert_eq!(out.layer(0).unwrap().locks(), doc.layer(0).unwrap().locks());
        assert_ne!(out.layer(0).unwrap().locks(), 0, "Lock All is exercised");
        assert!(Arc::ptr_eq(
            out.layer(0).unwrap().style().unwrap(),
            doc.layer(0).unwrap().style().unwrap()
        ));
        assert!(
            out.artwork_description(0).is_none(),
            "resampling clears descriptions"
        );

        for i in 0..doc.layer_count() {
            let source = doc.layer_source(i).unwrap();
            let source_extent = (
                doc.layer(i).unwrap().offset().0,
                doc.layer(i).unwrap().offset().1,
                source.dimensions().0,
                source.dimensions().1,
            );
            let destination = inverse_mapping::bounds(
                (source_extent.0, source_extent.1),
                (source_extent.2, source_extent.3),
                plan.matrix,
            );
            let got_layer = out.layer(i).unwrap();
            let got_image = out.layer_source(i).unwrap();
            assert_eq!(
                (
                    got_layer.offset().0,
                    got_layer.offset().1,
                    got_image.dimensions().0,
                    got_image.dimensions().1
                ),
                destination,
                "{filter}: raster {i} extent"
            );
            let source32: Vec<[u32; 4]> = source
                .to_rgba16()
                .ok()
                .unwrap()
                .iter()
                .map(|p| p.map(u32::from))
                .collect();
            let want = inverse_mapping::reference_rgba(
                &source32,
                source.dimensions(),
                (source_extent.0, source_extent.1),
                destination,
                inverse,
                filter,
                65_535,
            );
            let got: Vec<[u32; 4]> = got_image
                .to_rgba16()
                .ok()
                .unwrap()
                .iter()
                .map(|p| p.map(u32::from))
                .collect();
            inverse_mapping::assert_rgba_within_one(&got, &want, &format!("{filter}: raster {i}"));

            let before_mask = doc.layer(i).unwrap().mask().unwrap();
            let after_mask = got_layer.mask().unwrap();
            let mask_destination = inverse_mapping::bounds(
                before_mask.properties().offset,
                before_mask.plane().dimensions(),
                plan.matrix,
            );
            assert_eq!(
                after_mask.properties().offset,
                (mask_destination.0, mask_destination.1)
            );
            assert_eq!(
                after_mask.plane().dimensions(),
                (mask_destination.2, mask_destination.3)
            );
            assert_eq!(after_mask.enabled(), before_mask.enabled());
            assert_eq!(
                after_mask.properties().linked,
                before_mask.properties().linked
            );
            assert_eq!(
                after_mask.properties().density,
                before_mask.properties().density
            );
            assert_eq!(
                after_mask.properties().outside_coverage,
                before_mask.properties().outside_coverage
            );
            let source_plane: Vec<u32> = before_mask
                .plane()
                .samples()
                .iter()
                .map(|&v| u32::from(v))
                .collect();
            let want = inverse_mapping::reference_plane(
                &source_plane,
                before_mask.plane().dimensions(),
                before_mask.properties().offset,
                mask_destination,
                inverse,
                filter,
                u32::from(before_mask.properties().outside_coverage) * 257,
                65_535,
            );
            let got: Vec<u32> = after_mask
                .plane()
                .samples()
                .iter()
                .map(|&v| u32::from(v))
                .collect();
            inverse_mapping::assert_plane_within_one(&got, &want, &format!("{filter}: mask {i}"));
        }

        let source: Vec<u32> = doc.channels()[0]
            .plane()
            .samples()
            .iter()
            .map(|&v| u32::from(v))
            .collect();
        let want = inverse_mapping::reference_plane(
            &source,
            (11, 9),
            (0, 0),
            (0, 0, plan.width, plan.height),
            inverse,
            filter,
            0,
            65_535,
        );
        let got: Vec<u32> = out.channels()[0]
            .plane()
            .samples()
            .iter()
            .map(|&v| u32::from(v))
            .collect();
        inverse_mapping::assert_plane_within_one(&got, &want, &format!("{filter}: channel"));

        let origin = plan.origin();
        let chrome: Vec<(GuideOrientation, f64)> = out
            .guides()
            .iter()
            .map(|g| (g.orientation, g.position))
            .collect();
        assert_eq!(
            chrome,
            vec![
                (GuideOrientation::Horizontal, 7.5 + f64::from(origin.1)),
                (GuideOrientation::Vertical, 1.25 + f64::from(origin.0)),
            ]
        );
        assert_eq!(
            out.ruler_origin(),
            (2.0 + f64::from(origin.0), 3.0 + f64::from(origin.1))
        );
    }

    let linear = fixture(7, 5)
        .with_evaluation(Evaluation::LinearLight)
        .unwrap()
        .unwrap();
    let rotated = direct(&linear, request(31.0, "bilinear"));
    assert_eq!(rotated.evaluation(), Evaluation::LinearLight);
}

#[test]
fn plan_and_edit_ffi_accept_rotation_and_preserve_refusals() {
    let doc = fixture(7, 5);
    let text = CString::new(request(17.0, "bicubic").to_string()).unwrap();
    let mut error: *mut c_char = ptr::null_mut();
    let plan = unsafe { dr_precision_doc_edit_plan(&doc, text.as_ptr(), LIMIT, 0, &mut error) };
    assert!(!plan.is_null(), "{}", unsafe {
        error
            .as_ref()
            .map(|_| CStr::from_ptr(error).to_string_lossy().into_owned())
            .unwrap_or_default()
    });
    unsafe { dr_string_free(plan) };
    let out = unsafe {
        dr_precision_doc_edit(
            &doc,
            text.as_ptr(),
            ptr::null(),
            0,
            LIMIT,
            0,
            ptr::null(),
            &mut error,
        )
    };
    assert!(!out.is_null());
    unsafe { drop(Box::from_raw(out)) };

    let bad = CString::new(request(f64::INFINITY, "nearest").to_string()).unwrap();
    let refused = unsafe { dr_precision_doc_edit_plan(&doc, bad.as_ptr(), LIMIT, 0, &mut error) };
    assert!(refused.is_null());
    assert!(!error.is_null());
    unsafe { dr_string_free(error) };

    let retained = retained_fixture();
    let edit = request(17.0, "bicubic");
    let plan = retained.geometry_plan(&parse(&edit)).unwrap();
    assert!(plan.scratch_bytes > 0);
    let index = retained.layer_count() - 1;
    let original = retained.layer(index).unwrap().effective_source().unwrap();
    let rotated = retained
        .geometry(&parse(&edit), LIMIT, 0, None)
        .unwrap()
        .unwrap();
    let moved = rotated.layer(index).unwrap().effective_source().unwrap();
    assert!(Arc::ptr_eq(&original.asset, &moved.asset));
    assert_eq!(moved.asset.id, "rotation-master");
    let text = CString::new(edit.to_string()).unwrap();
    let mut error: *mut c_char = ptr::null_mut();
    let ffi_plan =
        unsafe { dr_precision_doc_edit_plan(&retained, text.as_ptr(), LIMIT, 0, &mut error) };
    assert!(!ffi_plan.is_null());
    assert!(error.is_null());
    unsafe { dr_string_free(ffi_plan) };
    let ffi_result = unsafe {
        dr_precision_doc_edit(
            &retained,
            text.as_ptr(),
            ptr::null(),
            0,
            LIMIT,
            0,
            ptr::null(),
            &mut error,
        )
    };
    assert!(!ffi_result.is_null());
    assert!(error.is_null());
    unsafe { drop(Box::from_raw(ffi_result)) };
}

/// A direct float `rotate_canvas` is admitted. The identity is a
/// no-op, an exact quarter turn delegates to the Nearest PERMUTATION (bit
/// for bit the `rotate90` answer), and a filtered angle resamples.
#[test]
fn direct_float_rotate_canvas_is_admitted_with_exact_delegation() {
    let path = "tests/fixtures/float_11f/scene.tiff";
    if !std::path::Path::new(path).exists() {
        return;
    }
    let doc = PrecisionDocument::open_preserving(path, LIMIT, 0, None).unwrap();
    assert!(doc.is_float());

    assert!(
        doc.geometry(&parse(&request(0.0, "nearest")), LIMIT, 0, None)
            .expect("the identity is not an error")
            .is_none(),
        "0 degrees is an exact no-op"
    );

    let bits = |d: &PrecisionDocument| -> Vec<Vec<u32>> {
        (0..d.layer_count())
            .filter_map(|i| d.layer_source(i))
            .map(|image| {
                image
                    .to_rgba32()
                    .unwrap()
                    .iter()
                    .flat_map(|p| p.iter().map(|v| v.to_bits()))
                    .collect()
            })
            .collect()
    };
    let turned = direct(&doc, request(90.0, "bicubic"));
    let quarter = direct(&doc, json!({"op": "rotate90"}));
    assert_eq!(turned.dimensions(), quarter.dimensions());
    assert_eq!(
        bits(&turned),
        bits(&quarter),
        "90 degrees delegates to the quarter-turn permutation, bit for bit"
    );

    for filter in ["nearest", "bicubic", "lanczos3"] {
        let value = request(31.0, filter);
        doc.geometry_plan(&parse(&value))
            .unwrap_or_else(|e| panic!("31 degrees {filter} plans: {e}"));
        let out = direct(&doc, value.clone());
        assert!(out.is_float(), "{filter}");
        let (w, h) = doc.dimensions();
        let plan = rotated_canvas(w, h, 31.0).unwrap();
        assert_eq!(
            out.dimensions(),
            (plan.width, plan.height),
            "{filter}: the expanded canvas"
        );
        assert_ffi_plan_and_edit_accept(&doc, &value);
    }
}

#[test]
fn resource_and_cancellation_refusals_leave_every_input_allocation_untouched() {
    let doc = decorated_fixture();
    let edit = parse(&request(31.0, "bicubic"));
    let before_rasters: Vec<PrecisionImage> = (0..doc.layer_count())
        .map(|i| doc.layer_source(i).unwrap().clone())
        .collect();
    let before_masks: Vec<usize> = (0..doc.layer_count())
        .map(|i| {
            doc.layer(i)
                .unwrap()
                .mask()
                .unwrap()
                .plane()
                .samples()
                .as_ptr() as usize
        })
        .collect();
    let channel = doc.channels()[0].plane().samples().as_ptr() as usize;

    assert!(
        doc.geometry_plan(&edit).is_ok(),
        "the ordinary plan is admitted"
    );
    let capacity = match doc.geometry(&edit, 1, 0, None) {
        Err(message) => message,
        Ok(_) => panic!("one-byte limit must refuse"),
    };
    assert!(capacity.contains("working limit is 1 bytes"), "{capacity}");

    let control = DrJobControl::default();
    control.cancel();
    let cancelled = match doc.geometry(&edit, LIMIT, 0, Some(&control)) {
        Err(message) => message,
        Ok(_) => panic!("cancelled edit must refuse"),
    };
    assert!(cancelled.contains("cancel"), "{cancelled}");

    let after_rasters: Vec<PrecisionImage> = (0..doc.layer_count())
        .map(|i| doc.layer_source(i).unwrap().clone())
        .collect();
    let after_masks: Vec<usize> = (0..doc.layer_count())
        .map(|i| {
            doc.layer(i)
                .unwrap()
                .mask()
                .unwrap()
                .plane()
                .samples()
                .as_ptr() as usize
        })
        .collect();
    assert_eq!(after_rasters.len(), before_rasters.len());
    assert!(after_rasters
        .iter()
        .zip(&before_rasters)
        .all(|(a, b)| PrecisionImage::ptr_eq(a, b)));
    assert_eq!(after_masks, before_masks);
    assert_eq!(
        doc.channels()[0].plane().samples().as_ptr() as usize,
        channel
    );
    assert_eq!(doc.dimensions(), (11, 9));
    assert!(doc.artwork_description(0).is_some());
}

#[test]
fn filtered_rotation_native_round_trip_is_byte_identical_and_input_is_pure() {
    let doc = decorated_fixture();
    let source_pointer = doc.layer_source(0).unwrap().clone();
    let out = direct(&doc, request(31.0, "bicubic"));
    assert_eq!(doc.dimensions(), (11, 9));
    assert!(PrecisionImage::ptr_eq(
        doc.layer_source(0).unwrap(),
        &source_pointer
    ));
    assert!(doc.artwork_description(0).is_some());

    let bytes = out.encode_native(LIMIT, 0, None).unwrap();
    let reopened = PrecisionDocument::parse_native(&bytes, LIMIT, 0, None).unwrap();
    assert_eq!(reopened.dimensions(), out.dimensions());
    assert_eq!(reopened.resolution(), out.resolution());
    assert_eq!(reopened.guides().len(), out.guides().len());
    assert_eq!(reopened.ruler_origin(), out.ruler_origin());
    assert_eq!(
        reopened.channels()[0].plane().samples(),
        out.channels()[0].plane().samples()
    );
    assert!(reopened.artwork_description(0).is_none());
    assert_eq!(reopened.evaluation(), Evaluation::Encoded);
    assert_eq!(reopened.encode_native(LIMIT, 0, None).unwrap(), bytes);
}
