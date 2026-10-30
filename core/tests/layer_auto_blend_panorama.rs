//! Edit ▸ Auto-Blend Layers ▸ Panorama through the FFI
//! (`dr_doc_auto_blend`, `dr_precision_doc_auto_blend` and their plans, with
//! `"method": "panorama"`), at 8 and 16 bits and in a 32-bit float document.
//!
//! # The scenes
//!
//! Everything is written here. A scene is a DISPLAY value `u` in 0…1 per
//! channel: two octaves of bilinear value noise from an integer hash in
//! 0.2…0.8, a flat vertical ramp, or a checkered disc. A depth stores `u` as
//! `round(u · 255)`, `round(u · 65535)`, or (float) the linear light whose
//! sRGB code is `u`, so a float document's retouch code is proportional to
//! `u` and every gain the core solves is a gain on `u` at every depth. The
//! sRGB curve here is the IEC one in f64, sharing no line with the core.
//! Two frames over a 260 × 96 canvas: frame 0 the columns 0..160, frame 1
//! the columns 80..240 (an 80-column overlap, 80..160), columns 240..260
//! uncovered.
//!
//! # The bounds, declared before the first run
//!
//! * `SEAM_SLACK` = 1 column: where the seam of two identical frames may sit
//!   from the overlap's far edge (column 160, frame 0's own edge: take2's
//!   W is Lower, the frame below keeps a flat overlap), in every row.
//! * `RATIO` = 0.01: `|g₁ · 0.8 / g₀ − 1|` per channel when frame 1 is
//!   `round(0.8 · frame 0)`.
//! * `MEAN_CODES` = 2 and `MAX_CODES` = 6 (× 257 at 16 bits; at float a
//!   relative 1% for both): the shown composite over the overlap against
//!   `g₀ · frame 0`.
//! * `FLOAT_GAINED` = 1e-4 display units: a float frame's un-won sample
//!   against `g · u` (the core's sRGB curve is f32, this one f64); 8 and 16
//!   bits are exact.
#![allow(clippy::needless_range_loop)]

use std::ptr;
use std::sync::Arc;

use doctored_core::doc_mask::MaskProperties;
use doctored_core::ffi_jobs::{dr_job_control_cancel, dr_job_control_create, dr_job_control_free};
use doctored_core::precision_mask::{PrecisionMask, PrecisionPlane};
use image::{GrayImage, Luma};
use serde_json::{json, Value};

use crate::auto_blend_fixture::{
    assert_partition, assert_refused, byte_document, typed_document, typed_layer, Depth, Frame,
    Stack, LIMIT,
};

const SEAM_SLACK: i64 = 1;
const RATIO: f64 = 0.01;
const MEAN_CODES: f64 = 2.0;
const MAX_CODES: f64 = 6.0;
const FLOAT_GAINED: f64 = 1e-4;

const CANVAS: (u32, u32) = (260, 96);
const FRAME: (u32, u32) = (160, 96);
const SECOND_X: i32 = 80;
const FAR_EDGE: i64 = 160;
/// The planted disc's centre column, beside the far edge so the seam
/// Lower prefers would cross it.
const DISC_X: f64 = 150.0;
const DEPTHS: [Depth; 3] = [Depth::Byte, Depth::Sixteen, Depth::Float];

// ---------------------------------------------------------------- the scenes --

fn hash(i: u32) -> u32 {
    let mut x = i.wrapping_mul(0x9E37_79B9) ^ 0x2C1B_3C6D;
    x ^= x >> 15;
    x = x.wrapping_mul(0x85EB_CA6B);
    x ^ (x >> 13)
}

