//! The Magic Wand, the Paint Bucket and Select ▸ Similar and Grow on a Lab
//! Color document, through the FFI (`dr_precision_doc_select`,
//! `dr_precision_doc_edit`). The wand's rule is the one
//! `scripts/photoshop-probe/lab_select_682_take.py`'s take fitted (C6 at 8
//! bits, C9 at 16), Similar's and Grow's range the per-key trim
//! `lab_select_5_take.py`'s takes measured. This file's oracle is written
//! from those docstrings, in f64: each code becomes a byte (8 bits: v / 257
//! rounded; 16 bits: Photoshop's 15-bit grid, v·32768/65535 for L\* and
//! v·32640/65535 for a\*/b\* rounded half up, then ⌊(i·255 + 16384)/32768⌋);
//! a pixel is admitted when L, a and b lie within ± t of the references'
//! range, the halved cyclic differences ⌊(a − L + 255)/2⌋, ⌊(b − a + 255)/2⌋
//! and ⌊(L − b + 255)/2⌋ within ± ⌊t/2⌋, and alpha within ± 257·t (the
//! host's ×257 tolerance). The wand's range is its seed's keys; Similar's
//! and Grow's is each key's sorted values over the N references with
//! ⌊N/10⌋ dropped from each end, every key on its own. Bounds, declared
//! before measuring, exact on every pixel, at 8 and 16 bits:
//!
//! * (a) the wand (seeds (3, 2) and (12, 9), t 0, 5, 15 and 30, Contiguous
//!   on and off) equals the oracle's single-seed region, flooded
//!   four-connected from the seed when Contiguous;
//! * (b) the Paint Bucket (t 15, Contiguous on and off) changes exactly the
//!   oracle's wand region and nothing else;
//! * (c) Select ▸ Similar (global) and Grow (contiguous) from a coverage
//!   whose references are every pixel at least half selected (32768 is one,
//!   32767 is not; 21 of them, so each key loses 2 at each end), and from a
//!   coverage of 9 (no trim), equal the oracle's trimmed range over those
//!   references at t 0, 5 and 15; the trim is informative (the 21
//!   references' trimmed range is not their full box, and some case's region
//!   differs from the full box's); no reference refuses with "No pixel is
//!   more than half selected, so there is no colour to match.".
use std::collections::VecDeque;

use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer as Layer};
use doctored_core::precision_image::PrecisionImage;
use serde_json::json;

use crate::gray_ops::{edit, free, layer, plan_samples};
use crate::lab_geometry::{document, twin_profile, DEPTHS, H, W};
use crate::precision_select::select;

/// The field's codes: Lab (60, −12, 18).
fn field(bits: u32) -> [f64; 3] {
    let (l, a, b) = (
        60.0 / 100.0,
        (-12.0 + 128.0) / 255.0,
        (18.0 + 128.0) / 255.0,
    );
    if bits == 8 {
        [l, a, b].map(|v: f64| (v * 255.0).round() * 257.0)
    } else {
        [l, a, b].map(|v: f64| (v * 65535.0).round())
    }
}

/// The scene: the field plus each pixel's own steps of up to ±20 8-bit codes
/// per channel (and, at 16 bits, a fraction of a code), every thirteenth
/// pixel transparent with zero codes (the composite the selection reads
/// holds no colour under alpha 0).
fn scene(bits: u32) -> Vec<[u16; 4]> {
    let base = field(bits);
    (0..W * H)
        .map(|i| {
            if i % 13 == 12 {
                return [0; 4];
            }
            let step = |k: u32, m: u32| f64::from((i * k) % m) - f64::from(m / 2);
            let fraction = if bits == 16 {
                f64::from((i * 97) % 257) - 128.0
            } else {
                0.0
            };
            let code = |c: usize, s: f64| (base[c] + 257.0 * s + fraction).clamp(0.0, 65535.0);
            [
                code(0, step(7, 23)) as u16,
                code(1, step(11, 29)) as u16,
                code(2, step(5, 31)) as u16,
                65535,
            ]
        })
        .collect()
}

fn lab_document(pixels: Vec<[u16; 4]>, bits: u32) -> *mut PrecisionDocument {
    let image = PrecisionImage::from_rgba16(W, H, pixels, twin_profile(), None).unwrap();
    let record = Layer::raster("Background", image, (0, 0), 1.0, true).unwrap();
    document(&[record], bits, true)
}

/// The oracle's byte of one code (module doc).
fn byte(v: u16, channel: usize, bits: u32) -> f64 {
    if bits == 8 {
        (f64::from(v) / 257.0).round()
    } else {
        let steps = if channel == 0 { 32768.0 } else { 32640.0 };
        let fifteen = (f64::from(v) * steps / 65535.0 + 0.5).floor();
        ((fifteen * 255.0 + 16384.0) / 32768.0).floor()
    }
}

