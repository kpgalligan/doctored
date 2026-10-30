//! Free Transform of the selected pixels, black-box through both FFI
//! seams: the byte `dr_doc_selected_pixels_edit` / `_plan` and the typed
//! `dr_precision_doc_edit` / `_plan` with `{"op":"transform_selected",…}`,
//! plus `dr_selection_u16`'s new `perspective` kind.
//!
//! Every pixel case runs at 8 AND 16 bits against an independent f64 oracle
//! written here from the measured Photoshop contract: the float is the
//! layer × coverage in PREMULTIPLIED form, inverse-mapped through the
//! transform (nearest = the covering pixel; bilinear = the triangle kernel
//! over pixel centres, transparent outside the canvas), unpremultiplied and
//! clamped, then composited W3C source-over onto the `a·(1−c)` hole. The
//! homography oracle solves its own 8×8 system. Fixtures stay away from .5
//! ties; the tolerance is ≤1 code unless a case says exact.

use crate::common;

use std::ffi::CString;
use std::ptr;
use std::sync::Arc;

use common::selected_pixels::*;
use doctored_core::doc::DrDocument;
use doctored_core::doc_lock::{LOCK_ALL, LOCK_PIXELS, LOCK_POSITION, LOCK_TRANSPARENCY};
use doctored_core::doc_mask::MaskProperties;
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_selection_u16::{dr_selection_u16, dr_selection_u16_plan};
use doctored_core::job_control::DrJobControl;
use doctored_core::precision_capability::capability;
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer};
use doctored_core::precision_mask::{PrecisionMask, PrecisionPlane};
use image::{GrayImage, Luma, RgbaImage};
use serde_json::{json, Value};

const EMPTY: &str = "The selection holds no pixels to transform.";

fn affine(layer: usize, m: [f64; 6], filter: &str) -> String {
    json!({"op":"transform_selected","layer":layer,"matrix":m,"filter":filter}).to_string()
}

fn quad(layer: usize, rect: [f64; 4], q: [f64; 8], filter: &str) -> String {
    json!({"op":"transform_selected","layer":layer,"source_rect":rect,"quad":q,"filter":filter})
        .to_string()
}

fn moved(layer: usize, dx: i32, dy: i32) -> String {
    json!({"op":"move_selected","layer":layer,"dx":dx,"dy":dy}).to_string()
}

/// The request with `"duplicate": flag` (⌥⌘T).
fn duplicated(text: &str, flag: bool) -> String {
    let mut value: Value = serde_json::from_str(text).unwrap();
    value["duplicate"] = json!(flag);
    value.to_string()
}

impl Doc {
    fn run(&self, text: &str, coverage: &[u16]) -> Doc {
        self.edit(text, coverage)
            .expect("the transform runs")
            .expect("the transform changes something")
    }
}

/// Maps a rect's corners (TL, TR, BR, BL) through an affine.
fn corners_of(rect: [f64; 4], m: [f64; 6]) -> [f64; 8] {
    let [x, y, w, h] = rect;
    let p = |px: f64, py: f64| (m[0] * px + m[2] * py + m[4], m[1] * px + m[3] * py + m[5]);
    let (a, b, c, d) = (p(x, y), p(x + w, y), p(x + w, y + h), p(x, y + h));
    [a.0, a.1, b.0, b.1, c.0, c.1, d.0, d.1]
}

// ----------------------------------------------------------------- oracle --

/// Canvas point → continuous source point, `None` beyond the horizon.
type Inverse = Box<dyn Fn(f64, f64) -> Option<(f64, f64)>>;

fn affine_inverse(m: [f64; 6]) -> Inverse {
    let det = m[0] * m[3] - m[1] * m[2];
    Box::new(move |x, y| {
        let (dx, dy) = (x - m[4], y - m[5]);
        Some((
            (m[3] * dx - m[2] * dy) / det,
            (-m[1] * dx + m[0] * dy) / det,
        ))
    })
}

/// The homography taking `rect` onto `quad`, solved independently (DLT with
/// h8 = 1, Gaussian elimination), then inverted exactly.
fn homography_inverse(rect: [f64; 4], q: [f64; 8]) -> Inverse {
    let [x, y, w, h] = rect;
    let src = [(x, y), (x + w, y), (x + w, y + h), (x, y + h)];
    let mut a = [[0.0f64; 9]; 8];
    for (i, &(sx, sy)) in src.iter().enumerate() {
        let (dx, dy) = (q[2 * i], q[2 * i + 1]);
        a[2 * i] = [sx, sy, 1.0, 0.0, 0.0, 0.0, -sx * dx, -sy * dx, dx];
        a[2 * i + 1] = [0.0, 0.0, 0.0, sx, sy, 1.0, -sx * dy, -sy * dy, dy];
    }
    for col in 0..8 {
        let pivot = (col..8)
            .max_by(|&i, &j| a[i][col].abs().total_cmp(&a[j][col].abs()))
            .unwrap();
        a.swap(col, pivot);
        for row in 0..8 {
            if row != col {
                let f = a[row][col] / a[col][col];
                let pivot_row = a[col];
                for (cell, p) in a[row].iter_mut().zip(pivot_row).skip(col) {
                    *cell -= f * p;
                }
            }
        }
    }
    let mut hm = [0.0; 9];
    for i in 0..8 {
        hm[i] = a[i][8] / a[i][i];
    }
    hm[8] = 1.0;
    let det = hm[0] * (hm[4] * hm[8] - hm[5] * hm[7]) - hm[1] * (hm[3] * hm[8] - hm[5] * hm[6])
        + hm[2] * (hm[3] * hm[7] - hm[4] * hm[6]);
    let adj = [
        hm[4] * hm[8] - hm[5] * hm[7],
        hm[2] * hm[7] - hm[1] * hm[8],
        hm[1] * hm[5] - hm[2] * hm[4],
        hm[5] * hm[6] - hm[3] * hm[8],
        hm[0] * hm[8] - hm[2] * hm[6],
        hm[2] * hm[3] - hm[0] * hm[5],
        hm[3] * hm[7] - hm[4] * hm[6],
        hm[1] * hm[6] - hm[0] * hm[7],
        hm[0] * hm[4] - hm[1] * hm[3],
    ];
    let inv: Vec<f64> = adj.iter().map(|v| v / det).collect();
    Box::new(move |x, y| {
        let w = inv[6] * x + inv[7] * y + inv[8];
        if w <= 0.0 {
            return None;
        }
        Some((
            (inv[0] * x + inv[1] * y + inv[2]) / w,
            (inv[3] * x + inv[4] * y + inv[5]) / w,
        ))
    })
}