/// One octave of bilinear value noise in 0…1 on a `cell` lattice.
fn octave(x: u32, y: u32, cell: u32, seed: u32) -> f64 {
    let lattice =
        |i: u32, j: u32| f64::from(hash(i * 7919 + j * 104_729 + seed) & 0xffff) / 65535.0;
    let (i, j) = (x / cell, y / cell);
    let (fx, fy) = (
        f64::from(x % cell) / f64::from(cell),
        f64::from(y % cell) / f64::from(cell),
    );
    let top = lattice(i, j) * (1.0 - fx) + lattice(i + 1, j) * fx;
    let bottom = lattice(i, j + 1) * (1.0 - fx) + lattice(i + 1, j + 1) * fx;
    top * (1.0 - fy) + bottom * fy
}

/// The textured scene: display values in 0.2…0.8.
fn texture(x: u32, y: u32) -> [f64; 3] {
    [0, 1, 2].map(|c| {
        let v = 0.6 * octave(x, y, 12, 31 * c + 5) + 0.4 * octave(x, y, 4, 31 * c + 11);
        0.2 + 0.6 * v
    })
}

/// A vertical ramp, constant along x.
fn ramp(_x: u32, y: u32) -> [f64; 3] {
    let v = 0.25 + 0.5 * f64::from(y) / f64::from(CANVAS.1 - 1);
    [v, v * 0.9, v * 0.8]
}

/// The disc of radius 20 across the overlap's far edge.
fn in_disc(x: u32, y: u32) -> bool {
    let (dx, dy) = (f64::from(x) - DISC_X, f64::from(y) - 48.0);
    dx * dx + dy * dy <= 400.0
}

/// A 4-pixel black/white checker inside the disc, over a flat ground.
fn checker_disc(x: u32, y: u32) -> [f64; 3] {
    if in_disc(x, y) {
        let v = if (x / 4 + y / 4).is_multiple_of(2) {
            0.05
        } else {
            0.95
        };
        [v; 3]
    } else {
        [0.45, 0.5, 0.55]
    }
}

fn flat_ground(_x: u32, _y: u32) -> [f64; 3] {
    [0.45, 0.5, 0.55]
}

fn srgb_to_linear(u: f64) -> f64 {
    if u <= 0.04045 {
        u / 12.92
    } else {
        ((u + 0.055) / 1.055).powf(2.4)
    }
}

fn linear_to_srgb(v: f64) -> f64 {
    if v <= 0.003_130_8 {
        v * 12.92
    } else {
        1.055 * v.powf(1.0 / 2.4) - 0.055
    }
}

/// What `depth` stores for display value `u`, as `Frame` carries it (a unit
/// code, or the float sample).
fn stored(depth: Depth, u: f64) -> f64 {
    match depth {
        Depth::Byte => (u * 255.0).round() / 255.0,
        Depth::Sixteen => (u * 65535.0).round() / 65535.0,
        Depth::Float => f64::from(srgb_to_linear(u) as f32),
    }
}

/// A stored sample as its display value.
fn display(depth: Depth, v: f64) -> f64 {
    match depth {
        Depth::Float => linear_to_srgb(v),
        _ => v,
    }
}

/// One code of `depth`, in display units.
fn code(depth: Depth) -> f64 {
    match depth {
        Depth::Byte => 1.0 / 255.0,
        _ => 257.0 / 65535.0,
    }
}

/// The opaque frame of `scene` at canvas column `x0`, `FRAME` in size, with
/// every display value passed through `adjust` before it is stored.
fn frame(
    depth: Depth,
    x0: i32,
    scene: fn(u32, u32) -> [f64; 3],
    adjust: &dyn Fn(Depth, f64) -> f64,
) -> Frame {
    let pixels = (0..FRAME.0 * FRAME.1)
        .map(|i| {
            let (x, y) = (i % FRAME.0, i / FRAME.0);
            let u = scene((x as i32 + x0) as u32, y);
            let [r, g, b] = u.map(|v| adjust(depth, v));
            [r, g, b, 1.0]
        })
        .collect();
    Frame {
        dims: FRAME,
        offset: (x0, 0),
        pixels,
    }
}

fn plain(depth: Depth, v: f64) -> f64 {
    stored(depth, v)
}

