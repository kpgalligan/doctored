//! Phase 13A-6c-1: native brush coverage over the stroke's FOOTPRINT with
//! exact hard-edge classification (`brush_coverage::rasterize`, reached
//! through the unchanged `dr_brush_u16_plan` / `dr_brush_u16`). The oracle is
//! the retained canvas-field path `BrushRequest::reference_rasterize` (every
//! dab's box, the 8×8 sum on every hard pixel, no work cap): the product
//! equals it sample for sample, within the cap and beyond it. The plan never
//! charges above the pre-13A-6c-1 formula (`2n + Σ box·(64 | 1)`, scratch
//! `12n`), which this suite writes independently; both refusal sentences
//! still fire.
//!
//! Default tests are load-independent (13A-5b-iii-b-1): a pre-cancelled
//! token, a cancel PARKED inside the dab loop (13A-5b-iii-b-3's seam) and a
//! cancelled-then-complete run equal to the reference. DECLARED before
//! measuring (13A-5a's model `T_f + W_s·c_s`, `T_f` = 1 ms): a cancel issued
//! inside the dab loop returns within `T_f + 64·(x1 − x0)·c_eval` (one dab
//! row entirely band); the `#[ignore]`d probe prints PASS/MISS through
//! `make cancel-bench`. Misses are recorded, never widened.

use std::ffi::{c_char, CStr, CString};
use std::ptr;

use doctored_core::brush_coverage::{band_bound, footprint, BrushRequest};
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_brush::{dr_brush_u16, dr_brush_u16_plan};
use doctored_core::ffi_jobs::*;
use doctored_core::job_control::DrJobControl;
use doctored_core::precision_image::REFERENCE_WORKING_BYTES;
use serde_json::Value;

use crate::common;
use common::probe::*;

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
const WORK: &str =
    "native brush exceeds 2 billion footprint work units; use a shorter stroke or a smaller brush";
const SENTINEL: u16 = 0x4a4a;
const MIB: u64 = 1024 * 1024;
const CAP: u64 = 2_000_000_000;
/// The earlier cap, which the pre-13A-6c-1 comparison is written against.
const OLD_CAP: u64 = 256_000_000;

/// A version-1 request.
#[derive(Clone, Copy)]
struct Brush {
    hardness: f64,
    roundness: f64,
    angle: f64,
    pixel: bool,
    opacity: f64,
}

impl Brush {
    fn hard(roundness: f64, angle: f64) -> Self {
        Self {
            hardness: 1.,
            roundness,
            angle,
            pixel: false,
            opacity: 1.,
        }
    }
    fn soft(hardness: f64, roundness: f64, angle: f64) -> Self {
        Self {
            hardness,
            ..Self::hard(roundness, angle)
        }
    }
    fn pixel() -> Self {
        Self {
            pixel: true,
            ..Self::hard(1., 0.)
        }
    }
    fn opacity(self, opacity: f64) -> Self {
        Self { opacity, ..self }
    }
    fn json(&self) -> String {
        serde_json::json!({"version": 1, "hardness": self.hardness, "roundness": self.roundness,
            "angle_degrees": self.angle, "pixel": self.pixel, "opacity": self.opacity})
        .to_string()
    }
    fn text(&self) -> CString {
        CString::new(self.json()).unwrap()
    }
    fn request(&self) -> BrushRequest {
        BrushRequest::parse(&self.json()).unwrap()
    }
    fn is_hard(&self) -> bool {
        !self.pixel && self.hardness >= 1.
    }
}

// ------------------------------------------------------------- plumbing --

unsafe fn message(e: *mut c_char) -> String {
    if e.is_null() {
        return String::new();
    }
    let s = unsafe { CStr::from_ptr(e) }.to_string_lossy().into_owned();
    unsafe { dr_string_free(e) };
    s
}

/// `dr_brush_u16_plan`'s JSON.
fn plan(brush: Brush, w: u32, h: u32, dabs: &[f64]) -> Result<Value, String> {
    let t = brush.text();
    let mut e = ptr::null_mut();
    let p = unsafe {
        dr_brush_u16_plan(
            w,
            h,
            t.as_ptr(),
            dabs.as_ptr(),
            dabs.len() / 4,
            LIMIT,
            0,
            &mut e,
        )
    };
    if p.is_null() {
        return Err(unsafe { message(e) });
    }
    let value = serde_json::from_slice(unsafe { CStr::from_ptr(p) }.to_bytes()).unwrap();
    unsafe { dr_string_free(p) };
    Ok(value)
}

