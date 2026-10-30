//! The three float filters that MOVE samples,
//! through `dr_precision_doc_selected_edit`: Offset (an exact move), and Lens
//! Correction and Liquify (Catmull-Rom resamples, whose negative lobes are
//! bounded by the layer's visible range instead of 0…1). The helpers, the
//! pictures and the shared cases are `float_filters.rs`'s.
//!
//! The oracles are written here from the documented formulas: the index map
//! of each Offset edge mode, a linear ramp under the Lens Correction map (a
//! Catmull-Rom resample reproduces a linear function exactly, so an interior
//! pixel is the ramp at its source point), the vignette's gain, and a 4×4
//! Catmull-Rom sum for the fractional Liquify push.
use serde_json::{json, Value};

use crate::float_filters::{bits, document, filter, filtered, hdr_picture, hdr_plate, plan};
use crate::float_filters::{Picture, FLOOR};

// ---- Offset ---------------------------------------------------------------

/// The source index of `(x, y)` under an offset inside `frame` (`x, y, width,
/// height`), or `None` for an undefined area that is not wrapped or repeated.
fn offset_source(
    picture: &Picture,
    (x, y): (i64, i64),
    (horizontal, vertical): (i64, i64),
    mode: &str,
    frame: (i64, i64, i64, i64),
) -> Option<usize> {
    let (fx, fy, fw, fh) = frame;
    let index = |x: i64, y: i64| (y * i64::from(picture.w) + x) as usize;
    if x < fx || y < fy || x >= fx + fw || y >= fy + fh {
        return Some(index(x, y));
    }
    let (sx, sy) = (x - horizontal, y - vertical);
    let inside = sx >= fx && sy >= fy && sx < fx + fw && sy < fy + fh;
    match mode {
        _ if inside => Some(index(sx, sy)),
        "wrap" => Some(index(
            fx + (sx - fx).rem_euclid(fw),
            fy + (sy - fy).rem_euclid(fh),
        )),
        "repeat" => Some(index(sx.clamp(fx, fx + fw - 1), sy.clamp(fy, fy + fh - 1))),
        _ => None,
    }
}

#[test]
fn offset_moves_every_sample_bit_for_bit_in_each_edge_mode() {
    let (doc, picture) = document(&hdr_picture(true));
    let plane = (0, 0, i64::from(picture.w), i64::from(picture.h));
    for mode in ["wrap", "repeat", "transparent"] {
        for by in [(7_i64, -5_i64), (-3, 0), (1000, -999)] {
            let params = json!({"horizontal": by.0, "vertical": by.1, "undefined_areas": mode});
            let out = filtered(&doc, "offset", &params);
            for (i, got) in out.iter().enumerate() {
                let at = (i as i64 % plane.2, i as i64 / plane.2);
                let want = offset_source(&picture, at, by, mode, plane)
                    .map_or([0.0; 4], |source| picture.pixels[source]);
                assert_eq!(
                    got.map(f32::to_bits),
                    want.map(f32::to_bits),
                    "{params} pixel {i}"
                );
            }
        }
    }
    // The values above white, below zero and in the thousands are all still
    // there after a wrap: it is a permutation.
    let wrapped = filtered(&doc, "offset", &json!({"horizontal": 9, "vertical": 4}));
    let sorted = |pixels: &[[f32; 4]]| {
        let mut all = bits(pixels);
        all.sort_unstable();
        all
    };
    assert_eq!(sorted(&wrapped), sorted(&picture.pixels));
}