/// Frame 1 of the exposure case: `round(0.8 · frame 0's code)` at 8 and 16
/// bits, the linear light of `0.8 · u` at float.
fn darker(depth: Depth, u: f64) -> f64 {
    match depth {
        Depth::Byte => ((u * 255.0).round() * 0.8).round() / 255.0,
        Depth::Sixteen => ((u * 65535.0).round() * 0.8).round() / 65535.0,
        Depth::Float => stored(depth, 0.8 * u),
    }
}

fn pair(depth: Depth, scenes: [fn(u32, u32) -> [f64; 3]; 2]) -> Stack {
    let frames = [
        frame(depth, 0, scenes[0], &plain),
        frame(depth, SECOND_X, scenes[1], &plain),
    ];
    Stack::build(depth, CANVAS, &frames)
}

fn panorama(layers: &[usize], seamless: bool) -> Value {
    json!({"layers": layers, "seamless": seamless, "method": "panorama"})
}

// ---------------------------------------------------------------- reading --

/// A raster as stored units, with its size and offset.
type Raster = ((u32, u32), (i32, i32), Vec<[f64; 4]>);

/// Layer `layer`'s raster.
fn raster(stack: &Stack, layer: usize) -> Raster {
    match stack {
        Stack::Byte(doc) => {
            let entry = &doc.layers[layer];
            let image = entry.pixels.contiguous_image();
            let pixels = image
                .pixels()
                .map(|p| p.0.map(|v| f64::from(v) / 255.0))
                .collect();
            (image.dimensions(), entry.offset, pixels)
        }
        Stack::Typed(doc) => {
            let entry = doc.layer(layer).unwrap();
            let image = entry.source().expect("a raster record");
            let pixels = if doc.is_float() {
                image
                    .to_rgba32()
                    .unwrap()
                    .iter()
                    .map(|p| p.map(f64::from))
                    .collect()
            } else {
                image
                    .to_rgba16()
                    .unwrap()
                    .iter()
                    .map(|p| p.map(|v| f64::from(v) / 65535.0))
                    .collect()
            };
            (image.dimensions(), entry.offset(), pixels)
        }
    }
}

/// Layer `layer`'s sample at canvas pixel `(x, y)`, when it has one.
fn sample_at(raster: &Raster, x: u32, y: u32) -> Option<[f64; 4]> {
    let ((w, h), (ox, oy), pixels) = raster;
    let (fx, fy) = (i64::from(x) - i64::from(*ox), i64::from(y) - i64::from(*oy));
    ((0..i64::from(*w)).contains(&fx) && (0..i64::from(*h)).contains(&fy))
        .then(|| pixels[(fy * i64::from(*w) + fx) as usize])
}

/// The label of each canvas pixel (the frame whose mask reveals it), after
/// checking the masks partition the covered set and a label's frame has
/// alpha there.
fn labels(out: &Stack, frames: usize) -> Vec<Option<usize>> {
    let masks: Vec<Vec<bool>> = (0..frames).map(|k| out.mask(k)).collect();
    let rasters: Vec<_> = (0..frames).map(|k| raster(out, k)).collect();
    let covered = |i: usize| {
        let (x, y) = (i as u32 % CANVAS.0, i as u32 / CANVAS.0);
        rasters
            .iter()
            .any(|r| sample_at(r, x, y).is_some_and(|p| p[3] > 0.0))
    };
    assert_partition(&masks, covered);
    (0..masks[0].len())
        .map(|i| {
            let k = (0..frames).find(|&k| masks[k][i])?;
            let (x, y) = (i as u32 % CANVAS.0, i as u32 / CANVAS.0);
            assert!(
                sample_at(&rasters[k], x, y).is_some_and(|p| p[3] > 0.0),
                "label {k} at ({x}, {y}) has no alpha"
            );
            Some(k)
        })
        .collect()
}

fn gains(report: &Value, frames: usize) -> Vec<[f64; 3]> {
    (0..frames)
        .map(|k| {
            let gain = &report["layers"][k]["gain"];
            [0, 1, 2].map(|c| gain[c].as_f64().expect("a gain"))
        })
        .collect()
}

