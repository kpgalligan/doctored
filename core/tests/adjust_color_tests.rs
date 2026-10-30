//! The two HSL-family ops of `dr_image_adjust_op`, `vibrance` and
//! `hue_saturation`, exercised through the C ABI, plus `color_balance`'s
//! defaults and malformed parameters (its form is pinned against Photoshop
//! in `adjust_color_balance_parity`).
//!
//! Every expected value comes from an ORACLE WRITTEN HERE from the published
//! formula — the hexcone conversions — or, for `hue_saturation`, from the
//! R0 oracle (`adjust_hue_saturation_parity`), never from the core's own
//! output. Vibrance's form is pinned against Photoshop, with its own oracle,
//! in `adjust_vibrance_posterize_parity` (#578); this file holds the
//! properties its sliders promise.

use std::ffi::{c_char, CString};
use std::ptr;

use doctored_core::ffi::*;
use doctored_core::ffi_adjust::*;
use doctored_core::DrImage;
use image::{Rgba, RgbaImage};
use tempfile::TempDir;

use crate::adjust_hue_saturation_parity::{master as r0_master, r0};
use crate::common;
use common::*;

// ------------------------------------------------------------- plumbing --

/// Runs `op` with `params` over `img` through the ONE destructive export.
fn adjust(img: *const DrImage, op: &str, params: &str) -> *mut DrImage {
    let c_op = CString::new(op).unwrap();
    let c_params = CString::new(params).unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    let out = unsafe { dr_image_adjust_op(img, c_op.as_ptr(), c_params.as_ptr(), &mut err) };
    assert!(
        !out.is_null(),
        "dr_image_adjust_op({op}, {params}) failed: {}",
        take_err_string(err)
    );
    assert!(err.is_null(), "err_out set on success");
    out
}

/// A one-row image of the given colours.
fn row_of(colors: &[[u8; 3]]) -> RgbaImage {
    RgbaImage::from_fn(colors.len() as u32, 1, |x, _| {
        let c = colors[x as usize];
        Rgba([c[0], c[1], c[2], 255])
    })
}

/// The RGB triples `op` produces for `src`, alpha asserted untouched.
fn run(dir: &TempDir, tag: &str, src: &RgbaImage, op: &str, params: &str) -> Vec<[u8; 3]> {
    let img = open_pattern(dir, &format!("{tag}.png"), src);
    let out = adjust(img, op, params);
    let bytes = pixels(out);
    let alpha: Vec<u8> = bytes.iter().skip(3).step_by(4).copied().collect();
    let expected: Vec<u8> = src.pixels().map(|p| p[3]).collect();
    assert_eq!(alpha, expected, "{tag}: alpha must never be touched");
    unsafe { dr_image_free(out) };
    free(img);
    bytes.chunks_exact(4).map(|p| [p[0], p[1], p[2]]).collect()
}

/// The raw RGBA bytes `op` produces for `src` — for the comparisons that
/// must be byte-for-byte rather than within a rounding step.
fn run_bytes(dir: &TempDir, tag: &str, src: &RgbaImage, op: &str, params: &str) -> Vec<u8> {
    let img = open_pattern(dir, &format!("{tag}.png"), src);
    let out = adjust(img, op, params);
    let bytes = pixels(out);
    unsafe { dr_image_free(out) };
    free(img);
    bytes
}

/// The op must refuse `params` with a message naming it.
fn refuses(dir: &TempDir, tag: &str, op: &str, params: &str) {
    let img = open_pattern(dir, &format!("{tag}.png"), &row_of(&[[128, 128, 128]]));
    let c_op = CString::new(op).unwrap();
    let c_params = CString::new(params).unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    let out = unsafe { dr_image_adjust_op(img, c_op.as_ptr(), c_params.as_ptr(), &mut err) };
    assert!(out.is_null(), "{op} {params} must be refused");
    assert!(
        take_err_string(err).contains(op),
        "{op} {params}: the message must name the op"
    );
    free(img);
}

fn norm(c: [u8; 3]) -> [f64; 3] {
    c.map(|v| f64::from(v) / 255.0)
}