/// `dr_brush_u16` into a `0x4a4a`-filled output under `control` (NULL:
/// none). On refusal the output must be untouched.
fn product_with(
    brush: Brush,
    w: u32,
    h: u32,
    dabs: &[f64],
    control: *const DrJobControl,
) -> Result<Vec<u16>, String> {
    let t = brush.text();
    let mut out = vec![SENTINEL; w as usize * h as usize];
    let mut e = ptr::null_mut();
    let ok = unsafe {
        dr_brush_u16(
            w,
            h,
            t.as_ptr(),
            dabs.as_ptr(),
            dabs.len() / 4,
            out.as_mut_ptr(),
            out.len(),
            LIMIT,
            0,
            control,
            &mut e,
        )
    };
    let m = unsafe { message(e) };
    if ok {
        Ok(out)
    } else {
        assert!(
            out.iter().all(|&v| v == SENTINEL),
            "a refused brush wrote its output"
        );
        Err(m)
    }
}

fn product(brush: Brush, w: u32, h: u32, dabs: &[f64]) -> Result<Vec<u16>, String> {
    product_with(brush, w, h, dabs, ptr::null())
}

fn reference(brush: Brush, w: u32, h: u32, dabs: &[f64]) -> Vec<u16> {
    brush
        .request()
        .reference_rasterize((w, h), dabs)
        .unwrap_or_else(|e| panic!("the reference refused: {e}"))
}

/// The test's own copy of the dab box (`bounds`).
fn dab_box(d: &[f64], w: u32, h: u32) -> u64 {
    let r = d[2] / 2.;
    let x0 = (d[0] - r).floor().clamp(0., f64::from(w));
    let y0 = (d[1] - r).floor().clamp(0., f64::from(h));
    let x1 = (d[0] + r).ceil().clamp(0., f64::from(w));
    let y1 = (d[1] + r).ceil().clamp(0., f64::from(h));
    ((x1 - x0) * (y1 - y0)) as u64
}

/// The pre-13A-6c-1 plan's total: `2n + Σ box·(64 hard | 1)`, every dab.
fn old_work(brush: Brush, w: u32, h: u32, dabs: &[f64]) -> u64 {
    let per = if brush.is_hard() { 64 } else { 1 };
    2 * u64::from(w) * u64::from(h)
        + dabs
            .chunks_exact(4)
            .map(|d| dab_box(d, w, h) * per)
            .sum::<u64>()
}

/// The pre-13A-6c-1 scratch: `4 MiB + 12n + 8·len`.
fn old_scratch(w: u32, h: u32, dabs: &[f64]) -> u64 {
    4 * MIB + 12 * u64::from(w) * u64::from(h) + 8 * dabs.len() as u64
}

fn first_difference(a: &[u16], b: &[u16], w: u32) -> String {
    match a.iter().zip(b).position(|(x, y)| x != y) {
        Some(i) => format!(
            "first difference at ({}, {}): {} vs {}",
            i as u32 % w,
            i as u32 / w,
            a[i],
            b[i]
        ),
        None => format!("lengths {} vs {}", a.len(), b.len()),
    }
}

/// Product == reference sample for sample, and the plan is never above the
/// old formula (work and scratch).
fn assert_exact(brush: Brush, w: u32, h: u32, dabs: &[f64], what: &str) {
    let expected = reference(brush, w, h, dabs);
    let actual = product(brush, w, h, dabs).unwrap_or_else(|e| panic!("{what}: product: {e}"));
    assert!(
        actual == expected,
        "{what} ({}) differs from the reference ({})",
        brush.json(),
        first_difference(&actual, &expected, w)
    );
    let p = plan(brush, w, h, dabs).unwrap();
    let work = p["work_units"].as_u64().unwrap();
    let scratch = p["scratch_bytes"].as_u64().unwrap();
    assert!(work <= old_work(brush, w, h, dabs), "{what}: work {work}");
    assert!(
        scratch <= old_scratch(w, h, dabs),
        "{what}: scratch {scratch}"
    );
}

