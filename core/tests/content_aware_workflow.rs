//! Independent exact-fill ABI contracts: analytic soft coverage, source geometry,
//! omitted-versus-all permission equivalence, immutable output placement and
//! active cancellation.
use doctored_core::{
    doc::DrDocument,
    ffi::dr_string_free,
    ffi_content_aware::*,
    ffi_jobs::dr_job_control_set_scratch_limit,
    ffi_precision_edit::{dr_precision_doc_edit, dr_precision_doc_edit_plan},
    icc::IccProfile,
    job_control::DrJobControl,
    metadata::{Metadata, Resolution},
    precision_document::PrecisionDocument,
    precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES},
};
use image::{Rgba, RgbaImage};
use serde_json::{json, Value};
use std::{
    ffi::{CStr, CString},
    ptr,
};

use crate::common;
use common::probe::{cancelled_parked_at, entry_count, parked_line};
/// `precision_inpaint::COMPONENT_BYTES`, restated (the module is private).
const COMPONENT_BYTES: u64 = 128;
fn request(x: u32, y: u32, w: u32, h: u32) -> Value {
    json!({"op":"content_aware_fill","layer":0,"x":x,"y":y,"width":w,"height":h,"seed":12345})
}
fn native(w: u32, h: u32, p: Vec<[u16; 4]>) -> PrecisionDocument {
    PrecisionDocument::from_image(
        PrecisionImage::from_rgba16(w, h, p, IccProfile::srgb(), None).unwrap(),
        Metadata::default(),
        Resolution::default(),
    )
    .unwrap()
}
unsafe fn result<T>(out: *mut T, err: *mut std::ffi::c_char) -> Result<Option<Box<T>>, String> {
    if !err.is_null() {
        let s = unsafe { CStr::from_ptr(err) }
            .to_string_lossy()
            .into_owned();
        unsafe { dr_string_free(err) };
        assert!(out.is_null());
        Err(s)
    } else {
        Ok((!out.is_null()).then(|| unsafe { Box::from_raw(out) }))
    }
}
fn byte(
    doc: &DrDocument,
    r: &Value,
    c: &[u8],
    job: Option<&DrJobControl>,
) -> Result<Option<Box<DrDocument>>, String> {
    let r = CString::new(r.to_string()).unwrap();
    let mut err = ptr::null_mut();
    unsafe {
        let out = dr_doc_content_aware_fill_edit(
            doc,
            r.as_ptr(),
            c.as_ptr(),
            job.map_or(ptr::null(), |j| j),
            &mut err,
        );
        result(out, err)
    }
}
fn typed(
    doc: &PrecisionDocument,
    r: &Value,
    c: &[u16],
    limit: u64,
    job: Option<&DrJobControl>,
) -> Result<Option<Box<PrecisionDocument>>, String> {
    let r = CString::new(r.to_string()).unwrap();
    let mut err = ptr::null_mut();
    unsafe {
        let out = dr_precision_doc_edit(
            doc,
            r.as_ptr(),
            c.as_ptr(),
            c.len(),
            limit,
            0,
            job.map_or(ptr::null(), |j| j),
            &mut err,
        );
        result(out, err)
    }
}
fn mask_all() -> Value {
    json!({"version":1,"initial":"all","operations":[]})
}
fn samples(d: &PrecisionDocument) -> Vec<[u16; 4]> {
    d.layer_source(0).unwrap().to_rgba16().unwrap()
}