#[test]
fn offset_wraps_inside_the_selections_bounding_box() {
    let (doc, picture) = document(&hdr_picture(true));
    let (w, h) = (i64::from(picture.w), i64::from(picture.h));
    let frame = (8_i64, 6_i64, 22_i64, 19_i64);
    let inside = |x: i64, y: i64| {
        x >= frame.0 && y >= frame.1 && x < frame.0 + frame.2 && y < frame.1 + frame.3
    };
    let coverage: Vec<u16> = (0..w * h)
        .map(|i| if inside(i % w, i / w) { u16::MAX } else { 0 })
        .collect();
    let by = (5_i64, -30_i64);
    let params = json!({"horizontal": by.0, "vertical": by.1});
    let out = filter(&doc, "offset", &params, &coverage).unwrap().unwrap();
    let out = out.layer_source(0).unwrap().to_rgba32().unwrap();
    for (i, got) in out.iter().enumerate() {
        let at = (i as i64 % w, i as i64 / w);
        let source = offset_source(&picture, at, by, "wrap", frame).unwrap();
        assert_eq!(
            got.map(f32::to_bits),
            picture.pixels[source].map(f32::to_bits),
            "pixel {i}"
        );
        if !inside(at.0, at.1) {
            assert_eq!(source, i);
        }
    }
}

// ---- Catmull-Rom ------------------------------------------------------------

/// Catmull-Rom at distance `t` from a tap.
fn catmull_rom(t: f64) -> f64 {
    let x = t.abs();
    if x < 1.0 {
        1.5 * x * x * x - 2.5 * x * x + 1.0
    } else if x < 2.0 {
        -0.5 * x * x * x + 2.5 * x * x - 4.0 * x + 2.0
    } else {
        0.0
    }
}

/// Per-channel `(min, max)` over the pixels with alpha above 0: the range a
/// float resample is clamped to.
fn visible_range(picture: &Picture) -> [(f64, f64); 3] {
    std::array::from_fn(|c| {
        let values = picture.pixels.iter().filter(|p| p[3] > 0.0).map(|p| p[c]);
        (
            f64::from(values.clone().fold(f32::INFINITY, f32::min)),
            f64::from(values.fold(f32::NEG_INFINITY, f32::max)),
        )
    })
}

/// The straight sample of `picture` at the continuous point `(qx, qy)` (pixel
/// centres at half-integers): 4×4 Catmull-Rom taps over premultiplied
/// samples, taps outside the picture dropped. With `renormalize` the alpha is
/// divided by the weight of the taps that were inside (Liquify); without, it
/// is the plain sum (Lens Correction, whose edge fades out). Below 2^-24 of
/// alpha the sample is transparent black, and each colour is clamped to
/// `clamp`'s range when one is given.
fn bicubic(
    picture: &Picture,
    (qx, qy): (f64, f64),
    renormalize: bool,
    clamp: Option<&[(f64, f64); 3]>,
) -> [f64; 4] {
    let (sx, sy) = (qx - 0.5, qy - 0.5);
    let (x0, y0) = (sx.floor() as i64, sy.floor() as i64);
    let mut acc = [0.0_f64; 4];
    let mut total = 0.0;
    for ty in -1..=2_i64 {
        for tx in -1..=2_i64 {
            let (px, py) = (x0 + tx, y0 + ty);
            if px < 0 || py < 0 || px >= i64::from(picture.w) || py >= i64::from(picture.h) {
                continue;
            }
            let weight = catmull_rom(sx - px as f64) * catmull_rom(sy - py as f64);
            let p = picture.pixels[(py * i64::from(picture.w) + px) as usize].map(f64::from);
            for c in 0..3 {
                acc[c] += weight * p[c] * p[3];
            }
            acc[3] += weight * p[3];
            total += weight;
        }
    }
    let alpha = if renormalize { acc[3] / total } else { acc[3] };
    if total <= 0.0 || alpha < FLOOR {
        return [0.0; 4];
    }
    let colour = |c: usize| {
        let v = acc[c] / acc[3];
        clamp.map_or(v, |range| v.clamp(range[c].0, range[c].1))
    };
    [colour(0), colour(1), colour(2), alpha.clamp(0.0, 1.0)]
}

fn distance(x: u32, y: u32, cx: f64, cy: f64) -> f64 {
    (f64::from(x) + 0.5 - cx).hypot(f64::from(y) + 0.5 - cy)
}

// ---- Lens Correction --------------------------------------------------------

/// Three channels linear in the pixel coordinates, each running from below 0
/// to above 1 across a plate of about 100 pixels.
fn ramp_at(x: f64, y: f64) -> [f64; 3] {
    [-0.5 + 0.03 * x, 2.0 - 0.04 * y, -0.25 + 0.01 * (x + y)]
}

