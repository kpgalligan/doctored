//! Black-box suite for 13B-3b-2: the typed `PrecisionImage` writers — the
//! whole-image `rebuilt_rgba16` / `rebuilt_rgba32_extended`, the rect
//! `with_rect_rgba16/32` and `read_rect_rgba16/32`, `from_rgba16_unseeded`
//! and a render's `reidentified` (the byte `LayerRaster` contract's typed
//! twin). Oracles are hand arithmetic over a patterned `Vec` built before the
//! writer adopts it, `to_rgba16/32`, `same_samples`, `shared_tile_count`,
//! `contiguous_view_bytes` and `dr_mixed_resources_report_with_raw_masks`.
//!
//! The view cache is process-wide, so every test holds `serial()`: a count
//! of `contiguous_views` must not see another test's images.
//!
//! Since 13B-3d-3a a rebuild seeds nothing: (a), (b) and (c) assert that and
//! fail at `63421b3` by design (13B-3d-3a).

use std::ffi::CString;
use std::ptr;
use std::sync::Arc;

use doctored_core::contiguous_views::contiguous_view_bytes;
use doctored_core::ffi_develop_inspect::dr_precision_doc_source_master16;
use doctored_core::ffi_precision::{dr_precision_image_free, dr_precision_image_storage_identity};
use doctored_core::ffi_precision_edit::dr_precision_doc_edit;
use doctored_core::ffi_retained_source::dr_precision_doc_source_edit;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use serde_json::{json, Value};

use crate::resource_report;
use crate::serial;

/// The multi-tile plane of (a)/(b)/(e)/(i): 4×3 tiles of 256.
const W: u32 = 1024;
const H: u32 = 768;
const TILES: usize = 12;

/// The 300×300 rect at (200, 200): crosses both seams at 256, so it touches
/// exactly the four tiles (0,0), (1,0), (0,1), (1,1).
const SEAM: (u32, u32, u32, u32) = (200, 200, 300, 300);

const EXTENDED: &str = "32-bit float samples must be finite within ±65504 with alpha in [0,1]";

fn pattern16(width: u32, height: u32) -> Vec<[u16; 4]> {
    (0..width * height)
        .map(|i| {
            [
                (i.wrapping_mul(977) & 0xffff) as u16,
                (65535 - (i.wrapping_mul(131) & 0xffff)) as u16,
                ((i ^ 0x5a5a) & 0xffff) as u16,
                (30000 + i % 30000) as u16,
            ]
        })
        .collect()
}

/// Extended-range samples (some above white, some negative); every sample
/// whose index is a multiple of 7 holds `+0.0` in R.
fn pattern32(width: u32, height: u32) -> Vec<[f32; 4]> {
    let n = (width * height) as f32;
    (0..width * height)
        .map(|i| {
            let v = i as f32 / n;
            let r = if i % 7 == 0 { 0.0 } else { v * 3.0 - 1.0 };
            [r, 1.0 - v, v * 0.5, if i % 2 == 0 { 1.0 } else { v }]
        })
        .collect()
}

fn image16(width: u32, height: u32, pixels: Vec<[u16; 4]>) -> PrecisionImage {
    PrecisionImage::from_rgba16(width, height, pixels, IccProfile::srgb(), None).unwrap()
}

fn image32(width: u32, height: u32, pixels: Vec<[f32; 4]>) -> PrecisionImage {
    PrecisionImage::from_rgba32_extended(width, height, pixels, IccProfile::srgb(), None).unwrap()
}

fn bits(samples: &[[f32; 4]]) -> Vec<[u32; 4]> {
    samples.iter().map(|p| p.map(f32::to_bits)).collect()
}

/// `plane` with the rect `(x, y, w, h)` overwritten by `samples` (row-major,
/// stride `w`): the hand oracle every rect write is compared against.
fn overwritten<S: Copy>(
    plane: &[S],
    width: u32,
    (x, y, w, h): (u32, u32, u32, u32),
    samples: &[S],
) -> Vec<S> {
    let mut out = plane.to_vec();
    for row in 0..h as usize {
        for col in 0..w as usize {
            out[(y as usize + row) * width as usize + x as usize + col] =
                samples[row * w as usize + col];
        }
    }
    out
}

