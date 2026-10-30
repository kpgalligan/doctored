//! #2: Vibrance and Posterize as Photoshop 26.10.0 computes them, as far as
//! the declared families reach (`scripts/photoshop-probe/adjust_v1x_take.py`,
//! `adjust_v1x_round2.py` and `adjust_arith_578_take.py`; `adjust-parity-v1x`
//! takes 1-4 and `adjust-arith-578/take1`).
//!
//! Vibrance, each stage `c' = clamp01(A + (c − A)·K)` on the document
//! profile's LINEAR values `L`:
//!
//! * Vibrance (v = slider/100) first: one anchor `A = M·(A/M)` and one gain `K`
//!   for the three components, read from member V3t-P's tables over the
//!   pixel's linear chroma and maximum (`common/vibrance_reference.rs`, which
//!   reads them from the runner's own record of the fit);
//! * Saturation (s = slider/100): `A = (0.288040, 0.711874, 0.000086)·L` (the
//!   ROMM Y row), `K = 1 + s`.
//!
//! At 16 bits a profile that fails the core's precision checks but whose
//! curves have an `exposure_domain` (Apple's ROMM RGB, ITU-709) binds that
//! per-channel power (ROMM16 = R-pow in the runner's judge): the ROMM 16-bit
//! rows below decode through x^1.8, not ROMM's own toe.
//!
//! Saturation meets the bound (take2: 8 bits 1 / 0.014 codes, 16 bits 0.17 /
//! 0.007). Vibrance does not — no member of either family did — and ships
//! because it is measurably closer than round 2's form (take1 v-only 11 / 0.64
//! and 11.3 / 0.54 codes against 21 / 4.98 and 19.4 / 5.13); each row below is
//! held to the shipped member's own residual on it, never above round 2's
//! bound.
//!
//! Posterize: at 8 bits the code `c` falls in the bin `k = floor(c·n/256)`
//! and the output code is `floor(255·k/(n − 1))` (takes 2-4: exact on every
//! component, where the `n`-bin rule it replaced missed by up to 128) and,
//! at 16 bits, `k = min(n − 1, floor(v15·n/32768))` on Photoshop's 15-bit
//! grid `v15 = round(c·32768/65535)`, output `k/(n − 1)` (take2: within
//! 2/65535).
//!
//! The oracles are binary64 from those rules, with this file's own reading of
//! each profile's TRC tag, never the core's model; the core is checked against
//! them through `dr_doc_adjust_image`, the layer render, `dr_image_adjust_op`,
//! `dr_image_posterize` and `PrecisionAdjustment`, and against Photoshop's own
//! rows and the committed `vibA_{8,16}` render.

use std::ffi::CString;
use std::path::Path;
use std::ptr;
use std::sync::Arc;

use doctored_core::adjust_precision::PrecisionAdjustment;
use doctored_core::doc::DrDocument;
use doctored_core::ffi::{dr_image_free, dr_image_from_rgba, dr_image_pixels_rgba};
use doctored_core::ffi_adjust::dr_image_adjust_op;
use doctored_core::ffi_calibrated::dr_doc_adjust_image;
use doctored_core::ffi_doc::{dr_doc_free, dr_doc_with_layer_meta};
use doctored_core::ffi_filters::dr_image_posterize;
use doctored_core::ffi_insert::dr_doc_adding_layer_at;
use doctored_core::icc::IccProfile;
use doctored_core::DrImage;
use image::{Rgba, RgbaImage};
use serde_json::{json, Value};

use crate::common::take_err_string;
use crate::vibrance_reference;

// ------------------------------------------------------------ the curve --

/// One profile's TRC as this file reads it: a `curv` tag's gamma or table
/// (linearly interpolated, inverted by search), or ROMM's `para` type 3.
enum Curve {
    Gamma(f64),
    Table(Vec<f64>),
    Romm,
}