fn assert_bytes(got: [u8; 3], want: [f64; 3], what: &str) {
    for (c, (g, w)) in got.iter().zip(want).enumerate() {
        let want_byte = (w.clamp(0.0, 1.0) * 255.0).round();
        assert!(
            (f64::from(*g) - want_byte).abs() <= 1.0,
            "{what} channel {c}: {g} vs {want_byte}"
        );
    }
}

// ------------------------------------------------------------- the oracle --
//
// Written from the published formulas, in f64, deliberately the long way.

fn rgb_to_hsl(rgb: [f64; 3]) -> [f64; 3] {
    let mx = rgb[0].max(rgb[1]).max(rgb[2]);
    let mn = rgb[0].min(rgb[1]).min(rgb[2]);
    let d = mx - mn;
    let l = (mx + mn) / 2.0;
    if d == 0.0 {
        return [0.0, 0.0, l];
    }
    let s = if l <= 0.5 {
        d / (mx + mn)
    } else {
        d / (2.0 - mx - mn)
    };
    let h = if mx == rgb[0] {
        60.0 * (((rgb[1] - rgb[2]) / d).rem_euclid(6.0))
    } else if mx == rgb[1] {
        60.0 * ((rgb[2] - rgb[0]) / d + 2.0)
    } else {
        60.0 * ((rgb[0] - rgb[1]) / d + 4.0)
    };
    [h.rem_euclid(360.0), s, l]
}

fn hsl_to_rgb(hsl: [f64; 3]) -> [f64; 3] {
    let (h, s, l) = (hsl[0].rem_euclid(360.0), hsl[1], hsl[2]);
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let hp = h / 60.0;
    let x = c * (1.0 - (hp % 2.0 - 1.0).abs());
    let (r, g, b) = match hp as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = l - c / 2.0;
    [r + m, g + m, b + m]
}

/// Hue/Saturation's two-sided slider curve: `+1` reaches the top of the
/// range, `-1` the bottom, continuous at 0.
fn two_sided(v: f64, a: f64) -> f64 {
    if a >= 0.0 {
        v + a * (1.0 - v)
    } else {
        v * (1.0 + a)
    }
    .clamp(0.0, 1.0)
}

// ------------------------------------------------------------- vibrance --

/// A full-chroma pixel has nothing left to boost, so a positive Vibrance
/// leaves it exactly; a negative one pulls it toward grey and never past it
/// (the form is pinned against Photoshop in `adjust_vibrance_posterize_parity`).
#[test]
fn vibrance_spares_full_chroma_above_zero_and_converges_toward_grey_below() {
    let dir = TempDir::new().unwrap();
    let src = row_of(&[[255, 0, 0], [0, 255, 0], [255, 64, 64], [200, 150, 120]]);
    let out = run(&dir, "vib-up", &src, "vibrance", "{\"vibrance\":1}");
    assert_eq!(out[0], [255, 0, 0], "full-chroma red is left alone");
    assert_eq!(out[1], [0, 255, 0], "full-chroma green is left alone");
    let extent = |p: [u8; 3]| {
        i32::from(p.iter().copied().max().unwrap()) - i32::from(p.iter().copied().min().unwrap())
    };
    for v in ["-0.3", "-0.7", "-1"] {
        let out = run(
            &dir,
            "vib-down",
            &src,
            "vibrance",
            &format!("{{\"vibrance\":{v}}}"),
        );
        for (i, (before, after)) in src.pixels().zip(&out).enumerate() {
            let before = [before[0], before[1], before[2]];
            assert!(
                extent(*after) <= extent(before),
                "vibrance {v}, pixel {i}: {before:?} -> {after:?} gained chroma"
            );
        }
    }
}

