//! A typed (16-bit or float) `.rz` may pass 2 GiB. The typed format's
//! file-length cap and its decoded-pixel cap are both 8 GiB, the working
//! limit of the assumed minimum machine, and no byte the writer produces
//! changed (`precision_native`'s `# Size caps`). Since the streamed
//! reader and writer (`native_stream`) neither side charges the file, so at
//! 8 GiB eight 100 MP 16-bit layers save and reopen.
//!
//! The default tests allocate no pixels: the plan is asked through
//! `native_color_plan` and the reader through sparse files. Every figure is
//! this file's own arithmetic, with the caps restated here rather than read
//! from the core: a stored 10000² RGBA16 PNG is 800,071,103 bytes
//! (`native_png_bound.rs`'s formula), a 16-bit plane decodes to 8 bytes a
//! pixel and a float one is 16 bytes a pixel raw.
//!
//! The ignored tests are the proof, run with `make t F=native_file_size
//! T=<name> A="--ignored --nocapture"`: a 10000² 16-bit document of four
//! layers saved and reopened as a file longer than 2^31 bytes at an 8 GiB
//! limit, one of eight layers longer than 2^32 bytes at the same limit, so a
//! stored PNG starts past each boundary and one lies across it, and a
//! two-layer 10000² float document past 2^31. The 16-bit layers are uniform
//! fills (a filled plane retains at most four tiles, and the reader refills a
//! uniform plane) except the two that carry position-dependent samples: the
//! layer whose stored bytes cross the boundary, in a full-width band of rows
//! at the crossing, and the top layer, whose PNG starts past it. The float
//! layers carry position-dependent samples everywhere.
//!
//! `stream_measure` (ignored) is the streamed plans' measured admission,
//! one case per process (`DR_NFS_CASE`); its record is
//! `precision_native_plan`'s `# Streamed reader and writer`.
//!
//! A binary of its own: every test sets the process working limit, and the
//! ignored ones hold several GB. Every test holds `SERIAL`.

use std::ffi::{CStr, CString};
use std::fs::File;
use std::io::Write;
use std::path::Path;
use std::ptr;
use std::sync::{Mutex, MutexGuard};

use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_metadata_report::dr_path_metadata_import_report;
use doctored_core::ffi_resources::dr_set_process_working_limit;
use doctored_core::icc::IccProfile;
use doctored_core::job_control::DrJobControl;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer};
use doctored_core::precision_image::PrecisionImage;
use doctored_core::precision_mask_edit::MaskEdit;
use doctored_core::precision_native::{native_color_plan, open_native_observed};
use serde_json::json;
use tempfile::TempDir;

#[path = "common/float_fixture.rs"]
mod float_fixture;

static SERIAL: Mutex<()> = Mutex::new(());

fn serial() -> MutexGuard<'static, ()> {
    SERIAL.lock().unwrap_or_else(|e| e.into_inner())
}

const GIB: u64 = 1 << 30;
/// Both typed caps, restated: the file's length and its decoded pixels.
const CAP: u64 = 8 * GIB;
const EDGE: u32 = 10_000;
/// The largest plane a document holds: 100,000,000 pixels.
const PLANE: (u32, u32) = (EDGE, EDGE);
/// A 100 MP 16-bit plane's decoded bytes.
const PLANE_BYTES: u64 = 800_000_000;

const TOO_LARGE_16: &str = "At 16 bits the result is too large to save and reopen on its own, \
     so closing other documents will not help. Use a smaller image or fewer layers, masks or \
     channels.";
const TOO_LARGE_32: &str = "At 32 bits the result is too large to save and reopen on its own, \
     so closing other documents will not help. Use a smaller image or fewer layers, masks or \
     channels.";
const REOPEN_16_AT_8_GIB: &str = "At 16 bits the result needs more than the 8 GB working limit \
     to save and reopen on its own, so closing other documents will not help. Use a smaller \
     image or fewer layers, masks or channels.";
const REOPEN_32_AT_8_GIB: &str = "At 32 bits the result needs more than the 8 GB working limit \
     to save and reopen on its own, so closing other documents will not help. Use a smaller \
     image or fewer layers, masks or channels.";
