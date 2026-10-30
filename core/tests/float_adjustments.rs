//! The adjustments a 32-bit float document now evaluates — Levels, Black &
//! White (with Tint, whose oracle is `black_and_white_tint`'s), Channel
//! Mixer, Photo Filter, `bcs` (its modern form against its own formula),
//! Exposure of either evaluation, point-form Curves, map-form Curves (whose
//! oracle is `float_lookup_tint_map`'s) and Hue/Saturation's Colorize,
//! master Lightness alone or master Hue alone (whose oracles are
//! `adjust_hue_saturation_parity`'s) — each under its own closed form,
//! through `add_adjustment` and the request API (`set_mask`, `layer_blend`,
//! `group_layers`, `merge_down`), the regional render, the native round trip
//! and `dr_precision_doc_paste_layers`; the closed and display forms (whose
//! oracles are `float_display_adjustments`') adding and pasting; and Invert,
//! Shadows/Highlights and the legacy White Balance refusing by name.
//!
//! The oracles are written here in binary64 from the forms FITTED to
//! Photoshop 26.10.0's 32-bit renders (`scripts/photoshop-probe/
//! float_adjust.py`, receipt `doctored-fixtures/photoshop/float-adjust-473/
//! take2`), never read off the implementation, and
//! `oracles_reproduce_photoshop_take2` pins each against Photoshop's own
//! numbers (within `2e-3·max(1, |v|)`; the mixer within `1e-6`). Renders
//! match their oracle within `1e-5·max(1, |want|)` (Photo Filter `5e-5`)
//! over take2's 42-patch grid: 12 neutrals, 6 colours, 6 over-range, 3
//! negative and the 15 in-range companions.

use std::ffi::{c_char, CStr, CString};
use std::ptr;
use std::sync::Arc;

use doctored_core::adjust_precision::PrecisionAdjustment;
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_layer_clipboard::dr_precision_doc_paste_layers;
use doctored_core::ffi_precision_edit::dr_precision_doc_edit;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_clipboard::ClipboardPayload;
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer as Layer};
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use doctored_core::RegionRequest;
use serde_json::{json, Value};

use crate::black_and_white_tint::{params as tint_params, tint32, Set};
use crate::float_lookup_tint_map::{
    cross_5_oracle, cross_5_params, curves_map_oracle, identity, m1, map_params, sqrt_map,
};
use crate::icc_spelling::srgb_with_media_white;

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
const MULTIPLY: i32 = 1;
const D50: [f64; 3] = [0.9642, 1.0, 0.8249];
/// The unadapted D65 macOS's `sRGB Profile.icc` holds in `wtpt`.
const APPLE_D65: [f64; 3] = [0.950455, 1.0, 1.089050];

// ----------------------------------------------------------------- grid --

const NEUTRALS: [f32; 12] = [
    0.0, 0.001, 0.004, 0.02, 0.05, 0.1, 0.18, 0.3, 0.5, 0.7, 0.9, 1.0,
];
const COLOURED: [[f32; 3]; 6] = [
    [0.5, 0.2, 0.1],
    [0.05, 0.2, 0.6],
    [0.9, 0.1, 0.6],
    [0.3, 0.6, 0.2],
    [0.8, 0.7, 0.05],
    [0.02, 0.01, 0.3],
];
const OVER: [[f32; 3]; 6] = [
    [1.5, 1.5, 1.5],
    [4.0, 4.0, 4.0],
    [16.0, 16.0, 16.0],
    [2.0, 0.5, 0.25],
    [1.5, 1.2, 0.1],
    [4.0, 0.3, 2.0],
];
const NEGATIVE: [[f32; 3]; 3] = [[-0.25, 0.5, 1.0], [0.5, -0.1, 0.2], [-0.05, -0.05, -0.05]];

/// `float_adjust.py`'s `GRID`, in its order: 42 patches.
pub(crate) fn grid() -> Vec<[f32; 3]> {
    let mut out: Vec<[f32; 3]> = NEUTRALS.iter().map(|&v| [v; 3]).collect();
    out.extend(COLOURED);
    out.extend(OVER);
    out.extend(NEGATIVE);
    out.extend(OVER.iter().map(|v| v.map(|c| c.clamp(0.0, 1.0))));
    out.extend(OVER.iter().map(|v| {
        let m = v[0].max(v[1]).max(v[2]).max(1.0);
        v.map(|c| c / m)
    }));
    out.extend(NEGATIVE.iter().map(|v| v.map(|c| c.max(0.0))));
    assert_eq!(out.len(), 42);
    out
}

fn wide(v: [f32; 3]) -> [f64; 3] {
    v.map(f64::from)
}

// -------------------------------------------------------------- oracles --

fn signed_pow(t: f64, p: f64) -> f64 {
    t.signum() * t.abs().powf(p)
}

/// One Levels stage, every clamp removed: black, white, gamma, output black
/// and white.
#[derive(Clone, Copy)]
struct Stage([f64; 5]);

const UNIT: Stage = Stage([0.0, 1.0, 1.0, 0.0, 1.0]);

fn stage(v: f64, Stage([black, white, gamma, ob, ow]): Stage) -> f64 {
    let p = signed_pow((v - black) / (white - black), 1.0 / gamma);
    ob + (ow - ob) * p
}

/// The channel's stage, then the master's.
fn levels_oracle(master: Stage, channels: [Stage; 3], rgb: [f64; 3]) -> [f64; 3] {
    std::array::from_fn(|c| stage(stage(rgb[c], channels[c]), master))
}

/// Black & White: `neutral + (max − mid)·W[primary] + (mid − min)·W[pair]`
/// on the six-slot wheel (reds, yellows, greens, cyans, blues, magentas),
/// floored at 0 with no upper clamp.
fn bw_oracle(weights: [f64; 6], rgb: [f64; 3]) -> f64 {
    let mut order = [0usize, 1, 2];
    order.sort_by(|&a, &b| rgb[b].total_cmp(&rgb[a]));
    let [hi, mid, lo] = order;
    let primary = 2 * hi;
    let pair = match (hi.min(mid), hi.max(mid)) {
        (0, 1) => 1,
        (1, 2) => 3,
        _ => 5,
    };
    let gray =
        rgb[lo] + (rgb[hi] - rgb[mid]) * weights[primary] + (rgb[mid] - rgb[lo]) * weights[pair];
    gray.max(0.0)
}

/// One mixer number in Photoshop's whole 1/1024 steps, toward zero.
fn q(c: f64) -> f64 {
    (1024.0 * c).trunc() / 1024.0
}

/// The mixer at 32 bits: truncated coefficients, the constant at half its
/// 16-bit amount (`trunc(512·K)/1024`), no clamp. `rows[i] = [r, g, b, K]`.
fn mixer_oracle(rows: [[f64; 4]; 3], rgb: [f64; 3]) -> [f64; 3] {
    rows.map(|r| {
        q(r[0]) * rgb[0] + q(r[1]) * rgb[1] + q(r[2]) * rgb[2] + (512.0 * r[3]).trunc() / 1024.0
    })
}

/// `bcs` unclamped: brightness, contrast about 0.5, Rec. 709 luma-anchored
/// saturation.
fn bcs_oracle(brightness: f64, contrast: f64, saturation: f64, rgb: [f64; 3]) -> [f64; 3] {
    let o = rgb.map(|v| (v + brightness - 0.5) * (1.0 + contrast) + 0.5);
    let a = 0.2126 * o[0] + 0.7152 * o[1] + 0.0722 * o[2];
    o.map(|v| a + (v - a) * (1.0 + saturation))
}

