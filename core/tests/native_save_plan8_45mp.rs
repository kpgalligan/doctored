//! The 8-bit native save's heap at gate M's sizes (the documents of
//! `drdc_save_plan`'s gate M, built one after another: 8256×5504 and the
//! 7500×7500 pair). Since 13C-2a the writer streams each plane row by row
//! into the temporary file, so what a save adds beside the document is rows,
//! not the file: this binary's allocator counts the live heap the save adds
//! (a count of bytes, not a footprint or a time, so other work on the machine
//! does not move it) and holds it to the test's own row bound, declared here
//! from the widest plane, and to the plan's `write_scratch_bytes`, which in
//! turn must stay below each noise file it admits. The recovery write into
//! the tile pool is held to the figure it is admitted with, the 8-bit flat
//! PNG save (13C-5, the same row encoder) to the row bound, and a small
//! canvas carrying three 16 MiB metadata packets to the row bound too (the
//! writer puts a slot's payload from the document's own buffer).
//!
//! The open (13C-2b) is counted the same way: `dr_doc_open` reads the file
//! through the stream reader and decodes each plane row by row into tiles,
//! so what an open adds beside the document it returns is a band, not the
//! file (S2), S1's open holds its placed layer and that layer's source plane
//! together for a moment (`drdc_reopen_plan`'s `decode_bytes` covers it),
//! and three 16 MiB metadata packets cost their arena copy and their owned
//! copy. A version-2 file whose raster layer and raw mask are over
//! `MAX_PIXELS` still opens through the whole-plane arms.
//!
//! Alone in its binary: its own `#[global_allocator]`, and each case holds
//! up to ~1.4 GB while it is checked. Every test takes [`SERIAL`] first, so
//! no other test allocates while one counts.

use doctored_core::doc::DrDocument;
use doctored_core::doc_color::MetadataKind;
use doctored_core::ffi::{dr_image_free, dr_image_from_rgba, dr_image_pixels_rgba, dr_string_free};
use doctored_core::ffi::{dr_image_height, dr_image_width};
use doctored_core::ffi_color::dr_doc_save_image;
use doctored_core::ffi_doc::{
    dr_doc_free, dr_doc_from_image, dr_doc_layer_mask_image, dr_doc_open, dr_doc_save_native,
};
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
use std::io::Write;
use std::ptr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use tempfile::TempDir;

#[path = "common/float_fixture.rs"]
mod float_fixture;
#[path = "common/resource_report.rs"]
mod resource_report;

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

