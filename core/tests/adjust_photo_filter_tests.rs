//! Photo Filter at 8 and 16 bits: the density multiply in the
//! document profile's own D50 PCS XYZ, then encode, then — Preserve
//! Luminosity — Photoshop's Luminosity blend against the input (at 8 bits on
//! the codes, `oracle8`).
//!
//! The oracle is written here in binary64 from the form fitted to Photoshop
//! 26.10.0 (`adjust_photo_filter`'s module doc), with its own s15Fixed16 tag
//! reader and the IEC 61966-2-1 transfer function, never the production ICC
//! model. `oracle_reproduces_photoshop_16bit` pins it against Photoshop's own
//! numbers (the take2 16-bit twins and the phase-10 picture); the renders are
//! then checked against it through `dr_doc_adjust_image`, the layer render and
//! `PrecisionAdjustment`.

use std::ffi::CString;
use std::ptr;
use std::sync::Arc;

use doctored_core::adjust_precision::PrecisionAdjustment;
use doctored_core::doc::DrDocument;
use doctored_core::ffi::*;
use doctored_core::ffi_adjust::dr_image_adjust_op;
use doctored_core::ffi_calibrated::dr_doc_adjust_image;
use doctored_core::ffi_doc::*;
use doctored_core::ffi_insert::dr_doc_adding_layer_at;
use doctored_core::icc::IccProfile;
use doctored_core::DrImage;
use image::{Rgba, RgbaImage};
use serde_json::{json, Value};

use crate::common;
use crate::icc_lut_builders::{build, tag_desc, tag_mft2, tag_wtpt};
use common::*;

const D50: [f64; 3] = [0.9642, 1.0, 0.8249];

// -------------------------------------------------------------- oracle --

type M3 = [[f64; 3]; 3];

fn mat_vec(m: M3, v: [f64; 3]) -> [f64; 3] {
    std::array::from_fn(|i| m[i][0] * v[0] + m[i][1] * v[1] + m[i][2] * v[2])
}

/// Cramer's rule: independent of the core's inverse.
fn inverse(m: M3) -> M3 {
    let det = m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
        - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
        + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0]);
    let cof = |r: usize, c: usize| {
        let rows: Vec<usize> = (0..3).filter(|&i| i != r).collect();
        let cols: Vec<usize> = (0..3).filter(|&j| j != c).collect();
        let minor =
            m[rows[0]][cols[0]] * m[rows[1]][cols[1]] - m[rows[0]][cols[1]] * m[rows[1]][cols[0]];
        if (r + c).is_multiple_of(2) {
            minor
        } else {
            -minor
        }
    };
    std::array::from_fn(|i| std::array::from_fn(|j| cof(j, i) / det))
}

#[derive(Clone)]
enum Trc {
    /// IEC 61966-2-1, the analytic sRGB curve (Photoshop's sRGB, and the
    /// curve the builtin sRGB and Display P3 tables sample).
    Iec,
    /// ROMM's `para` type 3: γ 1.8, c 1/16, d 1/512.
    Romm,
    /// A `curv` table as the profile stores it (u16 samples over [0, 1]),
    /// linearly interpolated, inverted by searching the same samples.
    Table(Vec<f64>),
}

impl Trc {
    fn decode(&self, x: f64) -> f64 {
        match self {
            Trc::Iec if x <= 0.04045 => x / 12.92,
            Trc::Iec => ((x + 0.055) / 1.055).powf(2.4),
            Trc::Romm if x < 1.0 / 512.0 => x / 16.0,
            Trc::Romm => x.powf(1.8),
            Trc::Table(t) => {
                let pos = x * (t.len() - 1) as f64;
                let i = (pos.floor() as usize).min(t.len() - 2);
                let f = pos - i as f64;
                t[i] * (1.0 - f) + t[i + 1] * f
            }
        }
    }
    fn encode(&self, y: f64) -> f64 {
        match self {
            Trc::Iec if y <= 0.0031308 => y * 12.92,
            Trc::Iec => 1.055 * y.powf(1.0 / 2.4) - 0.055,
            Trc::Romm if y < (1.0f64 / 512.0).powf(1.8) => y * 16.0,
            Trc::Romm => y.powf(1.0 / 1.8),
            Trc::Table(t) if y <= t[0] => 0.0,
            Trc::Table(t) if y >= t[t.len() - 1] => 1.0,
            Trc::Table(t) => {
                let upper = t.iter().position(|&s| s >= y).unwrap();
                let lower = upper - 1;
                let f = (y - t[lower]) / (t[upper] - t[lower]);
                (lower as f64 + f) / (t.len() - 1) as f64
            }
        }
    }
}