/// The rect `(x, y, w, h)` of `plane`, row-major: the hand read oracle.
fn sliced<S: Copy>(plane: &[S], width: u32, (x, y, w, h): (u32, u32, u32, u32)) -> Vec<S> {
    let mut out = Vec::new();
    for row in y..y + h {
        let at = (row * width + x) as usize;
        out.extend_from_slice(&plane[at..at + w as usize]);
    }
    out
}

/// `w·h` u16 samples that differ from every pattern sample (a code the
/// pattern's alpha never takes: alpha stays in 30000..60000).
fn ink16(w: u32, h: u32) -> Vec<[u16; 4]> {
    (0..w * h).map(|i| [i as u16, 7, 9, 1]).collect()
}

fn ink32(w: u32, h: u32) -> Vec<[f32; 4]> {
    (0..w * h)
        .map(|i| [2.5, -0.25, (i % 1000) as f32, 0.5])
        .collect()
}

fn report(documents: &[&PrecisionDocument], images: &[&PrecisionImage]) -> Value {
    let documents: Vec<*const PrecisionDocument> =
        documents.iter().map(|d| *d as *const _).collect();
    let images: Vec<*const PrecisionImage> = images.iter().map(|i| *i as *const _).collect();
    resource_report::report(&[], &[], &documents, &images, 0, 0).expect("a resource report")
}

fn evict() {
    resource_report::evict(&[]).expect("an eviction report");
}

fn category(report: &Value, category: &str, field: &str) -> u64 {
    report["categories"][category][field].as_u64().unwrap_or(0)
}

// ---------------------------------------------------------------- (a) --

#[test]
fn a_rebuilt_u16_image_shares_every_tile_outside_one_seam_crossing_change() {
    let _serial = serial();
    evict();
    let base_samples = pattern16(W, H);
    let base = image16(W, H, base_samples.clone());
    let before = contiguous_view_bytes();
    let changed = overwritten(&base_samples, W, SEAM, &ink16(SEAM.2, SEAM.3));
    let adopted = changed.clone();
    let next = base
        .rebuilt_rgba16(W, H, adopted, IccProfile::srgb())
        .unwrap();
    assert_eq!(next.shared_tile_count(&base), TILES - 4);
    assert!(!PrecisionImage::ptr_eq(&next, &base), "a fresh identity");
    assert_eq!(next.to_rgba16().unwrap(), changed, "samples exact");
    assert_eq!(base.to_rgba16().unwrap(), base_samples, "the input is pure");
    assert_eq!(
        contiguous_view_bytes(),
        before,
        "the rebuild seeds nothing (13B-3d-3a)"
    );
}

// ---------------------------------------------------------------- (b) --

#[test]
fn b_a_rebuild_that_changes_nothing_shares_every_tile_under_a_fresh_identity() {
    let _serial = serial();
    evict();
    let base = image16(W, H, pattern16(W, H));
    let one_view = u64::from(W) * u64::from(H) * 8;
    assert_eq!(contiguous_view_bytes(), one_view, "the base's seed");
    let same = pattern16(W, H);
    let buffer = same.as_ptr() as usize;
    let next = base.rebuilt_rgba16(W, H, same, IccProfile::srgb()).unwrap();
    assert_eq!(next.shared_tile_count(&base), TILES, "every tile shared");
    assert!(!PrecisionImage::ptr_eq(&next, &base), "a fresh identity");
    assert!(next.same_samples(&base));
    assert_eq!(
        contiguous_view_bytes(),
        one_view,
        "the rebuild seeds nothing (13B-3d-3a): only the base's seed"
    );
    let storage = unsafe { dr_precision_image_storage_identity(&next) } as usize;
    assert_ne!(storage, buffer, "the adopted buffer is not the storage");
    assert_eq!(
        unsafe { dr_precision_image_storage_identity(&base) } as usize,
        storage,
        "one plane: the unchanged rebuild's storage is the base's"
    );
    assert_eq!(contiguous_view_bytes(), one_view);
}

