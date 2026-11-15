//! The Magic Wand and the Paint Bucket admit a pixel by the halved-key form
//! anchored at the seed: R, G, B and alpha within ± tolerance of the seed's,
//! the halved cyclic differences ⌊(G − R + 255)/2⌋, ⌊(B − G + 255)/2⌋ and
//! ⌊(R − B + 255)/2⌋ within ± ⌊tolerance/2⌋ — the rule Select ▸ Grow/Similar
//! shares, measured in Photoshop 26.10.0
//! (`scripts/photoshop-probe/wand_key_79_take.py`; this file's committed
//! masks are `blur_wand_take.py`'s confirming take). Every expectation goes
//! through the C boundary (byte `dr_doc_magic_wand`, `dr_doc_bucket_fill`; the
//! 16-bit `magic_wand` selection) and is checked against this file's own
//! keys, predicate and four-connected flood, written without the core's.
use std::collections::VecDeque;
use std::ptr;

use doctored_core::doc::DrDocument;
use doctored_core::ffi_doc::*;
use image::{Rgba, RgbaImage};
use serde_json::json;
use tempfile::TempDir;

use crate::common::*;
use crate::native_scalar;

/// The oracle's keys of one pixel (R, G and B bytes, alpha in its own
/// codes): the channels, the three halved cyclic differences, alpha.
fn keys([r, g, b, a]: [i64; 4]) -> [i64; 7] {
    [
        r,
        g,
        b,
        (g - r + 255).div_euclid(2),
        (b - g + 255).div_euclid(2),
        (r - b + 255).div_euclid(2),
        a,
    ]
}

/// The oracle predicate: the channels within `tol` of the seed's, the halved
/// differences within ⌊tol/2⌋, alpha within `alpha_tol`.
fn similar(p: [i64; 4], seed: [i64; 4], tol: i64, alpha_tol: i64) -> bool {
    let bound = [tol, tol, tol, tol / 2, tol / 2, tol / 2, alpha_tol];
    let (a, b) = (keys(p), keys(seed));
    (0..7).all(|k| (a[k] - b[k]).abs() <= bound[k])
}

/// The oracle region on byte pixels: global, or four-connected from `seed`
/// (always in it).
fn region(
    img: &[[i64; 4]],
    w: usize,
    h: usize,
    seed: (usize, usize),
    tol: i64,
    contiguous: bool,
) -> Vec<bool> {
    let s = img[seed.1 * w + seed.0];
    let admitted: Vec<bool> = img.iter().map(|&p| similar(p, s, tol, tol)).collect();
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

/// The byte wand equals the oracle at every tolerance, odd and even,
/// contiguous or not.
#[test]
fn byte_wand_is_the_halved_key_form() {
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
        for tol in [0_u8, 1, 3, 10, 11, 17, 24, 32, 50, 100, 255] {
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

/// The facts the key form carries, from grey 100: at tolerance 10,
/// (105,95,100) is in and (106,94,100) — every channel within 6 — is out
/// (q1 moves 6 against 5); the grey shift (110,110,110) is in; and
/// (105,94,100), whose R − G is 11, one past the tolerance, is in (q1 moves
/// 5: G − R + 255 is odd on grey, so the halving rounds the step down). At
/// tolerance 15, (114,100,100) is in and (115,100,100), a single-channel step
/// of exactly t, is out (q3 moves 8 against 7). The seven-projection box the
/// wand shipped before refused (105,94,100) and admitted (115,100,100).
#[test]
fn the_halved_differences_decide_a_hue_shift_and_a_grey_shift_moves_none() {
    let dir = TempDir::new().unwrap();
    let row = [
        [100, 100, 100],
        [105, 95, 100],
        [106, 94, 100],
        [110, 110, 110],
        [111, 111, 111],
        [105, 94, 100],
        [114, 100, 100],
        [115, 100, 100],
    ];
    let mut img = RgbaImage::from_pixel(row.len() as u32, 1, Rgba([100, 100, 100, 255]));
    for (x, c) in row.iter().enumerate() {
        img.put_pixel(x as u32, 0, Rgba([c[0], c[1], c[2], 255]));
    }
    let doc = doc_from(&dir, "facts.png", &img);
    assert_eq!(
        wand(doc, 0, 0, 10, false),
        [true, true, false, true, false, true, false, false]
    );
    assert_eq!(
        wand(doc, 0, 0, 15, false),
        [true, true, true, true, true, true, true, false]
    );
    unsafe { dr_doc_free(doc) };
}

/// The bucket fills exactly the oracle region (the pixels that change to an
/// opaque fill colour no fixture pixel holds).
#[test]
fn bucket_fill_fills_the_halved_key_region() {
    let dir = TempDir::new().unwrap();
    let img = fixture([37, 180, 90], 60, 9);
    let pixels = codes(&img);
    let doc = doc_from(&dir, "bucket.png", &img);
    let fill = [0_u8, 255, 0, 255];
    assert!(img.pixels().all(|p| p.0 != fill));
    for tol in [3_u8, 17, 24, 50] {
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

/// 16 bits: the same predicate on the codes' nearest bytes, with the
/// tolerance the host sends (8-bit t × 257) read back to t. A byte picture
/// widened by ×257 selects exactly what the byte wand selects, and native
/// codes that only a halved difference separates are refused.
#[test]
fn sixteen_bit_wand_runs_the_same_predicate_on_its_bytes() {
    let dir = TempDir::new().unwrap();
    let img = fixture([128, 128, 128], 30, 3);
    let doc8 = doc_from(&dir, "wide.png", &img);
    let wide: Vec<[u16; 4]> = img
        .pixels()
        .map(|p| p.0.map(|c| u16::from(c) * 257))
        .collect();
    let d = native_scalar::doc(40, 30, wide);
    for tol in [0_u8, 10, 11, 32] {
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
    // Bytes 117 (the seed), (120, 114, 117), (119, 115, 117) and 121; the
    // tolerance 1000 reads as byte 4, so q1 may move 2: the second moves 3.
    let native = vec![
        [30000, 30000, 30000, 65535],
        [30800, 29200, 30000, 65535],
        [30500, 29500, 30000, 65535],
        [31000, 31000, 31000, 65535],
    ];
    let byte = |v: u16| (i64::from(v) + 128) / 257;
    let pixels: Vec<[i64; 4]> = native
        .iter()
        .map(|p| [byte(p[0]), byte(p[1]), byte(p[2]), i64::from(p[3])])
        .collect();
    let d = native_scalar::doc(4, 1, native);
    let got = native_scalar::select(
        &d,
        json!({"kind":"magic_wand","x":0,"y":0,"tolerance":1000,"contiguous":false}),
    );
    let want: Vec<bool> = pixels
        .iter()
        .map(|&p| similar(p, pixels[0], byte(1000), 1000))
        .collect();
    assert_eq!(want, [true, false, true, true]);
    assert_eq!(got.iter().map(|&c| c == 65535).collect::<Vec<_>>(), want);
}

/// Photoshop's own wand and bucket masks from `blur_wand_take.py`'s
/// confirming take (take2), committed with their manifest: the key form
/// meets every one exactly, and the wand and the bucket agree. The
/// seven-projection box the wand shipped before missed 25 pixels of the t50
/// global cases and 723 and 799 of the t17 ones.
#[test]
fn wand_and_bucket_match_photoshops_masks() {
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
        let mismatches = got.iter().zip(&photoshop).filter(|(a, b)| a != b).count();
        assert_eq!(mismatches, 0, "{row}: pixels off Photoshop's");
        unsafe { dr_doc_free(doc) };
    }
}