const ROUNDNESS: [f64; 3] = [0.01, 0.37, 1.];
const ANGLES: [f64; 5] = [0., 30., 45., -77.5, 180.];
/// Sub-pixel centre offsets: generic, an exact pixel centre, an exact pixel
/// corner.
const OFFSETS: [(f64, f64); 3] = [(0.37, 0.61), (0.5, 0.5), (0., 0.)];

/// One hard dab per call over diameters × roundness × angles × centres ×
/// opacity {1, 0.4}, each on a canvas holding the whole dab.
fn hard_sweep(diameters: &[f64]) {
    for &diameter in diameters {
        let side = diameter.ceil() as u32 + 6;
        let middle = f64::from(side / 2);
        for roundness in ROUNDNESS {
            for angle in ANGLES {
                for (ox, oy) in OFFSETS {
                    for opacity in [1., 0.4] {
                        let dab = [middle + ox, middle + oy, diameter, opacity];
                        assert_exact(
                            Brush::hard(roundness, angle),
                            side,
                            side,
                            &dab,
                            &format!("hard {diameter} at +({ox}, {oy}) opacity {opacity}"),
                        );
                    }
                }
            }
        }
    }
}

// --------------------------------------------------------- exactness --

#[test]
fn small_hard_dabs_equal_the_reference() {
    hard_sweep(&[0.6, 1., 1.2, 2., 5., 15.8]);
}

#[test]
fn medium_hard_dabs_equal_the_reference() {
    hard_sweep(&[66.6]);
}

#[test]
fn a_256_px_hard_dab_equals_the_reference() {
    hard_sweep(&[256.]);
}

#[test]
fn a_512_px_hard_dab_equals_the_reference() {
    hard_sweep(&[512.]);
}

/// A 0.6-px hard dab at a pixel centre: its whole area lies inside pixel
/// (10, 10) (≈ 18/64 in the reference). Sub-1-px hard dabs are never
/// classified; the pixel is band.
#[test]
fn hard_sub_pixel_dab_inside_one_pixel_is_band() {
    let brush = Brush::hard(1., 0.);
    let dab = [10.5, 10.5, 0.6, 1.];
    assert_exact(brush, 21, 21, &dab, "0.6-px hard dab");
    let out = reference(brush, 21, 21, &dab);
    let centre = out[10 * 21 + 10];
    assert!(
        centre > 0 && centre < 65535,
        "the reference's pixel: {centre}"
    );
    assert!(brush.request().band_count((21, 21), &dab) >= 1);
}

/// A 1.2-px, roundness-0.01, 45° needle at a pixel centre: its parallelogram
/// surrounds the origin with every edge ≈ 1.18 normalized units away, so an
/// edge-distance-only rule would call the centre pixel EXTERIOR.
#[test]
fn hard_needle_inside_one_pixel_is_band() {
    let brush = Brush::hard(0.01, 45.);
    let dab = [10.5, 10.5, 1.2, 1.];
    assert_exact(brush, 21, 21, &dab, "1.2-px needle");
    let out = reference(brush, 21, 21, &dab);
    assert!(out[10 * 21 + 10] > 0, "the reference's centre pixel is 0");
    assert!(brush.request().band_count((21, 21), &dab) >= 1);
}

#[test]
fn pixel_mode_and_soft_dabs_equal_the_reference() {
    let brushes = [
        Brush::pixel(),
        Brush::pixel().opacity(0.6),
        Brush::soft(0., 1., 0.),
        Brush::soft(0., 0.37, 30.),
        Brush::soft(0.5, 1., 0.),
        Brush::soft(0.5, 0.01, -77.5),
        Brush::soft(0.5, 0.37, 45.).opacity(0.6),
        Brush::hard(0.37, 30.).opacity(0.6),
    ];
    for brush in brushes {
        for diameter in [0.6, 1., 2., 15.8, 66.6] {
            for (ox, oy) in OFFSETS {
                for opacity in [1., 0.4] {
                    let dab = [40. + ox, 40. + oy, diameter, opacity];
                    assert_exact(brush, 80, 80, &dab, &format!("diameter {diameter}"));
                }
            }
        }
    }
}

