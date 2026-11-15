//! A RAW local's tone and detail controls on a 32-bit float (linear)
//! development, through `dr_precision_doc_develop_locals`: rule L of
//! `designs/v1-float-editing.md`, each control in its stated domain.
//!
//! Every oracle is written here or in `tests/common/`: the document
//! profile's curve and the logarithm above white restated
//! (`display_encoding_reference`), a 16-bit twin of the same tone built from
//! the same codes, f64 restatements of rule H and of the Levels stage, and
//! the float route's own selected edits (`presence`, `reduce_noise`,
//! `smart_sharpen`, `blur`) and adjustment (`vibrance`) on the same float
//! document, which are the filters and adjustment rule L names. Bounds,
//! declared before the first run and never widened:
//!
//! - EXPOSURE. On stored values, nothing clamped: +1 and −2 stops are `v·2^e`
//!   bit for bit, 0.7 stops within 1e-6 relative of `v·2^0.7`; at density
//!   0.6 the linear mix `b + 0.6·(t − b)` within `2e-6·max(1, |ref|)`.
//! - TWIN. A float base built from 16-bit codes (`v = EOTF(c/65535)`) against
//!   the 16-bit development of the same tone, full coverage: every component
//!   whose 16-bit result is not 0 or 65535 has `|E(out) − c16/65535| ≤
//!   4/65535` for one control, `≤ 8/65535` for the chain. Exposure and
//!   Saturation have no twin: each depth's local runs that depth's
//!   adjustment, and the two differ inside 0…1 as the adjustments do.
//!   Exposure at 16 bits is a gain in each channel's γ domain
//!   (`adjust_calibrated`'s rule Γ), at float `v·2^e`; Saturation's master
//!   at 16 bits is bound to the document's sampled TRC, at float the unbound
//!   sRGB-curve one (rule V). EXPOSURE and SATURATION below are their
//!   oracles. (Both were in the twin on the first run: Exposure 0.7 missed it
//!   by about 600 codes in the sRGB toe, Saturation 50 by 4.5 codes.)
//! - CONTRAST. A component above 1 or below 0 is kept bit for bit.
//! - H. On flat bands (2, 2, 2), (0.5, 1.5, 0.2), 0.18 grey and (1.5, 0.3,
//!   0.3), probes 8 px from a band edge: `|E(out) − rule| ≤ 8/65535`, the
//!   rule restated in f64 (Color 0.2 at and above white too). On a
//!   saturated ramp crossing encoded luma 1 under Highlights 100, no step
//!   between neighbours is more than 4 × the largest input step.
//! - WHITES AND BLACKS. `D(levels(E(v)))` restated with no clamp, within
//!   `1e-4·max(1, |ref|)`.
//! - SATURATION. The float `vibrance` adjustment on the same document within
//!   `1e-6·max(1, |ref|)`; `out(4p) = 4·out(p)` for `max p = 1` within 1e-5
//!   relative.
//! - FILTERS. Clarity, Texture, Noise Reduction and Sharpness ±60 against the
//!   float selected edit on the same document within `1e-6·max(1, |ref|)`
//!   (density 1) and the linear mix at density 0.6 within `2e-6·max(1,
//!   |ref|)`; Clarity then Sharpness against the two edits in turn within
//!   `2e-5·max(1, |ref|)`; Saturation then Clarity against the `vibrance`
//!   adjustment then the `presence` edit within `2e-6·max(1, |ref|)`, and
//!   Noise Reduction then Sharpness on the twin plate against the two edits
//!   in turn within `2e-5·max(1, |ref|)` (both added after the first run,
//!   each bound declared before its own first run: the first when Saturation
//!   left TWIN, the second when the chain's TWIN found Sharpness reading the
//!   source's visible range rather than the denoised plane's, 247 codes).
//! - STRUCTURE. An identity tone is NULL; a pixel the mask does not weigh
//!   keeps its bits; two overlapping Exposure +1 locals are `4v` and a
//!   camera variant with Exposure +1 is twice the variant, bit for bit;
//!   every output is finite with alpha kept. A camera variant with Clarity 60
//!   is the `presence` edit of the variant within `1e-6·max(1, |ref|)`
//!   (added at the build's self-review, its bound declared before its first
//!   run).
use crate::display_encoding_reference::{decode, encode, eotf};
use crate::float_fixture::float_doc;
use crate::raw_local_numeric::{image, mask_edit, rgba, success, Mask};
use crate::raw_local_tone::try_develop;
use doctored_core::ffi_precision_color::dr_precision_doc_apply_adjustment;
use doctored_core::icc::IccProfile;
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::REFERENCE_WORKING_BYTES;
use doctored_core::selected_adjustment::SelectedEdit;
use serde_json::{json, Value};
use std::ffi::CString;
use std::ptr;