// ------------------------------------------------------------ the partition --

#[test]
fn masks_partition_the_covered_canvas_at_every_depth() {
    for depth in DEPTHS {
        let stack = pair(depth, [texture, texture]);
        for seamless in [false, true] {
            let (out, report) = stack
                .run(&panorama(&[0, 1], seamless))
                .unwrap_or_else(|e| panic!("{depth:?}: {e}"));
            assert_eq!(out.layer_count(), 2, "{depth:?}: no layer is added");
            assert_eq!(report["method"], json!("panorama"));
            assert_eq!(report["merged_layer"], Value::Null);
            let covered = u64::from(240 * CANVAS.1);
            assert_eq!(report["covered_pixels"], json!(covered));
            assert_eq!(report["canvas_pixels"], json!(CANVAS.0 * CANVAS.1));
            let labelled = labels(&out, 2);
            for k in 0..2 {
                let won = labelled.iter().filter(|&&l| l == Some(k)).count();
                assert_eq!(report["layers"][k]["won_pixels"], json!(won), "{depth:?}");
                assert_eq!(
                    report["layers"][k].get("gain").is_some(),
                    seamless,
                    "{depth:?}: a gain only with Seamless Tones"
                );
            }
        }
    }
}

// ------------------------------------------------------------- the seam --

#[test]
fn identical_frames_meet_at_the_overlaps_far_edge() {
    for depth in DEPTHS {
        let (out, _) = pair(depth, [ramp, ramp])
            .run(&panorama(&[0, 1], false))
            .unwrap();
        let labelled = labels(&out, 2);
        for y in 0..CANVAS.1 {
            let row = &labelled[(y * CANVAS.0) as usize..((y + 1) * CANVAS.0) as usize];
            let seam = row
                .iter()
                .position(|&l| l == Some(1))
                .expect("frame 1 shows") as i64;
            assert!(
                (seam - FAR_EDGE).abs() <= SEAM_SLACK,
                "{depth:?}: row {y} meets at column {seam}"
            );
            assert!(row[..seam as usize].iter().all(|&l| l == Some(0)));
            assert!(row[seam as usize..240].iter().all(|&l| l == Some(1)));
        }
    }
}

/// No pixel inside the disc grown by one (its 4-neighbours, the seam's own
/// adjacency) has a 4-neighbour of another label.
fn assert_seam_avoids_disc(labelled: &[Option<usize>], case: &str) {
    let (w, h) = (CANVAS.0 as i64, CANVAS.1 as i64);
    let near = |x: i64, y: i64| {
        [(0i64, 0i64), (1, 0), (-1, 0), (0, 1), (0, -1)]
            .iter()
            .any(|&(dx, dy)| {
                let (nx, ny) = (x + dx, y + dy);
                (0..w).contains(&nx) && (0..h).contains(&ny) && in_disc(nx as u32, ny as u32)
            })
    };
    for y in 0..h {
        for x in 0..w {
            if !near(x, y) {
                continue;
            }
            let here = labelled[(y * w + x) as usize];
            for (dx, dy) in [(1i64, 0i64), (-1, 0), (0, 1), (0, -1)] {
                let (nx, ny) = (x + dx, y + dy);
                if (0..w).contains(&nx) && (0..h).contains(&ny) {
                    assert_eq!(
                        labelled[(ny * w + nx) as usize],
                        here,
                        "{case}: the seam crosses the disc at ({x}, {y})"
                    );
                }
            }
        }
    }
}

#[test]
fn the_seam_goes_around_a_planted_object() {
    for depth in DEPTHS {
        // In both frames: the frames agree there, so only the gradient
        // keeps the seam out.
        let (out, _) = pair(depth, [checker_disc, checker_disc])
            .run(&panorama(&[0, 1], false))
            .unwrap();
        assert_seam_avoids_disc(&labels(&out, 2), &format!("{depth:?} both"));
        // In the upper frame only: the frames disagree there.
        let (out, _) = pair(depth, [flat_ground, checker_disc])
            .run(&panorama(&[0, 1], false))
            .unwrap();
        assert_seam_avoids_disc(&labels(&out, 2), &format!("{depth:?} upper"));
    }
}