/// One profile as the oracle sees it: ROWS of its tag matrix and its TRC.
#[derive(Clone)]
struct Space {
    m: M3,
    trc: Trc,
}

impl Space {
    fn from_columns(columns: [[f64; 3]; 3], trc: Trc) -> Space {
        Space {
            m: std::array::from_fn(|r| std::array::from_fn(|c| columns[c][r])),
            trc,
        }
    }

    /// The profile's own rXYZ/gXYZ/bXYZ tags, read as s15Fixed16 here.
    fn columns(profile: &IccProfile) -> [[f64; 3]; 3] {
        let bytes = profile.bytes();
        [b"rXYZ", b"gXYZ", b"bXYZ"].map(|key| {
            let data = tag(bytes, key);
            std::array::from_fn(|r| s15(data, 8 + r * 4))
        })
    }

    /// The profile's tag matrix with the analytic IEC curve: the oracle
    /// Photoshop's numbers are pinned against.
    fn iec(profile: &IccProfile) -> Space {
        Space::from_columns(Space::columns(profile), Trc::Iec)
    }

    /// The profile's own tags, curve included: its `curv` table, which the
    /// three TRC tags of both builtins share. Why the renders need the table
    /// and not the IEC formula: the builtins sample the curve as 1024 u16
    /// values, and in the shadows that quantization alone moves a filtered
    /// 16-bit result by up to 1.9e-4 from the analytic curve (measured on
    /// this file's grid) — a fact of the profile, not of the form.
    fn own(profile: &IccProfile) -> Space {
        let bytes = profile.bytes();
        let curve = tag(bytes, b"rTRC");
        assert_eq!(tag(bytes, b"gTRC"), curve);
        assert_eq!(tag(bytes, b"bTRC"), curve);
        assert_eq!(&curve[..4], b"curv");
        let n = be32(curve, 8) as usize;
        let table = (0..n)
            .map(|i| {
                let at = 12 + 2 * i;
                f64::from(u16::from_be_bytes([curve[at], curve[at + 1]])) / 65535.0
            })
            .collect();
        Space::from_columns(Space::columns(profile), Trc::Table(table))
    }

    fn decode(&self, v: [f64; 3]) -> [f64; 3] {
        v.map(|x| self.trc.decode(x))
    }

    /// `M·decode(color) / D50`.
    fn ratio(&self, hex: [u8; 3]) -> [f64; 3] {
        let xyz = mat_vec(self.m, self.decode(hex.map(|c| f64::from(c) / 255.0)));
        std::array::from_fn(|c| xyz[c] / D50[c])
    }
}

fn be32(b: &[u8], at: usize) -> u32 {
    u32::from_be_bytes(b[at..at + 4].try_into().unwrap())
}

fn s15(b: &[u8], at: usize) -> f64 {
    f64::from(be32(b, at) as i32) / 65536.0
}

fn tag<'a>(icc: &'a [u8], sig: &[u8; 4]) -> &'a [u8] {
    for n in 0..be32(icc, 128) as usize {
        let entry = 132 + n * 12;
        if &icc[entry..entry + 4] == sig {
            let start = be32(icc, entry + 4) as usize;
            return &icc[start..start + be32(icc, entry + 8) as usize];
        }
    }
    panic!("missing tag");
}

fn lum(c: [f64; 3]) -> f64 {
    0.3 * c[0] + 0.59 * c[1] + 0.11 * c[2]
}

/// W3C compositing-1 ClipColor.
fn clip_color(c: [f64; 3]) -> [f64; 3] {
    let l = lum(c);
    let n = c[0].min(c[1]).min(c[2]);
    let x = c[0].max(c[1]).max(c[2]);
    let mut c = c;
    if n < 0.0 {
        c = c.map(|v| l + (v - l) * l / (l - n));
    }
    if x > 1.0 {
        c = c.map(|v| l + (v - l) * (1.0 - l) / (x - l));
    }
    c
}

