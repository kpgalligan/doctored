//! The 8-bit native save's heap at gate M's sizes (the documents of
//! `drdc_save_plan`'s gate M, built one after another: 8256×5504 and the
//! 7500×7500 pair). Since 13C-2a the writer streams each plane row by row
//! into the temporary file, so what a save adds beside the document is rows,
//! not the file: this binary's allocator counts the live heap the save adds
//! (a count of bytes, not a footprint or a time, so other work on the machine
//! does not move it) and holds it to the test's own row bound, declared here
//! from the widest plane, and to the plan's `write_scratch_bytes`, which in
//! turn must stay below each noise file it admits. The recovery write into
//! the tile pool is held to the figure it is admitted with, and the 8-bit
//! flat PNG save (13C-5, the same row encoder) to the row bound.
//!
//! Alone in its binary: its own `#[global_allocator]`, and each case holds
//! up to ~1.4 GB while it is checked. Every test takes [`SERIAL`] first, so
//! no other test allocates while one counts.

use doctored_core::doc::DrDocument;
use doctored_core::ffi::{dr_image_free, dr_image_from_rgba, dr_string_free};
use doctored_core::ffi_color::dr_doc_save_image;
use doctored_core::ffi_doc::{dr_doc_free, dr_doc_from_image, dr_doc_save_native};
use doctored_core::ffi_retained_source::dr_doc_mixed_source_edit;
use doctored_core::ffi_tile_pool::{
    dr_doc_tile_pool_write, dr_tile_pool_free, dr_tile_pool_open, dr_tile_pool_window_bytes,
};
use doctored_core::icc::IccProfile;
use doctored_core::precision_image::{set_process_working_limit, REFERENCE_WORKING_BYTES};
use doctored_core::retained_source::SourceEdit;
use image::{Rgba, RgbaImage};
use serde_json::json;
use std::alloc::{GlobalAlloc, Layout, System};
use std::ffi::{CStr, CString};
use std::ptr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use tempfile::TempDir;

#[path = "common/float_fixture.rs"]
mod float_fixture;

const W45: u32 = 8256;
const H45: u32 = 5504;
const MIB: u64 = 1024 * 1024;
/// `DrFormat`'s PNG.
const FORMAT_PNG: i32 = 0;

struct Tracked;
static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);
static SERIAL: Mutex<()> = Mutex::new(());

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

/// The live heap `run` adds at its peak, over what was live before it.
fn rise(run: impl FnOnce()) -> u64 {
    let baseline = LIVE.load(Ordering::Relaxed);
    PEAK.store(baseline, Ordering::Relaxed);
    run();
    (PEAK.load(Ordering::Relaxed) - baseline) as u64
}

/// The test's own bound on what a streamed save of planes at most `widest`
/// pixels wide adds: eight RGBA8 rows of the widest plane (the encoder keeps
/// three, its output block one row's overshoot) and 8 MiB for the sink's
/// buffer, the stored pass's IDAT window, the 64 KiB output block and the
/// writer's tables.
fn row_bound(widest: u32) -> u64 {
    8 * (4 * u64::from(widest) + 1) + 8 * MIB
}

