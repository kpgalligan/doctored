//! What a content-aware call COSTS, and what it refuses: the guards on
//! `inpaint_plan`'s two ladders and the caps they measure against.
//!
//! Separate from `content_aware_tests` — which is about the pixels — because
//! these are timing and refusal assertions with their own fixtures, and
//! neither file should carry the other's. Shared fixtures live in
//! `tests/common`.
//!
//! # How a timing assertion is written here, and why
//!
//! A bare ratio between two wall-clock samples is not a test, it is a
//! coin toss with a bias: `make test` runs the test binaries in parallel, a
//! transient stall inflates the shorter measurement far more than the longer
//! one it is divided into, and a preview-timing test (deleted with the
//! preview) duly failed the project's own gate at 2.79x against a
//! 3.0x assertion while measuring 4.2-4.9x on an idle machine. A red suite
//! for a change that did not touch the code costs more than the assertion
//! buys.
//!
//! So every timing test here is written in three layers, and the first one
//! is the point. Layer 1 is the default test; layers 2 and 3 are its
//! `mod bench` twin, which `make cancel-bench` runs:
//!
//! 1. **The deterministic property.** The sentence a refusal carries, the
//!    pixels a fill changed. This is what
//!    the test is really about and it cannot go flaky.
//! 2. **A generous ABSOLUTE ceiling**, sized from the documented budget
//!    rather than from the measurement — `a_300_square_hole_in_a_2000_by_1500_photograph`'s
//!    own precedent, whose comment says why (0.53 s measured against a 15 s
//!    bound).
//! 3. **A ratio with at least twice the headroom of the measured value**, and
//!    still strictly inside the number the regression it guards produced. The
//!    measured value is printed either way, so the module docs' figures can
//!    be checked by eye.

use doctored_core::doc::DrDocument;
use doctored_core::doc_inpaint::{
    INPAINT_SEARCH_PIXELS, MAX_INPAINT_PLAN_PIXELS, MAX_INPAINT_TARGET_PIXELS,
};
use image::{Rgba, RgbaImage};

use crate::common;
use common::*;

// ------------------------------------------------------- a cheap refusal --

/// A dust selection is refused from the components' BOUNDING BOXES, before a
/// single working window is built.
///
/// The planner used to allocate a window and run a full exact distance
/// transform for every component before comparing anything against a cap, so
/// the promise its own module doc makes — "a refusal must be cheap" — held
/// only for a selection made of a few big parts. Measured against this same
/// entry point before the fix, on a 1200 x 1200 canvas: 20 000 one-pixel
/// specks refused in 0.60 s, 90 000 in 2.4 s, 250 000 in 4.9 s and 999 698 —
/// still under `INPAINT_SEARCH_PIXELS` — in 28.7 s, all of it on the
/// caller's thread, which in the app is the main one. It is now 6 ms, because
/// the first ladder is arithmetic over the boxes: a component dilates to at
/// least a quarter-disc of the ring, so the part COUNT alone settles a dust
/// selection.
#[test]
fn a_dust_selection_is_refused_before_a_window_is_built() {
    let side = 1200u32;
    let doc = doc_of(side, side, plasma);
    // Every (even, even) pixel of a 600 x 600 corner: 90 000 separate
    // 4-connected components of one pixel each — the shape a grain, dust or
    // halftone selection makes, and a tenth of a megapixel.
    let dust = mask_of(side, side, |x, y| {
        x.is_multiple_of(2) && y.is_multiple_of(2) && x < 600 && y < 600
    });
    let covered = dust.iter().filter(|v| **v >= 128).count() as u64;
    assert_eq!(covered, 90_000, "the fixture must be 90 000 specks");
    assert!(
        covered * 10 <= INPAINT_SEARCH_PIXELS,
        "and far under a megapixel, so only the work caps may refuse it"
    );

    let (out, message) = fill(&doc, 0, &dust, (side, side), (0, 3), false);
    assert!(out.is_none(), "90 000 separate parts cannot fit the caps");
    let message = message.expect("a cap must speak");
    assert!(
        message.contains(&MAX_INPAINT_TARGET_PIXELS.to_string())
            || message.contains("working area"),
        "the refusal must name a work cap: {message}"
    );

    // The scale to measure it against: a plain compact fill of the same
    // picture, which is the cheapest thing this op does at all.
    let compact = mask_of(side, side, |x, y| {
        (450..750).contains(&x) && (450..750).contains(&y)
    });
    let broken = punch(&doc, &compact, [255, 255, 255]);
    let (out, message) = fill(&broken, 0, &compact, (side, side), (0, 3), false);
    assert!(message.is_none(), "unexpected message: {message:?}");
    assert!(out.is_some(), "the compact fill must land");
}