/// Overlapping strokes whose dabs differ in size and opacity: the source-over
/// accumulation must run in dab order on both paths.
#[test]
fn overlapping_strokes_equal_the_reference() {
    let stroke = (0..40)
        .flat_map(|k| {
            let t = f64::from(k) / 39.;
            [
                20. + 210. * t + 3.1 * (7. * t).sin(),
                30. + 70. * t + 25. * (4. * t).cos(),
                3. + 40. * (1. - t) + 7. * (5. * t).sin().abs(),
                0.15 + 0.8 * ((3. * t).sin() * 0.5 + 0.5),
            ]
        })
        .collect::<Vec<_>>();
    for brush in [
        Brush::hard(1., 0.),
        Brush::hard(0.37, 30.),
        Brush::hard(0.01, -77.5),
        Brush::soft(0.5, 0.37, 45.),
        Brush::soft(0., 1., 0.),
        Brush::pixel(),
        Brush::hard(1., 0.).opacity(0.6),
    ] {
        assert_exact(brush, 257, 129, &stroke, "overlapping stroke");
        let reversed = stroke
            .chunks_exact(4)
            .rev()
            .flatten()
            .copied()
            .collect::<Vec<_>>();
        assert_exact(brush, 257, 129, &reversed, "reversed stroke");
    }
}

/// Dabs straddling every edge and corner, entirely off canvas, of zero
/// diameter and of zero opacity, over 1×1, 3×2, 64×64 and 257×129.
#[test]
fn canvas_edges_and_degenerate_dabs_equal_the_reference() {
    for (w, h) in [(1, 1), (3, 2), (64, 64), (257, 129)] {
        let (fw, fh) = (f64::from(w), f64::from(h));
        let mut dabs = Vec::new();
        for diameter in [0.6, 1.7, 9.3, 40.2] {
            for (x, y) in [
                (0.3, fh / 2.),
                (fw - 0.2, fh / 2.),
                (fw / 2., 0.1),
                (fw / 2., fh - 0.4),
                (0., 0.),
                (fw, 0.),
                (0.2, fh + 0.1),
                (fw + 0.3, fh - 0.2),
            ] {
                dabs.extend([x, y, diameter, 0.7]);
            }
        }
        dabs.extend([-60., -60., 10., 1.]); // entirely off canvas
        dabs.extend([fw + 80., fh / 2., 30., 1.]);
        dabs.extend([fw / 2., fh / 2., 0., 1.]); // zero diameter
        dabs.extend([fw / 2., fh / 2., 12., 0.]); // zero opacity
        for brush in [
            Brush::hard(1., 0.),
            Brush::hard(0.37, 30.),
            Brush::soft(0.5, 1., 0.),
            Brush::pixel(),
        ] {
            assert_exact(brush, w, h, &dabs, &format!("edges on {w}×{h}"));
        }
    }
}

#[test]
fn a_stroke_with_nothing_to_draw_finishes_as_zeros() {
    let brush = Brush::hard(1., 0.);
    let off = [-60., -60., 10., 1., 50., 50., 0., 1., 50., 50., 12., 0.];
    for dabs in [&[][..], &off[..]] {
        assert_eq!(footprint((64, 64), dabs), None);
        let out = product(brush, 64, 64, dabs).unwrap();
        assert!(out.iter().all(|&v| v == 0));
        assert_exact(brush, 64, 64, dabs, "nothing to draw");
    }
    // The empty list: n + |F| = n, scratch 4 MiB + 2n.
    let p = plan(brush, 64, 64, &[]).unwrap();
    assert_eq!(p["work_units"], 4096);
    assert_eq!(p["scratch_bytes"], 4 * MIB + 2 * 4096);
}

#[test]
fn the_footprint_is_the_union_of_nonzero_dab_boxes() {
    // [15, 25, 26, 36) ∪ [56, 1, 64, 10) (clipped at 64) → [15, 1, 64, 36);
    // the zero-diameter and zero-opacity dabs join nothing.
    let dabs = [
        20.5, 30.5, 10., 1., 60.2, 5.9, 8., 0.3, 5., 5., 0., 1., 5., 5., 9., 0.,
    ];
    assert_eq!(footprint((64, 64), &dabs), Some([15, 1, 64, 36]));
    assert_eq!(footprint((64, 64), &dabs[..4]), Some([15, 25, 26, 36]));
}