#[derive(Clone, Copy, PartialEq)]
enum Kernel {
    Nearest,
    Bilinear,
}

struct Oracle<'a> {
    depth: Depth,
    canvas: (u32, u32),
    plane: &'a Plane,
    coverage: &'a [u16],
    /// `false` is the ⌥⌘T duplicate: the original keeps its alpha,
    /// so the hole's coverage is 0 and only the landing composites.
    hole: bool,
}

impl Oracle<'_> {
    fn on(&self, x: i64, y: i64) -> bool {
        x >= 0 && y >= 0 && x < i64::from(self.canvas.0) && y < i64::from(self.canvas.1)
    }
    fn cov(&self, x: i64, y: i64) -> f64 {
        if !self.on(x, y) {
            return 0.0;
        }
        f64::from(self.coverage[y as usize * self.canvas.0 as usize + x as usize]) / 65535.0
    }
    fn unit(&self, x: i64, y: i64) -> [f64; 4] {
        self.plane
            .at(x, y)
            .unwrap_or_default()
            .map(|v| f64::from(v) / self.depth.max())
    }
    /// The premultiplied float at an on-canvas source pixel.
    fn float(&self, x: i64, y: i64) -> [f64; 4] {
        if !self.on(x, y) {
            return [0.0; 4];
        }
        let p = self.unit(x, y);
        let a = p[3] * self.cov(x, y);
        [p[0] * a, p[1] * a, p[2] * a, a]
    }
    /// The float landing at canvas pixel (x, y), straight and clamped.
    fn landed(&self, inverse: &Inverse, kernel: Kernel, x: i64, y: i64) -> Option<[f64; 4]> {
        let (u, v) = inverse(x as f64 + 0.5, y as f64 + 0.5)?;
        let acc = match kernel {
            Kernel::Nearest => self.float(u.floor() as i64, v.floor() as i64),
            Kernel::Bilinear => {
                let (kx, ky) = ((u - 0.5).floor(), (v - 0.5).floor());
                let (fx, fy) = (u - 0.5 - kx, v - 0.5 - ky);
                let mut acc = [0.0; 4];
                for (dy, wy) in [(0, 1.0 - fy), (1, fy)] {
                    for (dx, wx) in [(0, 1.0 - fx), (1, fx)] {
                        let s = self.float(kx as i64 + dx, ky as i64 + dy);
                        for k in 0..4 {
                            acc[k] += s[k] * wx * wy;
                        }
                    }
                }
                acc
            }
        };
        if acc[3] <= 0.0 {
            return None;
        }
        let a = acc[3];
        Some([
            (acc[0] / a).clamp(0.0, 1.0),
            (acc[1] / a).clamp(0.0, 1.0),
            (acc[2] / a).clamp(0.0, 1.0),
            a.clamp(0.0, 1.0),
        ])
    }
    /// The expected value of every pixel of `extent` (x, y, w, h); `None`
    /// keeps the stored bits.
    fn expect(
        &self,
        inverse: &Inverse,
        kernel: Kernel,
        extent: (i64, i64, u32, u32),
    ) -> Vec<Option<[f64; 4]>> {
        let mut out = Vec::new();
        for y in extent.1..extent.1 + i64::from(extent.3) {
            for x in extent.0..extent.0 + i64::from(extent.2) {
                let landing = if self.on(x, y) {
                    self.landed(inverse, kernel, x, y).map(|p| (p, 1.0))
                } else {
                    None
                };
                let c = if self.hole { self.cov(x, y) } else { 0.0 };
                out.push(oracle_pixel(self.unit(x, y), c, landing));
            }
        }
        out
    }
}

/// `exact` accepts only the correctly rounded code (either side within 0.05
/// of a tie); otherwise the got code may be one away.
fn check(
    depth: Depth,
    before: &Plane,
    got: &Plane,
    expected: &[Option<[f64; 4]>],
    exact: bool,
    what: &str,
) {
    let max = depth.max();
    let slack = if exact { 0.55 } else { 1.5 };
    assert_eq!(got.px.len(), expected.len(), "{what} {depth:?}: extent");
    for (i, value) in expected.iter().enumerate() {
        let (x, y) = (
            i64::from(got.offset.0) + (i as i64 % i64::from(got.size.0)),
            i64::from(got.offset.1) + (i as i64 / i64::from(got.size.0)),
        );
        let Some(value) = value else {
            assert_eq!(
                got.px[i],
                before.at(x, y).unwrap_or_default(),
                "{what} {depth:?}: untouched {x},{y}"
            );
            continue;
        };
        for (k, v) in value.iter().enumerate() {
            let want = v.clamp(0.0, 1.0) * max;
            assert!(
                (f64::from(got.px[i][k]) - want).abs() <= slack,
                "{what} {depth:?}: {x},{y} component {k}: got {} want {want}",
                got.px[i][k]
            );
        }
    }
}

fn feather(cx: f64, cy: f64, r0: f64, r1: f64) -> impl Fn(u32, u32) -> u16 {
    move |x, y| {
        let (dx, dy) = (f64::from(x) + 0.5 - cx, f64::from(y) + 0.5 - cy);
        let r = (dx * dx + dy * dy).sqrt();
        ((1.0 - ((r - r0) / (r1 - r0)).clamp(0.0, 1.0)) * 65535.0).round() as u16
    }
}

fn varied(x: u32, y: u32) -> [u8; 4] {
    let mut p = pattern(x, y);
    p[3] = ((x * 23 + y * 31) % 200 + 40) as u8;
    p
}

fn full(depth: Depth, canvas: (u32, u32), f: impl Fn(u32, u32) -> [u8; 4]) -> Plane {
    Plane::of(depth, (0, 0), canvas, f)
}

fn whole(canvas: (u32, u32)) -> (i64, i64, u32, u32) {
    (0, 0, canvas.0, canvas.1)
}

// ------------------------------------------------------------------ cases --

