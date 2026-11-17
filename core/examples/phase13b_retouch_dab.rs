//! 13B-9's stated cost: the heap bytes one in-place 16-bit or 32-bit float
//! retouch dab allocates, by canvas size (a counting `#[global_allocator]`;
//! nothing is timed). Each JSON line is one case: the caller's input bytes,
//! the peak live heap above the baseline during the `dr_precision_doc_edit`
//! call (and per canvas pixel), the bytes allocated in the call and what the
//! answer retains (`designs/phase-13b-tiled-status.md`).
//!
//! Before 13B-9a a 100×100 Clone or Heal paid 24 bytes a canvas pixel at 16
//! bits and 48 at float (the source plate, its contiguous copy and the
//! transformed plane: 576,019,210 and 1,152,019,210 at 6000×4000). Since
//! 13B-9a a `sample: current` Clone or Heal reads and transforms its bound
//! only (`precision_retouch_apply`'s module doc): at 6000×4000 the 16-bit
//! Clone peaks at 1,215,530 and the Heal at 1,221,994, the float Clone at
//! 2,424,106 and the Heal at 2,437,034, the answer's new tiles included.
//! Since 13B-9b a `sample: current` Spot Heal reads its source layer in place
//! and plans its hole over its bounds: the 48×48 dab peaks at 1,160,946 at
//! 6000×4000 (384,022,322 before). What stays canvas-proportional: a
//! selection's coverage domain (2 bytes a pixel) and the `all` /
//! `current_and_below` plate's own render (24 bytes a pixel).
//!
//! `cd core && cargo run --release --example phase13b_retouch_dab`

use std::alloc::{GlobalAlloc, Layout, System};
use std::ffi::{CStr, CString};
use std::ptr;
use std::sync::atomic::{AtomicUsize, Ordering};

use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_precision_edit::{dr_precision_doc_edit, dr_precision_doc_edit_plan};
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_clipboard::ClipboardPayload;
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer as Layer};
use doctored_core::precision_image::PrecisionImage;
use serde_json::{json, Value};

/// The admission limit: high enough that only the bytes are measured.
const LIMIT: u64 = 64 << 30;

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

