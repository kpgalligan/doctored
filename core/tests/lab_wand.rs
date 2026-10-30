//! The Magic Wand, the Paint Bucket and Select ▸ Similar and Grow (#5) on a
//! Lab Color document, through the FFI (`dr_precision_doc_select`,
//! `dr_precision_doc_edit`). The rule is the one
//! `scripts/photoshop-probe/lab_select_682_take.py`'s take fitted (C6 at 8
//! bits, C9 at 16). This file's oracle is written from that docstring, in
//! f64: each code becomes a byte (8 bits: v / 257 rounded; 16 bits:
//! Photoshop's 15-bit grid, v·32768/65535 for L\* and v·32640/65535 for
//! a\*/b\* rounded half up, then ⌊(i·255 + 16384)/32768⌋); a pixel is
//! admitted when L, a and b lie within ± t of the references' range, the
//! halved cyclic differences ⌊(a − L + 255)/2⌋, ⌊(b − a + 255)/2⌋ and
//! ⌊(L − b + 255)/2⌋ within ± ⌊t/2⌋, and alpha within ± 257·t (the host's
//! ×257 tolerance). Bounds, declared before measuring, exact on every
//! pixel, at 8 and 16 bits:
//!
//! * (a) the wand (seeds (3, 2) and (12, 9), t 0, 5, 15 and 30, Contiguous
//!   on and off) equals the oracle's single-seed region, flooded
//!   four-connected from the seed when Contiguous;
//! * (b) the Paint Bucket (t 15, Contiguous on and off) changes exactly the
//!   oracle's wand region and nothing else;
//! * (c) at 16 bits Select ▸ Similar (global) and Grow (contiguous) from a
//!   coverage whose references are every pixel at least half selected
//!   (32768 is one, 32767 is not) equal the oracle's box over those
//!   references; no reference refuses with "No pixel is more than half
//!   selected, so there is no colour to match.";
//! * (d) at 8 bits Similar and Grow refuse naming #5, in their exact
//!   sentences.
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

/// The references' key box.
fn key_box(pixels: &[[u16; 4]], references: &[usize], bits: u32) -> ([f64; 7], [f64; 7]) {
    let mut low = [f64::INFINITY; 7];
    let mut high = [f64::NEG_INFINITY; 7];
    for &i in references {
        let k = keys(pixels[i], bits);
        for c in 0..7 {
            low[c] = low[c].min(k[c]);
            high[c] = high[c].max(k[c]);
        }
    }
    (low, high)
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

#[test]
fn lab_similar_and_grow_at_16_bits_equal_the_key_oracle() {
    let bits = 16;
    let pixels = scene(bits);
    let doc = lab_document(pixels.clone(), bits);
    let cover = coverage();
    let references: Vec<usize> = (0..cover.len()).filter(|&i| cover[i] >= 32768).collect();
    assert_eq!(references.len(), 21, "the references");
    let (low, high) = key_box(&pixels, &references, bits);
    for t in [0u32, 5, 15] {
        for contiguous in [false, true] {
            let what = format!("similar t {t} contiguous {contiguous}");
            let operation = json!({"kind":"similar","tolerance":257 * t,"contiguous":contiguous});
            let got = select(doc, &operation, &cover).unwrap_or_else(|e| panic!("{what}: {e}"));
            let want = region(
                |i| admitted(pixels[i], &low, &high, t, bits),
                &references,
                contiguous,
            );
            assert_region(&got, &want, &what);
        }
    }
    let none = vec![32767u16; cover.len()];
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

#[test]
fn lab_similar_and_grow_refuse_at_8_bits() {
    let doc = lab_document(scene(8), 8);
    let cover = coverage();
    for (contiguous, label) in [
        (false, "select_similar"),
        (true, "select_similar with contiguous true (Grow)"),
    ] {
        let operation = json!({"kind":"similar","tolerance":257 * 10,"contiguous":contiguous});
        assert_eq!(
            select(doc, &operation, &cover).unwrap_err(),
            format!(
                "{label} is unavailable in an 8-bit Lab Color document until its Lab tolerance \
                 rule is measured (#5)."
            )
        );
    }
    free(doc);
}