fn splitmix(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// Gate S's noise: RGB from the generator, alpha 255.
fn noise8(w: u32, h: u32, seed: u64) -> RgbaImage {
    let mut state = seed;
    let mut out = RgbaImage::new(w, h);
    for p in out.pixels_mut() {
        let v = splitmix(&mut state);
        p.0 = [v as u8, (v >> 8) as u8, (v >> 16) as u8, 255];
    }
    out
}

/// S5's noise: all four bytes of every pixel from the generator.
fn noise8_full(w: u32, h: u32, seed: u64) -> RgbaImage {
    let mut state = seed;
    let mut out = RgbaImage::new(w, h);
    for p in out.pixels_mut() {
        p.0 = (splitmix(&mut state) as u32).to_le_bytes();
    }
    out
}

/// File ▸ New's white Background with `source` placed over it.
fn placed(source: RgbaImage) -> DrDocument {
    let background = DrDocument::from_pixels(RgbaImage::from_pixel(W45, H45, Rgba([255; 4])));
    let source = DrDocument::from_pixels(source);
    let edit = SourceEdit::parse(
        &json!({"op":"place","source_id":"n","revision_id":"a","name":"Noise","x":0,"y":0})
            .to_string(),
    )
    .unwrap();
    background
        .source_edit(Some(&source), &edit, REFERENCE_WORKING_BYTES, 0, None)
        .unwrap()
        .unwrap()
}

/// Saves `doc` through the export and answers the file's length.
fn save(doc: &DrDocument, path: &CString) -> u64 {
    let mut error = ptr::null_mut();
    let ok = unsafe { dr_doc_save_native(doc, path.as_ptr(), ptr::null(), &mut error) };
    if !error.is_null() {
        let message = unsafe { CStr::from_ptr(error) }
            .to_string_lossy()
            .into_owned();
        unsafe { dr_string_free(error) };
        panic!("{message}");
    }
    assert!(ok);
    std::fs::metadata(path.to_str().unwrap()).unwrap().len()
}

/// One gate M case: the save's heap rise against the row bound and the
/// plan, and the plan below the file.
fn check(name: &str, doc: DrDocument, widest: u32) {
    let scratch = doc.native_save_scratch_bytes().unwrap();
    let dir = TempDir::new().unwrap();
    let path = CString::new(dir.path().join("gate-m.doctored").to_str().unwrap()).unwrap();
    let mut len = 0;
    let added = rise(|| len = save(&doc, &path));
    println!("{name}: save added {added} B; plan {scratch} B; file {len} B");
    assert!(
        added <= row_bound(widest),
        "{name}: the save added {added} B, over the row bound {}",
        row_bound(widest)
    );
    assert!(
        added <= scratch,
        "{name}: the save added {added} B, over the plan's {scratch} B"
    );
    assert!(scratch < len, "{name}: plan {scratch} B ≥ the {len} B file");
}

/// The recovery write (`dr_doc_tile_pool_write`) is admitted at the save's
/// figure plus the pool's window, and its sink cannot rewrite a byte, so
/// each PNG it writes is measured first: S1's 8-bit original must not be
/// staged whole (it was, in the body the former writer built).
#[test]
fn a_recovery_write_adds_rows_not_a_png_at_45_mp() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let doc = placed(noise8(W45, H45, 0x5EED));
    let admitted = doc.native_save_scratch_bytes().unwrap() + dr_tile_pool_window_bytes();
    let dir = TempDir::new().unwrap();
    let folder = CString::new(dir.path().join("pool").to_str().unwrap()).unwrap();
    let skeleton = CString::new(dir.path().join("state.drskel").to_str().unwrap()).unwrap();
    let mut error = ptr::null_mut();
    let pool = unsafe { dr_tile_pool_open(folder.as_ptr(), &mut error) };
    assert!(!pool.is_null());
    let mut ok = false;
    let added = rise(|| {
        ok = unsafe {
            dr_doc_tile_pool_write(&doc, pool, skeleton.as_ptr(), ptr::null(), &mut error)
        };
    });
    unsafe { dr_tile_pool_free(pool) };
    if !error.is_null() {
        let message = unsafe { CStr::from_ptr(error) }
            .to_string_lossy()
            .into_owned();
        unsafe { dr_string_free(error) };
        panic!("{message}");
    }
    assert!(ok);
    println!("S1 recovery write: added {added} B; admitted {admitted} B");
    assert!(
        added <= admitted,
        "S1: the recovery write added {added} B, over the admitted {admitted} B"
    );
}