const W: u32 = 96;
const H: u32 = 64;
const LIMIT: u64 = REFERENCE_WORKING_BYTES;
/// TWIN's bound for one control, in encoded units.
const TWIN: f64 = 4.0 / 65535.0;

// ---- plumbing -----------------------------------------------------------------

fn floats(document: &PrecisionDocument) -> Vec<[f32; 4]> {
    document.layer_source(0).unwrap().to_rgba32().unwrap()
}

fn bits(pixels: &[[f32; 4]]) -> Vec<[u32; 4]> {
    pixels.iter().map(|p| p.map(f32::to_bits)).collect()
}

fn local(tone: Value, density: f64) -> Value {
    json!({"enabled":true,"density":density,"feather":0,"tone":tone})
}

fn full(w: u32, h: u32) -> Mask {
    mask_edit(None, w, h, json!({"op":"full"})).unwrap()
}

/// The float development's samples (the base's when it answered NULL);
/// every one finite with its alpha kept.
fn developed(
    base: &PrecisionDocument,
    variants: &[Option<&PrecisionDocument>],
    masks: &[&Mask],
    settings: Value,
) -> Vec<[f32; 4]> {
    let out = try_develop(base, variants, masks, settings.clone())
        .unwrap_or_else(|e| panic!("{settings}: develop refused: {e}"))
        .map_or_else(|| floats(base), |d| floats(&d));
    for (i, (p, b)) in out.iter().zip(floats(base)).enumerate() {
        assert!(
            p.iter().all(|v| v.is_finite()),
            "{settings} pixel {i}: {p:?}"
        );
        assert_eq!(
            p[3].to_bits(),
            b[3].to_bits(),
            "{settings} pixel {i}: alpha"
        );
    }
    out
}

/// `|actual − expected| ≤ bound·max(1, |expected|)` on every RGB component.
fn assert_relative(actual: &[[f32; 4]], expected: &[[f64; 3]], bound: f64, label: &str) {
    assert_eq!(actual.len(), expected.len());
    for (i, (a, e)) in actual.iter().zip(expected).enumerate() {
        for c in 0..3 {
            let error = (f64::from(a[c]) - e[c]).abs();
            assert!(
                error <= bound * e[c].abs().max(1.0),
                "{label} pixel {i} channel {c}: {} expected {}",
                a[c],
                e[c]
            );
        }
    }
}

fn rgb64(pixels: &[[f32; 4]]) -> Vec<[f64; 3]> {
    pixels
        .iter()
        .map(|p| [p[0], p[1], p[2]].map(f64::from))
        .collect()
}

/// `b + t·(o − b)` per component, in f64.
fn mixed(base: &[[f32; 4]], op: &[[f32; 4]], t: f64) -> Vec<[f64; 3]> {
    base.iter()
        .zip(op)
        .map(|(b, o)| {
            std::array::from_fn(|c| f64::from(b[c]) + t * (f64::from(o[c]) - f64::from(b[c])))
        })
        .collect()
}

/// The float route's selected edit `op` on the whole layer of `document`.
fn edited(document: &PrecisionDocument, op: &str, params: Value) -> Vec<[f32; 4]> {
    let edit =
        SelectedEdit::parse(&json!({"layer":0,"op":op,"params":params}).to_string()).unwrap();
    let out = document
        .selected_edit(&edit, &[], LIMIT, 0, None)
        .unwrap_or_else(|e| panic!("{op} at float: {e}"))
        .unwrap_or_else(|| panic!("{op} at float: no change"));
    floats(&out)
}