// --------------------------------------------------------- beyond the cap --

/// 18 hard 512-px dabs fully on 1100×1100: the old charge `2n + Σ 64·box` is
/// ≈ 305M > 256M (refused before 13A-6c-1); the product finishes at 1100×1100
/// and equals the reference.
#[test]
fn a_hard_stroke_beyond_the_old_cap_finishes_and_equals_the_reference() {
    const W: u32 = 1100;
    let brush = Brush::hard(1., 0.);
    let dabs = (0..18)
        .flat_map(|k| {
            let (i, j) = (f64::from(k % 6), f64::from(k / 6));
            [300.3 + 100. * i, 300.7 + 250. * j, 512., 0.5]
        })
        .collect::<Vec<_>>();
    let old = old_work(brush, W, W, &dabs);
    assert!(
        old > OLD_CAP,
        "the fixture is not beyond the old cap: {old}"
    );
    let expected = reference(brush, W, W, &dabs);
    let actual = product(brush, W, W, &dabs).unwrap_or_else(|e| panic!("refused: {e}"));
    assert_eq!(actual.len(), (W * W) as usize);
    assert!(
        actual == expected,
        "the product differs from the reference ({})",
        first_difference(&actual, &expected, W)
    );
    assert!(actual.iter().any(|&v| v > 60000), "no ink");
    let work = plan(brush, W, W, &dabs).unwrap()["work_units"]
        .as_u64()
        .unwrap();
    eprintln!("18 hard 512-px dabs on 1100×1100: old charge {old}, new charge {work}");
    assert!(work <= OLD_CAP);
}

#[test]
fn oversized_strokes_still_refuse_with_the_work_sentence() {
    // A soft 1000-px 2,100-dab stroke: Σ box = 2,100·10⁶ > 2·10⁹ on its own.
    let soft = [550., 550., 1000., 1.].repeat(2100);
    // 50,000 hard 512-px dabs: each ≥ 512² + 64·band_bound(512) = 554,560,
    // 27.7·10⁹ in all.
    let hard = [550.5, 550.5, 512., 1.].repeat(50_000);
    assert_eq!(512 * 512 + 64 * band_bound(512.), 554_560);
    assert!(soft.len() as u64 / 4 * 1_000_000 > CAP);
    assert!(hard.len() as u64 / 4 * 554_560 > CAP);
    for (brush, dabs) in [
        (Brush::soft(0., 1., 0.), &soft[..]),
        (Brush::hard(1., 0.), &hard[..]),
    ] {
        assert_eq!(plan(brush, 1100, 1100, dabs).err().as_deref(), Some(WORK));
        assert_eq!(
            product(brush, 1100, 1100, dabs).err().as_deref(),
            Some(WORK)
        );
    }
}

#[test]
fn plan_checks_the_dab_count_before_the_footprint() {
    const COUNT: &str =
        "native brush exceeds 50000 dabs or has incomplete tuples; use a shorter stroke or wider spacing";
    let request = Brush::hard(1., 0.).request();
    let over = [32.5, 32.5, 8., 1.].repeat(50_001);
    let seven = [32.5, 32.5, 8., 1., 16.5, 16.5, 8.];
    assert_eq!(request.plan((64, 64), &over).err().as_deref(), Some(COUNT));
    assert_eq!(request.plan((64, 64), &seven).err().as_deref(), Some(COUNT));
}

// --------------------------------------------------------- band bound --