fn ramp(w: u32, h: u32) -> Picture {
    let pixels = (0..w * h)
        .map(|i| {
            let v = ramp_at(f64::from(i % w), f64::from(i / w));
            [v[0] as f32, v[1] as f32, v[2] as f32, 1.0]
        })
        .collect();
    Picture { w, h, pixels }
}

/// Lens Correction's parameters as the header states them, with their
/// defaults.
#[derive(Clone, Copy)]
struct Lens {
    distortion: f64,
    red_cyan: f64,
    blue_yellow: f64,
    vertical: f64,
    horizontal: f64,
    angle: f64,
    scale: f64,
}

impl Lens {
    fn of(params: &Value) -> Self {
        let get =
            |key: &str, default: f64| params.get(key).and_then(Value::as_f64).unwrap_or(default);
        Lens {
            distortion: get("distortion", 0.0),
            red_cyan: get("chromatic_red_cyan", 0.0),
            blue_yellow: get("chromatic_blue_yellow", 0.0),
            vertical: get("vertical_perspective", 0.0),
            horizontal: get("horizontal_perspective", 0.0),
            angle: get("angle", 0.0),
            scale: get("scale", 100.0),
        }
    }

    /// The source point (pixel centres at half-integers) of channel `c` for
    /// output pixel `(x, y)`, or `None` outside the corrected picture: the
    /// point about the centre over `R = max(W, H)/2`, divided by the scale,
    /// turned by the angle, divided by the perspective's `1 + 0.004·(h·x +
    /// v·y)`, then scaled by `(1 + k1·r²)·lateral` with `k1 = −0.0008 ·
    /// distortion` (outside past the fold `r² ≥ 1/(3·−k1)`).
    fn source(&self, w: u32, h: u32, x: u32, y: u32, c: usize) -> Option<(f64, f64)> {
        let (cx, cy) = (f64::from(w) / 2.0, f64::from(h) / 2.0);
        let r = f64::from(w.max(h)) / 2.0;
        let s = self.scale / 100.0;
        let (nx, ny) = (
            (f64::from(x) + 0.5 - cx) / r / s,
            (f64::from(y) + 0.5 - cy) / r / s,
        );
        let (sin, cos) = self.angle.to_radians().sin_cos();
        let (nx, ny) = (nx * cos - ny * sin, nx * sin + ny * cos);
        let d = 1.0 + 0.004 * (self.horizontal * nx + self.vertical * ny);
        if d <= 0.0 {
            return None;
        }
        let (nx, ny) = (nx / d, ny / d);
        let k1 = -0.0008 * self.distortion;
        let r2 = nx * nx + ny * ny;
        if k1 < 0.0 && r2 >= 1.0 / (3.0 * -k1) {
            return None;
        }
        let lateral = match c {
            0 => 1.0 + 0.00002 * self.red_cyan,
            2 => 1.0 + 0.00002 * self.blue_yellow,
            _ => 1.0,
        };
        let f = (1.0 + k1 * r2) * lateral;
        Some((cx + r * nx * f, cy + r * ny * f))
    }
}

/// Whether a source coordinate's 4×4 Catmull-Rom taps are all inside.
fn interior(s: f64, size: u32) -> bool {
    s >= 2.0 && s <= f64::from(size) - 3.0
}

/// The vignette's gain at a source point: `2^stops`, `stops = amount · 0.02 ·
/// fraction^falloff` (rounded to f32, as the filter holds it), `fraction` the
/// point's distance from the centre over the distance to the farthest pixel
/// centre, `falloff = 4^((midpoint − 50)/50)`.
fn vignette_gain(w: u32, h: u32, source: (f64, f64), amount: f64, midpoint: f64) -> f64 {
    let (cx, cy) = (f64::from(w) / 2.0, f64::from(h) / 2.0);
    let far = (cx - 0.5).hypot(cy - 0.5);
    let fraction = (source.0 - cx).hypot(source.1 - cy) / far;
    let falloff = 4.0_f64.powf((midpoint - 50.0) / 50.0);
    f64::from((amount * 0.02 * fraction.powf(falloff)) as f32).exp2()
}