#[test]
fn an_integer_translation_is_the_move_bit_for_bit() {
    let canvas = (20, 16);
    for depth in DEPTHS {
        let plane = full(depth, canvas, varied);
        let cov = coverage(canvas, feather(8.3, 7.1, 2.0, 5.0));
        let base = doc(depth, canvas, &plane);
        let want = base.run(&moved(0, 3, -2), &cov).plane(0);
        let m = [1.0, 0.0, 0.0, 1.0, 3.0, -2.0];
        let by_matrix = base.run(&affine(0, m, "bicubic"), &cov).plane(0);
        assert_eq!(by_matrix, want, "{depth:?} matrix");
        let rect = [2.0, 2.0, 11.0, 10.0];
        let by_quad = base
            .run(&quad(0, rect, corners_of(rect, m), "lanczos3"), &cov)
            .plane(0);
        assert_eq!(by_quad, want, "{depth:?} quad");
    }
}

#[test]
fn identity_is_null_with_no_error() {
    let canvas = (12, 10);
    for depth in DEPTHS {
        let base = doc(depth, canvas, &full(depth, canvas, pattern));
        let cov = coverage(canvas, rect(2, 2, 4, 4));
        let identity = affine(0, [1.0, 0.0, 0.0, 1.0, 0.0, 0.0], "bilinear");
        assert!(base.edit(&identity, &cov).unwrap().is_none(), "{depth:?}");
        let r = [2.0, 2.0, 4.0, 4.0];
        let same = quad(0, r, [2.0, 2.0, 6.0, 2.0, 6.0, 6.0, 2.0, 6.0], "bicubic");
        assert!(base.edit(&same, &cov).unwrap().is_none(), "{depth:?}");
    }
}

#[test]
fn a_binary_disc_rotated_a_quarter_turn_is_an_exact_permutation() {
    let canvas = (20, 20);
    let disc = |x: u32, y: u32| {
        let (dx, dy) = (f64::from(x) - 9.5, f64::from(y) - 9.5);
        if dx * dx + dy * dy <= 36.0 {
            65535
        } else {
            0
        }
    };
    // Quarter turn about (11, 9), composed from an angle as a host would;
    // the disc centred at (10, 10) is not symmetric about that pivot.
    let (s, c) = std::f64::consts::FRAC_PI_2.sin_cos();
    let m = [
        c,
        s,
        -s,
        c,
        11.0 + 9.0 * s - 11.0 * c,
        9.0 - 11.0 * s - 9.0 * c,
    ];
    for depth in DEPTHS {
        let plane = full(depth, canvas, pattern);
        let cov = coverage(canvas, disc);
        let got = doc(depth, canvas, &plane)
            .run(&affine(0, m, "bicubic"), &cov)
            .plane(0);
        let oracle = Oracle {
            depth,
            canvas,
            plane: &plane,
            coverage: &cov,
            hole: true,
        };
        let inverse = affine_inverse([0.0, 1.0, -1.0, 0.0, 20.0, -2.0]);
        let want = oracle.expect(&inverse, Kernel::Nearest, whole(canvas));
        check(depth, &plane, &got, &want, true, "quarter turn");
        let mut holes = 0;
        for y in 0..20i64 {
            for x in 0..20i64 {
                let inside = oracle.cov(x, y) == 1.0;
                let lands = oracle.cov(y + 2, 19 - x) == 1.0;
                if inside && !lands {
                    holes += 1;
                    let (g, b) = (got.at(x, y).unwrap(), plane.at(x, y).unwrap());
                    assert_eq!(g[3], 0, "{depth:?} hole alpha {x},{y}");
                    assert_eq!(g[..3], b[..3], "{depth:?} hole RGB {x},{y}");
                }
            }
        }
        assert!(holes > 0, "{depth:?}: the disc leaves a hole");
    }
}

#[test]
fn a_feathered_bilinear_scale_matches_the_premultiplied_oracle() {
    let canvas = (24, 20);
    let m = [2.0, 0.0, 0.0, 2.0, -8.0, -7.0];
    for depth in DEPTHS {
        let plane = full(depth, canvas, varied);
        let cov = coverage(canvas, feather(8.3, 7.1, 1.5, 4.6));
        let got = doc(depth, canvas, &plane)
            .run(&affine(0, m, "bilinear"), &cov)
            .plane(0);
        let oracle = Oracle {
            depth,
            canvas,
            plane: &plane,
            coverage: &cov,
            hole: true,
        };
        let want = oracle.expect(&affine_inverse(m), Kernel::Bilinear, whole(canvas));
        check(depth, &plane, &got, &want, false, "feathered ×2");
    }
}

// ------------------------------------------------------------- duplicate --

#[test]
fn a_duplicate_transform_keeps_the_original_and_lands_the_copy() {
    let canvas = (24, 20);
    let m = [2.0, 0.0, 0.0, 2.0, -8.0, -7.0];
    for depth in DEPTHS {
        let plane = full(depth, canvas, varied);
        let cov = coverage(canvas, feather(8.3, 7.1, 1.5, 4.6));
        let got = doc(depth, canvas, &plane)
            .run(&duplicated(&affine(0, m, "bilinear"), true), &cov)
            .plane(0);
        let oracle = Oracle {
            depth,
            canvas,
            plane: &plane,
            coverage: &cov,
            hole: false,
        };
        let want = oracle.expect(&affine_inverse(m), Kernel::Bilinear, whole(canvas));
        check(depth, &plane, &got, &want, false, "duplicate ×2");
        // Outside the landing the original is untouched.
        assert_eq!(got.at(0, 0), plane.at(0, 0), "{depth:?} corner");
    }
}

#[test]
fn a_duplicate_integer_translation_is_the_duplicate_move_bit_for_bit() {
    let canvas = (20, 16);
    for depth in DEPTHS {
        let plane = full(depth, canvas, varied);
        let cov = coverage(canvas, feather(8.3, 7.1, 2.0, 5.0));
        let base = doc(depth, canvas, &plane);
        let want = base.run(&duplicated(&moved(0, 3, -2), true), &cov).plane(0);
        let m = [1.0, 0.0, 0.0, 1.0, 3.0, -2.0];
        let got = base
            .run(&duplicated(&affine(0, m, "bicubic"), true), &cov)
            .plane(0);
        assert_eq!(got, want, "{depth:?}");
        let plain = base.run(&moved(0, 3, -2), &cov).plane(0);
        assert_ne!(got, plain, "{depth:?}: the duplicate leaves no hole");
    }
}

