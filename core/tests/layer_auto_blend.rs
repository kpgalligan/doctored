//! Edit ▸ Auto-Blend Layers ▸ Stack Images through the FFI
//! (`dr_doc_auto_blend`, `dr_precision_doc_auto_blend` and their plans), at 8
//! and 16 bits and in a 32-bit float document.
//!
//! # The scenes
//!
//! Everything is written here. `scene` is white noise in 0.2…0.8 per channel
//! from an integer hash, in unit floats (linear light in a float document).
//! `box_blur` is a separable 7×7 box mean with clamped edges in f64, sharing
//! no line with the core. Frame `k` of a strip stack is the scene inside its
//! own vertical strip and the blurred scene everywhere else, so the correct
//! answer is known: frame `k` wins strip `k`, and the blend is the scene.
//! A depth stores a unit value as `round(v · 255)`, `round(v · 65535)` or the
//! f32 itself, and every comparison is made against the stored value.
//!
//! # The bounds, declared before the first run
//!
//! * `MARGIN` = 14 pixels. A region's INTERIOR is every pixel at least this
//!   far from a boundary between two regions. The score is averaged over a
//!   box of radius 8 around a band of radius 4 (12 pixels) and the vote adds
//!   no reach, so a pixel 14 pixels inside its region sees only that region.
//! * `SHARE` = 0.99: the share of a region's interior its own frame's mask
//!   must reveal.
//! * `COLOUR` = 3/255 of the unit range: how far the merged layer may sit
//!   from the scene at an interior pixel, per channel. At an interior pixel
//!   every level but the coarsest has a weight of exactly 1 for the winning
//!   frame, so only the coarsest level mixes frames, and there the blurred
//!   and the sharp frame differ by under 1/255.
//! * `STEP` = 0.5: the largest step of a seamless blend across a mask
//!   boundary, as a fraction of the hard composite's step, when one frame is
//!   0.08 brighter. A step is the difference of two adjacent 4-column means
//!   of the column profile of (picture − scene).
//! * `RING` = 0.01: the share of the 12-pixel ring around a transparent hole
//!   the holed frame may win, when it is the blurred frame there.
//! * `FOOTPRINT` = 28 pixels: the reach of the coarsest level of a 4-level
//!   pyramid (a 3-level one reaches 12). Covered pixels within it of an
//!   uncovered band or of a hole must match the scene within `COLOUR`.
//! * `HOT_SHARE` = 0.90: the share of a region's interior the sharp frame
//!   keeps when the blurred frame holds three 5000-valued pixels there.
//! * `ABOVE_WHITE` = 5%: how far the merged layer may sit from a 6.0-valued
//!   patch of the scene, relative.
#![allow(clippy::needless_range_loop)]

use std::ptr;
use std::sync::Arc;

use doctored_core::doc::DrDocument;
use doctored_core::doc_mask::MaskProperties;
use doctored_core::ffi_jobs::{dr_job_control_cancel, dr_job_control_create, dr_job_control_free};
use doctored_core::icc::IccProfile;
use doctored_core::precision_document::PrecisionDocumentLayer;
use doctored_core::precision_mask::{PrecisionMask, PrecisionPlane};
use doctored_core::precision_render::MAX_RECORDS;
use image::{GrayImage, Luma, Rgba, RgbaImage};
use serde_json::{json, Value};

use crate::auto_blend_fixture::{
    assert_partition, assert_refused, byte_document, byte_image, request, typed_document,
    typed_layer, Depth, Frame, Stack, LIMIT,
};
const MARGIN: i64 = 14;
const SHARE: f64 = 0.99;
const COLOUR: f64 = 3.0 / 255.0;
const STEP: f64 = 0.5;
const RING: f64 = 0.01;
const FOOTPRINT: i64 = 28;
const HOT_SHARE: f64 = 0.90;
const ABOVE_WHITE: f64 = 0.05;
/// A strip's width and the strip stacks' height.
const STRIP: u32 = 64;
const HEIGHT: u32 = 96;
/// `drdc::MAX_DRDC_LAYERS`, the byte format's layer cap.
const MAX_BYTE_LAYERS: usize = 1024;