// --------------------------------------------- Seamless Tones and Colors --

#[test]
fn seamless_tones_match_an_exposure_shifted_crop() {
    for depth in DEPTHS {
        let frames = [
            frame(depth, 0, texture, &plain),
            frame(depth, SECOND_X, texture, &darker),
        ];
        let stack = Stack::build(depth, CANVAS, &frames);
        let (out, report) = stack
            .run(&panorama(&[0, 1], true))
            .unwrap_or_else(|e| panic!("{depth:?}: {e}"));
        let g = gains(&report, 2);
        for c in 0..3 {
            let ratio = g[1][c] * 0.8 / g[0][c];
            assert!(
                (ratio - 1.0).abs() <= RATIO,
                "{depth:?}: channel {c} gains {:?} give ratio {ratio}",
                g
            );
        }
        let labelled = labels(&out, 2);
        let rasters = [raster(&out, 0), raster(&out, 1)];
        let (mut sum, mut worst, mut count) = (0.0f64, 0.0f64, 0usize);
        for y in 0..CANVAS.1 {
            for x in 80..160u32 {
                let k = labelled[(y * CANVAS.0 + x) as usize].expect("covered");
                let shown = sample_at(&rasters[k], x, y).unwrap();
                let original = texture(x, y);
                for c in 0..3 {
                    let reference = g[0][c] * display(depth, stored(depth, original[c]));
                    let off = (display(depth, shown[c]) - reference).abs();
                    let off = if depth == Depth::Float {
                        off / reference
                    } else {
                        off / code(depth)
                    };
                    sum += off;
                    worst = worst.max(off);
                    count += 1;
                }
            }
        }
        let mean = sum / count as f64;
        let (mean_bound, max_bound) = if depth == Depth::Float {
            (0.01, 0.01)
        } else {
            (MEAN_CODES, MAX_CODES)
        };
        assert!(
            mean <= mean_bound && worst <= max_bound,
            "{depth:?}: the composite is {mean} on average and {worst} at worst from g0 · A"
        );
    }
}

