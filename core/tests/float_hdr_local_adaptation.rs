//! HDR Toning's Local Adaptation through the FFI
//! (`dr_precision_doc_hdr_toning`, its plan and
//! `dr_precision_doc_hdr_toning_histogram`): the closed forms, the guided
//! base against a binary64 guided filter, the Toning Curve, Saturation and
//! Shadow, the histogram, the refusals, the cancel and the NULL sweep.
//!
//! # The oracles
//!
//! Written here from `hdr_local_adaptation.rs`'s module doc, sharing no line
//! with the kernel: the Rec. 709 luminance, `ℓ = log2(max(Y, 2⁻²⁰))`, `t` over
//! the image's own log range, the He/Sun/Tang guided filter in binary64 with
//! clamped-edge box windows (alpha 1, so the alpha weights are plain counts),
//! `L = (B − 1)·span·k + (1 + detail/100)·D·span + exposure` with
//! `k = min(1, 8/span)/gamma`, `c·2^L/Y`, and Saturation as the pull about the
//! ROMM Y row `clamp01(A + (c − A)(1 + s))`. A scene with a channel below zero
//! adds the black floor `F` ([`black_floor`], kept only when the range's top
//! reaches `56F`) and the chroma weight `w = S²/(S² + (8F)²)` on the signed
//! luminance `S` ([`mapped_over_floor`]).
//! The linear results meet the sRGB
//! profile's own forward curve ([`encoded`]: `dr_precision_doc_float_to_u16`
//! under the clip map, as `float_hdr_toning` does), which is not under test.

use std::ffi::{c_char, CStr, CString};
use std::ptr;

use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_float::dr_precision_doc_float_to_u16;
use doctored_core::ffi_hdr_toning::{
    dr_precision_doc_hdr_toning, dr_precision_doc_hdr_toning_histogram,
    dr_precision_doc_hdr_toning_plan,
};
use doctored_core::float_document::ToneMap;
use doctored_core::icc::IccProfile;
use doctored_core::job_control::DrJobControl;
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::REFERENCE_WORKING_BYTES;
use serde_json::{json, Map, Value};

use crate::common::take_err_string;
use crate::float_fixture::float_doc;

const LIMIT: u64 = REFERENCE_WORKING_BYTES;

// ----------------------------------------------------------------- oracles --

fn luma(c: [f64; 3]) -> f64 {
    0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2]
}

fn log_luma(c: [f64; 3]) -> f64 {
    luma(c.map(|v| v.max(0.0))).max(2f64.powi(-20)).log2()
}

/// Saturation's pull about the ROMM RGB Y row (`adjust_color`'s module doc),
/// `clamp01(A + (c − A)·K)` on the unclamped linear values; at 0 the stage is
/// skipped and the display curve's clip is the clamp.
fn saturated(c: [f64; 3], saturation: f64) -> [f64; 3] {
    if saturation == 0.0 {
        return c.map(|v| v.clamp(0.0, 1.0));
    }
    let anchor = 0.288040 * c[0] + 0.711874 * c[1] + 0.000086 * c[2];
    c.map(|v| (anchor + (v - anchor) * (1.0 + saturation / 100.0)).clamp(0.0, 1.0))
}

/// The settings the oracle reads.
#[derive(Clone, Copy)]
struct Tone {
    gamma: f64,
    exposure: f64,
    detail: f64,
    saturation: f64,
}

const DEFAULTS: Tone = Tone {
    gamma: 1.0,
    exposure: 0.0,
    detail: 30.0,
    saturation: 20.0,
};

/// Steps 4 and 5 of the module doc for one pixel: its `t′`, its base delta
/// `d`, the span, then the linear result after Saturation.
fn mapped(c: [f64; 3], t: f64, d: f64, span: f64, tone: Tone) -> [f64; 3] {
    let k = (8.0 / span).min(1.0) / tone.gamma;
    let stops = (t + d - 1.0) * span * k + (1.0 + tone.detail / 100.0) * -d * span + tone.exposure;
    let target = stops.exp2();
    let y = luma(c.map(|v| v.max(0.0)));
    let scaled = if y > 0.0 {
        c.map(|v| v.max(0.0) * target / y)
    } else {
        [target; 3]
    };
    saturated(scaled, tone.saturation)
}

/// κ, the chroma weight's scale over the black floor (the module doc's
/// `FLOOR_CHROMA_SCALE`).
const FLOOR_SCALE: f64 = 8.0;

/// The multiple of `κF` the range's top must reach for a floor to be kept
/// (the module doc's `FLOOR_TOP_SCALE`).
const FLOOR_TOP: f64 = 7.0;