#[test]
fn new_exports_null_safety_sweep() {
    unsafe {
        for report in [false, true] {
            let mut error = ptr::null_mut();
            let e = if report { &mut error } else { ptr::null_mut() };
            assert_eq!(
                dr_doc_content_aware_fill_scratch(ptr::null(), ptr::null(), e),
                0
            );
            if !error.is_null() {
                dr_string_free(error)
            }
            error = ptr::null_mut();
            let e = if report { &mut error } else { ptr::null_mut() };
            assert!(dr_doc_content_aware_fill_edit(
                ptr::null(),
                ptr::null(),
                ptr::null(),
                ptr::null(),
                e
            )
            .is_null());
            if !error.is_null() {
                dr_string_free(error)
            }
        }
    }
}
#[test]
fn byte_explicit_all_permission_matches_the_omitted_mask_exactly() {
    let (w, h) = (96, 96);
    let mut p = RgbaImage::from_fn(w, h, |x, y| {
        Rgba([30 + ((x / 2 % 2) * 30) as u8, 70 + (y % 5) as u8, 110, 255])
    });
    let mut c = vec![0; (w * h) as usize];
    for y in 40..56 {
        for x in 40..56 {
            p.put_pixel(x, y, Rgba([230, 211, 192, 255]));
            c[(y * w + x) as usize] = if x < 44 { 17 } else { 255 };
        }
    }
    let doc = DrDocument::from_pixels(p.clone());
    let omitted = byte(&doc, &request(40, 40, 16, 16), &c, None)
        .unwrap()
        .unwrap();
    let mut r = request(40, 40, 16, 16);
    r["source_mask"] = mask_all();
    let out = byte(&doc, &r, &c, None).unwrap().unwrap();
    assert_eq!(
        out.layers[0].pixels.contiguous_image().as_raw(),
        omitted.layers[0].pixels.contiguous_image().as_raw()
    );
    assert_eq!(doc.layers[0].pixels.contiguous_image().as_raw(), p.as_raw());
}
#[test]
fn native_soft_coverage_matches_f64_oracle_with_partial_alpha_and_subbyte_codes() {
    let (w, h) = (64, 64);
    let mut p = vec![[12001, 23002, 34003, 65535]; (w * h) as usize];
    let mut c = vec![0; p.len()];
    for y in 24..40 {
        for x in 24..40 {
            let i = (y * w + x) as usize;
            p[i] = [61001, 59003, 58007, [0, 1, 2, 129, 32769, 65535][i % 6]];
            c[i] = [1, 2, 128, 257, 32767, 32768, 65534, 65535][x as usize % 8];
        }
    }
    let doc = native(w, h, p.clone());
    let mut r = request(24, 24, 16, 16);
    r["source_mask"] = mask_all();
    let out = typed(&doc, &r, &c, REFERENCE_WORKING_BYTES, None)
        .unwrap()
        .unwrap();
    let reference = [12001, 23002, 34003];
    let mut tiny = false;
    for (i, a) in samples(&out).iter().enumerate() {
        assert_eq!(a[3], p[i][3]);
        if c[i] == 0 || p[i][3] == 0 {
            assert_eq!(*a, p[i]);
            continue;
        }
        for ch in 0..3 {
            let expected = (f64::from(p[i][ch])
                + (f64::from(reference[ch]) - f64::from(p[i][ch])) * f64::from(c[i]) / 65535.)
                .round() as u16;
            assert!(
                a[ch].abs_diff(expected) <= 1,
                "pixel {i} channel {ch}: actual {} expected {expected}",
                a[ch]
            );
        }
        tiny |= c[i] == 1 && *a != p[i];
    }
    assert!(
        tiny,
        "coverage1 must participate in synthesis and final write"
    );
    assert_eq!(samples(&doc), p);
    let reopened = PrecisionDocument::parse_native(
        &out.encode_native(REFERENCE_WORKING_BYTES, 0, None).unwrap(),
        REFERENCE_WORKING_BYTES,
        0,
        None,
    )
    .unwrap();
    assert_eq!(samples(&reopened), samples(&out));
}
#[test]
fn source_rectangles_and_ordered_capsules_control_complete_patch_permission() {
    let (w, h) = (64, 64);
    let mut p = RgbaImage::from_pixel(w, h, Rgba([31, 72, 113, 255]));
    let mut c = vec![0; (w * h) as usize];
    for y in 28..36 {
        for x in 28..36 {
            p.put_pixel(x, y, Rgba([231, 222, 213, 255]));
            c[(y * w + x) as usize] = 255;
        }
    }
    let doc = DrDocument::from_pixels(p);
    let mut r = request(28, 28, 8, 8);
    let rect = json!({"op":"include_rect","x":8,"y":8,"width":7,"height":7});
    r["source_mask"] = json!({"version":1,"initial":"none","operations":[rect]});
    let good = byte(&doc, &r, &c, None).unwrap().unwrap();
    let good_image = good.layers[0].pixels.contiguous_image();
    for (i, coverage) in c.iter().enumerate() {
        if *coverage > 0 {
            assert_eq!(&good_image.as_raw()[i * 4..i * 4 + 4], &[31, 72, 113, 255]);
        }
    }
    let exclude = json!({"op":"exclude_brush","radius":0.5,"points":[[11.5,11.5]]});
    r["source_mask"]["operations"]
        .as_array_mut()
        .unwrap()
        .push(exclude);
    assert!(byte(&doc, &r, &c, None).err().unwrap().contains("source"));
    r["source_mask"]["operations"]
        .as_array_mut()
        .unwrap()
        .push(json!({"op":"include_brush","radius":0.5,"points":[[11.5,11.5],[11.5,11.5]]}));
    let restored = byte(&doc, &r, &c, None).unwrap().unwrap();
    assert_eq!(
        *restored.layers[0].pixels.contiguous_image(),
        *good.layers[0].pixels.contiguous_image()
    );
    r["source_mask"]["operations"]
        .as_array_mut()
        .unwrap()
        .push(json!({"op":"exclude_rect","x":10.5,"y":8,"width":1,"height":7}));
    assert!(byte(&doc, &r, &c, None).is_err());
}
#[test]
fn opaque_new_output_matches_in_place_and_preserves_originals_at_both_depths() {
    let (w, h) = (48, 48);
    let mut p = vec![[12001, 23002, 34003, 65535]; (w * h) as usize];
    let mut c = vec![0; p.len()];
    for y in 18..30 {
        for x in 18..30 {
            let i = (y * w + x) as usize;
            p[i] = [61001, 59003, 58007, 65535];
            c[i] = [257, 32767, 65535][x as usize % 3];
        }
    }
    let doc = native(w, h, p.clone());
    let mut r = request(18, 18, 12, 12);
    let into = typed(&doc, &r, &c, REFERENCE_WORKING_BYTES, None)
        .unwrap()
        .unwrap();
    r["output"] = json!("new_layer");
    let above = typed(&doc, &r, &c, REFERENCE_WORKING_BYTES, None)
        .unwrap()
        .unwrap();
    assert_eq!(above.layer_count(), 2);
    assert_eq!(samples(&above), p);
    assert_eq!(
        above
            .render(REFERENCE_WORKING_BYTES, 0, None)
            .unwrap()
            .to_rgba16()
            .ok(),
        into.render(REFERENCE_WORKING_BYTES, 0, None)
            .unwrap()
            .to_rgba16()
            .ok()
    );
    let lifted = above.layer_source(1).unwrap().to_rgba16().ok().unwrap();
    for (i, p) in lifted.iter().enumerate() {
        if c[i] == 0 {
            assert_eq!(*p, [0; 4]);
        }
    }
    let reopened = PrecisionDocument::parse_native(
        &above
            .encode_native(REFERENCE_WORKING_BYTES, 0, None)
            .unwrap(),
        REFERENCE_WORKING_BYTES,
        0,
        None,
    )
    .unwrap();
    assert_eq!(reopened.layer_count(), 2);
    assert_eq!(samples(&reopened), p);
    let bp = RgbaImage::from_fn(w, h, |x, y| {
        Rgba(p[(y * w + x) as usize].map(|v| ((u32::from(v) + 128) / 257) as u8))
    });
    let bc: Vec<u8> = c
        .iter()
        .map(|v| ((u32::from(*v) + 128) / 257) as u8)
        .collect();
    let bdoc = DrDocument::from_pixels(bp.clone());
    r["output"] = json!("layer");
    let into = byte(&bdoc, &r, &bc, None).unwrap().unwrap();
    r["output"] = json!("new_layer");
    let above = byte(&bdoc, &r, &bc, None).unwrap().unwrap();
    assert_eq!(above.layers.len(), 2);
    assert_eq!(*above.layers[0].pixels.contiguous_image(), bp);
    assert_eq!(above.flattened(), into.flattened());
}
#[test]
fn request_bounds_source_geometry_caps_and_budget_refuse_atomically() {
    let (w, h) = (64, 64);
    let original = vec![[12001, 23002, 34003, 65535]; (w * h) as usize];
    let doc = native(w, h, original.clone());
    let bdoc = DrDocument::from_pixels(RgbaImage::from_pixel(w, h, Rgba([12, 34, 56, 255])));
    let mut c = vec![0; original.len()];
    c[30 * w as usize + 30] = 65535;
    let bc: Vec<u8> = c.iter().map(|v| if *v > 0 { 255 } else { 0 }).collect();
    let r = request(24, 24, 16, 16);
    for bad in [
        json!({"version":2,"initial":"all","operations":[]}),
        json!({"version":1,"initial":"all","extra":true,"operations":[]}),
        json!({"version":1,"initial":"all","operations":[{"op":"exclude_rect","x":0,"y":0,"width":0,"height":10}]}),
        json!({"version":1,"initial":"none","operations":[{"op":"include_brush","radius":20,"points":[]}]}),
        json!({"version":1,"initial":"all","operations":[{"op":"exclude_brush","radius":4097,"points":[[1,1]]}]}),
    ] {
        let mut bad_r = r.clone();
        bad_r["source_mask"] = bad;
        assert!(typed(&doc, &bad_r, &c, REFERENCE_WORKING_BYTES, None).is_err());
        assert!(byte(&bdoc, &bad_r, &bc, None).is_err());
    }
    let mut narrow = r.clone();
    narrow["x"] = json!(31);
    assert!(typed(&doc, &narrow, &c, REFERENCE_WORKING_BYTES, None).is_err());
    assert!(byte(&bdoc, &narrow, &bc, None).is_err());
    let mut scalar = r.clone();
    scalar["target"] = json!("mask");
    assert!(typed(&doc, &scalar, &c, REFERENCE_WORKING_BYTES, None).is_err());
    assert!(byte(&bdoc, &scalar, &bc, None).is_err());
    let text = CString::new(r.to_string()).unwrap();
    let mut err = ptr::null_mut();
    let scratch = unsafe { dr_doc_content_aware_fill_scratch(&bdoc, text.as_ptr(), &mut err) };
    assert!(scratch > 0 && err.is_null());
    // The edit re-admits by its coverage's one start, not the 16 × 16
    // box's 256 records.
    let admitted = scratch - 255 * COMPONENT_BYTES;
    let control = DrJobControl::default();
    assert!(unsafe { dr_job_control_set_scratch_limit(&control, admitted - 1) });
    assert!(byte(&bdoc, &r, &bc, Some(&control))
        .err()
        .unwrap()
        .contains("memory"));
    assert_eq!(control.scratch_bytes(), 0);
    let control = DrJobControl::default();
    assert!(unsafe { dr_job_control_set_scratch_limit(&control, admitted) });
    if let Err(message) = byte(&bdoc, &r, &bc, Some(&control)) {
        assert!(!message.contains("memory"), "{message}");
    }
    assert_eq!(control.scratch_bytes(), 0);
    let plan = unsafe {
        dr_precision_doc_edit_plan(&doc, text.as_ptr(), REFERENCE_WORKING_BYTES, 0, &mut err)
    };
    assert!(!plan.is_null() && err.is_null());
    let info: Value =
        serde_json::from_str(unsafe { CStr::from_ptr(plan) }.to_str().unwrap()).unwrap();
    unsafe { dr_string_free(plan) };
    assert!(typed(
        &doc,
        &r,
        &c,
        info["retained_bytes"].as_u64().unwrap() + info["scratch_bytes"].as_u64().unwrap() - 1,
        None
    )
    .err()
    .unwrap()
    .contains("working"));
    assert_eq!(samples(&doc), original);
}
#[test]
fn actual_synthesis_cancels_and_releases_both_depths() {
    let (w, h) = (512, 512);
    let mut p = vec![[12001, 23002, 34003, 65535]; (w * h) as usize];
    let mut c = vec![0; p.len()];
    for y in 120..390 {
        for x in 120..390 {
            let i = (y * w + x) as usize;
            p[i] = [61001, 59003, 58007, 65535];
            c[i] = 65535;
        }
    }
    let doc = native(w, h, p.clone());
    let bdoc = DrDocument::from_pixels(RgbaImage::from_fn(w, h, |x, y| {
        Rgba(p[(y * w + x) as usize].map(|v| ((u32::from(v) + 128) / 257) as u8))
    }));
    let bc: Vec<u8> = c.iter().map(|v| if *v > 0 { 255 } else { 0 }).collect();
    let mut r = request(120, 120, 270, 270);
    r["source_mask"] = mask_all();
    r["sample"] = json!("all");
    r["output"] = json!("new_layer");
    for depth in [8, 16] {
        let render = || {
            let (doc, bdoc, r, c, bc) = (&doc, &bdoc, &r, &c, &bc);
            move |token: usize| {
                let control = unsafe { &*(token as *const DrJobControl) };
                let out = if depth == 16 {
                    typed(doc, r, c, REFERENCE_WORKING_BYTES, Some(control)).map(|v| v.is_some())
                } else {
                    byte(bdoc, r, bc, Some(control)).map(|v| v.is_some())
                };
                match out {
                    Ok(_) => (true, String::new()),
                    Err(e) => (false, e),
                }
            }
        };
        // Park one checkpoint into synthesis past 0.2 (a fresh twin's log).
        let n0 = entry_count(&|p, stage, _| stage == 3 && p > 0.2, render());
        let parked = cancelled_parked_at(
            n0 + 1,
            &|p, stage| stage == 3 && p > 0.2 && p < 0.98,
            render(),
        );
        eprintln!("{depth}-bit: {}", parked_line(&parked));
        assert!(parked.scratch > 0);
    }
    assert_eq!(samples(&doc), p);
}

