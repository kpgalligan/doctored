//! Presence is resolution-independent (the Develop RAW stage):
//! `presence_radii` scales the radii with the long side, so Presence of a
//! picture at half size looks like the half-size view of Presence at full
//! size. Develop's preview shows the full-size result through the Fit divisor,
//! so this is what makes a slider read the same on every camera.
//!
//! Every edit runs at 16 bits through `selected_edit`. The stated scale is ½:
//! the REFERENCE is the 2×2 box mean of the full-size result and the PREVIEW
//! is Presence of the 2×2 box mean of the source. Bounds, declared before the
//! first run and never widened (a miss is fixed in `presence_radii` with
//! `tests/filter_presence.rs` still green):
//!
//! - STRIP(P). A 2048×16 opaque grey strip `Y = 0.5 + 0.02·sin(2π(x + ½)/P)`;
//!   its half-size view is the same waveform at period P/2. The least-squares
//!   amplitude of that waveform over the middle half of the columns, over the
//!   half-size source's, is the contrast ratio. The preview's ratio is within
//!   0.10 of the reference's for Texture +100 at P = 24 and for Clarity +100
//!   at P = 96 (a box-response prediction gives 1.38 vs 1.39 and 1.80 vs
//!   1.79).
//! - HAZED. A 512×512 scene of 16-pixel tiles, each one colour with one
//!   channel at 0, hazed by `I = J·t + A·(1 − t)` with `A = (0.90, 0.92,
//!   0.95)`: 96 sky rows at `t = 0`, then `t = 0.8` on the left half and `t =
//!   0.5` on the right. For Dehaze +100 the mean |preview − reference| over the
//!   colour channels of the non-sky preview pixels more than 8 pixels from a
//!   zone boundary is ≤ 0.02.
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use doctored_core::selected_adjustment::SelectedEdit;
use serde_json::{json, Value};

/// The 16-bit Presence of `pixels` (straight samples, 0…1 per channel, alpha
/// opaque), as 0…1 samples.
fn presence(w: u32, h: u32, pixels: &[[f64; 3]], params: &Value) -> Vec<[f64; 3]> {
    let codes: Vec<[u16; 4]> = pixels
        .iter()
        .map(|p| {
            let c = p.map(|v| (v.clamp(0.0, 1.0) * 65535.0).round() as u16);
            [c[0], c[1], c[2], 65535]
        })
        .collect();
    let doc = PrecisionDocument::from_image(
        PrecisionImage::from_rgba16(w, h, codes, IccProfile::srgb(), None).unwrap(),
        Metadata::default(),
        Resolution::default(),
    )
    .unwrap();
    let edit = SelectedEdit::parse(&json!({"layer":0,"op":"presence","params":params}).to_string())
        .unwrap_or_else(|e| panic!("{params}: {e}"));
    doc.selected_edit(&edit, &[], REFERENCE_WORKING_BYTES, 0, None)
        .unwrap_or_else(|e| panic!("{params}: {e}"))
        .unwrap_or_else(|| panic!("{params}: no edit"))
        .layer_source(0)
        .unwrap()
        .to_rgba16()
        .unwrap()
        .iter()
        .map(|p| [0, 1, 2].map(|c| f64::from(p[c]) / 65535.0))
        .collect()
}

/// The 2×2 box mean of a `w`×`h` picture (both even).
fn half(w: u32, h: u32, pixels: &[[f64; 3]]) -> Vec<[f64; 3]> {
    let (hw, hh) = (w / 2, h / 2);
    let at = |x: u32, y: u32| pixels[(y * w + x) as usize];
    (0..hw * hh)
        .map(|i| {
            let (x, y) = (2 * (i % hw), 2 * (i / hw));
            let (a, b, c, d) = (at(x, y), at(x + 1, y), at(x, y + 1), at(x + 1, y + 1));
            [0, 1, 2].map(|k| (a[k] + b[k] + c[k] + d[k]) / 4.0)
        })
        .collect()
}

// ---- STRIP ----------------------------------------------------------------------

const STRIP_W: u32 = 2048;
const STRIP_H: u32 = 16;

/// A unit sinusoid of period `period` at column `x`'s centre.
fn wave(x: u32, period: f64) -> f64 {
    (2.0 * std::f64::consts::PI * (f64::from(x) + 0.5) / period).sin()
}

/// The least-squares amplitude `A` of `c + A·wave` over the middle half of
/// `width` columns (the normal equations solved directly).
fn amplitude(width: u32, row: impl Fn(u32) -> f64, period: f64) -> f64 {
    let (mut n, mut s, mut ss, mut v, mut sv) = (0.0, 0.0, 0.0, 0.0, 0.0);
    for x in width / 4..width * 3 / 4 {
        let (basis, value) = (wave(x, period), row(x));
        n += 1.0;
        s += basis;
        ss += basis * basis;
        v += value;
        sv += basis * value;
    }
    (n * sv - s * v) / (n * ss - s * s)
}

