//! #680 item 1: Develop grain on a 32-bit float RAW development, and the
//! float histogram the Develop window reads (item 4).
//!
//! Grain at float is the request `RawGrain.swift` sends (layer 0,
//! `add_grain` with amount, size, roughness and the camera file's seed),
//! through `dr_precision_doc_selected_edit`, which runs the one Add Grain
//! kernel on the layer's display encoding E and decodes the result back.
//! Doctored's rule, stated here and in the app: each pixel's encoded offset
//! δ is the 16-bit grain's δ for its encoded luma clamped to 0…1, added to
//! each channel's E(v) without the 16-bit 0…1 clamp.
//!
//! Every oracle is written here, read off no implementation. E is the
//! built-in sRGB profile's curve restated (its 1024-entry `curv` table of
//! IEC 61966-2.1, linearly interpolated and inverted on 0…1), continued above
//! white as `1 + s·ln v` with `s` the curve's slope at white (the table's
//! last segment, ≈ 1.055/2.4), odd below zero. The 16-bit twin holds
//! `round(65535·E(v))`. Bounds, declared before the first run and never
//! widened:
//!
//! - TWIN (a): every channel's `clamp(E(out), 0, 1)` is within 3/65535 of the
//!   twin's `out/65535` (twin source rounding 0.5 + output rounding 0.5 +
//!   the encoder's one interval + f32 slack), and some channel moved by more
//!   than that bound.
//! - WHITE (b): a pixel whose encoded luma is at or above 1 keeps its bits; a
//!   pixel with one channel above white and an encoded luma under 1 gets the
//!   same nonzero δ in all three channels within 1e-4.
//! - ENDS (c): near-white and near-black flat regions: every channel's
//!   encoded offset equals the green channel's within 1e-4, some |δ| > 0.01,
//!   and some pixel crosses white (resp. black) where the twin holds 65535
//!   (resp. 0).
//! - HISTOGRAM (d): the float histogram's above-white, exact-one,
//!   below-zero and exact-zero counts equal hand counts, and 4.0 lands in
//!   stop bin 199.
use std::ffi::{c_char, CStr, CString};
use std::ptr;

use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_scoped_statistics::dr_precision_image_scoped_analysis;
use doctored_core::ffi_selected_adjustment::dr_precision_doc_selected_edit;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use serde_json::{json, Value};

use crate::float_fixture::float_doc;

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
/// TWIN's bound, in encoded units.
const TWIN: f64 = 3.0 / 65535.0;
/// WHITE's and ENDS' bound on an encoded offset.
const OFFSET: f64 = 1e-4;
/// Rec. 709 luma weights, the kernel's luma over encoded values.
const LUMA: [f64; 3] = [0.2126, 0.7152, 0.0722];
/// A camera-file seed as `RawGrain` sends it: the digest's first 8 bytes as
/// a signed 64-bit integer.
const SEED: i64 = 0x1f2e_3d4c_5b6a_7988_u64 as i64;

// ---- the encoding, restated --------------------------------------------

/// The IEC 61966-2.1 electro-optical transfer function.
fn srgb(x: f64) -> f64 {
    if x <= 0.04045 {
        x / 12.92
    } else {
        ((x + 0.055) / 1.055).powf(2.4)
    }
}

/// The built-in sRGB profile's 1024 u16 `curv` samples, over 65535.
fn table() -> &'static [f64] {
    static TABLE: std::sync::OnceLock<Vec<f64>> = std::sync::OnceLock::new();
    TABLE.get_or_init(|| {
        (0..1024)
            .map(|i| (srgb(f64::from(i) / 1023.0) * 65535.0).round() / 65535.0)
            .collect()
    })
}

/// The profile's decode on 0…1: the table, linearly interpolated.
fn eotf(x: f64) -> f64 {
    let t = table();
    let pos = x.clamp(0.0, 1.0) * 1023.0;
    let i = (pos.floor() as usize).min(1022);
    let f = pos - i as f64;
    t[i] * (1.0 - f) + t[i + 1] * f
}