/// The two sliders in the one dialog must describe the SAME pixel, and both
/// properties asserted here follow from what the controls MEAN, not from how
/// they are written:
///
/// * Saturation at rest is the limit of approaching it, so a 0.1 % nudge
///   may not move a pixel more than one code.
/// * A positive Saturation may never REDUCE a pixel's chroma extent.
#[test]
fn vibrance_saturation_is_continuous_where_the_gain_leaves_the_gamut() {
    let dir = TempDir::new().unwrap();
    let probes = [
        [255u8, 128, 128],
        [128, 255, 128],
        [128, 128, 255],
        [250, 210, 120],
    ];
    let src = row_of(&probes);

    let extent = |p: [u8; 3]| {
        i32::from(p.iter().copied().max().unwrap_or(0))
            - i32::from(p.iter().copied().min().unwrap_or(0))
    };
    let at_rest = run(&dir, "vib-gamut-0", &src, "vibrance", "{\"vibrance\":1}");

    let nudged = run(
        &dir,
        "vib-gamut-eps",
        &src,
        "vibrance",
        "{\"vibrance\":1,\"saturation\":0.001}",
    );
    for (i, (rest, near)) in at_rest.iter().zip(&nudged).enumerate() {
        for c in 0..3 {
            assert!(
                (i32::from(near[c]) - i32::from(rest[c])).abs() <= 1,
                "probe {i} channel {c}: saturation 0.001 jumped {} -> {}",
                rest[c],
                near[c]
            );
        }
    }

    for s in ["0.01", "0.1", "0.5", "1"] {
        let out = run(
            &dir,
            "vib-gamut-s",
            &src,
            "vibrance",
            &format!("{{\"vibrance\":1,\"saturation\":{s}}}"),
        );
        for (i, (rest, more)) in at_rest.iter().zip(&out).enumerate() {
            assert!(
                extent(*more) >= extent(*rest) - 1,
                "probe {i}: saturation {s} DESATURATED {rest:?} -> {more:?}"
            );
        }
    }
}

#[test]
fn vibrance_defaults_are_a_byte_exact_no_op() {
    let dir = TempDir::new().unwrap();
    let src = opaque_pattern(16, 16);
    for params in ["{}", "{\"vibrance\":0,\"saturation\":0}"] {
        assert_eq!(
            run_bytes(&dir, "vib-id", &src, "vibrance", params),
            *src.as_raw(),
            "{params} must be an exact identity"
        );
    }
}

#[test]
fn vibrance_refuses_out_of_range_parameters() {
    let dir = TempDir::new().unwrap();
    for params in [
        "{\"vibrance\":1.5}",
        "{\"vibrance\":-1.01}",
        "{\"saturation\":2}",
        "{\"vibrance\":\"lots\"}",
    ] {
        refuses(&dir, "vib-bad", "vibrance", params);
    }
}

// -------------------------------------------------------- hue/saturation --

/// A 360-pixel hue sweep at half saturation, so both directions of every
/// slider have somewhere to go.
fn hue_sweep() -> RgbaImage {
    RgbaImage::from_fn(360, 1, |x, _| {
        let rgb = hsl_to_rgb([f64::from(x), 0.5, 0.5]);
        Rgba([
            (rgb[0] * 255.0).round() as u8,
            (rgb[1] * 255.0).round() as u8,
            (rgb[2] * 255.0).round() as u8,
            255,
        ])
    })
}

/// THE band invariant: the factory handles make adjacent bands sum to
/// exactly 1 at every hue, so the same Hue or Saturation edit on all six
/// bands is the same picture as one master edit. Asserted through the op at
/// every hue, which is the only way to see the weights at all. (Lightness is
/// not in the list: Photoshop's band lightness is netted toward the pixel's
/// max or min, a different operation from the master's per-component
/// lightness — R0, `adjust_hue_saturation`.)
#[test]
fn hue_saturation_bands_sum_to_one_at_every_hue() {
    let dir = TempDir::new().unwrap();
    let src = hue_sweep();
    let all_six = |key: &str, value: &str| {
        let bands = ["reds", "yellows", "greens", "cyans", "blues", "magentas"]
            .map(|name| format!("\"{name}\":{{\"{key}\":{value}}}"))
            .join(",");
        format!("{{\"bands\":{{{bands}}}}}")
    };
    for (key, value) in [("saturation", "0.4"), ("hue", "30")] {
        let banded = run_bytes(
            &dir,
            "hs-bands",
            &src,
            "hue_saturation",
            &all_six(key, value),
        );
        let master = run_bytes(
            &dir,
            "hs-master",
            &src,
            "hue_saturation",
            &format!("{{\"{key}\":{value}}}"),
        );
        for (i, (a, b)) in banded.iter().zip(&master).enumerate() {
            assert!(
                (i32::from(*a) - i32::from(*b)).abs() <= 1,
                "{key}: hue {} byte {} is {a} through the bands and {b} through the master",
                i / 4,
                i % 4
            );
        }
    }
}