/// The streamed reader's and writer's window (`native_stream`), the
/// bookkeeping allowance and the codec's fixed allowance, restated.
const WINDOW: u64 = 4 << 20;
const BOOKKEEPING: u64 = 1 << 20;
const CODEC_FIXED: u64 = 4 << 20;

fn at_limit(bytes: u64) {
    assert!(dr_set_process_working_limit(bytes));
}

/// The length of a stored RGBA16 PNG, `native_png_bound.rs`'s formula: R row
/// bytes (a filter byte per row), Z the zlib stream (2-byte header, 5 bytes
/// per stored block of at most 65,535, Adler-32), then signature, IHDR, one
/// IDAT per 2^31 − 1 bytes, IEND.
fn png_len(w: u32, h: u32) -> u64 {
    let r = u64::from(h) * (8 * u64::from(w) + 1);
    let z = 2 + r + 5 * r.div_ceil(65_535) + 4;
    8 + (12 + 13) + z + 12 * z.div_ceil((1 << 31) - 1) + 12
}

/// `count` canvas-sized planes.
fn planes(count: usize) -> Vec<(u32, u32)> {
    vec![PLANE; count]
}

/// The process's peak resident memory in bytes (macOS reports `ru_maxrss` in
/// bytes).
fn peak_resident_bytes() -> u64 {
    let mut usage = std::mem::MaybeUninit::<libc::rusage>::zeroed();
    // SAFETY: `getrusage` fills the struct it is handed and returns 0.
    let usage = unsafe {
        assert_eq!(libc::getrusage(libc::RUSAGE_SELF, usage.as_mut_ptr()), 0);
        usage.assume_init()
    };
    usage.ru_maxrss as u64
}

// ---------------------------------------------------------------- the plan --

#[test]
fn at_8_gib_eight_16_bit_planes_plan_and_the_reopen_plan_binds_first() {
    let _serial = serial();
    at_limit(8 * GIB);
    assert_eq!(png_len(EDGE, EDGE), 800_071_103);

    // The streamed reader holds no file. With no header, the 16-bit
    // reopen figure is 1.25 × (retained + held + band + bookkeeping):
    // retained = n · 800,000,000 + 1,048,576, held = the 4 MiB window, the
    // band 16 · (8 · 10000 + 1) + 4 MiB = 5,474,320, so n · 800,000,000 +
    // 11,765,776 before the quarter. Eight planes: 8,014,707,220 ≤
    // 8,589,934,592; nine: 9,014,707,220, over.
    let eight = native_color_plan(PLANE, &planes(8), &[], false).expect("eight planes plan");
    assert_eq!(eight.encoded_bound, 8 * png_len(EDGE, EDGE));
    assert!(eight.encoded_bound > 1 << 32, "the file passes 4 GiB");
    assert_eq!(eight.serialized_pixel_bytes, 8 * PLANE_BYTES);
    let retained = 8 * PLANE_BYTES + BOOKKEEPING;
    let band = 16 * (8 * 10_000 + 1) + CODEC_FIXED;
    let base = retained + WINDOW + band + BOOKKEEPING;
    assert_eq!(base, 6_411_765_776);
    assert_eq!(
        eight.reopen_retained_bytes + eight.decode_scratch_bytes,
        base + base.div_ceil(4)
    );
    assert!(eight.reopen_retained_bytes + eight.decode_scratch_bytes <= 8 * GIB);
    // The save holds no file either: twice the (empty) header, one row of
    // 8 · 10000 bytes and its 4 MiB allowance, the window and bookkeeping,
    // and a quarter.
    let write = 8 * 10_000 + CODEC_FIXED + WINDOW + BOOKKEEPING;
    assert_eq!(eight.write_scratch_bytes, write + write.div_ceil(4));

    // Nine are under both caps (7.2 GB each way); the reopen plan refuses.
    assert!(9 * png_len(EDGE, EDGE) <= CAP && 9 * PLANE_BYTES <= CAP);
    assert_eq!(
        native_color_plan(PLANE, &planes(9), &[], false).unwrap_err(),
        REOPEN_16_AT_8_GIB
    );

    // A float raster's reopen figure is retained + held + bookkeeping +
    // `FLOAT_REOPEN_MARGIN_PER_PIXEL` (9) per pixel: n · (1,600,000,000 +
    // 900,000,000) + 1,048,576 + (16 · n + 4 MiB) + 1,048,576. Three
    // planes: 7,506,291,504 ≤ 8,589,934,592; four: 10,006,291,520, over.
    let three = native_color_plan(PLANE, &planes(3), &[], true).expect("three float planes plan");
    assert_eq!(
        three.reopen_retained_bytes + three.decode_scratch_bytes,
        3 * 2_500_000_000 + BOOKKEEPING + 3 * 16 + WINDOW + BOOKKEEPING
    );
    assert_eq!(
        native_color_plan(PLANE, &planes(4), &[], true).unwrap_err(),
        REOPEN_32_AT_8_GIB
    );
}