/// The black floor `F` of the module doc's step 1, from the pixel list: `ℓ₀`
/// is the 0.1 % alpha-weighted order statistic of `ℓ` over the visible pixels
/// that are not exactly black (the kernel's 65536-level edge meets it within a
/// level, and every scene here keeps its pixels clear of the band's edge by
/// more than that); the band is those pixels with `ℓ < ℓ₀ + 1`; when its
/// pixels with a channel below zero carry at least a quarter of its weight,
/// `F` is the weighted median of their `|LUMA·c|` read from 256 levels of
/// `log2`, eight a stop up from −20, at the level's centre, kept when the
/// largest `ℓ` of the visible pixels reaches `log2(7κF)`, else 0.
fn black_floor(pixels: &[[f32; 4]]) -> f64 {
    let top = pixels
        .iter()
        .filter(|p| p[3] > 0.0)
        .map(|p| log_luma([p[0], p[1], p[2]].map(f64::from)))
        .fold(f64::NEG_INFINITY, f64::max);
    let lit: Vec<([f64; 3], f64)> = pixels
        .iter()
        .filter(|p| p[3] > 0.0 && (p[0] != 0.0 || p[1] != 0.0 || p[2] != 0.0))
        .map(|p| ([p[0], p[1], p[2]].map(f64::from), f64::from(p[3])))
        .collect();
    let mut logs: Vec<(f64, f64)> = lit.iter().map(|(c, a)| (log_luma(*c), *a)).collect();
    logs.sort_by(|a, b| a.0.total_cmp(&b.0));
    let total: f64 = logs.iter().map(|l| l.1).sum();
    let mut cumulative = 0.0;
    let Some(&(low, _)) = logs.iter().find(|(_, a)| {
        cumulative += a;
        cumulative >= 0.001 * total
    }) else {
        return 0.0;
    };
    let band: Vec<&([f64; 3], f64)> = lit
        .iter()
        .filter(|(c, _)| log_luma(*c) < low + 1.0)
        .collect();
    let band_weight: f64 = band.iter().map(|(_, a)| a).sum();
    let below: Vec<(f64, f64)> = band
        .iter()
        .filter(|(c, _)| c.iter().any(|&v| v < 0.0))
        .map(|(c, a)| (luma(*c).abs(), *a))
        .collect();
    let weight: f64 = below.iter().map(|b| b.1).sum();
    if weight == 0.0 || weight < 0.25 * band_weight {
        return 0.0;
    }
    let mut levels = [0.0f64; 256];
    for (y, a) in below {
        let level = ((y.max(2f64.powi(-20)).log2() + 20.0) * 8.0).floor();
        levels[level.clamp(0.0, 255.0) as usize] += a;
    }
    let mut cumulative = 0.0;
    for (level, w) in levels.iter().enumerate() {
        cumulative += w;
        if *w > 0.0 && cumulative >= 0.5 * weight {
            let floor = (-20.0 + (level as f64 + 0.5) / 8.0).exp2();
            return if (FLOOR_TOP * FLOOR_SCALE * floor).log2() <= top {
                floor
            } else {
                0.0
            };
        }
    }
    unreachable!("the median level is occupied")
}

/// Step 5 over the black floor `floor`: the signed luminance `S` of the
/// unclamped triple sets `w = S²/(S² + (κF)²)` (0 for `S ≤ 0`, 1 for `F = 0`),
/// and `c′ = Y′·((1 − w) + w·c/Y)` on the clamped triple, then Saturation;
/// `mapped` is this at `w = 1` for every pixel.
fn mapped_over_floor(c: [f64; 3], t: f64, d: f64, span: f64, tone: Tone, floor: f64) -> [f64; 3] {
    let k = (8.0 / span).min(1.0) / tone.gamma;
    let stops = (t + d - 1.0) * span * k + (1.0 + tone.detail / 100.0) * -d * span + tone.exposure;
    let target = stops.exp2();
    let clamped = c.map(|v| v.max(0.0));
    let y = luma(clamped);
    let signal = luma(c);
    let weight = if signal <= 0.0 {
        0.0
    } else if floor > 0.0 {
        signal * signal / (signal * signal + (FLOOR_SCALE * floor).powi(2))
    } else {
        1.0
    };
    let scaled = if y > 0.0 {
        clamped.map(|v| target * ((1.0 - weight) + weight * v / y))
    } else {
        [target; 3]
    };
    saturated(scaled, tone.saturation)
}

/// Every pixel of an opaque `w` × `h` scene through the module doc's map at
/// radius `r` and the default Strength, as linear values after Saturation:
/// over the floor `floor` ([`mapped_over_floor`]), or by today's `mapped` for
/// `None`. Each scene holds more than 0.1 % of its pixels at its smallest
/// log luminance, so `low` is that minimum.
fn scene_map(
    pixels: &[[f32; 4]],
    (w, h, r): (usize, usize, usize),
    tone: Tone,
    floor: Option<f64>,
) -> Vec<[f64; 3]> {
    let linear: Vec<[f64; 3]> = pixels
        .iter()
        .map(|p| [p[0], p[1], p[2]].map(f64::from))
        .collect();
    let logs: Vec<f64> = linear.iter().map(|c| log_luma(*c)).collect();
    let low = logs.iter().copied().fold(f64::INFINITY, f64::min);
    let high = logs.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let at_low = logs.iter().filter(|&&l| l == low).count();
    assert!(
        at_low as f64 > 0.001 * logs.len() as f64,
        "low is the minimum"
    );
    let span = (high - low).max(1.0);
    let t: Vec<f64> = logs
        .iter()
        .map(|l| ((l - low) / span).clamp(0.0, 1.0))
        .collect();
    let d = guided_delta(&t, w, h, r, 0.52 / span);
    (0..w * h)
        .map(|i| match floor {
            Some(floor) => mapped_over_floor(linear[i], t[i], d[i], span, tone, floor),
            None => mapped(linear[i], t[i], d[i], span, tone),
        })
        .collect()
}

/// A deterministic standard normal stream: xorshift32 through Box–Muller.
struct Normal(u32);

impl Normal {
    fn uniform(&mut self) -> f64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        (f64::from(self.0) + 0.5) / 4_294_967_296.0
    }

    fn next(&mut self) -> f64 {
        let (u, v) = (self.uniform(), self.uniform());
        (-2.0 * u.ln()).sqrt() * (std::f64::consts::TAU * v).cos()
    }
}