#[test]
fn seamless_writes_the_frames_and_keeps_their_alpha() {
    for depth in DEPTHS {
        let frames = [
            frame(depth, 0, texture, &plain),
            frame(depth, SECOND_X, texture, &darker),
        ];
        let stack = Stack::build(depth, CANVAS, &frames);
        let (out, report) = stack.run(&panorama(&[0, 1], true)).unwrap();
        assert_eq!(out.layer_count(), 2);
        let g = gains(&report, 2);
        let labelled = labels(&out, 2);
        for k in 0..2 {
            let (dims, offset, before) = raster(&stack, k);
            let (after_dims, after_offset, after) = raster(&out, k);
            assert_eq!((after_dims, after_offset), (dims, offset), "{depth:?} {k}");
            for (i, (old, new)) in before.iter().zip(&after).enumerate() {
                assert_eq!(
                    old[3].to_bits(),
                    new[3].to_bits(),
                    "{depth:?} {k}: alpha at {i}"
                );
                let (fx, fy) = (i as i64 % i64::from(dims.0), i as i64 / i64::from(dims.0));
                let (cx, cy) = (fx + i64::from(offset.0), fy + i64::from(offset.1));
                let inside = (0..i64::from(CANVAS.0)).contains(&cx)
                    && (0..i64::from(CANVAS.1)).contains(&cy);
                if inside && labelled[(cy * i64::from(CANVAS.0) + cx) as usize] == Some(k) {
                    continue;
                }
                for c in 0..3 {
                    let gain = g[k][c] as f32;
                    match depth {
                        Depth::Byte => {
                            let code = (old[c] * 255.0).round() as u8;
                            let want =
                                ((f32::from(code) / 255.0 * gain).clamp(0.0, 1.0) * 255.0).round();
                            assert_eq!((new[c] * 255.0).round(), f64::from(want), "{k} {i} {c}");
                        }
                        Depth::Sixteen => {
                            let code = (old[c] * 65535.0).round() as u16;
                            let want = ((f32::from(code) / 65535.0 * gain).clamp(0.0, 1.0)
                                * 65535.0)
                                .round();
                            assert_eq!((new[c] * 65535.0).round(), f64::from(want), "{k} {i} {c}");
                        }
                        Depth::Float => {
                            let want = f64::from(gain) * linear_to_srgb(old[c]);
                            let got = linear_to_srgb(new[c]);
                            assert!(
                                (got - want).abs() <= FLOAT_GAINED,
                                "{k} {i} {c}: {got} against {want}"
                            );
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn without_seamless_no_pixel_changes() {
    for depth in DEPTHS {
        let stack = pair(depth, [texture, texture]);
        let (out, _) = stack.run(&panorama(&[0, 1], false)).unwrap();
        for k in 0..2 {
            assert!(out.same_raster(k, &stack, k), "{depth:?} {k}");
            assert!(out.has_mask(k));
        }
    }
}

// --------------------------------------------------------------- cancel --

#[test]
fn a_cancelled_panorama_answers_operation_cancelled() {
    for depth in DEPTHS {
        let stack = pair(depth, [texture, texture]);
        let control = unsafe { dr_job_control_create() };
        unsafe { dr_job_control_cancel(control) };
        let refused = stack.run_with(&panorama(&[0, 1], true), LIMIT, control);
        unsafe { dr_job_control_free(control) };
        assert_eq!(refused.err().as_deref(), Some("operation cancelled"));
        assert!(!stack.has_mask(0) && !stack.has_mask(1));
    }
}

// ------------------------------------------------------------- refusals --

#[test]
fn panorama_refusals_name_the_layer() {
    let byte = |locks: [u32; 2], masked: bool| {
        let frames = [
            frame(Depth::Byte, 0, texture, &plain),
            frame(Depth::Byte, SECOND_X, texture, &plain),
        ];
        let mut doc = byte_document(CANVAS, &frames);
        doc.layers[0].locks = locks[0];
        doc.layers[1].locks = locks[1];
        if masked {
            doc.layers[1].mask = Some(Arc::new(GrayImage::from_pixel(
                CANVAS.0,
                CANVAS.1,
                Luma([255]),
            )));
        }
        Stack::Byte(doc)
    };
    let stack = byte([0, 0], false);
    assert_refused(
        &stack,
        &json!({"layers": [0, 1], "seamless": true, "method": "mosaic"}),
        "invalid auto-blend request",
    );
    assert_refused(&stack, &panorama(&[0], true), "at least two layers");
    assert_refused(&stack, &panorama(&[0, 5], true), "layer 5 is out of range");
    assert_refused(
        &byte([0, 0], true),
        &panorama(&[0, 1], true),
        "“F1” already has a layer mask",
    );
    // A pixel lock refuses the write-back only.
    let locked = byte([2, 0], false);
    assert_refused(&locked, &panorama(&[0, 1], true), "“F0” is locked");
    locked
        .run(&panorama(&[0, 1], false))
        .expect("masks alone write no pixel");
    locked
        .run(&json!({"layers": [0, 1], "seamless": true}))
        .expect("Stack Images writes no frame pixel");
    assert_refused(
        &byte([7, 0], false),
        &panorama(&[0, 1], false),
        "“F0” is locked",
    );

    // 16 bits.
    let frames = [
        frame(Depth::Sixteen, 0, texture, &plain),
        frame(Depth::Sixteen, SECOND_X, texture, &plain),
    ];
    let layers = |locks: u32, masked: bool| {
        let mut out = vec![typed_layer("F0", &frames[0]), typed_layer("F1", &frames[1])];
        out[0] = out[0].clone().with_locks(locks);
        if masked {
            let plane = PrecisionPlane::from_u16(
                FRAME.0,
                FRAME.1,
                vec![65535; (FRAME.0 * FRAME.1) as usize],
                None,
            )
            .unwrap();
            let mask = PrecisionMask::new(plane, MaskProperties::at((0, 0)), true).unwrap();
            out[1] = out[1].clone().with_mask(Some(mask));
        }
        Stack::Typed(typed_document(CANVAS, out))
    };
    assert_refused(
        &layers(2, false),
        &panorama(&[0, 1], true),
        "“F0” is locked",
    );
    layers(2, false)
        .run(&panorama(&[0, 1], false))
        .expect("masks alone write no pixel");
    assert_refused(
        &layers(0, true),
        &panorama(&[0, 1], false),
        "“F1” already has a layer mask",
    );
}

// --------------------------------------------------------------- the plan --

#[test]
fn the_plan_counts_the_write_back_and_is_the_admission() {
    let field = |stack: &Stack, seamless: bool| {
        let plan = stack.plan(&panorama(&[0, 1], seamless)).unwrap();
        assert_eq!(plan["merged_index"], Value::Null);
        plan["scratch_bytes"].as_u64().expect("a plan field")
    };
    for (depth, bytes) in [(Depth::Byte, 4u64), (Depth::Sixteen, 8)] {
        let narrow = pair(depth, [texture, texture]);
        // The same canvas and coverage, with frame 0 ten columns wider off
        // the canvas's left edge: only the written-back raster grows.
        let mut wide_frame = frame(depth, -10, texture, &plain);
        wide_frame.dims = (FRAME.0 + 10, FRAME.1);
        wide_frame.pixels = (0..(FRAME.0 + 10) * FRAME.1)
            .map(|i| {
                let (x, y) = (i % (FRAME.0 + 10), i / (FRAME.0 + 10));
                let [r, g, b] = texture(x.saturating_sub(10), y).map(|v| stored(depth, v));
                [r, g, b, 1.0]
            })
            .collect();
        let wide = Stack::build(
            depth,
            CANVAS,
            &[wide_frame, frame(depth, SECOND_X, texture, &plain)],
        );
        assert_eq!(
            field(&wide, true) - field(&narrow, true),
            10 * u64::from(FRAME.1) * bytes,
            "{depth:?}"
        );
        assert_eq!(field(&wide, false), field(&narrow, false), "{depth:?}");
    }

    // A float document's records are canvas-sized (the fixture's), so no
    // raster reaches off its canvas. The kernel and mask terms depend only
    // on the canvas and the frame count, so against the 16-bit twin only the
    // write-back differs: two canvas-sized records at 16 bytes a pixel
    // against two frames at 8.
    let float = pair(Depth::Float, [texture, texture]);
    let sixteen = pair(Depth::Sixteen, [texture, texture]);
    let canvas = u64::from(CANVAS.0) * u64::from(CANVAS.1);
    let frame_pixels = u64::from(FRAME.0) * u64::from(FRAME.1);
    assert_eq!(
        field(&float, true) - field(&sixteen, true),
        2 * (canvas * 16 - frame_pixels * 8)
    );
    assert_eq!(field(&float, false), field(&sixteen, false));

    for depth in DEPTHS {
        let narrow = pair(depth, [texture, texture]);
        let plan = narrow.plan(&panorama(&[0, 1], true)).unwrap();
        let needed =
            plan["retained_bytes"].as_u64().unwrap() + plan["scratch_bytes"].as_u64().unwrap();
        let refused = narrow
            .run_with(&panorama(&[0, 1], true), needed - 1, ptr::null())
            .err()
            .expect("a limit under the plan refuses");
        assert!(refused.contains("working limit"), "{refused}");
        narrow
            .run_with(&panorama(&[0, 1], true), needed, ptr::null())
            .unwrap_or_else(|e| panic!("{depth:?}: the plan itself is admitted: {e}"));
    }
}