/// A call whose working WINDOWS bust their budget is refused in the words of
/// working area, and refused cheaply.
///
/// `MAX_INPAINT_TARGET_PIXELS` bounds the region DILATED by its ring, and a
/// long thin component dilates to a small fraction of its bounding box — so
/// on its own it left the sum of the windows (the planes, the transform and
/// the integral image every component builds) unbounded. Measured: thirty
/// three-pixel scratches across a 3000 x 3000 photograph are 3.2 M dilated,
/// comfortably inside the work cap, and took 16.3 s.
#[test]
fn a_call_whose_windows_bust_their_budget_says_so() {
    let side = 2000u32;
    let doc = DrDocument::from_pixels(RgbaImage::from_pixel(side, side, Rgba([70, 80, 90, 255])));
    // Two-pixel-thick diagonals 14 px apart — thick because a one-pixel
    // diagonal is not 4-connected and would be one component per pixel.
    // Each spans the canvas, so each window is nearly the whole picture.
    let mut mask = vec![0u8; (side as usize) * (side as usize)];
    let mut parts = 0;
    for k in 0..143u32 {
        let offset = k * 14;
        if offset >= side {
            break;
        }
        parts += 1;
        for x in 0..side - offset {
            let y = x + offset;
            mask[(y * side + x) as usize] = 255;
            if y + 1 < side {
                mask[((y + 1) * side + x) as usize] = 255;
            }
        }
    }
    let covered = mask.iter().filter(|v| **v >= 128).count() as u64;
    assert!(
        parts > 100 && covered < INPAINT_SEARCH_PIXELS,
        "the fixture must be many long thin parts under a megapixel: {parts} parts, {covered} px"
    );

    let (out, message) = fill(&doc, 0, &mask, (side, side), (0, 1), false);
    assert!(out.is_none(), "the windows must not fit their budget");
    let message = message.expect("the budget must speak");
    assert!(
        message.contains("working area")
            && message.contains(&format!("{}", MAX_INPAINT_PLAN_PIXELS / 1_000_000)),
        "the refusal must be worded as working area and name the limit: {message}"
    );
    // One small region of the same picture, whose windows do fit, runs the
    // whole pipeline; the twin times it against this refusal.
    let one = mask_of(side, side, |x, y| {
        (900..1100).contains(&x) && (900..1100).contains(&y)
    });
    let _ = fill(&doc, 0, &one, (side, side), (0, 1), false);
}

/// A call in more than a million separate parts is refused before the
/// component walk, in the words of parts.
///
/// The covered count is no longer capped — a hole over a megapixel is lifted
/// — so the admission's component-record clamp (`min(records, 1,000,000)`)
/// needs its own refusal to stay a count of every record the walk makes. A
/// one-pixel checker is the densest such selection there is: every covered
/// pixel is its own 4-connected part.
#[test]
fn a_selection_in_over_a_million_parts_says_so() {
    let (w, h) = (1500u32, 1400u32);
    let doc = DrDocument::from_pixels(RgbaImage::from_pixel(w, h, Rgba([70, 80, 90, 255])));
    let checker = mask_of(w, h, |x, y| (x + y).is_multiple_of(2));
    let parts = checker.iter().filter(|v| **v >= 128).count() as u64;
    assert_eq!(parts, 1_050_000, "the fixture must be 1 050 000 specks");
    assert!(parts > INPAINT_SEARCH_PIXELS);

    let (out, message) = fill(&doc, 0, &checker, (w, h), (0, 3), false);
    assert!(out.is_none(), "over a million parts must be refused");
    let message = message.expect("the parts limit must speak");
    assert!(
        message.contains("separate parts")
            && message.contains(&parts.to_string())
            && message.contains(&INPAINT_SEARCH_PIXELS.to_string()),
        "the refusal must be worded as parts and name both numbers: {message}"
    );
}