/// The oracle's seven keys of one pixel (module doc).
fn keys(p: [u16; 4], bits: u32) -> [f64; 7] {
    let [l, a, b] = [0, 1, 2].map(|c| byte(p[c], c, bits));
    [
        l,
        a,
        b,
        ((a - l + 255.0) / 2.0).floor(),
        ((b - a + 255.0) / 2.0).floor(),
        ((l - b + 255.0) / 2.0).floor(),
        f64::from(p[3]),
    ]
}

/// The oracle's admission of `p` against the references' key box at `t`.
fn admitted(p: [u16; 4], low: &[f64; 7], high: &[f64; 7], t: u32, bits: u32) -> bool {
    let t = f64::from(t);
    let half = (t / 2.0).floor();
    let bound = [t, t, t, half, half, half, 257.0 * t];
    let k = keys(p, bits);
    (0..7).all(|c| k[c] >= low[c] - bound[c] && k[c] <= high[c] + bound[c])
}

/// The references' key range: each key's values sorted, `trim` dropped from
/// each end (0: the full box).
fn key_range(
    pixels: &[[u16; 4]],
    references: &[usize],
    bits: u32,
    trim: usize,
) -> ([f64; 7], [f64; 7]) {
    let mut low = [0.0; 7];
    let mut high = [0.0; 7];
    for c in 0..7 {
        let mut values: Vec<f64> = references
            .iter()
            .map(|&i| keys(pixels[i], bits)[c])
            .collect();
        values.sort_by(|a, b| a.partial_cmp(b).unwrap());
        low[c] = values[trim];
        high[c] = values[values.len() - 1 - trim];
    }
    (low, high)
}

/// The wand's range: its one seed's keys.
fn key_box(pixels: &[[u16; 4]], references: &[usize], bits: u32) -> ([f64; 7], [f64; 7]) {
    key_range(pixels, references, bits, 0)
}

/// The oracle's region: global, or flooded four-connected from `seeds`
/// (each marked whatever its admission).
fn region(admit: impl Fn(usize) -> bool, seeds: &[usize], contiguous: bool) -> Vec<bool> {
    let n = (W * H) as usize;
    if !contiguous {
        return (0..n).map(admit).collect();
    }
    let mut out = vec![false; n];
    let mut queue: VecDeque<usize> = VecDeque::new();
    for &s in seeds {
        if !out[s] {
            out[s] = true;
            queue.push_back(s);
        }
    }
    while let Some(i) = queue.pop_front() {
        let (x, y) = ((i as u32) % W, (i as u32) / W);
        let mut next = Vec::new();
        if x > 0 {
            next.push(i - 1);
        }
        if x + 1 < W {
            next.push(i + 1);
        }
        if y > 0 {
            next.push(i - W as usize);
        }
        if y + 1 < H {
            next.push(i + W as usize);
        }
        for j in next {
            if !out[j] && admit(j) {
                out[j] = true;
                queue.push_back(j);
            }
        }
    }
    out
}

fn wand_oracle(
    pixels: &[[u16; 4]],
    seed: (u32, u32),
    t: u32,
    contiguous: bool,
    bits: u32,
) -> Vec<bool> {
    let start = (seed.1 * W + seed.0) as usize;
    let (low, high) = key_box(pixels, &[start], bits);
    region(
        |i| admitted(pixels[i], &low, &high, t, bits),
        &[start],
        contiguous,
    )
}

fn assert_region(got: &[u16], want: &[bool], what: &str) {
    assert_eq!(got.len(), want.len(), "{what}");
    let wrong: Vec<usize> = (0..want.len())
        .filter(|&i| (got[i] == 65535) != want[i] || (got[i] != 0 && got[i] != 65535))
        .collect();
    assert!(wrong.is_empty(), "{what}: pixels off the oracle {wrong:?}");
    assert!(
        want.iter().any(|v| *v),
        "{what}: the oracle selects something"
    );
}

#[test]
fn lab_wand_selects_the_key_oracle_at_both_depths() {
    for bits in DEPTHS {
        let pixels = scene(bits);
        let doc = lab_document(pixels.clone(), bits);
        let mut sizes = Vec::new();
        let mut differs = Vec::new();
        for seed in [(3u32, 2u32), (12, 9)] {
            for t in [0u32, 5, 15, 30] {
                for contiguous in [true, false] {
                    let what = format!("wand {seed:?} t {t} contiguous {contiguous} at {bits}");
                    let operation = json!({"kind":"magic_wand","x":seed.0,"y":seed.1,
                        "tolerance":257 * t,"contiguous":contiguous});
                    let got =
                        select(doc, &operation, &[]).unwrap_or_else(|e| panic!("{what}: {e}"));
                    let want = wand_oracle(&pixels, seed, t, contiguous, bits);
                    assert_region(&got, &want, &what);
                    sizes.push(want.iter().filter(|v| **v).count());
                }
                differs.push(
                    wand_oracle(&pixels, seed, t, true, bits)
                        != wand_oracle(&pixels, seed, t, false, bits),
                );
            }
        }
        // The scene is informative: some cases select more than the seed and
        // fewer than every pixel, and some (seed, t) floods a contiguous
        // region that differs from its global twin.
        assert!(
            sizes.iter().any(|&s| s > 1 && s < (W * H) as usize),
            "{bits} bits: {sizes:?}"
        );
        assert!(
            differs.iter().any(|&d| d),
            "{bits} bits: some contiguous region differs from its global twin"
        );
        free(doc);
    }
}