// ---------------------------------------------------------------- the scenes --

fn hash(i: u32) -> u32 {
    let mut x = i.wrapping_mul(0x9E37_79B9) ^ 0x85EB_CA6B;
    x ^= x >> 15;
    x = x.wrapping_mul(0x2C1B_3C6D);
    x ^ (x >> 12)
}

/// White noise in 0.2…0.8, one independent value per pixel and channel.
fn scene(w: u32, h: u32) -> Vec<[f64; 3]> {
    (0..w * h)
        .map(|i| {
            let unit = |c: u32| f64::from(hash(i * 3 + c + 17) & 0xffff) / 65535.0;
            [
                0.2 + 0.6 * unit(0),
                0.2 + 0.6 * unit(1),
                0.2 + 0.6 * unit(2),
            ]
        })
        .collect()
}

/// A separable box mean of radius 3 with clamped edges.
fn box_blur(source: &[[f64; 3]], w: u32, h: u32) -> Vec<[f64; 3]> {
    let (w, h) = (w as i64, h as i64);
    let pass = |input: &[[f64; 3]], horizontal: bool| -> Vec<[f64; 3]> {
        let mut out = vec![[0.0; 3]; input.len()];
        for y in 0..h {
            for x in 0..w {
                let mut sum = [0.0; 3];
                for d in -3..=3i64 {
                    let (sx, sy) = if horizontal {
                        ((x + d).clamp(0, w - 1), y)
                    } else {
                        (x, (y + d).clamp(0, h - 1))
                    };
                    let p = input[(sy * w + sx) as usize];
                    for c in 0..3 {
                        sum[c] += p[c];
                    }
                }
                out[(y * w + x) as usize] = sum.map(|v| v / 7.0);
            }
        }
        out
    };
    pass(&pass(source, true), false)
}

/// A canvas-sized opaque frame: `sharp` where `own(x, y)`, `blurred`
/// elsewhere.
fn frame_of(
    (w, h): (u32, u32),
    sharp: &[[f64; 3]],
    blurred: &[[f64; 3]],
    own: impl Fn(u32, u32) -> bool,
) -> Frame {
    let pixels = (0..w * h)
        .map(|i| {
            let p = if own(i % w, i / w) {
                sharp[i as usize]
            } else {
                blurred[i as usize]
            };
            [p[0], p[1], p[2], 1.0]
        })
        .collect();
    Frame {
        dims: (w, h),
        offset: (0, 0),
        pixels,
    }
}

/// `n` strip frames over a `64n × 96` canvas, and the scene.
fn strip_frames(n: usize) -> ((u32, u32), Vec<[f64; 3]>, Vec<Frame>) {
    let size = (STRIP * n as u32, HEIGHT);
    let sharp = scene(size.0, size.1);
    let blurred = box_blur(&sharp, size.0, size.1);
    let frames = (0..n as u32)
        .map(|k| frame_of(size, &sharp, &blurred, |x, _| x / STRIP == k))
        .collect();
    (size, sharp, frames)
}

/// The strip a column is inside the interior of, when it is.
fn strip_interior(x: u32, n: usize) -> Option<usize> {
    let x = i64::from(x);
    // Boundary `j` lies between columns 64j − 1 and 64j; a column's distance
    // to it is the number of columns between them.
    let clear = (1..n as i64).all(|j| {
        let boundary = j * i64::from(STRIP);
        let distance = if x >= boundary {
            x - boundary
        } else {
            boundary - 1 - x
        };
        distance >= MARGIN
    });
    clear.then_some((x / i64::from(STRIP)) as usize)
}

/// The largest difference between two adjacent 4-column means of the column
/// profile of `picture − reference`, in the green channel.
fn largest_step(picture: &[[f64; 4]], reference: &[[f64; 3]], (w, h): (u32, u32)) -> f64 {
    let profile: Vec<f64> = (0..w)
        .map(|x| {
            (0..h)
                .map(|y| {
                    let i = (y * w + x) as usize;
                    picture[i][1] - reference[i][1]
                })
                .sum::<f64>()
                / f64::from(h)
        })
        .collect();
    let mean = |from: usize| profile[from..from + 4].iter().sum::<f64>() / 4.0;
    (4..=w as usize - 4)
        .map(|x| (mean(x - 4) - mean(x)).abs())
        .fold(0.0, f64::max)
}