// ---------------------------------------------------------------- (c) --

#[test]
fn c_a_rebuild_over_other_dimensions_or_storage_tiles_fresh() {
    let _serial = serial();
    evict();
    let base = image16(W, H, pattern16(W, H));
    let small = pattern16(300, 200);
    let adopted = small.clone();
    let next = base
        .rebuilt_rgba16(300, 200, adopted, IccProfile::srgb())
        .unwrap();
    assert_eq!(
        contiguous_view_bytes(),
        u64::from(W) * u64::from(H) * 8,
        "fresh tiles, nothing seeded (13B-3d-3a): only the base's seed"
    );
    assert_eq!(next.dimensions(), (300, 200));
    assert_eq!(next.to_rgba16().unwrap(), small);
    assert_eq!(next.shared_tile_count(&base), 0);

    let float = image32(W, H, pattern32(W, H));
    let samples = pattern16(W, H);
    let next = float
        .rebuilt_rgba16(W, H, samples.clone(), IccProfile::srgb())
        .unwrap();
    assert_eq!(
        contiguous_view_bytes(),
        u64::from(W) * u64::from(H) * 8,
        "fresh u16 tiles, nothing seeded (13B-3d-3a): only the u16 base's seed \
         (the f32 base seeds nothing since 13B-3e-4c-2a)"
    );
    assert_eq!(next.to_rgba16().unwrap(), samples, "fresh u16 tiles");
    assert_eq!(next.shared_tile_count(&float), 0);
    assert_eq!(next.shared_tile_count(&image16(W, H, pattern16(W, H))), 0);
    assert!(!PrecisionImage::ptr_eq(&next, &float));

    // The constructor's refusals are the writer's.
    let Err(refusal) = base.rebuilt_rgba16(W, H, vec![[0; 4]; 3], IccProfile::srgb()) else {
        panic!("a length mismatch must refuse");
    };
    assert_eq!(
        refusal,
        "precision pixel buffer length does not match dimensions"
    );
}

// ---------------------------------------------------------------- (d) --

#[test]
fn d_a_rebuilt_f32_image_compares_bitwise_and_validates_extended_range() {
    let _serial = serial();
    let samples = pattern32(W, H);
    let base = image32(W, H, samples.clone());
    // A sample of tile (1, 1) whose R is +0.0 (its index a multiple of 7).
    let index = (300 * W + 301) as usize;
    let index = index - index % 7;
    assert_eq!(samples[index][0].to_bits(), 0.0_f32.to_bits());
    let mut signed = samples.clone();
    signed[index][0] = -0.0;
    let next = base
        .rebuilt_rgba32_extended(W, H, signed.clone(), IccProfile::srgb())
        .unwrap();
    assert_eq!(
        next.shared_tile_count(&base),
        TILES - 1,
        "-0.0 over 0.0 allocates exactly one tile"
    );
    assert_eq!(bits(&next.to_rgba32().unwrap()), bits(&signed));
    assert!(!PrecisionImage::ptr_eq(&next, &base));

    for bad in [f32::NAN, 70000.0] {
        let mut refused = samples.clone();
        refused[index][1] = bad;
        let Err(refusal) = base.rebuilt_rgba32_extended(W, H, refused, IccProfile::srgb()) else {
            panic!("{bad} must refuse");
        };
        assert_eq!(refusal, EXTENDED, "{bad}");
    }
}

// ---------------------------------------------------------------- (e) --