/// The inverse of [`eotf`] on 0…1: the segment holding `v`, inverted.
fn oetf(v: f64) -> f64 {
    let t = table();
    if v <= 0.0 {
        return 0.0;
    }
    let i = t[1..].partition_point(|x| *x < v).min(1022);
    (i as f64 + (v - t[i]) / (t[i + 1] - t[i])).min(1023.0) / 1023.0
}

/// The curve's slope at white, its last segment's `Δcode / Δlight`.
fn shoulder() -> f64 {
    let t = table();
    (1.0 / 1023.0) / (t[1023] - t[1022])
}

/// `E(v)`: the curve inverted to 1, `1 + s·ln v` above, odd.
fn encode(v: f64) -> f64 {
    let u = v.abs();
    let e = if u <= 1.0 {
        oetf(u)
    } else {
        1.0 + shoulder() * u.ln()
    };
    e.copysign(v)
}

/// The inverse of [`encode`].
fn decode(f: f64) -> f64 {
    let u = f.abs();
    let l = if u <= 1.0 {
        eotf(u)
    } else {
        ((u - 1.0) / shoulder()).exp()
    };
    l.copysign(f)
}

fn luma_encoded(p: [f32; 4]) -> f64 {
    (0..3).map(|c| LUMA[c] * encode(f64::from(p[c]))).sum()
}

// ---- the edit, as RawGrain sends it -------------------------------------

fn take(err: *mut c_char) -> String {
    if err.is_null() {
        return String::new();
    }
    let text = unsafe { CStr::from_ptr(err) }
        .to_string_lossy()
        .into_owned();
    unsafe { dr_string_free(err) };
    text
}