/// The float adjustment `description` on `document`'s only layer.
fn adjusted(document: &PrecisionDocument, description: Value) -> Vec<[f32; 4]> {
    let text = CString::new(description.to_string()).unwrap();
    let mut error = ptr::null_mut();
    let output = unsafe {
        dr_precision_doc_apply_adjustment(
            document,
            0,
            text.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut error,
        )
    };
    success(error);
    assert!(!output.is_null(), "{description}: no change");
    let document = unsafe { Box::from_raw(output) };
    floats(&document)
}

// ---- fixtures -----------------------------------------------------------------

/// An opaque 16-bit plate inside 1500…64000: a ramp across, texture, a
/// colour cast by row and column and a dark and a bright band, so every
/// control has something to move.
fn twin_codes() -> Vec<[u16; 4]> {
    (0..W * H)
        .map(|i| {
            let (x, y) = (f64::from(i % W), f64::from(i / W));
            let ramp = 0.05 + 0.9 * x / f64::from(W - 1);
            let texture = 0.04 * (x * 1.3).sin() * (y * 0.9).sin();
            let band = if y < 8.0 {
                -0.2
            } else if y >= 56.0 {
                0.2
            } else {
                0.0
            };
            let t = ramp + texture + band;
            let code = |v: f64| (v.clamp(0.025, 0.975) * 65535.0).round() as u16;
            [
                code(t + 0.05 * (y / 5.0).sin()),
                code(t),
                code(t - 0.06 * (x / 9.0).cos()),
                65535,
            ]
        })
        .collect()
}

/// The float twin of `codes`: each code's linear light.
fn float_twin(codes: &[[u16; 4]]) -> PrecisionDocument {
    let pixels = codes
        .iter()
        .map(|p| {
            let c = |k: usize| eotf(f64::from(p[k]) / 65535.0) as f32;
            [c(0), c(1), c(2), 1.0]
        })
        .collect();
    float_doc(W, H, &IccProfile::srgb(), &[pixels])
}

/// An opaque scene-linear plate: a quadratic ramp from 0.01 to 6 across,
/// texture, a colour cast, a band whose blue is below zero and a few specks
/// of 40.
fn bright_pixels() -> Vec<[f32; 4]> {
    (0..W * H)
        .map(|i| {
            let (x, y) = (f64::from(i % W), f64::from(i / W));
            let t = x / f64::from(W - 1);
            let v = 0.01 + 6.0 * t * t;
            let texture = 1.0 + 0.08 * (x * 1.7).sin() * (y * 1.1).cos();
            let v = if i % 211 == 17 { 40.0 } else { v * texture };
            let blue = if (24..32).contains(&(i / W)) {
                -0.05 + 0.1 * v
            } else {
                0.8 * v
            };
            [v as f32, (0.9 * v) as f32, blue as f32, 1.0]
        })
        .collect()
}

fn bright() -> PrecisionDocument {
    float_doc(W, H, &IccProfile::srgb(), &[bright_pixels()])
}

// ---- EXPOSURE -------------------------------------------------------------------

#[test]
fn exposure_is_a_gain_on_the_stored_values_and_mixes_in_linear_light() {
    let base = bright();
    let original = floats(&base);
    let mask = full(W, H);
    let mut above = 0;
    for stops in [1.0_f64, -2.0] {
        let out = developed(
            &base,
            &[None],
            &[&mask],
            json!([local(json!({"exposure":stops}), 1.0)]),
        );
        let want: Vec<[f32; 4]> = original
            .iter()
            .map(|p| {
                let g = stops.exp2() as f32;
                [p[0] * g, p[1] * g, p[2] * g, p[3]]
            })
            .collect();
        assert_eq!(bits(&out), bits(&want), "exposure {stops}");
        above += out.iter().filter(|p| p[0] > 1.0).count();
    }
    assert!(above > 100, "nothing above white: {above}");
    let out = developed(
        &base,
        &[None],
        &[&mask],
        json!([local(json!({"exposure":0.7}), 1.0)]),
    );
    let gain = 0.7_f64.exp2();
    let want: Vec<[f64; 3]> = rgb64(&original)
        .iter()
        .map(|p| p.map(|v| v * gain))
        .collect();
    assert_relative(&out, &want, 1e-6, "exposure 0.7");
    let partial = developed(
        &base,
        &[None],
        &[&mask],
        json!([local(json!({"exposure":0.7}), 0.6)]),
    );
    assert_relative(
        &partial,
        &mixed(&original, &out, 0.6),
        2e-6,
        "exposure 0.7 at 0.6",
    );
    // A negative component stays below zero: nothing is clamped.
    assert!(partial.iter().any(|p| p[2] < 0.0));
}