/// One band edits its own hues and nothing else, with the ramp between.
#[test]
fn hue_saturation_band_reaches_only_its_own_hues() {
    let dir = TempDir::new().unwrap();
    let src = hue_sweep();
    let out = run(
        &dir,
        "hs-reds",
        &src,
        "hue_saturation",
        "{\"bands\":{\"reds\":{\"lightness\":-0.5}}}",
    );
    let flat = src.pixels().map(|p| [p[0], p[1], p[2]]).collect::<Vec<_>>();
    for hue in [0usize, 10, 15, 350, 359] {
        assert_ne!(out[hue], flat[hue], "hue {hue} is inside the Reds band");
    }
    // Photoshop places the outer handle at 45 deg on a 1536-unit wheel read
    // through 359 deg (take3's F-wheel), so the band ends just past 45.
    for hue in [50usize, 90, 180, 270, 315] {
        assert_eq!(out[hue], flat[hue], "hue {hue} is outside the Reds band");
    }
    // The ramp: at 30 deg the weight is about 0.5, so the edit is about half
    // of a full one — bracketed by the untouched and the fully edited pixel
    // (the band's netted lightness holds the minimum component).
    let full = run(
        &dir,
        "hs-full",
        &src,
        "hue_saturation",
        "{\"lightness\":-0.5}",
    );
    for c in 0..3 {
        let (plain, half, whole) = (
            i32::from(flat[30][c]),
            i32::from(out[30][c]),
            i32::from(full[30][c]),
        );
        assert!(
            half <= plain && half > whole,
            "channel {c} at hue 30: {plain} -> {half}, full edit {whole}"
        );
    }
    // A band may be moved and narrowed: centred on 180 with no falloff it
    // is a hard-edged window.
    let out = run(
        &dir,
        "hs-window",
        &src,
        "hue_saturation",
        "{\"bands\":{\"reds\":{\"lightness\":-0.5,\"center\":180,\"inner\":10,\"falloff\":0}}}",
    );
    assert_ne!(out[175], flat[175], "inside the moved window");
    assert_eq!(out[195], flat[195], "outside it, with no ramp at all");
}

/// The master sliders through the C ABI against the R0 oracle
/// (`adjust_hue_saturation_parity::r0`, Photoshop units).
#[test]
fn hue_saturation_matches_the_r0_oracle() {
    let dir = TempDir::new().unwrap();
    let src = opaque_pattern(16, 16);
    for (hue, sat, light) in [
        (0.0, 0.5, 0.0),
        (0.0, -0.5, 0.0),
        (45.0, 0.0, 0.0),
        (-120.0, 0.3, -0.2),
        (180.0, -1.0, 0.4),
    ] {
        let params = format!("{{\"hue\":{hue},\"saturation\":{sat},\"lightness\":{light}}}");
        let out = run(&dir, "hs-oracle", &src, "hue_saturation", &params);
        let setting = r0_master(hue, sat * 100.0, light * 100.0);
        for (px, got) in src.pixels().zip(&out) {
            let want = r0(norm([px[0], px[1], px[2]]), &setting);
            assert_bytes(*got, want, &format!("hue_saturation {params}"));
        }
    }
    // The two curve values the phase plan pins.
    assert!((two_sided(0.4, 0.5) - 0.7).abs() < 1e-12);
    assert!((two_sided(0.4, -0.5) - 0.2).abs() < 1e-12);
}

/// Colorize REPLACES hue and saturation and keeps only the lightness, which
/// is why it is a checkbox and not another slider.
#[test]
fn hue_saturation_colorize_replaces_hue_and_saturation() {
    let dir = TempDir::new().unwrap();
    let src = row_of(&[[200, 40, 40], [40, 200, 40], [128, 128, 128]]);
    let out = run(
        &dir,
        "hs-colorize",
        &src,
        "hue_saturation",
        "{\"colorize\":true,\"colorize_hue\":210,\"colorize_saturation\":0.5}",
    );
    for (px, got) in src.pixels().zip(&out) {
        let hsl = rgb_to_hsl(norm([px[0], px[1], px[2]]));
        assert_bytes(*got, hsl_to_rgb([210.0, 0.5, hsl[2]]), "colorize");
        let out_hsl = rgb_to_hsl(norm(*got));
        assert!(
            (out_hsl[0] - 210.0).abs() < 1.0 && (out_hsl[1] - 0.5).abs() < 0.02,
            "every pixel comes back at the colorize hue and saturation: {out_hsl:?}"
        );
    }
    // Colorize is what tints a NEUTRAL — the plain sliders cannot.
    assert_ne!(out[2], [128, 128, 128], "the grey is tinted by Colorize");
}

