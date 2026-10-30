//! Hue/Saturation on a CMYK document's ink (#1), all through the FFI,
//! against an oracle written here: an f64 Hue/Saturation written from
//! `adjust_hue_saturation`'s module-doc formulas (R0, and Colorize's HSL
//! replace), run as `cmyk_adjust`'s module doc says Photoshop runs it on
//! ink — on `1 − C`, `1 − M`, `1 − Y` with K kept EXACTLY, or under
//! Colorize on the naive `(1 − ink)(1 − K)` RGB with K the result's grey
//! component and C, M, Y its full-GCR remainder. Bound: 2/65535 per ink on
//! C, M and Y; K bit-exact outside Colorize; a pixel the kernel leaves
//! unchanged keeps its ink bit-exact. No profile is read: a profile with no
//! `B2A` takes the edit, and the plans charge no memo.

use std::ffi::{CStr, CString};
use std::path::Path;
use std::ptr;

use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_cmyk::dr_precision_doc_cmyk_sample;
use doctored_core::ffi_insert::dr_precision_doc_add_adjustment_at;
use doctored_core::ffi_precision::{
    dr_precision_doc_free, dr_precision_doc_info, dr_precision_doc_open, dr_precision_doc_render,
    dr_precision_doc_save_native, dr_precision_image_free,
};
use doctored_core::ffi_precision_color::{
    dr_precision_doc_add_adjustment, dr_precision_doc_adjustment_plan,
    dr_precision_doc_apply_adjustment, dr_precision_doc_set_adjustment,
};
use doctored_core::ffi_precision_inspect::dr_precision_doc_adjustment_description;
use doctored_core::ffi_regional::dr_precision_doc_render_region;
use doctored_core::ffi_selected_adjustment::dr_precision_doc_selected_edit;
use doctored_core::precision_document::PrecisionDocument;
use serde_json::Value;
use tempfile::TempDir;

use crate::cmyk_fixture::{press_profile, press_profile_without_b2a};
use crate::cmyk_structure_fixture::then;
use crate::cmyk_tiff_builder::cmyk_tiff;
use crate::common::{cpath, take_err_string};

const LIMIT: u64 = 2 * 1024 * 1024 * 1024;

fn free(document: *mut PrecisionDocument) {
    unsafe { dr_precision_doc_free(document) };
}

fn json(text: *mut std::ffi::c_char, err: *mut std::ffi::c_char) -> Value {
    assert!(!text.is_null(), "{}", take_err_string(err));
    let value = serde_json::from_str(unsafe { CStr::from_ptr(text) }.to_str().unwrap()).unwrap();
    unsafe { dr_string_free(text) };
    value
}

fn description(params: &str) -> String {
    format!(
        r#"{{"type":"adjust","evaluation":"precision_v1","op":"hue_saturation","params":{params}}}"#
    )
}

fn code16(v: f64) -> u16 {
    (v.clamp(0.0, 1.0) * 65535.0).round() as u16
}

// ------------------------------------------------------------- documents --

/// The test inks as fractions: K-only greys, chromatic patches (reds, a
/// blue, greens, a cyan, a yellow, mixed), and a rich black.
const GREYS: [[f64; 4]; 5] = [
    [0.0, 0.0, 0.0, 0.1],
    [0.0, 0.0, 0.0, 0.3],
    [0.0, 0.0, 0.0, 0.5],
    [0.0, 0.0, 0.0, 0.7],
    [0.0, 0.0, 0.0, 0.9],
];
const CHROMATIC: [[f64; 4]; 11] = [
    [0.1, 0.8, 0.7, 0.0],
    [0.1, 0.7, 0.9, 0.1],
    [0.05, 0.6, 0.3, 0.05],
    [0.7, 0.6, 0.05, 0.0],
    [0.9, 0.1, 0.9, 0.05],
    [0.8, 0.1, 0.1, 0.0],
    [0.05, 0.1, 0.85, 0.0],
    [0.3, 0.75, 0.1, 0.2],
    [0.6, 0.2, 0.4, 0.3],
    [0.2, 0.5, 0.6, 0.1],
    [0.45, 0.3, 0.2, 0.15],
];
const RICH_BLACK: [f64; 4] = [0.6, 0.5, 0.5, 0.9];