#[test]
fn the_decoded_cap_is_8_gib_at_both_depths() {
    let _serial = serial();
    at_limit(64 * GIB);

    // Each refused case below is past the file cap too, so this test holds
    // the decoded cap only from below; the reader pins it to the pixel
    // (`precision_document.rs`, `all_records_and_aggregate_planes_are_checked_before_pixel_decoding`:
    // 2^30 pixels admitted, one row more refused).
    let ten = native_color_plan(PLANE, &planes(10), &[], false).expect("ten 16-bit planes plan");
    assert_eq!(ten.serialized_pixel_bytes, 8_000_000_000);
    assert_eq!(ten.encoded_bound, 10 * png_len(EDGE, EDGE));
    assert!(ten.encoded_bound <= CAP);
    // Eleven are 8.8 GB decoded, past the cap.
    const { assert!(11 * PLANE_BYTES > CAP) };
    assert_eq!(
        native_color_plan(PLANE, &planes(11), &[], false).unwrap_err(),
        TOO_LARGE_16
    );

    // A float raster is raw: 16 bytes a pixel, behind two u32 dimensions and
    // a u64 length.
    let five = native_color_plan(PLANE, &planes(5), &[], true).expect("five float planes plan");
    assert_eq!(five.serialized_pixel_bytes, 8_000_000_000);
    assert_eq!(five.encoded_bound, 5 * (16 + 2 * PLANE_BYTES));
    assert!(five.encoded_bound <= CAP);
    // Six are 9.6 GB decoded, past the cap.
    const { assert!(6 * 2 * PLANE_BYTES > CAP) };
    assert_eq!(
        native_color_plan(PLANE, &planes(6), &[], true).unwrap_err(),
        TOO_LARGE_32
    );
}

#[test]
fn the_file_cap_is_8_gib() {
    let _serial = serial();
    at_limit(64 * GIB);
    // Ten full planes and an eleventh 8192 px wide. At 8990 rows the file is
    // 914 bytes under the cap; at 8991 it is 64,628 over. The decoded pixels
    // are under their cap both times, so the file's length is what decides.
    let with_last = |rows: u32| {
        let mut rasters = planes(10);
        rasters.push((8192, rows));
        let file = 10 * png_len(EDGE, EDGE) + png_len(8192, rows);
        let decoded = 10 * PLANE_BYTES + 8 * 8192 * u64::from(rows);
        (
            native_color_plan(PLANE, &rasters, &[], false),
            file,
            decoded,
        )
    };

    let (plan, file, decoded) = with_last(8990);
    assert_eq!(file, 8_589_933_678);
    assert!(file <= CAP && decoded <= CAP);
    let plan = plan.expect("a file 914 bytes under the cap plans");
    assert_eq!(plan.encoded_bound, file);
    assert_eq!(plan.serialized_pixel_bytes, decoded);

    let (plan, file, decoded) = with_last(8991);
    assert_eq!(file, 8_589_999_220);
    assert!(file > CAP && decoded <= CAP);
    assert_eq!(plan.unwrap_err(), TOO_LARGE_16);
}

