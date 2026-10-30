//! A 32-bit float plane above `CONTIGUOUS_CACHE_BYTES` (67,108,864 px)
//! is tiled at the 8 GiB working limit of a 16 GB Mac, where such a raster is
//! admitted. Phase 1 tiled every plane built without an adopted `Vec` (an
//! added, filled layer, the float `.rz` reopen and every `with_rect`
//! output); phase 2 tiles the ADOPTED ones too (the clipboard's, a render's)
//! and charges each plan the copy it makes: a float render's finalization
//! holds its accumulator beside the tiles, float paint's grown layer is
//! built straight into tiles, and the R (render), F (flatten) and O (float
//! `.rz` reopen) plans cover ⌈1.25 × the measured peak⌉ at 24 MP and 100 MP
//! (`precision_render`'s and `precision_native_plan`'s module docs).
//!
//! The edge is 8193 (67,125,249 px, 33 × 33 = 1089 tiles). Oracles are
//! independent of the implementation: the samples are `formula(i)`
//! (`precision_image_reads.rs`'s), tiling is read through
//! `shared_tile_count` against the image itself (0 for a contiguous plane,
//! the tile count for a tiled one), and the reopen's transient is the live
//! heap above its baseline, from the tracking allocator below.
//!
//! A binary of its own: it sets the process working limit and installs its
//! own `#[global_allocator]`. Every test holds `SERIAL`, because the
//! allocator's peak is process-wide.

use std::alloc::{GlobalAlloc, Layout, System};
use std::ffi::{CStr, CString};
use std::ptr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, MutexGuard};

use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_precision_edit::{dr_precision_doc_edit, dr_precision_doc_edit_plan};
use doctored_core::ffi_resources::dr_set_process_working_limit;
use doctored_core::icc::IccProfile;
use doctored_core::layer_insert::InsertPosition;
use doctored_core::precision_clipboard::ClipboardPayload;
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_geometry::GeometryEdit;
use doctored_core::precision_native::native_color_plan;
use serde_json::{json, Value};

#[path = "common/float_fixture.rs"]
mod float_fixture;

// ------------------------------------------------------------ allocator --

/// Tracks live heap bytes and their peak (`alloc_zeroed` / `realloc`
/// included), as `precision_f32_unseeded.rs` does.
struct Tracked;
static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

fn grew(bytes: usize) {
    let live = LIVE.fetch_add(bytes, Ordering::Relaxed) + bytes;
    PEAK.fetch_max(live, Ordering::Relaxed);
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
static GLOBAL: Tracked = Tracked;

/// The current live bytes, with the peak reset to them.
fn baseline() -> usize {
    let live = LIVE.load(Ordering::Relaxed);
    PEAK.store(live, Ordering::Relaxed);
    live
}

// -------------------------------------------------------------- fixture --

static SERIAL: Mutex<()> = Mutex::new(());

fn serial() -> MutexGuard<'static, ()> {
    SERIAL.lock().unwrap_or_else(|e| e.into_inner())
}

/// The 8 GiB working limit of a 16 GB Mac, the assumed minimum.
const LIMIT: u64 = 8 << 30;
const EDGE: u32 = 8193;
const TILES: usize = 33 * 33;

fn at_eight_gib() {
    assert!(dr_set_process_working_limit(LIMIT));
}

/// `precision_image_reads.rs`'s sample formula.
fn formula(i: usize) -> [f32; 4] {
    [
        (i % 251) as f32 / 251.0,
        (i % 509) as f32 / 509.0,
        (i % 1021) as f32 / 1021.0,
        (i % 17) as f32 / 16.0,
    ]
}

fn bits(pixel: [f32; 4]) -> [u32; 4] {
    pixel.map(f32::to_bits)
}

/// A 2×2 float document grown by Canvas Size to 8193² with an empty layer
/// "L" added above its record.
fn grown_with_added_layer() -> PrecisionDocument {
    grown_to(EDGE, EDGE)
        .add_layer_at("L", 0, InsertPosition::Above)
        .expect("the layer is admitted")
        .expect("a layer is added")
}

/// A 2×2 float document grown by Canvas Size to `width` × `height`. No layer
/// is added: the measured cases are one record each. (When they were
/// measured a second 100 MP float record passed the native plan's decoded
/// cap, then 1.6 GB, which refused the add; the cap is now 8 GiB.)
fn grown_to(width: u32, height: u32) -> PrecisionDocument {
    let srgb = IccProfile::srgb();
    let doc = float_fixture::float_doc(2, 2, &srgb, &[vec![[0.25, 0.5, 0.75, 1.0]; 4]]);
    let edit = GeometryEdit::CanvasSize {
        width,
        height,
        x: 0,
        y: 0,
    };
    doc.geometry(&edit, LIMIT, 0, None)
        .expect("canvas size is admitted")
        .expect("the canvas changes")
}