/// The relative chroma `(max − min)/max` of a pixel's codes.
fn code_chroma(codes: [i64; 3]) -> f64 {
    let (high, low) = (codes.iter().max().unwrap(), codes.iter().min().unwrap());
    if *high == 0 {
        0.0
    } else {
        (high - low) as f64 / *high as f64
    }
}

/// The He/Sun/Tang guided filter of `guide` (also the input) in binary64:
/// window statistics over clamped-edge boxes of half-width `r`, the slope
/// `var/(var + ε²)`, the offset `mean·(1 − slope)`, both box-averaged again,
/// and the result clamped to 0…1, less the guide.
fn guided_delta(guide: &[f64], w: usize, h: usize, r: usize, level: f64) -> Vec<f64> {
    let boxed = |field: &dyn Fn(usize, usize) -> f64, x: usize, y: usize| {
        let mut sum = 0.0;
        for dy in -(r as i64)..=r as i64 {
            for dx in -(r as i64)..=r as i64 {
                let xx = (x as i64 + dx).clamp(0, w as i64 - 1) as usize;
                let yy = (y as i64 + dy).clamp(0, h as i64 - 1) as usize;
                sum += field(xx, yy);
            }
        }
        sum
    };
    let count = ((2 * r + 1) * (2 * r + 1)) as f64;
    let mut slope = vec![0.0; w * h];
    let mut offset = vec![0.0; w * h];
    for y in 0..h {
        for x in 0..w {
            let mean = boxed(&|xx, yy| guide[yy * w + xx], x, y) / count;
            let square = boxed(&|xx, yy| guide[yy * w + xx].powi(2), x, y) / count;
            let variance = (square - mean * mean).max(0.0);
            let a = if variance > 0.0 {
                variance / (variance + level * level)
            } else {
                0.0
            };
            slope[y * w + x] = a;
            offset[y * w + x] = mean * (1.0 - a);
        }
    }
    let mut out = vec![0.0; w * h];
    for y in 0..h {
        for x in 0..w {
            let a = boxed(&|xx, yy| slope[yy * w + xx], x, y) / count;
            let b = boxed(&|xx, yy| offset[yy * w + xx], x, y) / count;
            let i = y * w + x;
            out[i] = (a * guide[i] + b).clamp(0.0, 1.0) - guide[i];
        }
    }
    out
}

// ---------------------------------------------------------------- fixtures --

fn scene(w: u32, h: u32, pixels: Vec<[f32; 4]>) -> PrecisionDocument {
    let doc = float_doc(w, h, &IccProfile::srgb(), &[pixels]);
    match doc.with_tone_map(ToneMap::Clip).unwrap() {
        Some(clip) => clip,
        None => doc,
    }
}

/// Linear triples in [0, 1] through the sRGB profile's own forward curve.
fn encoded(values: &[[f64; 3]]) -> Vec<[i64; 3]> {
    let pixels: Vec<[f32; 4]> = values
        .iter()
        .map(|v| [v[0] as f32, v[1] as f32, v[2] as f32, 1.0])
        .collect();
    let doc = scene(pixels.len() as u32, 1, pixels);
    let mut error = ptr::null_mut();
    let out = unsafe { dr_precision_doc_float_to_u16(&doc, LIMIT, 0, ptr::null(), &mut error) };
    assert!(!out.is_null(), "{}", take_err_string(error));
    let out = *unsafe { Box::from_raw(out) };
    let codes = out.layer(0).unwrap().source().unwrap().to_rgba16().unwrap();
    codes
        .iter()
        .map(|p| [0, 1, 2].map(|c| i64::from(p[c])))
        .collect()
}

/// A neutral two-level image, `dark` left of column `w/2` and `bright` from
/// it on.
fn two_level(w: u32, h: u32, dark: f32, bright: f32) -> Vec<[f32; 4]> {
    (0..w * h)
        .map(|i| {
            let v = if i % w < w / 2 { dark } else { bright };
            [v, v, v, 1.0]
        })
        .collect()
}

// --------------------------------------------------------------------- ffi --

fn taken(pointer: *mut c_char) -> Option<String> {
    if pointer.is_null() {
        return None;
    }
    let text = unsafe { CStr::from_ptr(pointer) }
        .to_string_lossy()
        .into_owned();
    unsafe { dr_string_free(pointer) };
    Some(text)
}

/// A Local Adaptation request with `extra`'s keys.
fn la(extra: Value) -> Value {
    let mut map = Map::new();
    map.insert("method".into(), json!("local_adaptation"));
    if let Value::Object(more) = extra {
        map.extend(more);
    }
    Value::Object(map)
}

fn toned_with(
    doc: &PrecisionDocument,
    request: &Value,
    control: Option<&DrJobControl>,
) -> Result<PrecisionDocument, String> {
    let text = CString::new(request.to_string()).unwrap();
    let mut error = ptr::null_mut();
    let control = control.map_or(ptr::null(), |c| c as *const DrJobControl);
    let out =
        unsafe { dr_precision_doc_hdr_toning(doc, text.as_ptr(), LIMIT, 0, control, &mut error) };
    if out.is_null() {
        return Err(taken(error).expect("a NULL result names its reason"));
    }
    Ok(*unsafe { Box::from_raw(out) })
}

fn codes(doc: &PrecisionDocument, request: &Value) -> Vec<[u16; 4]> {
    let out = toned_with(doc, request, None).unwrap_or_else(|e| panic!("{request}: {e}"));
    assert_eq!(out.layer_count(), 1, "the result is one raster");
    assert!(!out.is_float(), "the result is 16-bit");
    out.layer(0).unwrap().source().unwrap().to_rgba16().unwrap()
}

