//! Immutable source revisions, explicit cross-depth fallbacks and link
//! records, with independent code oracles and one mixed ownership inventory.
use crate::group_roots;
use doctored_core::doc::{BlendMode, DrDocument, LayerRaster};
use doctored_core::doc_mask::MaskProperties;
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_precision::dr_precision_resources_report;
use doctored_core::ffi_resources::dr_mixed_resources_report_with_raw_masks;
use doctored_core::ffi_retained_source::*;
use doctored_core::icc::IccProfile;
use doctored_core::job_control::DrJobControl;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use doctored_core::precision_mask::{PrecisionMask, PrecisionPlane};
use doctored_core::retained_source::{SourceEdit, SourceInput};
use doctored_core::style::LayerStyle;
use group_roots::GroupRoots;
use image::{GrayImage, Luma, Rgba, RgbaImage};
use serde_json::{json, Value};
use std::ffi::{c_char, CStr, CString};
use std::ptr;
use std::sync::Arc;

use crate::common;
use common::probe::{cancelled_parked_at, entry_then_cancel, parked_line};

use crate::resource_report;
const LIMIT: u64 = REFERENCE_WORKING_BYTES;
fn byte(w: u32, h: u32, seed: u32) -> DrDocument {
    DrDocument::from_pixels(RgbaImage::from_fn(w, h, |x, y| {
        Rgba([
            (x * 17 + y * 7 + seed) as u8,
            (x * 3 + y * 43 + seed * 2) as u8,
            (x * 53 + y * 13 + seed * 3) as u8,
            ((x * 29 + y * 11 + seed) % 256) as u8,
        ])
    }))
}
fn typed(w: u32, h: u32, seed: u32) -> PrecisionDocument {
    PrecisionDocument::from_image(
        PrecisionImage::from_rgba16(
            w,
            h,
            (0..w * h)
                .map(|i| {
                    [
                        i.wrapping_mul(1103).wrapping_add(seed) as u16,
                        i.wrapping_mul(7109).wrapping_add(seed) as u16,
                        i.wrapping_mul(9973).wrapping_add(seed) as u16,
                        if i % 11 == 0 {
                            0
                        } else {
                            (i * 47 + seed) as u16
                        },
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
fn place(link: bool, convert: bool) -> Value {
    json!({"op":"place","source_id":"family-a","revision_id":"version-a","name":"Placed","x":0,"y":0,"link":if link{json!({"path":"/unavailable/original.png","sha256":"a".repeat(64)})}else{Value::Null},"depth_conversion":if convert{"convert_fallback"}else{"reject"}})
}
fn transform(matrix: [f64; 6]) -> Value {
    json!({"op":"transform","layer":1,"matrix":matrix,"filter":"lanczos3"})
}
fn parsed(value: Value) -> SourceEdit {
    SourceEdit::parse(&value.to_string()).unwrap()
}
unsafe fn text(pointer: *mut c_char) -> String {
    if pointer.is_null() {
        return String::new();
    }
    let result = unsafe { CStr::from_ptr(pointer) }
        .to_str()
        .unwrap()
        .to_owned();
    unsafe { dr_string_free(pointer) };
    result
}
fn json_result(pointer: *mut c_char, error: *mut c_char) -> Value {
    assert!(error.is_null(), "{}", unsafe { text(error) });
    assert!(!pointer.is_null());
    serde_json::from_str(&unsafe { text(pointer) }).unwrap()
}
fn edit8(
    doc: &DrDocument,
    source8: Option<&DrDocument>,
    source16: Option<&PrecisionDocument>,
    value: Value,
) -> DrDocument {
    let request = CString::new(value.to_string()).unwrap();
    let mut error = ptr::null_mut();
    let out = unsafe {
        dr_doc_mixed_source_edit(
            doc,
            source8.map_or(ptr::null(), |s| s),
            source16.map_or(ptr::null(), |s| s),
            request.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut error,
        )
    };
    assert!(error.is_null(), "{}", unsafe { text(error) });
    assert!(!out.is_null());
    *unsafe { Box::from_raw(out) }
}
fn edit16(
    doc: &PrecisionDocument,
    source8: Option<&DrDocument>,
    source16: Option<&PrecisionDocument>,
    value: Value,
) -> PrecisionDocument {
    let request = CString::new(value.to_string()).unwrap();
    let mut error = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_mixed_source_edit(
            doc,
            source8.map_or(ptr::null(), |s| s),
            source16.map_or(ptr::null(), |s| s),
            request.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut error,
        )
    };
    assert!(error.is_null(), "{}", unsafe { text(error) });
    assert!(!out.is_null());
    *unsafe { Box::from_raw(out) }
}
#[test]
fn cross_depth_requires_authorization_and_retains_actual_master_codes() {
    let source16 = typed(32, 24, 1);
    let destination8 = byte(32, 24, 0);
    let rejected = destination8
        .source_plan_input(
            Some(SourceInput::Typed(&source16)),
            &parsed(place(false, false)),
            24_000_000_000,
        )
        .err()
        .unwrap();
    assert!(rejected.contains("convert_fallback"));
    let placed8 = edit8(&destination8, None, Some(&source16), place(false, true));
    let info = placed8.source_info(1).unwrap();
    assert_eq!(info["master_depth"], 16);
    assert_eq!(info["fallback_depth"], 8);
    for (actual, original) in placed8.layers[1].pixels.contiguous_image().pixels().zip(
        source16
            .layer_source(0)
            .unwrap()
            .to_rgba16()
            .ok()
            .unwrap()
            .iter(),
    ) {
        assert_eq!(
            actual.0,
            original.map(|v| ((u32::from(v) + 128) / 257) as u8)
        );
    }
    let master = placed8.layers[1]
        .effective_source(&placed8.profile)
        .unwrap();
    assert_eq!(
        master.asset.rgba16().unwrap().to_rgba16().ok(),
        source16.layer_source(0).unwrap().to_rgba16().ok()
    );
    let smaller = edit8(
        &placed8,
        None,
        None,
        transform([0.25, 0., 0., 0.25, 0., 0.]),
    );
    let restored = edit8(&smaller, None, None, transform([4., 0., 0., 4., 0., 0.]));
    assert_eq!(
        *restored.layers[1].pixels.contiguous_image(),
        *placed8.layers[1].pixels.contiguous_image()
    );
    let source8 = byte(32, 24, 3);
    let destination16 = typed(32, 24, 0);
    assert!(destination16
        .source_plan_input(
            Some(SourceInput::Byte(&source8)),
            &parsed(place(false, false)),
            24_000_000_000
        )
        .err()
        .unwrap()
        .contains("convert_fallback"));
    let placed16 = edit16(&destination16, Some(&source8), None, place(false, true));
    for (actual, original) in placed16
        .layer_source(1)
        .unwrap()
        .to_rgba16()
        .ok()
        .unwrap()
        .iter()
        .zip(source8.layers[0].pixels.contiguous_image().pixels())
    {
        assert_eq!(*actual, original.0.map(|v| u16::from(v) * 257));
    }
    let info = placed16.source_info(1).unwrap();
    assert_eq!(info["master_depth"], 8);
    assert_eq!(info["fallback_depth"], 16);
    let smaller = edit16(
        &placed16,
        None,
        None,
        transform([0.25, 0., 0., 0.25, 0., 0.]),
    );
    let restored = edit16(&smaller, None, None, transform([4., 0., 0., 4., 0., 0.]));
    assert_eq!(
        restored.layer_source(1).unwrap().to_rgba16().ok(),
        placed16.layer_source(1).unwrap().to_rgba16().ok()
    );
    assert!(LayerRaster::ptr_eq(
        restored
            .layer(1)
            .unwrap()
            .effective_source()
            .unwrap()
            .asset
            .rgba8()
            .unwrap(),
        &source8.layers[0].pixels
    ));
}
#[test]
fn replacement_preserves_affine_properties_and_old_duplicate_content_revision() {
    let source = byte(16, 16, 1);
    let mut placed = edit8(&source, Some(&source), None, place(true, false));
    placed = edit8(&placed, None, None, transform([0.5, 0., 0., 0.5, 3., 4.]));
    let style = Arc::new(
        LayerStyle::from_json(
            r#"{"fill_opacity":0.7,"effects":[{"type":"drop_shadow","distance":3}]}"#,
        )
        .unwrap(),
    );
    placed.layers[1].style = Some(style.clone());
    placed.layers[1].opacity = 0.4;
    placed.layers[1].name = "Kept name".into();
    placed.layers[1].blend = BlendMode::Multiply;
    placed.layers[1].mask = Some(Arc::new(GrayImage::from_pixel(5, 7, Luma([153]))));
    placed.layers[1].mask_properties = MaskProperties::at((-2, 6));
    placed.layers[1].mask_properties.linked = false;
    let duplicate = placed.duplicating_layer(1).unwrap();
    let replacement = byte(32, 16, 71);
    let changed = edit8(
        &duplicate,
        Some(&replacement),
        None,
        json!({"op":"replace","layer":1,"revision_id":"version-b","link":null}),
    );
    let old = &duplicate.layers[1];
    let new = &changed.layers[1];
    assert_eq!(new.name, old.name);
    assert_eq!(new.opacity, old.opacity);
    assert_eq!(new.blend, old.blend);
    assert_eq!(new.visible, old.visible);
    assert!(Arc::ptr_eq(new.style.as_ref().unwrap(), &style));
    assert!(Arc::ptr_eq(
        new.mask.as_ref().unwrap(),
        old.mask.as_ref().unwrap()
    ));
    assert_eq!(new.mask_properties.offset, old.mask_properties.offset);
    assert!(!new.mask_properties.linked);
    assert_eq!(new.pixels.dimensions(), (16, 8));
    let a = changed.layers[1]
        .effective_source(&changed.profile)
        .unwrap();
    let b = changed.layers[2]
        .effective_source(&changed.profile)
        .unwrap();
    assert_eq!(a.asset.id, b.asset.id);
    assert_eq!(a.asset.revision_id, "version-b");
    assert_eq!(b.asset.revision_id, "version-a");
    assert_eq!(a.transform, b.transform);
    assert!(a.link.is_none());
    assert!(b.link.is_some());
    assert!(!Arc::ptr_eq(&a.asset, &b.asset));
    assert!(b
        .asset
        .rgba8()
        .unwrap()
        .same_bytes(&source.layers[0].pixels));
    assert!(a
        .asset
        .rgba8()
        .unwrap()
        .same_bytes(&replacement.layers[0].pixels));
    assert!(changed
        .source_plan_input(
            Some(SourceInput::Byte(&replacement)),
            &parsed(json!({"op":"replace","layer":1,"revision_id":"version-a"})),
            24_000_000_000
        )
        .err()
        .unwrap()
        .contains("new source revision"));
}
#[test]
fn typed_replacement_keeps_mask_profile_and_structure_with_cross_depth_original() {
    let source = typed(16, 16, 1);
    let placed = edit16(&source, None, Some(&source), place(true, false));
    let mask = PrecisionMask::new(
        PrecisionPlane::from_u16(7, 5, vec![12345; 35], None).unwrap(),
        MaskProperties::at((2, -3)),
        true,
    )
    .unwrap();
    let placed = placed
        .set_layer_mask(1, Some(mask), LIMIT, 0, None)
        .unwrap()
        .unwrap()
        .set_layer_opacity(1, 0.625)
        .unwrap()
        .unwrap();
    let grouped = placed.group_all(0.75).unwrap().unwrap();
    let old = grouped.layer(1).unwrap().clone();
    let replacement = byte(24, 20, 17);
    let next = edit16(
        &grouped,
        Some(&replacement),
        None,
        json!({"op":"replace","layer":1,"revision_id":"version-b","depth_conversion":"convert_fallback","link":{"path":"/other/photo.png","sha256":"b".repeat(64)}}),
    );
    assert_eq!(next.layer_count(), grouped.layer_count());
    assert_eq!(next.layer(2).unwrap().opacity(), 0.75);
    assert_eq!(next.layer(1).unwrap().opacity(), 0.625);
    assert_eq!(next.layer(1).unwrap().name(), old.name());
    assert_eq!(
        next.layer(1)
            .unwrap()
            .mask()
            .unwrap()
            .plane()
            .samples()
            .as_ptr(),
        old.mask().unwrap().plane().samples().as_ptr()
    );
    assert!(Arc::ptr_eq(next.profile(), grouped.profile()));
    assert_eq!(next.source_info(1).unwrap()["master_depth"], 8);
    assert_eq!(next.source_info(1).unwrap()["source_id"], "family-a");
    assert_eq!(next.source_info(1).unwrap()["revision_id"], "version-b");
}
#[test]
fn cross_depth_profile_changes_always_return_to_original_profile_and_codes() {
    let source16 = typed(32, 24, 7);
    let destination8 = byte(32, 24, 0);
    let placed8 = edit8(&destination8, None, Some(&source16), place(false, true));
    let small = edit8(
        &placed8,
        None,
        None,
        transform([0.25, 0., 0., 0.25, 0., 0.]),
    );
    let converted = small
        .convert_to_profile(&IccProfile::display_p3())
        .unwrap()
        .unwrap();
    let converted = converted
        .convert_to_profile(&IccProfile::srgb())
        .unwrap()
        .unwrap();
    let restored = edit8(&converted, None, None, transform([4., 0., 0., 4., 0., 0.]));
    assert_eq!(
        *restored.layers[1].pixels.contiguous_image(),
        *placed8.layers[1].pixels.contiguous_image()
    );
    assert_eq!(
        restored.layers[1]
            .effective_source(&restored.profile)
            .unwrap()
            .asset
            .rgba16()
            .unwrap()
            .to_rgba16()
            .ok(),
        source16.layer_source(0).unwrap().to_rgba16().ok()
    );
    let source8 = byte(32, 24, 7);
    let destination16 = typed(32, 24, 0);
    let placed16 = edit16(&destination16, Some(&source8), None, place(false, true));
    let small = edit16(
        &placed16,
        None,
        None,
        transform([0.25, 0., 0., 0.25, 0., 0.]),
    );
    let converted = small
        .convert_to_profile(&IccProfile::display_p3(), LIMIT, 0, None)
        .unwrap()
        .unwrap();
    let converted = converted
        .convert_to_profile(&IccProfile::srgb(), LIMIT, 0, None)
        .unwrap()
        .unwrap();
    let restored = edit16(&converted, None, None, transform([4., 0., 0., 4., 0., 0.]));
    assert_eq!(
        restored.layer_source(1).unwrap().to_rgba16().ok(),
        placed16.layer_source(1).unwrap().to_rgba16().ok()
    );
    assert!(restored
        .layer(1)
        .unwrap()
        .effective_source()
        .unwrap()
        .asset
        .rgba8()
        .unwrap()
        .same_bytes(&source8.layers[0].pixels));
}
#[test]
fn mixed_inventory_deduplicates_the_genuine_original_across_handle_families() {
    let source = typed(64, 48, 1);
    let base = byte(64, 48, 3);
    let placed = edit8(&base, None, Some(&source), place(false, true));
    let bytes = [&placed as *const DrDocument];
    let typed = [&source as *const PrecisionDocument];
    let mut err = ptr::null_mut();
    let byte_report =
        resource_report::report(&bytes, &[], &[], &[], 0, LIMIT).expect("a byte report");
    let typed_report = json_result(
        unsafe { dr_precision_resources_report(typed.as_ptr(), 1, ptr::null(), 0, &mut err) },
        err,
    );
    let union =
        resource_report::report(&bytes, &[], &typed, &[], 0, LIMIT).expect("a mixed report");
    let separate = byte_report["unique_retained_bytes"].as_u64().unwrap()
        + typed_report["unique_retained_bytes"].as_u64().unwrap();
    let actual = union["unique_retained_bytes"].as_u64().unwrap();
    assert!(separate - actual >= 64 * 48 * 8);
    assert_eq!(union["precision_document_handle_count"], 1);
    let duplicated = [
        &source as *const PrecisionDocument,
        &source as *const PrecisionDocument,
    ];
    let again =
        resource_report::report(&bytes, &[], &duplicated, &[], 0, LIMIT).expect("a mixed report");
    assert_eq!(
        again["unique_retained_bytes"],
        union["unique_retained_bytes"]
    );
}
#[test]
fn replacement_locks_and_admission_publish_nothing() {
    let input = byte(16, 16, 1);
    let placed = edit8(&input, Some(&input), None, place(true, false));
    let replacement = byte(32, 24, 3);
    let request = parsed(json!({"op":"replace","layer":1,"revision_id":"version-b"}));
    for flags in [1, 2, 4, 7] {
        let locked = placed.with_layer_locks(1, flags).unwrap();
        assert!(locked
            .source_plan_input(
                Some(SourceInput::Byte(&replacement)),
                &request,
                24_000_000_000
            )
            .is_err());
        assert_eq!(locked.source_info(1).unwrap()["revision_id"], "version-a");
    }
    let plan = placed
        .source_plan_input(
            Some(SourceInput::Byte(&replacement)),
            &request,
            24_000_000_000,
        )
        .unwrap();
    let token = DrJobControl::default();
    assert!(placed
        .source_edit_input(
            Some(SourceInput::Byte(&replacement)),
            &request,
            plan.retained_bytes + plan.scratch_bytes - 1,
            0,
            Some(&token)
        )
        .is_err());
    assert_eq!(token.scratch_bytes(), 0);
    assert!(placed.layers[1]
        .effective_source(&placed.profile)
        .unwrap()
        .link
        .is_some());
}
#[test]
fn cancellation_during_revision_render_releases_scratch_and_keeps_original_revision() {
    let fixture = || {
        let first = typed(200, 160, 1);
        let placed = edit16(&first, None, Some(&first), place(false, false));
        let transformed = edit16(
            &placed,
            None,
            None,
            transform([1.07, 0.03, -0.02, 1.03, 0.3, 0.4]),
        );
        (transformed, typed(1600, 1000, 11))
    };
    let replace = |doc: &PrecisionDocument, input: &PrecisionDocument, token: usize| {
        let control = unsafe { &*(token as *const DrJobControl) };
        match doc.source_edit_input(
            Some(SourceInput::Typed(input)),
            &parsed(json!({"op":"replace","layer":1,"revision_id":"version-b"})),
            LIMIT,
            0,
            Some(control),
        ) {
            Ok(_) => (true, String::new()),
            Err(error) => (false, error),
        }
    };
    let (twin_doc, twin_input) = fixture();
    let (entry, _) = entry_then_cancel(&|_, stage, _| stage == 1, |token| {
        replace(&twin_doc, &twin_input, token)
    });
    assert_eq!(entry.checkpoints, 1, "the entry is begin()'s report");
    // begin() is checkpoint 1, the edit's own `job.check()` 2, and
    // `job.reserve(plan.scratch_bytes)` (`source_edit_input_with_context`)
    // checks at 3 BEFORE it stores: 4 is the first checkpoint after the store.
    let park = 4;
    let (transformed, replacement) = fixture();
    let parked = cancelled_parked_at(park, &|_, stage| stage == entry.stage, |token| {
        replace(&transformed, &replacement, token)
    });
    println!("revision render: {}", parked_line(&parked));
    assert!(
        parked.scratch > 0,
        "the revision render's reservation is live while it works"
    );
    assert_eq!(
        transformed.source_info(1).unwrap()["revision_id"],
        "version-a"
    );
}
#[test]
fn strict_link_schema_and_new_exports_null_sweep() {
    let base = byte(4, 4, 1);
    let high = typed(4, 4, 1);
    let req = CString::new(place(false, true).to_string()).unwrap();
    unsafe {
        assert!(dr_doc_mixed_source_plan(
            ptr::null(),
            ptr::null(),
            ptr::null(),
            req.as_ptr(),
            ptr::null_mut()
        )
        .is_null());
        assert!(dr_precision_doc_mixed_source_plan(
            ptr::null(),
            ptr::null(),
            ptr::null(),
            req.as_ptr(),
            ptr::null_mut()
        )
        .is_null());
        assert!(dr_doc_mixed_source_edit(
            ptr::null(),
            ptr::null(),
            ptr::null(),
            req.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            ptr::null_mut()
        )
        .is_null());
        assert!(dr_precision_doc_mixed_source_edit(
            ptr::null(),
            ptr::null(),
            ptr::null(),
            req.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            ptr::null_mut()
        )
        .is_null());
        assert!(
            dr_doc_mixed_source_plan(&base, &base, &high, req.as_ptr(), ptr::null_mut()).is_null()
        );
        let empty = dr_mixed_resources_report_with_raw_masks(
            ptr::null(),
            0,
            ptr::null(),
            0,
            ptr::null(),
            0,
            ptr::null(),
            0,
            ptr::null(),
            0,
            0,
            LIMIT,
            ptr::null_mut(),
        );
        assert!(!empty.is_null());
        dr_string_free(empty);
        assert!(dr_mixed_resources_report_with_raw_masks(
            ptr::null(),
            0,
            ptr::null(),
            0,
            ptr::null(),
            1,
            ptr::null(),
            0,
            ptr::null(),
            0,
            0,
            LIMIT,
            ptr::null_mut()
        )
        .is_null());
        assert!(dr_mixed_resources_report_with_raw_masks(
            ptr::null(),
            65537,
            ptr::null(),
            0,
            ptr::null(),
            0,
            ptr::null(),
            0,
            ptr::null(),
            0,
            0,
            LIMIT,
            ptr::null_mut()
        )
        .is_null());
    }
    for link in [
        json!({"path":"relative.png","sha256":"a".repeat(64)}),
        json!({"path":"/valid.png","sha256":"A".repeat(64)}),
        json!({"path":"/valid.png","sha256":"a".repeat(63)}),
        json!({"path":"/valid.png","sha256":"a".repeat(64),"unknown":true}),
    ] {
        assert!(SourceEdit::parse(
            &json!({"op":"place","source_id":"s","name":"n","x":0,"y":0,"link":link}).to_string()
        )
        .is_err());
    }
}