#[test]
fn lab_bucket_fills_the_wand_region() {
    for bits in DEPTHS {
        let pixels = scene(bits);
        let doc = lab_document(pixels.clone(), bits);
        for contiguous in [true, false] {
            let what = format!("bucket contiguous {contiguous} at {bits}");
            let request = json!({"op":"bucket_fill","layer":0,"color":[0.0,0.0,1.0,1.0],
                "color_space":"srgb","opacity":1.0,"blend_mode":0,"x":12,"y":9,
                "tolerance":257 * 15,"contiguous":contiguous});
            let n = plan_samples(doc, &request).unwrap_or_else(|e| panic!("{what}: {e}"));
            let out = edit(doc, &request, &vec![65535; n])
                .unwrap_or_else(|e| panic!("{what}: {e}"))
                .unwrap_or_else(|| panic!("{what}: nothing changed"));
            let after = layer(out, 0, bits);
            let want = wand_oracle(&pixels, (12, 9), 15, contiguous, bits);
            let changed: Vec<bool> = after
                .iter()
                .zip(&pixels)
                .map(|(a, b)| *a != b.map(u32::from))
                .collect();
            let wrong: Vec<usize> = (0..want.len())
                .filter(|&i| changed[i] && !want[i])
                .collect();
            assert!(
                wrong.is_empty(),
                "{what}: filled outside the region {wrong:?}"
            );
            let fill = after[12 + 9 * W as usize];
            let missed: Vec<usize> = (0..want.len())
                .filter(|&i| want[i] && after[i] != fill)
                .collect();
            assert!(
                missed.is_empty(),
                "{what}: the region not filled whole {missed:?}"
            );
            assert!(
                want.iter().filter(|v| **v).count() > 1,
                "{what}: informative"
            );
            free(out);
        }
        free(doc);
    }
}

/// (c)'s coverage: a full 5 × 4 rectangle at (2, 2), (10, 10) at 32768 (a
/// reference) and (11, 10) at 32767 (not one).
fn coverage() -> Vec<u16> {
    (0..W * H)
        .map(|i| {
            let (x, y) = (i % W, i / W);
            if (2..7).contains(&x) && (2..6).contains(&y) {
                65535
            } else if (x, y) == (10, 10) {
                32768
            } else if (x, y) == (11, 10) {
                32767
            } else {
                0
            }
        })
        .collect()
}

/// (c)'s second coverage: a full 3 × 3 rectangle at (12, 4), 9 references,
/// so ⌊9/10⌋ = 0 and nothing is trimmed.
fn small_coverage() -> Vec<u16> {
    (0..W * H)
        .map(|i| {
            let (x, y) = (i % W, i / W);
            if (12..15).contains(&x) && (4..7).contains(&y) {
                65535
            } else {
                0
            }
        })
        .collect()
}

#[test]
fn lab_similar_and_grow_equal_the_trimmed_key_oracle() {
    for bits in DEPTHS {
        let pixels = scene(bits);
        let doc = lab_document(pixels.clone(), bits);
        let mut differs = false;
        for (cover, count) in [(coverage(), 21usize), (small_coverage(), 9)] {
            let references: Vec<usize> = (0..cover.len()).filter(|&i| cover[i] >= 32768).collect();
            assert_eq!(references.len(), count, "the references at {bits} bits");
            let trim = count / 10;
            let (low, high) = key_range(&pixels, &references, bits, trim);
            let (full_low, full_high) = key_range(&pixels, &references, bits, 0);
            if trim > 0 {
                assert!(
                    (low, high) != (full_low, full_high),
                    "{bits} bits: the trim changes the 21 references' range"
                );
            }
            for t in [0u32, 5, 15] {
                for contiguous in [false, true] {
                    let what = format!(
                        "{count} references, t {t}, contiguous {contiguous}, at \
                                        {bits} bits"
                    );
                    let operation =
                        json!({"kind":"similar","tolerance":257 * t,"contiguous":contiguous});
                    let got =
                        select(doc, &operation, &cover).unwrap_or_else(|e| panic!("{what}: {e}"));
                    let admit = |l: &[f64; 7], h: &[f64; 7]| {
                        region(
                            |i| admitted(pixels[i], l, h, t, bits),
                            &references,
                            contiguous,
                        )
                    };
                    let want = admit(&low, &high);
                    differs |= want != admit(&full_low, &full_high);
                    assert_region(&got, &want, &what);
                }
            }
        }
        assert!(
            differs,
            "{bits} bits: some case's trimmed region differs from the full box's"
        );
        let none = vec![32767u16; (W * H) as usize];
        assert_eq!(
            select(
                doc,
                &json!({"kind":"similar","tolerance":257 * 5,"contiguous":false}),
                &none
            )
            .unwrap_err(),
            "No pixel is more than half selected, so there is no colour to match."
        );
        free(doc);
    }
}
