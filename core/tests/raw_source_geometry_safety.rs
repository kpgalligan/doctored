use crate::common;
use crate::raw_geometry_fixture as fixture;
use common::probe::{cancelled_parked_at, entry_then_cancel, parked_line};
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_raw_geometry::*;
use doctored_core::job_control::DrJobControl;
use fixture::{image, mask, LIMIT};
use std::ffi::{c_char, CString};
use std::ptr;

#[test]
fn strict_schema_bounds_budget_and_identity_owner() {
    let mask = mask(7, 5);
    let image = image(7, 5);
    let original_mask = mask.encoded().to_vec();
    let original_pixels = image.layer_source(0).unwrap().to_rgba16().unwrap();
    for request in [
        "{}",
        r#"{"version":2,"kind":"exif","orientation":1}"#,
        r#"{"version":1,"kind":"exif","orientation":9}"#,
        r#"{"version":1,"kind":"exif","orientation":1,"unknown":1}"#,
        r#"{"version":1,"kind":"affine","width":7,"height":5,"inverse":[1,0,0,0,0,0],"filter":"bilinear"}"#,
        r#"{"version":1,"kind":"affine","width":7,"height":5,"inverse":[1,0,0,1,1e100,0],"filter":"bilinear"}"#,
        r#"{"version":1,"kind":"affine","width":0,"height":5,"inverse":[1,0,0,1,0,0],"filter":"bilinear"}"#,
        r#"{"version":1,"kind":"lens","profile_id":"x","center":[0.5,0.5],"radial":[1,0,0],"red_scale":1,"blue_scale":1,"filter":"bilinear"}"#,
        r#"{"version":1,"kind":"lens","profile_id":"x","center":[0.5,0.5],"radial":[0,0,0],"red_scale":1,"blue_scale":1,"filter":"bilinear","reference_size":[7,5]}"#,
    ] {
        assert!(
            mask.remap_plan(request, 1, LIMIT, 0).is_err(),
            "accepted bad map {request}"
        );
        assert!(image.raw_source_geometry_plan(request, LIMIT, 0).is_err());
    }
    assert!(mask
        .remap_plan(
            r#"{"version":1,"kind":"exif","orientation":1}"#,
            0,
            LIMIT,
            0
        )
        .is_err());
    assert!(mask.remap_plan(&" ".repeat(4097), 1, LIMIT, 0).is_err());
    let identity = r#"{"version":1,"kind":"exif","orientation":1}"#;
    let plan = mask.remap_plan(identity, 1, LIMIT, 0).unwrap();
    assert_eq!(plan["unchanged"], true);
    assert_eq!(plan["exact"], true);
    assert!(mask.remap(identity, 1, LIMIT, 0, None).unwrap().is_none());
    assert!(image
        .raw_source_geometry(identity, LIMIT, 0, None)
        .unwrap()
        .is_none());
    let tiny_limit = mask.retained_bytes() + plan["scratch_bytes"].as_u64().unwrap() - 1;
    assert!(mask.remap(identity, 1, tiny_limit, 0, None).is_err());
    assert_eq!(mask.encoded(), original_mask);
    assert_eq!(
        image.layer_source(0).unwrap().to_rgba16().ok().unwrap(),
        original_pixels
    );
}

#[test]
fn new_exports_null_safe_and_owned_error_strings() {
    let request = CString::new(r#"{"version":1,"kind":"exif","orientation":1}"#).unwrap();
    let mut error: *mut c_char = ptr::null_mut();
    unsafe {
        assert!(dr_raw_local_mask_remap_plan(
            ptr::null(),
            request.as_ptr(),
            1,
            LIMIT,
            0,
            &mut error
        )
        .is_null());
        assert!(!error.is_null());
        dr_string_free(error);
        error = ptr::null_mut();
        assert!(dr_raw_local_mask_remap(
            ptr::null(),
            request.as_ptr(),
            1,
            LIMIT,
            0,
            ptr::null(),
            &mut error
        )
        .is_null());
        assert!(!error.is_null());
        dr_string_free(error);
        error = ptr::null_mut();
        assert!(
            dr_raw_source_geometry_plan(ptr::null(), request.as_ptr(), LIMIT, 0, &mut error)
                .is_null()
        );
        assert!(!error.is_null());
        dr_string_free(error);
        error = ptr::null_mut();
        assert!(dr_raw_source_geometry(
            ptr::null(),
            request.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut error
        )
        .is_null());
        assert!(!error.is_null());
        dr_string_free(error);
        error = ptr::null_mut();
        assert!(
            dr_raw_local_mask_resize(ptr::null(), 4, 4, LIMIT, 0, ptr::null(), &mut error)
                .is_null()
        );
        assert!(!error.is_null());
        dr_string_free(error);
    }
}

#[test]
fn mid_operation_cancel_drains_scratch_and_keeps_input() {
    let mask = mask(3000, 2500);
    let original = mask.encoded().to_vec();
    let request = r#"{"version":1,"kind":"affine","width":3000,"height":2500,"inverse":[1,0,0,1,0.31,-0.27],"filter":"bicubic"}"#;
    let render = |token: usize| {
        let control = unsafe { &*(token as *const DrJobControl) };
        match mask.remap(request, 1, LIMIT, 0, Some(control)) {
            Ok(_) => (true, String::new()),
            Err(error) => (false, error),
        }
    };
    let (entry, next) = entry_then_cancel(&|progress, _, _| progress > 0.0, render);
    let n0 = entry.checkpoints;
    let k = 1;
    assert!(
        next.expect("the entry's kernel never reported again")
            .checkpoints
            > n0 + 1 + k,
        "the entry's kernel polls fewer than {} checkpoints before its next report (entry {n0}, next {next:?})",
        k + 1
    );
    let parked = cancelled_parked_at(
        n0 + 1 + k,
        &|progress, stage| stage == entry.stage && progress == entry.progress,
        render,
    );
    assert!(
        parked.progress > 0.0 && parked.progress < 1.0 && parked.scratch > 0,
        "cancel witness progress={} scratch={}",
        parked.progress,
        parked.scratch
    );
    assert!(!original.is_empty());
    println!("raw-map-cancel: {}", parked_line(&parked));
}