/// `(preview, reference)` contrast ratios at the half-size period.
fn strip_ratios(period: f64, params: &Value) -> (f64, f64) {
    let source: Vec<[f64; 3]> = (0..STRIP_W * STRIP_H)
        .map(|i| {
            // The 16-bit source the full-size edit reads, so both paths start
            // from the same codes.
            let v = ((0.5 + 0.02 * wave(i % STRIP_W, period)) * 65535.0).round() / 65535.0;
            [v, v, v]
        })
        .collect();
    let (hw, hh) = (STRIP_W / 2, STRIP_H / 2);
    let small = half(STRIP_W, STRIP_H, &source);
    // The preview reads the half-size source as 16-bit codes, so its
    // denominator is the source it actually received.
    let small_codes: Vec<[f64; 3]> = small
        .iter()
        .map(|p| p.map(|v| (v * 65535.0).round() / 65535.0))
        .collect();
    let reference = half(
        STRIP_W,
        STRIP_H,
        &presence(STRIP_W, STRIP_H, &source, params),
    );
    let preview = presence(hw, hh, &small_codes, params);
    let row = |pixels: &[[f64; 3]], x: u32| pixels[((hh / 2) * hw + x) as usize][0];
    let p = period / 2.0;
    let reference_ratio =
        amplitude(hw, |x| row(&reference, x), p) / amplitude(hw, |x| row(&small, x), p);
    let preview_ratio =
        amplitude(hw, |x| row(&preview, x), p) / amplitude(hw, |x| row(&small_codes, x), p);
    (preview_ratio, reference_ratio)
}

#[test]
fn texture_and_clarity_read_the_same_at_half_size() {
    let mut failures = Vec::new();
    for (params, period) in [
        (json!({"texture":100}), 24.0),
        (json!({"clarity":100}), 96.0),
    ] {
        let (preview, reference) = strip_ratios(period, &params);
        if (preview - reference).abs() > 0.10 {
            failures.push(format!(
                "{params} at P {period}: preview ratio {preview:.4}, reference {reference:.4}"
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

// ---- HAZED ----------------------------------------------------------------------

const SCENE: u32 = 512;
const TILE: u32 = 16;
const SKY_ROWS: u32 = 96;
const AIRLIGHT: [f64; 3] = [0.90, 0.92, 0.95];

/// The clean full-size scene below the sky: 16-pixel tiles, each one colour
/// with one channel at 0 and the other two between 0.25 and 1.
fn scene(x: u32, y: u32) -> [f64; 3] {
    let (tx, ty) = (x / TILE, y / TILE);
    let level = |k: u32| 0.25 + 0.75 * f64::from((tx * 37 + ty * 59 + k * 101) % 97) / 96.0;
    let mut colour = [level(0), level(1), level(2)];
    colour[((tx + 2 * ty) % 3) as usize] = 0.0;
    colour
}

fn hazed(x: u32, y: u32) -> [f64; 3] {
    if y < SKY_ROWS {
        return AIRLIGHT;
    }
    let t = if x < SCENE / 2 { 0.8 } else { 0.5 };
    let j = scene(x, y);
    [0, 1, 2].map(|c| j[c] * t + AIRLIGHT[c] * (1.0 - t))
}

#[test]
fn dehaze_reads_the_same_at_half_size() {
    let params = json!({"dehaze":100});
    let source: Vec<[f64; 3]> = (0..SCENE * SCENE)
        .map(|i| hazed(i % SCENE, i / SCENE).map(|v| (v * 65535.0).round() / 65535.0))
        .collect();
    let reference = half(SCENE, SCENE, &presence(SCENE, SCENE, &source, &params));
    let small = half(SCENE, SCENE, &source);
    let (hs, sky, boundary) = (SCENE / 2, SKY_ROWS / 2, SCENE / 4);
    let preview = presence(hs, hs, &small, &params);
    let (mut sum, mut count) = (0.0, 0.0);
    for y in sky + 9..hs {
        for x in 0..hs {
            // More than 8 pixels from the sky's edge (the rows) and from the
            // line between the two transmission zones.
            if !(x + 9 < boundary || x > boundary + 8) {
                continue;
            }
            let i = (y * hs + x) as usize;
            for c in 0..3 {
                sum += (preview[i][c] - reference[i][c]).abs();
                count += 1.0;
            }
        }
    }
    assert!(count > 0.0);
    let mean = sum / count;
    assert!(mean <= 0.02, "mean |preview − reference| {mean:.4}");
}