// -------------------------------------------------------------- the reader --

#[test]
fn the_reader_refuses_a_file_past_8_gib_and_charges_only_its_window() {
    let _serial = serial();
    at_limit(8 * GIB);
    let dir = TempDir::new().unwrap();
    // Sparse: a length and no data.
    let sparse = |length: u64| {
        let path = dir.path().join(format!("{length}.rz"));
        File::create(&path).unwrap().set_len(length).unwrap();
        path.to_str().unwrap().to_string()
    };
    let open = |path: &str, limit: u64| {
        PrecisionDocument::open_native(path, limit, 0, None)
            .map(|_| ())
            .unwrap_err()
    };
    assert_eq!(
        open(&sparse(CAP + 1), 1 << 20),
        "high-depth native file exceeds the encoded size limit"
    );
    // Up to the cap, the streamed reader asks the caller's limit (1 MiB
    // here) for its window and bookkeeping alone, whatever the length.
    for length in [CAP, 2 * GIB + 1, 64] {
        let refused = open(&sparse(length), 1 << 20);
        assert!(refused.contains("working limit"), "{length}: {refused}");
        assert!(
            refused.contains(&format!("needs {} bytes", WINDOW + BOOKKEEPING)),
            "{length}: {refused}"
        );
    }
    // At 8 GiB a file of the cap's length is read from its start: zeros are
    // not a typed document, and nothing near its length is held. (The guard
    // proves nothing once the process has already peaked above about 1 GiB,
    // as it has after an ignored case or an allocating test in this binary.)
    let before = peak_resident_bytes();
    assert_eq!(
        open(&sparse(CAP), 8 * GIB),
        "not a high-depth DRDC document"
    );
    let grew = peak_resident_bytes() - before;
    assert!(
        grew < GIB / 4,
        "the reader buffered the file: peak resident memory grew {grew} bytes"
    );
}

/// `dr_path_metadata_import_report`'s JSON for `path`.
fn import_report(path: &Path) -> serde_json::Value {
    let c_path = CString::new(path.to_str().unwrap()).unwrap();
    let mut error = ptr::null_mut();
    let raw = unsafe { dr_path_metadata_import_report(c_path.as_ptr(), &mut error) };
    assert!(!raw.is_null() && error.is_null(), "the report is produced");
    let report = serde_json::from_slice(unsafe { CStr::from_ptr(raw) }.to_bytes()).unwrap();
    unsafe { dr_string_free(raw) };
    report
}

#[test]
fn a_native_file_s_metadata_report_reads_only_its_signature() {
    let _serial = serial();
    at_limit(8 * GIB);
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("typed.rz");
    // The signature, then a sparse GiB: a report that read the file whole
    // would touch all of it.
    let mut file = File::create(&path).unwrap();
    file.write_all(b"RZDC").unwrap();
    file.set_len(GIB).unwrap();
    drop(file);

    // The guard proves nothing once the process has already peaked above
    // about 1 GiB (after an ignored case, or an allocating test added here).
    let before = peak_resident_bytes();
    let report = import_report(&path);
    let grew = peak_resident_bytes() - before;

    assert_eq!(report["container"], "native");
    assert_eq!(report["metadata_walked"], true);
    assert_eq!(report["warnings"], serde_json::json!([]));
    assert!(report.get("resolution").is_none() && report.get("profile_refusal").is_none());
    assert!(
        grew < GIB / 4,
        "the report buffered the file: peak resident memory grew {grew} bytes"
    );
}

// ----------------------------------------------------- the ignored fixture --

/// Layer `k`'s uniform fill; every layer's differs.
fn fill(k: usize) -> [u16; 4] {
    let k = k as u16;
    [1_000 + 37 * k, 2_000 + 91 * k, 3_000 + 53 * k, 65_535 - k]
}

/// Layer `k`'s position-dependent sample at canvas pixel `(x, y)`.
fn formula(k: usize, x: u32, y: u32) -> [u16; 4] {
    [
        (x.wrapping_mul(7).wrapping_add(y.wrapping_mul(13)) as u16).wrapping_add(1_009 * k as u16),
        ((x ^ y) as u16).wrapping_add(k as u16),
        (u64::from(x) * u64::from(y) % 65_521) as u16,
        65_535 - k as u16,
    ]
}