fn histogram_with(
    doc: &PrecisionDocument,
    control: Option<&DrJobControl>,
) -> Result<Value, String> {
    let mut error = ptr::null_mut();
    let control = control.map_or(ptr::null(), |c| c as *const DrJobControl);
    let out = unsafe { dr_precision_doc_hdr_toning_histogram(doc, LIMIT, 0, control, &mut error) };
    match taken(out) {
        Some(reply) => Ok(serde_json::from_str(&reply).unwrap()),
        None => Err(taken(error).expect("a NULL histogram names its reason")),
    }
}

fn plan_bytes(doc: &PrecisionDocument, request: &Value) -> u64 {
    let text = CString::new(request.to_string()).unwrap();
    let mut error = ptr::null_mut();
    let out = unsafe { dr_precision_doc_hdr_toning_plan(doc, text.as_ptr(), 16, &mut error) };
    let reply = taken(out).unwrap_or_else(|| panic!("{}", take_err_string(error)));
    let reply: Value = serde_json::from_str(&reply).unwrap();
    reply["working_memory_plan_bytes"].as_u64().unwrap()
}

fn within(got: i64, want: i64, bound: i64, label: &str) {
    assert!(
        (got - want).abs() <= bound,
        "{label}: {got} against {want} (bound {bound})"
    );
}

// ------------------------------------------------------------------- tests --

#[test]
fn a_flat_image_is_the_closed_form() {
    let colour = [0.4f32, 0.2, 0.1];
    let pixels = vec![[colour[0], colour[1], colour[2], 1.0]; 12 * 8];
    let doc = scene(12, 8, pixels);
    let c = colour.map(f64::from);
    for (request, tone) in [
        (la(json!({})), DEFAULTS),
        (
            la(json!({"exposure": 0.5, "gamma": 1.5, "saturation": 0, "detail": 200})),
            Tone {
                gamma: 1.5,
                exposure: 0.5,
                detail: 200.0,
                saturation: 0.0,
            },
        ),
    ] {
        // span 1 (a single level), t = 0 and d = 0 on a flat plane.
        let want = encoded(&[mapped(c, 0.0, 0.0, 1.0, tone)])[0];
        for pixel in codes(&doc, &request) {
            for ch in 0..3 {
                within(
                    i64::from(pixel[ch]),
                    want[ch],
                    1,
                    &format!("{request} ch {ch}"),
                );
            }
            assert_eq!(pixel[3], 65535);
        }
    }
}

#[test]
fn a_step_keeps_each_side_away_from_the_edge() {
    let (w, h, radius) = (40u32, 12u32, 4u32);
    let (dark, bright) = (2f32.powi(-6), 4.0f32);
    let doc = scene(w, h, two_level(w, h, dark, bright));
    let request = la(json!({"radius": radius, "strength": 0.1, "detail": 30}));
    let got = codes(&doc, &request);
    let span = log_luma([f64::from(bright); 3]) - log_luma([f64::from(dark); 3]);
    let tone = DEFAULTS;
    let want_dark = encoded(&[mapped([f64::from(dark); 3], 0.0, 0.0, span, tone)])[0];
    let want_bright = encoded(&[mapped([f64::from(bright); 3], 1.0, 0.0, span, tone)])[0];
    let reach = 2 * radius + 1;
    for (i, pixel) in got.iter().enumerate() {
        let x = i as u32 % w;
        let want = if x + reach < w / 2 {
            want_dark
        } else if x >= w / 2 + reach {
            want_bright
        } else {
            continue;
        };
        for ch in 0..3 {
            within(
                i64::from(pixel[ch]),
                want[ch],
                2,
                &format!("step x {x} ch {ch}"),
            );
        }
    }
    assert_eq!(want_bright, [65535; 3], "the bright side is white");
    assert!(want_dark[0] < 10000, "the dark side is eight stops down");
}

#[test]
fn one_black_pixel_barely_moves_the_low_end() {
    let (w, h) = (64u32, 32u32);
    let pixels = two_level(w, h, 2f32.powi(-6), 2.0);
    let before = histogram_with(&scene(w, h, pixels.clone()), None).unwrap();
    let mut with_black = pixels;
    with_black[5] = [0.0, 0.0, 0.0, 1.0];
    let after = histogram_with(&scene(w, h, with_black), None).unwrap();
    let low = |v: &Value| v["low_stops"].as_f64().unwrap();
    assert!((low(&before) + 6.0).abs() < 1e-4, "{before}");
    assert!(
        (low(&after) - low(&before)).abs() <= 1.0 / 1024.0,
        "{} against {}",
        low(&after),
        low(&before)
    );
}