/// Inks with no red: cyan, blue (C+M) and green (C+Y) only.
const NO_REDS: [[f64; 4]; 4] = [
    [0.8, 0.0, 0.0, 0.0],
    [0.7, 0.6, 0.05, 0.0],
    [0.9, 0.1, 0.9, 0.05],
    [0.5, 0.45, 0.0, 0.2],
];

/// `inks` as stored codes at the declared depth: an 8-bit file stores
/// `round(255 v) × 257`, a 16-bit one `round(65535 v)` nudged off the
/// 8-bit grid so the two depths read different codes.
fn codes(inks: &[[f64; 4]], bits: u16) -> Vec<[u16; 4]> {
    inks.iter()
        .map(|ink| {
            ink.map(|v| match bits {
                8 => (v * 255.0).round() as u16 * 257,
                _ => code16(v).saturating_add(if v > 0.0 { 77 } else { 0 }),
            })
        })
        .collect()
}

/// A CMYK document from a TIFF of `samples` (row-major, `w` wide) at
/// `bits`, carrying `icc`.
fn cmyk_doc(
    dir: &TempDir,
    w: u32,
    samples: &[[u16; 4]],
    bits: u16,
    icc: &[u8],
) -> *mut PrecisionDocument {
    let h = samples.len() as u32 / w;
    let data: Vec<u8> = match bits {
        8 => samples.iter().flatten().map(|v| (v / 257) as u8).collect(),
        _ => samples
            .iter()
            .flatten()
            .flat_map(|v| v.to_le_bytes())
            .collect(),
    };
    let path = dir
        .path()
        .join(format!("ink-{bits}-{w}x{h}-{}.tif", samples.len()));
    std::fs::write(&path, cmyk_tiff(w, h, bits, 4, 0, &data, Some(icc))).unwrap();
    open(&path)
}

fn open(path: &Path) -> *mut PrecisionDocument {
    let c = cpath(path);
    let mut err = ptr::null_mut();
    let doc = unsafe { dr_precision_doc_open(c.as_ptr(), LIMIT, 0, ptr::null(), &mut err) };
    assert!(!doc.is_null(), "open {path:?}: {}", take_err_string(err));
    doc
}

fn samples(doc: *const PrecisionDocument, index: usize) -> Vec<[u16; 5]> {
    unsafe { &*doc }
        .cmyk_layer_samples(index)
        .expect("a CMYK record")
}