/// A pixel with no chroma has no hue to saturate, so the saturation curve
/// leaves it alone however far the slider goes. That fixed point is the
/// model's own rule and the reason Colorize exists.
#[test]
fn hue_saturation_leaves_neutrals_neutral() {
    let dir = TempDir::new().unwrap();
    let src = row_of(&[[0, 0, 0], [64, 64, 64], [128, 128, 128], [255, 255, 255]]);
    let out = run(
        &dir,
        "hs-grey",
        &src,
        "hue_saturation",
        "{\"saturation\":1,\"hue\":90}",
    );
    for (px, got) in src.pixels().zip(&out) {
        assert_eq!(
            *got,
            [px[0], px[1], px[2]],
            "a neutral pixel is not colorized by the saturation slider"
        );
    }
    // Lightness has no such rule: pushing black to white is its whole job.
    let out = run(
        &dir,
        "hs-light",
        &src,
        "hue_saturation",
        "{\"lightness\":1}",
    );
    assert_eq!(out[0], [255, 255, 255], "black to white at lightness +1");

    // A BAND makes the same promise, and it is the one a hue alone cannot
    // keep: `rgb_to_hsl` answers 0 deg for a neutral, so every grey in the
    // document used to sit at the dead centre of Reds and took its edits in
    // full. Hue and Saturation hid it (a zero-chroma pixel is a fixed point
    // of both); LIGHTNESS did not, which made one slider of one range
    // behave unlike all the others. Each band in turn, with all three of
    // its sliders pushed, must be a byte-exact identity on every neutral.
    for band in ["reds", "yellows", "greens", "cyans", "blues", "magentas"] {
        let params = format!(
            "{{\"bands\":{{\"{band}\":{{\"hue\":60,\"saturation\":0.5,\"lightness\":0.5}}}}}}"
        );
        let out = run(&dir, "hs-band-grey", &src, "hue_saturation", &params);
        for (px, got) in src.pixels().zip(&out) {
            assert_eq!(
                *got,
                [px[0], px[1], px[2]],
                "the {band} band must not edit a neutral, which has no hue to be one of"
            );
        }
    }
}