#[test]
fn the_base_is_a_binary64_guided_filter() {
    let (w, h, r) = (16usize, 12usize, 2usize);
    let mut seed = 0x2545_f491_u32;
    let mut next = || {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        f64::from(seed % 10_000) / 10_000.0
    };
    let pixels: Vec<[f32; 4]> = (0..w * h)
        .map(|_| {
            let stops = -7.0 + 7.5 * next();
            let y = stops.exp2();
            [
                (y * (0.6 + 0.8 * next())) as f32,
                y as f32,
                (y * (0.5 + next())) as f32,
                1.0,
            ]
        })
        .collect();
    let doc = scene(w as u32, h as u32, pixels.clone());
    let strength = 0.52;
    let got = codes(&doc, &la(json!({"radius": r})));
    let linear: Vec<[f64; 3]> = pixels
        .iter()
        .map(|p| [p[0], p[1], p[2]].map(f64::from))
        .collect();
    let logs: Vec<f64> = linear.iter().map(|c| log_luma(*c)).collect();
    let low = logs.iter().copied().fold(f64::INFINITY, f64::min);
    let high = logs.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let span = (high - low).max(1.0);
    assert!(span > 3.0, "the noise spans several stops");
    let t: Vec<f64> = logs
        .iter()
        .map(|l| ((l - low) / span).clamp(0.0, 1.0))
        .collect();
    let d = guided_delta(&t, w, h, r, strength / span);
    assert!(d.iter().any(|v| v.abs() > 0.01), "the base moves");
    let values: Vec<[f64; 3]> = (0..w * h)
        .map(|i| mapped(linear[i], t[i], d[i], span, DEFAULTS))
        .collect();
    let want = encoded(&values);
    for (i, pixel) in got.iter().enumerate() {
        for ch in 0..3 {
            within(
                i64::from(pixel[ch]),
                want[i][ch],
                2,
                &format!("pixel {i} ch {ch}"),
            );
        }
    }
}

/// Every pixel of the toned `doc` within 2 codes of the oracle over `floor`,
/// at the defaults and at Saturation 0, both at the scene's radius.
fn matches_over_floor(
    doc: &PrecisionDocument,
    pixels: &[[f32; 4]],
    size: (usize, usize, usize),
    floor: f64,
) {
    let grey = Tone {
        saturation: 0.0,
        ..DEFAULTS
    };
    for (request, tone) in [
        (la(json!({"radius": size.2})), DEFAULTS),
        (la(json!({"radius": size.2, "saturation": 0})), grey),
    ] {
        let want = encoded(&scene_map(pixels, size, tone, Some(floor)));
        for (i, pixel) in codes(doc, &request).iter().enumerate() {
            for ch in 0..3 {
                within(
                    i64::from(pixel[ch]),
                    want[i][ch],
                    2,
                    &format!("{request} pixel {i} ch {ch}"),
                );
            }
        }
    }
}

#[test]
fn near_black_noise_lifts_as_a_neutral() {
    let (w, h, r) = (64usize, 48usize, 2usize);
    let mut normal = Normal(0x9e37_79b9);
    // Zero-mean noise at black on the left, blue the noisiest channel; a
    // neutral field on the right.
    let mut pixels: Vec<[f32; 4]> = (0..w * h)
        .map(|i| {
            if i % w < w / 2 {
                let [r, g, b] = [1.5e-4, 1.0e-4, 4.0e-4].map(|s| (s * normal.next()) as f32);
                [r, g, b, 1.0]
            } else {
                [0.4, 0.4, 0.4, 1.0]
            }
        })
        .collect();
    let at = |x: usize, y: usize| y * w + x;
    let (issue, patch, red, outside, partial) =
        (at(8, 8), at(13, 37), at(44, 10), at(52, 30), at(16, 20));
    // The issue's pixel: blue over slightly negative red and green.
    pixels[issue] = [-2e-4, -1e-4, 3e-4, 1.0];
    // Two large all-negative outliers, which the floor's median passes over.
    pixels[at(20, 30)] = [-0.3, -0.3, -0.3, 1.0];
    pixels[at(26, 40)] = [-0.3, -0.3, -0.3, 1.0];
    // A dark orange patch fifty times the noise, a saturated red, and a
    // bright blue outside the gamut.
    for y in 36..39 {
        for x in 12..15 {
            pixels[at(x, y)] = [0.012, 0.006, 0.002, 1.0];
        }
    }
    pixels[red] = [0.6, 0.08, 0.03, 1.0];
    pixels[outside] = [-0.05, -0.02, 0.9, 1.0];
    pixels[partial] = [1e-3, 1e-3, 1e-3, 1.0];
    let floor = black_floor(&pixels);
    let sigma = ((0.2126f64 * 1.5e-4).powi(2)
        + (0.7152f64 * 1.0e-4).powi(2)
        + (0.0722f64 * 4.0e-4).powi(2))
    .sqrt();
    assert!(
        floor > sigma / 2.0 && floor < 2.0 * sigma,
        "the floor {floor} is the noise's {sigma}, not the outliers'"
    );
    // A pixel whose signed luminance is κF keeps half its chroma; a pixel
    // above the band never moves the floor.
    let scale = FLOOR_SCALE * floor / luma([2.0, 0.8, 0.5]);
    pixels[partial] = [2.0 * scale, 0.8 * scale, 0.5 * scale, 1.0].map(|v| v as f32);
    assert_eq!(black_floor(&pixels), floor);
    let doc = scene(w as u32, h as u32, pixels.clone());
    let got = codes(&doc, &la(json!({"radius": r})));
    let code = |i: usize| [0, 1, 2].map(|c| i64::from(got[i][c]));
    let today = encoded(&scene_map(&pixels, (w, h, r), DEFAULTS, None));
    assert!(
        today[issue][2] > today[issue][0] + 10_000,
        "today's map makes the issue's pixel blue: {:?}",
        today[issue]
    );
    let lifted = code(issue);
    assert!(
        lifted.iter().max().unwrap() - lifted.iter().min().unwrap() <= 1 && lifted[0] > 0,
        "the issue's pixel lifts as a neutral: {lifted:?}"
    );
    for (i, label) in [
        (red, "the saturated red"),
        (outside, "the blue outside the gamut"),
    ] {
        for (ch, (got, want)) in code(i).into_iter().zip(today[i]).enumerate() {
            within(got, want, 257, &format!("{label} ch {ch}"));
        }
    }
    assert!(
        code_chroma(code(patch)) > 0.9 * code_chroma(today[patch]),
        "the dark patch keeps its colour: {:?} against {:?}",
        code(patch),
        today[patch]
    );
    matches_over_floor(&doc, &pixels, (w, h, r), floor);
}