/// The record (bottom-first) whose stored PNG lies across file offset
/// `boundary`, and the image row stored there. A record is its u64 length
/// word and its PNG; the file's header, each record's own few dozen bytes
/// and the stored-block headers move the row by less than one, and the band
/// written around it is 100 rows.
fn crossing(boundary: u64) -> (usize, u32) {
    let record = 8 + png_len(EDGE, EDGE);
    let layer = boundary / record;
    let row = (boundary - layer * record) / (8 * u64::from(EDGE) + 1);
    (layer as usize, row as u32)
}

/// The rects `(x, y, w, h)` of layer `k` that hold `formula`, in a document
/// of `layers`: the band at the crossing in its layer, and two corner blocks
/// and an interior block in the top layer. Every other layer is its fill.
fn formula_rects(k: usize, layers: usize, band: (usize, u32)) -> Vec<(u32, u32, u32, u32)> {
    if k == band.0 {
        vec![(0, band.1 - 50, EDGE, 100)]
    } else if k == layers - 1 {
        vec![
            (0, 0, 64, 64),
            (EDGE - 64, EDGE - 64, 64, 64),
            (3_000, 4_000, 300, 200),
        ]
    } else {
        Vec::new()
    }
}

/// What layer `k` holds at `(x, y)`, by this file's rule alone.
fn expected(k: usize, layers: usize, band: (usize, u32), x: u32, y: u32) -> [u16; 4] {
    let inside = formula_rects(k, layers, band)
        .into_iter()
        .any(|(rx, ry, w, h)| (rx..rx + w).contains(&x) && (ry..ry + h).contains(&y));
    if inside {
        formula(k, x, y)
    } else {
        fill(k)
    }
}

/// A 10000² 16-bit document of `layers` canvas-sized layers.
fn document(layers: usize, band: (usize, u32)) -> PrecisionDocument {
    let profile = IccProfile::srgb();
    let records = (0..layers)
        .map(|k| {
            let mut image = PrecisionImage::filled_rgba16(EDGE, EDGE, fill(k), profile.clone())
                .expect("a filled plane");
            for (x, y, w, h) in formula_rects(k, layers, band) {
                let samples: Vec<[u16; 4]> = (y..y + h)
                    .flat_map(|py| (x..x + w).map(move |px| formula(k, px, py)))
                    .collect();
                image = image
                    .with_rect_rgba16(x, y, w, h, &samples)
                    .expect("the write is valid")
                    .expect("the write changes the layer");
            }
            PrecisionDocumentLayer::raster(&format!("Layer {k}"), image, (0, 0), 1.0, true)
                .expect("a raster record")
        })
        .collect();
    PrecisionDocument::from_parts(
        EDGE,
        EDGE,
        profile,
        Metadata::default(),
        Resolution::default(),
        records,
    )
    .expect("the plan admits the document")
}