#[test]
fn duplicate_false_is_the_plain_transform() {
    let canvas = (24, 20);
    let m = [2.0, 0.0, 0.0, 2.0, -8.0, -7.0];
    for depth in DEPTHS {
        let plane = full(depth, canvas, varied);
        let cov = coverage(canvas, feather(8.3, 7.1, 1.5, 4.6));
        let base = doc(depth, canvas, &plane);
        let text = affine(0, m, "bilinear");
        let plain = base.run(&text, &cov).plane(0);
        let explicit = base.run(&duplicated(&text, false), &cov).plane(0);
        assert_eq!(explicit, plain, "{depth:?}");
    }
}

fn selection(canvas: (u32, u32), request: Value, input: &[u16]) -> Result<Vec<u16>, String> {
    let text = CString::new(request.to_string()).unwrap();
    let mut err = ptr::null_mut();
    let plan =
        unsafe { dr_selection_u16_plan(canvas.0, canvas.1, text.as_ptr(), LIMIT, 0, &mut err) };
    if let Some(message) = take_err(err) {
        return Err(message);
    }
    unsafe { dr_string_free(plan) };
    let mut out = vec![0u16; input.len()];
    let mut err = ptr::null_mut();
    let ok = unsafe {
        dr_selection_u16(
            canvas.0,
            canvas.1,
            text.as_ptr(),
            input.as_ptr(),
            input.len(),
            out.as_mut_ptr(),
            out.len(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    if let Some(message) = take_err(err) {
        return Err(message);
    }
    assert!(ok);
    Ok(out)
}

#[test]
fn the_landed_alpha_covers_the_transformed_selection_ring() {
    let canvas = (32, 32);
    let square = |x: u32, y: u32| {
        let mut p = pattern(x, y);
        p[3] = if (8..16).contains(&x) && (8..16).contains(&y) {
            255
        } else {
            0
        };
        p
    };
    let m = [2.0, 0.0, 0.0, 2.0, -12.0, -12.0];
    let r = [8.0, 8.0, 8.0, 8.0];
    let q = [5.3, 6.1, 19.7, 4.2, 23.4, 21.8, 3.9, 18.6];
    let cases = [
        (
            affine(0, m, "bilinear"),
            json!({"kind":"transform","matrix":m,"filter":"bilinear"}),
            true,
        ),
        (
            affine(0, m, "bicubic"),
            json!({"kind":"transform","matrix":m,"filter":"bicubic"}),
            false,
        ),
        (
            quad(0, r, q, "bilinear"),
            json!({"kind":"perspective","source_rect":r,"quad":q,"filter":"bilinear"}),
            false,
        ),
    ];
    for depth in DEPTHS {
        let plane = full(depth, canvas, square);
        let cov = coverage(canvas, rect(8, 8, 8, 8));
        for (text, op, bilinear) in &cases {
            let got = doc(depth, canvas, &plane).run(text, &cov).plane(0);
            assert_eq!((got.offset, got.size), ((0, 0), canvas));
            let selected = selection(canvas, op.clone(), &cov).unwrap();
            for y in 0..32i64 {
                for x in 0..32i64 {
                    let i = (y * 32 + x) as usize;
                    let alpha = f64::from(got.px[i][3]);
                    let want = match depth {
                        Depth::D8 => f64::from(selected[i]) / 257.0,
                        Depth::D16 => f64::from(selected[i]),
                    };
                    assert!(
                        (alpha - want).abs() <= 1.5,
                        "{depth:?} {text} {x},{y}: alpha {alpha} coverage {want}"
                    );
                    if want.round() > 0.0 {
                        assert!(alpha > 0.0, "{depth:?} {text} {x},{y}: selected but empty");
                    }
                }
            }
            if *bilinear {
                // Column 3 samples a quarter of the square's edge.
                let i = 12 * 32 + 3;
                assert!((f64::from(selected[i]) / 65535.0 - 0.25).abs() < 1e-3);
                assert!(got.px[i][3] > 0, "{depth:?}: the ring at (3, 12)");
            }
        }
    }
}

#[test]
fn the_landing_is_clipped_to_the_canvas() {
    let canvas = (24, 16);
    // Scale ×3 about (19, 7); F = (16, 4, 6, 6), grown by bilinear's reach 1.
    let m = [3.0, 0.0, 0.0, 3.0, -38.0, -14.0];
    for depth in DEPTHS {
        // The layer extends 8 px past the right edge.
        let plane = Plane::of(depth, (8, 0), (24, 16), pattern);
        let cov = coverage(canvas, rect(16, 4, 6, 6));
        let got = doc(depth, canvas, &plane)
            .run(&affine(0, m, "bilinear"), &cov)
            .plane(0);
        // Grown F (15, 3)..(23, 11) → (7, -5)..(31, 19), ∩ canvas
        // (7, 0)..(24, 16), ∪ layer (8, 0)..(32, 16).
        assert_eq!((got.offset, got.size), ((7, 0), (25, 16)), "{depth:?}");
        for y in 0..16 {
            for x in 24..32 {
                assert_eq!(got.at(x, y), plane.at(x, y), "{depth:?} off-canvas {x},{y}");
            }
        }
        let oracle = Oracle {
            depth,
            canvas,
            plane: &plane,
            coverage: &cov,
            hole: true,
        };
        let want = oracle.expect(&affine_inverse(m), Kernel::Bilinear, (7, 0, 25, 16));
        check(depth, &plane, &got, &want, false, "clip");
    }
}

#[test]
fn a_translation_off_the_canvas_leaves_only_the_hole() {
    let canvas = (16, 12);
    let m = [1.0, 0.0, 0.0, 1.0, 1000.5, 0.0];
    for depth in DEPTHS {
        let plane = full(depth, canvas, varied);
        let cov = coverage(canvas, feather(6.2, 5.3, 1.0, 3.7));
        let got = doc(depth, canvas, &plane)
            .run(&affine(0, m, "bilinear"), &cov)
            .plane(0);
        let oracle = Oracle {
            depth,
            canvas,
            plane: &plane,
            coverage: &cov,
            hole: true,
        };
        let none: Inverse = Box::new(|_, _| None);
        let want = oracle.expect(&none, Kernel::Nearest, whole(canvas));
        check(depth, &plane, &got, &want, true, "+1000.5");
    }
}

#[test]
fn the_layer_mask_stays_put() {
    let canvas = (20, 16);
    let values: Vec<u8> = (0..60).map(|i| (i * 7 + 3) as u8).collect();
    let props = MaskProperties::at((1, 2));
    let cov = coverage(canvas, rect(3, 3, 5, 4));
    let text = affine(0, [2.0, 0.0, 0.0, 2.0, -3.0, -3.0], "bilinear");
    // 8 bits.
    let plane = full(Depth::D8, canvas, pattern);
    let mut base = byte_doc(canvas, &plane);
    let mask = Arc::new(GrayImage::from_fn(10, 6, |x, y| {
        Luma([values[(y * 10 + x) as usize]])
    }));
    base.layers[0].mask = Some(mask.clone());
    base.layers[0].mask_properties = props.clone();
    base.layers[0].mask_enabled = false;
    let out = edit8(&base, &text, &cov, None).unwrap().unwrap();
    let layer = &out.layers[0];
    assert_eq!(layer.mask.as_deref().unwrap().as_raw(), mask.as_raw());
    assert!(layer.mask_properties == props);
    assert!(!layer.mask_enabled);
    assert_eq!(layer.offset, (0, 0));
    // 16 bits.
    let plane = full(Depth::D16, canvas, pattern);
    let samples: Vec<u16> = values.iter().map(|v| u16::from(*v) * 257 + 1).collect();
    let typed_mask = PrecisionMask::new(
        PrecisionPlane::from_u16(10, 6, samples.clone(), None).unwrap(),
        props.clone(),
        false,
    )
    .unwrap();
    let base = typed_doc(
        canvas,
        vec![typed_layer(&plane).with_mask(Some(typed_mask))],
    );
    let out = edit16(&base, &text, &cov, None).unwrap().unwrap();
    let mask = out.layer(0).unwrap().mask().unwrap();
    assert_eq!(mask.plane().samples(), &samples[..]);
    assert!(mask.properties() == &props);
    assert!(!mask.enabled());
    assert_eq!(out.layer(0).unwrap().offset(), (0, 0));
}

#[test]
fn a_parallelogram_quad_is_its_affine_bit_for_bit() {
    let canvas = (24, 20);
    let m = [1.5, 0.25, -0.5, 1.25, 3.0, -2.5];
    let r = [4.0, 4.0, 8.0, 8.0];
    for depth in DEPTHS {
        let plane = full(depth, canvas, varied);
        let cov = coverage(canvas, feather(8.3, 7.1, 1.5, 4.6));
        let base = doc(depth, canvas, &plane);
        for filter in ["bilinear", "lanczos3"] {
            let a = base.run(&affine(0, m, filter), &cov).plane(0);
            let b = base
                .run(&quad(0, r, corners_of(r, m), filter), &cov)
                .plane(0);
            assert_eq!(a, b, "{depth:?} {filter}");
        }
    }
}

#[test]
fn a_true_quad_at_nearest_matches_the_homography_oracle() {
    let canvas = (24, 20);
    let r = [6.0, 6.0, 8.0, 8.0];
    let q = [5.0, 4.3, 15.7, 6.1, 14.2, 15.4, 4.4, 13.2];
    let inverse = homography_inverse(r, q);
    for depth in DEPTHS {
        let plane = full(depth, canvas, pattern);
        let cov = coverage(canvas, rect(6, 6, 8, 8));
        let got = doc(depth, canvas, &plane)
            .run(&quad(0, r, q, "nearest"), &cov)
            .plane(0);
        let oracle = Oracle {
            depth,
            canvas,
            plane: &plane,
            coverage: &cov,
            hole: true,
        };
        let mut want = oracle.expect(&inverse, Kernel::Nearest, whole(canvas));
        // A centre within 1e-6 of a pixel edge may floor either way.
        let mut ties = 0;
        for (i, value) in want.iter_mut().enumerate() {
            let (x, y) = ((i % 24) as f64 + 0.5, (i / 24) as f64 + 0.5);
            if let Some((u, v)) = inverse(x, y) {
                let near = |t: f64| (t - t.round()).abs() < 1e-6;
                if near(u) || near(v) {
                    ties += 1;
                    *value = Some(got.px[i].map(|c| f64::from(c) / depth.max()));
                }
            }
        }
        assert!(ties < 4, "{depth:?}: the fixture sits away from ties");
        check(depth, &plane, &got, &want, true, "true quad");
    }
}

/// A byte document whose only canvas-sized buffer is none: a small layer on
/// a large canvas.
fn small_byte_doc(canvas: (u32, u32), plane: &Plane) -> DrDocument {
    let raw = plane.px.iter().flat_map(|p| p.map(|v| v as u8)).collect();
    let image = RgbaImage::from_raw(plane.size.0, plane.size.1, raw).unwrap();
    let mut doc = DrDocument::from_pixels(image);
    doc.width = canvas.0;
    doc.height = canvas.1;
    doc.layers[0].offset = plane.offset;
    doc
}

fn small(depth: Depth, canvas: (u32, u32), plane: &Plane) -> Doc {
    match depth {
        Depth::D8 => Doc::Byte(small_byte_doc(canvas, plane)),
        Depth::D16 => Doc::Typed(typed_doc(canvas, vec![typed_layer(plane)])),
    }
}

#[test]
fn a_landing_past_the_old_pixel_work_bound_is_planned_and_entered() {
    let canvas = (3800, 3800);
    // Lanczos3 ×500 about the square's centre (1900, 1900): the grown
    // landing covers the whole canvas, 14.44M·36·4 > 2e9, which once refused.
    // The plan answers, and the run is entered: with its
    // control already cancelled it returns `operation cancelled`, not a
    // work refusal.
    let m = [500.0, 0.0, 0.0, 500.0, -499.0 * 1900.0, -499.0 * 1900.0];
    let cov = coverage(canvas, rect(1896, 1896, 8, 8));
    let text = affine(0, m, "lanczos3");
    for depth in DEPTHS {
        let plane = Plane::of(depth, (1896, 1896), (8, 8), pattern);
        let base = small(depth, canvas, &plane);
        let plan = base
            .plan(&text)
            .unwrap_or_else(|e| panic!("{depth:?}: the plan refused: {e}"));
        assert!(
            plan["scratch_bytes"].as_u64().unwrap() >= 16 * 3800 * 3800,
            "{depth:?}: the plan charges the whole-canvas landing"
        );
        let control = DrJobControl::default();
        control.cancel();
        let err = match &base {
            Doc::Byte(d) => edit8(d, &text, &cov, Some(&control)).err(),
            Doc::Typed(d) => edit16(d, &text, &cov, Some(&control)).err(),
        };
        assert_eq!(err.as_deref(), Some("operation cancelled"), "{depth:?}");
    }
}

#[test]
fn a_companion_selection_past_the_old_work_bound_plans() {
    // 56.25M·36 > 2e9: the companion selection transform refused the plan
    // once. It has no work refusal now, at either filter.
    let canvas = (7500, 7500);
    let (s, c) = 30f64.to_radians().sin_cos();
    let m = [c, s, -s, c, 100.0, -50.0];
    let plane8 = Plane::of(Depth::D8, (10, 10), (8, 8), pattern);
    let d8 = small_byte_doc(canvas, &plane8);
    let plane16 = Plane::of(Depth::D16, (10, 10), (8, 8), pattern);
    let d16 = typed_doc(canvas, vec![typed_layer(&plane16)]);
    for filter in ["lanczos3", "bicubic"] {
        let text = affine(0, m, filter);
        assert!(plan8(&d8, &text).is_ok(), "{filter}");
        assert!(plan16(&d16, &text).is_ok(), "{filter}");
    }
}

#[test]
fn pixel_locks_refuse_and_transparency_and_position_locks_allow() {
    let canvas = (12, 8);
    let cov = coverage(canvas, rect(2, 2, 3, 3));
    let text = affine(0, [2.0, 0.0, 0.0, 2.0, -2.0, -2.0], "bilinear");
    for depth in DEPTHS {
        let plane = full(depth, canvas, pattern);
        let locked = |locks: u32| -> Doc {
            match depth {
                Depth::D8 => {
                    let mut d = byte_doc(canvas, &plane);
                    d.layers[0].locks = locks;
                    Doc::Byte(d)
                }
                Depth::D16 => Doc::Typed(
                    typed_doc(canvas, vec![typed_layer(&plane)])
                        .with_layer_locks(0, locks)
                        .unwrap()
                        .unwrap(),
                ),
            }
        };
        for locks in [LOCK_PIXELS, LOCK_ALL] {
            let err = locked(locks).edit(&text, &cov).err().unwrap();
            assert!(err.contains("locked"), "{depth:?} locks {locks}: {err}");
        }
        for locks in [LOCK_TRANSPARENCY, LOCK_POSITION] {
            // Rotate a quarter turn about (4, 4): the square lands on
            // x 3..6, so the hole at (2, 4) is punched, not stamped.
            let quarter = affine(0, [0.0, 1.0, -1.0, 0.0, 8.0, 0.0], "bilinear");
            let got = locked(locks).run(&quarter, &cov).plane(0);
            assert_eq!(got.at(2, 4).unwrap()[3], 0, "{depth:?} locks {locks}");
            let _ = locked(locks).run(&text, &cov);
        }
    }
    let plane = full(Depth::D8, canvas, pattern);
    let (mut d, group, _, _) = byte_doc(canvas, &plane).group_layers(&[0], "G").unwrap();
    d.layers[group].locks = LOCK_PIXELS;
    let err = edit8(&d, &text, &cov, None).err().unwrap();
    assert!(err.contains("inside group"), "{err}");
    let plane = full(Depth::D16, canvas, pattern);
    let group =
        PrecisionDocumentLayer::isolated_normal("G", vec![typed_layer(&plane)], 1.0, true).unwrap();
    let d = typed_doc(canvas, vec![group])
        .with_layer_locks(1, LOCK_PIXELS)
        .unwrap()
        .unwrap();
    let err = edit16(&d, &text, &cov, None).err().unwrap();
    assert!(err.contains("inside group"), "{err}");
}

#[test]
fn bad_targets_and_bad_requests_refuse() {
    let canvas = (8, 6);
    let cov = coverage(canvas, rect(1, 1, 3, 3));
    let text = affine(0, [2.0, 0.0, 0.0, 2.0, -1.0, -1.0], "bilinear");
    let plane8 = full(Depth::D8, canvas, pattern);
    let (grouped, group, _, _) = byte_doc(canvas, &plane8).group_layers(&[0], "G").unwrap();
    let err = edit8(
        &grouped,
        &affine(group, [2.0, 0.0, 0.0, 2.0, 0.0, 0.0], "bilinear"),
        &cov,
        None,
    )
    .err()
    .unwrap();
    assert!(
        err.contains("Transforming selected pixels needs a pixel layer"),
        "{err}"
    );
    assert!(err.contains("is a group"), "{err}");
    let mut adjusted = byte_doc(canvas, &plane8)
        .adding_image_layer(0, RgbaImage::new(8, 6), "Fx")
        .unwrap();
    adjusted.layers[1].meta = Some(adjustment_meta());
    let err = edit8(
        &adjusted,
        &affine(1, [2.0, 0.0, 0.0, 2.0, 0.0, 0.0], "bilinear"),
        &cov,
        None,
    )
    .err()
    .unwrap();
    assert!(err.contains("adjustment layer"), "{err}");

    let plane16 = full(Depth::D16, canvas, pattern);
    let group =
        PrecisionDocumentLayer::isolated_normal("G", vec![typed_layer(&plane16)], 1.0, true)
            .unwrap();
    let err = edit16(
        &typed_doc(canvas, vec![group]),
        &affine(1, [2.0, 0.0, 0.0, 2.0, 0.0, 0.0], "bilinear"),
        &cov,
        None,
    )
    .err()
    .unwrap();
    assert!(err.contains("is a group"), "{err}");
    let adjusted = typed_doc(canvas, vec![typed_layer(&plane16)])
        .add_adjustment("Fx", &adjustment_meta(), 1.0, LIMIT, 0, None)
        .unwrap()
        .unwrap();
    let err = edit16(
        &adjusted,
        &affine(1, [2.0, 0.0, 0.0, 2.0, 0.0, 0.0], "bilinear"),
        &cov,
        None,
    )
    .err()
    .unwrap();
    assert!(err.contains("adjustment layer"), "{err}");

    let r = [1.0, 1.0, 3.0, 3.0];
    let malformed = [
        (
            json!({"op":"transform_selected","layer":0,"matrix":[2,0,0,2,0,0],"filter":"bilinear","x":1}).to_string(),
            "unknown field",
        ),
        (
            json!({"op":"transform_selected","layer":0,"matrix":[2,0,0,2,0,0],"source_rect":r,"quad":[0,0,4,0,4,4,0,4],"filter":"bilinear"}).to_string(),
            "invalid selected pixels request",
        ),
        (
            json!({"op":"transform_selected","layer":0,"filter":"bilinear"}).to_string(),
            "invalid selected pixels request",
        ),
        (
            json!({"op":"transform_selected","layer":0,"source_rect":r,"filter":"bilinear"}).to_string(),
            "invalid selected pixels request",
        ),
        (
            affine(0, [1.0, 2.0, 2.0, 4.0, 0.0, 0.0], "bilinear"),
            "The transform matrix is degenerate or not finite.",
        ),
        (
            r#"{"op":"transform_selected","layer":0,"matrix":[NaN,0,0,1,0,0],"filter":"bilinear"}"#
                .to_string(),
            "invalid",
        ),
        (
            quad(0, r, [1.0, 1.0, 4.0, 1.0, 2.0, 2.0, 1.0, 4.0], "bilinear"),
            "The perspective corners must form a convex quad.",
        ),
    ];
    for depth in DEPTHS {
        let base = doc(depth, canvas, &full(depth, canvas, pattern));
        let short = &cov[..cov.len() - 1];
        assert!(base.edit(&text, short).is_err(), "{depth:?} length");
        let far = affine(9, [2.0, 0.0, 0.0, 2.0, 0.0, 0.0], "bilinear");
        assert!(base.edit(&far, &cov).is_err(), "{depth:?} index");
        for (request, sentence) in &malformed {
            let err = base.edit(request, &cov).err().unwrap();
            assert!(err.contains(sentence), "{depth:?} {request}: {err}");
        }
        let empty = vec![0u16; 48];
        assert_eq!(base.edit(&text, &empty).err().as_deref(), Some(EMPTY));
    }
}

#[test]
fn a_float_document_transforms_its_layer_pixels() {
    let path = "tests/fixtures/float_11f/scene.tiff";
    if !std::path::Path::new(path).exists() {
        return;
    }
    let doc = PrecisionDocument::open_preserving(path, LIMIT, 0, None).unwrap();
    assert!(doc.is_float());
    let (w, h) = doc.dimensions();
    let cov = vec![65535u16; (w * h) as usize];
    let text = affine(0, [2.0, 0.0, 0.0, 2.0, 0.0, 0.0], "bilinear");
    // Admitted; the float arm's own tests are `float_selected_pixels.rs`.
    let moved = edit16(&doc, &text, &cov, None).unwrap().unwrap();
    assert!(moved.is_float());
}

/// The landing oracle: `f` (x, y, w, h) grown by the filter's reach, its
/// corners through `m`, the outward floor/ceil box ∩ the canvas, at 16 bytes
/// a pixel.
fn landing_bytes(canvas: (u32, u32), f: [f64; 4], m: [f64; 6], reach: f64) -> u64 {
    let grown = [
        f[0] - reach,
        f[1] - reach,
        f[2] + 2.0 * reach,
        f[3] + 2.0 * reach,
    ];
    let c = corners_of(grown, m);
    let xs = [c[0], c[2], c[4], c[6]];
    let ys = [c[1], c[3], c[5], c[7]];
    let min = |v: [f64; 4]| v.iter().copied().fold(f64::INFINITY, f64::min);
    let max = |v: [f64; 4]| v.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let left = min(xs).floor().max(0.0);
    let top = min(ys).floor().max(0.0);
    let right = max(xs).ceil().min(f64::from(canvas.0));
    let bottom = max(ys).ceil().min(f64::from(canvas.1));
    if left >= right || top >= bottom {
        return 0;
    }
    16 * ((right - left) as u64) * ((bottom - top) as u64)
}

/// The transform plan's `scratch_bytes` minus the one-pixel move's, at both
/// depths, after checking the two plans agree on everything else.
fn plan_extra(d8: &DrDocument, d16: &PrecisionDocument, text: &str) -> [u64; 2] {
    let scratch = |p: &Value| p["scratch_bytes"].as_u64().unwrap();
    let base8 = plan8(d8, &moved(0, 1, 0)).unwrap();
    let p8 = plan8(d8, text).unwrap();
    assert_eq!(p8["input_samples"], base8["input_samples"]);
    assert_eq!(p8["retained_bytes"], base8["retained_bytes"]);
    let base16 = plan16(d16, &moved(0, 1, 0)).unwrap();
    let p16 = plan16(d16, text).unwrap();
    assert_eq!(p16["input_samples"], base16["input_samples"]);
    assert_eq!(p16["retained_bytes"], base16["retained_bytes"]);
    [
        scratch(&p8) - scratch(&base8),
        scratch(&p16) - scratch(&base16),
    ]
}

#[test]
fn the_plan_charges_the_landing_of_the_layer_rect() {
    let canvas = (12, 9);
    let plane8 = Plane::of(Depth::D8, (-2, 1), (10, 6), pattern);
    let d8 = byte_doc(canvas, &plane8);
    let plane16 = Plane::of(Depth::D16, (-2, 1), (10, 6), pattern);
    let d16 = typed_doc(canvas, vec![typed_layer(&plane16)]);
    assert_eq!(plan8(&d8, &moved(0, 1, 0)).unwrap()["input_samples"], 108);
    // The layer's on-canvas rect.
    let on_canvas = [0.0, 1.0, 8.0, 6.0];
    // A coverage rect strictly inside it.
    let inner = [2.0, 2.0, 4.0, 3.0];

    // (a) A bilinear shrink (a 5×4 landing) and a Lanczos3 rotation.
    let shrink = [0.5, 0.0, 0.0, 0.5, 0.0, 0.0];
    let rotation = [0.8, 0.3, -0.3, 0.8, 4.0, 1.0];
    assert_eq!(landing_bytes(canvas, on_canvas, shrink, 1.0), 16 * 5 * 4);
    for (m, filter, reach) in [(shrink, "bilinear", 1.0), (rotation, "lanczos3", 3.0)] {
        let expected = landing_bytes(canvas, on_canvas, m, reach);
        let extra = plan_extra(&d8, &d16, &affine(0, m, filter));
        assert_eq!(extra, [expected; 2], "{filter}");
        // (e) Any float inside the layer lands inside what the plan charged.
        let inside = landing_bytes(canvas, inner, m, reach);
        assert!(extra.iter().all(|&e| e >= inside), "{filter}");
    }

    // (b) A nearest true quad over the on-canvas rect, with integer corners:
    // the landing is the quad's own box.
    let corners = [1.0, 2.0, 7.0, 1.0, 6.0, 7.0, 2.0, 6.0];
    let extra = plan_extra(&d8, &d16, &quad(0, on_canvas, corners, "nearest"));
    assert_eq!(extra, [16 * 6 * 6; 2]);

    // (d) Identity and an integer translation charge nothing.
    for m in [
        [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
        [1.0, 0.0, 0.0, 1.0, 3.0, -2.0],
    ] {
        assert_eq!(plan_extra(&d8, &d16, &affine(0, m, "bilinear")), [0, 0]);
    }

    // (f) Degenerate geometry still refuses.
    let degenerate = affine(0, [1.0, 2.0, 2.0, 4.0, 0.0, 0.0], "bilinear");
    assert!(plan8(&d8, &degenerate).is_err());
    assert!(plan16(&d16, &degenerate).is_err());

    // (c) A small layer on a large canvas charges its landing, not the canvas.
    let big = (3800, 3800);
    let small8 = small_byte_doc(big, &Plane::of(Depth::D8, (1896, 1896), (8, 8), pattern));
    let plane16 = Plane::of(Depth::D16, (1896, 1896), (8, 8), pattern);
    let small16 = typed_doc(big, vec![typed_layer(&plane16)]);
    let double = [2.0, 0.0, 0.0, 2.0, -1900.0, -1900.0];
    assert_eq!(
        landing_bytes(big, [1896.0, 1896.0, 8.0, 8.0], double, 1.0),
        6400
    );
    let extra = plan_extra(&small8, &small16, &affine(0, double, "bilinear"));
    assert_eq!(extra, [6400; 2]);
    assert!(extra.iter().all(|&e| e < 16 * 3800 * 3800));
}

#[test]
fn the_raster_capability_admits_sixteen_and_thirty_two_bits() {
    assert_eq!(
        capability("selected_pixels_transform", "raster", 16)["supported"],
        true
    );
    assert_eq!(
        capability("selected_pixels_transform", "group", 16)["supported"],
        false
    );
    assert_eq!(
        capability("selected_pixels_transform", "raster", 32)["supported"],
        true
    );
}

#[test]
fn a_cancelled_control_returns_operation_cancelled() {
    let canvas = (8, 6);
    let cov = coverage(canvas, rect(1, 1, 3, 3));
    let text = affine(0, [2.0, 0.0, 0.0, 2.0, -1.0, -1.0], "bilinear");
    let control = DrJobControl::default();
    control.cancel();
    let d8 = byte_doc(canvas, &full(Depth::D8, canvas, pattern));
    let err = edit8(&d8, &text, &cov, Some(&control)).err().unwrap();
    assert_eq!(err, "operation cancelled");
    let control = DrJobControl::default();
    control.cancel();
    let d16 = typed_doc(
        canvas,
        vec![typed_layer(&full(Depth::D16, canvas, pattern))],
    );
    let err = edit16(&d16, &text, &cov, Some(&control)).err().unwrap();
    assert_eq!(err, "operation cancelled");
}

#[test]
fn the_perspective_selection_kind() {
    let canvas = (24, 20);
    let input = coverage(canvas, feather(9.2, 8.3, 1.5, 4.5));
    // A parallelogram is its affine, bit for bit.
    let m = [1.5, 0.25, -0.5, 1.25, 3.0, -2.5];
    let r = [4.0, 4.0, 8.0, 8.0];
    let by_matrix = selection(
        canvas,
        json!({"kind":"transform","matrix":m,"filter":"bicubic"}),
        &input,
    )
    .unwrap();
    let by_quad = selection(
        canvas,
        json!({"kind":"perspective","source_rect":r,"quad":corners_of(r, m),"filter":"bicubic"}),
        &input,
    )
    .unwrap();
    assert_eq!(by_matrix, by_quad);
    // A true quad at nearest copies the covering source sample.
    let r = [6.0, 6.0, 8.0, 8.0];
    let q = [5.0, 4.3, 15.7, 6.1, 14.2, 15.4, 4.4, 13.2];
    let got = selection(
        canvas,
        json!({"kind":"perspective","source_rect":r,"quad":q,"filter":"nearest"}),
        &input,
    )
    .unwrap();
    let inverse = homography_inverse(r, q);
    for y in 0..20usize {
        for x in 0..24usize {
            let want = inverse(x as f64 + 0.5, y as f64 + 0.5).map_or(0, |(u, v)| {
                let (sx, sy) = (u.floor(), v.floor());
                if sx < 0.0 || sy < 0.0 || sx >= 24.0 || sy >= 20.0 {
                    0
                } else {
                    input[sy as usize * 24 + sx as usize]
                }
            });
            assert_eq!(got[y * 24 + x], want, "{x},{y}");
        }
    }
    // A concave quad refuses.
    let concave = [1.0, 1.0, 12.0, 1.0, 5.0, 5.0, 1.0, 12.0];
    let err = selection(
        canvas,
        json!({"kind":"perspective","source_rect":r,"quad":concave,"filter":"bilinear"}),
        &input,
    )
    .err()
    .unwrap();
    assert!(err.contains("convex"), "{err}");
}

#[test]
fn the_transform_request_reaches_the_byte_export_with_nulls_tolerated() {
    // No new export, so the null sweep is unchanged. The request still
    // refuses a NULL coverage through the byte export.
    let text = CString::new(affine(0, [2.0, 0.0, 0.0, 2.0, 0.0, 0.0], "bilinear")).unwrap();
    let canvas = (4, 4);
    let d = byte_doc(canvas, &full(Depth::D8, canvas, pattern));
    let mut err = ptr::null_mut();
    let out = unsafe {
        doctored_core::ffi_selected_pixels::dr_doc_selected_pixels_edit(
            &d,
            text.as_ptr(),
            ptr::null(),
            16,
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(out.is_null());
    assert!(take_err(err).is_some());
}