// ---- TWIN -------------------------------------------------------------------------

/// The float development of `tone` encoded against its 16-bit twin.
fn against_twin(tone: Value, bound: f64) {
    let codes = twin_codes();
    let sixteen = image(W, H, &codes, &IccProfile::srgb());
    let float = float_twin(&codes);
    let mask = full(W, H);
    let settings = json!([local(tone.clone(), 1.0)]);
    let twin = try_develop(&sixteen, &[None], &[&mask], settings.clone())
        .unwrap()
        .map_or_else(|| codes.clone(), |d| rgba(&d));
    assert!(twin != codes, "{tone}: the 16-bit twin moved nothing");
    let got = developed(&float, &[None], &[&mask], settings);
    let mut compared = 0;
    for (i, (g, c16)) in got.iter().zip(&twin).enumerate() {
        for c in 0..3 {
            if c16[c] == 0 || c16[c] == 65535 {
                continue;
            }
            let (e, want) = (encode(f64::from(g[c])), f64::from(c16[c]) / 65535.0);
            assert!(
                (e - want).abs() <= bound,
                "{tone} pixel {i} channel {c}: E(out) {e}, 16-bit {want}"
            );
            compared += 1;
        }
    }
    assert!(compared > got.len(), "{tone}: only {compared} compared");
}

#[test]
fn inside_white_each_control_matches_its_16_bit_twin() {
    for tone in [
        json!({"contrast":40}),
        json!({"contrast":-30}),
        json!({"highlights":60,"shadows":50}),
        json!({"whites":40,"blacks":-50}),
        json!({"whites":-40,"blacks":30}),
        json!({"clarity":60}),
        json!({"texture":70}),
        json!({"noise_reduction":60}),
        json!({"sharpness":60}),
    ] {
        against_twin(tone, TWIN);
    }
    // The chain keeps every stage before the last inside 0…1 at 16 bits
    // (Whites and Blacks on the output side): where a 16-bit intermediate
    // clips and the float one does not, a later neighbourhood stage reads
    // two different pictures. (With Whites +20 the chain missed by 14 codes
    // beside such a clip on its first run.)
    against_twin(
        json!({"contrast":35,"highlights":30,"whites":-20,"blacks":20,"clarity":30,
               "noise_reduction":40,"sharpness":30}),
        2.0 * TWIN,
    );
}

// ---- CONTRAST ---------------------------------------------------------------------

#[test]
fn contrast_keeps_a_component_above_white_or_below_zero() {
    let base = bright();
    let original = floats(&base);
    let mask = full(W, H);
    let out = developed(
        &base,
        &[None],
        &[&mask],
        json!([local(json!({"contrast":50}), 1.0)]),
    );
    let (mut kept, mut moved) = (0, 0);
    for (i, (o, b)) in out.iter().zip(&original).enumerate() {
        for c in 0..3 {
            if b[c] > 1.0 || b[c] < 0.0 {
                assert_eq!(o[c].to_bits(), b[c].to_bits(), "pixel {i} channel {c}");
                kept += 1;
            } else {
                moved += usize::from(o[c].to_bits() != b[c].to_bits());
            }
        }
    }
    assert!(kept > 500 && moved > 500, "{kept} kept, {moved} moved");
}

// ---- H ------------------------------------------------------------------------------

const BH: u32 = 96;
/// The Highlights and Shadows bands, top to bottom.
const TONE_BANDS: [[f32; 3]; 4] = [
    [2.0, 2.0, 2.0],
    [0.5, 1.5, 0.2],
    [0.18, 0.18, 0.18],
    [1.5, 0.3, 0.3],
];

