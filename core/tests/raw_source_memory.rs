//! Allocation instrumentation verifies that large original bytes and unrelated
//! metadata are borrowed during plans and refused before either is copied.
use doctored_core::doc::DrDocument;
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_raw_source::*;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use doctored_core::retained_source::{SourceEdit, SourceInput};
use image::{Rgba, RgbaImage};
use serde_json::{json, Value};
use std::alloc::{GlobalAlloc, Layout, System};
use std::ffi::{c_char, CStr, CString};
use std::ptr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
struct Tracked;
static LARGEST: AtomicUsize = AtomicUsize::new(0);
unsafe impl GlobalAlloc for Tracked {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        LARGEST.fetch_max(layout.size(), Ordering::Relaxed);
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        unsafe { System.dealloc(pointer, layout) }
    }
    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, length: usize) -> *mut u8 {
        LARGEST.fetch_max(length, Ordering::Relaxed);
        unsafe { System.realloc(pointer, layout, length) }
    }
}
#[global_allocator]
static ALLOCATOR: Tracked = Tracked;
unsafe fn text(value: *mut c_char) -> String {
    assert!(!value.is_null());
    let out = unsafe { CStr::from_ptr(value) }
        .to_str()
        .unwrap()
        .to_string();
    unsafe { dr_string_free(value) };
    out
}
#[test]
fn plans_borrow_large_payload_and_metadata_and_admit_both_owned_copies() {
    let plain = DrDocument::from_pixels(RgbaImage::from_pixel(1, 1, Rgba([0; 4])));
    let input = PrecisionDocument::from_image(
        PrecisionImage::from_rgba16(1, 1, vec![[11, 373, 1729, 65535]], IccProfile::srgb(), None)
            .unwrap(),
        Metadata::default(),
        Resolution::default(),
    )
    .unwrap();
    let request = SourceEdit::parse(&json!({"op":"place","source_id":"raw","name":"Camera","x":0,"y":0,"depth_conversion":"convert_fallback"}).to_string()).unwrap();
    let mut doc = plain
        .source_edit_input(
            Some(SourceInput::Typed(&input)),
            &request,
            REFERENCE_WORKING_BYTES,
            0,
            None,
        )
        .unwrap()
        .unwrap();
    let length = 8 * 1024 * 1024;
    doc.layers[0].meta = Some("m".repeat(length));
    let bytes = vec![0x91; length];
    let descriptor = CString::new(json!({"schema_version":1,"original_filename":"camera.dng","content_sha256":"d".repeat(64),"decoder":{"identifier":"test","version":"1","implementation":"test"},"recipe":{"exposure":0.0}}).to_string()).unwrap();
    let mut err = ptr::null_mut();
    let large_descriptor = CString::new(json!({"schema_version":1,"original_filename":"camera.dng","content_sha256":"d".repeat(64),"decoder":{"identifier":"test","version":"1","implementation":"test"},"recipe":{"opaque_future_control":"x".repeat(40 * 1024)}}).to_string()).unwrap();
    LARGEST.store(0, Ordering::Relaxed);
    let refused = unsafe {
        dr_doc_attach_raw_source(
            &doc,
            1,
            bytes.as_ptr(),
            bytes.len(),
            large_descriptor.as_ptr(),
            1,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(refused.is_null());
    assert!(unsafe { text(err) }.contains("working limit"));
    assert!(
        LARGEST.load(Ordering::Relaxed) < 16 * 1024,
        "descriptor parsed before admission"
    );
    err = ptr::null_mut();
    LARGEST.store(0, Ordering::Relaxed);
    let planned =
        unsafe { dr_doc_raw_source_plan(&doc, 1, bytes.len(), descriptor.as_ptr(), &mut err) };
    assert!(err.is_null());
    let plan: Value = serde_json::from_str(&unsafe { text(planned) }).unwrap();
    assert!(
        LARGEST.load(Ordering::Relaxed) < 1024 * 1024,
        "plan copied payload/metadata"
    );
    let scratch = plan["scratch_bytes"].as_u64().unwrap();
    let retained = plan["retained_bytes"].as_u64().unwrap();
    assert!(scratch >= 2 * length as u64 + 4 * 1024 * 1024);
    assert!(retained >= 2 * length as u64);
    LARGEST.store(0, Ordering::Relaxed);
    let refused = unsafe {
        dr_doc_attach_raw_source(
            &doc,
            1,
            bytes.as_ptr(),
            bytes.len(),
            descriptor.as_ptr(),
            retained + 4 * 1024 * 1024,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(refused.is_null());
    assert!(unsafe { text(err) }.contains("working limit"));
    err = ptr::null_mut();
    assert!(
        LARGEST.load(Ordering::Relaxed) < 1024 * 1024,
        "refused edit copied payload/metadata"
    );
    let old = unsafe {
        dr_doc_attach_raw_source(
            &doc,
            1,
            bytes.as_ptr(),
            bytes.len(),
            descriptor.as_ptr(),
            REFERENCE_WORKING_BYTES,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(err.is_null());
    assert!(!old.is_null());
    let old = *unsafe { Box::from_raw(old) };
    let replacement = SourceEdit::parse(&json!({"op":"replace","layer":1,"revision_id":"second","depth_conversion":"convert_fallback"}).to_string()).unwrap();
    let next = old
        .source_edit_input(
            Some(SourceInput::Typed(&input)),
            &replacement,
            REFERENCE_WORKING_BYTES,
            0,
            None,
        )
        .unwrap()
        .unwrap();
    LARGEST.store(0, Ordering::Relaxed);
    let planned =
        unsafe { dr_doc_raw_source_from_plan(&next, 1, &old, 1, descriptor.as_ptr(), &mut err) };
    assert!(err.is_null());
    let plan: Value = serde_json::from_str(&unsafe { text(planned) }).unwrap();
    assert!(
        LARGEST.load(Ordering::Relaxed) < 1024 * 1024,
        "redevelopment plan copied payload/metadata"
    );
    assert_eq!(plan["work_units"], 0);
    assert!(plan["retained_bytes"].as_u64().unwrap() >= 3 * length as u64);
    let attached = unsafe {
        dr_doc_attach_raw_source_from(
            &next,
            1,
            &old,
            1,
            descriptor.as_ptr(),
            REFERENCE_WORKING_BYTES,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(err.is_null());
    assert!(!attached.is_null());
    let attached = *unsafe { Box::from_raw(attached) };
    let old = old.layers[1].effective_source(&old.profile).unwrap();
    let attached = attached.layers[1]
        .effective_source(&attached.profile)
        .unwrap();
    assert!(Arc::ptr_eq(
        &old.asset.raw.as_ref().unwrap().encoded,
        &attached.asset.raw.as_ref().unwrap().encoded
    ));
}