/// One opaque `w × h` 32-bit float layer of `pattern16`'s samples scaled
/// into an extended range, through an RZCB v3 payload (the clipboard's
/// float document constructor).
fn float_document(w: u32, h: u32) -> PrecisionDocument {
    let profile = IccProfile::srgb();
    let mut bytes = b"RZCB".to_vec();
    for word in [3u32, 2, w, h, 0, 0] {
        bytes.extend(word.to_le_bytes());
    }
    bytes.extend(72f32.to_le_bytes());
    bytes.extend(72f32.to_le_bytes());
    bytes.extend((profile.bytes().len() as u32).to_le_bytes());
    bytes.extend(profile.bytes().iter());
    bytes.push(0);
    bytes.extend(0u32.to_le_bytes());
    bytes.extend(4f32.to_le_bytes());
    bytes.extend(0u32.to_le_bytes());
    bytes.reserve(w as usize * h as usize * 16);
    for p in pattern16(w, h, 1) {
        let scaled = [
            f32::from(p[0]) / 65535.0 * 2.0,
            f32::from(p[1]) / 65535.0,
            f32::from(p[2]) / 65535.0 * 0.5,
            1.0,
        ];
        scaled.iter().for_each(|v| bytes.extend(v.to_le_bytes()));
    }
    let payload = ClipboardPayload::decode(&bytes, LIMIT, 0).expect("a payload");
    drop(bytes);
    let doc =
        PrecisionDocument::from_clipboard(&payload, LIMIT, 0, None).expect("a float document");
    assert!(doc.is_float());
    doc
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

/// `plane` (canvas-sized, `w` wide) cropped to `rect`, row-major.
fn crop(plane: &[u16], w: u32, (x, y, rw, rh): (u32, u32, u32, u32)) -> Vec<u16> {
    let mut out = Vec::with_capacity(rw as usize * rh as usize);
    for row in y..y + rh {
        let at = row as usize * w as usize + x as usize;
        out.extend_from_slice(&plane[at..at + rw as usize]);
    }
    out
}

/// The request's `scratch_bytes` (`dr_precision_doc_edit_plan`); 0 on a
/// refusal.
fn planned(doc: &PrecisionDocument, request: &Value) -> u64 {
    let text = CString::new(request.to_string()).expect("a request");
    let mut error = ptr::null_mut();
    let out = unsafe { dr_precision_doc_edit_plan(doc, text.as_ptr(), LIMIT, 0, &mut error) };
    if out.is_null() {
        if !error.is_null() {
            unsafe { dr_string_free(error) };
        }
        return 0;
    }
    let plan: Value = serde_json::from_str(&unsafe { CStr::from_ptr(out) }.to_string_lossy())
        .unwrap_or(Value::Null);
    unsafe { dr_string_free(out) };
    plan["scratch_bytes"].as_u64().unwrap_or(0)
}

fn measure(
    label: &str,
    canvas: (u32, u32),
    doc: &PrecisionDocument,
    request: &Value,
    samples: &[u16],
) {
    let plan = planned(doc, request);
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
            LIMIT,
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
            "plan_scratch": plan,
            "plan_over_peak": (plan as f64 / peak.max(1) as f64 * 100.0).round() / 100.0,
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
    // Rotated 0.3 rad and scaled 1.07: the source rect is no longer the dab
    // moved, and the walk's sums are not integers.
    let (s, c) = (0.3f64.sin() * 1.07, 0.3f64.cos() * 1.07);
    let rotated = [c, -s, s, c, 211.37, -97.61];
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
        // 13B-9c's rect form: the dab and the selection cropped to the dab
        // grown by 2 px, the rect the app names (`healBounds`).
        let rect = (dab.0 - 2, dab.1 - 2, dab.2 + 4, dab.3 + 4);
        let rect_form = |mut request: Value| {
            request["coverage_rect"] = json!([rect.0, rect.1, rect.2, rect.3]);
            request
        };
        let mut rect_selected = crop(&cover, w, rect);
        rect_selected.extend(crop(&selected[cover.len()..], w, rect));
        measure(
            "clone current + selection, rect form",
            canvas,
            &one,
            &rect_form(clone("current", true)),
            &rect_selected,
        );
        measure(
            "heal current, rect form",
            canvas,
            &one,
            &rect_form(json!({"op":"retouch_heal","layer":0,"sample":"current",
                "matrix":matrix,"opacity":1.0})),
            &crop(&cover, w, rect),
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
        measure(
            "clone current, rotated and scaled",
            canvas,
            &one,
            &json!({"op":"retouch_clone","layer":0,"sample":"current",
                "matrix":rotated,"opacity":0.8}),
            &cover,
        );
        measure(
            "heal current, rotated and scaled",
            canvas,
            &one,
            &json!({"op":"retouch_heal","layer":0,"sample":"current","matrix":rotated,
                "opacity":1.0}),
            &cover,
        );
        measure(
            "heal current at the canvas corner",
            canvas,
            &one,
            &json!({"op":"retouch_heal","layer":0,"sample":"current",
                "matrix":[1.0,0.0,0.0,1.0,0.0,-300.0],"opacity":1.0}),
            &coverage(w, h, (0, h - 60, 80, 60)),
        );
        drop(three);
        let float = float_document(w, h);
        measure(
            "float clone current",
            canvas,
            &float,
            &clone("current", false),
            &cover,
        );
        measure(
            "float heal current",
            canvas,
            &float,
            &json!({"op":"retouch_heal","layer":0,"sample":"current","matrix":matrix,
                "opacity":1.0}),
            &cover,
        );
        if canvas == (6000, 4000) {
            // `selected_color_planes_24mp`'s RH32, RS32 and RF32: the
            // 1000×1000 box at (2500, 1500), in place (its floors are
            // ⌈1.25 ×⌉ these counted peaks).
            let square = (2500u32, 1500u32, 1000u32, 1000u32);
            let boxed = coverage(w, h, square);
            let bounds = json!({"layer":0,"x":square.0,"y":square.1,"width":square.2,
                "height":square.3,"ring":0,"seed":7});
            let with = |op: &str| {
                let mut request = bounds.clone();
                request["op"] = json!(op);
                request
            };
            measure(
                "float heal 1000x1000 box",
                canvas,
                &float,
                &json!({"op":"retouch_heal","layer":0,"matrix":[1.0,0.0,0.0,1.0,64.0,0.0],
                    "x":square.0,"y":square.1,"width":square.2,"height":square.3}),
                &boxed,
            );
            measure(
                "float spot heal 1000x1000 box",
                canvas,
                &float,
                &with("spot_heal"),
                &boxed,
            );
            measure(
                "float content-aware fill 1000x1000 box",
                canvas,
                &float,
                &with("content_aware_fill"),
                &boxed,
            );
        }
        drop(float);
        let hole = (dab.0 + 20, dab.1 + 20, 48u32, 48u32);
        let spot = json!({"op":"spot_heal","layer":0,"x":hole.0,"y":hole.1,"width":hole.2,
            "height":hole.3,"seed":1});
        let holed = coverage(w, h, hole);
        measure("spot heal 48x48", canvas, &one, &spot, &holed);
        let around = (hole.0 - 2, hole.1 - 2, hole.2 + 4, hole.3 + 4);
        let mut spot_rect = spot.clone();
        spot_rect["coverage_rect"] = json!([around.0, around.1, around.2, around.3]);
        measure(
            "spot heal 48x48, rect form",
            canvas,
            &one,
            &spot_rect,
            &crop(&holed, w, around),
        );
    }
}