/// Saves a `layers`-layer document at `limit`, drops it, reopens the file and
/// reads every layer back. The file must be longer than `boundary`.
fn saves_and_reopens(layers: usize, limit: u64, boundary: u64) {
    at_limit(limit);
    let band = crossing(boundary);
    assert!(
        band.0 < layers - 1,
        "a layer above the crossing starts past it"
    );
    let document = document(layers, band);
    let bound = document.native_plan().expect("the plan").encoded_bound;
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("large.rz");
    let path = path.to_str().unwrap();
    document
        .save_native(path, limit, 0, None)
        .expect("the document saves");
    drop(document);

    let length = std::fs::metadata(path).unwrap().len();
    assert!(
        length > boundary,
        "the file is {length} bytes, not past {boundary}"
    );
    assert!(
        length <= bound,
        "the file is {length} bytes, over its bound {bound}"
    );

    let reopened = PrecisionDocument::open_native(path, limit, 0, None).expect("the file reopens");
    eprintln!(
        "native_file_size: {layers} layers, file {length} bytes (bound {bound}), \
         peak resident {} bytes",
        peak_resident_bytes()
    );
    assert_eq!(reopened.dimensions(), PLANE);
    assert_eq!(reopened.layer_count(), layers);
    let row = band.1;
    let points = [
        (0, 0),
        (63, 63),
        (64, 64),
        (EDGE - 1, EDGE - 1),
        (EDGE - 64, EDGE - 64),
        (EDGE - 65, EDGE - 65),
        (EDGE - 1, 0),
        (0, EDGE - 1),
        (3_000, 4_000),
        (3_299, 4_199),
        (3_150, 4_100),
        (2_999, 4_000),
        (5_000, 5_000),
        (0, row - 50),
        (EDGE - 1, row + 49),
        (0, row),
        (EDGE - 1, row),
        (5_000, row),
        (5_000, row - 51),
        (5_000, row + 50),
    ];
    for k in 0..layers {
        let image = reopened.layer_source(k).expect("a raster record");
        assert_eq!(image.dimensions(), PLANE, "layer {k}");
        for (x, y) in points {
            assert_eq!(
                image.rgba16_at(x, y),
                Some(expected(k, layers, band, x, y)),
                "layer {k} at ({x}, {y})"
            );
        }
    }
}

#[test]
#[ignore = "writes a ~3.2 GB file; run once per size-cap change"]
fn a_typed_document_over_2_gib_saves_and_reopens() {
    let _serial = serial();
    saves_and_reopens(4, 8 * GIB, 1 << 31);
}

#[test]
#[ignore = "writes a ~6.4 GB file at the 8 GiB limit; run once per size-cap change"]
fn eight_100_mp_16_bit_layers_save_and_reopen_at_8_gib() {
    let _serial = serial();
    saves_and_reopens(8, 8 * GIB, 1 << 32);
}

/// Float layer `k`'s sample at `(x, y)`: position-dependent in every
/// channel but alpha, and different in each layer.
fn float_formula(k: usize, x: u32, y: u32) -> [f32; 4] {
    [
        x as f32 / 10_000.0 + k as f32,
        y as f32 / 10_000.0 - 0.5 * k as f32,
        ((x * 7 + y * 13 + 101 * k as u32) % 1021) as f32 / 1021.0,
        1.0,
    ]
}

#[test]
#[ignore = "holds ~12 GB building two 100 MP float layers and writes a ~3.2 GB file; run once per size-cap change"]
fn two_100_mp_float_layers_save_and_reopen_at_8_gib() {
    let _serial = serial();
    let limit = 8 * GIB;
    at_limit(limit);
    let records: Vec<Vec<[f32; 4]>> = (0..2)
        .map(|k| {
            (0..EDGE * EDGE)
                .map(|i| float_formula(k, i % EDGE, i / EDGE))
                .collect()
        })
        .collect();
    let document = float_fixture::float_doc_at(limit, EDGE, EDGE, &IccProfile::srgb(), &records);
    drop(records);
    let bound = document.native_plan().expect("the plan").encoded_bound;
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("float.rz");
    let path = path.to_str().unwrap();
    document
        .save_native(path, limit, 0, None)
        .expect("the document saves");
    drop(document);

    let length = std::fs::metadata(path).unwrap().len();
    assert!(
        length > 1 << 31,
        "the file is {length} bytes, not past 2^31"
    );
    assert!(
        length <= bound,
        "the file is {length} bytes, over its bound {bound}"
    );
    let reopened = PrecisionDocument::open_native(path, limit, 0, None).expect("the file reopens");
    eprintln!(
        "native_file_size: 2 float layers, file {length} bytes (bound {bound}), \
         peak resident {} bytes",
        peak_resident_bytes()
    );
    assert!(reopened.is_float());
    assert_eq!(reopened.layer_count(), 2);
    let points = [
        (0, 0),
        (1, 0),
        (0, 1),
        (EDGE - 1, EDGE - 1),
        (EDGE - 1, 0),
        (0, EDGE - 1),
        (255, 256),
        (256, 255),
        (4_097, 3_001),
        (9_000, 7_777),
        (5_000, 5_000),
    ];
    for k in 0..2 {
        let image = reopened.layer_source(k).expect("a raster record");
        assert_eq!(image.dimensions(), PLANE, "layer {k}");
        for (x, y) in points {
            assert_eq!(
                image.sample_at(x, y).map(|p| p.map(f32::to_bits)),
                Some(float_formula(k, x, y).map(f32::to_bits)),
                "layer {k} at ({x}, {y})"
            );
        }
    }
}

