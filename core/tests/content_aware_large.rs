//! Content-Aware Fill over a megapixel: the lift.
//!
//! A call covering more than `INPAINT_SEARCH_PIXELS` used to be refused. It
//! is now LIFTED (`doc_inpaint::inpaint_lift`): the search runs at `1/2^L`
//! scale and each finer level takes the coarser field doubled and votes
//! once. Everything here runs through `dr_doc_content_aware_fill_edit`
//! (`common::fill`) on synthetic pictures whose correct fill is known without
//! the inpainter:
//!
//! - a flat field must come back byte for byte;
//! - a column ramp (a value from `x` alone) has, for every column of the
//!   hole, its own colour above and below it, so the error against the
//!   unpunched ramp is the measure;
//! - a random two-colour texture has no single correct continuation, so it
//!   is held to what a fill of it must keep: the level (its mean) and the
//!   sharpness (the mean squared horizontal step), each against the same
//!   statistic of the picture outside the hole, and the same bytes for the
//!   same seed.
//!
//! Every bound was declared in the batch's spec before the first run. The
//! two shapes at the end are the ones the lift had to be made safe for: a
//! band whose box touches all four canvas edges (no pyramid by the margin
//! test, so the lift forces its levels) and a hole too narrow to reduce at
//! all (the search guard refuses it).
//!
//! One test is at lift 0 and is here for the ramp it shares: a hole of
//! exactly a megapixel with one narrow pair of margins, the shape
//! `patchmatch::should_coarsen`'s per-axis rule was written for.

use doctored_core::doc::DrDocument;
use doctored_core::doc_inpaint::{inpaint_lift, INPAINT_SEARCH_PIXELS};

use crate::common;
use common::*;

/// The fill, asserted to land without a message; its layer bytes.
fn large_fill(doc: &DrDocument, mask: &[u8], dims: (u32, u32), seed: u64) -> Vec<u8> {
    let covered = mask.iter().filter(|v| **v >= 128).count() as u64;
    assert!(
        covered > INPAINT_SEARCH_PIXELS,
        "the fixture must be over a megapixel: {covered} px"
    );
    let (out, message) = fill(doc, 0, mask, dims, (0, seed), false);
    assert!(message.is_none(), "unexpected message: {message:?}");
    layer_bytes(&out.expect("a hole over a megapixel must fill"), 0)
}

/// `got` against `want` (both RGBA8, `mask`-sized): every pixel the mask
/// leaves out must be byte-identical; over the masked ones, the mean and the
/// largest absolute difference of the colour channels.
fn error_inside(got: &[u8], want: &[u8], mask: &[u8], w: u32) -> (f64, u8) {
    let (mut sum, mut count, mut worst) = (0u64, 0u64, 0u8);
    for (i, covered) in mask.iter().enumerate() {
        let (g, e) = (&got[i * 4..i * 4 + 4], &want[i * 4..i * 4 + 4]);
        if *covered == 0 {
            assert_eq!(
                g,
                e,
                "pixel ({}, {}) outside the selection moved",
                i as u32 % w,
                i as u32 / w
            );
            continue;
        }
        assert_eq!(g[3], e[3], "alpha moved inside the selection");
        for c in 0..3 {
            let d = g[c].abs_diff(e[c]);
            sum += u64::from(d);
            count += 1;
            worst = worst.max(d);
        }
    }
    (sum as f64 / count as f64, worst)
}

/// A value from `x` alone, 0.1 code a pixel over a 2000 px canvas.
fn column_ramp(x: u32, _y: u32) -> [u8; 3] {
    let v = (30 + x / 10) as u8;
    [v, v / 2 + 20, 255 - v]
}

#[test]
fn the_lift_is_one_level_per_factor_of_four() {
    for (covered, lift) in [
        (1u64, 0u32),
        (1_000_000, 0),
        (1_000_001, 1),
        (4_000_000, 1),
        (4_000_001, 2),
        (16_000_001, 3),
    ] {
        assert_eq!(inpaint_lift(covered), lift, "{covered} covered pixels");
    }
}

/// A 1300 x 1000 hole punched magenta in a flat 1500 x 1300 field fills to
/// the field's exact bytes: every source patch is the field's colour, and the
/// membrane solve is exact for a zero mismatch.
#[test]
fn a_large_hole_in_a_flat_field_comes_back_exact() {
    let (w, h) = (1500u32, 1300u32);
    let field = [90u8, 120, 150];
    let doc = doc_of(w, h, |_, _| field);
    let mask = mask_of(w, h, |x, y| {
        (100..1400).contains(&x) && (150..1150).contains(&y)
    });
    let broken = punch(&doc, &mask, [255, 0, 255]);
    let bytes = large_fill(&broken, &mask, (w, h), 11);
    for (i, px) in bytes.chunks_exact(4).enumerate() {
        assert_eq!(
            px,
            [field[0], field[1], field[2], 255],
            "pixel ({}, {})",
            i as u32 % w,
            i as u32 / w
        );
    }
}

