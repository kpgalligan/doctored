//! 13B-9's stated cost: the heap bytes one in-place 16-bit retouch dab
//! allocates, by canvas size (a counting `#[global_allocator]`; nothing is
//! timed). Each JSON line is one case: the caller's input bytes, the peak live
//! heap above the baseline during the `dr_precision_doc_edit` call (and per
//! canvas pixel), the bytes allocated in the call and what the answer retains.
//! A 100×100 dab pays a canvas-proportional peak today (24 bytes a canvas
//! pixel for Clone and Heal, 16 for Spot Heal): the source plate, its
//! contiguous copy and the transformed plane (`designs/phase-13b-tiled-status.md`).
//!
//! `cd core && cargo run --release --example phase13b_retouch_dab`

use std::alloc::{GlobalAlloc, Layout, System};
use std::ffi::{CStr, CString};
use std::ptr;
use std::sync::atomic::{AtomicUsize, Ordering};

use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_precision_edit::dr_precision_doc_edit;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer as Layer};
use doctored_core::precision_image::PrecisionImage;
use serde_json::{json, Value};

struct Tracked;
static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);
static TOTAL: AtomicUsize = AtomicUsize::new(0);

fn grew(bytes: usize) {
    let live = LIVE.fetch_add(bytes, Ordering::Relaxed) + bytes;
    PEAK.fetch_max(live, Ordering::Relaxed);
    TOTAL.fetch_add(bytes, Ordering::Relaxed);
}

unsafe impl GlobalAlloc for Tracked {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let p = unsafe { System.alloc(layout) };
        if !p.is_null() {
            grew(layout.size());
        }
        p
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let p = unsafe { System.alloc_zeroed(layout) };
        if !p.is_null() {
            grew(layout.size());
        }
        p
    }
    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        unsafe { System.dealloc(pointer, layout) };
        LIVE.fetch_sub(layout.size(), Ordering::Relaxed);
    }
    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, length: usize) -> *mut u8 {
        let p = unsafe { System.realloc(pointer, layout, length) };
        if !p.is_null() {
            if length >= layout.size() {
                grew(length - layout.size());
            } else {
                LIVE.fetch_sub(layout.size() - length, Ordering::Relaxed);
            }
        }
        p
    }
}

#[global_allocator]
static ALLOCATOR: Tracked = Tracked;

fn pattern16(width: u32, height: u32, seed: u32) -> Vec<[u16; 4]> {
    (0..width * height)
        .map(|i| {
            let (x, y) = (i % width, i / width);
            [
                ((x * 2711 + y * 911 + seed * 1234) % 65536) as u16,
                ((x * 523 + y * 3301 + seed * 77) % 65536) as u16,
                (((x ^ y) * 1999 + seed) % 65536) as u16,
                65535,
            ]
        })
        .collect()
}

fn document(w: u32, h: u32, layers: u32) -> PrecisionDocument {
    let profile = IccProfile::srgb();
    let layers = (0..layers)
        .map(|k| {
            let image =
                PrecisionImage::from_rgba16(w, h, pattern16(w, h, k + 1), profile.clone(), None)
                    .expect("a layer image");
            Layer::raster("L", image, (0, 0), 1.0, true).expect("a layer")
        })
        .collect();
    PrecisionDocument::from_parts(
        w,
        h,
        profile,
        Metadata::default(),
        Resolution::default(),
        layers,
    )
    .expect("a document")
}

fn coverage(w: u32, h: u32, rect: (u32, u32, u32, u32)) -> Vec<u16> {
    let mut out = vec![0u16; w as usize * h as usize];
    for y in rect.1..rect.1 + rect.3 {
        for x in rect.0..rect.0 + rect.2 {
            out[y as usize * w as usize + x as usize] = u16::MAX;
        }
    }
    out
}

fn measure(
    label: &str,
    canvas: (u32, u32),
    doc: &PrecisionDocument,
    request: &Value,
    samples: &[u16],
) {
    let text = CString::new(request.to_string()).expect("a request");
    let mut error = ptr::null_mut();
    let base = LIVE.load(Ordering::Relaxed);
    PEAK.store(base, Ordering::Relaxed);
    let total = TOTAL.load(Ordering::Relaxed);
    let out = unsafe {
        dr_precision_doc_edit(
            doc,
            text.as_ptr(),
            samples.as_ptr(),
            samples.len(),
            64 << 30,
            0,
            ptr::null(),
            &mut error,
        )
    };
    let peak = PEAK.load(Ordering::Relaxed) - base;
    let allocated = TOTAL.load(Ordering::Relaxed) - total;
    let retained = LIVE.load(Ordering::Relaxed) - base;
    let n = canvas.0 as usize * canvas.1 as usize;
    let outcome = if !error.is_null() {
        let message = unsafe { CStr::from_ptr(error) }
            .to_string_lossy()
            .into_owned();
        unsafe { dr_string_free(error) };
        format!("err:{message}")
    } else if out.is_null() {
        "null".into()
    } else {
        "ok".into()
    };
    println!(
        "{}",
        json!({
            "case": label,
            "canvas": format!("{}x{}", canvas.0, canvas.1),
            "input_bytes": samples.len() * 2,
            "peak_above_baseline": peak,
            "peak_per_canvas_px": (peak as f64 / n as f64 * 100.0).round() / 100.0,
            "allocated_in_call": allocated,
            "retained_by_answer": retained,
            "outcome": outcome,
        })
    );
    if !out.is_null() {
        drop(unsafe { Box::from_raw(out) });
    }
}

fn main() {
    let dab = (1000u32, 800u32, 100u32, 100u32);
    let matrix = [1.0, 0.0, 0.0, 1.0, 300.0, 0.0];
    for canvas in [(3000u32, 2000u32), (6000, 4000)] {
        let (w, h) = canvas;
        let one = document(w, h, 1);
        let three = document(w, h, 3);
        let cover = coverage(w, h, dab);
        let mut selected = cover.clone();
        selected.extend(coverage(w, h, (900, 700, 400, 400)));
        let clone = |sample: &str, selection: bool| {
            json!({"op":"retouch_clone","layer":0,"sample":sample,
                "matrix":matrix,"opacity":1.0,"selection":selection})
        };
        measure(
            "clone current",
            canvas,
            &one,
            &clone("current", false),
            &cover,
        );
        measure(
            "clone current + selection",
            canvas,
            &one,
            &clone("current", true),
            &selected,
        );
        measure(
            "clone all, 3 layers",
            canvas,
            &three,
            &clone("all", false),
            &cover,
        );
        measure(
            "heal current",
            canvas,
            &one,
            &json!({"op":"retouch_heal","layer":0,"sample":"current","matrix":matrix,
                "opacity":1.0}),
            &cover,
        );
        let hole = (dab.0 + 20, dab.1 + 20, 48u32, 48u32);
        measure(
            "spot heal 48x48",
            canvas,
            &one,
            &json!({"op":"spot_heal","layer":0,"x":hole.0,"y":hole.1,"width":hole.2,
                "height":hole.3,"seed":1}),
            &coverage(w, h, hole),
        );
    }
}