#[test]
fn a_floor_above_zero_luminance_lifts_as_a_neutral() {
    // A RAW developed into linear light keeps no negative luminance, but its
    // black floor is blue, with red and green dipping below zero; a black
    // border sits under it.
    let (w, h, r) = (64usize, 48usize, 2usize);
    let mut normal = Normal(0x2545_f491);
    let mut spread = Normal(0x1b87_3593);
    let pixels: Vec<[f32; 4]> = (0..w * h)
        .map(|i| {
            let (x, y) = (i % w, i / w);
            if y < 2 {
                [0.0, 0.0, 0.0, 1.0]
            } else if x < w / 2 {
                loop {
                    let c = [
                        1.6e-4 + 1.2e-4 * normal.next(),
                        1.0e-5 + 6.0e-5 * normal.next(),
                        9.0e-4 + 2.0e-4 * normal.next(),
                    ];
                    let y = luma(c.map(|v| v.max(0.0)));
                    if luma(c) > 0.0 {
                        let scale = (-13.4 + 0.3 * spread.uniform()).exp2() / y;
                        break [c[0] * scale, c[1] * scale, c[2] * scale, 1.0].map(|v| v as f32);
                    }
                }
            } else if (y / 8) % 2 == 0 {
                [0.4, 0.4, 0.4, 1.0]
            } else {
                [0.3, 0.12, 0.05, 1.0]
            }
        })
        .collect();
    let floor = black_floor(&pixels);
    assert!(floor > 0.0, "the darkest stop is the floor");
    let doc = scene(w as u32, h as u32, pixels.clone());
    let got = codes(&doc, &la(json!({"radius": r})));
    let today = encoded(&scene_map(&pixels, (w, h, r), DEFAULTS, None));
    let floor_pixels: Vec<usize> = (2 * w..w * h).filter(|i| i % w < w / 2).collect();
    let mean_chroma = |codes: &dyn Fn(usize) -> [i64; 3]| {
        floor_pixels
            .iter()
            .map(|&i| code_chroma(codes(i)))
            .sum::<f64>()
            / floor_pixels.len() as f64
    };
    let (now, before) = (
        mean_chroma(&|i| [0, 1, 2].map(|c| i64::from(got[i][c]))),
        mean_chroma(&|i| today[i]),
    );
    assert!(before > 0.5, "today's map makes the floor blue: {before}");
    assert!(
        now < 0.1 * before,
        "the floor lifts as a neutral: chroma {now} against today's {before}"
    );
    let orange = 10 * w + 40;
    for ch in 0..3 {
        within(
            i64::from(got[orange][ch]),
            today[orange][ch],
            257,
            &format!("the orange ch {ch}"),
        );
    }
    matches_over_floor(&doc, &pixels, (w, h, r), floor);
}

#[test]
fn a_lone_pixel_below_zero_sets_no_floor() {
    let (w, h, r) = (192usize, 128usize, 2usize);
    let mut seed = 0x6d2b_79f5_u32;
    let mut next = || {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        f64::from(seed % 10_000) / 10_000.0
    };
    let mut pixels: Vec<[f32; 4]> = (0..w * h)
        .map(|_| {
            let y = (-7.0 + 7.5 * next()).exp2();
            [y * (0.6 + 0.8 * next()), y, y * (0.5 + next()), 1.0].map(|v| v as f32)
        })
        .collect();
    for pixel in pixels.iter_mut().take(32) {
        *pixel = [0.0, 0.0, 0.0, 1.0];
    }
    // A stray far below black and a blue whose luminance is below zero: two
    // pixels in a picture of 24576 are no floor.
    let (stray, below) = (5000, 9000);
    pixels[stray] = [-0.5, -0.5, -0.5, 1.0];
    pixels[below] = [-0.02, -0.01, 0.03, 1.0];
    assert_eq!(black_floor(&pixels), 0.0);
    let doc = scene(w as u32, h as u32, pixels.clone());
    let got = codes(&doc, &la(json!({"radius": r})));
    let today = encoded(&scene_map(&pixels, (w, h, r), DEFAULTS, None));
    assert!(
        today[below][2] > today[below][0] + 10_000,
        "today's map makes it blue: {:?}",
        today[below]
    );
    let lifted = [0, 1, 2].map(|c| i64::from(got[below][c]));
    assert!(
        lifted.iter().max().unwrap() - lifted.iter().min().unwrap() <= 1,
        "a pixel with no light lifts as a neutral: {lifted:?}"
    );
    matches_over_floor(&doc, &pixels, (w, h, r), 0.0);
}

