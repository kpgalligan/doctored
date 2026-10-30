//! The Magic Wand and the Paint Bucket admit a pixel by the
//! seven-projection box anchored at the seed — R, G, B, A, R−G, G−B and R−B
//! each within ± tolerance of the seed's — the rule batch v1x-blur-wand
//! measured in Photoshop 26.10.0 (`scripts/photoshop-probe/blur_wand_take.py`)
//! and the one Select ▸ Grow/Similar already used. Every expectation goes
//! through the C boundary (byte `dr_doc_magic_wand`, `dr_doc_bucket_fill`; the
//! 16-bit `magic_wand` selection) and is checked against this file's own
//! predicate and four-connected flood, written without the core's region walk.
use std::collections::VecDeque;
use std::ptr;

use doctored_core::doc::DrDocument;
use doctored_core::ffi_doc::*;
use image::{Rgba, RgbaImage};
use serde_json::json;
use tempfile::TempDir;

use crate::common::*;
use crate::native_scalar;

/// The oracle predicate: every one of the seven projections of `p` lies
/// within `tol` of the seed's. Codes are i64 so 16-bit differences never wrap.
fn similar(p: [i64; 4], seed: [i64; 4], tol: i64) -> bool {
    let project = |c: [i64; 4]| {
        [
            c[0],
            c[1],
            c[2],
            c[3],
            c[0] - c[1],
            c[1] - c[2],
            c[0] - c[2],
        ]
    };
    let (a, b) = (project(p), project(seed));
    (0..7).all(|k| (a[k] - b[k]).abs() <= tol)
}

/// The oracle region: global, or four-connected from `seed` (always in it).
fn region(
    img: &[[i64; 4]],
    w: usize,
    h: usize,
    seed: (usize, usize),
    tol: i64,
    contiguous: bool,
) -> Vec<bool> {
    let s = img[seed.1 * w + seed.0];
    let admitted: Vec<bool> = img.iter().map(|&p| similar(p, s, tol)).collect();
    if !contiguous {
        return admitted;
    }
    let mut out = vec![false; w * h];
    out[seed.1 * w + seed.0] = true;
    let mut queue = VecDeque::from([seed]);
    while let Some((x, y)) = queue.pop_front() {
        let mut next = Vec::new();
        if x > 0 {
            next.push((x - 1, y));
        }
        if x + 1 < w {
            next.push((x + 1, y));
        }
        if y > 0 {
            next.push((x, y - 1));
        }
        if y + 1 < h {
            next.push((x, y + 1));
        }
        for (nx, ny) in next {
            let i = ny * w + nx;
            if !out[i] && admitted[i] {
                out[i] = true;
                queue.push_back((nx, ny));
            }
        }
    }
    out
}