fn set_lum(c: [f64; 3], l: f64) -> [f64; 3] {
    let d = l - lum(c);
    clip_color(c.map(|v| v + d))
}

/// The fitted form on one ENCODED triple in [0, 1].
fn oracle(space: &Space, ratio: [f64; 3], density: f64, preserve: bool, v: [f64; 3]) -> [f64; 3] {
    oracle_on(space, ratio, density, preserve, v, |e| e)
}

/// [`oracle`] at 8 bits: `e` put on the 8-bit grid, as the byte path encodes
/// (`adjust_calibrated`: to the document's 8-bit grid), and — Preserve
/// Luminosity — the Luminosity blend on CODES ([`set_lum8`]), the form
/// `adjust-render-2` take2 confirmed. [`lum8`] and [`set_lum8`] restate the
/// shipped kernel's own steps and rounding (`adjust_bw8::set_lum8`,
/// `blend::lum_fixed`), so for the 8-bit blend this oracle checks agreement
/// with that statement; its independent check is Photoshop's own render,
/// `adjust_render_take.rs`'s committed take2 fixture.
fn oracle8(space: &Space, ratio: [f64; 3], density: f64, preserve: bool, v: [f64; 3]) -> [f64; 3] {
    let e = oracle_on(space, ratio, density, false, v, |e| {
        (e * 255.0).round() / 255.0
    });
    if !preserve {
        return e;
    }
    let codes = |c: [f64; 3]| c.map(|k| (k * 255.0).round() as i64);
    set_lum8(codes(e), lum8(codes(v))).map(|k| k as f64 / 255.0)
}

/// The 30/59/11 luma of 8-bit codes in 2^15 fixed point — 0.30 and 0.11
/// floored, green the remainder — rounded half up.
fn lum8(c: [i64; 3]) -> i64 {
    let r = (0.30_f64 * 32768.0).floor() as i64;
    let b = (0.11_f64 * 32768.0).floor() as i64;
    (r * c[0] + (32768 - r - b) * c[1] + b * c[2] + 16384) / 32768
}

/// The Luminosity blend on codes: `e` shifted to the luminosity `target`;
/// out of 0..=255, ClipColor in f64 about `target` (the minimum's branch,
/// then the maximum's against 255), each component rounded half up and one
/// code added, at most to `target`, below it.
fn set_lum8(e: [i64; 3], target: i64) -> [i64; 3] {
    let shift = target - lum8(e);
    let c = e.map(|k| k + shift);
    if c.iter().all(|k| (0..=255).contains(k)) {
        return c;
    }
    let l = target as f64;
    let mut c = c.map(|k| k as f64);
    let low = c[0].min(c[1]).min(c[2]);
    if low < 0.0 {
        c = c.map(|k| l + (k - l) * l / (l - low));
    }
    let high = c[0].max(c[1]).max(c[2]);
    if high > 255.0 && high > l {
        c = c.map(|k| l + (k - l) * (255.0 - l) / (high - l));
    }
    c.map(|k| {
        let q = (k + 0.5).floor() as i64;
        let q = if k < l { (q + 1).min(target) } else { q };
        q.clamp(0, 255)
    })
}

fn oracle_on(
    space: &Space,
    ratio: [f64; 3],
    density: f64,
    preserve: bool,
    v: [f64; 3],
    grid: impl Fn(f64) -> f64,
) -> [f64; 3] {
    let x = mat_vec(space.m, space.decode(v));
    let y: [f64; 3] = std::array::from_fn(|c| x[c] * (1.0 - density + density * ratio[c]));
    let linear = mat_vec(inverse(space.m), y);
    let e = linear.map(|l| grid(space.trc.encode(l.clamp(0.0, 1.0))));
    if preserve {
        set_lum(e, lum(v))
    } else {
        e
    }
}

fn unit16(v: [u16; 3]) -> [f64; 3] {
    v.map(|c| f64::from(c) / 65535.0)
}

fn hex(c: [u8; 3]) -> String {
    format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2])
}

