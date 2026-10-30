//! #547 Indexed Color phase 3c: Image ▸ Mode ▸ Indexed Color's Pattern and
//! Noise dithers (declared candidates, kept by phase 3d's take), Forced
//! Custom and the four palettes phase 3d refuses for good, through the FFI.
//!
//! The oracles are the tests' own: each pixel moved by `(t − ½)·51` on r, g
//! and b, clamped and rounded, then mapped to its nearest entry
//! (`indexed_fixture::nearest`), with `t` from the 8 × 8 Bayer matrix typed
//! out below (Pattern) or the Murmur-style mix re-implemented below (Noise).

use serde_json::json;

use crate::indexed_fixture::{free, indices, nearest, palette_of, rgb_doc, to_indexed};

const W: u32 = 16;
const H: u32 = 8;

/// The classic 8 × 8 Bayer matrix, row y, column x.
const BAYER8: [[u32; 8]; 8] = [
    [0, 32, 8, 40, 2, 34, 10, 42],
    [48, 16, 56, 24, 50, 18, 58, 26],
    [12, 44, 4, 36, 14, 46, 6, 38],
    [60, 28, 52, 20, 62, 30, 54, 22],
    [3, 35, 11, 43, 1, 33, 9, 41],
    [51, 19, 59, 27, 49, 17, 57, 25],
    [15, 47, 7, 39, 13, 45, 5, 37],
    [63, 31, 55, 23, 61, 29, 53, 21],
];

fn pattern_t(x: u32, y: u32) -> f64 {
    f64::from(2 * BAYER8[(y % 8) as usize][(x % 8) as usize] + 1) / 128.0
}

/// A Murmur3-style finalizer of the coordinates' mix, its top 24 bits as a
/// fraction of 2²⁴.
fn noise_t(x: u32, y: u32) -> f64 {
    let mut h = x
        .wrapping_mul(0x9E37_79B9)
        .wrapping_add(y.wrapping_mul(0x85EB_CA6B));
    h ^= h >> 16;
    h = h.wrapping_mul(0x85EB_CA6B);
    h ^= h >> 13;
    h = h.wrapping_mul(0xC2B2_AE35);
    h ^= h >> 16;
    f64::from(((h >> 8) as f32) * (1.0 / 16_777_216.0))
}

/// The 16 × 8 colour ramp; pixel (3, 2) clear when `clear`.
fn ramp(clear: bool) -> Vec<[u8; 4]> {
    let mut out = Vec::new();
    for y in 0..H {
        for x in 0..W {
            if clear && (x, y) == (3, 2) {
                out.push([10, 20, 30, 0]);
            } else {
                out.push([
                    (x * 16 + y) as u8,
                    (255 - x * 15 - y * 3) as u8,
                    (y * 30 + x) as u8,
                    255,
                ]);
            }
        }
    }
    out
}

/// The oracle's indices of `pixels` dithered by `t` onto `palette`.
fn oracle(
    pixels: &[[u8; 4]],
    palette: &[[u8; 3]],
    transparent: Option<u8>,
    t: fn(u32, u32) -> f64,
) -> Vec<i32> {
    pixels
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let (x, y) = (i as u32 % W, i as u32 / W);
            if p[3] < 128 {
                return i32::from(transparent.expect("a clear pixel needs t"));
            }
            let offset = (t(x, y) - 0.5) * 51.0;
            let w = [0, 1, 2].map(|c| (f64::from(p[c]) + offset).clamp(0.0, 255.0).round() as u8);
            nearest(palette, transparent, w)
        })
        .collect()
}

#[test]
fn pattern_and_noise_follow_their_declared_rule() {
    for (options, clear) in [
        (json!({"palette":"web"}), true),
        (json!({"palette":"uniform","colors":8}), false),
    ] {
        let pixels = ramp(clear);
        let rgb = rgb_doc(W, H, &pixels);
        for (dither, t) in [
            ("pattern", pattern_t as fn(u32, u32) -> f64),
            ("noise", noise_t),
        ] {
            let mut request = options.clone();
            request["dither"] = json!(dither);
            let doc = to_indexed(rgb, &request).unwrap();
            let (palette, transparent) = palette_of(doc);
            assert_eq!(transparent.is_some(), clear, "{request}");
            let none = {
                let mut plain = options.clone();
                plain["dither"] = json!("none");
                to_indexed(rgb, &plain).unwrap()
            };
            let got = indices(doc);
            assert_eq!(got, oracle(&pixels, &palette, transparent, t), "{request}");
            assert_ne!(got, indices(none), "{request}: the dither moves some pixel");
            free(none);
            free(doc);
        }
        free(rgb);
    }
}