/// The column ramp of the two large-fill twins: a value that depends on `x`
/// alone, so every column of a hole has its own colour above and below it.
fn column_ramp(x: u32, _y: u32) -> [u8; 3] {
    let v = (30 + x * 200 / 3000) as u8;
    [v, v / 2 + 20, 255 - v]
}

// ----------------------------------------------- the ring is not a knob --

/// The ring is a QUALITY control on a scattered selection too, which is what
/// the schema and the module doc both promise and what was false for the one
/// shape it mattered on.
///
/// Every component's working window used to be sized from the ring the
/// caller ASKED for rather than the one the narrowing settled on, so the
/// planner and the fill both paid `parts x ring²` with nothing bounding it.
/// Measured in the app on a 2000 x 1500 photograph with 1131 disjoint 12-px
/// specks: `ring: 21` returned in 3.0 s and `ring: 512` — the same selection,
/// neither cap firing — in 78.9 s, with the main thread blocked throughout.
#[test]
fn the_ring_does_not_drive_the_cost_of_a_scattered_selection() {
    let (w, h) = (1200u32, 900u32);
    let doc = doc_of(w, h, plasma);
    let speck = |v: u32| v > 20 && (v % 50) < 12;
    let mask = mask_of(w, h, |x, y| speck(x) && speck(y));
    let covered = mask.iter().filter(|v| **v >= 128).count() as u64;
    assert!(
        covered * 4 < INPAINT_SEARCH_PIXELS,
        "the fixture must be far under a megapixel: {covered} px"
    );
    let broken = punch(&doc, &mask, [255, 255, 255]);
    let run = |ring: u32| {
        let (out, message) = fill(&broken, 0, &mask, (w, h), (ring, 4), false);
        assert!(
            message.is_none(),
            "unexpected message at ring {ring}: {message:?}"
        );
        assert!(out.is_some(), "the fill must land at ring {ring}");
    };
    run(21);
    run(512);
}

mod bench {
    use super::*;
    use std::time::Instant;

    /// Fills `mask` punched magenta in a 3000 x 2600 [`column_ramp`] and
    /// returns the time the fill took.
    fn large_fill(what: &str, mask: &[u8]) -> std::time::Duration {
        let (w, h) = (3000u32, 2600u32);
        let doc = doc_of(w, h, column_ramp);
        let covered = mask.iter().filter(|v| **v >= 128).count() as u64;
        assert!(
            covered > INPAINT_SEARCH_PIXELS,
            "{what}: the fixture must be over a megapixel: {covered} px"
        );
        let broken = punch(&doc, mask, [255, 0, 255]);
        let started = Instant::now();
        let (out, message) = fill(&broken, 0, mask, (w, h), (0, 7), false);
        let elapsed = started.elapsed();
        assert!(message.is_none(), "{what}: unexpected message: {message:?}");
        assert!(out.is_some(), "{what}: the fill must land");
        println!("{what}: {covered} px filled in {elapsed:?}");
        elapsed
    }

    /// A hole over a megapixel fills in bounded time. A 2000 x 2000
    /// hole is 4 MP, the most lift 1 takes: its search runs over the 1 MP the
    /// hole is at half scale. The bound was declared before the first run.
    #[test]
    #[ignore = "make cancel-bench: a wall-clock verdict"]
    fn a_large_hole_fills_in_bounded_time() {
        let mask = mask_of(3000, 2600, |x, y| {
            (500..2500).contains(&x) && (300..2300).contains(&y)
        });
        let elapsed = large_fill("a 2000 x 2000 hole in 3000 x 2600", &mask);
        assert!(
            elapsed.as_secs_f64() <= 30.0,
            "a 4 MP hole must fill within 30 s: {elapsed:?}"
        );
    }