#[test]
fn the_band_never_exceeds_its_bound() {
    const SIDE: u32 = 1300;
    for diameter in [
        0.3, 0.99, 1., 1.01, 2., 3.7, 7., 16., 33.3, 64., 128., 255.5, 512., 800., 1200.,
    ] {
        let bound = band_bound(diameter);
        for roundness in ROUNDNESS {
            for angle in ANGLES {
                let request = Brush::hard(roundness, angle).request();
                for (ox, oy) in OFFSETS {
                    let dab = [650. + ox, 650. + oy, diameter, 1.];
                    let band = request.band_count((SIDE, SIDE), &dab);
                    assert!(
                        band <= bound,
                        "diameter {diameter}, roundness {roundness}, angle {angle}, \
                         +({ox}, {oy}): band {band} > bound {bound}"
                    );
                }
            }
        }
    }
    // Pixel and soft dabs have no band.
    let dab = [650.5, 650.5, 64., 1.];
    assert_eq!(Brush::pixel().request().band_count((SIDE, SIDE), &dab), 0);
    assert_eq!(
        Brush::soft(0.5, 1., 0.)
            .request()
            .band_count((SIDE, SIDE), &dab),
        0
    );
}

// ------------------------------------------------------------- scratch --

#[test]
fn scratch_is_pinned_to_the_footprint() {
    // One hard 10-px dab at (20.5, 30.5) on 64×64: F = [15, 25, 26, 36), 121
    // samples: 4 MiB + 8·121 + 2·4096 + 8·4.
    let one = [20.5, 30.5, 10., 1.];
    assert_eq!(
        plan(Brush::hard(1., 0.), 64, 64, &one).unwrap()["scratch_bytes"],
        4 * MIB + 968 + 8192 + 32
    );
    // Two soft dabs on 257×129: [15, 25, 26, 36) ∪ [200, 80, 257, 129) →
    // [15, 25, 257, 129) = 242·104 = 25,168: 4 MiB + 201,344 + 66,306 + 64.
    let two = [20.5, 30.5, 10., 1., 240.2, 120.9, 80., 0.3];
    assert_eq!(footprint((257, 129), &two), Some([15, 25, 257, 129]));
    assert_eq!(
        plan(Brush::soft(0.5, 1., 0.), 257, 129, &two).unwrap()["scratch_bytes"],
        4 * MIB + 201_344 + 66_306 + 64
    );
    // The beyond-cap fixture's shape: one 512-px dab at (300.3, 300.7) on
    // 1100×1100: F = [44, 44, 557, 557) = 263,169: 4 MiB + 2,105,352 +
    // 2,420,000 + 32.
    let big = [300.3, 300.7, 512., 1.];
    assert_eq!(
        plan(Brush::hard(1., 0.), 1100, 1100, &big).unwrap()["scratch_bytes"],
        4 * MIB + 2_105_352 + 2_420_000 + 32
    );
}

// ------------------------------------------------------------ cancellation --

const PW: u32 = 256;

fn parked_dabs() -> Vec<f64> {
    (0..6)
        .flat_map(|k| {
            [
                40.3 + 30. * f64::from(k),
                60.7 + 20. * f64::from(k),
                64.,
                0.8,
            ]
        })
        .collect()
}

fn edit(brush: Brush, w: u32, dabs: Vec<f64>) -> impl FnOnce(usize) -> (bool, String) {
    move |token| match product_with(brush, w, w, &dabs, token as *const DrJobControl) {
        Ok(_) => (true, String::new()),
        Err(e) => (false, e),
    }
}

#[test]
fn a_pre_cancelled_token_returns_nothing_and_holds_nothing() {
    let control = unsafe { dr_job_control_create() };
    unsafe { dr_job_control_cancel(control) };
    let result = product_with(Brush::hard(1., 0.), PW, PW, &parked_dabs(), control);
    let scratch = unsafe { dr_job_control_scratch_bytes(control) };
    unsafe { dr_job_control_free(control) };
    let e = result.expect_err("a pre-cancelled brush completed");
    assert!(e.contains("cancel"), "{e}");
    assert_eq!(scratch, 0);
}

#[test]
fn a_cancel_parked_inside_the_dab_loop_returns_nothing_then_completes_exactly() {
    let brush = Brush::hard(1., 0.);
    let n0 = entry_count(
        &|progress, stage, _| stage == 3 && progress > 0.,
        edit(brush, PW, parked_dabs()),
    );
    let p = cancelled_parked_at(
        n0 + 1 + 40,
        &|progress, stage| stage == 3 && progress > 0. && progress < 0.9,
        edit(brush, PW, parked_dabs()),
    );
    eprintln!(
        "13A-6c-1 parked cancel inside the brush's dab loop: {}",
        parked_line(&p)
    );
    let control = unsafe { dr_job_control_create() };
    let again = product_with(brush, PW, PW, &parked_dabs(), control);
    unsafe { dr_job_control_free(control) };
    let expected = reference(brush, PW, PW, &parked_dabs());
    assert!(again.unwrap() == expected, "the completed re-run differs");
}