#[test]
fn e_with_rect_writes_only_the_rect_and_shares_every_untouched_tile() {
    let _serial = serial();
    let samples = pattern16(W, H);
    let base = image16(W, H, samples.clone());
    evict();
    // (rect, tiles it touches)
    let rects = [
        ((10, 20, 30, 40), 1),     // interior of tile (0, 0)
        ((0, 300, 5, 10), 1),      // left edge
        ((600, 0, 10, 5), 1),      // top edge
        ((W - 7, 500, 7, 3), 1),   // right edge
        ((700, H - 2, 9, 2), 1),   // bottom edge
        ((W - 1, H - 1, 1, 1), 1), // the last sample
        ((513, 257, 1, 1), 1),     // a 1×1 rect
        (SEAM, 4),                 // across both seams
        ((0, 0, W, H), TILES),     // the whole plane
    ];
    for (rect, touched) in rects {
        let ink = ink16(rect.2, rect.3);
        let next = base
            .with_rect_rgba16(rect.0, rect.1, rect.2, rect.3, &ink)
            .unwrap()
            .unwrap_or_else(|| panic!("{rect:?}: a changed image"));
        assert_eq!(
            next.to_rgba16().unwrap(),
            overwritten(&samples, W, rect, &ink),
            "{rect:?}: samples"
        );
        assert_eq!(
            next.shared_tile_count(&base),
            TILES - touched,
            "{rect:?}: fresh tiles"
        );
        assert!(!PrecisionImage::ptr_eq(&next, &base));
        assert!(
            Arc::ptr_eq(next.profile(), base.profile()),
            "{rect:?}: profile"
        );
    }
    assert_eq!(contiguous_view_bytes(), 0, "a rect write seeds no view");

    // A 1×1 plane.
    let one = image16(1, 1, vec![[1, 2, 3, 4]]);
    let next = one.with_rect_rgba16(0, 0, 1, 1, &[[5, 6, 7, 8]]).unwrap();
    assert_eq!(next.unwrap().to_rgba16().unwrap(), vec![[5, 6, 7, 8]]);
    assert!(one
        .with_rect_rgba16(0, 0, 1, 1, &[[1, 2, 3, 4]])
        .unwrap()
        .is_none());

    // Zero area and no change answer Ok(None).
    assert!(base.with_rect_rgba16(5, 5, 0, 9, &[]).unwrap().is_none());
    assert!(base.with_rect_rgba16(5, 5, 9, 0, &[]).unwrap().is_none());
    let same = sliced(&samples, W, SEAM);
    assert!(base
        .with_rect_rgba16(SEAM.0, SEAM.1, SEAM.2, SEAM.3, &same)
        .unwrap()
        .is_none());
    // Refusals.
    assert!(base.with_rect_rgba16(W - 1, 0, 2, 1, &ink16(2, 1)).is_err());
    assert!(base.with_rect_rgba16(0, H, 1, 1, &ink16(1, 1)).is_err());
    assert!(base.with_rect_rgba16(0, 0, 2, 2, &ink16(3, 1)).is_err());
    let float_samples = pattern32(W, H);
    let float = image32(W, H, float_samples.clone());
    assert_eq!(
        float.with_rect_rgba16(0, 0, 1, 1, &ink16(1, 1)).err(),
        Some("precision image storage is not RGBA16".to_string())
    );
    assert!(base.with_rect_rgba32(0, 0, 1, 1, &ink32(1, 1)).is_err());
    evict();

    // The f32 twin.
    for (rect, touched) in [((10, 20, 30, 40), 1), (SEAM, 4)] {
        let ink = ink32(rect.2, rect.3);
        let next = float
            .with_rect_rgba32(rect.0, rect.1, rect.2, rect.3, &ink)
            .unwrap()
            .unwrap();
        assert_eq!(
            bits(&next.to_rgba32().unwrap()),
            bits(&overwritten(&float_samples, W, rect, &ink)),
            "{rect:?}: f32 samples"
        );
        assert_eq!(next.shared_tile_count(&float), TILES - touched);
    }
    let index = 7 * 40; // (280, 0): R is +0.0 there.
    assert_eq!(float_samples[index][0].to_bits(), 0.0_f32.to_bits());
    let mut signed = float_samples[index];
    signed[0] = -0.0;
    let next = float
        .with_rect_rgba32(index as u32, 0, 1, 1, &[signed])
        .unwrap()
        .expect("-0.0 over 0.0 is a change");
    assert_eq!(next.shared_tile_count(&float), TILES - 1);
    assert!(float
        .with_rect_rgba32(index as u32, 0, 1, 1, &[float_samples[index]])
        .unwrap()
        .is_none());
    for bad in [f32::NAN, -70000.0] {
        assert_eq!(
            float
                .with_rect_rgba32(0, 0, 1, 1, &[[bad, 0.0, 0.0, 1.0]])
                .err(),
            Some(EXTENDED.to_string()),
            "{bad}"
        );
    }
    assert!(float.with_rect_rgba32(W, 0, 1, 1, &ink32(1, 1)).is_err());
    assert_eq!(contiguous_view_bytes(), 0, "no view left");
}