#[test]
fn oracle_reproduces_photoshop_16bit() {
    let srgb = Space::iec(&IccProfile::srgb());
    let warming = [0xec, 0x8a, 0x00];
    let lab = [0.473671, 0.36632, 0.044935];
    let check =
        |what: &str, space: &Space, ratio, density, preserve, input, want: [f64; 3], tol| {
            let got = oracle(space, ratio, density, preserve, unit16(input));
            for c in 0..3 {
                assert!(
                    (got[c] - want[c]).abs() <= tol,
                    "{what} {input:?}: oracle {got:?} vs Photoshop {want:?}"
                );
            }
        };
    // Phase-10: sRGB, #ec8a00, density 0.4, preserve on.
    let phase10: [([u16; 3], [u16; 3]); 5] = [
        ([62193, 21074, 48573], [64209, 20680, 45183]),
        ([29042, 48573, 35465], [34745, 46307, 32048]),
        ([57311, 0, 65535], [56837, 2182, 55109]),
        ([65535, 52427, 53713], [65535, 52815, 51655]),
        ([65535, 48573, 33923], [65535, 48273, 35555]),
    ];
    let pf = srgb.ratio(warming);
    for (input, want) in phase10 {
        check("phase-10", &srgb, pf, 0.4, true, input, unit16(want), 1e-3);
    }
    // take2 16-bit twins, Photoshop's outputs as fractions.
    let romm = Space::from_columns(
        [
            [0.797668, 0.288040, 0.0],
            [0.135193, 0.711884, 0.0],
            [0.031357, 0.0000916, 0.825195],
        ],
        Trc::Romm,
    );
    type Case<'a> = (&'a str, &'a Space, [f64; 3], f64, bool, [u16; 3], [f64; 3]);
    #[rustfmt::skip]
    let cases: [Case; 9] = [
        ("A1", &srgb, lab, 1.0, false, [48192; 3], [0.672938, 0.393103, 0.00732433]),
        ("A1", &srgb, lab, 1.0, false, [0, 48192, 65535], [0.520104, 0.409613, 0.169772]),
        ("I1", &srgb, pf, 1.0, false, [65535; 3], [0.925414, 0.541192, 0.0000916]),
        ("I1", &srgb, pf, 1.0, false, [38261, 52280, 31753], [0.566064, 0.456855, 0.0]),
        ("T1", &srgb, lab, 0.25, true, [65535; 3], [1.0, 1.0, 1.0]),
        ("T1", &srgb, lab, 0.25, true, [65535, 65535, 22884], [1.0, 0.966857, 0.52697]),
        ("T1", &srgb, lab, 0.25, true, [48192; 3], [0.772976, 0.723049, 0.698787]),
        ("ROMM", &romm, lab, 1.0, false, [44590; 3], [0.472755, 0.351476, 0.121462]),
        ("ROMM", &romm, lab, 1.0, false, [44590, 26801, 18236], [0.459327, 0.184329, 0.0495918]),
    ];
    for (what, space, ratio, density, preserve, input, want) in cases {
        check(what, space, ratio, density, preserve, input, want, 2e-3);
    }
}

// ------------------------------------------------------------ plumbing --

struct TestImage(*mut DrImage);
impl Drop for TestImage {
    fn drop(&mut self) {
        unsafe {
            dr_image_free(self.0);
        }
    }
}

fn test_image(pixels: &RgbaImage) -> TestImage {
    let raw =
        unsafe { dr_image_from_rgba(pixels.as_raw().as_ptr(), pixels.width(), pixels.height()) };
    assert!(!raw.is_null());
    TestImage(raw)
}

fn params(color: [u8; 3], density: f64, preserve: bool) -> Value {
    json!({"color": hex(color), "density": density, "preserve_luminosity": preserve})
}

/// `dr_doc_adjust_image` (the sheets' and `apply_filter`'s path).
fn adjusted(doc: &DrDocument, src: &RgbaImage, params: &Value) -> Vec<u8> {
    let image = test_image(src);
    let op = CString::new("photo_filter").unwrap();
    let params = CString::new(params.to_string()).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe { dr_doc_adjust_image(doc, image.0, op.as_ptr(), params.as_ptr(), &mut err) };
    assert!(!out.is_null(), "{}", take_err_string(err));
    let bytes = pixels(out);
    unsafe {
        dr_image_free(out);
    }
    bytes
}