/// The band of row `y` among the equal bands of a `BH`-row plate, and whether
/// the row is at least `margin` rows from the band's edges.
fn band_row(y: u32, margin: u32) -> (usize, bool) {
    let bands = TONE_BANDS.len() as u32;
    let b = y * bands / BH;
    let first = (0..BH).find(|r| r * bands / BH == b).unwrap();
    let last = (0..BH).rev().find(|r| r * bands / BH == b).unwrap();
    (b as usize, y >= first + margin && y + margin <= last)
}

/// The Rec. 709 luma of an encoded triple.
fn luma(e: [f64; 3]) -> f64 {
    0.2126 * e[0] + 0.7152 * e[1] + 0.0722 * e[2]
}

/// Rule H at a flat-region pixel with code `code`, restated in f64: the
/// local's mapping (both bands at tone 0.5, Color 0.2, no midtone contrast),
/// the guide the region's clamped luma. At or above white the luma is held
/// and Color scales the chroma about it.
fn rule_h(code: [f64; 3], highlights: f64, shadows: f64) -> [f64; 3] {
    let (s, h) = (shadows / 100.0, highlights / 100.0);
    let l = luma(code);
    let b = l.clamp(0.0, 1.0);
    let w_s = (1.0 - b / 0.5).clamp(0.0, 1.0).powi(2);
    let w_h = (1.0 - (1.0 - b) / 0.5).clamp(0.0, 1.0).powi(2);
    let boost = 1.0 + 0.2 * (s * w_s + h * w_h).clamp(0.0, 1.0);
    if l >= 1.0 {
        return code.map(|c| l + (c - l) * boost);
    }
    let e = (1.0 + h * w_h) / (1.0 + s * w_s);
    let lifted = if l <= 0.0 { 0.0 } else { l.powf(e) };
    let k = if l > 0.0 { lifted / l } else { 1.0 };
    code.map(|c| lifted + (c * k - lifted) * boost)
}

#[test]
fn highlights_and_shadows_follow_rule_h_on_both_sides_of_white() {
    let pixels: Vec<[f32; 4]> = (0..W * BH)
        .map(|i| {
            let p = TONE_BANDS[band_row(i / W, 0).0];
            [p[0], p[1], p[2], 1.0]
        })
        .collect();
    let base = float_doc(W, BH, &IccProfile::srgb(), std::slice::from_ref(&pixels));
    let mask = full(W, BH);
    for (highlights, shadows) in [(60.0_f64, 50.0_f64), (100.0, 0.0), (0.0, 100.0)] {
        let tone = json!({"highlights":highlights,"shadows":shadows});
        let out = developed(&base, &[None], &[&mask], json!([local(tone.clone(), 1.0)]));
        let mut probes = [0; 4];
        for (i, (got, was)) in out.iter().zip(&pixels).enumerate() {
            let (x, y) = (i as u32 % W, i as u32 / W);
            let (band, inside) = band_row(y, 8);
            if !inside || x < 8 || x + 8 >= W {
                continue;
            }
            let want = rule_h(
                [was[0], was[1], was[2]].map(|v| encode(f64::from(v))),
                highlights,
                shadows,
            );
            for c in 0..3 {
                let e = encode(f64::from(got[c]));
                assert!(
                    (e - want[c]).abs() <= 2.0 * TWIN,
                    "{tone} pixel {i} channel {c}: E(out) {e}, rule {}",
                    want[c]
                );
            }
            probes[band] += 1;
        }
        assert!(probes.iter().all(|n| *n > 100), "{tone}: probes {probes:?}");
    }
}