// --------------------------------------------- (a), (b), (d): the strip stacks --

fn strips(depth: Depth, n: usize) {
    let (size, sharp, frames) = strip_frames(n);
    let stack = Stack::build(depth, size, &frames);
    let layers: Vec<usize> = (0..n).collect();
    let (out, report) = stack
        .run(&request(&layers, true))
        .unwrap_or_else(|e| panic!("{depth:?} {n}: {e}"));
    assert_eq!(out.layer_count(), n + 1, "{depth:?} {n}");
    assert_eq!(report["merged_layer"], json!(n));
    assert_eq!(report["canvas_pixels"], json!(size.0 * size.1));
    assert_eq!(report["covered_pixels"], json!(size.0 * size.1));

    let masks: Vec<Vec<bool>> = layers.iter().map(|&k| out.mask(k)).collect();
    assert_partition(&masks, |_| true);
    for k in 0..n {
        let won = masks[k].iter().filter(|&&v| v).count();
        assert_eq!(report["layers"][k], json!({"layer": k, "won_pixels": won}));
        let (mut inside, mut revealed) = (0usize, 0usize);
        for (i, &value) in masks[k].iter().enumerate() {
            if strip_interior(i as u32 % size.0, n) == Some(k) {
                inside += 1;
                revealed += usize::from(value);
            }
        }
        assert!(inside > 1000, "strip {k} has an interior");
        assert!(
            revealed as f64 >= SHARE * inside as f64,
            "{depth:?} {n}: frame {k} reveals {revealed} of {inside} interior pixels"
        );
    }

    let merged = out.pixels(n);
    let mut worst = 0.0f64;
    for (i, pixel) in merged.iter().enumerate() {
        assert_eq!(pixel[3], 1.0, "an opaque stack blends opaque");
        if strip_interior(i as u32 % size.0, n).is_some() {
            for c in 0..3 {
                worst = worst.max((pixel[c] - depth.stored(sharp[i][c])).abs());
            }
        }
    }
    assert!(
        worst <= COLOUR,
        "{depth:?} {n}: the merged layer is {worst} from the scene"
    );
}

#[test]
fn each_frame_wins_its_sharp_region_and_the_blend_is_the_scene() {
    for depth in [Depth::Byte, Depth::Sixteen, Depth::Float] {
        strips(depth, 3);
    }
}

#[test]
fn two_to_five_frames() {
    for n in [2, 4, 5] {
        strips(Depth::Byte, n);
    }
    strips(Depth::Sixteen, 5);
}

// ------------------------------------------------------- (c): exposure offset --

#[test]
fn seamless_tones_soften_an_exposure_step() {
    const OFFSET: f64 = 0.08;
    let size = (128, 128);
    let sharp = scene(size.0, size.1);
    let blurred = box_blur(&sharp, size.0, size.1);
    let lifted = |values: &[[f64; 3]]| -> Vec<[f64; 3]> {
        values.iter().map(|p| p.map(|v| v + OFFSET)).collect()
    };
    let frames = [
        frame_of(size, &sharp, &blurred, |x, _| x < 64),
        frame_of(size, &lifted(&sharp), &lifted(&blurred), |x, _| x >= 64),
    ];
    for depth in [Depth::Byte, Depth::Sixteen] {
        let stack = Stack::build(depth, size, &frames);
        let (out, _) = stack.run(&request(&[0, 1], true)).unwrap();
        let masks = [out.mask(0), out.mask(1)];
        assert_partition(&masks, |_| true);
        // The hard composite is what the masked layers show: each pixel from
        // the frame whose mask reveals it.
        let hard: Vec<[f64; 4]> = (0..masks[0].len())
            .map(|i| frames[usize::from(masks[1][i])].pixels[i])
            .collect();
        let hard_step = largest_step(&hard, &sharp, size);
        let seamless_step = largest_step(&out.pixels(2), &sharp, size);
        assert!(
            hard_step >= 0.75 * OFFSET,
            "{depth:?}: the hard composite steps by {hard_step}"
        );
        assert!(
            seamless_step <= STEP * hard_step,
            "{depth:?}: the seamless step {seamless_step} against the hard step {hard_step}"
        );
    }
}