/// Modern `bcs` unclamped: a value outside [0, 1] passes the
/// brightness power and the contrast S-curve unchanged; saturation as legacy.
fn bcs_modern_oracle(b: f64, c: f64, s: f64, rgb: [f64; 3]) -> [f64; 3] {
    let (g, k) = ((-1.5 * b).exp2(), (1.5 * c).exp2());
    let o = rgb.map(|x| match x.powf(g) {
        _ if !(0.0..=1.0).contains(&x) => x,
        y if y < 0.5 => 0.5 * (2.0 * y).powf(k),
        y => 1.0 - 0.5 * (2.0 * (1.0 - y)).powf(k),
    });
    let a = 0.2126 * o[0] + 0.7152 * o[1] + 0.0722 * o[2];
    o.map(|v| a + (v - a) * (1.0 + s))
}

/// Exposure per stored channel, no reference-basis matrices: `u = v·2^e +
/// o`, then the RECIPROCAL signed power `sgn(u)·|u|^(1/gamma)`.
fn exposure_oracle(stops: f64, offset: f64, gamma: f64, rgb: [f64; 3]) -> [f64; 3] {
    rgb.map(|v| signed_pow(v * stops.exp2() + offset, 1.0 / gamma))
}

/// Photoshop's curve: the natural cubic spline (`S'' = 0` at both ends)
/// through `points` in 0–255 units. The second derivatives come from the
/// full linear system by Gaussian elimination with partial pivoting, and
/// each segment is evaluated in its power basis `y_i + b·t + c·t² + d·t³`.
struct Natural {
    points: Vec<(f64, f64)>,
    m: Vec<f64>,
}

impl Natural {
    fn new(points: &[(f64, f64)]) -> Self {
        let n = points.len();
        let mut a = vec![vec![0.0; n + 1]; n];
        a[0][0] = 1.0;
        a[n - 1][n - 1] = 1.0;
        for i in 1..n - 1 {
            let (h0, h1) = (points[i].0 - points[i - 1].0, points[i + 1].0 - points[i].0);
            a[i][i - 1] = h0;
            a[i][i] = 2.0 * (h0 + h1);
            a[i][i + 1] = h1;
            a[i][n] =
                6.0 * ((points[i + 1].1 - points[i].1) / h1 - (points[i].1 - points[i - 1].1) / h0);
        }
        for col in 0..n {
            let pivot = (col..n)
                .max_by(|&p, &q| a[p][col].abs().total_cmp(&a[q][col].abs()))
                .unwrap();
            a.swap(col, pivot);
            let pivot_row = a[col].clone();
            for row in a.iter_mut().skip(col + 1) {
                let f = row[col] / pivot_row[col];
                for (cell, p) in row.iter_mut().zip(&pivot_row).skip(col) {
                    *cell -= f * p;
                }
            }
        }
        let mut m = vec![0.0; n];
        for row in (0..n).rev() {
            let tail: f64 = (row + 1..n).map(|k| a[row][k] * m[k]).sum();
            m[row] = (a[row][n] - tail) / a[row][row];
        }
        Natural {
            points: points.to_vec(),
            m,
        }
    }

    /// The power-basis coefficients `(b, c, d)` of segment `i`.
    fn coefficients(&self, i: usize) -> (f64, f64, f64) {
        let h = self.points[i + 1].0 - self.points[i].0;
        let secant = (self.points[i + 1].1 - self.points[i].1) / h;
        (
            secant - h * (2.0 * self.m[i] + self.m[i + 1]) / 6.0,
            self.m[i] / 2.0,
            (self.m[i + 1] - self.m[i]) / (6.0 * h),
        )
    }

    /// `S` inside the points; Photoshop's 32-bit ends outside them: the end
    /// slope above the last point, flat below the first unless it sits at 0.
    fn float(&self, x: f64) -> f64 {
        let n = self.points.len();
        let (x0, y0) = self.points[0];
        let (xn, yn) = self.points[n - 1];
        if x < x0 {
            return if x0 == 0.0 {
                y0 + self.coefficients(0).0 * x
            } else {
                y0
            };
        }
        if x > xn {
            let h = xn - self.points[n - 2].0;
            let (b, c, d) = self.coefficients(n - 2);
            return yn + (b + 2.0 * c * h + 3.0 * d * h * h) * (x - xn);
        }
        let i = (0..n - 1).rev().find(|&i| x >= self.points[i].0).unwrap();
        let (b, c, d) = self.coefficients(i);
        let t = x - self.points[i].0;
        self.points[i].1 + b * t + c * t * t + d * t * t * t
    }

    /// The 16-bit curve: flat beyond the ends, clamped to [0, 255].
    fn clamped(&self, x: f64) -> f64 {
        let (first, last) = (self.points[0], self.points[self.points.len() - 1]);
        if x <= first.0 {
            first.1
        } else if x >= last.0 {
            last.1
        } else {
            self.float(x).clamp(0.0, 255.0)
        }
    }
}

const IDENTITY_CURVE: [(f64, f64); 2] = [(0.0, 0.0), (255.0, 255.0)];

/// Curves at 32 bits: each channel's curve, then the master's, on `255·v`,
/// no clamp. `channels` is `[r, g, b]`.
fn curves_oracle(master: &[(f64, f64)], channels: [&[(f64, f64)]; 3], rgb: [f64; 3]) -> [f64; 3] {
    let master = Natural::new(master);
    std::array::from_fn(|c| master.float(Natural::new(channels[c]).float(rgb[c] * 255.0)) / 255.0)
}

const CURVES_T1: [(f64, f64); 4] = [(0.0, 0.0), (64.0, 48.0), (192.0, 214.0), (255.0, 255.0)];
const CURVES_I1: [(f64, f64); 2] = [(0.0, 30.0), (192.0, 255.0)];
const CURVES_I2: [(f64, f64); 2] = [(64.0, 0.0), (192.0, 255.0)];

type M3 = [[f64; 3]; 3];

fn mul(a: M3, b: M3) -> M3 {
    std::array::from_fn(|i| std::array::from_fn(|j| (0..3).map(|k| a[i][k] * b[k][j]).sum()))
}

fn apply(m: M3, v: [f64; 3]) -> [f64; 3] {
    m.map(|r| r[0] * v[0] + r[1] * v[1] + r[2] * v[2])
}

fn inv(m: M3) -> M3 {
    let det = m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
        - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
        + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0]);
    let c = |r: usize, k: usize| {
        let (r0, r1, k0, k1) = ((r + 1) % 3, (r + 2) % 3, (k + 1) % 3, (k + 2) % 3);
        m[r0][k0] * m[r1][k1] - m[r0][k1] * m[r1][k0]
    };
    std::array::from_fn(|i| std::array::from_fn(|j| c(j, i) / det))
}

/// The Bradford adaptation from white `s` to white `d`.
fn bradford(s: [f64; 3], d: [f64; 3]) -> M3 {
    const B: M3 = [
        [0.8951, 0.2664, -0.1614],
        [-0.7502, 1.7135, 0.0367],
        [0.0389, -0.0685, 1.0296],
    ];
    let (cs, cd) = (apply(B, s), apply(B, d));
    let scale: M3 =
        std::array::from_fn(|i| std::array::from_fn(|j| if i == j { cd[i] / cs[i] } else { 0.0 }));
    mul(inv(B), mul(scale, B))
}

/// A profile's D50 matrix `m` (row-major, columns = the XYZ tags) and its
/// native→D50 adaptation `d`.
#[derive(Clone, Copy)]
struct Basis {
    m: M3,
    d: M3,
}

