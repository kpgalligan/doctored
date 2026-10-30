//! A source plan must borrow unrelated descriptions. Executing its immutable
//! result may copy them, but only after that owned-clone cost was admitted.
use doctored_core::doc::DrDocument;
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_retained_source::{dr_doc_mixed_source_edit, dr_doc_mixed_source_plan};
use doctored_core::job_control::DrJobControl;
use doctored_core::precision_image::REFERENCE_WORKING_BYTES;
use doctored_core::retained_source::SourceEdit;
use image::{Rgba, RgbaImage};
use serde_json::{json, Value};
use std::alloc::{GlobalAlloc, Layout, System};
use std::ffi::{c_char, CStr, CString};
use std::ptr;
use std::sync::atomic::{AtomicUsize, Ordering};

struct Tracked;
static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);
static LARGEST: AtomicUsize = AtomicUsize::new(0);
fn added(bytes: usize) {
    let live = LIVE.fetch_add(bytes, Ordering::Relaxed) + bytes;
    PEAK.fetch_max(live, Ordering::Relaxed);
    LARGEST.fetch_max(bytes, Ordering::Relaxed);
}
unsafe impl GlobalAlloc for Tracked {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let p = unsafe { System.alloc(layout) };
        if !p.is_null() {
            added(layout.size());
        }
        p
    }
    unsafe fn dealloc(&self, p: *mut u8, layout: Layout) {
        LIVE.fetch_sub(layout.size(), Ordering::Relaxed);
        unsafe { System.dealloc(p, layout) }
    }
    unsafe fn realloc(&self, p: *mut u8, layout: Layout, new: usize) -> *mut u8 {
        let q = unsafe { System.realloc(p, layout, new) };
        if !q.is_null() {
            LIVE.fetch_sub(layout.size(), Ordering::Relaxed);
            added(new);
        }
        q
    }
}
#[global_allocator]
static ALLOCATOR: Tracked = Tracked;
unsafe fn text(p: *mut c_char) -> String {
    if p.is_null() {
        return String::new();
    }
    let s = unsafe { CStr::from_ptr(p) }.to_str().unwrap().to_owned();
    unsafe { dr_string_free(p) };
    s
}
fn reset() -> usize {
    let live = LIVE.load(Ordering::Relaxed);
    PEAK.store(live, Ordering::Relaxed);
    LARGEST.store(0, Ordering::Relaxed);
    live
}
fn fixture() -> DrDocument {
    DrDocument::from_pixels(RgbaImage::from_pixel(1, 1, Rgba([31, 63, 127, 255])))
}
#[test]
fn large_unrelated_metadata_is_borrowed_by_plans_and_charged_before_owned_edits() {
    let input = fixture();
    let place=SourceEdit::parse(&json!({"op":"place","source_id":"source","name":"Photo","x":0,"y":0,"link":{"path":"/old.png","sha256":"a".repeat(64)}}).to_string()).unwrap();
    let mut doc = input
        .source_edit(Some(&input), &place, REFERENCE_WORKING_BYTES, 0, None)
        .unwrap()
        .unwrap();
    let metadata_bytes = 8 * 1024 * 1024;
    doc.layers[0].meta = Some("x".repeat(metadata_bytes));
    doc.layers[0].name = "Long layer name".repeat(128);
    for request in [
        json!({"op":"replace","layer":1,"revision_id":"next"}),
        json!({"op":"transform","layer":1,"matrix":[1,0,0,1,0.25,0.5],"filter":"bilinear"}),
    ] {
        let input_pointer = if request["op"] == "replace" {
            &input as *const DrDocument
        } else {
            ptr::null()
        };
        let request = CString::new(request.to_string()).unwrap();
        let mut error = ptr::null_mut();
        reset();
        let plan = unsafe {
            dr_doc_mixed_source_plan(
                &doc,
                input_pointer,
                ptr::null(),
                request.as_ptr(),
                &mut error,
            )
        };
        assert!(error.is_null(), "{}", unsafe { text(error) });
        assert!(!plan.is_null());
        let plan: Value = serde_json::from_str(&unsafe { text(plan) }).unwrap();
        assert!(
            LARGEST.load(Ordering::Relaxed) < 1024 * 1024,
            "read-only plan copied a large description"
        );
        let scratch = plan["scratch_bytes"].as_u64().unwrap();
        let retained = plan["retained_bytes"].as_u64().unwrap();
        assert!(scratch >= metadata_bytes as u64 + 4 * 1024 * 1024);
        let token = DrJobControl::default();
        reset();
        let refused = unsafe {
            dr_doc_mixed_source_edit(
                &doc,
                input_pointer,
                ptr::null(),
                request.as_ptr(),
                retained + 4 * 1024 * 1024,
                0,
                &token,
                &mut error,
            )
        };
        assert!(refused.is_null());
        assert!(!unsafe { text(error) }.is_empty());
        assert_eq!(token.scratch_bytes(), 0);
        assert!(
            LARGEST.load(Ordering::Relaxed) < 1024 * 1024,
            "refused edit cloned metadata before admission"
        );
        error = ptr::null_mut();
        let baseline = reset();
        let result = unsafe {
            dr_doc_mixed_source_edit(
                &doc,
                input_pointer,
                ptr::null(),
                request.as_ptr(),
                retained + scratch,
                0,
                ptr::null(),
                &mut error,
            )
        };
        assert!(error.is_null(), "{}", unsafe { text(error) });
        assert!(!result.is_null());
        let result = unsafe { Box::from_raw(result) };
        let extra = PEAK.load(Ordering::Relaxed).saturating_sub(baseline);
        assert!(
            extra as u64 <= scratch,
            "actual {extra} allocation bytes exceed plan {scratch}"
        );
        assert_eq!(
            result.layers[0].meta.as_deref(),
            doc.layers[0].meta.as_deref()
        );
        assert_eq!(result.layers[0].name, doc.layers[0].name);
        assert_eq!(doc.layers[0].meta.as_ref().unwrap().len(), metadata_bytes);
    }
}