/// One 8-bit level of chroma is not a hue, it is quantization: a pixel with
/// `max - min == 1` lands on an exact multiple of 60 deg, so a band edit
/// must not blotch a flat grey — sensor noise in a sky, a grey wall behind a
/// red jacket. Photoshop's band lightness (R0) is netted toward the
/// pixel's own max or min, so it moves a near-neutral by at most its own
/// chroma: nothing at zero chroma, and monotone in the chroma.
#[test]
fn hue_saturation_bands_leave_a_near_neutral_near_neutral() {
    let dir = TempDir::new().unwrap();
    let reds = "{\"bands\":{\"reds\":{\"lightness\":0.5}}}";

    // A 64x64 patch of rgb(128 +/- 1): every channel independent, so all six
    // quantized hues occur. FNV-1a over the coordinate keeps it deterministic
    // — a failure reproduces exactly.
    let noise = RgbaImage::from_fn(64, 64, |x, y| {
        let level = |c: u32| {
            let mut h: u64 = 0xcbf2_9ce4_8422_2325;
            for b in [x as u8, (x >> 8) as u8, y as u8, (y >> 8) as u8, c as u8] {
                h = (h ^ u64::from(b)).wrapping_mul(0x100_0000_01b3);
            }
            127 + (h % 3) as u8
        };
        Rgba([level(0), level(1), level(2), 255])
    });
    let flat: Vec<[u8; 3]> = noise.pixels().map(|p| [p[0], p[1], p[2]]).collect();
    let spread = |v: &[[u8; 3]]| {
        let lo = v.iter().flatten().min().copied().unwrap_or(0);
        let hi = v.iter().flatten().max().copied().unwrap_or(0);
        i32::from(hi) - i32::from(lo)
    };
    assert_eq!(spread(&flat), 2, "the fixture is one level of noise");
    let out = run(&dir, "hs-noise", &noise, "hue_saturation", reds);
    // A residual proportional to the chroma is by design and cannot be
    // zero; what must not happen is that it exceeds the noise it is reading.
    assert!(
        spread(&out) <= spread(&flat) + 2,
        "a Reds Lightness edit spread a near-neutral patch over {} levels",
        spread(&out)
    );
    for (i, (before, after)) in flat.iter().zip(&out).enumerate() {
        for c in 0..3 {
            assert!(
                (i32::from(after[c]) - i32::from(before[c])).abs() <= 2,
                "pixel {i} channel {c} moved from {} to {}",
                before[c],
                after[c]
            );
        }
    }

    // r = 128 + d, g = b = 128 is hue 0 deg exactly (the Reds centre, band
    // weight 1) at chroma d/255: green moves half way to the max, so the
    // row must be non-decreasing, pinned at zero chroma.
    let ramp: Vec<[u8; 3]> = [0u8, 1, 2, 3, 5, 8, 13, 26, 64]
        .iter()
        .map(|d| [128 + d, 128, 128])
        .collect();
    let src = row_of(&ramp);
    let out = run(&dir, "hs-ramp", &src, "hue_saturation", reds);
    assert_eq!(out[0], ramp[0], "zero chroma takes none of the band");
    for i in 1..out.len() {
        assert!(
            out[i][1] >= out[i - 1][1],
            "the band edit must be monotone in chroma: {:?} then {:?}",
            out[i - 1],
            out[i]
        );
    }
}

#[test]
fn hue_saturation_defaults_are_a_byte_exact_no_op() {
    let dir = TempDir::new().unwrap();
    let src = opaque_pattern(16, 16);
    for params in [
        "{}",
        "{\"hue\":0,\"saturation\":0,\"lightness\":0}",
        // A band listed but left at rest changes nothing either.
        "{\"bands\":{\"reds\":{}}}",
    ] {
        assert_eq!(
            run_bytes(&dir, "hs-id", &src, "hue_saturation", params),
            *src.as_raw(),
            "{params} must be an exact identity"
        );
    }
}

#[test]
fn hue_saturation_refuses_malformed_parameters() {
    let dir = TempDir::new().unwrap();
    for params in [
        "{\"hue\":181}",
        "{\"hue\":-181}",
        "{\"saturation\":1.5}",
        "{\"bands\":7}",
        "{\"bands\":{\"reds\":\"hot\"}}",
        "{\"bands\":{\"reds\":{\"center\":360}}}",
        "{\"bands\":{\"reds\":{\"inner\":181}}}",
        "{\"bands\":{\"reds\":{\"falloff\":-1}}}",
        "{\"colorize\":\"yes\"}",
        "{\"colorize_hue\":360}",
        "{\"colorize_saturation\":-0.1}",
    ] {
        refuses(&dir, "hs-bad", "hue_saturation", params);
    }
}

// --------------------------------------------------------- color balance --

#[test]
fn color_balance_defaults_are_a_byte_exact_no_op() {
    let dir = TempDir::new().unwrap();
    let src = opaque_pattern(16, 16);
    // Preserve Luminosity defaults ON, and must still be an exact identity
    // when nothing moved: all nine units are 0.
    for params in ["{}", "{\"midtones\":{}}", "{\"preserve_luminosity\":true}"] {
        assert_eq!(
            run_bytes(&dir, "cb-id", &src, "color_balance", params),
            *src.as_raw(),
            "{params} must be an exact identity"
        );
    }
}

#[test]
fn color_balance_refuses_malformed_parameters() {
    let dir = TempDir::new().unwrap();
    for params in [
        "{\"shadows\":{\"cyan_red\":1.5}}",
        "{\"midtones\":{\"magenta_green\":-2}}",
        "{\"highlights\":[1,2,3]}",
        "{\"shadows\":{\"cyan_red\":\"warm\"}}",
        "{\"preserve_luminosity\":1}",
    ] {
        refuses(&dir, "cb-bad", "color_balance", params);
    }
}