// ------------------------------------- (e): a hole and an uncovered band --

#[test]
fn a_hole_and_an_uncovered_band() {
    let canvas = (256u32, 96u32);
    // Both frames stop at column 208, so columns 208..256 are uncovered: 48
    // columns, wider than FOOTPRINT.
    let layer = (208u32, 96u32);
    let boundary = 80u32;
    let hole = |x: u32, y: u32| (150..166).contains(&x) && (40..56).contains(&y);
    let near_hole = |x: u32, y: u32, reach: i64| {
        let dx = (150 - i64::from(x)).max(i64::from(x) - 165).max(0);
        let dy = (40 - i64::from(y)).max(i64::from(y) - 55).max(0);
        dx.max(dy) <= reach
    };
    let sharp = scene(layer.0, layer.1);
    let blurred = box_blur(&sharp, layer.0, layer.1);
    let mut holed = frame_of(layer, &sharp, &blurred, |x, _| x < boundary);
    for y in 0..layer.1 {
        for x in 0..layer.0 {
            if hole(x, y) {
                // Loud hidden RGB under alpha 0: counted as detail, it would
                // win the ring; blended, it would show.
                holed.pixels[(y * layer.0 + x) as usize] = [1.0, 0.0, 1.0, 0.0];
            }
        }
    }
    let frames = [
        holed,
        frame_of(layer, &sharp, &blurred, |x, _| x >= boundary),
    ];
    for depth in [Depth::Byte, Depth::Sixteen, Depth::Float] {
        let stack = Stack::build(depth, canvas, &frames);
        let (out, report) = stack
            .run(&request(&[0, 1], true))
            .unwrap_or_else(|e| panic!("{depth:?}: {e}"));
        assert_eq!(report["covered_pixels"], json!(layer.0 * layer.1));
        assert_eq!(report["canvas_pixels"], json!(canvas.0 * canvas.1));
        let masks = [out.mask(0), out.mask(1)];
        let at = |i: usize| (i as u32 % canvas.0, i as u32 / canvas.0);
        assert_partition(&masks, |i| at(i).0 < layer.0);
        let merged = out.pixels(2);
        let (mut ring, mut ring_won) = (0usize, 0usize);
        let mut worst = 0.0f64;
        for i in 0..merged.len() {
            let (x, y) = at(i);
            if x >= layer.0 {
                assert_eq!(merged[i], [0.0; 4], "{depth:?}: uncovered pixel ({x}, {y})");
                continue;
            }
            assert_eq!(merged[i][3], 1.0, "{depth:?}: covered pixel ({x}, {y})");
            if hole(x, y) {
                assert!(
                    !masks[0][i],
                    "{depth:?}: the holed frame wins inside its hole"
                );
            } else if near_hole(x, y, 12) {
                ring += 1;
                ring_won += usize::from(masks[0][i]);
            }
            let beside_band = i64::from(layer.0) - i64::from(x) <= FOOTPRINT;
            if beside_band || near_hole(x, y, FOOTPRINT) {
                let reference = sharp[(y * layer.0 + x) as usize];
                for c in 0..3 {
                    worst = worst.max((merged[i][c] - depth.stored(reference[c])).abs());
                }
            }
        }
        assert!(ring > 500);
        assert!(
            ring_won as f64 <= RING * ring as f64,
            "{depth:?}: the holed frame wins {ring_won} of {ring} ring pixels"
        );
        assert!(
            worst <= COLOUR,
            "{depth:?}: beside the band and the hole the merged layer is {worst} from the scene"
        );
    }
}