// ---------------------------------------------------------------- (f) --

#[test]
fn f_read_rect_copies_exactly_the_rect_and_binds_no_view() {
    let _serial = serial();
    let samples = pattern16(W, H);
    let base = image16(W, H, samples.clone());
    let float_samples = pattern32(W, H);
    let float = image32(W, H, float_samples.clone());
    evict();
    for rect in [
        (0, 0, 1, 1),
        (10, 20, 30, 40),
        SEAM,
        (W - 3, H - 2, 3, 2),
        (0, 0, W, H),
    ] {
        assert_eq!(
            base.read_rect_rgba16(rect.0, rect.1, rect.2, rect.3),
            Some(sliced(&samples, W, rect)),
            "{rect:?}"
        );
        assert_eq!(
            float
                .read_rect_rgba32(rect.0, rect.1, rect.2, rect.3)
                .map(|v| bits(&v)),
            Some(bits(&sliced(&float_samples, W, rect))),
            "{rect:?}"
        );
    }
    for rect in [(0, 0, 0, 1), (0, 0, 1, 0), (W, 0, 1, 1), (0, H - 1, 1, 2)] {
        assert!(base
            .read_rect_rgba16(rect.0, rect.1, rect.2, rect.3)
            .is_none());
        assert!(float
            .read_rect_rgba32(rect.0, rect.1, rect.2, rect.3)
            .is_none());
    }
    assert!(base.read_rect_rgba32(0, 0, 1, 1).is_none(), "other storage");
    assert!(
        float.read_rect_rgba16(0, 0, 1, 1).is_none(),
        "other storage"
    );
    assert_eq!(contiguous_view_bytes(), 0, "a rect read binds no view");
}

// ---------------------------------------------------------------- (g) --

#[test]
fn g_an_unseeded_construction_tiles_and_caches_nothing() {
    let _serial = serial();
    evict();
    let samples = pattern16(W, H);
    let image =
        PrecisionImage::from_rgba16_unseeded(W, H, samples.clone(), IccProfile::srgb(), None)
            .unwrap();
    assert_eq!(contiguous_view_bytes(), 0, "no seed");
    let measured = report(&[], &[&image]);
    assert!(
        measured["categories"].get("tile_structure").is_some(),
        "tiled"
    );
    assert_eq!(
        category(&measured, "precision_pixels", "unique_bytes"),
        u64::from(W) * u64::from(H) * 8
    );
    assert_eq!(image.to_rgba16().ok().unwrap(), samples);
    assert_eq!(contiguous_view_bytes(), 0, "an owned copy binds no view");
    let Err(refusal) =
        PrecisionImage::from_rgba16_unseeded(W, H, vec![[0; 4]; 2], IccProfile::srgb(), None)
    else {
        panic!("a length mismatch must refuse");
    };
    assert_eq!(
        refusal,
        "precision pixel buffer length does not match dimensions"
    );
}

// ---------------------------------------------------------------- (h) --