/// `dr_image_adjust_op`, the profile-free export.
fn image_adjusted(src: &RgbaImage, params: &Value) -> Vec<u8> {
    let image = test_image(src);
    let op = CString::new("photo_filter").unwrap();
    let params = CString::new(params.to_string()).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe { dr_image_adjust_op(image.0, op.as_ptr(), params.as_ptr(), &mut err) };
    assert!(!out.is_null(), "{}", take_err_string(err));
    let bytes = pixels(out);
    unsafe {
        dr_image_free(out);
    }
    bytes
}

/// The same description as an adjustment layer over `doc`, flattened.
fn layer_render(doc: &DrDocument, params: &Value) -> Vec<u8> {
    let meta = json!({"type": "adjust", "op": "photo_filter", "params": params}).to_string();
    let name = CString::new("Photo Filter").unwrap();
    let added = unsafe {
        dr_doc_adding_layer_at(doc, 0, 0 /* DR_INSERT_ABOVE */, name.as_ptr())
    };
    assert!(!added.is_null());
    let meta = CString::new(meta).unwrap();
    let layered = unsafe { dr_doc_with_layer_meta(added, 1, meta.as_ptr()) };
    unsafe {
        dr_doc_free(added);
    }
    assert!(!layered.is_null());
    let layered = unsafe { Box::from_raw(layered) };
    layered.flattened().unwrap().into_raw()
}

fn doc_in(src: &RgbaImage, profile: Arc<IccProfile>) -> DrDocument {
    let mut doc = DrDocument::from_pixels(src.clone());
    doc.profile = profile;
    doc
}