/// The test's own bound on what a streamed open of RGBA8 planes at most
/// `widest` pixels wide adds beside the document it returns, declared before
/// measuring: sixteen PNG rows of the widest plane (png's unfiltering window
/// and the row the tile band is filled from) and 8 MiB for the reader's
/// read-ahead, the payload's section buffer, the inflater and the reader's
/// tables.
fn band_bound(widest: u32) -> u64 {
    16 * (4 * u64::from(widest) + 1) + 8 * MIB
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

/// The length-prefixed slots go to the file from the document's own
/// buffers: three 16 MiB metadata packets on a small canvas add rows, not a
/// copy of the packets in the writer's scratch `Vec` (which took all three,
/// at a doubled capacity, before the slots were put straight to the sink).
#[test]
fn a_native_save_puts_metadata_packets_straight_to_the_file() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let packet = vec![0x5A_u8; 16 * MIB as usize];
    let mut doc = DrDocument::from_pixels(noise8(256, 256, 7));
    for kind in [MetadataKind::Exif, MetadataKind::Xmp, MetadataKind::Iptc] {
        doc = doc.set_metadata(kind, Some(&packet)).unwrap().unwrap();
    }
    drop(packet);
    let scratch = doc.native_save_scratch_bytes().unwrap();
    let dir = TempDir::new().unwrap();
    let path = CString::new(dir.path().join("packets.doctored").to_str().unwrap()).unwrap();
    let mut len = 0;
    let added = rise(|| len = save(&doc, &path));
    println!("metadata packets: save added {added} B; plan {scratch} B; file {len} B");
    assert!(len > 48 * MIB, "the file holds the three packets");
    assert!(
        added <= row_bound(256),
        "metadata packets: the save added {added} B, over the row bound {}",
        row_bound(256)
    );
    assert!(
        added <= scratch,
        "metadata packets: the save added {added} B, over the plan's {scratch} B"
    );
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

/// Opens `path` through the export, panicking with its error sentence.
fn open(path: &CString) -> *mut DrDocument {
    let mut error = ptr::null_mut();
    let doc = unsafe { dr_doc_open(path.as_ptr(), ptr::null(), &mut error) };
    if doc.is_null() {
        let message = unsafe { CStr::from_ptr(error) }
            .to_string_lossy()
            .into_owned();
        unsafe { dr_string_free(error) };
        panic!("{message}");
    }
    doc
}

/// The authoritative bytes a document retains: unique minus caches
/// (`native_reopen_plan8::held`).
fn held(doc: *const DrDocument) -> u64 {
    let report = resource_report::report(&[doc], &[], &[], &[], 0, 0).unwrap();
    report["unique_retained_bytes"].as_u64().unwrap() - report["cache_bytes"].as_u64().unwrap()
}

/// Saves `doc`, drops it, then opens the file: the heap the open added at its
/// peak, what the opened document holds, and the reopen plan's `decode_bytes`
/// taken before the save.
fn reopened(name: &str, doc: DrDocument) -> (u64, u64, u64) {
    let decode = doc.native_reopen_plan().unwrap().decode_bytes;
    let dir = TempDir::new().unwrap();
    let path = CString::new(dir.path().join("open.doctored").to_str().unwrap()).unwrap();
    let len = save(&doc, &path);
    drop(doc);
    let mut opened = ptr::null_mut();
    let added = rise(|| opened = open(&path));
    let kept = held(opened);
    unsafe { dr_doc_free(opened) };
    println!("{name}: open added {added} B; holds {kept} B; plan {decode} B; file {len} B");
    (added, kept, decode)
}

/// S2's open adds a band beside the raster it returns: not the file, not a
/// decoded plane.
#[test]
fn open_s2_adds_a_band_not_the_file_at_45_mp() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (added, kept, _) = reopened("S2 open", DrDocument::from_pixels(noise8(W45, H45, 0x5EED)));
    let beside = added.saturating_sub(kept);
    assert!(
        beside <= band_bound(W45),
        "S2 open: {beside} B beside the document, over the band bound {}",
        band_bound(W45)
    );
}

/// S1's open decodes the white Background, the placed layer and the placed
/// layer's 8-bit source before `materialize_byte` adopts the source's tiles
/// for the layer: three planes and a band at its peak, within the plan.
#[test]
fn open_s1_holds_the_placed_layer_and_its_source_at_45_mp() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (added, _, decode) = reopened("S1 open", placed(noise8(W45, H45, 0x5EED)));
    let bound = 3 * 4 * u64::from(W45) * u64::from(H45) + band_bound(W45);
    assert!(
        added <= bound,
        "S1 open: added {added} B, over three planes and a band ({bound} B)"
    );
    assert!(
        added <= decode,
        "S1 open: added {added} B, over the plan's {decode} B"
    );
}

/// Three 16 MiB metadata packets on a small canvas: the open holds each
/// packet's arena copy and its owned copy beside the document, and stays
/// within the plan.
#[test]
fn open_metadata_packets_cost_two_copies() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let packet = vec![0x5A_u8; 16 * MIB as usize];
    let mut doc = DrDocument::from_pixels(noise8(256, 256, 7));
    for kind in [MetadataKind::Exif, MetadataKind::Xmp, MetadataKind::Iptc] {
        doc = doc.set_metadata(kind, Some(&packet)).unwrap().unwrap();
    }
    drop(packet);
    let (added, kept, decode) = reopened("metadata packets open", doc);
    let beside = added.saturating_sub(kept);
    let bound = 2 * 48 * MIB + band_bound(256);
    assert!(
        beside <= bound,
        "metadata packets open: {beside} B beside the document, over {bound} B"
    );
    assert!(
        added <= decode,
        "metadata packets open: added {added} B, over the plan's {decode} B"
    );
}