#[test]
fn h_an_identity_place_answers_a_fresh_identity_over_the_masters_tiles() {
    let _serial = serial();
    let (w, h) = (600, 300);
    let source = PrecisionDocument::from_image(
        image16(w, h, pattern16(w, h)),
        Metadata::default(),
        Resolution::default(),
    )
    .unwrap();
    let request = CString::new(
        json!({"op":"place","source_id":"fixture-a","name":"Placed","x":0,"y":0}).to_string(),
    )
    .unwrap();
    let mut error = ptr::null_mut();
    let placed = unsafe {
        dr_precision_doc_source_edit(
            &source,
            &source,
            request.as_ptr(),
            REFERENCE_WORKING_BYTES,
            0,
            ptr::null(),
            &mut error,
        )
    };
    assert!(error.is_null() && !placed.is_null(), "place failed");
    let placed = unsafe { Box::from_raw(placed) };
    let master = unsafe { dr_precision_doc_source_master16(&*placed, 1, &mut error) };
    assert!(error.is_null() && !master.is_null(), "a u16 master");
    let layer = placed.layer_source(1).unwrap();
    let input = source.layer_source(0).unwrap();
    {
        let master = unsafe { &*master };
        assert!(
            PrecisionImage::ptr_eq(master, input),
            "the master IS the input"
        );
        assert!(
            !PrecisionImage::ptr_eq(layer, master),
            "the render is a fresh identity"
        );
        assert!(layer.same_samples(master));
        let tiles = (w.div_ceil(256) * h.div_ceil(256)) as usize;
        assert_eq!(layer.shared_tile_count(master), tiles, "every tile shared");
    }
    unsafe { dr_precision_image_free(master) };
    let measured = report(&[&placed], &[]);
    assert_eq!(
        category(&measured, "precision_pixels", "unique_bytes"),
        u64::from(w) * u64::from(h) * 8,
        "host, asset and placed layer are one plane"
    );
}

// ---------------------------------------------------------------- (i) --

#[test]
fn i_two_typed_dabs_retain_at_most_four_new_tiles_each() {
    let _serial = serial();
    let base = Box::new(
        PrecisionDocument::from_image(
            image16(W, H, pattern16(W, H)),
            Metadata::default(),
            Resolution::default(),
        )
        .unwrap(),
    );
    let request = CString::new(
        json!({"op":"paint","layer":0,"color":[0.312501, 0.500009, 0.812503, 1.0],
            "color_space":"document","opacity":1.0})
        .to_string(),
    )
    .unwrap();
    let mut states: Vec<*mut PrecisionDocument> = vec![Box::into_raw(base)];
    for i in 0..2 {
        let mut coverage = vec![0_u16; (W * H) as usize];
        let x0 = 100 + 512 * i;
        for y in 100..400 {
            let at = y * W as usize + x0;
            coverage[at..at + 300].fill(65535);
        }
        let mut error = ptr::null_mut();
        let next = unsafe {
            dr_precision_doc_edit(
                states[i],
                request.as_ptr(),
                coverage.as_ptr(),
                coverage.len(),
                REFERENCE_WORKING_BYTES,
                0,
                ptr::null(),
                &mut error,
            )
        };
        assert!(error.is_null() && !next.is_null(), "dab {i}");
        states.push(next);
    }
    let documents: Vec<&PrecisionDocument> = states.iter().map(|d| unsafe { &**d }).collect();
    let measured = report(&documents, &[]);
    let unique = category(&measured, "precision_pixels", "unique_bytes");
    let tile = 256 * 256 * 8;
    assert!(
        unique <= u64::from(W) * u64::from(H) * 8 + 2 * 4 * tile,
        "unique precision pixels {unique}"
    );
    for state in states {
        unsafe { doctored_core::ffi_precision::dr_precision_doc_free(state) };
    }
}

// ---------------------------------------------------------------- (j) --

/// `precision_image_reads` (g)'s per-index formula (copied: tests share no
/// module but `tests/common`), so the 8193² plane is checked without a
/// second 1 GB copy.
fn formula(i: usize) -> [f32; 4] {
    [
        (i % 251) as f32 / 251.0,
        (i % 509) as f32 / 509.0,
        (i % 1021) as f32 / 1021.0,
        (i % 17) as f32 / 16.0,
    ]
}