#[test]
fn lens_correction_moves_a_ramp_that_leaves_zero_to_one_to_its_source_point() {
    let (w, h) = (96_u32, 80_u32);
    let (doc, picture) = document(&ramp(w, h));
    for params in [
        json!({"scale": 50}),
        json!({"scale": 150}),
        json!({"distortion": 100}),
        json!({"distortion": -100}),
        json!({"vertical_perspective": -100}),
        json!({"horizontal_perspective": 100}),
        json!({"angle": 30}),
        json!({"scale": 120, "distortion": 60, "vertical_perspective": -40,
            "horizontal_perspective": 30, "angle": 30}),
        json!({"chromatic_red_cyan": 100}),
        json!({"chromatic_red_cyan": -100, "chromatic_blue_yellow": 100}),
        json!({"scale": 110, "vignette_amount": 100}),
        json!({"distortion": 40, "vignette_amount": -100, "vignette_midpoint": 80}),
    ] {
        let lens = Lens::of(&params);
        let amount = params.get("vignette_amount").and_then(Value::as_f64);
        let midpoint = params
            .get("vignette_midpoint")
            .and_then(Value::as_f64)
            .unwrap_or(50.0);
        let out = filtered(&doc, "lens_correction", &params);
        let (mut checked, mut above, mut below) = (0, false, false);
        for y in 0..h {
            for x in 0..w {
                let sources: Vec<Option<(f64, f64)>> =
                    (0..3).map(|c| lens.source(w, h, x, y, c)).collect();
                if !sources
                    .iter()
                    .all(|s| s.is_some_and(|(sx, sy)| interior(sx, w) && interior(sy, h)))
                {
                    continue;
                }
                let got = out[(y * w + x) as usize];
                // The vignette is read at the green source point.
                let gain = amount.map_or(1.0, |a| {
                    vignette_gain(w, h, sources[1].unwrap(), a, midpoint)
                });
                for c in 0..3 {
                    let (sx, sy) = sources[c].unwrap();
                    let want = ramp_at(sx - 0.5, sy - 0.5)[c] * gain;
                    let value = f64::from(got[c]);
                    assert!(
                        (value - want).abs() <= 3e-6 * want.abs().max(1.0),
                        "{params} pixel {x},{y} channel {c}: {value} vs {want}"
                    );
                    above |= value > 1.0;
                    below |= value < 0.0;
                }
                assert_eq!(got[3], 1.0, "{params} pixel {x},{y}: alpha");
                checked += 1;
            }
        }
        assert!(checked > 500, "{params}: only {checked} interior pixels");
        assert!(above && below, "{params}: the range was not kept");
    }
    // Scale 50 shows the whole plate at half size: the corners are outside it.
    let small = filtered(&doc, "lens_correction", &json!({"scale": 50}));
    for corner in [0, w - 1, w * (h - 1), w * h - 1] {
        assert_eq!(small[corner as usize].map(f32::to_bits), [0; 4]);
    }
    assert_eq!(picture.pixels.len(), small.len());
}

#[test]
fn the_vignette_multiplies_linear_light_with_no_clamp() {
    // Odd sides, so one pixel centre is the plate's centre.
    let (w, h) = (41_u32, 31_u32);
    let colour = [0.18_f32, 1.5, -0.2];
    let flat = Picture {
        w,
        h,
        pixels: vec![[colour[0], colour[1], colour[2], 1.0]; (w * h) as usize],
    };
    let (doc, _) = document(&flat);
    for amount in [-100.0, 100.0] {
        for midpoint in [0.0, 50.0, 100.0] {
            let params = json!({"vignette_amount": amount, "vignette_midpoint": midpoint});
            let out = filtered(&doc, "lens_correction", &params);
            for (i, got) in out.iter().enumerate() {
                let (x, y) = (i as u32 % w, i as u32 / w);
                let source = (f64::from(x) + 0.5, f64::from(y) + 0.5);
                let gain = vignette_gain(w, h, source, amount, midpoint);
                for c in 0..3 {
                    let want = f64::from(colour[c]) * gain;
                    assert!(
                        (f64::from(got[c]) - want).abs() <= 1e-6 * want.abs(),
                        "{params} {x},{y} channel {c}: {} vs {want}",
                        got[c]
                    );
                }
                assert_eq!(got[3], 1.0);
            }
            let centre = ((h / 2) * w + w / 2) as usize;
            assert_eq!(
                out[centre].map(f32::to_bits),
                flat.pixels[centre].map(f32::to_bits)
            );
            // Two stops at the corner: 1.5 becomes 6 or 0.375, and the
            // negative component is scaled, not clipped.
            let corner = out[0];
            if amount > 0.0 {
                assert!(
                    (corner[1] - 6.0).abs() < 1e-4 && corner[2] < -0.79,
                    "{corner:?}"
                );
            } else {
                assert!(
                    (corner[1] - 0.375).abs() < 1e-5 && corner[2] > -0.051,
                    "{corner:?}"
                );
            }
        }
    }
}