impl Basis {
    fn new(columns: [[f64; 3]; 3], wtpt: Option<[f64; 3]>, chad: Option<M3>) -> Basis {
        let m = std::array::from_fn(|i| std::array::from_fn(|c| columns[c][i]));
        let d = match (chad, wtpt) {
            (Some(chad), _) => chad,
            (None, Some(w)) if w.iter().zip(D50).any(|(a, b)| (a - b).abs() > 1e-4) => {
                bradford(w, D50)
            }
            _ => [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
        };
        Basis { m, d }
    }

    /// Linear RGB → native XYZ: `D⁻¹·M`.
    fn native(&self) -> M3 {
        mul(inv(self.d), self.m)
    }
}

/// Photo Filter at 32 bits: the density multiply on native XYZ by the
/// filter's white-normalized `ratio`, Y held when `preserve`, back by
/// `M⁻¹·D`.
fn photo_filter_oracle(
    basis: Basis,
    ratio: [f64; 3],
    d: f64,
    preserve: bool,
    v: [f64; 3],
) -> [f64; 3] {
    let x = apply(basis.native(), v);
    let mut y: [f64; 3] = std::array::from_fn(|c| x[c] * (1.0 - d + d * ratio[c]));
    if preserve && y[1] != 0.0 {
        let k = x[1] / y[1];
        y = y.map(|c| c * k);
    }
    apply(mul(inv(basis.m), basis.d), y)
}

/// The IEC 61966-2-1 decode.
fn srgb_decode(x: f64) -> f64 {
    if x <= 0.04045 {
        x / 12.92
    } else {
        ((x + 0.055) / 1.055).powf(2.4)
    }
}

/// The ratio a `#rrggbb` filter colour takes on `basis`: `M·decode(c/255)`
/// over the D50 white. Every profile here has the sRGB curve.
fn ratio_of(basis: Basis, hex: [u8; 3]) -> [f64; 3] {
    let xyz = apply(basis.m, hex.map(|c| srgb_decode(f64::from(c) / 255.0)));
    std::array::from_fn(|c| xyz[c] / D50[c])
}

// ----------------------------------------------- the test's own ICC read --

fn be32(b: &[u8], at: usize) -> u32 {
    u32::from_be_bytes(b[at..at + 4].try_into().unwrap())
}

fn s15(b: &[u8], at: usize) -> f64 {
    f64::from(be32(b, at) as i32) / 65536.0
}

fn tag<'a>(icc: &'a [u8], sig: &[u8; 4]) -> Option<&'a [u8]> {
    (0..be32(icc, 128) as usize).find_map(|i| {
        let e = 132 + 12 * i;
        (&icc[e..e + 4] == sig).then(|| {
            let (off, len) = (be32(icc, e + 4) as usize, be32(icc, e + 8) as usize);
            &icc[off..off + len]
        })
    })
}

fn xyz_tag(icc: &[u8], sig: &[u8; 4]) -> Option<[f64; 3]> {
    tag(icc, sig).map(|t| std::array::from_fn(|k| s15(t, 8 + 4 * k)))
}

/// `rXYZ/gXYZ/bXYZ/wtpt/chad` read straight from the profile bytes.
fn profile_basis(icc: &[u8]) -> Basis {
    let columns = [b"rXYZ", b"gXYZ", b"bXYZ"].map(|s| xyz_tag(icc, s).expect("a colorant"));
    let chad = tag(icc, b"chad").map(|t| {
        assert_eq!(&t[..4], b"sf32");
        std::array::from_fn(|i| std::array::from_fn(|j| s15(t, 8 + 4 * (3 * i + j))))
    });
    Basis::new(columns, xyz_tag(icc, b"wtpt"), chad)
}

// ------------------------------------------------------------- plumbing --

fn close(got: f64, want: f64, tol: f64) -> bool {
    (got - want).abs() <= tol * want.abs().max(1.0)
}

fn assert_close3(got: [f64; 3], want: [f64; 3], tol: f64, what: &str) {
    assert!(
        (0..3).all(|c| close(got[c], want[c], tol)),
        "{what}: got {got:?}, want {want:?}"
    );
}

/// The test's own RZCB v3 writer (`float_blend_modes.rs`'s): float samples,
/// no coverage, no editable section, offset (0, 0).
fn drcb(w: u32, h: u32, profile: &[u8], pixels: &[[f32; 4]]) -> Vec<u8> {
    let mut out = b"RZCB".to_vec();
    out.extend(3u32.to_le_bytes());
    out.extend(2u32.to_le_bytes());
    out.extend(w.to_le_bytes());
    out.extend(h.to_le_bytes());
    out.extend(0i32.to_le_bytes());
    out.extend(0i32.to_le_bytes());
    out.extend(72f32.to_le_bytes());
    out.extend(72f32.to_le_bytes());
    out.extend((profile.len() as u32).to_le_bytes());
    out.extend(profile);
    out.push(0);
    out.extend(0u32.to_le_bytes());
    out.extend(4f32.to_le_bytes());
    out.extend(0u32.to_le_bytes());
    pixels
        .iter()
        .flatten()
        .for_each(|v| out.extend(v.to_le_bytes()));
    out
}

/// A float document in `profile` whose one raster holds `pixels` as a
/// `pixels.len()` × 1 row at alpha 1.
pub(crate) fn float_row(profile: &[u8], pixels: &[[f32; 3]]) -> PrecisionDocument {
    let samples: Vec<[f32; 4]> = pixels.iter().map(|p| [p[0], p[1], p[2], 1.0]).collect();
    let bytes = drcb(samples.len() as u32, 1, profile, &samples);
    let payload = ClipboardPayload::decode(&bytes, LIMIT, 0).expect("a payload");
    let doc =
        PrecisionDocument::from_clipboard(&payload, LIMIT, 0, None).expect("a float document");
    assert!(doc.is_float());
    doc
}

pub(crate) fn grid_doc(profile: &[u8]) -> PrecisionDocument {
    float_row(profile, &grid())
}

fn description(op: &str, params: &Value) -> String {
    json!({"type":"adjust","evaluation":"precision_v1","op":op,"params":params}).to_string()
}

pub(crate) fn add(
    doc: &PrecisionDocument,
    op: &str,
    params: &Value,
    opacity: f32,
) -> PrecisionDocument {
    doc.add_adjustment("Adjust", &description(op, params), opacity, LIMIT, 0, None)
        .expect("the adjustment is added")
        .expect("the stack changed")
}

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