#[test]
fn highlights_with_color_are_continuous_where_the_encoded_luma_crosses_white() {
    // Each row the same ramp of one saturated colour, its encoded luma from
    // 0.85 to 1.15, so the guide reads the ramp's own neighbourhood.
    let unit = [1.6_f64, 0.9, 0.3];
    let scale = 1.0 / luma(unit);
    let codes: Vec<[f64; 3]> = (0..W)
        .map(|x| {
            let s = (0.85 + 0.3 * f64::from(x) / f64::from(W - 1)) * scale;
            unit.map(|c| c * s)
        })
        .collect();
    let rows = 24;
    let pixels: Vec<[f32; 4]> = (0..W * rows)
        .map(|i| {
            let c = codes[(i % W) as usize];
            [
                decode(c[0]) as f32,
                decode(c[1]) as f32,
                decode(c[2]) as f32,
                1.0,
            ]
        })
        .collect();
    let base = float_doc(W, rows, &IccProfile::srgb(), std::slice::from_ref(&pixels));
    let mask = full(W, rows);
    let out = developed(
        &base,
        &[None],
        &[&mask],
        json!([local(json!({"highlights":100}), 1.0)]),
    );
    let row = (rows / 2 * W) as usize;
    let encoded = |p: [f32; 4]| [p[0], p[1], p[2]].map(|v| encode(f64::from(v)));
    let (mut crossed, mut largest) = (false, 0.0_f64);
    for x in 8..(W - 9) as usize {
        let (a, b) = (encoded(pixels[row + x]), encoded(pixels[row + x + 1]));
        let step_in = (0..3).map(|c| (b[c] - a[c]).abs()).fold(0.0, f64::max);
        let (p, q) = (encoded(out[row + x]), encoded(out[row + x + 1]));
        for c in 0..3 {
            let step_out = (q[c] - p[c]).abs();
            largest = largest.max(step_out / step_in);
            assert!(
                step_out <= 4.0 * step_in,
                "x {x} channel {c}: output step {step_out}, input step {step_in}"
            );
        }
        crossed |= luma(a) < 1.0 && luma(b) >= 1.0;
        assert!(p != a, "x {x}: Highlights moved nothing");
    }
    assert!(
        crossed,
        "the probed ramp never crossed luma 1 (largest ratio {largest})"
    );
}

// ---- WHITES AND BLACKS -----------------------------------------------------------------

#[test]
fn whites_and_blacks_continue_the_levels_line_past_both_ends() {
    let base = bright();
    let original = floats(&base);
    let mask = full(W, H);
    // (whites, blacks) → (black, white, output black, output white).
    for (whites, blacks, stage) in [
        (60.0, -60.0, [0.15, 0.85, 0.0, 1.0]),
        (-40.0, 30.0, [0.0, 1.0, 0.075, 0.9]),
    ] {
        let tone = json!({"whites":whites,"blacks":blacks});
        let out = developed(&base, &[None], &[&mask], json!([local(tone.clone(), 1.0)]));
        let [black, white, out_black, out_white] = stage;
        let want: Vec<[f64; 3]> = rgb64(&original)
            .iter()
            .map(|p| {
                p.map(|v| {
                    let t = (encode(v) - black) / (white - black);
                    decode(out_black + (out_white - out_black) * t)
                })
            })
            .collect();
        assert_relative(&out, &want, 1e-4, &tone.to_string());
        assert!(out.iter().any(|p| p[0] > 6.0) && out.iter().any(|p| p[2] < 0.0));
    }
}

// ---- SATURATION --------------------------------------------------------------------------

#[test]
fn saturation_is_the_float_vibrance_and_homogeneous_above_white() {
    let base = bright();
    let mask = full(W, H);
    for amount in [50.0_f64, -60.0] {
        let out = developed(
            &base,
            &[None],
            &[&mask],
            json!([local(json!({"saturation":amount}), 1.0)]),
        );
        let want = adjusted(
            &base,
            json!({"type":"adjust","evaluation":"precision_v1","op":"vibrance",
                   "params":{"saturation":amount / 100.0}}),
        );
        assert_relative(&out, &rgb64(&want), 1e-6, &format!("saturation {amount}"));
    }
    // Pairs p and 4p with max p = 1.
    let unit: Vec<[f32; 4]> = (0..W * H)
        .map(|i| {
            let t = (i % W) as f32 / W as f32;
            let p = [1.0, 0.2 + 0.7 * t, 0.9 - 0.8 * t];
            let k = if (i / W).is_multiple_of(2) { 1.0 } else { 4.0 };
            [p[0] * k, p[1] * k, p[2] * k, 1.0]
        })
        .collect();
    let paired = float_doc(W, H, &IccProfile::srgb(), &[unit]);
    let out = developed(
        &paired,
        &[None],
        &[&mask],
        json!([local(json!({"saturation":70}), 1.0)]),
    );
    for i in 0..(W * H / 2) as usize {
        let (y, x) = (i / W as usize, i % W as usize);
        let (one, four) = (
            out[2 * y * W as usize + x],
            out[(2 * y + 1) * W as usize + x],
        );
        for c in 0..3 {
            let want = 4.0 * f64::from(one[c]);
            assert!(
                (f64::from(four[c]) - want).abs() <= 1e-5 * want.abs().max(1.0),
                "pixel {i} channel {c}: {} against 4 × {}",
                four[c],
                one[c]
            );
        }
    }
}