/// A `side`² opaque float document whose one record holds `formula(i)` at
/// alpha 1.
fn opaque_doc(side: u32) -> PrecisionDocument {
    let pixels = (0..side as usize * side as usize)
        .map(|i| {
            let [r, g, b, _] = formula(i);
            [r, g, b, 1.0]
        })
        .collect();
    float_fixture::float_doc(side, side, &IccProfile::srgb(), &[pixels])
}

/// The request's plan, which must be admitted.
fn edit_plan(doc: &PrecisionDocument, request: &Value) -> Value {
    let text = CString::new(request.to_string()).unwrap();
    let mut error = ptr::null_mut();
    let plan = unsafe { dr_precision_doc_edit_plan(doc, text.as_ptr(), LIMIT, 0, &mut error) };
    assert!(!plan.is_null(), "{request} must be admitted");
    let parsed = serde_json::from_slice(unsafe { CStr::from_ptr(plan) }.to_bytes()).unwrap();
    unsafe { dr_string_free(plan) };
    parsed
}

/// The request's result over `samples`, which must change the document.
fn edited(doc: &PrecisionDocument, request: &Value, samples: &[u16]) -> PrecisionDocument {
    let text = CString::new(request.to_string()).unwrap();
    let mut error = ptr::null_mut();
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
    assert!(
        error.is_null() && !out.is_null(),
        "{request} must change the document"
    );
    *unsafe { Box::from_raw(out) }
}

/// ⌈1.25 × the largest measured footprint E⌉ of each case, `(case, 24 MP
/// floor, 100 MP floor)`: the records in `precision_render`'s module doc
/// (R, F) and `precision_native_plan`'s (O, re-measured for the streamed
/// reader: `native_file_size`'s `stream_measure` reopen32 and
/// reopen32l, max E 388,563,232 and 1,609,991,296).
const FLOORS: [(&str, u64, u64); 3] = [
    ("R", 962_622_130, 4_010_109_760),
    ("F", 962_560_690, 4_009_925_470),
    ("O", 485_704_040, 2_012_489_120),
];

// ---------------------------------------------------------------- tests --

#[test]
fn a_float_render_is_charged_its_tiling_copy() {
    let _serial = serial();
    at_eight_gib();
    let doc = opaque_doc(1024);
    let plan = doc.render_plan().expect("the render plan");
    let base = baseline();
    let out = doc.render(LIMIT, 0, None).expect("the render is admitted");
    let peak = PEAK.load(Ordering::Relaxed) - base;
    assert_eq!(
        out.shared_tile_count(&out),
        16,
        "the adopted result is tiled"
    );
    assert!(
        peak as u64 <= plan.scratch_bytes,
        "the plan charges the accumulator beside its tiles: peak {peak} > {}",
        plan.scratch_bytes
    );
}

#[test]
fn a_grown_float_fill_holds_no_adoption_copy() {
    let _serial = serial();
    at_eight_gib();
    let resize = GeometryEdit::CanvasSize {
        width: 1032,
        height: 1030,
        x: 4,
        y: 3,
    };
    let doc = opaque_doc(1024)
        .geometry(&resize, LIMIT, 0, None)
        .expect("canvas size is admitted")
        .expect("the canvas changes");
    let n = 1032 * 1030;
    let request = json!({"op":"paint_blend","layer":0,"color":[0.9, 0.2, 0.1, 1.0],
        "color_space":"document","opacity":1.0,"blend_mode":0,"selection":false});
    let plan = edit_plan(&doc, &request);
    assert_eq!(plan["input_samples"].as_u64(), Some(n as u64));
    let scratch = plan["scratch_bytes"].as_u64().unwrap();
    let coverage = vec![65535u16; n];
    let base = baseline();
    let out = edited(&doc, &request, &coverage);
    let peak = PEAK.load(Ordering::Relaxed) - base;
    let layer = out.layer(0).expect("the record");
    assert_eq!(layer.offset(), (0, 0));
    assert_eq!(
        layer.source().unwrap().dimensions(),
        (1032, 1030),
        "the layer grows"
    );
    let bound = 32 * n + (8 << 20);
    assert!(
        peak <= bound,
        "the grown plane is built into tiles, no adoption copy: peak {peak} > {bound}"
    );
    assert!(peak as u64 <= scratch, "peak {peak} > the plan's {scratch}");
}

#[test]
fn plans_cover_the_measured_peaks() {
    let _serial = serial();
    at_eight_gib();
    for (index, (w, h)) in [(6000, 4000), (10000, 10000)].into_iter().enumerate() {
        let doc = grown_to(w, h);
        let render = doc.render_plan().expect("the render plan").scratch_bytes;
        let flatten = edit_plan(&doc, &json!({"op":"flatten"}))["scratch_bytes"]
            .as_u64()
            .unwrap();
        let reopen = native_color_plan((w, h), &[(w, h)], &[], true).expect("the native plan");
        let reopen = reopen.reopen_retained_bytes + reopen.decode_scratch_bytes;
        for (case, figure) in [("R", render), ("F", flatten), ("O", reopen)] {
            let (_, small, large) = FLOORS.iter().find(|f| f.0 == case).unwrap();
            let floor = if index == 0 { *small } else { *large };
            assert!(figure >= floor, "{case} at {w}×{h}: {figure} < {floor}");
        }
    }
}

