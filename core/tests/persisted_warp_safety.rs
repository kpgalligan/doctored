use crate::warp_fixture as fixture;

use doctored_core::doc::LayerRaster;
use doctored_core::job_control::DrJobControl;
use doctored_core::precision_image::PrecisionImage;
use doctored_core::precision_paint::PaintEdit;
use doctored_core::retained_source::SourceEdit;
use doctored_core::warp::WarpEdit;
use fixture::{byte_document, edit, field, typed_document, LIMIT};
use serde_json::{json, Value};
use std::ffi::CStr;
use std::ptr;

use crate::common;
use crate::resource_report;
use common::probe::{cancelled_parked_at, entry_then_cancel, parked_line};

fn constant(columns: u32, rows: u32, x: f64, y: f64) -> Value {
    json!({"op":"set","layer":0,"target":"pixels","columns":columns,"rows":rows,
        "displacements":vec![[x,y];(columns*rows) as usize],"filter":"bicubic"})
}

#[test]
fn canonical_noops_need_no_working_budget_but_keep_semantic_refusals() {
    let identity = edit(constant(2, 2, 0.0, -0.0));
    let clear = edit(json!({"op":"clear","layer":0}));
    let bake = edit(json!({"op":"bake","layer":0}));
    let same = edit(field(3, 3, "bicubic"));
    let byte = byte_document();
    let typed = typed_document();
    for plan in [
        byte.warp_plan(&identity).unwrap(),
        byte.warp_plan(&clear).unwrap(),
        byte.warp_plan(&bake).unwrap(),
        typed.warp_plan(&identity).unwrap(),
        typed.warp_plan(&clear).unwrap(),
        typed.warp_plan(&bake).unwrap(),
    ] {
        assert_eq!(
            (plan.retained_bytes, plan.scratch_bytes, plan.work_units),
            (0, 0, 0)
        );
    }
    for request in [&identity, &clear, &bake] {
        assert!(byte
            .warp_edit(request, 1, u64::MAX, None)
            .unwrap()
            .is_none());
        assert!(typed
            .warp_edit(request, 1, u64::MAX, None)
            .unwrap()
            .is_none());
    }
    let byte_set = byte.warp_edit(&same, LIMIT, 0, None).unwrap().unwrap();
    let typed_set = typed.warp_edit(&same, LIMIT, 0, None).unwrap().unwrap();
    for plan in [
        byte_set.warp_plan(&same).unwrap(),
        typed_set.warp_plan(&same).unwrap(),
    ] {
        assert_eq!(
            (plan.retained_bytes, plan.scratch_bytes, plan.work_units),
            (0, 0, 0)
        );
    }
    assert!(byte_set
        .warp_edit(&same, 1, u64::MAX, None)
        .unwrap()
        .is_none());
    assert!(typed_set
        .warp_edit(&same, 1, u64::MAX, None)
        .unwrap()
        .is_none());

    // Identity after a live field clears the binding; it is not a no-op.
    assert!(byte_set.warp_plan(&identity).unwrap().scratch_bytes > 0);
    assert!(typed_set.warp_plan(&identity).unwrap().scratch_bytes > 0);
    assert!(byte_set.warp_edit(&identity, 1, 0, None).is_err());
    assert!(typed_set.warp_edit(&identity, 1, 0, None).is_err());
    let locked = byte
        .with_layer_locks(0, doctored_core::doc_lock::LOCK_POSITION)
        .unwrap();
    assert!(locked
        .warp_edit(&identity, 1, 0, None)
        .err()
        .unwrap()
        .contains("lock"));
    let locked = typed
        .with_layer_locks(0, doctored_core::doc_lock::LOCK_POSITION)
        .unwrap()
        .unwrap();
    assert!(locked
        .warp_edit(&identity, 1, 0, None)
        .err()
        .unwrap()
        .contains("lock"));
    let cancelled = DrJobControl::default();
    cancelled.cancel();
    assert!(byte.warp_edit(&identity, 1, 0, Some(&cancelled)).is_err());
    assert!(typed.warp_edit(&identity, 1, 0, Some(&cancelled)).is_err());
}

