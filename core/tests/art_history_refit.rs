//! The Art History Brush's refit (batch 2; `art_history_stroke`'s
//! module doc), through the FFI, each against an oracle written here:
//!
//! * a stroke heads along the source's iso-LUMINANCE contours and travels
//!   with the brighter side on its right (BRIGHT-RIGHT): the oracle is the
//!   fixture's own luma gradient `g` (W3C Lum, 0.3/0.59/0.11, of the RGB or
//!   of the ink's naive RGB `1 − ink`), whose contour direction travelling
//!   bright-right is `atan2(−g_x, g_y)` (y down); a curl turns the heading
//!   angle down;
//! * a stroke is ONE colour, the source at its start pixel, at 8-bit RGB
//!   and 8-bit CMYK, on every fully covered pixel;
//! * the same seed and dabs paint the same bytes, and a dab prefix paints
//!   the full stroke's bytes outside the later clicks' reach.
//!
//! Every case is Area 0 on a 2 × 2 grid of clicks (one stroke a click,
//! starting at the click), so each quadrant holds exactly one isolated
//! stroke.

use std::ffi::{c_char, CString};
use std::ptr;

use doctored_core::doc::DrDocument;
use doctored_core::ffi_doc::dr_doc_free;
use doctored_core::ffi_retouch_brush::{
    dr_doc_art_history_brush_layer, dr_precision_doc_art_history_brush,
};
use doctored_core::precision_document::PrecisionDocument;
use image::{Rgba, RgbaImage};
use tempfile::TempDir;

use crate::cmyk_fixture::press_profile;
use crate::cmyk_structure_fixture::{free, open, LIMIT};
use crate::cmyk_tiff_builder::cmyk_tiff;
use crate::common::{layer_pixels, take_doc, take_err_string};

const W: u32 = 256;
const D: f64 = 5.;
const HARD: &str =
    r#"{"version":1,"hardness":1.0,"roundness":1.0,"angle_degrees":0.0,"pixel":false}"#;
const PATH_STYLES: [&str; 9] = [
    "tight_short",
    "tight_medium",
    "tight_long",
    "loose_medium",
    "loose_long",
    "tight_curl",
    "tight_curl_long",
    "loose_curl",
    "loose_curl_long",
];
/// The fitted table's longest `L` (`art_history_stroke::TABLE`,
/// loose_curl) and Loose's largest length factor.
const L_MAX: f64 = 3.55;
const MAX_SPREAD: f64 = 1.5;

fn request(style: &str, area: f64, seed: u64) -> String {
    format!(
        r#"{{"version":1,"style":"{style}","area":{area},"tolerance":0,"opacity":1,"blend_mode":0,"seed":{seed}}}"#
    )
}

/// Clicks at 64.5 and 192.5 on both axes: a stroke reaches at most
/// `1.5·L_max·D + D ≈ 32` px from its click, never the next quadrant.
fn grid() -> Vec<[f64; 4]> {
    let mut out = Vec::new();
    for j in 0..2 {
        for i in 0..2 {
            out.push([
                64.5 + 128. * f64::from(i),
                64.5 + 128. * f64::from(j),
                D,
                1.,
            ]);
        }
    }
    out
}

fn quadrant(x: u32, y: u32) -> usize {
    usize::from(x >= 128) + 2 * usize::from(y >= 128)
}

/// `degrees` into [−period/2, period/2).
fn wrap(degrees: f64, period: f64) -> f64 {
    (degrees + period / 2.).rem_euclid(period) - period / 2.
}

/// W3C Lum.
fn lum(c: [f64; 3]) -> f64 {
    0.3 * c[0] + 0.59 * c[1] + 0.11 * c[2]
}

/// The bright-right contour heading of a linear source whose RGB (unit) at
/// `(x, y)` is `rgb(x, y)`: the luma gradient by differences over the
/// canvas, then `atan2(−g_x, g_y)` in degrees.
fn bright_right(rgb: impl Fn(f64, f64) -> [f64; 3]) -> f64 {
    let gx = lum(rgb(200., 100.)) - lum(rgb(100., 100.));
    let gy = lum(rgb(100., 200.)) - lum(rgb(100., 100.));
    (-gx).atan2(gy).to_degrees()
}