/// One request through `dr_precision_doc_edit`; `Err` is its refusal.
fn edit_with(
    doc: &PrecisionDocument,
    request: Value,
    input: &[u16],
) -> Result<PrecisionDocument, String> {
    let text = CString::new(request.to_string()).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_edit(
            doc,
            text.as_ptr(),
            if input.is_empty() {
                ptr::null()
            } else {
                input.as_ptr()
            },
            input.len(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    if !err.is_null() {
        assert!(out.is_null());
        return Err(take(err));
    }
    assert!(!out.is_null(), "{request} changed nothing");
    Ok(*unsafe { Box::from_raw(out) })
}

pub(crate) fn render(doc: &PrecisionDocument) -> Vec<[f32; 4]> {
    assert_eq!(doc.float_refusal(), None);
    doc.render(LIMIT, 0, None)
        .expect("the float scene renders")
        .to_rgba32()
        .unwrap()
}

fn bits(pixels: &[[f32; 4]]) -> Vec<[u32; 4]> {
    pixels.iter().map(|p| p.map(f32::to_bits)).collect()
}

/// Renders `op` over the grid on `profile` and checks every patch against
/// `oracle` within `tol`.
fn assert_renders(
    profile: &[u8],
    op: &str,
    params: Value,
    tol: f64,
    oracle: impl Fn([f64; 3]) -> [f64; 3],
) {
    let out = render(&add(&grid_doc(profile), op, &params, 1.0));
    for (patch, got) in grid().iter().zip(&out) {
        let want = oracle(wide(*patch));
        assert_close3(
            [got[0], got[1], got[2]].map(f64::from),
            want,
            tol,
            &format!("{op} {params} on {patch:?}"),
        );
        assert_eq!(got[3], 1.0);
    }
}

// ------------------------------------------------------- the built set --

fn levels_t1() -> (Value, Stage, [Stage; 3]) {
    let params = json!({"black": 20.0 / 255.0, "white": 230.0 / 255.0, "gamma": 1.4,
        "output_black": 10.0 / 255.0, "output_white": 245.0 / 255.0,
        "r": {"white": 220.0 / 255.0}});
    let master = Stage([
        20.0 / 255.0,
        230.0 / 255.0,
        1.4,
        10.0 / 255.0,
        245.0 / 255.0,
    ]);
    let red = Stage([0.0, 220.0 / 255.0, 1.0, 0.0, 1.0]);
    (params, master, [red, UNIT, UNIT])
}

const BW_DEFAULT: [f64; 6] = [0.4, 0.6, 0.4, 0.6, 0.2, 0.8];
const BW_T1: [f64; 6] = [1.2, 0.6, 0.4, 0.6, -0.5, 0.8];
const TINT_B3: Set = (BW_T1, [200, 60, 90]);
const MIXER_IDENTITY: [[f64; 4]; 3] = [
    [1.0, 0.0, 0.0, 0.0],
    [0.0, 1.0, 0.0, 0.0],
    [0.0, 0.0, 1.0, 0.0],
];
const MIXER_A2: [[f64; 4]; 3] = [
    [1.0, 0.0, 0.0, 0.1],
    [0.0, 1.0, 0.0, 0.0],
    [0.0, 0.0, 1.0, 0.0],
];
const MIXER_MONO: [[f64; 4]; 3] = [[0.4, 0.4, 0.2, 0.0]; 3];

/// One parameter set per built op with its oracle, for the shared cases.
type Oracle = Box<dyn Fn([f64; 3]) -> [f64; 3]>;

fn built() -> Vec<(&'static str, Value, Oracle)> {
    let (levels, master, channels) = levels_t1();
    let srgb = profile_basis(IccProfile::srgb().bytes());
    let ratio = ratio_of(srgb, [0xec, 0x8a, 0x00]);
    vec![
        (
            "levels",
            levels,
            Box::new(move |v| levels_oracle(master, channels, v)),
        ),
        (
            "black_and_white",
            json!({"reds": 1.2, "blues": -0.5}),
            Box::new(|v| [bw_oracle(BW_T1, v); 3]),
        ),
        // Take6's B3: its weights with the tint [200, 60, 90].
        (
            "black_and_white",
            tint_params(TINT_B3),
            Box::new(|v| tint32(TINT_B3, v)),
        ),
        (
            "channel_mixer",
            json!({"red": {"r": 1, "g": 0, "b": 0, "constant": 0.1}}),
            Box::new(|v| mixer_oracle(MIXER_A2, v)),
        ),
        (
            "photo_filter",
            json!({"color": "#ec8a00", "density": 0.25, "preserve_luminosity": true}),
            Box::new(move |v| photo_filter_oracle(srgb, ratio, 0.25, true, v)),
        ),
        (
            "bcs",
            json!({"brightness": 20.0 / 255.0, "contrast": 3.0 / 7.0}),
            Box::new(|v| bcs_oracle(20.0 / 255.0, 3.0 / 7.0, 0.0, v)),
        ),
        (
            "bcs",
            json!({"brightness": 0.3, "contrast": 0.05, "saturation": 0.0, "legacy": false}),
            Box::new(|v| bcs_modern_oracle(0.3, 0.05, 0.0, v)),
        ),
        (
            "curves",
            json!({"rgb": [[0, 0], [64, 48], [192, 214], [255, 255]], "r": [[0, 30], [192, 255]]}),
            Box::new(|v| {
                curves_oracle(
                    &CURVES_T1,
                    [&CURVES_I1, &IDENTITY_CURVE, &IDENTITY_CURVE],
                    v,
                )
            }),
        ),
        // Take3's M3: a channel map under a master map.
        (
            "curves",
            map_params(&m1(), Some(&sqrt_map())),
            Box::new(|v| {
                let id = identity();
                curves_map_oracle(&m1(), [&sqrt_map(), &id, &id], v)
            }),
        ),
        // Take3's L4: a 3D table within 0…1 under the link form.
        ("color_lookup", cross_5_params(), Box::new(cross_5_oracle)),
    ]
}

fn tolerance(op: &str) -> f64 {
    if op == "photo_filter" {
        5e-5
    } else {
        1e-5
    }
}

// ---------------------------------------------------------------- tests --

#[test]
fn oracles_reproduce_photoshop_take2() {
    const T: f64 = 2e-3;
    // Levels T1 on sRGB.
    let (_, master, channels) = levels_t1();
    for (v, want) in [
        ([0.02; 3], [-0.0945731, -0.100032, -0.100032]),
        ([16.0; 3], [8.53739, 7.68317, 7.68317]),
        ([-0.25, 0.5, 1.0], [-0.479369, 0.610438, 1.03788]),
    ] {
        assert_close3(levels_oracle(master, channels, v), want, T, "levels T1");
    }
    // Black & White T1 and A1.
    for (v, want) in [
        ([0.02, 0.01, 0.3], 0.0),
        ([2.0, 0.5, 0.25], 2.2),
        ([-0.25, 0.5, 1.0], 0.0),
    ] {
        assert!(close(bw_oracle(BW_T1, v), want, T), "B&W T1 {v:?}");
    }
    assert!(close(bw_oracle(BW_DEFAULT, [4.0, 0.3, 2.0]), 2.46, T));
    // Channel Mixer A2, I2 and T1, then the truncation rule's own rows.
    let m = 1e-6;
    assert!(close(mixer_oracle(MIXER_A2, [0.1; 3])[0], 0.149805, m));
    let mut i2 = MIXER_IDENTITY;
    i2[0][3] = -0.1;
    assert!(close(mixer_oracle(i2, [-0.05; 3])[0], -0.0998047, m));
    assert_close3(
        mixer_oracle(MIXER_MONO, [1.5; 3]),
        [1.49707; 3],
        m,
        "mixer T1",
    );
    let thirds = [[0.33, 0.33, 0.33, 0.0]; 3];
    assert!(close(
        mixer_oracle(thirds, [2.0; 3])[0],
        2.0 * 1011.0 / 1024.0,
        1e-12
    ));
    let mut near = MIXER_IDENTITY;
    near[0] = [0.5, -0.499, 0.0, 0.0];
    assert!(close(
        mixer_oracle(near, [16.0, 16.0, 0.0])[0],
        0.03125,
        1e-12
    ));
    // Photo Filter: Apple's sRGB (Bradford from its D65 wtpt) and ROMM (D50).
    let apple = Basis::new(
        [
            [0.436066, 0.222488, 0.013916],
            [0.385147, 0.716873, 0.097076],
            [0.143066, 0.060608, 0.714096],
        ],
        Some(APPLE_D65),
        None,
    );
    let rgb_ratio = [0.63473, 0.593866, 0.0793];
    let lab_ratio = [0.473671, 0.36632, 0.044935];
    assert_close3(
        photo_filter_oracle(apple, rgb_ratio, 1.0, false, [1.0; 3]),
        [0.998781, 0.533056, 0.0037175],
        T,
        "PF I1 white",
    );
    assert_close3(
        photo_filter_oracle(apple, rgb_ratio, 1.0, false, [2.0, 0.5, 0.25]),
        [1.41239, 0.248344, -0.0318379],
        T,
        "PF I1 over",
    );
    let romm = Basis::new(
        [
            [0.797668, 0.288040, 0.0],
            [0.135193, 0.711884, 0.0],
            [0.031357, 0.0000916, 0.825195],
        ],
        Some(D50),
        None,
    );
    assert_close3(
        photo_filter_oracle(romm, lab_ratio, 1.0, false, [4.0, 0.3, 2.0]),
        [1.96827, -0.0935111, 0.0899053],
        T,
        "PF A1 ROMM",
    );
    assert_close3(
        photo_filter_oracle(apple, lab_ratio, 0.25, true, [0.5, 0.2, 0.1]),
        [0.536673, 0.190353, 0.0875507],
        T,
        "PF T1",
    );
    // bcs against Photoshop's legacy Brightness 20 and Contrast 30.
    assert_close3(
        bcs_oracle(20.0 / 255.0, 0.0, 0.0, [-0.25, 0.5, 1.0]),
        [-0.171569, 0.578431, 1.07843],
        T,
        "bcs brightness",
    );
    let c: f64 = 1.0 / (1.0 - 0.3) - 1.0;
    assert!((c - 3.0 / 7.0).abs() < 1e-12);
    assert_close3(
        bcs_oracle(0.0, c, 0.0, [1.0; 3]),
        [1.21513; 3],
        T,
        "bcs contrast",
    );
    assert_close3(
        bcs_oracle(0.0, c, 0.0, [16.0; 3]),
        [22.6437; 3],
        T,
        "bcs contrast 16",
    );
}

#[test]
fn levels_renders_its_unclamped_signed_power() {
    let srgb = IccProfile::srgb();
    let (params, master, channels) = levels_t1();
    assert_renders(srgb.bytes(), "levels", params, 1e-5, |v| {
        levels_oracle(master, channels, v)
    });
    let low = Stage([0.05, 0.8, 0.6, 0.0, 1.0]);
    assert_renders(
        srgb.bytes(),
        "levels",
        json!({"black": 0.05, "white": 0.8, "gamma": 0.6}),
        1e-5,
        |v| levels_oracle(low, [UNIT; 3], v),
    );
}

#[test]
fn black_and_white_renders_its_floored_gray() {
    let srgb = IccProfile::srgb();
    assert_renders(
        srgb.bytes(),
        "black_and_white",
        json!({"reds": 1.2, "blues": -0.5}),
        1e-5,
        |v| [bw_oracle(BW_T1, v); 3],
    );
    assert_renders(srgb.bytes(), "black_and_white", json!({}), 1e-5, |v| {
        [bw_oracle(BW_DEFAULT, v); 3]
    });
}

#[test]
fn channel_mixer_renders_its_1024_steps_and_half_constant() {
    let srgb = IccProfile::srgb();
    assert_renders(
        srgb.bytes(),
        "channel_mixer",
        json!({"red": {"r": 1, "g": 0, "b": 0, "constant": 0.1}}),
        1e-5,
        |v| mixer_oracle(MIXER_A2, v),
    );
    assert_renders(
        srgb.bytes(),
        "channel_mixer",
        json!({"monochrome": true, "gray": {"r": 0.4, "g": 0.4, "b": 0.2}}),
        1e-5,
        |v| mixer_oracle(MIXER_MONO, v),
    );
}

#[test]
fn photo_filter_renders_in_each_profiles_native_basis() {
    let builtin = IccProfile::srgb();
    let spelled = srgb_with_media_white(APPLE_D65);
    let p3 = IccProfile::display_p3();
    let by_chad = profile_basis(builtin.bytes());
    let by_wtpt = profile_basis(&spelled);
    assert!(
        tag(builtin.bytes(), b"chad").is_some(),
        "the builtin sRGB carries chad"
    );
    assert!(
        tag(&spelled, b"chad").is_none(),
        "the spelling carries only wtpt"
    );
    // The two sRGB spellings reach one native basis, and its red column is
    // IEC 61966-2-1's. Within 3e-4, not tighter: the builtin's colorant
    // tags are its own rounding of the published primaries (blue Z
    // 0.713928) and the spelling's are Apple's (0.714096), 2.4e-4 apart
    // before any adaptation.
    let (a, b) = (by_chad.native(), by_wtpt.native());
    for i in 0..3 {
        for j in 0..3 {
            assert!((a[i][j] - b[i][j]).abs() <= 3e-4, "{a:?} vs {b:?}");
        }
    }
    for (got, want) in [a[0][0], a[1][0], a[2][0]]
        .iter()
        .zip([0.4124, 0.2126, 0.0193])
    {
        assert!((got - want).abs() <= 2e-4, "red column {got} vs {want}");
    }
    for (bytes, basis) in [
        (builtin.bytes().to_vec(), by_chad),
        (spelled.clone(), by_wtpt),
        (p3.bytes().to_vec(), profile_basis(p3.bytes())),
    ] {
        let hex = [0xec, 0x8a, 0x00];
        let ratio = ratio_of(basis, hex);
        assert_renders(
            &bytes,
            "photo_filter",
            json!({"color": "#ec8a00", "density": 1.0, "preserve_luminosity": false}),
            5e-5,
            |v| photo_filter_oracle(basis, ratio, 1.0, false, v),
        );
        assert_renders(
            &bytes,
            "photo_filter",
            json!({"color": "#ec8a00", "density": 0.25, "preserve_luminosity": true}),
            5e-5,
            |v| photo_filter_oracle(basis, ratio, 0.25, true, v),
        );
    }
}

#[test]
fn bcs_renders_its_unclamped_affine() {
    let srgb = IccProfile::srgb();
    assert_renders(
        srgb.bytes(),
        "bcs",
        json!({"brightness": 20.0 / 255.0, "contrast": 3.0 / 7.0, "saturation": 0.25}),
        1e-5,
        |v| bcs_oracle(20.0 / 255.0, 3.0 / 7.0, 0.25, v),
    );
}

/// Stored 4.0 and −0.25 pass a modern layer's power stages
/// unchanged, and a saturation-only edit leaves an above-1 grey above 1.
#[test]
fn bcs_modern_keeps_values_outside_the_unit_range() {
    let srgb = IccProfile::srgb();
    let doc = float_row(srgb.bytes(), &[[4.0, -0.25, 0.5], [2.5, 2.5, 2.5]]);
    let modern = json!({"brightness": 0.3, "contrast": 0.05, "saturation": 0.0, "legacy": false});
    let out = render(&add(&doc, "bcs", &modern, 1.0));
    let want = bcs_modern_oracle(0.3, 0.05, 0.0, [4.0, -0.25, 0.5]);
    assert_eq!([out[0][0], out[0][1]], [4.0, -0.25], "{out:?}");
    assert_close3(wide([out[0][0], out[0][1], out[0][2]]), want, 1e-5, "mid");
    let saturation = json!({"saturation": 0.6, "legacy": false});
    let out = render(&add(&doc, "bcs", &saturation, 1.0));
    assert!((0..3).all(|c| (out[1][c] - 2.5).abs() < 1e-5), "{out:?}");
}

/// Exposure's take2 sets as `(exposure, offset, gamma)`.
const EXPOSURE_X1: [f64; 3] = [1.0, 0.0, 1.0];
const EXPOSURE_X2: [f64; 3] = [0.5, 0.02, 0.8];
const EXPOSURE_X3: [f64; 3] = [-1.0, -0.05, 1.3];

#[test]
fn exposure_oracle_reproduces_photoshop_take2() {
    const T: f64 = 2e-3;
    let [e, o, g] = EXPOSURE_X2;
    assert_close3(
        exposure_oracle(e, o, g, [16.0; 3]),
        [49.4053; 3],
        T,
        "exposure X2 over",
    );
    assert_close3(
        exposure_oracle(e, o, g, [-0.25, 0.5, 1.0]),
        [-0.253488, 0.671425, 1.56952],
        T,
        "exposure X2 negative",
    );
    let [e, o, g] = EXPOSURE_X3;
    assert_close3(
        exposure_oracle(e, o, g, [2.0, 0.5, 0.25]),
        [0.961312, 0.289955, 0.136352],
        T,
        "exposure X3",
    );
}

/// Both evaluations take the one per-channel form on every profile: the
/// reference-basis matrices of the 8/16-bit `document_v2` path are not run.
#[test]
fn exposure_renders_its_reciprocal_power_per_channel_in_both_evaluations() {
    for profile in [IccProfile::srgb(), IccProfile::display_p3()] {
        for evaluation in ["document_v2", "legacy_srgb"] {
            for [e, o, g] in [EXPOSURE_X1, EXPOSURE_X2, EXPOSURE_X3] {
                assert_renders(
                    profile.bytes(),
                    "exposure",
                    json!({"exposure": e, "offset": o, "gamma": g, "evaluation": evaluation}),
                    1e-5,
                    |v| exposure_oracle(e, o, g, v),
                );
            }
        }
    }
}

/// Photoshop's take2 curves sets: T1 (4 points), I2 (both ends inside),
/// I1 (a first point at 0 above 0), and the 16-bit T1 on the linear twin.
#[test]
fn curves_oracle_reproduces_photoshop_take2() {
    const T: f64 = 2e-3;
    let identity: [&[(f64, f64)]; 3] = [&IDENTITY_CURVE; 3];
    for (v, want) in [
        ([16.0; 3], [8.45499; 3]),
        ([-0.25, 0.5, 1.0], [-0.151694, 0.515787, 1.0]),
        ([0.5; 3], [0.515787; 3]),
    ] {
        assert_close3(curves_oracle(&CURVES_T1, identity, v), want, T, "curves T1");
    }
    for (v, want) in [
        ([1.0; 3], [1.49219; 3]),
        ([-0.25, 0.5, 1.0], [0.0, 0.496094, 1.49219]),
    ] {
        assert_close3(curves_oracle(&CURVES_I2, identity, v), want, T, "curves I2");
    }
    assert!(close(
        curves_oracle(&CURVES_I1, identity, [-0.25, 0.5, 1.0])[0],
        -0.175322,
        T
    ));
    // PS16 T1 on the linear twin: inside [0, 1] the 16-bit curve is the
    // same spline.
    let t1 = Natural::new(&CURVES_T1);
    assert!(close(t1.clamped(127.5) / 255.0, 0.51577, T));
}

#[test]
fn curves_renders_its_natural_spline_continued_past_its_ends() {
    let srgb = IccProfile::srgb();
    let identity: [&[(f64, f64)]; 3] = [&IDENTITY_CURVE; 3];
    for (params, master) in [
        (
            json!({"rgb": [[0, 0], [64, 48], [192, 214], [255, 255]]}),
            &CURVES_T1[..],
        ),
        (json!({"rgb": [[0, 30], [192, 255]]}), &CURVES_I1[..]),
        (json!({"rgb": [[64, 0], [192, 255]]}), &CURVES_I2[..]),
        (
            json!({"rgb": [[0, 0], [192, 255]]}),
            &[(0.0, 0.0), (192.0, 255.0)][..],
        ),
        (
            json!({"rgb": [[64, 0], [255, 255]]}),
            &[(64.0, 0.0), (255.0, 255.0)][..],
        ),
    ] {
        assert_renders(srgb.bytes(), "curves", params, 1e-5, |v| {
            curves_oracle(master, identity, v)
        });
    }
    // Each channel's curve runs before the master's.
    let green = [(0.0, 10.0), (100.0, 160.0), (200.0, 180.0), (255.0, 250.0)];
    assert_renders(
        srgb.bytes(),
        "curves",
        json!({"rgb": [[0, 0], [64, 48], [192, 214], [255, 255]],
               "r": [[64, 0], [192, 255]],
               "g": [[0, 10], [100, 160], [200, 180], [255, 250]]}),
        1e-5,
        |v| curves_oracle(&CURVES_T1, [&CURVES_I2, &green, &IDENTITY_CURVE], v),
    );
}

/// At 16 bits the same spline, flat past its ends and clamped to [0, 1]:
/// take2's PS16 T1 on the linear twin lands (0.5)³ on 0.51577.
#[test]
fn curves_at_sixteen_bits_is_the_same_natural_spline() {
    let srgb = IccProfile::srgb();
    let codes: Vec<u16> = vec![0, 1000, 16384, 32768, 50000, 65535];
    let pixels: Vec<[u16; 4]> = codes.iter().map(|&c| [c, c, c, 65535]).collect();
    let image =
        PrecisionImage::from_rgba16(codes.len() as u32, 1, pixels, srgb.clone(), None).unwrap();
    let photo = Layer::raster("Photo", image, (0, 0), 1.0, true).unwrap();
    let doc = PrecisionDocument::from_parts(
        codes.len() as u32,
        1,
        srgb,
        Metadata::default(),
        Resolution::default(),
        vec![photo],
    )
    .unwrap();
    let params = json!({"rgb": [[0, 0], [64, 48], [192, 214], [255, 255]]});
    let out = doc
        .add_adjustment("T1", &description("curves", &params), 1.0, LIMIT, 0, None)
        .unwrap()
        .unwrap()
        .render(LIMIT, 0, None)
        .unwrap()
        .to_rgba16()
        .unwrap();
    let t1 = Natural::new(&CURVES_T1);
    for (code, px) in codes.iter().zip(&out) {
        let want = t1.clamped(f64::from(*code) / 65535.0 * 255.0) / 255.0;
        for c in &px[..3] {
            let got = f64::from(*c) / 65535.0;
            assert!(
                (got - want).abs() <= 1.0 / 65535.0,
                "{code}: {got} vs {want}"
            );
        }
        if *code == 32768 {
            assert!(close(want, 0.51577, 2e-3), "{want}");
        }
    }
}

/// Opacity 0.5 under a mask ramp, Multiply: the adjusted colour pushed
/// through the mode as the SOURCE (`cb·a`, unclamped) and mixed in by
/// `0.5·coverage`; then the same pair nested in an isolated group.
#[test]
fn each_op_composites_with_opacity_mask_multiply_and_inside_an_isolated_group() {
    let srgb = IccProfile::srgb();
    let patches = grid();
    let n = patches.len();
    let ramp: Vec<u16> = (0..n).map(|i| (i * 65535 / (n - 1)) as u16).collect();
    for (op, params, oracle) in built() {
        let tol = tolerance(op);
        let doc = add(&grid_doc(srgb.bytes()), op, &params, 0.5);
        let masked = edit_with(
            &doc,
            json!({"op":"set_mask","layer":1,"width":n,"height":1,"x":0,"y":0,"linked":true}),
            &ramp,
        )
        .expect("a mask");
        let moded = edit_with(
            &masked,
            json!({"op":"layer_blend","version":1,"layer":1,"blend_mode":MULTIPLY}),
            &[],
        )
        .expect("Multiply");
        let grouped = edit_with(
            &moded,
            json!({"op":"group_layers","layers":[0, 1],"name":"G","pass_through":false}),
            &[],
        )
        .expect("grouped");
        for (what, scene) in [("layer", &moded), ("group", &grouped)] {
            let out = render(scene);
            for (i, got) in out.iter().enumerate() {
                let cb = wide(patches[i]);
                let a = oracle(cb);
                let k = 0.5 * f64::from(ramp[i]) / 65535.0;
                let want: [f64; 3] = std::array::from_fn(|c| cb[c] + (cb[c] * a[c] - cb[c]) * k);
                assert_close3(
                    [got[0], got[1], got[2]].map(f64::from),
                    want,
                    tol,
                    &format!("{op} {what} patch {i}"),
                );
                assert_eq!(got[3], 1.0, "{op} {what} patch {i}");
            }
        }
    }
}

#[test]
fn each_op_renders_regionally_and_merges_down_as_its_projection() {
    let srgb = IccProfile::srgb();
    for (op, params, _) in built() {
        let doc = add(&grid_doc(srgb.bytes()), op, &params, 0.75);
        let full = render(&doc);
        let window = RegionRequest {
            x: 7,
            y: 0,
            width: 20,
            height: 1,
            preview_divisor: 1,
        };
        let region = doc
            .render_region(window, LIMIT, 0, None)
            .expect("the region renders")
            .to_rgba32()
            .unwrap();
        assert_eq!(
            bits(&region),
            bits(&full[7..27]),
            "{op}: the region is the window"
        );
        let merged = edit_with(&doc, json!({"op":"merge_down","layer":1}), &[]).expect("merged");
        assert_eq!(merged.layer_count(), 1);
        assert_eq!(
            bits(&render(&merged)),
            bits(&full),
            "{op}: merge down bakes the projection"
        );
    }
}

#[test]
fn an_identity_description_renders_the_backdrop_bit_for_bit() {
    let srgb = IccProfile::srgb();
    let base = grid_doc(srgb.bytes());
    let reference = render(&base);
    // Color Lookup's unloaded table: the 1D 2-node identity the sheet writes
    // before a file is chosen (`no_lut`), at any strength. The grid holds
    // over-range and negative patches, which pass through too.
    let no_lut: Vec<u8> = [0.0f32, 0.0, 0.0, 1.0, 1.0, 1.0]
        .iter()
        .flat_map(|v| v.to_le_bytes())
        .collect();
    // Black & White has no identity description: any weights make a grey.
    for (op, params) in [
        ("levels", json!({})),
        ("channel_mixer", json!({})),
        ("photo_filter", json!({"density": 0.0})),
        ("bcs", json!({})),
        ("curves", json!({})),
        ("curves", json!({"rgb": [[0, 0], [128, 128], [255, 255]]})),
        (
            "color_lookup",
            json!({"kind": "1d", "size": 2, "table": base64(&no_lut)}),
        ),
        (
            "color_lookup",
            json!({"kind": "1d", "size": 2, "table": base64(&no_lut), "strength": 0.5}),
        ),
    ] {
        let out = render(&add(&base, op, &params, 1.0));
        assert_eq!(bits(&out), bits(&reference), "{op}");
    }
}

#[test]
fn each_op_survives_a_native_round_trip_bit_for_bit() {
    let srgb = IccProfile::srgb();
    for (op, params, _) in built() {
        let doc = add(&grid_doc(srgb.bytes()), op, &params, 0.5);
        let bytes = doc.encode_native(LIMIT, 0, None).expect("it saves");
        let reopened = PrecisionDocument::parse_native(&bytes, LIMIT, 0, None).expect("it reopens");
        assert!(reopened.is_float());
        assert_eq!(
            reopened.adjustment_description(1),
            doc.adjustment_description(1),
            "{op}"
        );
        assert_eq!(bits(&render(&reopened)), bits(&render(&doc)), "{op}");
        assert_eq!(
            reopened.encode_native(LIMIT, 0, None).unwrap(),
            bytes,
            "{op}: re-encoding is byte-identical"
        );
    }
}

#[test]
fn a_levels_overflow_refuses_as_nonfinite() {
    let doc = add(
        &float_row(IccProfile::srgb().bytes(), &[[65504.0, 1.0, 1.0]]),
        "levels",
        &json!({"gamma": 0.1}),
        1.0,
    );
    assert_eq!(doc.float_refusal(), None, "levels is admitted");
    assert_eq!(
        doc.render(LIMIT, 0, None)
            .err()
            .expect("the render refuses"),
        "high-depth adjustment produced nonfinite RGB"
    );
}

pub(crate) fn base64(bytes: &[u8]) -> String {
    const DIGITS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut encoded = String::new();
    for chunk in bytes.chunks(3) {
        let value = (u32::from(chunk[0]) << 16)
            | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8)
            | u32::from(*chunk.get(2).unwrap_or(&0));
        for digit in 0..4 {
            encoded.push(if digit > chunk.len() {
                '='
            } else {
                DIGITS[((value >> (18 - 6 * digit)) & 63) as usize] as char
            });
        }
    }
    encoded
}

/// Every adjustment the float mode does not evaluate (decision A),
/// with the feature its refusal names.
fn refused_cases() -> Vec<(&'static str, Value, &'static str)> {
    vec![
        ("invert", json!({}), "the invert adjustment"),
        (
            "shadows_highlights",
            json!({"shadows": {"amount": 0.35}}),
            "the shadows_highlights adjustment",
        ),
        (
            "white_balance",
            json!({"temperature": 5000, "evaluation": "legacy_srgb"}),
            "the white_balance adjustment with legacy evaluation",
        ),
    ]
}

/// The adjustments admitted by decision A: closed forms, display forms
/// and the Color Lookup tables outside the measured device-link form.
fn admitted_487_cases() -> Vec<(&'static str, Value)> {
    let table = |entries: &[f32]| -> String {
        base64(
            &entries
                .iter()
                .flat_map(|v| v.to_le_bytes())
                .collect::<Vec<u8>>(),
        )
    };
    // A 2-node 3D table whose node holds `lo` or `hi` per axis, red fastest.
    let cube = |lo: f32, hi: f32| -> String {
        let at = |i: usize| if i == 0 { lo } else { hi };
        let mut entries = Vec::new();
        for b in 0..2 {
            for g in 0..2 {
                for r in 0..2 {
                    entries.extend([at(r), at(g), at(b)]);
                }
            }
        }
        table(&entries)
    };
    let stops =
        json!({"stops":[{"position":0,"color":"#000000"},{"position":1,"color":"#ffffff"}]});
    vec![
        ("hue_saturation", json!({"saturation": 0.25})),
        (
            "color_lookup",
            json!({"kind": "1d", "size": 2, "table": table(&[0.0, 0.0, 0.0, 0.5, 0.5, 0.5]),
                "strength": 0.5}),
        ),
        // An abstract look renders as its table (`float_lookup_tint_map.rs`);
        // a paste re-bakes it, which a stand-in profile cannot survive.
        (
            "color_lookup",
            json!({"kind": "3d", "size": 2, "table": cube(0.0, 1.0),
                "domain_max": [2.0, 2.0, 2.0]}),
        ),
        (
            "color_lookup",
            json!({"kind": "3d", "size": 2, "table": cube(0.0, 2.0)}),
        ),
        ("threshold", json!({"level": 0.5})),
        ("posterize", json!({"levels": 4})),
        ("vibrance", json!({"vibrance": 0.3})),
        ("selective_color", json!({"reds": {"c": -0.3, "y": 0.2}})),
        ("gradient_map", json!({"gradient": stops, "dither": false})),
        ("hue_rotate", json!({"degrees": 30})),
        ("grayscale", json!({})),
        ("sepia", json!({})),
        ("replace_color", json!({"colors": ["#808080"], "hue": 30})),
    ]
}

pub(crate) fn sentence(feature: &str) -> String {
    format!("{feature} is unavailable in a 32-bit float document")
}

/// `dr_precision_doc_paste_layers` above record 0; `Err` is its refusal.
fn paste_layers(
    doc: &PrecisionDocument,
    clip: &PrecisionDocument,
) -> Result<PrecisionDocument, String> {
    let (mut report, mut err) = (ptr::null_mut(), ptr::null_mut());
    let out = unsafe {
        dr_precision_doc_paste_layers(
            doc,
            clip,
            0,
            0,
            0,
            0,
            false,
            LIMIT,
            0,
            &mut report,
            &mut err,
        )
    };
    if !err.is_null() {
        assert!(out.is_null());
        assert!(report.is_null(), "no report on a refusal");
        return Err(take(err));
    }
    take(report);
    Ok(*unsafe { Box::from_raw(out) })
}

fn sixteen_bit_clip(profile: &Arc<IccProfile>, adjustment: Layer) -> PrecisionDocument {
    let image = PrecisionImage::from_rgba16(
        4,
        1,
        vec![[20000, 40000, 60000, 65535]; 4],
        profile.clone(),
        None,
    )
    .unwrap();
    let photo = Layer::raster("Photo", image, (0, 0), 1.0, true).unwrap();
    PrecisionDocument::from_parts(
        4,
        1,
        profile.clone(),
        Metadata::default(),
        Resolution::default(),
        vec![photo, adjustment],
    )
    .unwrap()
}

#[test]
fn invert_shadows_highlights_and_legacy_white_balance_refuse_by_name_and_the_rest_add() {
    let srgb = IccProfile::srgb();
    let dest = float_row(srgb.bytes(), &[[0.5, 0.25, 2.0]; 4]);
    for (op, params, feature) in refused_cases() {
        let added = add(&dest, op, &params, 1.0);
        assert_eq!(added.float_refusal(), Some(sentence(feature)), "{op}");
        assert_eq!(
            added.render(LIMIT, 0, None).err().as_deref(),
            Some(sentence(feature).as_str()),
            "{op}: the render refuses too"
        );
        let layer =
            Layer::adjustment("Refused", &description(op, &params), 1.0, true, &srgb).unwrap();
        assert_eq!(
            paste_layers(&dest, &sixteen_bit_clip(&srgb, layer))
                .err()
                .as_deref(),
            Some(
                format!(
                    "the copied layer “Refused” cannot be pasted into a 32-bit float document: {}",
                    sentence(feature)
                )
                .as_str()
            ),
            "{op}"
        );
    }
    // The built set and the admitted forms paste from the same clip shape,
    // and the admitted forms add and render.
    for (op, params) in admitted_487_cases() {
        let added = add(&dest, op, &params, 1.0);
        assert_eq!(added.float_refusal(), None, "{op} {params}");
        render(&added);
        let layer =
            Layer::adjustment("Built", &description(op, &params), 1.0, true, &srgb).unwrap();
        let out = paste_layers(&dest, &sixteen_bit_clip(&srgb, layer)).expect(op);
        assert_eq!(out.float_refusal(), None, "{op}");
    }
    for (op, params, _) in built() {
        let layer =
            Layer::adjustment("Built", &description(op, &params), 1.0, true, &srgb).unwrap();
        let out = paste_layers(&dest, &sixteen_bit_clip(&srgb, layer)).expect(op);
        assert_eq!(out.float_refusal(), None, "{op}");
    }
}

/// Color Balance renders at 32 bits under its measured form: every
/// setting is admitted, and a patch outside [0, 1] continues along the
/// curve's stage end slopes, exactly `apply_linear`.
#[test]
fn color_balance_renders_in_a_float_document() {
    let srgb = IccProfile::srgb();
    let doc = grid_doc(srgb.bytes());
    for params in [
        json!({"shadows": {"cyan_red": -0.4, "magenta_green": 0.3},
               "midtones": {"cyan_red": 0.2, "magenta_green": -0.5, "yellow_blue": 0.8},
               "highlights": {"cyan_red": 0.6, "yellow_blue": -0.7},
               "preserve_luminosity": false}),
        json!({"shadows": {"cyan_red": -0.3, "magenta_green": 0.25, "yellow_blue": 0.1},
               "midtones": {"cyan_red": -0.15, "magenta_green": 0.4, "yellow_blue": -0.6},
               "highlights": {"cyan_red": 0.2, "magenta_green": -0.45, "yellow_blue": 0.35}}),
    ] {
        let added = add(&doc, "color_balance", &params, 1.0);
        assert_eq!(added.float_refusal(), None, "{params}");
        let kernel = PrecisionAdjustment::parse(&description("color_balance", &params), &srgb)
            .expect("the description parses");
        let out = render(&added);
        for (patch, got) in grid().iter().zip(&out) {
            let want = kernel.apply_linear(*patch, (0, 0)).expect("admitted");
            assert_eq!(
                bits(&[*got]),
                bits(&[[want[0], want[1], want[2], 1.0]]),
                "{params} on {patch:?}"
            );
        }
    }
    let params = json!({"midtones": {"cyan_red": 0.4}});
    let layer = Layer::adjustment(
        "Color Balance",
        &description("color_balance", &params),
        1.0,
        true,
        &srgb,
    )
    .unwrap();
    let dest = float_row(srgb.bytes(), &[[0.5, 0.25, 2.0]; 4]);
    let out = paste_layers(&dest, &sixteen_bit_clip(&srgb, layer)).expect("it pastes");
    assert_eq!(out.float_refusal(), None);
}

#[test]
fn hue_saturation_colorize_lightness_and_hue_render_in_a_float_document() {
    let srgb = IccProfile::srgb();
    let doc = grid_doc(srgb.bytes());
    for params in [
        json!({"colorize": true, "colorize_hue": 310, "colorize_saturation": 0.8,
               "colorize_lightness": -0.65}),
        json!({"lightness": 0.33}),
        json!({"lightness": -0.8}),
        json!({"hue": -55}),
    ] {
        let added = add(&doc, "hue_saturation", &params, 1.0);
        assert_eq!(added.float_refusal(), None, "{params}");
        let kernel = PrecisionAdjustment::parse(&description("hue_saturation", &params), &srgb)
            .expect("the description parses");
        let out = render(&added);
        for (patch, got) in grid().iter().zip(&out) {
            let want = kernel.apply_linear(*patch, (0, 0)).expect("admitted");
            assert_eq!(
                bits(&[*got]),
                bits(&[[want[0], want[1], want[2], 1.0]]),
                "{params} on {patch:?}"
            );
        }
    }
    let colorize = json!({"colorize": true, "colorize_hue": 75, "colorize_saturation": 0.4});
    let layer = Layer::adjustment(
        "Colorize",
        &description("hue_saturation", &colorize),
        1.0,
        true,
        &srgb,
    )
    .unwrap();
    let dest = float_row(srgb.bytes(), &[[0.5, 0.25, 2.0]; 4]);
    let out = paste_layers(&dest, &sixteen_bit_clip(&srgb, layer)).expect("a Colorize pastes");
    assert_eq!(out.float_refusal(), None);
}