    /// The same for the shape the margin test used to leave at full
    /// resolution — a band from corner to corner, whose box is the canvas, so
    /// `should_coarsen` builds no pyramid for it and the lift's forced depth
    /// is what keeps its search at half scale.
    #[test]
    #[ignore = "make cancel-bench: a wall-clock verdict"]
    fn a_large_band_fills_in_bounded_time() {
        let mask = mask_of(3000, 2600, |x, y| {
            (y as f32 - (2600.0 / 3000.0) * x as f32).abs() <= 700.0
        });
        let elapsed = large_fill("a 1400 px corner-to-corner band in 3000 x 2600", &mask);
        assert!(
            elapsed.as_secs_f64() <= 60.0,
            "a 3.6 MP band must fill within 60 s: {elapsed:?}"
        );
    }

    #[test]
    #[ignore = "make cancel-bench: a wall-clock verdict"]
    fn a_dust_selection_is_refused_before_a_window_is_built() {
        let side = 1200u32;
        let doc = doc_of(side, side, plasma);
        // Every (even, even) pixel of a 600 x 600 corner: 90 000 separate
        // 4-connected components of one pixel each — the shape a grain, dust or
        // halftone selection makes, and a tenth of a megapixel.
        let dust = mask_of(side, side, |x, y| {
            x.is_multiple_of(2) && y.is_multiple_of(2) && x < 600 && y < 600
        });
        let covered = dust.iter().filter(|v| **v >= 128).count() as u64;
        assert_eq!(covered, 90_000, "the fixture must be 90 000 specks");
        assert!(
            covered * 10 <= INPAINT_SEARCH_PIXELS,
            "and far under a megapixel, so only the work caps may refuse it"
        );

        let started = Instant::now();
        let (out, message) = fill(&doc, 0, &dust, (side, side), (0, 3), false);
        let refused = started.elapsed();
        assert!(out.is_none(), "90 000 separate parts cannot fit the caps");
        let message = message.expect("a cap must speak");
        assert!(
            message.contains(&MAX_INPAINT_TARGET_PIXELS.to_string())
                || message.contains("working area"),
            "the refusal must name a work cap: {message}"
        );

        // The scale to measure it against: a plain compact fill of the same
        // picture, which is the cheapest thing this op does at all.
        let compact = mask_of(side, side, |x, y| {
            (450..750).contains(&x) && (450..750).contains(&y)
        });
        let broken = punch(&doc, &compact, [255, 255, 255]);
        let started = Instant::now();
        let (out, message) = fill(&broken, 0, &compact, (side, side), (0, 3), false);
        let filled = started.elapsed();
        assert!(message.is_none(), "unexpected message: {message:?}");
        assert!(out.is_some(), "the compact fill must land");

        println!("dust: refused in {refused:?}, a 300 x 300 fill takes {filled:?}");
        assert!(
            refused.as_secs_f64() * 10.0 < filled.as_secs_f64(),
            "a refusal must cost a fraction of the smallest fill, not a multiple of it: \
             refused {refused:?}, filled {filled:?}"
        );
    }