/// The Contiguous (non-tiled) branches of `read_rect_rgba32` /
/// `with_rect_rgba32` (carried from 13B-3b-2's review). The 8193² f32 plane
/// is built through the test seam `contiguous_rgba32_extended`, so it stays
/// contiguous as a tile-store refusal leaves it (every adopted plane tiles);
/// a changed rect is written straight into tiles (no whole contiguous copy),
/// so the output is tiled.
#[test]
#[ignore = "allocates ~2.2 GB; run once per 13B-3d change"]
fn j_contiguous_fallback_rect_write_and_read() {
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
    let expected = |(x, y, w, h): (u32, u32, u32, u32)| -> Vec<[u32; 4]> {
        (y..y + h)
            .flat_map(|row| (x..x + w).map(move |col| (col, row)))
            .map(|(col, row)| formula(at(col, row)).map(f32::to_bits))
            .collect()
    };
    for rect in [
        (0, 0, 1, 1),
        (255, 256, 300, 300),
        (8000, 8100, 193, 93),
        (4097, 3001, 17, 1),
        (0, 8192, 8193, 1),
    ] {
        let read = image
            .read_rect_rgba32(rect.0, rect.1, rect.2, rect.3)
            .unwrap_or_else(|| panic!("{rect:?}: a read"));
        assert_eq!(bits(&read), expected(rect), "{rect:?}: samples");
    }
    assert!(image.read_rect_rgba32(5, 5, 0, 3).is_none(), "zero area");
    assert!(image.read_rect_rgba32(8000, 0, 194, 1).is_none(), "leaving");

    let same = image.read_rect_rgba32(255, 256, 300, 300).unwrap();
    assert!(
        image
            .with_rect_rgba32(255, 256, 300, 300, &same)
            .unwrap()
            .is_none(),
        "an unchanged rect is Ok(None)"
    );
    drop(same);
    let rect = (4000, 5000, 300, 300);
    let ink = ink32(rect.2, rect.3);
    let next = image
        .with_rect_rgba32(rect.0, rect.1, rect.2, rect.3, &ink)
        .unwrap()
        .expect("a changed image");
    assert_eq!(
        bits(
            &next
                .read_rect_rgba32(rect.0, rect.1, rect.2, rect.3)
                .unwrap()
        ),
        bits(&ink),
        "the rect holds the ink"
    );
    assert_eq!(
        next.sample_at(3999, 5000).map(|p| p.map(f32::to_bits)),
        Some(formula(at(3999, 5000)).map(f32::to_bits)),
        "a sample outside the rect is unchanged"
    );
    assert_eq!(
        next.shared_tile_count(&image),
        0,
        "a contiguous plane has no tiles"
    );
    assert_eq!(
        next.shared_tile_count(&next),
        33 * 33,
        "the written image is tiled"
    );
    drop(next);
    // Sample 0 holds +0.0 in R: -0.0 over it is a bitwise change.
    let mut zero = formula(0);
    assert_eq!(zero[0].to_bits(), 0.0f32.to_bits());
    zero[0] = -0.0;
    let next = image.with_rect_rgba32(0, 0, 1, 1, &[zero]).unwrap();
    assert!(next.is_some(), "-0.0 over +0.0 is a change");
    drop(next);
    assert!(
        image.with_rect_rgba32(5, 5, 0, 0, &[]).unwrap().is_none(),
        "zero area"
    );
    assert!(
        image
            .with_rect_rgba32(8100, 8100, 94, 1, &ink32(94, 1))
            .is_err(),
        "leaving"
    );
    assert!(
        image.with_rect_rgba32(0, 0, 2, 2, &ink32(3, 1)).is_err(),
        "count mismatch"
    );
    assert_eq!(contiguous_view_bytes(), 0, "the fallback seeds nothing");
}