#[test]
fn exact_identity_translation_reedit_clear_and_bake_keep_original_allocations() {
    for size in [2, 16] {
        let original = byte_document();
        let original_pixels = original.layers[0].pixels.clone();
        assert!(original
            .warp_edit(&edit(constant(size, size, 0.0, -0.0)), LIMIT, 0, None)
            .unwrap()
            .is_none());
        let translated = original
            .warp_edit(&edit(constant(size, size, 7.0, -4.0)), LIMIT, 0, None)
            .unwrap()
            .unwrap();
        assert_eq!(translated.layers[0].offset, (4, -2));
        assert!(LayerRaster::ptr_eq(
            &translated.layers[0].pixels,
            &original_pixels
        ));
        assert!(translated
            .warp_edit(&edit(constant(size, size, 7.0, -4.0)), LIMIT, 0, None)
            .unwrap()
            .is_none());
        let cleared = translated
            .warp_edit(&edit(json!({"op":"clear","layer":0})), LIMIT, 0, None)
            .unwrap()
            .unwrap();
        assert_eq!(cleared.layers[0].offset, original.layers[0].offset);
        assert!(LayerRaster::ptr_eq(
            &cleared.layers[0].pixels,
            &original_pixels
        ));
        assert!(cleared
            .warp_edit(&edit(json!({"op":"clear","layer":0})), LIMIT, 0, None)
            .unwrap()
            .is_none());
        let baked = translated
            .warp_edit(&edit(json!({"op":"bake","layer":0})), LIMIT, 0, None)
            .unwrap()
            .unwrap();
        assert_eq!(baked.layers[0].offset, translated.layers[0].offset);
        assert!(LayerRaster::ptr_eq(
            &baked.layers[0].pixels,
            &original_pixels
        ));
        assert!(baked
            .warp_edit(&edit(json!({"op":"bake","layer":0})), LIMIT, 0, None)
            .unwrap()
            .is_none());

        let original = typed_document();
        let ptr = original.layer_source(0).unwrap().clone();
        assert!(original
            .warp_edit(&edit(constant(size, size, 0.0, -0.0)), LIMIT, 0, None)
            .unwrap()
            .is_none());
        let translated = original
            .warp_edit(&edit(constant(size, size, -5.0, 3.0)), LIMIT, 0, None)
            .unwrap()
            .unwrap();
        assert_eq!(translated.layer(0).unwrap().offset(), (-8, 5));
        assert!(PrecisionImage::ptr_eq(
            translated.layer_source(0).unwrap(),
            &ptr
        ));
        let cleared = translated
            .warp_edit(&edit(json!({"op":"clear","layer":0})), LIMIT, 0, None)
            .unwrap()
            .unwrap();
        assert_eq!(
            cleared.layer(0).unwrap().offset(),
            original.layer(0).unwrap().offset()
        );
        assert!(PrecisionImage::ptr_eq(
            cleared.layer_source(0).unwrap(),
            &ptr
        ));
    }
}

