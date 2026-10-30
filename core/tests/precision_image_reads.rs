//! Black-box suite for 13B-3c-1: the typed read primitives that read straight
//! from the tiles — `PrecisionImage::row_segments`, `sample_at` and
//! `runs_rgba16` / `runs_rgba32` (the typed twins of `LayerRaster`'s 13B-2c-1
//! byte reads) — and the view facts of the reads moved onto them (the typed
//! compositor, the regional tree and the geometry comparison).
//!
//! Oracles are independent of the implementation: the expected samples are
//! the row-major `Vec` built by a closure BEFORE the constructor adopts it,
//! sliced by hand; a u16 sample's expected read is `f32::from(c) / 65535.0`
//! written out here; tile columns are `x / 256`.
//!
//! The view cache is process-wide, so every test holds `serial()` and a view
//! count is taken after `evict()`.

use std::ffi::{c_char, CStr, CString};
use std::ptr;

use doctored_core::contiguous_views::contiguous_view_bytes;
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_precision::{
    dr_precision_doc_render, dr_precision_image_free, dr_precision_image_height,
    dr_precision_image_width,
};
use doctored_core::ffi_precision_edit::dr_precision_doc_edit;
use doctored_core::ffi_regional::dr_precision_doc_render_region;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer as Layer};
use doctored_core::precision_geometry::GeometryEdit;
use doctored_core::precision_image::{PrecisionImage, PrecisionRun, REFERENCE_WORKING_BYTES};
use doctored_core::tiff_float::FloatTiff;
use serde_json::{json, Value};

use crate::resource_report;
use crate::serial;

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
const W: u32 = 600;
const H: u32 = 520;

// ------------------------------------------------------------- plumbing --

fn pattern16(width: u32, height: u32, seed: u32) -> Vec<[u16; 4]> {
    (0..width * height)
        .map(|i| {
            let (x, y) = (i % width, i / width);
            [
                ((x * 2711 + y * 911 + seed * 1234) % 65536) as u16,
                ((x * 523 + y * 3301 + seed * 77) % 65536) as u16,
                (((x ^ y) * 1999 + seed) % 65536) as u16,
                (9000 + (x * 3001 + y * 1777 + seed * 97) % 56536) as u16,
            ]
        })
        .collect()
}

/// Normalized samples (what `from_rgba32` admits).
fn pattern32(width: u32, height: u32, seed: u32) -> Vec<[f32; 4]> {
    (0..width * height)
        .map(|i| {
            let (x, y) = (i % width, i / width);
            let v = ((x * 37 + y * 101 + seed * 13) % 1000) as f32 / 1000.0;
            [
                v,
                1.0 - v,
                ((x ^ y) % 97) as f32 / 96.0,
                0.2 + 0.8 * (((x + 3 * y + seed) % 64) as f32 / 63.0),
            ]
        })
        .collect()
}

/// Extended-range samples (negative and above-white RGB) for a float TIFF.
fn extended32(width: u32, height: u32, seed: u32) -> Vec<f32> {
    pattern32(width, height, seed)
        .into_iter()
        .flat_map(|[r, g, b, a]| [r * 2.5 - 0.5, g, b * 1.5, a])
        .collect()
}

fn image16(width: u32, height: u32, pixels: Vec<[u16; 4]>) -> PrecisionImage {
    PrecisionImage::from_rgba16(width, height, pixels, IccProfile::srgb(), None).unwrap()
}

fn image32(width: u32, height: u32, pixels: Vec<[f32; 4]>) -> PrecisionImage {
    PrecisionImage::from_rgba32(width, height, pixels, IccProfile::srgb(), None).unwrap()
}

/// The hand-normalized u16 read (`f32::from(c) / 65535.0`), as bits.
fn norm16(p: [u16; 4]) -> [u32; 4] {
    p.map(|c| (f32::from(c) / 65535.0).to_bits())
}

fn bits32(p: [f32; 4]) -> [u32; 4] {
    p.map(f32::to_bits)
}