#[test]
fn a_resample_beside_a_highlight_never_rings_outside_the_layers_range() {
    // A 5000 pixel on 0.1: Catmull-Rom's negative lobes would write about
    // −350 beside it. A float layer clamps each resampled colour to the range
    // its visible pixels hold.
    let (w, h) = (64_u32, 48_u32);
    let mut pixels = vec![[0.1_f32, 0.1, 0.1, 1.0]; (w * h) as usize];
    pixels[(w * 21 + 37) as usize] = [5000.0, 5000.0, 5000.0, 1.0];
    let (doc, picture) = document(&Picture { w, h, pixels });
    let range = visible_range(&picture);
    let params = json!({"scale": 137, "angle": 11});
    let lens = Lens::of(&params);
    let out = filtered(&doc, "lens_correction", &params);
    let (mut clamped, mut bright) = (0, 0);
    for y in 0..h {
        for x in 0..w {
            let (sx, sy) = lens.source(w, h, x, y, 1).unwrap();
            if !(interior(sx, w) && interior(sy, h)) {
                continue;
            }
            let got = out[(y * w + x) as usize];
            let free = bicubic(&picture, (sx, sy), false, None);
            let want = bicubic(&picture, (sx, sy), false, Some(&range));
            for c in 0..3 {
                assert!(got[c] >= 0.1 && got[c] <= 5000.0, "{x},{y}: {got:?}");
                assert!(
                    (f64::from(got[c]) - want[c]).abs() <= 1e-5 * want[c],
                    "{x},{y} channel {c}: {} vs {}",
                    got[c],
                    want[c]
                );
            }
            assert_eq!(got[3], 1.0, "{x},{y}");
            if free[0] < -1.0 {
                assert_eq!(got[0], 0.1, "{x},{y}: clamped to the darkest visible value");
                clamped += 1;
            }
            bright += usize::from(got[0] > 100.0);
        }
    }
    assert!(
        clamped >= 4 && bright >= 2,
        "{clamped} clamped, {bright} bright"
    );
}

// ---- Liquify ----------------------------------------------------------------

fn stroke(tool: &str, size: f64, strength: f64, points: Value) -> Value {
    json!({"tool": tool, "size": size, "strength": strength, "density": 1, "points": points})
}

/// A full-strength push of a 40 px brush from (40, 48) by `(dx, 0)`.
fn push(dx: f64) -> Value {
    json!({"strokes": [stroke("forward_warp", 40.0, 1.0, json!([[40, 48], [40.0 + dx, 48]]))]})
}

/// The 96×96 HDR plate, its pixel in the thousands inside the brush's core.
fn liquify_plate() -> Picture {
    hdr_plate(96, 96, Some((41, 47)))
}