    #[test]
    #[ignore = "make cancel-bench: a wall-clock verdict"]
    fn a_call_whose_windows_bust_their_budget_says_so() {
        let side = 2000u32;
        let doc =
            DrDocument::from_pixels(RgbaImage::from_pixel(side, side, Rgba([70, 80, 90, 255])));
        // Two-pixel-thick diagonals 14 px apart — thick because a one-pixel
        // diagonal is not 4-connected and would be one component per pixel.
        // Each spans the canvas, so each window is nearly the whole picture.
        let mut mask = vec![0u8; (side as usize) * (side as usize)];
        let mut parts = 0;
        for k in 0..143u32 {
            let offset = k * 14;
            if offset >= side {
                break;
            }
            parts += 1;
            for x in 0..side - offset {
                let y = x + offset;
                mask[(y * side + x) as usize] = 255;
                if y + 1 < side {
                    mask[((y + 1) * side + x) as usize] = 255;
                }
            }
        }
        let covered = mask.iter().filter(|v| **v >= 128).count() as u64;
        assert!(
            parts > 100 && covered < INPAINT_SEARCH_PIXELS,
            "the fixture must be many long thin parts under a megapixel: {parts} parts, {covered} px"
        );

        let started = Instant::now();
        let (out, message) = fill(&doc, 0, &mask, (side, side), (0, 1), false);
        let refused = started.elapsed();
        assert!(out.is_none(), "the windows must not fit their budget");
        let message = message.expect("the budget must speak");
        println!("{parts} spanning scratches ({covered} px): refused in {refused:?} — {message}");
        assert!(
            message.contains("working area")
                && message.contains(&format!("{}", MAX_INPAINT_PLAN_PIXELS / 1_000_000)),
            "the refusal must be worded as working area and name the limit: {message}"
        );
        // And it is the CHEAP ladder that refused it: one small region of the
        // same picture, whose windows do fit, costs the whole pipeline — and far
        // more than this refusal did.
        let one = mask_of(side, side, |x, y| {
            (900..1100).contains(&x) && (900..1100).contains(&y)
        });
        let started = Instant::now();
        let _ = fill(&doc, 0, &one, (side, side), (0, 1), false);
        let filled = started.elapsed();
        println!("one 200 px region of the same picture fills in {filled:?}");
        // Measured 13 ms against 100 ms; the ratio the fix was written against was
        // about 1 (essentially the whole cost of a fill, paid to be told no), so
        // two is a bound a loaded machine cannot cross and a regression cannot
        // hide behind.
        assert!(
            refused.as_secs_f64() < 2.0,
            "a refusal must be cheap in absolute terms: {refused:?}"
        );
        assert!(
            refused.as_secs_f64() * 2.0 < filled.as_secs_f64(),
            "the refusal must not cost like a fill: refused {refused:?}, filled {filled:?}"
        );
    }

    #[test]
    #[ignore = "make cancel-bench: a wall-clock verdict"]
    fn the_ring_does_not_drive_the_cost_of_a_scattered_selection() {
        let (w, h) = (1200u32, 900u32);
        let doc = doc_of(w, h, plasma);
        let speck = |v: u32| v > 20 && (v % 50) < 12;
        let mask = mask_of(w, h, |x, y| speck(x) && speck(y));
        let covered = mask.iter().filter(|v| **v >= 128).count() as u64;
        assert!(
            covered * 4 < INPAINT_SEARCH_PIXELS,
            "the fixture must be far under a megapixel: {covered} px"
        );
        let broken = punch(&doc, &mask, [255, 255, 255]);
        let run = |ring: u32| {
            let started = Instant::now();
            let (out, message) = fill(&broken, 0, &mask, (w, h), (ring, 4), false);
            let elapsed = started.elapsed();
            assert!(
                message.is_none(),
                "unexpected message at ring {ring}: {message:?}"
            );
            assert!(out.is_some(), "the fill must land at ring {ring}");
            elapsed
        };
        let narrow = run(21);
        let wide = run(512);
        println!("scattered selection: ring 21 {narrow:?}, ring 512 {wide:?}");
        // Measured 1.29 s against 1.81 s, a ratio of 1.4; the defect this guards
        // was 26x, so four leaves the assertion three times the headroom of the
        // measurement and still an order below what it has to catch.
        assert!(
            wide.as_secs_f64() < narrow.as_secs_f64() * 4.0,
            "the ring must not multiply the cost of a scattered selection: \
             ring 21 {narrow:?}, ring 512 {wide:?}"
        );
    }
}