#[test]
fn reedit_uses_original_instead_of_prior_filtered_output_and_stale_binding_cannot_resurrect() {
    let base = byte_document();
    let a = base
        .warp_edit(&edit(field(3, 3, "bilinear")), LIMIT, 0, None)
        .unwrap()
        .unwrap();
    let request = edit(field(4, 2, "bicubic"));
    let from_a = a.warp_edit(&request, LIMIT, 0, None).unwrap().unwrap();
    let fresh = base.warp_edit(&request, LIMIT, 0, None).unwrap().unwrap();
    assert_eq!(
        from_a.layers[0].pixels.contiguous_image().as_raw(),
        fresh.layers[0].pixels.contiguous_image().as_raw()
    );
    assert_eq!(from_a.layers[0].offset, fresh.layers[0].offset);
    let replacement =
        image::RgbaImage::new(a.layers[0].pixels.width(), a.layers[0].pixels.height());
    let destructive = a.with_layer_pixels(0, replacement).unwrap();
    assert!(destructive.warp_info(0).unwrap()["field"].is_null());
    assert!(destructive
        .warp_edit(&edit(json!({"op":"clear","layer":0})), LIMIT, 0, None)
        .unwrap()
        .is_none());

    let base = typed_document();
    let a = base
        .warp_edit(&edit(field(3, 3, "bilinear")), LIMIT, 0, None)
        .unwrap()
        .unwrap();
    let from_a = a.warp_edit(&request, LIMIT, 0, None).unwrap().unwrap();
    let fresh = base.warp_edit(&request, LIMIT, 0, None).unwrap().unwrap();
    assert_eq!(
        from_a.layer_source(0).unwrap().to_rgba16().ok(),
        fresh.layer_source(0).unwrap().to_rgba16().ok()
    );
    let erase = PaintEdit::parse(r#"{"op":"erase","layer":0,"opacity":1.0}"#).unwrap();
    let painted = a
        .paint(
            &erase,
            &vec![65535; (fixture::W * fixture::H) as usize],
            LIMIT,
            0,
            None,
        )
        .unwrap()
        .unwrap();
    assert!(painted.warp_info(0).unwrap()["field"].is_null());
    assert!(painted
        .warp_edit(&edit(json!({"op":"clear","layer":0})), LIMIT, 0, None)
        .unwrap()
        .is_none());
}

#[test]
fn duplicate_and_reorder_keep_bindings_on_their_layer() {
    let request = edit(field(3, 3, "bilinear"));
    let byte = byte_document()
        .warp_edit(&request, LIMIT, 0, None)
        .unwrap()
        .unwrap();
    let duplicate = byte.duplicate_layers(&[0]).unwrap();
    assert_eq!(duplicate.layers.len(), 2);
    assert!(duplicate.warp_info(0).unwrap()["field"].is_object());
    assert!(duplicate.warp_info(1).unwrap()["field"].is_object());
    let moved = duplicate.move_layer_to(0, 1, 0).unwrap();
    assert!(moved.warp_info(0).unwrap()["field"].is_object());
    assert!(moved.warp_info(1).unwrap()["field"].is_object());

    let typed = typed_document()
        .warp_edit(&request, LIMIT, 0, None)
        .unwrap()
        .unwrap();
    let duplicate = typed.duplicate_layer(0).unwrap().unwrap();
    assert_eq!(duplicate.layer_count(), 2);
    assert!(duplicate.warp_info(0).unwrap()["field"].is_object());
    assert!(duplicate.warp_info(1).unwrap()["field"].is_object());
    let moved = duplicate.move_layer_to(0, 1, 0).unwrap().unwrap();
    assert!(moved.warp_info(0).unwrap()["field"].is_object());
    assert!(moved.warp_info(1).unwrap()["field"].is_object());
}

#[test]
fn strict_request_contraction_limit_and_job_cancellation_refuse_without_mutation() {
    for bad in [
        json!({"op":"set","layer":0,"target":"pixels","columns":2,"rows":2,
            "displacements":[[0.0,0.0]],"filter":"bicubic"}),
        json!({"op":"clear","layer":0,"extra":true}),
        json!({"op":"set","layer":0,"target":"pixels","columns":17,"rows":2,
            "displacements":[],"filter":"bicubic"}),
    ] {
        assert!(WarpEdit::parse(&bad.to_string()).is_err());
    }
    for raw in [
        "{\"op\":\"clear\",\"layer\":true}",
        "{\"op\":\"set\",\"layer\":0,\"target\":\"pixels\",\"columns\":2,\"rows\":2,\"displacements\":[[NaN,0],[0,0],[0,0],[0,0]],\"filter\":\"bilinear\"}",
        "{\"op\":\"clear\",\"layer\":0,\"unexpected\":0}",
    ] {
        assert!(WarpEdit::parse(raw).is_err(), "{raw}");
    }
    assert!(WarpEdit::parse(&format!(
        "{{\"op\":\"clear\",\"layer\":0,\"pad\":\"{}\"}}",
        "x".repeat(65536)
    ))
    .err()
    .unwrap()
    .contains("64 KiB"));
    for (columns, rows, displacement, accepted) in [
        (2, 2, 30_000.0, true),
        (16, 16, -30_000.0, true),
        (1, 2, 0.0, false),
        (17, 2, 0.0, false),
        (2, 1, 0.0, false),
        (2, 17, 0.0, false),
        (2, 2, 30_000.000_1, false),
    ] {
        let request = constant(columns, rows, displacement, 0.0).to_string();
        assert_eq!(WarpEdit::parse(&request).is_ok(), accepted, "{request}");
    }
    let folded = edit(
        json!({"op":"set","layer":0,"target":"pixels","columns":2,"rows":2,
        "displacements":[[0.0,0.0],[10.0,0.0],[0.0,0.0],[10.0,0.0]],"filter":"bicubic"}),
    );
    let doc = byte_document();
    assert!(doc
        .warp_plan(&folded)
        .err()
        .unwrap()
        .contains("contraction"));
    let typed = typed_document();
    assert!(typed
        .warp_plan(&folded)
        .err()
        .unwrap()
        .contains("contraction"));
    // A positive Jacobian can still exceed the deliberately stricter 0.5
    // contraction admission; this is not only a folded-field test.
    let over_bound = edit(json!({"op":"set","layer":0,"target":"pixels",
        "columns":2,"rows":2,"displacements":[[0,0],[9,0],[0,0],[9,0]],
        "filter":"bilinear"}));
    assert!(doc
        .warp_plan(&over_bound)
        .err()
        .unwrap()
        .contains("contraction"));
    assert!(typed
        .warp_plan(&over_bound)
        .err()
        .unwrap()
        .contains("contraction"));
    let control = DrJobControl::default();
    control.cancel();
    let request = edit(field(3, 3, "bicubic"));
    assert!(doc.warp_edit(&request, LIMIT, 0, Some(&control)).is_err());
    assert!(doc.warp_info(0).unwrap()["field"].is_null());
    assert!(typed.warp_edit(&request, LIMIT, 0, Some(&control)).is_err());
    assert!(typed.warp_info(0).unwrap()["field"].is_null());
}

#[test]
fn missing_mask_target_switch_locks_and_working_budget_refuse() {
    let doc = byte_document();
    let mut mask_request = field(3, 3, "bilinear");
    mask_request["target"] = json!("mask");
    assert!(doc
        .warp_plan(&edit(mask_request.clone()))
        .err()
        .unwrap()
        .contains("mask"));
    let locked = doc
        .with_layer_locks(0, doctored_core::doc_lock::LOCK_POSITION)
        .unwrap();
    assert!(locked
        .warp_plan(&edit(field(3, 3, "bicubic")))
        .err()
        .unwrap()
        .contains("lock"));
    let mut described = doc.clone();
    described.layers[0].meta = Some("editable artwork".into());
    assert!(described
        .warp_plan(&edit(field(3, 3, "bicubic")))
        .err()
        .unwrap()
        .contains("rasterize"));
    let request = edit(field(3, 3, "bicubic"));
    let plan = doc.warp_plan(&request).unwrap();
    let below = plan.retained_bytes + plan.scratch_bytes - 1;
    assert!(doc
        .warp_edit(&request, below, 0, None)
        .err()
        .unwrap()
        .contains("working limit"));
    assert!(doc.warp_info(0).unwrap()["field"].is_null());
    let set = doc.warp_edit(&request, LIMIT, 0, None).unwrap().unwrap();
    assert!(set
        .warp_plan(&edit(mask_request))
        .err()
        .unwrap()
        .contains("clear or bake"));

    let typed = typed_document();
    let locked = typed
        .with_layer_locks(0, doctored_core::doc_lock::LOCK_POSITION)
        .unwrap()
        .unwrap();
    assert!(locked.warp_plan(&request).err().unwrap().contains("lock"));
    let plan = typed.warp_plan(&request).unwrap();
    let below = plan.retained_bytes + plan.scratch_bytes - 1;
    assert!(typed
        .warp_edit(&request, below, 0, None)
        .err()
        .unwrap()
        .contains("working limit"));
    let place = SourceEdit::parse(
        &json!({"op":"place","source_id":"warp-master",
        "revision_id":"first","name":"Master","x":0,"y":0})
        .to_string(),
    )
    .unwrap();
    let sourced = typed
        .source_edit(Some(&typed_document()), &place, LIMIT, 0, None)
        .unwrap()
        .unwrap();
    let mut sourced_request = field(3, 3, "bicubic");
    sourced_request["layer"] = json!(1);
    assert!(sourced
        .warp_plan(&edit(sourced_request))
        .err()
        .unwrap()
        .contains("rasterize"));
}

#[test]
fn mixed_resource_inventory_counts_shared_warp_originals_once() {
    let doc = byte_document();
    let request = edit(field(3, 3, "bilinear"));
    let set = doc.warp_edit(&request, LIMIT, 0, None).unwrap().unwrap();
    let clone = set.clone();
    let typed = typed_document()
        .warp_edit(&request, LIMIT, 0, None)
        .unwrap()
        .unwrap();
    fn report(
        byte: &[&doctored_core::doc::DrDocument],
        typed: &[&doctored_core::precision_document::PrecisionDocument],
    ) -> Value {
        let bytes: Vec<_> = byte.iter().map(|d| *d as *const _).collect();
        let typed: Vec<_> = typed.iter().map(|d| *d as *const _).collect();
        resource_report::report(&bytes, &[], &typed, &[], 0, LIMIT).expect("a resource report")
    }
    let one = report(&[&set], &[&typed]);
    let shared = report(&[&set, &clone], &[&typed]);
    assert_eq!(
        one["categories"]["warp_lattice"]["unique_bytes"],
        shared["categories"]["warp_lattice"]["unique_bytes"]
    );
    assert_eq!(
        one["categories"]["warp_pixels"]["unique_bytes"],
        shared["categories"]["warp_pixels"]["unique_bytes"]
    );
    assert!(
        shared["referenced_bytes"].as_u64().unwrap() > one["referenced_bytes"].as_u64().unwrap()
    );
    assert!(
        shared["categories"]["warp_lattice"]["unique_bytes"]
            .as_u64()
            .unwrap()
            > 0
    );
}

#[test]
fn active_job_cancellation_drains_its_own_scratch_at_both_depths() {
    let width = 800;
    let height = 800;
    let request = edit(field(3, 3, "bicubic"));
    let byte_doc =
        || {
            let mut byte = doctored_core::doc::DrDocument::from_pixels(
                image::RgbaImage::from_pixel(width, height, image::Rgba([173, 71, 39, 127])),
            );
            byte.layers[0].offset = (-3, 2);
            byte
        };
    let byte_warp = |byte: &doctored_core::doc::DrDocument, token: usize| {
        let control = unsafe { &*(token as *const DrJobControl) };
        match byte.warp_edit(&request, LIMIT, 0, Some(control)) {
            Ok(_) => (true, String::new()),
            Err(error) => (false, error),
        }
    };
    let twin = byte_doc();
    let (entry, _) = entry_then_cancel(&|_, stage, _| stage == 1, |token| byte_warp(&twin, token));
    assert_eq!(entry.checkpoints, 1, "the entry is begin()'s report");
    // begin() is checkpoint 1, `warp_edit`'s own `job.check()` 2, and
    // `job.reserve(plan.scratch_bytes)` (`doc_warp.rs` `warp_edit`) checks
    // at 3 BEFORE it stores: 4 is the first checkpoint after the store.
    let byte = byte_doc();
    let parked = cancelled_parked_at(4, &|_, stage| stage == entry.stage, |token| {
        byte_warp(&byte, token)
    });
    println!("byte warp: {}", parked_line(&parked));
    assert!(
        parked.scratch > 0,
        "byte warp never registered active scratch"
    );

    let typed_doc = || {
        let profile = doctored_core::icc::IccProfile::srgb();
        let pixels = vec![[60001u16, 27183, 10101, 32769]; (width * height) as usize];
        let image = doctored_core::precision_image::PrecisionImage::from_rgba16(
            width,
            height,
            pixels,
            profile.clone(),
            None,
        )
        .unwrap();
        let layer = doctored_core::precision_document::PrecisionDocumentLayer::raster(
            "Large",
            image,
            (-3, 2),
            1.0,
            true,
        )
        .unwrap();
        doctored_core::precision_document::PrecisionDocument::from_parts(
            width,
            height,
            profile,
            doctored_core::metadata::Metadata::default(),
            doctored_core::metadata::Resolution::default(),
            vec![layer],
        )
        .unwrap()
    };
    let typed_warp = |typed: &doctored_core::precision_document::PrecisionDocument,
                      token: usize| {
        let control = unsafe { &*(token as *const DrJobControl) };
        match typed.warp_edit(&request, LIMIT, 0, Some(control)) {
            Ok(_) => (true, String::new()),
            Err(error) => (false, error),
        }
    };
    let twin = typed_doc();
    let (entry, _) = entry_then_cancel(&|_, stage, _| stage == 1, |token| typed_warp(&twin, token));
    assert_eq!(entry.checkpoints, 1, "the entry is begin()'s report");
    // begin() is checkpoint 1, `warp_edit`'s own `job.check()` 2, and
    // `job.reserve(plan.scratch_bytes)` (`precision_warp.rs` `warp_edit`)
    // checks at 3 BEFORE it stores: 4 is the first checkpoint after the store.
    let typed = typed_doc();
    let parked = cancelled_parked_at(4, &|_, stage| stage == entry.stage, |token| {
        typed_warp(&typed, token)
    });
    println!("u16 warp: {}", parked_line(&parked));
    assert!(
        parked.scratch > 0,
        "u16 warp never registered active scratch"
    );
}

/// The byte twin of `clean_typed_layers` (`precision_document.rs`): a profile
/// edit invalidates every warp binding (`effective_warp` compares the
/// profile by pointer), and the byte colour shims keep `doc_op`'s
/// `detach_invalid_sources` step, so neither Assign nor Convert hands back a
/// stale binding.
#[test]
fn byte_profile_edits_drop_stale_warp_bindings() {
    use doctored_core::ffi_color::{dr_doc_assign_profile, dr_doc_convert_to_profile};
    use doctored_core::ffi_doc::dr_doc_free;
    use doctored_core::icc::IccProfile;

    let warped = byte_document()
        .warp_edit(&edit(field(3, 3, "bilinear")), LIMIT, 0, None)
        .unwrap()
        .unwrap();
    assert!(warped.layers[0].warp.is_some());
    let p3 = IccProfile::display_p3();
    let bytes = p3.bytes();
    for op in [dr_doc_assign_profile, dr_doc_convert_to_profile] {
        let mut err = ptr::null_mut();
        let out = unsafe { op(&warped, bytes.as_ptr(), bytes.len(), &mut err) };
        let message = if err.is_null() {
            String::new()
        } else {
            let text = unsafe { CStr::from_ptr(err) }
                .to_string_lossy()
                .into_owned();
            unsafe { doctored_core::ffi::dr_string_free(err) };
            text
        };
        assert!(!out.is_null(), "{message}");
        assert!(
            unsafe { &*out }.layers[0].warp.is_none(),
            "the stale binding is detached"
        );
        unsafe { dr_doc_free(out) };
    }
}