#[test]
fn a_whole_pixel_push_moves_the_core_and_leaves_the_rest_bit_for_bit() {
    let (doc, picture) = document(&liquify_plate());
    let w = picture.w;
    let out = filtered(&doc, "liquify", &push(5.0));
    let (mut moved, mut kept) = (0, 0);
    for (i, got) in out.iter().enumerate() {
        let (x, y) = (i as u32 % w, i as u32 / w);
        let d = distance(x, y, 45.0, 48.0);
        if d <= 18.0 {
            // The displacement is a whole number of pixels, so the resample is
            // the one tap it lands on.
            let want = picture.pixels[i - 5];
            assert_eq!(got[3].to_bits(), want[3].to_bits(), "{x},{y}: alpha");
            for c in 0..3 {
                assert!(
                    (got[c] - want[c]).abs() <= 2e-6 * want[c].abs().max(1.0),
                    "{x},{y} channel {c}: {} vs {}",
                    got[c],
                    want[c]
                );
            }
            moved += 1;
        } else if d >= 21.0 {
            assert_eq!(
                got.map(f32::to_bits),
                picture.pixels[i].map(f32::to_bits),
                "{x},{y}"
            );
            kept += 1;
        }
    }
    assert!(moved > 900 && kept > 7000, "{moved} {kept}");
    // The pixel in the thousands went with the core.
    assert_eq!(out[(47 * w + 46) as usize][1], 4500.0);
}

#[test]
fn a_fractional_push_is_the_catmull_rom_resample_within_the_layers_range() {
    let (doc, picture) = document(&liquify_plate());
    let w = picture.w;
    let range = visible_range(&picture);
    let out = filtered(&doc, "liquify", &push(2.5));
    let (mut checked, mut clamped) = (0, 0);
    for (i, got) in out.iter().enumerate() {
        let (x, y) = (i as u32 % w, i as u32 / w);
        if distance(x, y, 42.5, 48.0) > 18.0 {
            continue;
        }
        let at = (f64::from(x) + 0.5 - 2.5, f64::from(y) + 0.5);
        let want = bicubic(&picture, at, true, Some(&range));
        let free = bicubic(&picture, at, true, None);
        if want[3] == 0.0 {
            assert_eq!(got.map(f32::to_bits), [0; 4], "{x},{y}");
            continue;
        }
        assert!(
            (f64::from(got[3]) - want[3]).abs() <= 1e-6 * want[3],
            "{x},{y}: alpha {} vs {}",
            got[3],
            want[3]
        );
        for c in 0..3 {
            // The premultiplied sums cancel near the highlight, so the
            // tolerance follows the colours the taps hold, not the result.
            let slack = 1e-6 * (want[c].abs() + range[c].1.abs() * 1e-3).max(1.0) / want[3];
            assert!(
                (f64::from(got[c]) - want[c]).abs() <= slack,
                "{x},{y} channel {c}: {} vs {}",
                got[c],
                want[c]
            );
            clamped += usize::from(free[c] != want[c]);
        }
        checked += 1;
    }
    assert!(checked > 900, "{checked}");
    // Beside the highlight the unclamped resample left the layer's range.
    assert!(clamped >= 4, "{clamped} components clamped");
}

#[test]
fn liquify_is_confined_by_coverage_and_a_still_field_is_no_change() {
    let (doc, picture) = document(&liquify_plate());
    let w = picture.w;
    let inside =
        |i: usize| (30..60).contains(&(i as u32 % w)) && (40..70).contains(&(i as u32 / w));
    let coverage: Vec<u16> = (0..picture.pixels.len())
        .map(|i| if inside(i) { u16::MAX } else { 0 })
        .collect();
    let whole = filtered(&doc, "liquify", &push(5.0));
    let confined = filter(&doc, "liquify", &push(5.0), &coverage)
        .unwrap()
        .unwrap();
    let confined = confined.layer_source(0).unwrap().to_rgba32().unwrap();
    for (i, got) in confined.iter().enumerate() {
        let want = if inside(i) {
            whole[i]
        } else {
            picture.pixels[i]
        };
        assert_eq!(got.map(f32::to_bits), want.map(f32::to_bits), "pixel {i}");
    }
    // Strength 0 moves nothing: no change, no error, and a plan all the same.
    let still =
        json!({"strokes": [stroke("forward_warp", 30.0, 0.0, json!([[10, 10], [30, 20]]))]});
    assert!(filter(&doc, "liquify", &still, &[]).unwrap().is_none());
    let report = plan(&doc, "liquify", &still).unwrap();
    assert_eq!(report["output_bytes"], picture.pixels.len() as u64 * 16);
}