fn take(p: *mut c_char) -> String {
    if p.is_null() {
        return "<no error message>".into();
    }
    let text = unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned();
    unsafe { dr_string_free(p) };
    text
}

fn evict() {
    resource_report::evict(&[]).expect("an eviction report");
}

/// The contiguous views held over `documents` (the report's top-level
/// `contiguous_view_bytes`).
fn document_views(documents: &[&PrecisionDocument]) -> u64 {
    let handles: Vec<*const PrecisionDocument> = documents.iter().map(|d| *d as *const _).collect();
    let value = resource_report::report(&[], &[], &handles, &[], 0, 0).expect("a resource report");
    value["contiguous_view_bytes"].as_u64().unwrap()
}

fn edit(doc: &PrecisionDocument, request: Value) -> PrecisionDocument {
    let text = CString::new(request.to_string()).unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_edit(
            doc,
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

fn geometry(doc: &PrecisionDocument, request: Value) -> Option<PrecisionDocument> {
    let parsed = GeometryEdit::parse(&request.to_string()).expect("a geometry request");
    doc.geometry(&parsed, LIMIT, 0, None)
        .expect("the edit runs")
}

/// A float document holding one `width`×`height` raster of `samples` (RGBA,
/// straight alpha), through a float TIFF opened with `open_preserving`.
fn float_document(width: u32, height: u32, samples: Vec<f32>) -> PrecisionDocument {
    let tiff = FloatTiff {
        width,
        height,
        channels: 4,
        alpha: Some(false),
        samples,
        icc: None,
        resolution: None,
        metadata: Metadata::default(),
        non_finite: 0,
    };
    let bytes = doctored_core::tiff_float::encode_to_vec(&tiff, false).expect("float TIFF");
    let directory = tempfile::TempDir::new().unwrap();
    let path = directory.path().join("reads.tiff");
    std::fs::write(&path, &bytes).unwrap();
    let doc = PrecisionDocument::open_preserving(path.to_str().unwrap(), LIMIT, 0, None)
        .expect("the float document opens");
    assert!(doc.is_float());
    doc
}

// ------------------------------------------------ (a) row_segments rules --

const ROWS: [u32; 4] = [0, 255, 256, 519];
const SPANS: [(u32, u32); 7] = [
    (0, W),
    (250, 262),
    (255, 256),
    (599, 600),
    (0, 10000),
    (300, 300),
    (400, 300),
];

/// One row span's runs checked against the hand slice: adjoining, each
/// inside one tile column, concatenating to `source[y·w + x0 .. y·w +
/// min(x1, w)]`, and every `pixel(j)` bitwise the hand-normalized sample.
fn check_span<T: Copy>(
    image: &PrecisionImage,
    source: &[T],
    (y, x0, x1): (u32, u32, u32),
    expect: impl Fn(T) -> [u32; 4],
    run_bits: impl Fn(&PrecisionRun<'_>, usize) -> Option<[u32; 4]>,
) {
    let (w, h) = image.dimensions();
    let end = x1.min(w);
    let wanted: Vec<[u32; 4]> = if y < h && x0 < end {
        source[(y * w + x0) as usize..(y * w + end) as usize]
            .iter()
            .map(|p| expect(*p))
            .collect()
    } else {
        Vec::new()
    };
    let mut next = x0;
    let mut got = Vec::new();
    for (x, run) in image.row_segments(y, x0, x1) {
        assert_eq!(x, next, "row {y} span {x0}..{x1}: runs adjoin");
        assert!(!run.is_empty(), "no empty run");
        assert_eq!(
            x / 256,
            (x + run.len() as u32 - 1) / 256,
            "row {y} span {x0}..{x1}: a run stays inside one tile column"
        );
        for j in 0..run.len() {
            let raw = run_bits(&run, j).expect("the run's variant is the storage");
            assert_eq!(
                bits32(run.pixel(j)),
                raw,
                "pixel(j) is the normalized sample"
            );
            got.push(raw);
        }
        next = x + run.len() as u32;
    }
    assert_eq!(got, wanted, "row {y} span {x0}..{x1}");
}

#[test]
fn row_segments_follow_the_tile_rules_on_both_storages() {
    let _serial = serial();
    let source16 = pattern16(W, H, 3);
    let image = image16(W, H, source16.clone());
    let source32 = pattern32(W, H, 5);
    let float = image32(W, H, source32.clone());
    for y in ROWS.into_iter().chain([H]) {
        for (x0, x1) in SPANS {
            check_span(&image, &source16, (y, x0, x1), norm16, |run, j| match run {
                PrecisionRun::Rgba16(s) => Some(norm16(s[j])),
                PrecisionRun::Rgba32(_) => None,
            });
            check_span(&float, &source32, (y, x0, x1), bits32, |run, j| match run {
                PrecisionRun::Rgba32(s) => Some(bits32(s[j])),
                PrecisionRun::Rgba16(_) => None,
            });
        }
    }
    assert_eq!(
        image.row_segments(H, 0, W).count(),
        0,
        "y = height is empty"
    );
    assert_eq!(image.row_segments(0, 300, 300).count(), 0);
    assert_eq!(image.row_segments(0, 400, 300).count(), 0);
    assert_eq!(image.row_segments(0, 0, W).count(), 3, "three tile columns");
    assert_eq!(float.row_segments(519, 0, 10000).count(), 3);
}

// ------------------------------------------------------- (b) sample_at --

#[test]
fn sample_at_reads_every_tile_corner_and_seam() {
    let _serial = serial();
    let source16 = pattern16(W, H, 7);
    let image = image16(W, H, source16.clone());
    let source32 = pattern32(W, H, 9);
    let float = image32(W, H, source32.clone());
    for y in [0, 1, 254, 255, 256, 257, 511, 512, 518, 519] {
        for x in [0, 1, 254, 255, 256, 257, 511, 512, 598, 599] {
            let at = (y * W + x) as usize;
            assert_eq!(
                image.sample_at(x, y).map(bits32),
                Some(norm16(source16[at])),
                "u16 ({x}, {y})"
            );
            assert_eq!(
                float.sample_at(x, y).map(bits32),
                Some(bits32(source32[at])),
                "f32 ({x}, {y})"
            );
        }
    }
    for (x, y) in [(W, 0), (0, H), (u32::MAX, u32::MAX)] {
        assert!(image.sample_at(x, y).is_none(), "u16 ({x}, {y}) is outside");
        assert!(float.sample_at(x, y).is_none(), "f32 ({x}, {y}) is outside");
    }
}

// ------------------------------------------------ (b2) rgba16_at (13B-3c-2a) --

#[test]
fn rgba16_at_answers_the_stored_code_at_every_tile_corner_and_seam() {
    let _serial = serial();
    let source16 = pattern16(W, H, 23);
    let image = image16(W, H, source16.clone());
    let float = image32(W, H, pattern32(W, H, 25));
    let one = image16(1, 1, vec![[1, 2, 3, 65535]]);
    evict();
    assert_eq!(contiguous_view_bytes(), 0, "evicted");
    for y in [0, 1, 254, 255, 256, 257, 511, 512, 518, 519] {
        for x in [0, 1, 254, 255, 256, 257, 511, 512, 598, 599] {
            assert_eq!(
                image.rgba16_at(x, y),
                Some(source16[(y * W + x) as usize]),
                "u16 ({x}, {y}): the stored code, not normalized"
            );
            assert!(float.rgba16_at(x, y).is_none(), "f32 has no u16 code");
        }
    }
    for (x, y) in [(W, 0), (0, H), (u32::MAX, u32::MAX)] {
        assert!(image.rgba16_at(x, y).is_none(), "u16 ({x}, {y}) is outside");
    }
    assert_eq!(one.rgba16_at(0, 0), Some([1, 2, 3, 65535]));
    assert!(one.rgba16_at(1, 0).is_none());
    assert_eq!(contiguous_view_bytes(), 0, "rgba16_at binds no view");
}

// ------------------------------------------------------------ (c) runs --

#[test]
fn runs_flatten_to_the_owned_copy_and_answer_none_for_the_other_storage() {
    let _serial = serial();
    for (w, h) in [(W, H), (1, 1), (257, 3)] {
        let image = image16(w, h, pattern16(w, h, 11));
        let flat: Vec<[u16; 4]> = image.runs_rgba16().unwrap().flatten().copied().collect();
        assert_eq!(flat, image.to_rgba16().unwrap(), "{w}×{h} u16");
        assert!(image.runs_rgba32().is_none(), "u16 has no f32 runs");

        let float = image32(w, h, pattern32(w, h, 13));
        let flat: Vec<[u32; 4]> = float
            .runs_rgba32()
            .unwrap()
            .flatten()
            .map(|p| bits32(*p))
            .collect();
        let owned: Vec<[u32; 4]> = float.to_rgba32().unwrap().into_iter().map(bits32).collect();
        assert_eq!(flat, owned, "{w}×{h} f32");
        assert!(float.runs_rgba16().is_none(), "f32 has no u16 runs");
    }
    let one = image16(1, 1, vec![[1, 2, 3, 65535]]);
    let runs: Vec<_> = one.row_segments(0, 0, 1).collect();
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].0, 0);
    assert_eq!(
        bits32(runs[0].1.pixel(0)),
        norm16([1, 2, 3, 65535]),
        "a 1×1 image's one sample"
    );
    assert_eq!(
        one.sample_at(0, 0).map(bits32),
        Some(norm16([1, 2, 3, 65535]))
    );
    assert!(one.sample_at(1, 0).is_none());
}

// --------------------------------------------- (d) no primitive binds a view --

#[test]
fn no_primitive_leaves_a_view_behind() {
    let _serial = serial();
    let image = image16(W, H, pattern16(W, H, 17));
    let float = image32(W, H, pattern32(W, H, 19));
    evict();
    assert_eq!(contiguous_view_bytes(), 0, "evicted");
    let mut sink = 0.0f32;
    for img in [&image, &float] {
        for y in [0, 256, 519] {
            for (_, run) in img.row_segments(y, 0, W) {
                sink += run.pixel(run.len() - 1)[0];
            }
        }
        sink += img.sample_at(300, 300).unwrap()[1];
    }
    sink += image.runs_rgba16().unwrap().flatten().count() as f32;
    sink += float.runs_rgba32().unwrap().flatten().count() as f32;
    assert!(sink.is_finite());
    assert_eq!(
        contiguous_view_bytes(),
        0,
        "row_segments / sample_at / runs bind no view"
    );
}

// ------------------------- (e) a projection and a window leave no view --

fn u16_document() -> PrecisionDocument {
    let layers = vec![
        Layer::raster("Base", image16(W, H, pattern16(W, H, 1)), (0, 0), 1.0, true).unwrap(),
        Layer::raster(
            "Off",
            image16(300, 200, pattern16(300, 200, 2)),
            (-100, 400),
            0.8,
            true,
        )
        .unwrap(),
        Layer::raster(
            "Seam",
            image16(390, 260, pattern16(390, 260, 3)),
            (130, 140),
            1.0,
            true,
        )
        .unwrap(),
    ];
    PrecisionDocument::from_parts(
        W,
        H,
        IccProfile::srgb(),
        Metadata::default(),
        Resolution::default(),
        layers,
    )
    .unwrap()
}

/// A float document of three rasters: the base, a copy of one window pasted
/// in place, then its duplicate moved by an integer translation (the
/// admitted float `move_layers` form).
fn float_multi_document() -> PrecisionDocument {
    let doc = float_document(W, H, extended32(W, H, 21));
    let copied = doc
        .clipboard_copy(
            &serde_json::from_value(
                json!({"op":"copy","layer":0,"x":130,"y":140,"width":390,"height":260}),
            )
            .unwrap(),
            None,
            LIMIT,
            0,
            None,
        )
        .expect("a float copy");
    let doc = doc
        .clipboard_paste(
            &copied,
            &serde_json::from_value(
                json!({"name":"Seam","anchor":0,"position":"top","placement":"in_place"}),
            )
            .unwrap(),
            LIMIT,
            0,
            None,
        )
        .expect("a float paste");
    let doc = doc
        .duplicate_layer(1)
        .expect("a duplicate")
        .expect("a changed document");
    let top = doc.layer_count() - 1;
    let doc = geometry(
        &doc,
        json!({"op":"affine","layer":top,"target":"pixels","matrix":[1,0,0,1,-200,250],"filter":"bilinear"}),
    )
    .expect("a move");
    edit(
        &doc,
        json!({"op":"layer_blend","version":1,"layer":top,"blend_mode":1}),
    )
}

fn free_image(image: *mut PrecisionImage) -> (u32, u32) {
    assert!(!image.is_null());
    let dimensions = unsafe {
        (
            dr_precision_image_width(image),
            dr_precision_image_height(image),
        )
    };
    unsafe { dr_precision_image_free(image) };
    dimensions
}

#[test]
fn a_projection_and_a_regional_window_leave_no_view_over_the_document() {
    let _serial = serial();
    for (doc, bpp) in [(u16_document(), 8u64), (float_multi_document(), 16u64)] {
        evict();
        assert_eq!(contiguous_view_bytes(), 0);
        let mut err: *mut c_char = ptr::null_mut();
        let projection = unsafe { dr_precision_doc_render(&doc, LIMIT, 0, ptr::null(), &mut err) };
        assert!(!projection.is_null(), "render: {}", take(err));
        let (window_w, window_h) = (300u32, 200u32);
        let window = unsafe {
            dr_precision_doc_render_region(
                &doc,
                200,
                230,
                window_w,
                window_h,
                1,
                LIMIT,
                0,
                ptr::null(),
                &mut err,
            )
        };
        assert!(!window.is_null(), "region: {}", take(err));
        assert_eq!(
            document_views(&[&doc]),
            0,
            "no view over the document's planes (bpp {bpp})"
        );
        // 13B-3c-2b: the host projection and the regional window are
        // unseeded, so neither output seeds a view either.
        assert_eq!(
            contiguous_view_bytes(),
            0,
            "process-wide, no view at all (bpp {bpp})"
        );
        assert_eq!(free_image(projection), (W, H));
        assert_eq!(free_image(window), (window_w, window_h));
    }
}

// ------------------------------------ (f) image_equal over multi-tile planes --

/// A horizontally symmetric sample for `(x, y)` of a `W`-wide plane.
fn mirrored(x: u32) -> u32 {
    x.min(W - 1 - x)
}

fn symmetric16(asymmetric: bool) -> Vec<[u16; 4]> {
    let mut samples: Vec<[u16; 4]> = (0..W * H)
        .map(|i| {
            let (x, y) = (mirrored(i % W), i / W);
            [
                ((x * 211 + y * 17) % 65536) as u16,
                ((x * 7 + y * 331) % 65536) as u16,
                ((x ^ y) * 13 % 65536) as u16,
                65535,
            ]
        })
        .collect();
    if asymmetric {
        // Its mirror (0, 519) holds another value; both sit in the last
        // tile row, the sample itself in the last tile.
        samples[(H * W - 1) as usize][0] ^= 0x0101;
    }
    samples
}

fn symmetric32(asymmetric: bool) -> Vec<f32> {
    let mut samples: Vec<f32> = (0..W * H)
        .flat_map(|i| {
            let (x, y) = (mirrored(i % W), i / W);
            let v = (x % 173) as f32 * 0.02 - 1.5 + (y % 11) as f32 * 0.25;
            [v, v + 1.25, -v, 1.0]
        })
        .collect();
    if asymmetric {
        let last = ((H * W - 1) * 4) as usize;
        samples[last] += 0.5;
    }
    samples
}

fn centred(doc: PrecisionDocument) -> PrecisionDocument {
    doc.set_ruler_origin(f64::from(W) / 2.0, f64::from(H) / 2.0)
        .expect("the origin moves")
}

#[test]
fn image_equal_walks_every_seam_at_u16_and_float() {
    let _serial = serial();
    for asymmetric in [false, true] {
        let doc = centred(
            PrecisionDocument::from_image(
                image16(W, H, symmetric16(asymmetric)),
                Metadata::default(),
                Resolution::default(),
            )
            .unwrap(),
        );
        let flipped = geometry(&doc, json!({"op":"flip_horizontal"}));
        assert_eq!(
            flipped.is_some(),
            asymmetric,
            "u16: a symmetric flip is None, an asymmetric one an edit"
        );

        let doc = centred(float_document(W, H, symmetric32(asymmetric)));
        let flipped = geometry(&doc, json!({"op":"flip_horizontal"}));
        assert_eq!(
            flipped.is_some(),
            asymmetric,
            "f32: a symmetric flip is None, an asymmetric one an edit"
        );
    }
}

// -------------------------------- (g) the contiguous fallback, read in place --

/// The per-index formula the 8193² plane is built from (and checked
/// against, so no second 1 GB copy exists).
fn formula(i: usize) -> [f32; 4] {
    [
        (i % 251) as f32 / 251.0,
        (i % 509) as f32 / 509.0,
        (i % 1021) as f32 / 1021.0,
        (i % 17) as f32 / 16.0,
    ]
}

/// 8193² f32 = 1,074,003,984 bytes, above `CONTIGUOUS_CACHE_BYTES`, built
/// through the test seam `contiguous_rgba32_extended`, so the plane stays
/// contiguous (as a tile-store refusal leaves it; every adopted plane tiles)
/// and every read goes through its one buffer.
#[test]
#[ignore = "allocates ~1.07 GB; run once per 13B-3c change"]
fn the_contiguous_fallback_reads_in_place() {
    let _serial = serial();
    evict();
    let edge = 8193u32;
    let n = edge as usize * edge as usize;
    let image = PrecisionImage::contiguous_rgba32_extended(
        edge,
        edge,
        (0..n).map(formula).collect(),
        IccProfile::srgb(),
    )
    .unwrap();
    let at = |x: u32, y: u32| y as usize * edge as usize + x as usize;
    for (x, y) in [
        (0, 0),
        (255, 256),
        (8192, 0),
        (0, 8192),
        (8192, 8192),
        (4097, 3001),
    ] {
        assert_eq!(
            image.sample_at(x, y).map(bits32),
            Some(bits32(formula(at(x, y)))),
            "({x}, {y})"
        );
    }
    assert!(image.sample_at(edge, 0).is_none());
    assert!(image.sample_at(0, edge).is_none());
    for (y, x0, x1) in [(0, 0, edge), (4096, 250, 262), (8192, 8000, 20000)] {
        let runs: Vec<_> = image.row_segments(y, x0, x1).collect();
        assert_eq!(runs.len(), 1, "a contiguous plane answers ONE run");
        let (x, run) = runs[0];
        assert_eq!(x, x0);
        assert_eq!(run.len() as u32, x1.min(edge) - x0);
        for j in [0, run.len() / 2, run.len() - 1] {
            assert_eq!(bits32(run.pixel(j)), bits32(formula(at(x0 + j as u32, y))));
        }
    }
    assert_eq!(image.row_segments(edge, 0, edge).count(), 0);
    let mut count = 0usize;
    for (i, p) in image.runs_rgba32().unwrap().flatten().enumerate() {
        assert_eq!(bits32(*p), bits32(formula(i)), "sample {i}");
        count += 1;
    }
    assert_eq!(count, n);
    assert_eq!(contiguous_view_bytes(), 0, "the fallback seeds nothing");
}