// ------------------------------------------------------------ measurement --

#[repr(C)]
struct RusageInfoV4 {
    uuid: [u8; 16],
    fields: [u64; 35],
}
const PHYS_FOOTPRINT: usize = 7;
const INTERVAL_MAX_PHYS_FOOTPRINT: usize = 33;

extern "C" {
    fn proc_pid_rusage(pid: i32, flavor: i32, buffer: *mut RusageInfoV4) -> i32;
    fn proc_reset_footprint_interval(pid: i32) -> i32;
}

fn rusage() -> RusageInfoV4 {
    let mut info = RusageInfoV4 {
        uuid: [0; 16],
        fields: [0; 35],
    };
    let rc = unsafe { proc_pid_rusage(std::process::id() as i32, 4, &mut info) };
    assert_eq!(rc, 0, "proc_pid_rusage");
    info
}

/// xorshift64 noise, one u16 per sample.
fn noise_samples(count: usize, seed: u64) -> Vec<u16> {
    let mut state = seed;
    (0..count)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            (state >> 16) as u16
        })
        .collect()
}

/// The 16-bit measured document: 6000×4000, two xorshift-noise layers, a
/// noise mask on the upper one and a noise channel.
fn measured16(limit: u64) -> PrecisionDocument {
    let (w, h) = (6000_u32, 4000_u32);
    let n = w as usize * h as usize;
    let layer = |k: usize, seed: u64| {
        let samples = noise_samples(4 * n, seed);
        let pixels = samples
            .chunks_exact(4)
            .map(|c| [c[0], c[1], c[2], c[3]])
            .collect();
        let image = PrecisionImage::from_rgba16(w, h, pixels, IccProfile::srgb(), None).unwrap();
        PrecisionDocumentLayer::raster(&format!("Noise {k}"), image, (0, 0), 1.0, true).unwrap()
    };
    let doc = PrecisionDocument::from_parts(
        w,
        h,
        IccProfile::srgb(),
        Metadata::default(),
        Resolution::default(),
        vec![
            layer(0, 0x2545_F491_4F6C_DD1D),
            layer(1, 0x9E37_79B9_7F4A_7C15),
        ],
    )
    .unwrap();
    let mask_edit = |doc: &PrecisionDocument, request: serde_json::Value, samples: &[u16]| {
        doc.mask_edit(
            &MaskEdit::parse(&request.to_string()).unwrap(),
            samples,
            limit,
            0,
            None,
        )
        .unwrap()
        .unwrap()
    };
    let masked = mask_edit(
        &doc,
        json!({"op":"set_mask","layer":1,"width":w,"height":h,"x":0,"linked":false,
            "density":1.0,"feather":0.0,"enabled":true}),
        &noise_samples(n, 0x1234_5678_9ABC_DEF1),
    );
    mask_edit(
        &masked,
        json!({"op":"set_channel","index":null,"name":"Noise"}),
        &noise_samples(n, 0x0F1E_2D3C_4B5A_6978),
    )
}

/// `float_above_cache_bound.rs`'s sample formula at alpha 1.
fn opaque_formula(i: usize) -> [f32; 4] {
    [
        (i % 251) as f32 / 251.0,
        (i % 509) as f32 / 509.0,
        (i % 1021) as f32 / 1021.0,
        1.0,
    ]
}

/// The float measured document: one opaque `formula` layer, `side` wide.
fn measured32(limit: u64, w: u32, h: u32) -> PrecisionDocument {
    let pixels = (0..w as usize * h as usize).map(opaque_formula).collect();
    float_fixture::float_doc_at(limit, w, h, &IccProfile::srgb(), &[pixels])
}