#[test]
fn bounded_recipe_counts_and_aggregate_visits_refuse_before_synthesis() {
    let (w, h) = (1024, 1024);
    let doc = native(w, h, vec![[12001, 23002, 34003, 65535]; (w * h) as usize]);
    let bdoc = DrDocument::from_pixels(RgbaImage::from_pixel(w, h, Rgba([12, 34, 56, 255])));
    let mut c = vec![0u16; (w * h) as usize];
    c[500 * w as usize + 500] = 65535;
    let bc: Vec<u8> = c.iter().map(|v| if *v > 0 { 255 } else { 0 }).collect();
    let cases = [
        (
            json!({"version":1,"initial":"all","operations":vec![json!({"op":"exclude_rect","x":0,"y":0,"width":1,"height":1});513]}),
            "512",
        ),
        (
            json!({"version":1,"initial":"all","operations":[{"op":"exclude_brush","radius":1,"points":vec![[1,1];4097]}]}),
            "4096",
        ),
        (
            json!({"version":1,"initial":"all","operations":vec![json!({"op":"exclude_rect","x":0,"y":0,"width":1024,"height":1024});257]}),
            "256 million",
        ),
    ];
    for (recipe, expected) in cases {
        let mut r = request(500, 500, 1, 1);
        r["source_mask"] = recipe;
        for failure in [
            typed(&doc, &r, &c, REFERENCE_WORKING_BYTES, None)
                .err()
                .unwrap(),
            byte(&bdoc, &r, &bc, None).err().unwrap(),
        ] {
            assert!(failure.contains(expected), "{failure}");
        }
    }
}