// ------------------------------------------------------ (f): float highlights --

#[test]
fn a_float_highlight_does_not_take_a_blurred_region() {
    let size = (256u32, 128u32);
    let patch = |x: u32, y: u32| (180..188).contains(&x) && (100..108).contains(&y);
    let mut sharp = scene(size.0, size.1);
    for i in 0..sharp.len() {
        if patch(i as u32 % size.0, i as u32 / size.0) {
            sharp[i] = [6.0; 3];
        }
    }
    let blurred = box_blur(&sharp, size.0, size.1);
    let mut hot = frame_of(size, &sharp, &blurred, |x, _| x < 128);
    // Three highlights in the half where this frame is the blurred one.
    for (x, y) in [(150u32, 20u32), (190, 40), (230, 24)] {
        hot.pixels[(y * size.0 + x) as usize] = [5000.0, 5000.0, 5000.0, 1.0];
    }
    let frames = [hot, frame_of(size, &sharp, &blurred, |x, _| x >= 128)];
    let stack = Stack::build(Depth::Float, size, &frames);
    let (out, _) = stack.run(&request(&[0, 1], true)).unwrap();
    let masks = [out.mask(0), out.mask(1)];
    assert_partition(&masks, |_| true);
    let (mut inside, mut kept) = (0usize, 0usize);
    for (i, &value) in masks[1].iter().enumerate() {
        if i64::from(i as u32 % size.0) >= 128 + MARGIN {
            inside += 1;
            kept += usize::from(value);
        }
    }
    assert!(
        kept as f64 >= HOT_SHARE * inside as f64,
        "the sharp frame keeps {kept} of {inside} pixels beside the highlights"
    );
    // Values above white survive the blend: the patch's inner 4×4.
    let merged = out.pixels(2);
    for y in 102..106u32 {
        for x in 182..186u32 {
            let pixel = merged[(y * size.0 + x) as usize];
            for c in 0..3 {
                assert!(
                    (pixel[c] - 6.0).abs() <= ABOVE_WHITE * 6.0,
                    "({x}, {y}) channel {c} is {}",
                    pixel[c]
                );
            }
        }
    }
}

// ------------------------------------------------------------- (g): structure --

/// `[F0, F1, C, T]` with `C` clipped to `F1`, or `[F0, F1, T]`.
fn structured(depth: Depth, clipped: bool) -> Stack {
    let (size, _, frames) = strip_frames(2);
    let extra = |seed: u32| Frame {
        dims: (8, 8),
        offset: (3, 5),
        pixels: (0..64)
            .map(|i| [f64::from(hash(i + seed) & 0xff) / 255.0, 0.5, 0.25, 1.0])
            .collect(),
    };
    match depth {
        Depth::Byte => {
            let mut doc = byte_document(size, &frames);
            doc = doc
                .adding_image_layer(1, byte_image(&extra(7)), "T")
                .unwrap();
            doc.layers[2].offset = (3, 5);
            if clipped {
                doc = doc
                    .adding_image_layer(1, byte_image(&extra(9)), "C")
                    .unwrap();
                doc.layers[2].offset = (3, 5);
                doc.layers[2].clipped = true;
                assert_eq!(doc.layers[3].name, "T");
                assert!(!doc.layers[3].clipped);
            }
            Stack::Byte(doc)
        }
        _ => {
            let mut layers = vec![typed_layer("F0", &frames[0]), typed_layer("F1", &frames[1])];
            if clipped {
                layers.push(typed_layer("C", &extra(9)).with_clipped(true));
            }
            layers.push(typed_layer("T", &extra(7)));
            Stack::Typed(typed_document(size, layers))
        }
    }
}