/// A 1200 x 1000 hole in a 2000 x 1500 column ramp: mean absolute error at
/// most 4 codes, no channel off by more than 40, nothing outside the hole
/// moved.
#[test]
fn a_large_hole_in_a_ramp_follows_the_ramp() {
    let (w, h) = (2000u32, 1500u32);
    let doc = doc_of(w, h, column_ramp);
    let mask = mask_of(w, h, |x, y| {
        (400..1600).contains(&x) && (250..1250).contains(&y)
    });
    let broken = punch(&doc, &mask, [255, 0, 255]);
    let bytes = large_fill(&broken, &mask, (w, h), 12);
    let (mean, worst) = error_inside(&bytes, &layer_bytes(&doc, 0), &mask, w);
    println!("1200 x 1000 hole in a ramp: mean abs error {mean:.3}, max {worst}");
    assert!(mean <= 4.0, "mean absolute error {mean:.3} codes");
    assert!(worst <= 40, "largest error {worst} codes");
}

/// One narrow pair of margins, at lift 0: a 1000 x 1000 hole, exactly
/// a megapixel, at (300, 150) in the 1600 x 1300 column ramp. It has 300 px
/// beside it and 150 px above and below, and only the bands above and below
/// hold the hole's own columns.
///
/// The pyramid used to be built on the widest margin alone, so at its
/// coarsest level those two bands were under one patch, every source patch
/// lay beside the hole, and this fill measured a mean absolute error of
/// 22.405 codes (largest 44). `patchmatch::should_coarsen` now stops one
/// level sooner for the narrower axis. The bound is what the same hole
/// measured with 300 px on every side before that change (0.778, largest 8):
/// a mean of at most 1 code, no channel off by more than 8, nothing outside
/// the hole moved.
#[test]
fn a_megapixel_hole_with_narrow_margins_follows_the_ramp() {
    let (w, h) = (1600u32, 1300u32);
    let doc = doc_of(w, h, column_ramp);
    let mask = mask_of(w, h, |x, y| {
        (300..1300).contains(&x) && (150..1150).contains(&y)
    });
    let covered = mask.iter().filter(|v| **v >= 128).count() as u64;
    assert_eq!(covered, INPAINT_SEARCH_PIXELS, "the fixture is lift 0");
    let broken = punch(&doc, &mask, [255, 0, 255]);
    let (out, message) = fill(&broken, 0, &mask, (w, h), (0, 12), false);
    assert!(message.is_none(), "unexpected message: {message:?}");
    let bytes = layer_bytes(&out.expect("the hole must fill"), 0);
    let (mean, worst) = error_inside(&bytes, &layer_bytes(&doc, 0), &mask, w);
    println!(
        "1000 x 1000 hole in a ramp, 150 px above and below: mean abs error {mean:.3}, max {worst}"
    );
    assert!(mean <= 1.0, "mean absolute error {mean:.3} codes");
    assert!(worst <= 8, "largest error {worst} codes");
}

/// The two colours of [`cells`].
const CELL: [[u8; 3]; 2] = [[60, 70, 80], [190, 180, 170]];

/// A random two-colour texture in 3 px cells: a fixed hash of the cell picks
/// the colour, so it has no period and no direction.
fn cells(x: u32, y: u32) -> [u8; 3] {
    let (cx, cy) = (x / 3, y / 3);
    let hash =
        (cx.wrapping_mul(0x9E37_79B1) ^ cy.wrapping_mul(0x85EB_CA6B)).wrapping_mul(0xC2B2_AE35);
    CELL[((hash >> 15) & 1) as usize]
}

/// The mean of the colour channels and the mean squared horizontal first
/// difference of them, over the pixels `inside` admits (the difference where
/// it admits both the pixel and its right neighbour).
fn level_and_sharpness(
    bytes: &[u8],
    w: u32,
    h: u32,
    inside: impl Fn(u32, u32) -> bool,
) -> (f64, f64) {
    let (mut sum, mut count) = (0f64, 0u64);
    let (mut steps, mut pairs) = (0f64, 0u64);
    for y in 0..h {
        for x in 0..w {
            if !inside(x, y) {
                continue;
            }
            let i = ((y * w + x) * 4) as usize;
            for c in 0..3 {
                sum += f64::from(bytes[i + c]);
                count += 1;
            }
            if x + 1 < w && inside(x + 1, y) {
                for c in 0..3 {
                    let d = f64::from(bytes[i + c]) - f64::from(bytes[i + 4 + c]);
                    steps += d * d;
                    pairs += 1;
                }
            }
        }
    }
    (sum / count as f64, steps / pairs as f64)
}