fn apply(
    doc: *const PrecisionDocument,
    index: usize,
    desc: &str,
) -> Result<*mut PrecisionDocument, String> {
    let text = CString::new(desc).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_apply_adjustment(
            doc,
            index,
            text.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    if out.is_null() && !err.is_null() {
        Err(take_err_string(err))
    } else {
        Ok(out)
    }
}

fn add(
    doc: *const PrecisionDocument,
    desc: &str,
    opacity: f32,
) -> Result<*mut PrecisionDocument, String> {
    let text = CString::new(desc).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_add_adjustment(
            doc,
            c"Hue/Saturation".as_ptr(),
            text.as_ptr(),
            opacity,
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    if out.is_null() {
        Err(take_err_string(err))
    } else {
        Ok(out)
    }
}

fn sample_ink(doc: *const PrecisionDocument, x: i32, y: i32) -> [u16; 4] {
    let mut err = ptr::null_mut();
    let value = json(
        unsafe { dr_precision_doc_cmyk_sample(doc, x, y, 0, LIMIT, 0, &mut err) },
        err,
    );
    let ink: Vec<u16> = value["ink16"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_u64().unwrap() as u16)
        .collect();
    [ink[0], ink[1], ink[2], ink[3]]
}

fn render(doc: *const PrecisionDocument) -> Vec<[u16; 4]> {
    let mut err = ptr::null_mut();
    let image = unsafe { dr_precision_doc_render(doc, LIMIT, 0, ptr::null(), &mut err) };
    assert!(!image.is_null(), "{}", take_err_string(err));
    let pixels = unsafe { &*image }.to_rgba16().unwrap();
    unsafe { dr_precision_image_free(image) };
    pixels
}

// ---------------------------------------------------------------- oracle --

/// One band as the oracle reads it, in degrees and slider units.
#[derive(Clone, Copy)]
struct Band {
    center: f64,
    hue: f64,
    saturation: f64,
    lightness: f64,
}

/// The f64 Hue/Saturation of `adjust_hue_saturation`'s module doc.
#[derive(Clone, Copy, Default)]
struct Oracle {
    hue: f64,
    saturation: f64,
    lightness: f64,
    band: Option<Band>,
    colorize: Option<[f64; 3]>,
}

const WHEEL: f64 = 1536.0;

/// Lm (step 1, Colorize's lightness): the two-sided curve.
fn two_sided(c: f64, l: f64) -> f64 {
    if l >= 0.0 {
        c + (1.0 - c) * l
    } else {
        c * (1.0 + l)
    }
}

/// The hexcone hue of `v` in degrees, [0, 360).
fn hue_of(v: [f64; 3]) -> f64 {
    let big = v[0].max(v[1]).max(v[2]);
    let small = v[0].min(v[1]).min(v[2]);
    let d = big - small;
    let h = if big == v[0] {
        ((v[1] - v[2]) / d).rem_euclid(6.0)
    } else if big == v[1] {
        (v[2] - v[0]) / d + 2.0
    } else {
        (v[0] - v[1]) / d + 4.0
    };
    (60.0 * h).rem_euclid(360.0)
}

/// The triple of mid `(max + min)/2` = `mid`, chroma `max − min` = `c` and
/// hue `h` degrees.
fn hexcone(mid: f64, c: f64, h: f64) -> [f64; 3] {
    let hp = h.rem_euclid(360.0) / 60.0;
    let x = c * (1.0 - (hp % 2.0 - 1.0).abs());
    let (r, g, b) = match hp.floor() as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = mid - c / 2.0;
    [r + m, g + m, b + m]
}

/// `q(d)`: a shift on the 1/24576-turn grid, truncated toward zero.
fn grid_hue(d: f64) -> f64 {
    (d * 24576.0 / 360.0).trunc() * 360.0 / 24576.0
}

/// `H(d)`: a handle on the 1536-unit wheel.
fn handle(d: f64) -> f64 {
    ((d.rem_euclid(360.0) * WHEEL / 359.0 + 0.5).floor()).rem_euclid(WHEEL)
}

/// A band's weight at wheel position `t`: up from A to B, 1 to C, down to D.
fn weight(t: f64, center: f64) -> f64 {
    let [a, b, c, d] = [center - 45.0, center - 15.0, center + 15.0, center + 45.0].map(handle);
    let t = (t - a).rem_euclid(WHEEL);
    let (tb, tc, td) = (
        (b - a).rem_euclid(WHEEL),
        (c - a).rem_euclid(WHEEL),
        (d - a).rem_euclid(WHEEL),
    );
    if t <= tb {
        t / tb
    } else if t <= tc {
        1.0
    } else if t <= td {
        (td - t) / (td - tc)
    } else {
        0.0
    }
}

/// `G = 256 K` for a slider in [−1, 1] (Photoshop units `s = 100 v`).
fn grid_gain(v: f64) -> f64 {
    let s = (v * 100.0).round();
    let km = if s <= 0.0 {
        1.0 + s / 100.0
    } else {
        255.0 / (255.0 - (254.0 * s / 100.0).floor())
    };
    256.0 + (256.0 * km - 256.0).trunc()
}

impl Oracle {
    fn apply(&self, rgb: [f64; 3]) -> [f64; 3] {
        if let Some([h, s, l]) = self.colorize {
            let big = rgb[0].max(rgb[1]).max(rgb[2]);
            let small = rgb[0].min(rgb[1]).min(rgb[2]);
            let lightness = two_sided((big + small) / 2.0, l);
            let c = (1.0 - (2.0 * lightness - 1.0).abs()) * s;
            return hexcone(lightness, c, h);
        }
        let mut v = rgb;
        if self.lightness != 0.0 {
            v = v.map(|c| two_sided(c, self.lightness));
        }
        let big = v[0].max(v[1]).max(v[2]);
        let small = v[0].min(v[1]).min(v[2]);
        if big == small {
            return v;
        }
        let h = hue_of(v);
        let p = h * WHEEL / 360.0;
        let w = self.band.map_or(0.0, |b| weight(p, b.center));
        let shift = grid_hue(self.hue) + self.band.map_or(0.0, |b| w * grid_hue(b.hue));
        let lb = self.band.map_or(0.0, |b| w * b.lightness);
        if shift != 0.0 {
            v = hexcone((big + small) / 2.0, big - small, h + shift);
        }
        if lb != 0.0 {
            let big = v[0].max(v[1]).max(v[2]);
            let small = v[0].min(v[1]).min(v[2]);
            v = if lb > 0.0 {
                v.map(|c| c + (big - c) * lb)
            } else {
                v.map(|c| c + (c - small) * lb)
            };
        }
        let g0 = grid_gain(self.saturation);
        let k = match self.band.filter(|b| b.saturation != 0.0) {
            Some(b) => {
                let delta = (g0 * (grid_gain(b.saturation) - 256.0) / 256.0).round();
                let table =
                    |k: f64| (g0 + (weight(k, b.center) * delta).round()).clamp(0.0, 32767.0);
                let k0 = p.floor().min(WHEEL - 1.0);
                let f = p - k0;
                let k1 = if k0 + 1.0 >= WHEEL { 0.0 } else { k0 + 1.0 };
                (table(k0) + f * (table(k1) - table(k0))) / 256.0
            }
            None => g0.min(32767.0) / 256.0,
        };
        if k != 1.0 {
            let big = v[0].max(v[1]).max(v[2]);
            let small = v[0].min(v[1]).min(v[2]);
            if big != small {
                let mid = (big + small) / 2.0;
                let mut k = k;
                for c in v {
                    if c > mid {
                        k = k.min((1.0 - mid) / (c - mid));
                    } else if c < mid {
                        k = k.min(-mid / (c - mid));
                    }
                }
                v = v.map(|c| mid + (c - mid) * k);
            }
        }
        v.map(|c| c.clamp(0.0, 1.0))
    }

    /// The expected ink of `ink` (stored codes) in f64 (module doc), and
    /// whether the kernel leaves its RGB exactly where it was (kept).
    fn ink(&self, ink: [u16; 4]) -> ([f64; 4], bool) {
        let [c, m, y, k] = ink.map(|v| f64::from(v) / 65535.0);
        if self.colorize.is_none() {
            let rgb = [1.0 - c, 1.0 - m, 1.0 - y];
            let out = self.apply(rgb);
            return ([1.0 - out[0], 1.0 - out[1], 1.0 - out[2], k], out == rgb);
        }
        let paper = 1.0 - k;
        let out = self.apply([(1.0 - c) * paper, (1.0 - m) * paper, (1.0 - y) * paper]);
        let k_out = (1.0 - out[0].max(out[1]).max(out[2])).clamp(0.0, 1.0);
        let plate = |v: f64| {
            if k_out >= 1.0 {
                0.0
            } else {
                ((1.0 - v - k_out) / (1.0 - k_out)).clamp(0.0, 1.0)
            }
        };
        ([plate(out[0]), plate(out[1]), plate(out[2]), k_out], false)
    }
}

/// Asserts `got` (the result's samples) against the oracle over `before`;
/// answers how many pixels were kept and how many moved.
fn check(oracle: &Oracle, before: &[[u16; 5]], got: &[[u16; 5]], what: &str) -> (usize, usize) {
    let (mut kept, mut moved) = (0, 0);
    for (i, (s, g)) in before.iter().zip(got).enumerate() {
        let ink = [s[0], s[1], s[2], s[3]];
        let (want, unchanged) = oracle.ink(ink);
        if unchanged {
            assert_eq!(s, g, "{what}: pixel {i} is kept bit-exact");
            kept += 1;
            continue;
        }
        for c in 0..3 {
            let want = code16(want[c]);
            assert!(
                g[c].abs_diff(want) <= 2,
                "{what}: pixel {i} ink {c}: {} vs {want} (input {ink:?})",
                g[c]
            );
        }
        if oracle.colorize.is_none() {
            assert_eq!(g[3], s[3], "{what}: pixel {i} keeps K exactly");
        } else {
            let want = code16(want[3]);
            assert!(
                g[3].abs_diff(want) <= 2,
                "{what}: pixel {i} K {} vs {want}",
                g[3]
            );
        }
        assert_eq!(g[4], s[4], "{what}: alpha is kept");
        moved += 1;
    }
    (kept, moved)
}

fn picture() -> Vec<[f64; 4]> {
    GREYS
        .iter()
        .chain(CHROMATIC.iter())
        .chain(std::iter::once(&RICH_BLACK))
        .copied()
        .collect()
}

// ----------------------------------------------------------------- tests --

#[test]
fn master_bands_and_colorize_hold_k_at_8_and_16_bits() {
    let dir = TempDir::new().unwrap();
    let profile = press_profile("Press Hue");
    let reds = Band {
        center: 0.0,
        hue: 30.0,
        saturation: 0.25,
        lightness: -0.15,
    };
    let cases: [(&str, Oracle); 8] = [
        (
            r#"{"hue":40}"#,
            Oracle {
                hue: 40.0,
                ..Oracle::default()
            },
        ),
        (
            r#"{"hue":-90}"#,
            Oracle {
                hue: -90.0,
                ..Oracle::default()
            },
        ),
        (
            r#"{"saturation":0.5}"#,
            Oracle {
                saturation: 0.5,
                ..Oracle::default()
            },
        ),
        (
            r#"{"saturation":-0.6}"#,
            Oracle {
                saturation: -0.6,
                ..Oracle::default()
            },
        ),
        (
            r#"{"lightness":0.4}"#,
            Oracle {
                lightness: 0.4,
                ..Oracle::default()
            },
        ),
        (
            r#"{"bands":{"reds":{"hue":30,"saturation":0.25,"lightness":-0.15}}}"#,
            Oracle {
                band: Some(reds),
                ..Oracle::default()
            },
        ),
        (
            r#"{"colorize":true,"colorize_hue":200,"colorize_saturation":0.4,"colorize_lightness":0}"#,
            Oracle {
                colorize: Some([200.0, 0.4, 0.0]),
                ..Oracle::default()
            },
        ),
        (
            r#"{"colorize":true,"colorize_hue":30,"colorize_saturation":0.6,"colorize_lightness":0.25}"#,
            Oracle {
                colorize: Some([30.0, 0.6, 0.25]),
                ..Oracle::default()
            },
        ),
    ];
    let inks = picture();
    for (params, oracle) in cases {
        for bits in [8u16, 16] {
            let doc = cmyk_doc(&dir, inks.len() as u32, &codes(&inks, bits), bits, &profile);
            let before = samples(doc, 0);
            let out = apply(doc, 0, &description(params)).unwrap();
            assert!(!out.is_null(), "{params} at {bits} bits changes the layer");
            let what = format!("{params} at {bits} bits");
            let after = samples(out, 0);
            let (kept, moved) = check(&oracle, &before, &after, &what);
            assert!(moved > 0, "{what}: {kept} kept, {moved} moved");
            if oracle.colorize.is_none() && oracle.lightness == 0.0 {
                // A K-only grey has no hue and no band reaches it: it stays
                // K-only, its ink untouched.
                for g in 0..GREYS.len() {
                    assert_eq!(after[g], before[g], "{what}: K-only grey {g} is kept");
                }
            }
            free(out);
            free(doc);
        }
    }
}

#[test]
fn lightness_moves_k_only_greys_onto_c_m_and_y_never_k() {
    let dir = TempDir::new().unwrap();
    let profile = press_profile("Press Light");
    for bits in [8u16, 16] {
        let doc = cmyk_doc(
            &dir,
            GREYS.len() as u32,
            &codes(&GREYS, bits),
            bits,
            &profile,
        );
        let before = samples(doc, 0);
        let out = apply(doc, 0, &description(r#"{"lightness":-0.4}"#)).unwrap();
        let after = samples(out, 0);
        for (s, g) in before.iter().zip(&after) {
            assert_eq!(g[3], s[3], "K is held");
            // L − 40 on paper-white C, M, Y: 1 · 0.6, so 40 % of each.
            for c in 0..3 {
                assert!(g[c].abs_diff(code16(0.4)) <= 2, "{g:?}");
            }
        }
        free(out);
        free(doc);
    }
}

#[test]
fn the_reds_band_keeps_blues_and_a_layer_with_no_reds_bit_exact() {
    let dir = TempDir::new().unwrap();
    let profile = press_profile("Press Reds");
    let params = r#"{"bands":{"reds":{"hue":30,"saturation":0.25,"lightness":-0.15}}}"#;
    let oracle = Oracle {
        band: Some(Band {
            center: 0.0,
            hue: 30.0,
            saturation: 0.25,
            lightness: -0.15,
        }),
        ..Oracle::default()
    };
    let inks = picture();
    for bits in [8u16, 16] {
        let doc = cmyk_doc(&dir, inks.len() as u32, &codes(&inks, bits), bits, &profile);
        let before = samples(doc, 0);
        let out = apply(doc, 0, &description(params)).unwrap();
        assert!(!out.is_null());
        let (kept, moved) = check(&oracle, &before, &samples(out, 0), "reds");
        assert!(kept > 0 && moved > 0, "reds: {kept} kept, {moved} moved");
        // The blue patch (C+M, index 3 of CHROMATIC) is outside the band.
        let blue = GREYS.len() + 3;
        assert_eq!(samples(out, 0)[blue], before[blue], "the blue is kept");
        free(out);
        free(doc);
        let none = cmyk_doc(
            &dir,
            NO_REDS.len() as u32,
            &codes(&NO_REDS, bits),
            bits,
            &profile,
        );
        assert!(apply(none, 0, &description(params)).unwrap().is_null());
        free(none);
    }
}

#[test]
fn identity_settings_and_the_same_description_change_nothing() {
    let dir = TempDir::new().unwrap();
    let profile = press_profile("Press Identity");
    let inks = picture();
    let doc = cmyk_doc(&dir, inks.len() as u32, &codes(&inks, 16), 16, &profile);
    for params in ["{}", r#"{"hue":0}"#] {
        let text = CString::new(description(params)).unwrap();
        let mut err = ptr::null_mut();
        let out = unsafe {
            dr_precision_doc_apply_adjustment(
                doc,
                0,
                text.as_ptr(),
                LIMIT,
                0,
                ptr::null(),
                &mut err,
            )
        };
        assert!(
            out.is_null() && err.is_null(),
            "{params}: no change, no error"
        );
    }
    let desc = description(r#"{"hue":25}"#);
    let layered = add(doc, &desc, 1.0).unwrap();
    let text = CString::new(desc.as_str()).unwrap();
    let mut err = ptr::null_mut();
    let again = unsafe {
        dr_precision_doc_set_adjustment(layered, 1, text.as_ptr(), LIMIT, 0, ptr::null(), &mut err)
    };
    assert!(
        again.is_null() && err.is_null(),
        "the same description is a no-op"
    );
    free(layered);
    free(doc);
}

fn plan(doc: *const PrecisionDocument, index: i64, desc: &str, destructive: bool) -> u64 {
    let text = CString::new(desc).unwrap();
    let mut err = ptr::null_mut();
    let plan = json(
        unsafe {
            dr_precision_doc_adjustment_plan(doc, index, text.as_ptr(), destructive, &mut err)
        },
        err,
    );
    plan["scratch_bytes"].as_u64().unwrap()
}

#[test]
fn every_adjustment_export_takes_hue_saturation_and_no_plan_charges_a_memo() {
    let dir = TempDir::new().unwrap();
    let profile = press_profile("Press Exports");
    let inks = picture();
    let doc = cmyk_doc(&dir, inks.len() as u32, &codes(&inks, 16), 16, &profile);
    let desc = description(r#"{"hue":10}"#);
    // A Levels description of the same length: the two plans differ only by
    // what parsing their text holds, so Hue/Saturation charges nothing more.
    let levels =
        r#"{"type":"adjust","evaluation":"precision_v1","op":"levels","params":{"gamma":1.1}}"#;
    let pad = desc.len() as i64 - levels.len() as i64;
    for (index, destructive) in [(-1i64, false), (0, true)] {
        let hue = plan(doc, index, &desc, destructive);
        let level = plan(doc, index, levels, destructive);
        assert_eq!(hue as i64 - level as i64, 64 * pad, "{index} {destructive}");
    }
    let layered = add(doc, &desc, 1.0).unwrap();
    let mut err = ptr::null_mut();
    let info = json(unsafe { dr_precision_doc_info(layered, &mut err) }, err);
    assert_eq!(info["layers"][1]["kind"], "adjustment");
    assert_eq!(
        info["layers"][1]["adjustment"]["operation"],
        "hue_saturation"
    );
    let read = json(
        unsafe { dr_precision_doc_adjustment_description(layered, 1, &mut err) },
        err,
    );
    assert_eq!(read["op"], "hue_saturation");
    let text = CString::new(desc.as_str()).unwrap();
    let other = CString::new(description(r#"{"saturation":-0.3}"#)).unwrap();
    let set = unsafe {
        dr_precision_doc_set_adjustment(layered, 1, other.as_ptr(), LIMIT, 0, ptr::null(), &mut err)
    };
    assert!(!set.is_null(), "{}", take_err_string(err));
    let at = unsafe {
        dr_precision_doc_add_adjustment_at(
            doc,
            c"Below".as_ptr(),
            text.as_ptr(),
            1.0,
            0,
            0,
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!at.is_null(), "{}", take_err_string(err));
    let applied = apply(doc, 0, &desc).unwrap();
    assert!(!applied.is_null());
    let request = serde_json::json!({"layer":0,"op":"adjustment","description":desc}).to_string();
    let request = CString::new(request).unwrap();
    let selected = unsafe {
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
    assert!(!selected.is_null(), "{}", take_err_string(err));
    assert_eq!(samples(selected, 0), samples(applied, 0));
    for d in [layered, set, at, applied, selected, doc] {
        free(d);
    }
}

#[test]
fn a_record_renders_the_destructive_result_at_the_root_and_in_a_group() {
    let dir = TempDir::new().unwrap();
    let profile = press_profile("Press Record");
    let inks: Vec<[f64; 4]> = picture().into_iter().cycle().take(36).collect();
    let w = 9u32;
    let doc = cmyk_doc(&dir, w, &codes(&inks, 16), 16, &profile);
    let desc = description(r#"{"hue":-60,"saturation":0.2,"lightness":-0.1}"#);
    let before = samples(doc, 0);
    let destructive = apply(doc, 0, &desc).unwrap();
    let after = samples(destructive, 0);
    let full = add(doc, &desc, 1.0).unwrap();
    let half = add(doc, &desc, 0.5).unwrap();
    // The raster and the record inside a Normal group: the stack walks the
    // tree, so the group's composite is the record's result.
    let grouped = then(
        add(doc, &desc, 1.0).unwrap(),
        r#"{"op":"group_layers","layers":[0,1],"name":"Group","pass_through":false}"#,
        &[],
    );
    for (i, (s, d)) in before.iter().zip(&after).enumerate() {
        let (x, y) = ((i as u32 % w) as i32, (i as u32 / w) as i32);
        let want = [d[0], d[1], d[2], d[3]];
        assert_eq!(sample_ink(full, x, y), want, "({x},{y}) at opacity 1");
        assert_eq!(sample_ink(grouped, x, y), want, "({x},{y}) inside a group");
        let got = sample_ink(half, x, y);
        for c in 0..4 {
            let ink = f64::from(s[c]) / 65535.0;
            let adjusted = f64::from(d[c]) / 65535.0;
            // The record's 50 % at its byte, 128 / 255 (#2).
            let want = code16(ink + (adjusted - ink) * crate::common::byte_unit(0.5));
            assert!(
                got[c].abs_diff(want) <= 1,
                "({x},{y}) ink {c} at 0.5: {} vs {want}",
                got[c]
            );
        }
    }
    // A region render is the full render's window, sample for sample.
    let whole = render(full);
    let mut err = ptr::null_mut();
    let region = unsafe {
        dr_precision_doc_render_region(full, 2, 1, 5, 3, 1, LIMIT, 0, ptr::null(), &mut err)
    };
    assert!(!region.is_null(), "{}", take_err_string(err));
    let window = unsafe { &*region }.to_rgba16().unwrap();
    for y in 0..3usize {
        for x in 0..5usize {
            assert_eq!(window[y * 5 + x], whole[(y + 1) * w as usize + x + 2]);
        }
    }
    unsafe { dr_precision_image_free(region) };
    for d in [destructive, full, half, grouped, doc] {
        free(d);
    }
}

fn save(doc: *const PrecisionDocument, path: &Path) -> Vec<u8> {
    let c = cpath(path);
    let mut err = ptr::null_mut();
    assert!(
        unsafe { dr_precision_doc_save_native(doc, c.as_ptr(), LIMIT, 0, ptr::null(), &mut err) },
        "{}",
        take_err_string(err)
    );
    std::fs::read(path).unwrap()
}

#[test]
fn a_hue_saturation_record_reopens_bit_exact_at_word_83() {
    let dir = TempDir::new().unwrap();
    let profile = press_profile("Press Native");
    let inks: Vec<[f64; 4]> = picture().into_iter().cycle().take(24).collect();
    let doc = cmyk_doc(&dir, 6, &codes(&inks, 8), 8, &profile);
    let desc = description(
        r#"{"colorize":true,"colorize_hue":200,"colorize_saturation":0.4,"bands":{"reds":{"hue":20}}}"#,
    );
    let layered = add(doc, &desc, 0.8).unwrap();
    let path = dir.path().join("huesat.rz");
    let bytes = save(layered, &path);
    let word = u32::from_le_bytes(bytes[8..12].try_into().unwrap());
    assert_eq!(word, 83);
    let again = open(&path);
    let (a, b) = (unsafe { &*layered }, unsafe { &*again });
    assert_eq!(a.layer_count(), b.layer_count());
    for i in 0..a.layer_count() {
        assert_eq!(
            a.cmyk_layer_samples(i),
            b.cmyk_layer_samples(i),
            "record {i}"
        );
        assert_eq!(
            a.adjustment_description(i),
            b.adjustment_description(i),
            "record {i}"
        );
        assert_eq!(a.layer(i).unwrap().opacity(), b.layer(i).unwrap().opacity());
    }
    assert_eq!(
        render(layered),
        render(again),
        "the composite reopens bit-exact"
    );
    for d in [doc, layered, again] {
        free(d);
    }
}

#[test]
fn a_profile_with_no_b2a_takes_the_edit() {
    // The rule reads no profile, so a read-only profile no longer refuses.
    let dir = TempDir::new().unwrap();
    let profile = press_profile_without_b2a("Read Only");
    let inks = picture();
    let doc = cmyk_doc(&dir, inks.len() as u32, &codes(&inks, 16), 16, &profile);
    let desc = description(r#"{"hue":10}"#);
    for (index, destructive) in [(-1i64, false), (0, true)] {
        assert!(plan(doc, index, &desc, destructive) > 0);
    }
    let before = samples(doc, 0);
    let out = apply(doc, 0, &desc).unwrap();
    assert!(!out.is_null());
    let oracle = Oracle {
        hue: 10.0,
        ..Oracle::default()
    };
    check(&oracle, &before, &samples(out, 0), "a read-only profile");
    free(out);
    free(doc);
}