#[test]
fn the_merged_layer_lands_above_the_top_layers_clip_run() {
    for depth in [Depth::Byte, Depth::Sixteen] {
        // Without `seamless` no layer is added.
        let plain = structured(depth, false);
        let (out, report) = plain.run(&request(&[0, 1], false)).unwrap();
        assert_eq!(out.layer_count(), 3, "{depth:?}");
        assert_eq!(report["merged_layer"], Value::Null);
        assert!(out.has_mask(0) && out.has_mask(1) && !out.has_mask(2));
        for layer in 0..3 {
            assert!(out.same_raster(layer, &plain, layer), "{depth:?} {layer}");
        }

        // With it, one layer directly above the top selected layer.
        let (out, report) = plain
            .run(&json!({"layers": [1, 0], "seamless": true, "name": "Stacked"}))
            .unwrap();
        assert_eq!(out.layer_count(), 4, "{depth:?}");
        assert_eq!(report["merged_layer"], json!(2));
        assert_eq!(
            plain.plan(&request(&[0, 1], true)).unwrap()["merged_index"],
            json!(2)
        );
        assert_eq!(out.name(2), "Stacked");
        assert!(!out.clipped(2) && !out.has_mask(2));
        assert_eq!(out.name(3), "T");
        for (after, before) in [(0, 0), (1, 1), (3, 2)] {
            assert!(out.same_raster(after, &plain, before), "{depth:?} {after}");
        }

        // A layer clipped to the top selected layer keeps its base: the
        // merged layer lands above it and is not clipped.
        let run = structured(depth, true);
        assert!(run.clipped(2));
        let (out, report) = run.run(&request(&[0, 1], true)).unwrap();
        assert_eq!(out.layer_count(), 5, "{depth:?}");
        assert_eq!(report["merged_layer"], json!(3));
        assert_eq!(
            run.plan(&request(&[0, 1], true)).unwrap()["merged_index"],
            json!(3)
        );
        assert_eq!(out.name(2), "C");
        assert!(
            out.clipped(2),
            "{depth:?}: the clipped layer keeps its flag"
        );
        assert_eq!(out.name(3), "Auto-Blend");
        assert!(!out.clipped(3), "{depth:?}: the merged layer is unclipped");
        assert_eq!(out.name(4), "T");
        assert!(!out.clipped(4));
        for (after, before) in [(0, 0), (1, 1), (2, 2), (4, 3)] {
            assert!(out.same_raster(after, &run, before), "{depth:?} {after}");
        }
    }
}

// -------------------------------------------------------------- (h): refusals --

