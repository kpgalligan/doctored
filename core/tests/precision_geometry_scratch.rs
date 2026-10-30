//! The u16 typed geometry plan charges the tiled→contiguous RGBA16 source copy
//! its resample binds AND the destination's contiguous→tiled adoption
//! copy, which coexist because `samples` is still bound when
//! `from_rgba16_unseeded` tiles `output`. Measured under a tracked global
//! allocator: the edit's allocation peak above the live baseline must fit the
//! plan's `scratch_bytes` with no allowance, and must include the source copy.
//! The two downscales keep the adoption small next to the source copy; the
//! 2× affine upscale makes it the largest addend, so its peak must hold the
//! source copy, the output and the adoption copy at once.
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_precision::dr_precision_doc_free;
use doctored_core::ffi_precision_edit::{dr_precision_doc_edit, dr_precision_doc_edit_plan};
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use serde_json::{json, Value};
use std::alloc::{GlobalAlloc, Layout, System};
use std::ffi::{CStr, CString};
use std::ptr;
use std::sync::atomic::{AtomicUsize, Ordering};

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

const SIDE: u32 = 1024;

fn take(p: *mut std::ffi::c_char) -> String {
    if p.is_null() {
        return "<no error message>".into();
    }
    let text = unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned();
    unsafe { dr_string_free(p) };
    text
}

#[test]
fn the_u16_geometry_plan_covers_the_source_copy_its_resample_binds() {
    let pixels: Vec<[u16; 4]> = (0..SIDE * SIDE)
        .map(|i| {
            let (x, y) = (i % SIDE, i / SIDE);
            [
                ((x * 61 + y * 7) % 65536) as u16,
                ((x * 13 + y * 59) % 65536) as u16,
                ((x ^ y) * 64 % 65536) as u16,
                65535,
            ]
        })
        .collect();
    let image =
        PrecisionImage::from_rgba16_unseeded(SIDE, SIDE, pixels, IccProfile::srgb(), None).unwrap();
    let doc =
        PrecisionDocument::from_image(image, Metadata::default(), Resolution::default()).unwrap();
    let side = u64::from(SIDE);
    let source_copy = side * side * 8;
    // The 2× upscale's source copy, output and adoption copy (2048² RGBA16
    // each) are all live when the output is tiled.
    let upscale = source_copy + 2 * (2 * side) * (2 * side) * 8;
    for (request, floor) in [
        (
            json!({"op":"affine","layer":0,"target":"pixels","matrix":[0.25,0,0,0.25,0,0],"filter":"bilinear"}),
            source_copy,
        ),
        (
            json!({"op":"resize","width":256,"height":256,"filter":"lanczos3"}),
            source_copy,
        ),
        (
            json!({"op":"affine","layer":0,"target":"pixels","matrix":[2,0,0,2,0,0],"filter":"bilinear"}),
            upscale,
        ),
    ] {
        let text = CString::new(request.to_string()).unwrap();
        let mut err = ptr::null_mut();
        let plan = unsafe {
            dr_precision_doc_edit_plan(&doc, text.as_ptr(), REFERENCE_WORKING_BYTES, 0, &mut err)
        };
        assert!(!plan.is_null(), "{request}: {}", take(err));
        let info: Value =
            serde_json::from_str(unsafe { CStr::from_ptr(plan) }.to_str().unwrap()).unwrap();
        unsafe { dr_string_free(plan) };
        let scratch = info["scratch_bytes"].as_u64().unwrap();

        let baseline = LIVE.load(Ordering::SeqCst);
        PEAK.store(baseline, Ordering::SeqCst);
        let out = unsafe {
            dr_precision_doc_edit(
                &doc,
                text.as_ptr(),
                ptr::null(),
                0,
                REFERENCE_WORKING_BYTES,
                0,
                ptr::null(),
                &mut err,
            )
        };
        let measured = (PEAK.load(Ordering::SeqCst) - baseline) as u64;
        assert!(!out.is_null(), "{request}: {}", take(err));
        unsafe { dr_precision_doc_free(out) };

        assert!(
            measured <= scratch,
            "{request}: measured {measured} B exceeds the plan's {scratch} B"
        );
        assert!(
            measured >= floor,
            "{request}: measured {measured} B is below the {floor} B of live copies"
        );
    }
}