/// One stroke's changed pixels (pixel centres): its principal axis and the
/// direction from its click to its centroid, in degrees.
fn axis_and_travel(pts: &[(f64, f64)], click: [f64; 4]) -> (f64, f64) {
    let n = pts.len() as f64;
    let mx = pts.iter().map(|p| p.0).sum::<f64>() / n;
    let my = pts.iter().map(|p| p.1).sum::<f64>() / n;
    let (mut sxx, mut syy, mut sxy) = (0., 0., 0.);
    for (x, y) in pts {
        sxx += (x - mx) * (x - mx);
        syy += (y - my) * (y - my);
        sxy += (x - mx) * (y - my);
    }
    let axis = 0.5 * (2. * sxy).atan2(sxx - syy);
    let travel = (my - click[1]).atan2(mx - click[0]);
    (axis.to_degrees(), travel.to_degrees())
}

/// The changed pixel centres of each quadrant.
type Quadrants = [Vec<(f64, f64)>; 4];

fn changed_by_quadrant(changed: impl Iterator<Item = (u32, u32)>) -> Quadrants {
    let mut out: [Vec<(f64, f64)>; 4] = Default::default();
    for (x, y) in changed {
        out[quadrant(x, y)].push((f64::from(x) + 0.5, f64::from(y) + 0.5));
    }
    out
}

fn median(mut v: Vec<f64>) -> f64 {
    v.sort_by(f64::total_cmp);
    let m = v.len() / 2;
    if v.len() % 2 == 1 {
        v[m]
    } else {
        (v[m - 1] + v[m]) / 2.
    }
}

// ------------------------------------------------------------- 8-bit RGB --

fn handle(d: &DrDocument) -> *mut DrDocument {
    Box::into_raw(Box::new(d.clone()))
}

/// `dr_doc_art_history_brush_layer` on layer 0 from layer 0 of `source`.
fn rgb_art(
    current: &DrDocument,
    source: &DrDocument,
    request: &str,
    dabs: &[[f64; 4]],
) -> Vec<[u8; 4]> {
    let (d, s) = (handle(current), handle(source));
    let request = CString::new(request).unwrap();
    let brush = CString::new(HARD).unwrap();
    let flat: Vec<f64> = dabs.iter().flatten().copied().collect();
    let mut err: *mut c_char = ptr::null_mut();
    let out = unsafe {
        dr_doc_art_history_brush_layer(
            d,
            0,
            s,
            0,
            request.as_ptr(),
            brush.as_ptr(),
            flat.as_ptr(),
            dabs.len(),
            ptr::null(),
            0,
            &mut err,
        )
    };
    unsafe {
        dr_doc_free(d);
        dr_doc_free(s);
    }
    let (out, err) = take_doc(out, err);
    let out = out.unwrap_or_else(|| panic!("{request:?}: {err:?}"));
    layer_pixels(&out, 0)
        .chunks_exact(4)
        .map(|p| [p[0], p[1], p[2], p[3]])
        .collect()
}

fn rgb_doc(f: impl Fn(u32, u32) -> [u8; 3]) -> DrDocument {
    DrDocument::from_pixels(RgbaImage::from_fn(W, W, |x, y| {
        let [r, g, b] = f(x, y);
        Rgba([r, g, b, 255])
    }))
}

fn white() -> DrDocument {
    DrDocument::from_pixels(RgbaImage::from_pixel(W, W, Rgba([255; 4])))
}

/// The take's fixtures A, B and C (`art_history_take.py` TAKE2).
type Fixture = fn(u32, u32) -> [u8; 3];
const RGB_FIXTURES: [(&str, Fixture); 3] = [
    ("A", |x, y| [x as u8, y as u8, 0]),
    ("B", |x, y| [255 - x as u8, 255 - y as u8, 0]),
    ("C", |x, y| [x as u8, 0, y as u8]),
];

fn unit(f: Fixture) -> impl Fn(f64, f64) -> [f64; 3] {
    move |x, y| f(x as u32, y as u32).map(|v| f64::from(v) / 255.)
}

fn rgb_strokes(fixture: Fixture, style: &str, seed: u64) -> Quadrants {
    let blank = white();
    let before = layer_pixels(&blank, 0);
    let after = rgb_art(
        &blank,
        &rgb_doc(fixture),
        &request(style, 0., seed),
        &grid(),
    );
    changed_by_quadrant(after.iter().enumerate().filter_map(|(i, p)| {
        (p[..] != before[i * 4..i * 4 + 4]).then_some((i as u32 % W, i as u32 / W))
    }))
}

// ------------------------------------------------------------ 8-bit CMYK --