#[test]
fn a_colour_outside_the_gamut_near_the_top_sets_no_floor() {
    // A green outside the gamut (red below zero), as a field and as a gradient
    // over four stops: every pixel of the darkest stop has a channel below
    // zero, but that stop lies within a few stops of the top, so it is no
    // floor and every pixel keeps its colour.
    let (w, h, r) = (32usize, 24usize, 2usize);
    let colour = [-0.05f32, 0.60, 0.02];
    let field = vec![[colour[0], colour[1], colour[2], 1.0]; w * h];
    let gradient: Vec<[f32; 4]> = (0..w * h)
        .map(|i| {
            let s = (-4.0 + 4.0 * (i % w) as f32 / (w - 1) as f32).exp2();
            [colour[0] * s, colour[1] * s, colour[2] * s, 1.0]
        })
        .collect();
    for (label, pixels) in [("the field", field), ("the gradient", gradient)] {
        assert_eq!(black_floor(&pixels), 0.0, "{label} sets no floor");
        let doc = scene(w as u32, h as u32, pixels.clone());
        let got = codes(&doc, &la(json!({"radius": r})));
        let today = encoded(&scene_map(&pixels, (w, h, r), DEFAULTS, None));
        for (i, pixel) in got.iter().enumerate() {
            let now = [0, 1, 2].map(|c| i64::from(pixel[c]));
            assert!(
                today[i][1] > today[i][0] + 10_000,
                "{label} is green today at {i}: {:?}",
                today[i]
            );
            assert!(
                code_chroma(now) > 0.9 * code_chroma(today[i]),
                "{label} keeps its colour at {i}: {now:?} against {:?}",
                today[i]
            );
        }
        matches_over_floor(&doc, &pixels, (w, h, r), 0.0);
    }
}

#[test]
fn a_floor_six_and_a_half_stops_under_the_top_is_kept() {
    // The noise at black of `near_black_noise_lifts_as_a_neutral` beside a
    // neutral field 6.5 stops over the floor it sets, a dim capture's range:
    // the top clears the 7κF (about 5.8 stops) a floor needs.
    let (w, h, r) = (64usize, 48usize, 2usize);
    let mut normal = Normal(0x9e37_79b9);
    let noise = |i: usize| i % w < w / 2;
    let mut pixels: Vec<[f32; 4]> = (0..w * h)
        .map(|i| {
            if noise(i) {
                let [r, g, b] = [1.5e-4, 1.0e-4, 4.0e-4].map(|s| (s * normal.next()) as f32);
                [r, g, b, 1.0]
            } else {
                [0.4, 0.4, 0.4, 1.0]
            }
        })
        .collect();
    let floor = black_floor(&pixels);
    assert!(floor > 0.0, "the darkest stop is the floor");
    let top = (floor * 6.5f64.exp2()) as f32;
    for (i, pixel) in pixels.iter_mut().enumerate() {
        if !noise(i) {
            *pixel = [top, top, top, 1.0];
        }
    }
    assert_eq!(
        black_floor(&pixels),
        floor,
        "the floor is kept under the top"
    );
    let doc = scene(w as u32, h as u32, pixels.clone());
    let got = codes(&doc, &la(json!({"radius": r})));
    let today = encoded(&scene_map(&pixels, (w, h, r), DEFAULTS, None));
    let floor_pixels: Vec<usize> = (0..w * h).filter(|&i| noise(i)).collect();
    let mean_chroma = |codes: &dyn Fn(usize) -> [i64; 3]| {
        floor_pixels
            .iter()
            .map(|&i| code_chroma(codes(i)))
            .sum::<f64>()
            / floor_pixels.len() as f64
    };
    let (now, before) = (
        mean_chroma(&|i| [0, 1, 2].map(|c| i64::from(got[i][c]))),
        mean_chroma(&|i| today[i]),
    );
    assert!(before > 0.5, "today's map colours the floor: {before}");
    assert!(
        now < 0.1 * before,
        "the floor lifts as a neutral: chroma {now} against today's {before}"
    );
    matches_over_floor(&doc, &pixels, (w, h, r), floor);
}

#[test]
fn the_toning_curve_is_the_curves_spline() {
    let (w, h) = (16u32, 4u32);
    let ramp: Vec<[f32; 4]> = (0..w * h)
        .map(|i| {
            let v = (-5.0 + 5.0 * (i % w) as f32 / (w - 1) as f32).exp2();
            [v, v, v, 1.0]
        })
        .collect();
    let doc = scene(w, h, ramp);
    let none = codes(&doc, &la(json!({})));
    for identity in [
        json!([[0, 0], [255, 255]]),
        json!([[0, 0], [128, 128], [255, 255]]),
    ] {
        assert_eq!(
            codes(&doc, &la(json!({ "toning_curve": identity }))),
            none,
            "an identity curve is no curve"
        );
    }
    // Detail 0 cancels the base split (module doc, step 4 with k = 1), so the
    // output follows t′ alone and an inverting curve reverses the ramp.
    let plain = codes(&doc, &la(json!({"detail": 0})));
    let inverted = codes(
        &doc,
        &la(json!({"detail": 0, "toning_curve": [[0, 255], [255, 0]]})),
    );
    for x in 1..w as usize {
        assert!(plain[x][0] > plain[x - 1][0], "the ramp rises at {x}");
        assert!(
            inverted[x][0] < inverted[x - 1][0],
            "the curve reverses it at {x}"
        );
    }
}

#[test]
fn saturation_holds_neutrals_and_shadow_lifts_the_dark() {
    let (w, h) = (32u32, 8u32);
    let doc = scene(w, h, two_level(w, h, 2f32.powi(-7), 1.5));
    assert_eq!(
        codes(&doc, &la(json!({"saturation": 100}))),
        codes(&doc, &la(json!({"saturation": 0}))),
        "saturation leaves a neutral unchanged"
    );
    let off = codes(&doc, &la(json!({})));
    let lifted = codes(&doc, &la(json!({"shadow": 100})));
    assert!(
        lifted[0][0] > off[0][0] + 1000,
        "{} against {}",
        lifted[0][0],
        off[0][0]
    );
}