/// One case per process, selected by `DR_NFS_CASE`: `base16`, `save16`,
/// `reopen16` (the 16-bit document), `base32`, `save32`, `reopen32` (the
/// 6000×4000 float document) and `reopen32l` (10000×10000). The document is
/// built in setup (for a reopen, built and saved by a child process,
/// `setup_in_child`); then the
/// footprint is read, the interval peak reset, the measured call run with
/// its result held until E = interval peak − pre is read. H is the plan's
/// figure; A is what the call admitted: a save's `write_scratch_bytes`, a
/// reopen's reservation read inside `open_native_observed`'s hook (its
/// retained bytes plus its scratch). A `base` twin runs nothing.
#[test]
#[ignore = "measurement: DR_NFS_CASE=base16|save16|reopen16|base32|save32|reopen32|reopen32l, Release, MallocLargeCache=0"]
fn stream_measure() {
    let _serial = serial();
    let case = std::env::var("DR_NFS_CASE").expect("DR_NFS_CASE");
    let limit = 8 * GIB;
    at_limit(limit);
    let build = || match case.as_str() {
        "base16" | "save16" | "reopen16" => measured16(limit),
        "base32" | "save32" | "reopen32" => measured32(limit, 6000, 4000),
        "reopen32l" => measured32(limit, 10_000, 10_000),
        other => panic!("unknown DR_NFS_CASE {other}"),
    };
    let reopen = case.starts_with("reopen");
    if let Ok(path) = std::env::var("DR_NFS_SETUP") {
        // The setup child (`setup_in_child`): save the file and its H.
        let doc = build();
        let plan = doc.native_plan().expect("the plan");
        doc.save_native(&path, limit, 0, None)
            .expect("the setup save");
        let h = plan.reopen_retained_bytes + plan.decode_scratch_bytes;
        std::fs::write(format!("{path}.h"), h.to_string()).unwrap();
        return;
    }
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join("measured.rz");
    let path = path.to_str().unwrap();
    let (doc, h, mut a) = if reopen {
        setup_in_child("DR_NFS_SETUP", path);
        let h = std::fs::read_to_string(format!("{path}.h"))
            .unwrap()
            .parse::<u64>()
            .unwrap();
        (None, h, 0)
    } else {
        let doc = build();
        let write = doc.native_plan().expect("the plan").write_scratch_bytes;
        (Some(doc), write, write)
    };
    let control = DrJobControl::default();
    let pre = rusage().fields[PHYS_FOOTPRINT];
    unsafe { proc_reset_footprint_interval(std::process::id() as i32) };
    let held = if reopen {
        Some(
            open_native_observed(path, limit, 0, Some(&control), &mut || {
                a = control.scratch_bytes();
            })
            .expect("the reopen is admitted"),
        )
    } else if case.starts_with("save") {
        doc.as_ref()
            .unwrap()
            .save_native(path, limit, 0, None)
            .expect("the save is admitted");
        None
    } else {
        None
    };
    let peak = rusage().fields[INTERVAL_MAX_PHYS_FOOTPRINT];
    drop(held);
    let e = peak.saturating_sub(pre);
    println!(
        "NFS_MEASURE case={case} H={h} A={a} pre={pre} peak={peak} E={e} floor={}",
        (e * 5).div_ceil(4)
    );
    assert!(h >= a, "{case}: H {h} < A {a}");
}

/// Runs this binary's `stream_measure` again as a child with `DR_NFS_SETUP`
/// set to `path`: the child builds the case's document and saves it there,
/// with its plan figure H beside it, so the measured process has never held
/// the document (a freed document's tiles stay in the allocator's free
/// lists, counted in the footprint and reused by the reopen, which would
/// then read short).
fn setup_in_child(variable: &str, path: &str) {
    let status = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--ignored", "--exact", "stream_measure", "--nocapture"])
        .env(variable, path)
        .status()
        .unwrap();
    assert!(status.success(), "the setup child");
}