/// An 8-bit CMYK document of `ink(x, y)` (`[c, m, y, k]` codes), opened
/// from a hand-built TIFF in the synthetic press profile.
fn cmyk_doc(
    dir: &TempDir,
    name: &str,
    ink: impl Fn(u32, u32) -> [u8; 4],
) -> *mut PrecisionDocument {
    let data: Vec<u8> = (0..W * W).flat_map(|i| ink(i % W, i / W)).collect();
    let path = dir.path().join(format!("{name}.tif"));
    let profile = press_profile("Art History Refit Press");
    std::fs::write(&path, cmyk_tiff(W, W, 8, 4, 0, &data, Some(&profile))).unwrap();
    open(&path).unwrap()
}

/// Ink C = x, M = y (its naive RGB `1 − ink` is fixture B's luma).
fn ink_ramp(x: u32, y: u32) -> [u8; 4] {
    [x as u8, y as u8, 0, 0]
}

/// `dr_precision_doc_art_history_brush` on layer 0 from layer 0 of
/// `source`: the layer's samples after.
fn cmyk_art(
    current: *const PrecisionDocument,
    source: *const PrecisionDocument,
    request: &str,
    dabs: &[[f64; 4]],
) -> Vec<[u16; 5]> {
    let text = CString::new(request).unwrap();
    let brush = CString::new(HARD).unwrap();
    let flat: Vec<f64> = dabs.iter().flatten().copied().collect();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_art_history_brush(
            current,
            0,
            source,
            0,
            text.as_ptr(),
            brush.as_ptr(),
            flat.as_ptr(),
            dabs.len(),
            ptr::null(),
            0,
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!out.is_null(), "{request}: {}", take_err_string(err));
    let samples = unsafe { &*out }.cmyk_layer_samples(0).unwrap();
    free(out);
    samples
}

// ------------------------------------------------------------------ tests --

#[test]
fn tight_strokes_head_along_the_luma_contours_and_travel_bright_right() {
    let dir = TempDir::new().unwrap();
    let paper = cmyk_doc(&dir, "paper", |_, _| [0; 4]);
    let ink = cmyk_doc(&dir, "ink", ink_ramp);
    let paper_samples = unsafe { &*paper }.cmyk_layer_samples(0).unwrap();
    let ink_rgb = |x: f64, y: f64| [1. - x / 255., 1. - y / 255., 1.];
    let mut cases: Vec<(&str, f64, Vec<Quadrants>)> = Vec::new();
    for (name, fixture) in RGB_FIXTURES {
        let seeds = (0..20)
            .map(|s| rgb_strokes(fixture, "tight_long", s))
            .collect();
        cases.push((name, bright_right(unit(fixture)), seeds));
    }
    let seeds = (0..20)
        .map(|s| {
            let after = cmyk_art(paper, ink, &request("tight_long", 0., s), &grid());
            changed_by_quadrant(after.iter().enumerate().filter_map(|(i, p)| {
                (*p != paper_samples[i]).then_some((i as u32 % W, i as u32 / W))
            }))
        })
        .collect();
    cases.push(("cmyk8", bright_right(ink_rgb), seeds));
    // The oracle agrees with the take's declared predictions.
    let declared = [-26.95, 153.05, -69.86, 153.05];
    for ((name, travel, _), want) in cases.iter().zip(declared) {
        assert!((travel - want).abs() < 0.01, "{name}: {travel}");
    }
    let clicks = grid();
    for (name, travel, seeds) in &cases {
        let mut offsets = Vec::new();
        for quadrants in seeds {
            for (q, pts) in quadrants.iter().enumerate() {
                assert!(pts.len() >= 20, "{name}: a stroke of {} px", pts.len());
                let (axis, heading) = axis_and_travel(pts, clicks[q]);
                offsets.push(wrap(axis - travel, 180.));
                assert!(
                    wrap(heading - travel, 360.).abs() <= 45.,
                    "{name}: a stroke travels {heading:.1}° against {travel:.2}°"
                );
            }
        }
        let axis = median(offsets);
        assert!(
            axis.abs() <= 8.,
            "{name}: the median axis is {axis:.1}° off"
        );
    }
    // A curl turns the heading angle down: every tight_curl stroke's travel
    // lies on the decreasing side of the straight heading.
    let (_, fixture) = RGB_FIXTURES[0];
    let straight = bright_right(unit(fixture));
    for seed in 0..20 {
        for (q, pts) in rgb_strokes(fixture, "tight_curl", seed).iter().enumerate() {
            let (_, heading) = axis_and_travel(pts, clicks[q]);
            let offset = wrap(heading - straight, 360.);
            assert!(
                offset > -180. && offset < 0.,
                "tight_curl seed {seed}: offset {offset:.1}°"
            );
        }
    }
    free(paper);
    free(ink);
}