#[test]
fn the_exact_palette_keeps_each_pixels_own_entry() {
    let pixels: Vec<[u8; 4]> = (0..32u32)
        .map(|i| {
            let g = ((i % 16) * 17) as u8;
            [g, g, g, 255]
        })
        .collect();
    let rgb = rgb_doc(16, 2, &pixels);
    let none = to_indexed(rgb, &json!({"palette":"exact","dither":"none"})).unwrap();
    for dither in ["pattern", "noise"] {
        let doc = to_indexed(rgb, &json!({"palette":"exact","dither":dither})).unwrap();
        assert_eq!(indices(doc), indices(none), "{dither}");
        assert_eq!(palette_of(doc), palette_of(none), "{dither}");
        free(doc);
    }
    free(none);
    free(rgb);
}

#[test]
fn amount_and_preserve_exact_are_diffusions() {
    let rgb = rgb_doc(W, H, &ramp(false));
    for options in [
        json!({"palette":"web","dither":"pattern","amount":50}),
        json!({"palette":"web","dither":"noise","preserve_exact":true}),
        json!({"palette":"web","dither":"none","amount":75}),
    ] {
        assert_eq!(
            to_indexed(rgb, &options).unwrap_err(),
            "mode options amount and preserve_exact apply to Diffusion only",
            "{options}"
        );
    }
    let ok = to_indexed(
        rgb,
        &json!({"palette":"web","dither":"pattern","preserve_exact":false}),
    )
    .unwrap();
    free(ok);
    free(rgb);
}

#[test]
fn forced_custom_leads_the_palette() {
    let rgb = rgb_doc(W, H, &ramp(false));
    let table = json!([[12, 34, 56], [200, 10, 90], [12, 34, 56]]);
    let uniform = to_indexed(
        rgb,
        &json!({"palette":"uniform","colors":32,"forced":"custom","forced_table":table}),
    )
    .unwrap();
    let (palette, _) = palette_of(uniform);
    // The repeat dropped, then the 3-level cube (29 ≤ 32).
    assert_eq!(palette.len(), 29);
    assert_eq!(palette[..3], [[12, 34, 56], [200, 10, 90], [0, 0, 0]]);
    free(uniform);
    let adaptive = to_indexed(
        rgb,
        &json!({"palette":"adaptive","colors":16,"forced":"custom","forced_table":table}),
    )
    .unwrap();
    let (palette, _) = palette_of(adaptive);
    assert_eq!(palette[..2], [[12, 34, 56], [200, 10, 90]]);
    assert!(!palette[2..].contains(&[12, 34, 56]));
    free(adaptive);
    free(rgb);
}

#[test]
fn forced_custom_refuses_by_sentence() {
    let rgb = rgb_doc(W, H, &ramp(false));
    let three = json!([[12, 34, 56], [200, 10, 90], [1, 2, 3]]);
    for (options, sentence) in [
        (
            json!({"palette":"uniform","colors":8,"forced":"custom","forced_table":three}),
            "The Uniform palette needs at least 11 colours with these options.",
        ),
        (
            json!({"palette":"adaptive","colors":3,"forced":"custom","forced_table":three}),
            "The Adaptive palette needs at least 4 colours with these options.",
        ),
        (
            json!({"palette":"uniform","forced":"custom"}),
            "forced custom needs mode option forced_table",
        ),
        (
            json!({"palette":"uniform","forced":"web","forced_table":three}),
            "mode option forced_table applies to forced custom only",
        ),
        (
            json!({"palette":"adaptive","forced_table":three}),
            "mode option forced_table applies to forced custom only",
        ),
        (
            json!({"palette":"uniform","forced":"custom","forced_table":[[1, 2, 3]]}),
            "a colour table holds 2 to 256 colours",
        ),
        (
            json!({"palette":"web","forced":"custom","forced_table":three}),
            "Forced colours apply to the Uniform and Adaptive palettes only.",
        ),
    ] {
        assert_eq!(
            to_indexed(rgb, &options).unwrap_err(),
            sentence,
            "{options}"
        );
    }
    free(rgb);
}

#[test]
fn the_unpublished_palettes_refuse_for_good() {
    let rgb = rgb_doc(W, H, &ramp(false));
    for (wire, title) in [
        ("local_perceptual", "Local Perceptual"),
        ("local_selective", "Local Selective"),
        ("master_perceptual", "Master Perceptual"),
        ("master_selective", "Master Selective"),
    ] {
        assert_eq!(
            to_indexed(rgb, &json!({ "palette": wire })).unwrap_err(),
            format!(
                "The {title} palette is not available: its algorithm is unpublished. Use \
                 Adaptive (median cut) instead."
            )
        );
    }
    free(rgb);
}