#[test]
fn the_histogram_counts_the_visible_weight() {
    let (w, h) = (16u32, 8u32);
    let mut pixels = two_level(w, h, 2f32.powi(-4), 3.0);
    pixels[3][3] = 0.5;
    pixels[4][3] = 0.0;
    pixels[40] = [0.3, 0.2, 0.1, 0.25];
    let weight: f64 = pixels.iter().map(|p| f64::from(p[3])).sum();
    let reply = histogram_with(&scene(w, h, pixels), None).unwrap();
    let bins: Vec<f64> = reply["bins"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_f64().unwrap())
        .collect();
    assert_eq!(bins.len(), 256);
    assert!((bins.iter().sum::<f64>() - weight).abs() < 1e-6);
    let two = histogram_with(&scene(w, h, two_level(w, h, 2f32.powi(-4), 3.0)), None).unwrap();
    let bins: Vec<f64> = two["bins"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_f64().unwrap())
        .collect();
    let half = f64::from(w * h / 2);
    assert_eq!((bins[0], bins[255]), (half, half));
    assert_eq!(bins[1..255].iter().sum::<f64>(), 0.0);
    let (low, high) = (
        two["low_stops"].as_f64().unwrap(),
        two["high_stops"].as_f64().unwrap(),
    );
    assert!(
        (low + 4.0).abs() < 1e-4 && (high - 3f64.log2()).abs() < 1e-4,
        "{two}"
    );
}

#[test]
fn every_value_refuses_by_name() {
    let doc = scene(8, 4, vec![[0.5, 0.4, 0.3, 1.0]; 32]);
    let refused = |request: Value| toned_with(&doc, &request, None).err().expect("a refusal");
    for (key, value) in [
        ("radius", json!(0)),
        ("radius", json!(501)),
        ("strength", json!(0.05)),
        ("strength", json!(5)),
        ("gamma", json!(0.05)),
        ("gamma", json!(2.5)),
        ("exposure", json!(-6)),
        ("exposure", json!(5.5)),
        ("detail", json!(-101)),
        ("detail", json!(400)),
        ("shadow", json!(101)),
        ("highlight", json!(-101)),
        ("vibrance", json!(101)),
        ("saturation", json!(-101)),
        ("smooth_edges", json!("yes")),
        ("toning_curve", json!([[0, 0]])),
        ("toning_curve", json!([[0, 300], [255, 0]])),
        (
            "toning_curve",
            json!((0..17).map(|i| [i, i]).collect::<Vec<_>>()),
        ),
    ] {
        let request = la(Value::Object(Map::from_iter([(
            key.to_string(),
            value.clone(),
        )])));
        let message = refused(request);
        assert!(message.contains(key), "{key} {value}: {message}");
    }
    assert_eq!(
        refused(la(json!({"amount": 1}))),
        "\"amount\" is not an argument of hdr toning's local_adaptation method"
    );
    assert_eq!(
        refused(json!({"method": "exposure_gamma", "detail": 3})),
        "\"detail\" is not an argument of hdr toning's exposure_gamma method"
    );
    // Every key at an end of its range is taken.
    codes(
        &doc,
        &la(json!({
            "radius": 500, "strength": 4, "smooth_edges": true, "gamma": 0.1,
            "exposure": -5, "detail": -100, "shadow": -100, "highlight": 100,
            "vibrance": -100, "saturation": 100, "toning_curve": [[0, 30], [255, 220]],
        })),
    );
}

#[test]
fn the_local_plan_is_above_exposure_and_gamma() {
    let doc = scene(256, 256, vec![[0.5, 0.4, 0.3, 1.0]; 256 * 256]);
    let local = plan_bytes(&doc, &la(json!({})));
    let global = plan_bytes(&doc, &json!({"method": "exposure_gamma"}));
    assert!(local > global, "{local} against {global}");
}

type Run = Result<(), String>;

#[test]
fn a_cancelled_control_answers_operation_cancelled() {
    let doc = scene(16, 8, two_level(16, 8, 0.01, 2.0));
    let tone = |control: &DrJobControl| toned_with(&doc, &la(json!({})), Some(control)).map(|_| ());
    let histogram = |control: &DrJobControl| histogram_with(&doc, Some(control)).map(|_| ());
    let runs: [&dyn Fn(&DrJobControl) -> Run; 2] = [&tone, &histogram];
    for run in runs {
        let control = DrJobControl::default();
        control.cancel();
        assert_eq!(run(&control).expect_err("a refusal"), "operation cancelled");
        run(&DrJobControl::default()).unwrap();
    }
}

#[test]
fn the_histogram_export_tolerates_null() {
    let doc = scene(4, 4, vec![[0.5, 0.4, 0.3, 1.0]; 16]);
    unsafe {
        let mut error = ptr::null_mut();
        let out =
            dr_precision_doc_hdr_toning_histogram(ptr::null(), LIMIT, 0, ptr::null(), &mut error);
        assert!(out.is_null());
        assert!(taken(error).is_some());
        let out = dr_precision_doc_hdr_toning_histogram(
            ptr::null(),
            LIMIT,
            0,
            ptr::null(),
            ptr::null_mut(),
        );
        assert!(out.is_null());
        let out =
            dr_precision_doc_hdr_toning_histogram(&doc, LIMIT, 0, ptr::null(), ptr::null_mut());
        assert!(taken(out).is_some());
    }
}