/// A 1200 x 1100 hole in an 1800 x 1500 random texture. Nothing outside the
/// hole moves; over the hole eroded by 64 px the mean is within 20 codes of
/// the unselected picture's and the mean squared horizontal step is at least
/// 0.4 of it (a fill that averaged the texture away would be near 0); and
/// the same seed gives the same bytes.
#[test]
fn a_large_hole_in_a_texture_keeps_its_level_and_its_grain() {
    let (w, h) = (1800u32, 1500u32);
    let doc = doc_of(w, h, cells);
    let hole = |x: u32, y: u32| (300..1500).contains(&x) && (200..1300).contains(&y);
    let mask = mask_of(w, h, hole);
    let broken = punch(&doc, &mask, [255, 0, 255]);
    let bytes = large_fill(&broken, &mask, (w, h), 13);
    let original = layer_bytes(&doc, 0);
    for (i, covered) in mask.iter().enumerate() {
        if *covered == 0 {
            assert_eq!(
                bytes[i * 4..i * 4 + 4],
                original[i * 4..i * 4 + 4],
                "pixel ({}, {}) outside the selection moved",
                i as u32 % w,
                i as u32 / w
            );
        }
    }
    let eroded = |x: u32, y: u32| (364..1436).contains(&x) && (264..1236).contains(&y);
    let (band_mean, band_steps) = level_and_sharpness(&bytes, w, h, |x, y| !hole(x, y));
    let (mean, steps) = level_and_sharpness(&bytes, w, h, eroded);
    println!(
        "1200 x 1100 hole in a texture: mean {mean:.2} against {band_mean:.2}, \
         squared step {steps:.1} against {band_steps:.1} ({:.3} of it)",
        steps / band_steps
    );
    assert!(
        (mean - band_mean).abs() <= 20.0,
        "the fill's level {mean:.2} against the picture's {band_mean:.2}"
    );
    assert!(
        steps >= 0.4 * band_steps,
        "the fill's squared step {steps:.1} against the picture's {band_steps:.1}"
    );
    let again = large_fill(&broken, &mask, (w, h), 13);
    assert!(bytes == again, "the same seed must give the same bytes");
}

/// The band: a corner-to-corner hole whose bounding box is the whole canvas.
/// `should_coarsen`'s margin test builds no pyramid for it, so before the
/// lift's forced depth it would have searched over a megapixel at full
/// resolution on the single-level schedule. It fills, to the ramp: mean
/// absolute error at most 6 codes, no channel off by more than 48, nothing
/// outside it moved.
#[test]
fn a_band_touching_all_four_edges_fills() {
    let (w, h) = (2000u32, 1500u32);
    let doc = doc_of(w, h, column_ramp);
    let mask = mask_of(w, h, |x, y| (y as f32 - 0.75 * x as f32).abs() <= 320.0);
    let covered = mask.iter().filter(|v| **v >= 128).count() as u64;
    assert!(
        covered > 1_000_000,
        "the band must be over a megapixel: {covered} px"
    );
    let (mut x0, mut y0, mut x1, mut y1) = (w, h, 0u32, 0u32);
    for (i, c) in mask.iter().enumerate() {
        if *c > 0 {
            let (x, y) = (i as u32 % w, i as u32 / w);
            (x0, y0, x1, y1) = (x0.min(x), y0.min(y), x1.max(x), y1.max(y));
        }
    }
    assert_eq!(
        (x0, y0, x1, y1),
        (0, 0, w - 1, h - 1),
        "the band's box must be the canvas"
    );
    let broken = punch(&doc, &mask, [255, 0, 255]);
    let bytes = large_fill(&broken, &mask, (w, h), 14);
    let (mean, worst) = error_inside(&bytes, &layer_bytes(&doc, 0), &mask, w);
    println!("{covered} px corner-to-corner band in a ramp: mean abs error {mean:.3}, max {worst}");
    assert!(mean <= 6.0, "mean absolute error {mean:.3} codes");
    assert!(worst <= 48, "largest error {worst} codes");
}

/// The guard: a hole over a megapixel in a window too narrow to reduce. A
/// 60000 x 24 canvas is under four patches high, so no coarser level can be
/// built and the lift has nothing to lift to; the part is refused before any
/// search, naming what it would have searched and the limit.
#[test]
fn a_large_hole_too_narrow_to_reduce_is_refused() {
    let (w, h) = (60_000u32, 24u32);
    let doc = doc_of(w, h, |_, _| [90, 120, 150]);
    let mask = mask_of(w, h, |x, _| (200..59_800).contains(&x));
    let covered = mask.iter().filter(|v| **v >= 128).count() as u64;
    assert_eq!(covered, 59_600 * 24);
    let broken = punch(&doc, &mask, [255, 0, 255]);
    let (out, message) = fill(&broken, 0, &mask, (w, h), (0, 15), false);
    assert!(out.is_none(), "the narrow hole must be refused");
    assert_eq!(
        message.as_deref(),
        Some(
            "one part of the selection has 1430400 pixels to search at the smallest scale its \
             working box allows, because that box is too narrow to work at a smaller one; \
             content-aware fill searches at most 1000000 pixels at one scale. Select less of \
             that part, or fill it in pieces."
        ),
        "the search guard must speak"
    );
}