#[test]
fn a_stroke_is_one_colour_the_source_at_its_start() {
    // A pixel is FULLY covered when the fixture's marker channel says so:
    // fixture A's B is 0 everywhere and white's 255, so a stroke pixel at
    // coverage c has B = (1 − c)·255 and B = 0 leaves R and G within half a
    // code of the stroke's colour; on CMYK the ramp's Y is 255 everywhere
    // and paper's 0, so Y = 255 does the same for C and M. (A geometric
    // interior cannot tell: a partly covered pixel at a slanted or concave
    // edge can have every neighbour changed.)
    let clicks = grid();
    let start = |x: u32, y: u32| {
        let q = quadrant(x, y);
        (clicks[q][0].floor() as u32, clicks[q][1].floor() as u32)
    };
    let (_, fixture) = RGB_FIXTURES[0];
    let (source, blank) = (rgb_doc(fixture), white());
    for style in PATH_STYLES {
        for seed in 0..3 {
            let after = rgb_art(&blank, &source, &request(style, 0., seed), &clicks);
            let mut covered = [0; 4];
            for (i, p) in after.iter().enumerate() {
                let (x, y) = (i as u32 % W, i as u32 / W);
                if p[2] != 0 {
                    continue;
                }
                let (sx, sy) = start(x, y);
                let [r, g, b] = fixture(sx, sy);
                assert_eq!(*p, [r, g, b, 255], "{style} seed {seed} at ({x}, {y})");
                covered[quadrant(x, y)] += 1;
            }
            assert!(
                covered.iter().all(|&n| n >= 4),
                "{style} seed {seed}: {covered:?}"
            );
        }
    }
    let dir = TempDir::new().unwrap();
    let paper = cmyk_doc(&dir, "paper", |_, _| [0; 4]);
    let ink = cmyk_doc(&dir, "ink", |x, y| [x as u8, y as u8, 255, 0]);
    let source = unsafe { &*ink }.cmyk_layer_samples(0).unwrap();
    for style in PATH_STYLES {
        for seed in 0..3 {
            let after = cmyk_art(paper, ink, &request(style, 0., seed), &clicks);
            let mut covered = [0; 4];
            for (i, p) in after.iter().enumerate() {
                let (x, y) = (i as u32 % W, i as u32 / W);
                if p[2] != 65535 {
                    continue;
                }
                let (sx, sy) = start(x, y);
                assert_eq!(
                    *p,
                    source[(sy * W + sx) as usize],
                    "{style} seed {seed} at ({x}, {y})"
                );
                covered[quadrant(x, y)] += 1;
            }
            assert!(
                covered.iter().all(|&n| n >= 4),
                "{style} seed {seed}: {covered:?}"
            );
        }
    }
    free(paper);
    free(ink);
}

#[test]
fn the_same_seed_and_a_dab_prefix_paint_the_same_bytes() {
    let (_, fixture) = RGB_FIXTURES[0];
    let (source, blank) = (rgb_doc(fixture), white());
    // Two clicks 44.7 px apart at Area 50 (the second is past area/2).
    let dabs = [[100., 100., 6., 1.], [140., 120., 6., 1.]];
    for style in PATH_STYLES {
        let req = request(style, 50., 7);
        let full = rgb_art(&blank, &source, &req, &dabs);
        assert_eq!(full, rgb_art(&blank, &source, &req, &dabs), "{style}");
        let prefix = rgb_art(&blank, &source, &req, &dabs[..1]);
        // `art_history_stroke::reach` at the fitted table's longest L.
        let reach = 50. / 2. + MAX_SPREAD * L_MAX * 6. + 6. + 2.;
        let mut compared = 0;
        for (i, (a, b)) in full.iter().zip(&prefix).enumerate() {
            let (x, y) = (f64::from(i as u32 % W) + 0.5, f64::from(i as u32 / W) + 0.5);
            if (x - dabs[1][0]).hypot(y - dabs[1][1]) > reach {
                assert_eq!(a, b, "{style} at ({x}, {y})");
                compared += 1;
            }
        }
        assert!(compared > 0);
        assert!(
            prefix.iter().any(|p| *p != [255; 4]),
            "{style}: the prefix paints"
        );
    }
}