fn splitmix(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// A 40×30 opaque field of `seed` where every pixel off the seed's row and
/// column is the seed plus a random offset of up to ±`spread` per channel,
/// so hue shifts and brightness shifts both sit near every tolerance.
fn fixture(seed: [u8; 3], spread: i64, salt: u64) -> RgbaImage {
    let mut state = 0x140 ^ salt;
    RgbaImage::from_fn(40, 30, |x, y| {
        if x == 5 || y == 7 {
            return Rgba([seed[0], seed[1], seed[2], 255]);
        }
        let v = splitmix(&mut state);
        let channel = |c: usize| {
            let delta = ((v >> (16 * c)) % (2 * spread as u64 + 1)) as i64 - spread;
            (i64::from(seed[c]) + delta).clamp(0, 255) as u8
        };
        Rgba([channel(0), channel(1), channel(2), 255])
    })
}

fn codes(img: &RgbaImage) -> Vec<[i64; 4]> {
    img.pixels().map(|p| p.0.map(i64::from)).collect()
}

fn wand(doc: *const DrDocument, x: u32, y: u32, tol: u8, contiguous: bool) -> Vec<bool> {
    let n = unsafe { dr_doc_width(doc) * dr_doc_height(doc) } as usize;
    let mut mask = vec![7u8; n];
    assert!(unsafe { dr_doc_magic_wand(doc, x, y, tol, contiguous, 1, mask.as_mut_ptr()) });
    assert!(mask.iter().all(|&m| m == 0 || m == 255), "a hard mask");
    mask.iter().map(|&m| m == 255).collect()
}

/// The byte wand equals the oracle at every tolerance, contiguous or not.
#[test]
fn byte_wand_is_the_seven_projection_box() {
    let dir = TempDir::new().unwrap();
    for (k, (seed, spread)) in [
        ([100, 100, 100], 40),
        ([37, 180, 90], 60),
        ([240, 20, 128], 25),
    ]
    .into_iter()
    .enumerate()
    {
        let img = fixture(seed, spread, k as u64);
        let doc = doc_from(&dir, &format!("wand{k}.png"), &img);
        let pixels = codes(&img);
        for tol in [0_u8, 1, 3, 10, 17, 32, 50, 100, 255] {
            for contiguous in [true, false] {
                let want = region(&pixels, 40, 30, (5, 7), i64::from(tol), contiguous);
                assert_eq!(
                    wand(doc, 5, 7, tol, contiguous),
                    want,
                    "seed {seed:?} t{tol} contiguous {contiguous}"
                );
            }
        }
        unsafe { dr_doc_free(doc) };
    }
}

/// The facts that separate it from the per-channel box it replaced: from
/// grey 100 at tolerance 10, (105,95,100) is in, (106,94,100) — every channel
/// within 6 — is out (R−G moves 12), and the grey shift (110,110,110) is in.
#[test]
fn a_hue_shift_counts_double_and_a_grey_shift_does_not() {
    let dir = TempDir::new().unwrap();
    let mut img = RgbaImage::from_pixel(5, 1, Rgba([100, 100, 100, 255]));
    img.put_pixel(1, 0, Rgba([105, 95, 100, 255]));
    img.put_pixel(2, 0, Rgba([106, 94, 100, 255]));
    img.put_pixel(3, 0, Rgba([110, 110, 110, 255]));
    img.put_pixel(4, 0, Rgba([111, 111, 111, 255]));
    let doc = doc_from(&dir, "facts.png", &img);
    assert_eq!(wand(doc, 0, 0, 10, false), [true, true, false, true, false]);
    unsafe { dr_doc_free(doc) };
}

/// The bucket fills exactly the oracle region (the pixels that change to an
/// opaque fill colour no fixture pixel holds).
#[test]
fn bucket_fill_fills_the_seven_projection_region() {
    let dir = TempDir::new().unwrap();
    let img = fixture([37, 180, 90], 60, 9);
    let pixels = codes(&img);
    let doc = doc_from(&dir, "bucket.png", &img);
    let fill = [0_u8, 255, 0, 255];
    assert!(img.pixels().all(|p| p.0 != fill));
    for tol in [3_u8, 17, 50] {
        for contiguous in [true, false] {
            let out = unsafe {
                dr_doc_bucket_fill(doc, 0, 5, 7, tol, fill.as_ptr(), contiguous, ptr::null())
            };
            assert!(!out.is_null());
            let px = layer_pixels(out, 0);
            let got: Vec<bool> = px.chunks_exact(4).map(|p| p == fill).collect();
            let want = region(&pixels, 40, 30, (5, 7), i64::from(tol), contiguous);
            assert_eq!(got, want, "t{tol} contiguous {contiguous}");
            unsafe { dr_doc_free(out) };
        }
    }
    unsafe { dr_doc_free(doc) };
}

/// 16 bits: the same predicate on u16 codes, with the tolerance the host
/// sends (8-bit t × 257). A byte picture widened by ×257 selects exactly
/// what the byte wand selects, and native codes that only a difference
/// projection separates are refused.
#[test]
fn sixteen_bit_wand_runs_the_same_predicate_on_its_own_codes() {
    let dir = TempDir::new().unwrap();
    let img = fixture([128, 128, 128], 30, 3);
    let doc8 = doc_from(&dir, "wide.png", &img);
    let wide: Vec<[u16; 4]> = img
        .pixels()
        .map(|p| p.0.map(|c| u16::from(c) * 257))
        .collect();
    let d = native_scalar::doc(40, 30, wide);
    for tol in [0_u8, 10, 32] {
        for contiguous in [true, false] {
            let got = native_scalar::select(
                &d,
                json!({"kind":"magic_wand","x":5,"y":7,"tolerance":u32::from(tol) * 257,"contiguous":contiguous}),
            );
            let got: Vec<bool> = got.iter().map(|&c| c == 65535).collect();
            assert_eq!(
                got,
                wand(doc8, 5, 7, tol, contiguous),
                "t{tol} {contiguous}"
            );
        }
    }
    unsafe { dr_doc_free(doc8) };
    // Every channel within 1000 of the seed, but R−G moves 1600.
    let native = vec![
        [30000, 30000, 30000, 65535],
        [30800, 29200, 30000, 65535],
        [30500, 29500, 30000, 65535],
        [31000, 31000, 31000, 65535],
    ];
    let pixels: Vec<[i64; 4]> = native.iter().map(|p| p.map(i64::from)).collect();
    let d = native_scalar::doc(4, 1, native);
    let got = native_scalar::select(
        &d,
        json!({"kind":"magic_wand","x":0,"y":0,"tolerance":1000,"contiguous":false}),
    );
    let want = region(&pixels, 4, 1, (0, 0), 1000, false);
    assert_eq!(want, [true, false, true, true]);
    assert_eq!(got.iter().map(|&c| c == 65535).collect::<Vec<_>>(), want);
}

/// Photoshop's own wand and bucket masks from the confirming take (take2),
/// committed with their manifest. The seven-projection box was the fitted
/// member but not exact on every case: Photoshop refuses a few pixels whose
/// difference projections sit at the tolerance when it is small, and admits
/// a few just past it when it is large (the take's record). So each case is
/// held to the mismatch count the take judged — 0 on the t50 contiguous
/// cases — and the wand and the bucket must agree with each other.
#[test]
fn wand_and_bucket_are_within_the_judged_residual_of_photoshops_masks() {
    let folder =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/photoshop_blur_wand");
    let manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(folder.join("manifest.json")).unwrap()).unwrap();
    let dir = TempDir::new().unwrap();
    let rows = manifest["wand"].as_array().unwrap();
    assert_eq!(rows.len(), 8);
    let fill = [0_u8, 255, 0, 255];
    for row in rows {
        let input = image::open(folder.join(row["input"].as_str().unwrap()))
            .unwrap()
            .to_rgba8();
        assert!(input.pixels().all(|p| p.0 != fill));
        let photoshop: Vec<bool> = image::open(folder.join(row["mask"].as_str().unwrap()))
            .unwrap()
            .to_rgba8()
            .pixels()
            .map(|p| p[0] == 255)
            .collect();
        let doc = doc_from(&dir, row["input"].as_str().unwrap(), &input);
        let (x, y) = (
            row["x"].as_u64().unwrap() as u32,
            row["y"].as_u64().unwrap() as u32,
        );
        let tol = row["tolerance"].as_u64().unwrap() as u8;
        let contiguous = row["contiguous"].as_bool().unwrap();
        let got = if row["tool"] == "wand" {
            wand(doc, x, y, tol, contiguous)
        } else {
            let out = unsafe {
                dr_doc_bucket_fill(
                    doc,
                    0,
                    x as i32,
                    y as i32,
                    tol,
                    fill.as_ptr(),
                    contiguous,
                    ptr::null(),
                )
            };
            assert!(!out.is_null());
            let px = layer_pixels(out, 0);
            unsafe { dr_doc_free(out) };
            px.chunks_exact(4).map(|p| p == fill).collect()
        };
        let pixels = codes(&input);
        let oracle = region(
            &pixels,
            input.width() as usize,
            input.height() as usize,
            (x as usize, y as usize),
            i64::from(tol),
            contiguous,
        );
        assert_eq!(got, oracle, "{row}: the oracle");
        let mismatches = got.iter().zip(&photoshop).filter(|(a, b)| a != b).count() as u64;
        assert_eq!(mismatches, row["p7_mismatches"].as_u64().unwrap(), "{row}");
        unsafe { dr_doc_free(doc) };
    }
}