#[test]
fn byte_refusals_name_the_layer() {
    let (size, _, frames) = strip_frames(2);
    let base = || {
        let mut doc = byte_document(size, &frames);
        doc = doc
            .adding_image_layer(1, byte_image(&frames[0]), "T")
            .unwrap();
        doc
    };
    let stack = Stack::Byte(base());
    assert_refused(&stack, &request(&[0], true), "at least two layers");
    assert_refused(&stack, &request(&[1, 1], true), "at least two layers");
    assert_refused(&stack, &request(&[0, 9], true), "layer 9 is out of range");
    assert_refused(
        &stack,
        &json!({"layers": [0, 1], "seamless": true, "feather": 2}),
        "invalid auto-blend request",
    );
    assert_refused(
        &stack,
        &json!({"layers": [0, 1]}),
        "invalid auto-blend request",
    );

    let (grouped, group, _, _) = base().group_layers(&[2], "G").unwrap();
    assert_refused(
        &Stack::Byte(grouped),
        &request(&[0, group], true),
        "“G” is a group",
    );

    let mut adjusted = base();
    adjusted.layers[2].meta = Some(r#"{"type":"adjust","op":"invert"}"#.into());
    assert_refused(
        &Stack::Byte(adjusted),
        &request(&[0, 2], false),
        "“T” is an adjustment layer",
    );

    let mut masked = base();
    masked.layers[1].mask = Some(Arc::new(GrayImage::from_pixel(size.0, size.1, Luma([255]))));
    assert_refused(
        &Stack::Byte(masked),
        &request(&[0, 1], true),
        "“F1” already has a layer mask. Apply or remove it before Auto-Blend Layers.",
    );

    let mut locked = base();
    locked.layers[0].locks = 7;
    assert_refused(
        &Stack::Byte(locked),
        &request(&[0, 1], true),
        "“F0” is locked",
    );

    let clear = Frame {
        dims: size,
        offset: (0, 0),
        pixels: vec![[0.3, 0.6, 0.9, 0.0]; (size.0 * size.1) as usize],
    };
    assert_refused(
        &Stack::Byte(byte_document(size, &[clear.clone(), clear])),
        &request(&[0, 1], true),
        "nothing to blend",
    );
}

#[test]
fn typed_refusals_name_the_layer() {
    let (size, _, frames) = strip_frames(2);
    let layers = || vec![typed_layer("F0", &frames[0]), typed_layer("F1", &frames[1])];
    let stack = Stack::Typed(typed_document(size, layers()));
    assert_refused(&stack, &request(&[1], true), "at least two layers");
    assert_refused(&stack, &request(&[0, 2], true), "layer 2 is out of range");

    let mut grouped = layers();
    let child = typed_layer("Inside", &frames[0]);
    grouped.push(PrecisionDocumentLayer::isolated_normal("G", vec![child], 1.0, true).unwrap());
    // Postorder: F0, F1, Inside, G.
    assert_refused(
        &Stack::Typed(typed_document(size, grouped)),
        &request(&[0, 3], true),
        "“G” is a group",
    );

    let description = json!({"type": "adjust", "op": "shadows_highlights",
        "evaluation": "precision_v1",
        "params": {"shadows": {"amount": 0.7, "tone": 0.45}, "highlights": {"amount": 0.35},
            "radius": 30}})
    .to_string();
    let mut adjusted = layers();
    adjusted.push(
        PrecisionDocumentLayer::adjustment("T", &description, 1.0, true, &IccProfile::srgb())
            .unwrap(),
    );
    assert_refused(
        &Stack::Typed(typed_document(size, adjusted)),
        &request(&[0, 2], false),
        "“T” is an adjustment layer",
    );

    let plane = PrecisionPlane::from_u16(
        size.0,
        size.1,
        vec![65535; (size.0 * size.1) as usize],
        None,
    )
    .unwrap();
    let mask = PrecisionMask::new(plane, MaskProperties::at((0, 0)), true).unwrap();
    let mut masked = layers();
    masked[1] = masked[1].clone().with_mask(Some(mask));
    assert_refused(
        &Stack::Typed(typed_document(size, masked)),
        &request(&[0, 1], true),
        "“F1” already has a layer mask. Apply or remove it before Auto-Blend Layers.",
    );

    let mut locked = layers();
    locked[0] = locked[0].clone().with_locks(7);
    assert_refused(
        &Stack::Typed(typed_document(size, locked)),
        &request(&[0, 1], false),
        "“F0” is locked",
    );

    let clear = Frame {
        dims: size,
        offset: (0, 0),
        pixels: vec![[0.3, 0.6, 0.9, 0.0]; (size.0 * size.1) as usize],
    };
    let transparent = vec![typed_layer("F0", &clear), typed_layer("F1", &clear)];
    assert_refused(
        &Stack::Typed(typed_document(size, transparent)),
        &request(&[0, 1], true),
        "nothing to blend",
    );
}

#[test]
fn growth_past_native_capacity_is_refused_before_any_work() {
    // 8 bits: the layer cap. A canvas-sized base and 1023 one-pixel layers.
    let canvas = (16u32, 16u32);
    let dot = RgbaImage::from_pixel(1, 1, Rgba([200, 100, 50, 255]));
    let mut doc = DrDocument::from_pixels(RgbaImage::from_pixel(
        canvas.0,
        canvas.1,
        Rgba([10, 20, 30, 255]),
    ));
    for k in 1..MAX_BYTE_LAYERS {
        doc = doc
            .adding_image_layer(k - 1, dot.clone(), "dot")
            .expect("a layer under the cap");
    }
    assert_eq!(doc.layers.len(), MAX_BYTE_LAYERS);
    let stack = Stack::Byte(doc);
    assert_refused(&stack, &request(&[0, 1], true), "native capacity");
    assert_refused(
        &stack,
        &request(&[0, 1], true),
        "the merged layer does not fit",
    );
    let (out, report) = stack
        .run(&request(&[0, 1], false))
        .expect("masks alone add no layer");
    assert_eq!(out.layer_count(), MAX_BYTE_LAYERS);
    assert_eq!(report["merged_layer"], Value::Null);
    assert!(out.has_mask(0) && out.has_mask(1));
    // More than 255 frames.
    let many: Vec<usize> = (0..256).collect();
    assert_refused(&stack, &request(&many, false), "at most 255 layers");

    // 16 bits: the record cap.
    let dot = Frame {
        dims: (1, 1),
        offset: (0, 0),
        pixels: vec![[0.8, 0.4, 0.2, 1.0]],
    };
    let layers = (0..MAX_RECORDS)
        .map(|k| typed_layer(&format!("dot {k}"), &dot))
        .collect();
    let stack = Stack::Typed(typed_document(canvas, layers));
    assert_eq!(stack.layer_count(), MAX_RECORDS);
    assert_refused(&stack, &request(&[0, 1], true), "native capacity");
    assert_refused(
        &stack,
        &request(&[0, 1], true),
        "the merged layer does not fit",
    );
}

// --------------------------------------------------------- (i): cancellation --

#[test]
fn a_cancelled_control_answers_operation_cancelled() {
    let (size, _, frames) = strip_frames(2);
    for depth in [Depth::Byte, Depth::Sixteen] {
        let stack = Stack::build(depth, size, &frames);
        let control = unsafe { dr_job_control_create() };
        unsafe { dr_job_control_cancel(control) };
        let refused = stack.run_with(&request(&[0, 1], true), LIMIT, control);
        unsafe { dr_job_control_free(control) };
        assert_eq!(refused.err().as_deref(), Some("operation cancelled"));
        assert!(!stack.has_mask(0) && !stack.has_mask(1));
    }
}

// ------------------------------------------------------------- (j): the plan --

#[test]
fn the_plan_is_the_edits_admission() {
    let size = (64u32, 64u32);
    let sharp = scene(size.0, size.1);
    let blurred = box_blur(&sharp, size.0, size.1);
    let frames: Vec<Frame> = (0..8u32)
        .map(|k| frame_of(size, &sharp, &blurred, |x, _| x / 8 == k))
        .collect();
    let pixels = u64::from(size.0 * size.1);
    for (depth, mask_bytes) in [(Depth::Byte, 1u64), (Depth::Sixteen, 2)] {
        let stack = Stack::build(depth, size, &frames);
        let two = stack.plan(&request(&[0, 1], true)).unwrap();
        let eight = stack
            .plan(&request(&[0, 1, 2, 3, 4, 5, 6, 7], true))
            .unwrap();
        let field = |plan: &Value, name: &str| plan[name].as_u64().expect("a plan field");
        // The kernel's working set does not grow with the frame count: six
        // more frames cost six more mask planes, each charged at 5/4, and
        // nothing else.
        let plane = pixels * mask_bytes;
        assert_eq!(
            field(&eight, "scratch_bytes") - field(&two, "scratch_bytes"),
            6 * (plane + plane.div_ceil(4)),
            "{depth:?}"
        );
        assert_eq!(
            field(&eight, "retained_bytes"),
            field(&two, "retained_bytes")
        );
        assert!(field(&eight, "work_units") > field(&two, "work_units"));
        assert_eq!(eight["merged_index"], json!(8));
        assert_eq!(two["merged_index"], json!(2));
        assert_eq!(
            stack.plan(&request(&[0, 1], false)).unwrap()["merged_index"],
            Value::Null
        );

        let needed = field(&two, "retained_bytes") + field(&two, "scratch_bytes");
        let refused = stack
            .run_with(&request(&[0, 1], true), needed - 1, ptr::null())
            .err()
            .expect("a limit under the plan refuses");
        assert!(refused.contains("working limit"), "{refused}");
        assert!(!stack.has_mask(0));
        stack
            .run_with(&request(&[0, 1], true), needed, ptr::null())
            .unwrap_or_else(|e| panic!("{depth:?}: the plan itself is admitted: {e}"));
    }
}