// ---- FILTERS --------------------------------------------------------------------------------

/// Each plane control beside the float filter rule L names for it.
fn filter_cases() -> [(Value, &'static str, Value); 6] {
    [
        (json!({"clarity":60}), "presence", json!({"clarity":60})),
        (json!({"texture":-50}), "presence", json!({"texture":-50})),
        (
            json!({"noise_reduction":60}),
            "reduce_noise",
            json!({"radius":0.5,"luminance":0.06,"detail":0,"color":0}),
        ),
        (
            json!({"sharpness":60}),
            "smart_sharpen",
            json!({"amount":90,"radius":1,"noise":0,"remove":"gaussian"}),
        ),
        (json!({"sharpness":-60}), "blur", json!({"sigma":1.8})),
        (json!({"sharpness":-100}), "blur", json!({"sigma":3})),
    ]
}

#[test]
fn plane_controls_are_their_float_filters_times_coverage() {
    let base = bright();
    let original = floats(&base);
    let mask = full(W, H);
    for (tone, op, params) in filter_cases() {
        let filtered = edited(&base, op, params);
        let out = developed(&base, &[None], &[&mask], json!([local(tone.clone(), 1.0)]));
        assert_relative(&out, &rgb64(&filtered), 1e-6, &tone.to_string());
        let partial = developed(&base, &[None], &[&mask], json!([local(tone.clone(), 0.6)]));
        assert_relative(
            &partial,
            &mixed(&original, &filtered, 0.6),
            2e-6,
            &format!("{tone} at 0.6"),
        );
    }
}

#[test]
fn staged_controls_run_in_the_rule_l_order() {
    let base = bright();
    let mask = full(W, H);
    // Sharpness reads the Presence result.
    let out = developed(
        &base,
        &[None],
        &[&mask],
        json!([local(json!({"clarity":40,"sharpness":60}), 1.0)]),
    );
    let [_, _, _, (_, op, params), ..] = filter_cases();
    let presence = edited(&base, "presence", json!({"clarity":40}));
    let staged = float_doc(W, H, &IccProfile::srgb(), &[presence]);
    let want = edited(&staged, op, params);
    assert_relative(&out, &rgb64(&want), 2e-5, "clarity, then sharpness");
    // Saturation runs on the decoded linear value before Presence, which
    // reads the encoded copy of its result.
    let out = developed(
        &base,
        &[None],
        &[&mask],
        json!([local(json!({"saturation":-40,"clarity":30}), 1.0)]),
    );
    let saturated = adjusted(
        &base,
        json!({"type":"adjust","evaluation":"precision_v1","op":"vibrance",
               "params":{"saturation":-0.4}}),
    );
    let staged = float_doc(W, H, &IccProfile::srgb(), &[saturated]);
    let want = edited(&staged, "presence", json!({"clarity":30}));
    assert_relative(&out, &rgb64(&want), 2e-6, "saturation, then clarity");
    // Sharpness reads the Noise Reduction result, at that result's own
    // visible range: on the twin plate Reduce Noise takes a blue below the
    // plate's darkest, which a low-pass held to the source's range would
    // sharpen into the wrong side of it.
    let plate = float_twin(&twin_codes());
    let out = developed(
        &plate,
        &[None],
        &[&mask],
        json!([local(json!({"noise_reduction":40,"sharpness":30}), 1.0)]),
    );
    let denoised = edited(
        &plate,
        "reduce_noise",
        json!({"radius":0.5,"luminance":0.04,"detail":0,"color":0}),
    );
    let darkest = floats(&plate).iter().map(|p| p[2]).fold(f32::MAX, f32::min);
    assert!(
        denoised.iter().any(|d| d[2] < darkest),
        "Reduce Noise took no blue below the plate's darkest {darkest}"
    );
    let staged = float_doc(W, H, &IccProfile::srgb(), &[denoised]);
    let want = edited(
        &staged,
        "smart_sharpen",
        json!({"amount":45,"radius":1,"noise":0,"remove":"gaussian"}),
    );
    assert_relative(&out, &rgb64(&want), 2e-5, "noise reduction, then sharpness");
}

// ---- STRUCTURE ----------------------------------------------------------------------------

#[test]
fn identity_coverage_overlap_and_variants_keep_their_contracts() {
    let base = bright();
    let original = floats(&base);
    let mask = full(W, H);
    // The identity tone is the exact no-op.
    for settings in [
        json!([local(json!({}), 1.0)]),
        json!([local(json!({"exposure":0,"clarity":0,"sharpness":0}), 1.0)]),
        json!([local(json!({"contrast":80}), 0.0)]),
    ] {
        assert!(
            try_develop(&base, &[None], &[&mask], settings.clone())
                .unwrap()
                .is_none(),
            "{settings}: not the exact no-op"
        );
    }
    // A pixel outside a radial keeps its bits.
    let radial = mask_edit(
        None,
        W,
        H,
        json!({"op":"radial","center":[48,32],"radius":[20,14],"hardness":0.5}),
    )
    .unwrap();
    let out = developed(
        &base,
        &[None],
        &[&radial],
        json!([local(
            json!({"contrast":60,"clarity":50,"shadows":40,"sharpness":-40}),
            1.0
        )]),
    );
    let (mut outside, mut moved) = (0, 0);
    for (i, (o, b)) in out.iter().zip(&original).enumerate() {
        let (x, y) = (f64::from(i as u32 % W) + 0.5, f64::from(i as u32 / W) + 0.5);
        if ((x - 48.0) / 20.0).powi(2) + ((y - 32.0) / 14.0).powi(2) > 1.1 {
            assert_eq!(
                o.map(f32::to_bits),
                b.map(f32::to_bits),
                "pixel {i} outside moved"
            );
            outside += 1;
        } else {
            moved += usize::from(o != b);
        }
    }
    assert!(
        outside > 3000 && moved > 500,
        "{outside} outside, {moved} moved"
    );
    // Two overlapping tone-only locals compound: the second reads the first.
    let other = full(W, H);
    let twice = developed(
        &base,
        &[None, None],
        &[&mask, &other],
        json!([
            local(json!({"exposure":1}), 1.0),
            local(json!({"exposure":1}), 1.0)
        ]),
    );
    let quadrupled: Vec<[f32; 4]> = original
        .iter()
        .map(|p| [p[0] * 4.0, p[1] * 4.0, p[2] * 4.0, p[3]])
        .collect();
    assert_eq!(bits(&twice), bits(&quadrupled), "two Exposure +1 locals");
    // A camera variant with a tone takes the tone of the variant.
    let shifted: Vec<[f32; 4]> = original
        .iter()
        .map(|p| [p[0] * 1.25, p[1], p[2] * 0.75 - 0.01, 1.0])
        .collect();
    let variant = float_doc(W, H, &IccProfile::srgb(), std::slice::from_ref(&shifted));
    let out = developed(
        &base,
        &[Some(&variant)],
        &[&mask],
        json!([local(json!({"exposure":1}), 1.0)]),
    );
    let doubled: Vec<[f32; 4]> = shifted
        .iter()
        .map(|p| [p[0] * 2.0, p[1] * 2.0, p[2] * 2.0, p[3]])
        .collect();
    assert_eq!(bits(&out), bits(&doubled), "the tone of the variant");
    // A plane tone is built over the variant's samples, not the base's.
    let out = developed(
        &base,
        &[Some(&variant)],
        &[&mask],
        json!([local(json!({"clarity":60}), 1.0)]),
    );
    let want = edited(&variant, "presence", json!({"clarity":60}));
    assert_relative(&out, &rgb64(&want), 1e-6, "clarity of the variant");
}