impl Curve {
    fn of(profile: &IccProfile) -> Curve {
        let bytes = profile.bytes();
        let be32 = |at: usize| u32::from_be_bytes(bytes[at..at + 4].try_into().unwrap());
        let tag = (0..be32(128) as usize)
            .map(|n| 132 + n * 12)
            .find(|&entry| &bytes[entry..entry + 4] == b"rTRC")
            .map(|entry| &bytes[be32(entry + 4) as usize..][..be32(entry + 8) as usize])
            .expect("an rTRC tag");
        assert_eq!(&tag[..4], b"curv");
        let n = u32::from_be_bytes(tag[8..12].try_into().unwrap()) as usize;
        let u16_at = |i: usize| f64::from(u16::from_be_bytes([tag[12 + 2 * i], tag[13 + 2 * i]]));
        match n {
            0 => Curve::Gamma(1.0),
            1 => Curve::Gamma(u16_at(0) / 256.0),
            _ => Curve::Table((0..n).map(|i| u16_at(i) / 65535.0).collect()),
        }
    }

    fn decode(&self, x: f64) -> f64 {
        match self {
            Curve::Gamma(g) => x.powf(*g),
            Curve::Romm if x < 1.0 / 512.0 => x / 16.0,
            Curve::Romm => x.powf(1.8),
            Curve::Table(t) => {
                let pos = x * (t.len() - 1) as f64;
                let i = (pos.floor() as usize).min(t.len() - 2);
                let f = pos - i as f64;
                t[i] * (1.0 - f) + t[i + 1] * f
            }
        }
    }

    fn encode(&self, y: f64) -> f64 {
        match self {
            Curve::Gamma(g) => y.powf(1.0 / g),
            Curve::Romm if y < (1.0f64 / 512.0).powf(1.8) => y * 16.0,
            Curve::Romm => y.powf(1.0 / 1.8),
            Curve::Table(t) if y <= t[0] => 0.0,
            Curve::Table(t) if y >= t[t.len() - 1] => 1.0,
            Curve::Table(t) => {
                let upper = t.iter().position(|&s| s >= y).unwrap();
                let lower = upper - 1;
                let f = (y - t[lower]) / (t[upper] - t[lower]);
                (lower as f64 + f) / (t.len() - 1) as f64
            }
        }
    }
}

// --------------------------------------------------- the Vibrance oracle --

const S_ANCHOR: [f64; 3] = [0.288040, 0.711874, 0.000086];

fn pull(l: [f64; 3], anchor: f64, k: f64) -> [f64; 3] {
    l.map(|c| (anchor + (c - anchor) * k).clamp(0.0, 1.0))
}

/// The declared form (module doc) on one ENCODED triple through `curve`;
/// sliders in Photoshop's units. A slider at 0 skips its stage.
fn vibrance(x: [f64; 3], v: f64, s: f64, curve: &Curve) -> [f64; 3] {
    if v == 0.0 && s == 0.0 {
        return x;
    }
    let l = vibrance_reference::stage(x.map(|c| curve.decode(c)), v / 100.0);
    if s == 0.0 {
        return l.map(|c| curve.encode(c));
    }
    let anchor = (0..3).map(|c| S_ANCHOR[c] * l[c]).sum();
    pull(l, anchor, (1.0 + s / 100.0).max(0.0)).map(|c| curve.encode(c))
}