/// Develop grain on layer 0 of `doc`, which must change it.
fn grained(doc: &PrecisionDocument, amount: f64, size: f64, roughness: f64) -> PrecisionDocument {
    let request = CString::new(
        json!({"layer": 0, "op": "add_grain", "params": {
            "amount": amount, "size": size, "roughness": roughness, "seed": SEED}})
        .to_string(),
    )
    .unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_selected_edit(
            doc,
            request.as_ptr(),
            ptr::null(),
            0,
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    let error = take(err);
    assert!(!out.is_null(), "grain refused or unchanged: {error}");
    *unsafe { Box::from_raw(out) }
}

fn float_scene(w: u32, h: u32, pixels: Vec<[f32; 4]>) -> PrecisionDocument {
    let doc = float_doc(w, h, &IccProfile::srgb(), &[pixels]);
    assert!(doc.is_float());
    doc
}

/// The 16-bit twin: each sample's code `round(65535·E(v))`, clamped to the
/// code range, alpha `round(65535·a)`.
fn twin(w: u32, h: u32, pixels: &[[f32; 4]]) -> PrecisionDocument {
    let code = |v: f64| (v.clamp(0.0, 1.0) * 65535.0).round() as u16;
    let codes = pixels
        .iter()
        .map(|p| {
            [
                code(encode(f64::from(p[0]))),
                code(encode(f64::from(p[1]))),
                code(encode(f64::from(p[2]))),
                code(f64::from(p[3])),
            ]
        })
        .collect();
    PrecisionDocument::from_image(
        PrecisionImage::from_rgba16(w, h, codes, IccProfile::srgb(), None).unwrap(),
        Metadata::default(),
        Resolution::default(),
    )
    .unwrap()
}

fn floats(doc: &PrecisionDocument) -> Vec<[f32; 4]> {
    doc.layer_source(0).unwrap().to_rgba32().unwrap()
}

fn codes(doc: &PrecisionDocument) -> Vec<[u16; 4]> {
    doc.layer_source(0).unwrap().to_rgba16().unwrap()
}

// ---- (a) TWIN --------------------------------------------------------------

#[test]
fn grain_at_float_is_the_16_bit_grain_of_each_encoded_value() {
    let (w, h) = (48_u32, 32_u32);
    // Opaque SDR light from codes inside 1500…64000: ramps, a hard edge
    // and texture, so luma spans the midtones and both ends.
    let pixels: Vec<[f32; 4]> = (0..w * h)
        .map(|i| {
            let (x, y) = (i % w, i / w);
            let ramp = 1500 + x * 1100 + y * 700;
            let edge = if x > w / 2 { 17000 } else { 0 };
            let texture = ((x * 7 + y * 13) % 17) * 500;
            let v = |o: u32| {
                let code = (ramp + edge + texture + o) % 62500 + 1500;
                eotf(f64::from(code) / 65535.0) as f32
            };
            [v(0), v(9000), v(31000), 1.0]
        })
        .collect();
    let float = grained(&float_scene(w, h, pixels.clone()), 50.0, 1.5, 50.0);
    let sixteen = grained(&twin(w, h, &pixels), 50.0, 1.5, 50.0);
    let (got, want) = (floats(&float), codes(&sixteen));
    let mut moved = false;
    for (i, ((g, c16), p)) in got.iter().zip(&want).zip(&pixels).enumerate() {
        assert_eq!(g[3], 1.0, "pixel {i}: alpha kept");
        for c in 0..3 {
            let e = encode(f64::from(g[c]));
            let expected = f64::from(c16[c]) / 65535.0;
            assert!(
                (e.clamp(0.0, 1.0) - expected).abs() <= TWIN,
                "pixel {i} channel {c}: E(out) {e}, 16-bit {expected}"
            );
            moved |= (e - encode(f64::from(p[c]))).abs() > TWIN;
        }
    }
    assert!(moved, "the grain moved some channel past the bound");
}

// ---- (b) WHITE -------------------------------------------------------------

#[test]
fn white_holds_and_an_above_white_channel_takes_its_pixels_encoded_offset() {
    let (w, h) = (48_u32, 16_u32);
    let regions: [[f32; 4]; 3] = [
        [2.0, 2.0, 2.0, 1.0],
        // One channel far above white, encoded luma ≈ 1.18.
        [4.0, 1.2, 0.9, 1.0],
        // One channel above white, two inside, encoded luma ≈ 0.63.
        [1.5, 0.2, 0.2, 1.0],
    ];
    let pixels: Vec<[f32; 4]> = (0..w * h).map(|i| regions[(i % w / 16) as usize]).collect();
    assert!(luma_encoded(regions[0]) >= 1.0);
    assert!(luma_encoded(regions[1]) >= 1.0);
    assert!(luma_encoded(regions[2]) < 1.0);
    let out = floats(&grained(
        &float_scene(w, h, pixels.clone()),
        80.0,
        1.0,
        50.0,
    ));
    let mut nonzero = 0;
    for (i, (g, p)) in out.iter().zip(&pixels).enumerate() {
        if luma_encoded(*p) >= 1.0 {
            assert_eq!(g.map(f32::to_bits), p.map(f32::to_bits), "pixel {i}");
            continue;
        }
        assert_eq!(g[3], p[3]);
        let delta: Vec<f64> = (0..3)
            .map(|c| encode(f64::from(g[c])) - encode(f64::from(p[c])))
            .collect();
        for c in [0, 2] {
            assert!(
                (delta[c] - delta[1]).abs() <= OFFSET,
                "pixel {i}: offsets {delta:?}"
            );
        }
        if delta[1].abs() > 1e-3 {
            nonzero += 1;
        }
    }
    assert!(nonzero > 0, "the darker pixels took a nonzero offset");
}

// ---- (c) ENDS --------------------------------------------------------------

#[test]
fn grain_at_float_carries_a_channel_past_white_and_past_black_where_16_bits_clips() {
    let (w, h) = (64_u32, 32_u32);
    let left = [0.99, 0.30, 0.30].map(decode);
    let right = [0.005, 0.50, 0.50].map(decode);
    let pixels: Vec<[f32; 4]> = (0..w * h)
        .map(|i| {
            let e = if i % w < 32 { left } else { right };
            [e[0] as f32, e[1] as f32, e[2] as f32, 1.0]
        })
        .collect();
    let out = floats(&grained(
        &float_scene(w, h, pixels.clone()),
        100.0,
        1.0,
        50.0,
    ));
    let sixteen = codes(&grained(&twin(w, h, &pixels), 100.0, 1.0, 50.0));
    let (mut large, mut over_white, mut under_black) = (false, false, false);
    for (i, ((g, p), c16)) in out.iter().zip(&pixels).zip(&sixteen).enumerate() {
        let offset = |c: usize| encode(f64::from(g[c])) - encode(f64::from(p[c]));
        let delta = offset(1);
        large |= delta.abs() > 0.01;
        for c in [0, 2] {
            assert!(
                (offset(c) - delta).abs() <= OFFSET,
                "pixel {i} channel {c}: {} against δ {delta}",
                offset(c)
            );
        }
        if i as u32 % w < 32 {
            over_white |= encode(f64::from(g[0])) > 1.0 && g[0] > 1.0 && c16[0] == 65535;
        } else {
            under_black |= g[0] < 0.0 && c16[0] == 0;
        }
    }
    assert!(large, "some |δ| exceeds 0.01");
    assert!(
        over_white,
        "some near-white pixel crossed white where the twin holds 65535"
    );
    assert!(
        under_black,
        "some near-black pixel crossed black where the twin holds 0"
    );
}

// ---- (d) HISTOGRAM ---------------------------------------------------------

#[test]
fn the_float_histogram_counts_above_white_samples_against_hand_counts() {
    let (w, h) = (4_u32, 3_u32);
    let mut pixels = vec![[0.3_f32, 0.3, 0.3, 1.0]; 12];
    pixels[0] = [4.0, 0.5, 0.5, 1.0];
    pixels[1] = [1.0, 1.0, 0.2, 1.0];
    pixels[2] = [0.0, -0.5, 2.0, 1.0];
    // Alpha 0: its colour is not counted, its alpha is.
    pixels[3] = [8.0, 8.0, 8.0, 0.0];
    pixels[4] = [0.25, 1.5, -0.1, 0.5];
    let image =
        PrecisionImage::from_rgba32_extended(w, h, pixels, IccProfile::srgb(), None).unwrap();
    let request = CString::new(
        json!({"operation": "histogram", "source": "composite", "stride": 1}).to_string(),
    )
    .unwrap();
    let mut error = ptr::null_mut();
    let out = unsafe {
        dr_precision_image_scoped_analysis(
            &image,
            request.as_ptr(),
            ptr::null(),
            0,
            LIMIT,
            0,
            ptr::null(),
            &mut error,
        )
    };
    let message = take(error);
    assert!(!out.is_null(), "{message}");
    let report: Value = serde_json::from_str(&take(out)).unwrap();
    assert_eq!(report["storage"], "rgba32_float");
    // Hand counts, RGB over the eleven alpha-positive pixels, alpha over all
    // twelve: above white 4.0 (r), 1.5 (g), 2.0 (b); exactly one 1.0 (r, g)
    // and ten alphas; below zero −0.5 (g), −0.1 (b); exactly zero 0.0 (r)
    // and one alpha.
    assert_eq!(report["above_white_rgba"], json!([1, 1, 1, 0]));
    assert_eq!(report["exact_one_rgba"], json!([1, 1, 0, 10]));
    assert_eq!(report["below_zero_rgba"], json!([0, 1, 1, 0]));
    assert_eq!(report["exact_zero_rgba"], json!([1, 0, 0, 1]));
    // 4.0 is +2 EV: floor((2 + 12) / 18 · 256) = 199, the only red there.
    let unit = report["full_rgb_weight_units"].as_u64().unwrap()
        / report["full_rgb_pixel_count"].as_u64().unwrap();
    let red = &report["bin_weight_units"][0];
    assert_eq!(red[199].as_u64(), Some(unit));
    let red_total: u64 = red
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_u64().unwrap())
        .sum();
    assert_eq!(red_total, 11 * unit);
}
