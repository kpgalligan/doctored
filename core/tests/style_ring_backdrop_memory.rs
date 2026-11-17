//! The Inner Shadow ring's T0 store (`style_composite::BandBackdrop::of_ring`,
//! 20 B per ring pixel, `style_composite::backdrop_store_bytes`) is charged
//! by the typed render plan and fits it, measured under a tracked global
//! allocator: a 16-bit document whose 512 × 512 layer is at alpha 128
//! everywhere — every pixel on the ring — under an Inner Shadow of Size 0,
//! Choke 0 and Distance 0.
//!
//! - (a) The plan exceeds the same document's with the Inner Shadow replaced
//!   by a Drop Shadow of Size 0 and Distance 0 (the same pad, effect count
//!   and blur) by exactly `20 · 512 · 512`.
//! - (b) The render's allocation peak above the live baseline fits the plan.
//! - (c) A measurement, printed and not asserted: the alpha-128 render's
//!   peak beside its opaque twin's (no ring), at 16 bits and at 8 bits
//!   through `flattened()`.
use std::alloc::{GlobalAlloc, Layout, System};
use std::ffi::{c_char, CStr, CString};
use std::ptr;
use std::sync::atomic::{AtomicUsize, Ordering};

use doctored_core::doc::DrDocument;
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_doc::dr_doc_free;
use doctored_core::ffi_precision_edit::dr_precision_doc_edit;
use doctored_core::ffi_style::dr_doc_set_layer_style;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer as Layer};
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use image::{Rgba, RgbaImage};
use serde_json::{json, Value};

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

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
const SIDE: u32 = 512;

fn take(p: *mut c_char) -> String {
    if p.is_null() {
        return "<no error message>".into();
    }
    let text = unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned();
    unsafe { dr_string_free(p) };
    text
}

fn inner_shadow() -> Value {
    json!({"effects":[{"type":"inner_shadow","blend":"normal","color":"#fae614","opacity":1,
        "use_global_light":false,"angle":120,"distance":0,"size":0,"choke":0,"noise":0}]})
}

/// The twin of (a): a Drop Shadow of Size 0, Spread 0 and Distance 0 has the
/// Inner Shadow's pad (reach 1, pad 2), effect count and blur (none) and
/// holds no backdrop store. A Color Overlay's pad is 1, which plans 2060 more
/// plane pixels' planes.
fn drop_shadow() -> Value {
    json!({"effects":[{"type":"drop_shadow","blend":"normal","color":"#fae614","opacity":1,
        "use_global_light":false,"angle":120,"distance":0,"size":0,"spread":0,"noise":0}]})
}

/// An opaque base under a `SIDE`² layer at 16-bit alpha `alpha`, the layer
/// carrying `style`.
fn doc16(style: &Value, alpha: u16) -> PrecisionDocument {
    let image = |px: [u16; 4]| {
        PrecisionImage::from_rgba16(
            SIDE,
            SIDE,
            vec![px; (SIDE * SIDE) as usize],
            IccProfile::srgb(),
            None,
        )
        .unwrap()
    };
    let doc = PrecisionDocument::from_parts(
        SIDE,
        SIDE,
        IccProfile::srgb(),
        Metadata::default(),
        Resolution::default(),
        vec![
            Layer::raster(
                "Base",
                image([10280, 41120, 56540, 65535]),
                (0, 0),
                1.0,
                true,
            )
            .unwrap(),
            Layer::raster(
                "Layer",
                image([56540, 15420, 7710, alpha]),
                (0, 0),
                1.0,
                true,
            )
            .unwrap(),
        ],
    )
    .unwrap();
    let request = json!({"op":"layer_style","version":1,"layer":1,"style":style});
    let text = CString::new(request.to_string()).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_edit(
            &doc,
            text.as_ptr(),
            ptr::null(),
            0,
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!out.is_null(), "{request}: {}", take(err));
    *unsafe { Box::from_raw(out) }
}

/// The same pair at 8 bits: the layer at byte alpha `alpha`.
fn doc8(style: &Value, alpha: u8) -> DrDocument {
    let base =
        DrDocument::from_pixels(RgbaImage::from_pixel(SIDE, SIDE, Rgba([40, 160, 220, 255])));
    let doc = base
        .adding_image_layer(
            0,
            RgbaImage::from_pixel(SIDE, SIDE, Rgba([220, 60, 30, alpha])),
            "Layer",
        )
        .unwrap();
    let handle = Box::into_raw(Box::new(doc));
    let text = CString::new(style.to_string()).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe { dr_doc_set_layer_style(handle, 1, text.as_ptr(), &mut err) };
    unsafe { dr_doc_free(handle) };
    assert!(!out.is_null(), "{style}: {}", take(err));
    *unsafe { Box::from_raw(out) }
}

/// The allocation peak of `f` above the live baseline.
fn peak_of<T>(f: impl FnOnce() -> T) -> u64 {
    let baseline = LIVE.load(Ordering::SeqCst);
    PEAK.store(baseline, Ordering::SeqCst);
    let out = f();
    let measured = (PEAK.load(Ordering::SeqCst) - baseline) as u64;
    drop(out);
    measured
}

#[test]
fn the_ring_store_is_charged_by_the_plan_and_the_render_fits_it() {
    let pixels = u64::from(SIDE) * u64::from(SIDE);
    let ring = doc16(&inner_shadow(), 128 * 257);
    let planned = ring.render_plan().unwrap().scratch_bytes;
    let twin_plan = doc16(&drop_shadow(), 128 * 257)
        .render_plan()
        .unwrap()
        .scratch_bytes;
    // (a)
    assert_eq!(
        planned.checked_sub(twin_plan),
        Some(20 * pixels),
        "the plan charges the ring store: {planned} B against the twin's {twin_plan} B"
    );
    // (b)
    let measured = peak_of(|| ring.render(LIMIT, 0, None).unwrap());
    assert!(
        measured <= planned,
        "the render's peak {measured} B exceeds the plan's {planned} B"
    );
    // (c), printed only.
    let opaque = doc16(&inner_shadow(), 65535);
    let twin = peak_of(|| opaque.render(LIMIT, 0, None).unwrap());
    eprintln!(
        "16 bits: alpha 128 peak {measured} B, opaque twin peak {twin} B, difference {} B, \
         ring store {} B",
        measured as i64 - twin as i64,
        20 * pixels
    );
    let (ring8, opaque8) = (doc8(&inner_shadow(), 128), doc8(&inner_shadow(), 255));
    let peak8 = peak_of(|| ring8.flattened().unwrap());
    let twin8 = peak_of(|| opaque8.flattened().unwrap());
    eprintln!(
        "8 bits: alpha 128 peak {peak8} B, opaque twin peak {twin8} B, difference {} B, ring \
         store {} B",
        peak8 as i64 - twin8 as i64,
        20 * pixels
    );
}