// ------------------------------------------------ bench (cancel-bench) --

/// The bench's render: the bare FFI call into a caller-owned output, so the
/// probe's return stamp does not include `product_with`'s O(n) sentinel scan
/// or the output's deallocation.
fn bare_edit<'a>(
    text: &'a CString,
    w: u32,
    dabs: &'a [f64],
    out: &'a mut [u16],
) -> impl FnOnce(usize) -> (bool, String) + 'a {
    move |token| {
        let mut e = ptr::null_mut();
        let ok = unsafe {
            dr_brush_u16(
                w,
                w,
                text.as_ptr(),
                dabs.as_ptr(),
                dabs.len() / 4,
                out.as_mut_ptr(),
                out.len(),
                LIMIT,
                0,
                token as *const DrJobControl,
                &mut e,
            )
        };
        (ok, unsafe { message(e) })
    }
}

#[test]
#[ignore = "make cancel-bench: a wall-clock verdict (13A-6c-1)"]
fn a_concurrently_cancelled_brush_returns_within_one_dab_row() {
    // A hard 512-px round tip, 120 dabs 8 px apart along a diagonal, fully on
    // 1600×1600. `c_eval` from the uncancelled progress log between the first
    // logged progress ≥ 0.05 and the last ≤ 0.5, per evaluation `E_dab = box
    // + 64·band_count`.
    const W: u32 = 1600;
    const COUNT: usize = 120;
    let brush = Brush::hard(1., 0.);
    let step = 8. / std::f64::consts::SQRT_2;
    let dabs = (0..COUNT)
        .flat_map(|k| {
            let c = 300.3 + step * k as f64;
            [c, c + 0.4, 512., 1.]
        })
        .collect::<Vec<_>>();
    let request = brush.request();
    let e_dab = dabs
        .chunks_exact(4)
        .map(|d| (dab_box(d, W, W) + 64 * request.band_count((W, W), d)) as f64)
        .sum::<f64>()
        / COUNT as f64;
    let width = {
        let d = &dabs[..4];
        (d[0] + 256.).ceil() - (d[0] - 256.).floor()
    };
    let (log, _) = logged(edit(brush, W, dabs.clone()));
    let (t1, p1) = *log.iter().find(|&&(_, v)| v >= 0.05).unwrap();
    let (t2, p2) = *log.iter().rev().find(|&&(_, v)| v <= 0.5).unwrap();
    let between = (p2 - p1) / 0.9 * COUNT as f64;
    let c_eval = (t2 - t1).as_secs_f64() / (between * e_dab);
    let text = brush.text();
    let mut out = vec![SENTINEL; (W * W) as usize];
    let p = probe(
        &|progress, stage, _| stage == 3 && progress > 0.05,
        Trigger::After(500),
        &|progress, stage| stage == 3 && progress <= 0.6,
        bare_edit(&text, W, &dabs, &mut out),
    );
    assert!(
        out.iter().all(|&v| v == SENTINEL),
        "a cancelled brush wrote its output"
    );
    let row = 64. * width * c_eval;
    let bound = RETURN_FLOOR_S + row;
    eprintln!(
        "13A-6c-1 brush ({W}×{W}, {COUNT} hard 512-px dabs, E_dab {e_dab:.0}) cancel inside the \
         dab loop: {:.3} ms cancel-to-return (issued at {:.1} ms; {}); c_eval {:.3} ns ({:.1} \
         dabs timed); bound T_f + 64·{width}·c_eval = {:.3} ms (T_f 1 ms); ≈{:.2} rows past the \
         floor; {}",
        p.latency.as_secs_f64() * 1e3,
        p.at.as_secs_f64() * 1e3,
        checkpoints_line(&p),
        c_eval * 1e9,
        between,
        bound * 1e3,
        past(p.latency, row),
        verdict(p.latency, bound),
    );
}