/// A flat PNG save of S2's pixels, handed in as the composite so nothing is
/// flattened (13C-5): the encoder adds rows, not `png`'s zlib stream (which
/// for noise is at least the stored size, more than `4·w·h`).
#[test]
fn a_flat_png_save_adds_rows_not_the_stream_at_45_mp() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let pixels = noise8(W45, H45, 0x5EED);
    let image = unsafe { dr_image_from_rgba(pixels.as_ptr(), W45, H45) };
    assert!(!image.is_null());
    drop(pixels);
    let doc = unsafe { dr_doc_from_image(image) };
    assert!(!doc.is_null());
    let dir = TempDir::new().unwrap();
    let path = CString::new(dir.path().join("flat.png").to_str().unwrap()).unwrap();
    let mut error = ptr::null_mut();
    let mut carried = 0_u32;
    let mut ok = false;
    let added = rise(|| {
        ok = unsafe {
            dr_doc_save_image(
                doc,
                image,
                path.as_ptr(),
                FORMAT_PNG,
                90,
                true,
                false,
                &mut carried,
                ptr::null(),
                &mut error,
            )
        };
    });
    unsafe {
        dr_doc_free(doc);
        dr_image_free(image);
    }
    if !error.is_null() {
        let message = unsafe { CStr::from_ptr(error) }
            .to_string_lossy()
            .into_owned();
        unsafe { dr_string_free(error) };
        panic!("{message}");
    }
    assert!(ok);
    let len = std::fs::metadata(path.to_str().unwrap()).unwrap().len();
    println!("S2 flat PNG: save added {added} B; file {len} B");
    assert!(
        added <= row_bound(W45),
        "S2 flat PNG: the save added {added} B, over the row bound {}",
        row_bound(W45)
    );
}

/// `float_master_24mp`'s 8-bit case: a 6000×4000 32-bit float master placed
/// with `convert_fallback` into a white 6000×4000 byte document. Its save
/// streams the master's samples and the layers' rows; the heap it adds is
/// held to the row bound and to the plan (whose former floor measured the
/// whole-file writer).
#[test]
fn a_float_master_save_adds_rows_at_24_mp() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    // `float_master_24mp`'s working limit (a 16 GB Mac's), which the place
    // of a 24 MP float master needs.
    const LIMIT: u64 = 8_589_934_592;
    assert!(set_process_working_limit(LIMIT));
    let (w, h) = (6000_u32, 4000_u32);
    let pixels = (0..w * h)
        .map(|i| {
            let (x, y) = (i % w, i / w);
            [
                x as f32 / w as f32,
                y as f32 / h as f32,
                ((x / 8) % 8) as f32 * 0.25,
                1.0,
            ]
        })
        .collect();
    let master = float_fixture::float_doc(w, h, &IccProfile::srgb(), &[pixels]);
    let white = DrDocument::from_pixels(RgbaImage::from_pixel(w, h, Rgba([255; 4])));
    let request = CString::new(
        json!({"op":"place","source_id":"m","name":"Placed","x":0,"y":0,
            "depth_conversion":"convert_fallback"})
        .to_string(),
    )
    .unwrap();
    let mut error = ptr::null_mut();
    let placed = unsafe {
        dr_doc_mixed_source_edit(
            &white,
            ptr::null(),
            &master,
            request.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut error,
        )
    };
    assert!(error.is_null() && !placed.is_null(), "the place failed");
    let doc = *unsafe { Box::from_raw(placed) };
    drop((white, master));
    check("float master", doc, w);
}

#[test]
fn a_native_save_adds_rows_not_the_file_at_45_mp() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    check("S1", placed(noise8(W45, H45, 0x5EED)), W45);
    check("S2", DrDocument::from_pixels(noise8(W45, H45, 0x5EED)), W45);
    check(
        "S2p",
        placed(noise8(W45, H45, 0x5EED))
            .remove_layers(&[0])
            .unwrap(),
        W45,
    );
    check(
        "S3",
        DrDocument::from_pixels(noise8(7500, 7500, 1))
            .adding_image_layer(0, noise8(7500, 7500, 2), "B")
            .unwrap(),
        7500,
    );
    check("S5", placed(noise8_full(W45, H45, 0x5EED)), W45);
}