#[test]
fn an_added_float_layer_above_the_cache_bound_is_filled() {
    let _serial = serial();
    at_eight_gib();
    let doc = grown_with_added_layer();
    let added = doc.layer_source(1).expect("the added layer's source");
    assert_eq!(added.dimensions(), (EDGE, EDGE));
    assert_eq!(added.shared_tile_count(added), TILES, "the layer is tiled");
    assert!(
        added.retained_bytes() <= 8 << 20,
        "a filled layer retains at most 4 tiles: {} bytes",
        added.retained_bytes()
    );
}

#[test]
#[ignore = "allocates ~4.5 GB; run once per float-plane tiling change"]
fn a_float_raster_above_the_cache_bound_is_admitted_at_8_gib_and_reopens_tiled() {
    let _serial = serial();
    at_eight_gib();
    let n = EDGE as usize * EDGE as usize;
    let srgb = IccProfile::srgb();
    let bytes = {
        let pixels: Vec<[f32; 4]> = (0..n).map(formula).collect();
        float_fixture::drcb(EDGE, EDGE, srgb.bytes(), &pixels)
    };
    let payload = ClipboardPayload::decode(&bytes, LIMIT, 0).expect("the payload decodes");
    drop(bytes);
    let doc = PrecisionDocument::from_clipboard(&payload, LIMIT, 0, None)
        .expect("an 8193² float raster is admitted at 8 GiB");
    drop(payload);
    let image = doc.layer_source(0).expect("the record");
    assert_eq!(image.dimensions(), (EDGE, EDGE));
    assert_eq!(
        image.shared_tile_count(image),
        TILES,
        "an adopted plane is tiled since phase 2"
    );

    let file = doc
        .encode_native(LIMIT, 0, None)
        .expect("the document saves");
    drop(doc);
    let base = baseline();
    let reopened = PrecisionDocument::parse_native(&file, LIMIT, 0, None).expect("it reopens");
    let peak = PEAK.load(Ordering::Relaxed) - base;
    drop(file);
    let image = reopened.layer_source(0).expect("the reopened record");
    assert_eq!(image.shared_tile_count(image), TILES, "the reopen is tiled");
    let at = |x: u32, y: u32| y as usize * EDGE as usize + x as usize;
    for (x, y) in [
        (0, 0),
        (255, 256),
        (8192, 0),
        (0, 8192),
        (8192, 8192),
        (4097, 3001),
    ] {
        assert_eq!(
            image.sample_at(x, y).map(bits),
            Some(bits(formula(at(x, y)))),
            "({x}, {y})"
        );
    }
    let bound = 16 * n + (64 << 20);
    assert!(
        peak <= bound,
        "the reopen holds the tiles beside the file, no adoption copy: peak {peak} > {bound}"
    );

    let ink = vec![[2.0, -0.5, 0.125, 1.0]; 300 * 300];
    let next = image
        .with_rect_rgba32(4000, 5000, 300, 300, &ink)
        .expect("the write is valid")
        .expect("the write changes the image");
    assert_eq!(
        next.shared_tile_count(image),
        TILES - 4,
        "the write shares every untouched tile"
    );
    drop(next);

    let scratch = reopened
        .render_plan()
        .expect("the render plan")
        .scratch_bytes;
    let base = baseline();
    let rendered = reopened
        .render(LIMIT, 0, None)
        .expect("the render is admitted");
    let peak = PEAK.load(Ordering::Relaxed) - base;
    assert_eq!(
        rendered.shared_tile_count(&rendered),
        TILES,
        "the render is tiled"
    );
    assert!(
        peak as u64 <= scratch,
        "the render's plan covers its peak: {peak} > {scratch}"
    );
}

#[test]
#[ignore = "allocates ~2.2 GB; run once per float-plane tiling change"]
fn a_reopened_empty_layer_above_the_cache_bound_is_filled() {
    let _serial = serial();
    at_eight_gib();
    let doc = grown_with_added_layer();
    let file = doc
        .encode_native(LIMIT, 0, None)
        .expect("the document saves");
    drop(doc);
    let reopened = PrecisionDocument::parse_native(&file, LIMIT, 0, None).expect("it reopens");
    drop(file);
    let layer = reopened.layer_source(1).expect("the empty layer");
    assert_eq!(layer.dimensions(), (EDGE, EDGE));
    assert!(
        layer.retained_bytes() <= 8 << 20,
        "a reopened empty layer is filled: {} bytes",
        layer.retained_bytes()
    );
}