/// Every combination of six levels per channel, opaque: neutrals, the
/// primaries and secondaries, and highlights a preserve-on filter pushes
/// past white (ClipColor's `x > 1` branch).
fn byte_grid() -> RgbaImage {
    const LEVELS: [u8; 6] = [0, 37, 96, 128, 200, 255];
    RgbaImage::from_fn(36, 6, |x, y| {
        Rgba([
            LEVELS[(x / 6) as usize],
            LEVELS[(x % 6) as usize],
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

const FILTERS: [([u8; 3], f64); 3] = [
    ([0xec, 0x8a, 0x00], 0.4),
    ([0x00, 0x6d, 0xff], 0.9),
    ([0x80, 0x40, 0xc0], 1.0),
];

fn precision(params: &Value, profile: &IccProfile) -> PrecisionAdjustment {
    let description = json!({
        "type": "adjust", "evaluation": "precision_v1", "op": "photo_filter", "params": params
    })
    .to_string();
    PrecisionAdjustment::parse(&description, profile).unwrap()
}

fn apply16(op: &PrecisionAdjustment, v: [u16; 3]) -> [f32; 3] {
    op.apply_at(v.map(|c| f32::from(c) / 65535.0), 0.0, (0, 0))
}

/// A matrix-free RGB profile: `A2B0`/`B2A0` tables only, so no model.
fn lut_profile() -> Arc<IccProfile> {
    let bytes = build(
        [0x02, 0x20, 0, 0],
        b"mntr",
        b"RGB ",
        b"XYZ ",
        &[
            (b"desc", tag_desc("Synthetic LUT RGB")),
            (b"wtpt", tag_wtpt(D50)),
            (b"A2B0", tag_mft2(3, 3, 2, &|c| c.to_vec())),
            (b"B2A0", tag_mft2(3, 3, 2, &|c| c.to_vec())),
        ],
    );
    let profile = Arc::new(IccProfile::parse(&bytes).unwrap());
    assert!(profile.model().is_none());
    profile
}

// ------------------------------------------------------------- renders --

/// Why the bound is `|out − round(255·oracle8)| ≤ 1`: the byte path
/// quantizes the encoded `e` to codes BEFORE the Luminosity blend, and
/// [`oracle8`] does too, but through its own transfer formula where the
/// byte encoder reads the profile's sampled thresholds, so a value a hair
/// from a half code can land one code apart; with Preserve Luminosity on,
/// the blend on codes carries that code through.
///
/// Why the oracle puts `e` on the 8-bit grid at all: near black, the blend
/// lands below 0 and ClipColor's minimum branch rescales about a tiny
/// luminosity, which amplifies any difference in `e` — measured under the
/// float blend this replaced: input (0, 0, 37) under `#006dff` at density
/// 0.9, preserve on, is 32 on blue against 33.7 from a continuous `e`, and
/// 32.08 from a quantized one.
#[test]
fn eight_bit_renders_follow_the_oracle_and_the_layer_render_equals_them() {
    let src = byte_grid();
    for profile in [IccProfile::srgb(), IccProfile::display_p3()] {
        let space = Space::own(&profile);
        let doc = doc_in(&src, profile.clone());
        for (color, density) in FILTERS {
            for preserve in [true, false] {
                let p = params(color, density, preserve);
                let out = adjusted(&doc, &src, &p);
                for (px, got) in src.pixels().zip(out.chunks_exact(4)) {
                    let v = [px[0], px[1], px[2]].map(|c| f64::from(c) / 255.0);
                    let want = oracle8(&space, space.ratio(color), density, preserve, v)
                        .map(|c| (c * 255.0).round());
                    for c in 0..3 {
                        assert!(
                            (f64::from(got[c]) - want[c]).abs() <= 1.0,
                            "{p}, input {px:?}: {got:?} vs {want:?}"
                        );
                    }
                    assert_eq!(got[3], 255);
                }
                assert_eq!(layer_render(&doc, &p), out, "{p}: layer vs destructive");
            }
        }
    }
}

#[test]
fn sixteen_bit_renders_follow_the_oracle() {
    for profile in [IccProfile::srgb(), IccProfile::display_p3()] {
        let space = Space::own(&profile);
        for (color, density) in FILTERS {
            for preserve in [true, false] {
                let p = params(color, density, preserve);
                let op = precision(&p, &profile);
                for v in u16_grid() {
                    let got = apply16(&op, v);
                    let want = oracle(&space, space.ratio(color), density, preserve, unit16(v));
                    for c in 0..3 {
                        assert!(
                            (f64::from(got[c]) - want[c]).abs() <= 1e-4,
                            "{p}, input {v:?}: {got:?} vs {want:?}"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn a_profile_without_a_model_renders_as_builtin_srgb_and_so_does_the_image_export() {
    let src = byte_grid();
    let lut = lut_profile();
    let srgb = IccProfile::srgb();
    for (color, density) in FILTERS {
        for preserve in [true, false] {
            let p = params(color, density, preserve);
            let reference = adjusted(&doc_in(&src, srgb.clone()), &src, &p);
            assert_eq!(
                adjusted(&doc_in(&src, lut.clone()), &src, &p),
                reference,
                "{p}"
            );
            assert_eq!(image_adjusted(&src, &p), reference, "{p}: image export");
            let on_lut = precision(&p, &lut);
            let on_srgb = precision(&p, &srgb);
            for v in u16_grid() {
                assert_eq!(apply16(&on_lut, v), apply16(&on_srgb, v), "{p}, {v:?}");
            }
        }
    }
}

#[test]
fn density_zero_is_a_bit_exact_identity_at_both_depths() {
    let src = byte_grid();
    for profile in [IccProfile::srgb(), IccProfile::display_p3()] {
        let doc = doc_in(&src, profile.clone());
        for preserve in [true, false] {
            let p = params([0x00, 0xff, 0x00], 0.0, preserve);
            assert_eq!(adjusted(&doc, &src, &p), *src.as_raw(), "{p}");
            let op = precision(&p, &profile);
            assert!(op.is_identity());
            for v in u16_grid() {
                let input = v.map(|c| f32::from(c) / 65535.0);
                assert_eq!(apply16(&op, v), input, "{p}, {v:?}");
            }
        }
    }
}

#[test]
fn a_prepared_sixteen_bit_filter_retains_its_encoder_and_binds_but_never_reads_the_profile() {
    let op = precision(
        &params([0xec, 0x8a, 0x00], 0.25, true),
        &IccProfile::display_p3(),
    );
    assert!(
        op.retained_bytes() >= 3 * 65537 * 8,
        "the precision encoder tables are recorded: {}",
        op.retained_bytes()
    );
    assert!(op.binds_profile());
    assert!(!op.reads_profile());
}