/// A layer side past `MAX_PIXELS` (10001² = 100,020,001 pixels).
const LEGACY_SIDE: u32 = 10_001;

/// The legacy mask's byte at `(x, y)`.
fn legacy_mask(x: u32, y: u32) -> u8 {
    (x.wrapping_mul(31) ^ y.wrapping_mul(17) ^ (x >> 7)) as u8
}

/// A version-2 file, written here field by field (`drdc_read.rs`'s record
/// order: magic, version, canvas, count; per record the name, offset,
/// opacity, blend, visibility, PNG, mask presence, enable, length, raw
/// bytes and the meta slot), whose one raster layer and its layer-sized raw
/// mask are both over `MAX_PIXELS`, which no version-10 mask may be. The
/// tile store refuses such a plane, so both planes open through their whole
/// arms and stay contiguous; the mask must still read back exactly.
#[test]
fn open_legacy_mask_over_max_pixels() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let side = LEGACY_SIDE as usize;
    let mut png_bytes = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut png_bytes, LEGACY_SIDE, LEGACY_SIDE);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.set_compression(png::Compression::Fast);
        let mut writer = encoder.write_header().unwrap();
        let mut stream = writer.stream_writer().unwrap();
        let row = [40_u8, 90, 160, 255].repeat(side);
        for _ in 0..LEGACY_SIDE {
            stream.write_all(&row).unwrap();
        }
        stream.finish().unwrap();
    }
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("legacy.rz");
    {
        let mut out = std::io::BufWriter::new(std::fs::File::create(&file).unwrap());
        let mut put = |bytes: &[u8]| out.write_all(bytes).unwrap();
        put(b"DRDC");
        put(&2_u32.to_le_bytes());
        put(&64_u32.to_le_bytes());
        put(&64_u32.to_le_bytes());
        put(&1_u32.to_le_bytes());
        put(&6_u32.to_le_bytes());
        put(b"Legacy");
        put(&0_i32.to_le_bytes());
        put(&0_i32.to_le_bytes());
        put(&1.0_f32.to_le_bytes());
        put(&0_u32.to_le_bytes());
        put(&[1]);
        put(&(png_bytes.len() as u32).to_le_bytes());
        put(&png_bytes);
        put(&[1, 1]);
        put(&(LEGACY_SIDE * LEGACY_SIDE).to_le_bytes());
        let mut row = vec![0_u8; side];
        for y in 0..LEGACY_SIDE {
            for (x, byte) in row.iter_mut().enumerate() {
                *byte = legacy_mask(x as u32, y);
            }
            put(&row);
        }
        put(&[0]);
        out.flush().unwrap();
    }
    drop(png_bytes);
    let path = CString::new(file.to_str().unwrap()).unwrap();
    let doc = open(&path);
    let mask = unsafe { dr_doc_layer_mask_image(doc, 0) };
    assert!(!mask.is_null(), "the legacy layer opens with its mask");
    let dimensions = unsafe { (dr_image_width(mask), dr_image_height(mask)) };
    assert_eq!(dimensions, (LEGACY_SIDE, LEGACY_SIDE));
    let pixels = unsafe { std::slice::from_raw_parts(dr_image_pixels_rgba(mask), side * side * 4) };
    let at = |x: u32, y: u32| pixels[(y as usize * side + x as usize) * 4];
    for y in [0, 1, 255, 256, 4999, 5000, LEGACY_SIDE - 2, LEGACY_SIDE - 1] {
        for x in 0..LEGACY_SIDE {
            assert_eq!(at(x, y), legacy_mask(x, y), "mask at ({x}, {y})");
        }
    }
    for (x, y) in [
        (0, 0),
        (LEGACY_SIDE - 1, 0),
        (0, LEGACY_SIDE - 1),
        (LEGACY_SIDE - 1, LEGACY_SIDE - 1),
    ] {
        assert_eq!(at(x, y), legacy_mask(x, y), "corner ({x}, {y})");
    }
    unsafe {
        dr_image_free(mask);
        dr_doc_free(doc);
    }
}
