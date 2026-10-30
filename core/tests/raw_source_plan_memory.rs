//! Native source planning may retain descriptor references, not one JSON copy
//! for every developed asset. All masters share tiny planes in this allocator
//! regression; a large allocation peak therefore exposes metadata copying.
use doctored_core::doc::DrDocument;
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_retained_source::{
    dr_doc_has_retained_source, dr_doc_mixed_source_plan, dr_doc_source_info_scratch_bytes,
};
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use doctored_core::raw_source::{RawDescriptor, RawSource};
use doctored_core::retained_source::{SourceEdit, SourceInput};
use image::{Rgba, RgbaImage};
use serde_json::json;
use std::alloc::{GlobalAlloc, Layout, System};
use std::ffi::{CStr, CString};
use std::ptr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

struct Tracked;
static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);
fn added(bytes: usize) {
    let live = LIVE.fetch_add(bytes, Ordering::Relaxed) + bytes;
    PEAK.fetch_max(live, Ordering::Relaxed);
}
unsafe impl GlobalAlloc for Tracked {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let pointer = unsafe { System.alloc(layout) };
        if !pointer.is_null() {
            added(layout.size());
        }
        pointer
    }
    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        LIVE.fetch_sub(layout.size(), Ordering::Relaxed);
        unsafe { System.dealloc(pointer, layout) }
    }
    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        let result = unsafe { System.realloc(pointer, layout, size) };
        if !result.is_null() {
            LIVE.fetch_sub(layout.size(), Ordering::Relaxed);
            added(size);
        }
        result
    }
}
#[global_allocator]
static ALLOCATOR: Tracked = Tracked;

#[test]
fn raw_plans_borrow_existing_descriptors_without_aggregate_json_copies() {
    let master = PrecisionImage::from_rgba16(
        1,
        1,
        vec![[12345, 23456, 34567, 65535]],
        IccProfile::srgb(),
        None,
    )
    .unwrap();
    let input =
        PrecisionDocument::from_image(master, Metadata::default(), Resolution::default()).unwrap();
    let base = DrDocument::from_pixels(RgbaImage::from_pixel(1, 1, Rgba([0, 0, 0, 0])));
    let placement = SourceEdit::parse(r#"{"op":"place","source_id":"camera","name":"Camera","x":0,"y":0,"depth_conversion":"convert_fallback"}"#).unwrap();
    let mut doc = base
        .source_edit_input(
            Some(SourceInput::Typed(&input)),
            &placement,
            REFERENCE_WORKING_BYTES,
            0,
            None,
        )
        .unwrap()
        .unwrap();
    let descriptor = RawDescriptor::parse(&json!({"schema_version":1,"original_filename":"camera.NEF",
        "content_sha256":"a".repeat(64), "decoder":{"identifier":"unavailable","version":"1","implementation":"saved"},
        "recipe":{"opaque_future_control":"x".repeat(40 * 1024)}}).to_string()).unwrap();
    let raw = Arc::new(RawSource {
        encoded: Arc::new(vec![1, 2, 3]),
        descriptor,
    });
    let template = doc.layers[1].clone();
    doc.layers = (0..96)
        .map(|index| {
            let mut layer = template.clone();
            let mut source = layer.retained_source.take().unwrap();
            let mut asset = (*source.asset).clone();
            asset.id = format!("camera-{index}");
            asset.revision_id = format!("developed-{index}");
            asset.raw = Some(raw.clone());
            source.asset = Arc::new(asset);
            layer.retained_source = Some(source);
            layer
        })
        .collect();
    let request =
        CString::new(r#"{"op":"transform","layer":95,"matrix":[1,0,0,1,1,0],"filter":"nearest"}"#)
            .unwrap();
    let baseline = LIVE.load(Ordering::Relaxed);
    PEAK.store(baseline, Ordering::Relaxed);
    assert!(unsafe { dr_doc_has_retained_source(&doc, 95) });
    assert_eq!(
        PEAK.load(Ordering::Relaxed),
        baseline,
        "binding guard allocated"
    );
    let info_scratch = unsafe { dr_doc_source_info_scratch_bytes(&doc, 95) };
    assert!(info_scratch >= 40 * 1024 * 768);
    assert!(
        PEAK.load(Ordering::Relaxed).saturating_sub(baseline) < 4096,
        "source-info scratch query cloned or serialized a recipe"
    );
    PEAK.store(baseline, Ordering::Relaxed);
    let mut error = ptr::null_mut();
    let plan = unsafe {
        dr_doc_mixed_source_plan(&doc, ptr::null(), ptr::null(), request.as_ptr(), &mut error)
    };
    assert!(
        error.is_null(),
        "{}",
        if error.is_null() {
            String::new()
        } else {
            unsafe { CStr::from_ptr(error).to_string_lossy().into_owned() }
        }
    );
    assert!(!plan.is_null());
    unsafe { dr_string_free(plan) };
    let peak = PEAK.load(Ordering::Relaxed).saturating_sub(baseline);
    assert!(
        peak < 2 * 1024 * 1024,
        "planning retained {peak} extra bytes; descriptors must remain shared"
    );
}