/// (vibrance, saturation, document, input codes, Photoshop's codes).
type VibranceRow = (f64, f64, &'static str, [u32; 3], [u32; 3]);

/// Photoshop's rows: take2's worst Saturation rows (judged in round 1), take4's
/// worst Vibrance and combined rows of each document (round 2), then take1's
/// worst Vibrance row of each document under the shipped member.
#[rustfmt::skip]
const VIBRANCE_ROWS: [VibranceRow; 25] = [
    (0.0, 85.0, "srgb8", [43, 150, 66], [0, 164, 0]),
    (0.0, 85.0, "srgb8", [25, 76, 230], [0, 84, 255]),
    (0.0, 20.0, "srgb8", [230, 25, 179], [244, 0, 187]),
    (0.0, 85.0, "srgb16", [21332, 9766, 55769], [25672, 2502, 65535]),
    (0.0, 85.0, "srgb16", [4370, 7454, 53455], [1328, 8042, 65535]),
    (0.0, -70.0, "srgb16", [16448, 0, 65535], [11438, 6684, 38857]),
    (-90.0, 0.0, "srgb8", [115, 249, 3], [159, 249, 128]),
    (-90.0, 0.0, "srgb8", [9, 222, 252], [130, 229, 252]),
    (-60.0, 0.0, "srgb8", [115, 249, 3], [146, 249, 106]),
    (-90.0, 0.0, "srgb16", [29556, 63993, 772], [40865, 63991, 32819]),
    (-90.0, 0.0, "srgb16", [1800, 41891, 64763], [33385, 48373, 64761]),
    (-60.0, 0.0, "srgb16", [29556, 63993, 772], [37577, 63991, 27190]),
    (-60.0, 0.0, "wide16", [12594, 3342, 62707], [25706, 22508, 62691]),
    (-60.0, 0.0, "wide16", [4884, 59881, 19018], [22176, 59817, 28612]),
    (35.0, 0.0, "wide16", [62965, 52427, 41633], [63015, 52157, 40963]),
    (35.0, 20.0, "srgb8", [111, 235, 94], [53, 240, 0]),
    (35.0, 20.0, "srgb8", [94, 235, 111], [0, 241, 55]),
    (-60.0, -25.0, "srgb8", [235, 12, 95], [219, 118, 137]),
    (35.0, 20.0, "srgb16", [28528, 60395, 24158], [13634, 61753, 0]),
    (35.0, 20.0, "srgb16", [24158, 60395, 28528], [0, 61841, 14190]),
    (-60.0, -25.0, "srgb16", [29556, 63993, 772], [43869, 62559, 38087]),
    (90.0, 0.0, "srgb8", [255, 103, 58], [255, 93, 31]),
    (90.0, 0.0, "srgb16", [65535, 26472, 14906], [65535, 23946, 7992]),
    (90.0, 0.0, "wide16", [39321, 34181, 27500], [40509, 33839, 24714]),
    (65.0, 0.0, "wide8", [153, 133, 107], [156, 133, 102]),
];

/// The bounds per family and depth, in 8-bit codes: the Saturation target
/// (take2), then the shipped member's own worst residual on these rows (each
/// below round 2's bound on the same rows: v-only 19 / 19.2554, combined 31 /
/// 30.8876). The take judges read each 16-bit input as Photoshop's readback
/// (within 1.5e-5 of the written code) and these rows carry the code, so a
/// 16-bit bound takes 0.01 codes of slack.
fn vibrance_bound(v: f64, s: f64, sixteen: bool) -> f64 {
    const READBACK_SLACK: f64 = 0.01;
    match (v != 0.0, s != 0.0, sixteen) {
        (false, _, _) => 2.0,
        (true, false, false) => 11.0,
        (true, false, true) => 11.2735 + READBACK_SLACK,
        (true, true, false) => 0.0,
        (true, true, true) => 0.0577 + READBACK_SLACK,
    }
}

/// The document's curve as the shipped binding reads it: sRGB's table, ROMM's
/// own `para` at 8 bits and its 1.8 power at 16 (module doc).
fn row_curve(doc: &str) -> (Curve, f64) {
    match doc {
        "srgb8" => (Curve::of(&IccProfile::srgb()), 255.0),
        "srgb16" => (Curve::of(&IccProfile::srgb()), 65535.0),
        "wide8" => (Curve::Romm, 255.0),
        _ => (Curve::Gamma(1.8), 65535.0),
    }
}

#[test]
fn vibrance_oracle_holds_photoshops_rows_to_their_judged_residuals() {
    for (v, s, doc, input, photoshop) in VIBRANCE_ROWS {
        let (curve, top) = row_curve(doc);
        let x = input.map(|c| f64::from(c) / top);
        let want = vibrance(x, v, s, &curve);
        let err = (0..3)
            .map(|c| {
                if top == 255.0 {
                    ((want[c] * 255.0).round() - f64::from(photoshop[c])).abs()
                } else {
                    255.0 * (want[c] - f64::from(photoshop[c]) / 65535.0).abs()
                }
            })
            .fold(0.0, f64::max);
        assert!(
            err <= vibrance_bound(v, s, top != 255.0),
            "v {v} s {s} {doc} {input:?}: {:?} vs Photoshop {photoshop:?} ({err:.2} codes)",
            want.map(|w| (w * top).round())
        );
    }
}

// ------------------------------------------------------------- plumbing --

struct TestImage(*mut DrImage);
impl Drop for TestImage {
    fn drop(&mut self) {
        unsafe { dr_image_free(self.0) };
    }
}

fn test_image(pixels: &RgbaImage) -> TestImage {
    let raw =
        unsafe { dr_image_from_rgba(pixels.as_raw().as_ptr(), pixels.width(), pixels.height()) };
    assert!(!raw.is_null());
    TestImage(raw)
}

fn take_pixels(out: *mut DrImage, len: usize) -> Vec<u8> {
    assert!(!out.is_null());
    let bytes = unsafe { std::slice::from_raw_parts(dr_image_pixels_rgba(out), len) }.to_vec();
    unsafe { dr_image_free(out) };
    bytes
}

/// `dr_doc_adjust_image` (the sheets' and `apply_filter`'s path).
fn adjusted(doc: &DrDocument, src: &RgbaImage, op: &str, params: &Value) -> Vec<u8> {
    let image = test_image(src);
    let op = CString::new(op).unwrap();
    let params = CString::new(params.to_string()).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe { dr_doc_adjust_image(doc, image.0, op.as_ptr(), params.as_ptr(), &mut err) };
    assert!(!out.is_null(), "{}", take_err_string(err));
    take_pixels(out, src.as_raw().len())
}

/// `dr_image_adjust_op`, the profile-free export.
fn image_adjusted(src: &RgbaImage, op: &str, params: &Value) -> Vec<u8> {
    let image = test_image(src);
    let op = CString::new(op).unwrap();
    let params = CString::new(params.to_string()).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe { dr_image_adjust_op(image.0, op.as_ptr(), params.as_ptr(), &mut err) };
    assert!(!out.is_null(), "{}", take_err_string(err));
    take_pixels(out, src.as_raw().len())
}

/// The same description as an adjustment layer over `doc`, flattened.
fn layer_render(doc: &DrDocument, op: &str, params: &Value) -> Vec<u8> {
    let meta = json!({"type": "adjust", "op": op, "params": params}).to_string();
    let name = CString::new("Adjustment").unwrap();
    let added = unsafe {
        dr_doc_adding_layer_at(doc, 0, 0 /* DR_INSERT_ABOVE */, name.as_ptr())
    };
    assert!(!added.is_null());
    let meta = CString::new(meta).unwrap();
    let layered = unsafe { dr_doc_with_layer_meta(added, 1, meta.as_ptr()) };
    unsafe { dr_doc_free(added) };
    assert!(!layered.is_null());
    let layered = unsafe { Box::from_raw(layered) };
    layered.flattened().unwrap().into_raw()
}

fn doc_in(src: &RgbaImage, profile: Arc<IccProfile>) -> DrDocument {
    let mut doc = DrDocument::from_pixels(src.clone());
    doc.profile = profile;
    doc
}

fn precision(op: &str, params: &Value, profile: &IccProfile) -> PrecisionAdjustment {
    let meta = json!({
        "type": "adjust", "evaluation": "precision_v1", "op": op, "params": params
    })
    .to_string();
    PrecisionAdjustment::parse(&meta, profile).expect("parses")
}

/// Seven levels per channel, opaque: neutrals, primaries, secondaries,
/// skin-like and pale colours.
fn byte_grid() -> RgbaImage {
    const LEVELS: [u8; 7] = [0, 23, 70, 128, 171, 230, 255];
    RgbaImage::from_fn(49, 7, |x, y| {
        Rgba([
            LEVELS[(x / 7) as usize],
            LEVELS[(x % 7) as usize],
            LEVELS[y as usize],
            255,
        ])
    })
}

fn u16_grid() -> Vec<[u16; 3]> {
    const LEVELS: [u16; 7] = [0, 1000, 9000, 30000, 47000, 60000, 65535];
    let mut out = Vec::new();
    for r in LEVELS {
        for g in LEVELS {
            for b in LEVELS {
                out.push([r, g, b]);
            }
        }
    }
    out
}

/// Settings in Photoshop's units: each slider alone at both signs, both
/// together, and the ends.
const SETTINGS: [(f64, f64); 8] = [
    (40.0, 0.0),
    (-60.0, 0.0),
    (100.0, 0.0),
    (0.0, 35.0),
    (0.0, -100.0),
    (40.0, -20.0),
    (-50.0, 40.0),
    (-100.0, 100.0),
];

fn vibrance_params(v: f64, s: f64) -> Value {
    json!({"vibrance": v / 100.0, "saturation": s / 100.0})
}

// ------------------------------------------------------ Vibrance renders --

/// `|out − round(255·oracle)| ≤ 1`: the byte encoder places codes by
/// forward thresholds where the oracle inverts the table.
#[test]
fn eight_bit_vibrance_follows_the_oracle_on_each_profile_and_the_layer_equals_it() {
    let src = byte_grid();
    for profile in [IccProfile::srgb(), IccProfile::prophoto_rgb()] {
        let curve = Curve::of(&profile);
        let doc = doc_in(&src, profile.clone());
        for (v, s) in SETTINGS {
            let p = vibrance_params(v, s);
            let out = adjusted(&doc, &src, "vibrance", &p);
            for (px, got) in src.pixels().zip(out.chunks_exact(4)) {
                let x = [px[0], px[1], px[2]].map(|c| f64::from(c) / 255.0);
                let want = vibrance(x, v, s, &curve).map(|c| (c * 255.0).round());
                for c in 0..3 {
                    assert!(
                        (f64::from(got[c]) - want[c]).abs() <= 1.0,
                        "{p}, input {px:?}: {got:?} vs {want:?}"
                    );
                }
                assert_eq!(got[3], 255);
            }
            assert_eq!(
                layer_render(&doc, "vibrance", &p),
                out,
                "{p}: layer vs destructive"
            );
        }
    }
}

#[test]
fn sixteen_bit_vibrance_follows_the_oracle_on_each_profile() {
    for profile in [IccProfile::srgb(), IccProfile::prophoto_rgb()] {
        let curve = Curve::of(&profile);
        for (v, s) in SETTINGS {
            let op = precision("vibrance", &vibrance_params(v, s), &profile);
            for input in u16_grid() {
                let got = op.apply_at(input.map(|c| f32::from(c) / 65535.0), 0.0, (0, 0));
                let want = vibrance(input.map(|c| f64::from(c) / 65535.0), v, s, &curve);
                for c in 0..3 {
                    assert!(
                        (f64::from(got[c]) - want[c]).abs() <= 1e-4,
                        "v {v} s {s}, input {input:?}: {got:?} vs {want:?}"
                    );
                }
            }
        }
    }
}

/// At 16 bits Apple's ROMM RGB and ITU-709 fail the core's precision checks
/// (a falling `para` branch boundary), so Vibrance binds their
/// `exposure_domain` power per channel (ROMM16 = R-pow in
/// `adjust_arith_578_take.py`'s judge: 16.8 / 0.40 codes on the ROMM 16-bit
/// rows, where the builtin sRGB curve this replaced missed by 72.8 / 4.01):
/// x^1.8 for ROMM's pure power, the fitted 1.883 for ITU-709's offset curve.
/// Skipped where the system profile is absent. At rest the op is the
/// identity, bit for bit.
#[test]
fn sixteen_bit_vibrance_binds_the_shipped_basis_on_apple_romm_and_itu709() {
    const PROFILES: &str = "/System/Library/ColorSync/Profiles";
    for (file, gamma) in [("ROMM RGB.icc", 1.8), ("ITU-709.icc", 1.883)] {
        let Ok(bytes) = std::fs::read(Path::new(PROFILES).join(file)) else {
            eprintln!("skipping {file}: {PROFILES}/{file} is absent");
            continue;
        };
        let profile = IccProfile::parse(&bytes).expect("an RGB matrix/TRC profile");
        let curve = Curve::Gamma(gamma);
        for (v, s) in SETTINGS {
            let op = precision("vibrance", &vibrance_params(v, s), &profile);
            for input in u16_grid() {
                let got = op.apply_at(input.map(|c| f32::from(c) / 65535.0), 0.0, (0, 0));
                let want = vibrance(input.map(|c| f64::from(c) / 65535.0), v, s, &curve);
                for c in 0..3 {
                    assert!(
                        (f64::from(got[c]) - want[c]).abs() <= 1e-4,
                        "{file} v {v} s {s}, input {input:?}: {got:?} vs {want:?}"
                    );
                }
            }
        }
        let rest = precision("vibrance", &vibrance_params(0.0, 0.0), &profile);
        for input in u16_grid() {
            let x = input.map(|c| f32::from(c) / 65535.0);
            assert_eq!(rest.apply_at(x, 0.0, (0, 0)), x, "{file} at rest");
        }
    }
}

#[test]
fn the_unbound_image_op_is_the_builtin_srgb_document_and_rest_is_exact() {
    let src = byte_grid();
    let doc = doc_in(&src, IccProfile::srgb());
    for (v, s) in SETTINGS {
        let p = vibrance_params(v, s);
        assert_eq!(
            image_adjusted(&src, "vibrance", &p),
            adjusted(&doc, &src, "vibrance", &p),
            "{p}"
        );
    }
    let rest = vibrance_params(0.0, 0.0);
    assert_eq!(adjusted(&doc, &src, "vibrance", &rest), *src.as_raw());
    let op = precision("vibrance", &rest, &IccProfile::prophoto_rgb());
    assert!(op.is_identity());
    for input in u16_grid() {
        let x = input.map(|c| f32::from(c) / 65535.0);
        assert_eq!(op.apply_at(x, 0.0, (0, 0)), x);
    }
}

/// The committed Photoshop render of Vibrance 40 / Saturation −20 over the
/// phase-10 picture, held to the shipped member's own residual on it, per
/// sample as rendered here: 8 bits 8 / 0.6226, 16 bits 7.5711 / 0.5310 codes
/// (round 2's form left 9 / 3.5012 and 8.9692 / 3.4845; the runner's model of
/// V3t-P, `adjust_arith_578_take.viba_stats`, 8 / 0.6226 and 7.5711 / 0.5309).
#[test]
fn photoshops_viba_render_is_inside_the_forms_residual() {
    let dir =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/psd_photoshop/adjustments");
    let p = vibrance_params(40.0, -20.0);
    let base = image::open(dir.join("base.png")).unwrap().to_rgba8();
    let want = image::open(dir.join("vibA_8.png")).unwrap().to_rgba8();
    let got = adjusted(&doc_in(&base, IccProfile::srgb()), &base, "vibrance", &p);
    let errs: Vec<f64> = got
        .chunks_exact(4)
        .zip(want.pixels())
        .map(|(g, w)| {
            (0..3)
                .map(|c| (f64::from(g[c]) - f64::from(w[c])).abs())
                .fold(0.0, f64::max)
        })
        .collect();
    let (max, mean) = (
        errs.iter().cloned().fold(0.0, f64::max),
        errs.iter().sum::<f64>() / errs.len() as f64,
    );
    assert!(
        max <= 8.0 && mean <= 0.6226,
        "8 bits: max {max}, mean {mean}"
    );

    let base = image::open(dir.join("base_16.png")).unwrap().to_rgba16();
    let want = image::open(dir.join("vibA_16.png")).unwrap().to_rgba16();
    let op = precision("vibrance", &p, &IccProfile::srgb());
    let errs: Vec<f64> = base
        .pixels()
        .zip(want.pixels())
        .map(|(b, w)| {
            let got = op.apply_at(
                [b[0], b[1], b[2]].map(|c| f32::from(c) / 65535.0),
                0.0,
                (0, 0),
            );
            (0..3)
                .map(|c| 255.0 * (f64::from(got[c]) - f64::from(w[c]) / 65535.0).abs())
                .fold(0.0, f64::max)
        })
        .collect();
    let (max, mean) = (
        errs.iter().cloned().fold(0.0, f64::max),
        errs.iter().sum::<f64>() / errs.len() as f64,
    );
    assert!(
        max <= 7.5711 && mean <= 0.5310,
        "16 bits: max {max}, mean {mean}"
    );
}

// ----------------------------------------------------------- Posterize --

/// The 256 codes as one 256 x 1 opaque grey ramp.
fn ramp() -> RgbaImage {
    RgbaImage::from_fn(256, 1, |x, _| Rgba([x as u8, x as u8, x as u8, 255]))
}

fn reds(bytes: Vec<u8>) -> Vec<u8> {
    bytes
        .chunks(4)
        .map(|p| {
            assert!(p[0] == p[1] && p[1] == p[2], "{p:?}");
            p[0]
        })
        .collect()
}

/// The declared 8-bit rule on one code.
fn posterize8(code: u32, n: u32) -> u32 {
    let k = code * n / 256;
    255 * k / (n - 1)
}

/// The declared 16-bit rule on one code, as a fraction.
fn posterize16(code: u32, n: u32) -> f64 {
    let v15 = (u64::from(code) * 65536 + 65535) / 131_070;
    let k = (v15 * u64::from(n) / 32768).min(u64::from(n) - 1);
    k as f64 / f64::from(n - 1)
}

#[test]
fn posterize_is_the_declared_bins_at_8_bits() {
    let src = ramp();
    for n in 2..=64u32 {
        let want: Vec<u8> = (0..=255u32).map(|c| posterize8(c, n) as u8).collect();
        let image = test_image(&src);
        let filter = reds(take_pixels(unsafe { dr_image_posterize(image.0, n) }, 1024));
        assert_eq!(filter, want, "dr_image_posterize({n})");
        let layer = reds(image_adjusted(&src, "posterize", &json!({"levels": n})));
        assert_eq!(layer, want, "posterize adjustment, {n} levels");
    }
}

#[test]
fn posterize_is_the_declared_15_bit_bins_at_16_bits() {
    for n in [2u32, 3, 4, 5, 7, 11, 16, 24, 33, 64] {
        let op = precision("posterize", &json!({"levels": n}), &IccProfile::srgb());
        for c in (0..=65535u32).step_by(3).chain([65534, 65535]) {
            let got = op.apply_at([c as f32 / 65535.0; 3], 0.0, (0, 0));
            let want = posterize16(c, n);
            for g in got {
                assert!(
                    (f64::from(g) - want).abs() <= 1e-6,
                    "{n} levels, code {c}: got {g}, want {want}"
                );
            }
        }
    }
}

/// Photoshop's rows: at 8 bits take3's and take4's worst under the rule this
/// one replaced (`n`-bin rounding, 128 codes at most), at 16 bits take2's
/// worst under the shipped rule: (n, depth, input codes, Photoshop's codes).
#[rustfmt::skip]
const POSTERIZE_ROWS: [(u32, u32, [u32; 3], [u32; 3]); 18] = [
    (3, 8, [0, 85, 170], [0, 0, 127]),
    (5, 8, [17, 102, 187], [0, 63, 191]),
    (7, 8, [49, 134, 219], [42, 127, 212]),
    (10, 8, [34, 119, 204], [28, 113, 198]),
    (11, 8, [8, 93, 178], [0, 76, 178]),
    (12, 8, [0, 85, 170], [0, 69, 162]),
    (6, 8, [0, 85, 170], [0, 51, 153]),
    (9, 8, [0, 85, 170], [0, 63, 159]),
    (15, 8, [0, 85, 170], [0, 72, 163]),
    (24, 8, [0, 85, 170], [0, 77, 166]),
    (40, 8, [9, 94, 179], [6, 91, 176]),
    (11, 8, [232, 57, 65], [229, 51, 51]),
    (8, 16, [54483, 17220, 3598], [56171, 18724, 0]),
    (8, 16, [51143, 49857, 42147], [56171, 56171, 46809]),
    (8, 16, [35979, 24158, 64763], [37447, 18724, 65535]),
    (8, 16, [23644, 44203, 15934], [18724, 46809, 9362]),
    (8, 16, [58595, 7968, 35979], [65535, 0, 37447]),
    (8, 16, [44203, 514, 41891], [46809, 0, 46809]),
];

/// The judged residuals as the bounds: 0 codes at 8 bits (takes 2-4 exact,
/// inside round 2's declared bound of 1), 2/65535 at 16.
#[test]
fn posterize_holds_photoshops_rows_to_their_judged_residuals() {
    for (n, depth, input, photoshop) in POSTERIZE_ROWS {
        for c in 0..3 {
            let err = if depth == 8 {
                (f64::from(posterize8(input[c], n)) - f64::from(photoshop[c])).abs()
            } else {
                (posterize16(input[c], n) * 65535.0 - f64::from(photoshop[c])).abs()
            };
            let bound = if depth == 8 { 0.0 } else { 2.0 };
            assert!(
                err <= bound,
                "{n} levels, {depth} bits, {input:?} vs {photoshop:?}"
            );
        }
    }
}
