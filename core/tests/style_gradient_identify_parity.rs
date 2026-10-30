//! Linear, Angle, Reflected and Radial gradients against Photoshop's
//! own gradient-identify-673 take1 and take2 profiles, at 8 and 16 bits, plus
//! an independent oracle of the shipped law. The fixtures
//! (`tests/fixtures/psd_photoshop/gradient_identify_673/profiles.json` and
//! `take2.json`, README) hold every Linear, Angle, Reflected and Radial
//! identify row the runner judged, written by
//! `scripts/photoshop-probe/gradient_identify_673_fit.py --fixtures`: Linear
//! and Angle on the SHIP of its `--judge-style-split` (the controller's style
//! split: a style ships ramp and geometry together), Reflected on its
//! `--judge-phase2` SHIP (the controller's phase-2 ruling), Radial on its
//! `--judge-rint` SHIP (the controller's take-2 RULING-1; take1's 48 rows on
//! 256 x 256 and take2's 216 on six boxes, odd and even). Diamond keeps
//! today's kernel; the Shape Burst's take-2 rows are
//! `style_shape_burst_identify_parity.rs`'s.
//!
//! Each row is rebuilt as the take runner builds it on a sRGB canvas the size
//! of the row's `box` (absent: 256 x 256, take1): a Gradient Fill layer
//! (`f_*`) is the `gradient`
//! artwork component rendered by `dr_artwork_render` over the canvas and
//! published by `dr_doc_artwork` (8 bits) or `dr_precision_doc_artwork`
//! (16 bits); a Gradient Overlay (`o_*`) is a Normal 100 % overlay on a
//! full-canvas opaque black layer over opaque black
//! (`common/identify_578.rs`'s `render8` / `render16`). The profile is the
//! red channel at the fixture's pixels, in 8-bit codes (16-bit samples /
//! 257). A row's error is the max and the mean of `|ours − Photoshop|` over
//! its entries outside its bin (the fixture's `seam`): the Angle entries
//! within 2 degrees of Photoshop's seam, where a pixel straddling the
//! discontinuity reads the first or the last stop on any sub-pixel offset,
//! the Reflected entries within 1.5 pixels of the fold line and the Radial
//! entries within 1.5 pixels of the centre (CENTRE).
//!
//! The bound, declared in the runner before the take, is max <= 2 and mean
//! <= 0.5. A row that misses it asserts its own measured number (`PINNED`,
//! rounded up in the third decimal), never a wider one, and that it still
//! misses.
//!
//! The oracle test holds the same renders to the law as this paragraph
//! states it, written here and never read off the core: the
//! pixel's TOP-LEFT CORNER `(x, y)` is the sample; Linear is `t = 0.5 + u /
//! E`, `u` the corner's projection on the angle from the box centre and `E
//! = min(bw / |cos a|, bh / |sin a|) · scale`; Reflected is the Linear
//! coordinate folded, `t = |2u / E|` with the same `E`; Angle is `t = ((a −
//! θ) mod 360°) / 360°`, `θ = atan2(−dy, dx)` (clockwise on screen); Reverse
//! is `1 − t`; Radial is `t = |p − c| / (E / 2)` with `p` the pixel's integer
//! coordinate, `c` the integer centre `(⌊bw/2⌋, ⌊bh/2⌋)` and `E / 2` the
//! length of the vector `(⌊R |cos a|⌋, ⌊R |sin a|⌋)`, `R` half Linear's `E`;
//! the colour is `common::classic_ramp` held to its bracketing stops (the
//! Gradient Fill layer's bracket bound) and the opacity linear. Below
//! Smoothness 100 % the colour is `common::bracket_ramp`, `(1 − k) ·
//! linear + k · classic_ramp` (bracket-held) with `k` the Smoothness
//! fraction (take 2's law K1).
//!
//! Smoothness: `take2_smooth.json` holds take 2's 20 Smoothness rows
//! (Linear, 0°, two and three stops, 0 to 100 %, 8 and 16 bits), each scored
//! as a Gradient Fill row at the same bound; a row that misses is pinned in
//! `PINNED_SMOOTH`.
//!
//! Midpoints: `take2_midpoint.json` holds take 2's 10 midpoint rows
//! (Linear, 0°, the three stops each at one `Mdpn`, 10 to 90 %, 8 and 16
//! bits), written on the fit runner's `--judge-midpoints` SHIP; each is
//! scored as a Gradient Fill row whose stops carry the row's midpoints, at
//! the same bound; a row that misses is pinned in `PINNED_MIDPOINT`, below
//! the judge's TODAY number only.

use std::collections::BTreeMap;
use std::ffi::{c_char, CStr, CString};
use std::path::Path;
use std::ptr;

use doctored_core::doc::DrDocument;
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_artwork::{dr_artwork_render, dr_doc_artwork, dr_precision_doc_artwork};
use doctored_core::icc::IccProfile;
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use image::{Rgba, RgbaImage};
use serde_json::{json, Value};

use crate::common::{bracket_ramp, layer_pixels};
use crate::identify_578::{render16, render8};

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
const SIZE: u32 = 256;
const MAX: f64 = 2.0;
const MEAN: f64 = 0.5;

// Measured on the shipped kernel, max and mean in codes rounded up in the
// third decimal, for the rows that miss 2 / 0.5. Reflected's two take1 8-bit
// rows at 135° and scale 50 (the runner's `--judge-phase2` measured the same
// 2.0 / 0.5039 and 2.0 / 0.5118; today's kernel was 16 / 4.40). Radial's 32
// take2 rows are its `--judge-rint` NAMED BINS, the residuals the law leaves
// open: ODD150, scale 150 on the odd boxes 13 x 19, 19 x 19 and 61 x 19
// off the axes, where the end point leaves the box; INT, 244 x 76 at 135° and
// scales 50 and 150, a radius component exactly on an integer (19.0, 57.0).
const PINNED: &[(&str, f64, f64)] = &[
    ("f_ref_a135_s50_r0_8", 2.0, 0.504),
    ("f_ref_a135_s50_r1_8", 2.0, 0.512),
    ("f_rad_b13x19_a135_s150_16", 11.221, 6.495),
    ("f_rad_b13x19_a135_s150_8", 11.0, 6.77),
    ("f_rad_b13x19_a60_s150_16", 9.882, 5.013),
    ("f_rad_b13x19_a60_s150_8", 10.0, 5.231),
    ("f_rad_b19x19_a135_s150_16", 11.118, 6.157),
    ("f_rad_b19x19_a135_s150_8", 11.0, 6.125),
    ("f_rad_b19x19_a60_s150_16", 9.882, 5.726),
    ("f_rad_b19x19_a60_s150_8", 10.0, 5.875),
    ("f_rad_b244x76_a135_s150_16", 1.848, 0.735),
    ("f_rad_b244x76_a135_s150_8", 2.0, 0.785),
    ("f_rad_b244x76_a135_s50_16", 5.39, 1.136),
    ("f_rad_b244x76_a135_s50_8", 6.0, 1.165),
    ("f_rad_b61x19_a135_s150_16", 7.201, 2.993),
    ("f_rad_b61x19_a135_s150_8", 8.0, 3.0),
    ("f_rad_b61x19_a60_s150_16", 11.104, 4.166),
    ("f_rad_b61x19_a60_s150_8", 11.0, 4.244),
    ("o_rad_b13x19_a135_s150_16", 11.211, 6.493),
    ("o_rad_b13x19_a135_s150_8", 11.0, 6.77),
    ("o_rad_b13x19_a60_s150_16", 9.872, 5.012),
    ("o_rad_b13x19_a60_s150_8", 10.0, 5.231),
    ("o_rad_b19x19_a135_s150_16", 11.118, 6.157),
    ("o_rad_b19x19_a135_s150_8", 11.0, 6.125),
    ("o_rad_b19x19_a60_s150_16", 9.872, 5.724),
    ("o_rad_b19x19_a60_s150_8", 10.0, 5.875),
    ("o_rad_b244x76_a135_s150_16", 1.858, 0.74),
    ("o_rad_b244x76_a135_s150_8", 2.0, 0.785),
    ("o_rad_b244x76_a135_s50_16", 5.401, 1.144),
    ("o_rad_b244x76_a135_s50_8", 6.0, 1.165),
    ("o_rad_b61x19_a135_s150_16", 7.191, 2.994),
    ("o_rad_b61x19_a135_s150_8", 8.0, 3.0),
    ("o_rad_b61x19_a60_s150_16", 11.094, 4.168),
    ("o_rad_b61x19_a60_s150_8", 11.0, 4.244),
];

fn owned(p: *mut c_char) -> Option<String> {
    if p.is_null() {
        return None;
    }
    let text = unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned();
    unsafe { dr_string_free(p) };
    Some(text)
}

fn fixture(file: &str) -> Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/psd_photoshop/gradient_identify_673")
        .join(file);
    serde_json::from_str(&std::fs::read_to_string(path).expect(file)).expect("json")
}

/// One row's gradient as the take sends it, over its `w × h` document.
struct Gradient {
    w: u32,
    h: u32,
    style: &'static str,
    angle: f64,
    scale: f64,
    reverse: bool,
    stops: Vec<(f64, f64)>,
    /// Smoothness as a fraction (1 = 100 %, the Classic ramp).
    smoothness: f64,
    /// Each stop's midpoint as a fraction (0.5 = Photoshop's 50 %).
    midpoints: Vec<f64>,
}

fn gradient_of(row: &Value) -> Gradient {
    let style = match row["style"].as_str().expect("style") {
        "lin" => "linear",
        "ang" => "angle",
        "ref" => "reflected",
        "rad" => "radial",
        other => panic!("{other}: not a shipped style"),
    };
    let (w, h) = match row.get("box") {
        Some(b) => (
            b[0].as_u64().expect("bw") as u32,
            b[1].as_u64().expect("bh") as u32,
        ),
        None => (SIZE, SIZE),
    };
    Gradient {
        w,
        h,
        style,
        angle: row["angle"].as_f64().expect("angle"),
        scale: row["scale"].as_f64().expect("scale") / 100.0,
        reverse: row["reverse"].as_i64().expect("reverse") != 0,
        stops: row["stops"]
            .as_array()
            .expect("stops")
            .iter()
            .map(|s| {
                (
                    s[0].as_f64().expect("position"),
                    s[1].as_f64().expect("value"),
                )
            })
            .collect(),
        smoothness: row
            .get("smoothness")
            .map_or(1.0, |s| s.as_f64().expect("smoothness") / 100.0),
        midpoints: match row.get("midpoints") {
            Some(m) => m
                .as_array()
                .expect("midpoints")
                .iter()
                .map(|v| v.as_f64().expect("midpoint") / 100.0)
                .collect(),
            None => vec![0.5; row["stops"].as_array().expect("stops").len()],
        },
    }
}

/// The `GradientFill` object (`style_json::gradient`): gray stops, opaque;
/// `smoothness` only when not 100 %, a stop's `midpoint` only when not 50 %.
fn gradient_json(g: &Gradient) -> Value {
    let stops: Vec<Value> = g
        .stops
        .iter()
        .zip(&g.midpoints)
        .map(|(&(position, value), &midpoint)| {
            let code = (value * 255.0).round() as u8;
            let mut stop = json!({"position": position,
                "color": format!("#{code:02x}{code:02x}{code:02x}"), "opacity": 1.0});
            if midpoint != 0.5 {
                stop["midpoint"] = json!(midpoint);
            }
            stop
        })
        .collect();
    let mut out = json!({"stops": stops, "style": g.style, "angle": g.angle, "scale": g.scale,
        "reverse": g.reverse, "align_with_layer": false});
    if g.smoothness != 1.0 {
        out["smoothness"] = json!(g.smoothness);
    }
    out
}

/// The fill layer's 16-bit render over the canvas (`dr_artwork_render`).
fn render_fill(g: &Gradient) -> PrecisionImage {
    let colors: Vec<[f64; 3]> = g.stops.iter().map(|s| [s.1; 3]).collect();
    let components = json!([{"kind": "gradient", "gradient": gradient_json(g), "colors": colors}]);
    let description = CString::new(components.to_string()).unwrap();
    let profile = IccProfile::srgb();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_artwork_render(
            g.w,
            g.h,
            description.as_ptr(),
            ptr::null(),
            0,
            ptr::null(),
            0,
            profile.bytes().as_ptr(),
            profile.bytes().len(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert_eq!(owned(err), None);
    assert!(!out.is_null());
    *unsafe { Box::from_raw(out) }
}

fn fill_description(g: &Gradient) -> CString {
    let text = json!({"type": "fill", "version": 1, "canvas": [g.w, g.h],
        "fill": "gradient", "gradient": gradient_json(g)});
    CString::new(text.to_string()).unwrap()
}

/// The fill layer published at 8 bits (`dr_doc_artwork`): its red codes.
fn fill8(g: &Gradient) -> Vec<f64> {
    let image = render_fill(g);
    let doc = DrDocument::from_pixels(RgbaImage::from_pixel(g.w, g.h, Rgba([0, 0, 0, 255])));
    let name = CString::new("Fill").unwrap();
    let text = fill_description(g);
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_doc_artwork(
            &doc,
            0,
            false,
            &image,
            0,
            0,
            name.as_ptr(),
            text.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert_eq!(owned(err), None);
    assert!(!out.is_null());
    let published = unsafe { Box::from_raw(out) };
    layer_pixels(&*published, 1)
        .chunks_exact(4)
        .map(|p| f64::from(p[0]))
        .collect()
}

/// The fill layer published at 16 bits (`dr_precision_doc_artwork`): its
/// red in 8-bit codes.
fn fill16(g: &Gradient) -> Vec<f64> {
    let image = render_fill(g);
    let doc = PrecisionDocument::from_image(
        PrecisionImage::from_rgba16(
            g.w,
            g.h,
            vec![[0, 0, 0, 65535]; (g.w * g.h) as usize],
            IccProfile::srgb(),
            None,
        )
        .unwrap(),
        Default::default(),
        Default::default(),
    )
    .unwrap();
    let name = CString::new("Fill").unwrap();
    let text = fill_description(g);
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_artwork(
            &doc,
            0,
            false,
            &image,
            0,
            0,
            name.as_ptr(),
            text.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert_eq!(owned(err), None);
    assert!(!out.is_null());
    let published = *unsafe { Box::from_raw(out) };
    published
        .layer_source(1)
        .unwrap()
        .to_rgba16()
        .ok()
        .unwrap()
        .into_iter()
        .map(|p| f64::from(p[0]) / 257.0)
        .collect()
}

/// A Normal 100 % Gradient Overlay on the full-canvas black layer: red codes.
fn overlay(g: &Gradient, depth: u64) -> Vec<f64> {
    let style = json!({"effects": [{"type": "gradient_overlay", "blend": "normal",
        "opacity": 1.0, "gradient": gradient_json(g)}]});
    let mut style = style;
    style["effects"][0]["gradient"]["align_with_layer"] = json!(true);
    let inside = vec![true; (g.w * g.h) as usize];
    if depth == 8 {
        render8(g.w, g.h, &inside, &style)
    } else {
        render16(g.w, g.h, &inside, &style)
    }
}

/// The red codes of one row at its depth, the whole canvas row-major, and
/// the canvas width.
fn render(row: &Value) -> (Vec<f64>, u32) {
    let g = gradient_of(row);
    let depth = row["depth"].as_u64().expect("depth");
    let codes = match (row["item"].as_str().expect("item"), depth) {
        ("fill", 8) => fill8(&g),
        ("fill", 16) => fill16(&g),
        ("overlay", d) => overlay(&g, d),
        (item, d) => panic!("{item} at {d} bits"),
    };
    (codes, g.w)
}

fn pixels_of<'a>(fixture: &'a Value, row: &Value) -> &'a Vec<Value> {
    let key = row["pixels"].as_str().expect("pixels key");
    fixture["pixels"][key].as_array().expect("pixels")
}

/// `(max, mean)` of one row outside its SEAM bin, in 8-bit codes.
fn measure(fixture: &Value, row: &Value) -> (f64, f64) {
    let (codes, w) = render(row);
    let pixels = pixels_of(fixture, row);
    let ps = row["codes"].as_array().expect("codes");
    assert_eq!(pixels.len(), ps.len(), "the profile's length");
    // A Smoothness or midpoint row carries no bin (`take2_smooth.json`,
    // `take2_midpoint.json`).
    let seam: Vec<u64> = row.get("seam").map_or_else(Vec::new, |seam| {
        seam.as_array()
            .expect("seam")
            .iter()
            .map(|k| k.as_u64().expect("index"))
            .collect()
    });
    let (mut worst, mut total, mut n) = (0.0f64, 0.0, 0usize);
    for (k, (p, want)) in pixels.iter().zip(ps).enumerate() {
        if seam.contains(&(k as u64)) {
            continue;
        }
        let (x, y) = (p[0].as_u64().unwrap(), p[1].as_u64().unwrap());
        let ours = codes[(y * u64::from(w) + x) as usize];
        let e = (ours - want.as_f64().expect("code")).abs();
        worst = worst.max(e);
        total += e;
        n += 1;
    }
    assert!(n > 0, "a row with every entry binned");
    (worst, total / n as f64)
}

#[test]
fn every_shipped_identify_row_meets_the_bound_or_its_measured_residual() {
    let take1 = fixture("profiles.json");
    let take2 = fixture("take2.json");
    let rows1: BTreeMap<String, Value> =
        serde_json::from_value(take1["rows"].clone()).expect("rows");
    let rows2: BTreeMap<String, Value> =
        serde_json::from_value(take2["rows"].clone()).expect("rows");
    assert_eq!(
        rows1.len(),
        154 + 48,
        "54 Linear, 52 Angle, 48 Reflected and 48 Radial take1 rows"
    );
    assert_eq!(rows2.len(), 216, "216 Radial take2 rows");
    for (name, ..) in PINNED {
        assert!(
            rows1.contains_key(*name) || rows2.contains_key(*name),
            "{name}: pinned but not in a fixture"
        );
    }
    let mut failures = Vec::new();
    let all = rows1
        .iter()
        .map(|r| (&take1, r))
        .chain(rows2.iter().map(|r| (&take2, r)));
    for (fixture, (name, row)) in all {
        let (max, mean) = measure(fixture, row);
        eprintln!("{name}: max {max:.6} mean {mean:.6}");
        match PINNED.iter().find(|p| p.0 == name.as_str()) {
            None => {
                if !(max <= MAX && mean <= MEAN) {
                    failures.push(format!(
                        "{name}: max {max:.6} mean {mean:.6} misses 2 / 0.5"
                    ));
                }
            }
            Some((_, max_pin, mean_pin)) => {
                if !(max <= *max_pin && mean <= *mean_pin) {
                    failures.push(format!(
                        "{name}: max {max:.6} (pinned {max_pin}), mean {mean:.6} (pinned \
                         {mean_pin})"
                    ));
                }
                if max <= MAX && mean <= MEAN {
                    failures.push(format!(
                        "{name}: max {max:.6} mean {mean:.6} now meets 2 / 0.5: unpin it"
                    ));
                }
            }
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

/// Measured on the shipped kernel for the midpoint rows that miss 2 / 0.5
/// (`take2_midpoint.json`), max and mean rounded up in the third decimal.
const PINNED_MIDPOINT: &[(&str, f64, f64)] = &[];

#[test]
fn every_midpoint_row_meets_the_bound_or_its_measured_residual() {
    let midpoint = fixture("take2_midpoint.json");
    let rows: BTreeMap<String, Value> =
        serde_json::from_value(midpoint["rows"].clone()).expect("rows");
    assert_eq!(rows.len(), 10, "take 2's 10 midpoint rows");
    for (name, ..) in PINNED_MIDPOINT {
        assert!(
            rows.contains_key(*name),
            "{name}: pinned but not in the fixture"
        );
    }
    let mut failures = Vec::new();
    for (name, row) in &rows {
        assert_eq!(row["item"], "fill", "{name}: a Gradient Fill row");
        let (max, mean) = measure(&midpoint, row);
        eprintln!("{name}: max {max:.6} mean {mean:.6}");
        match PINNED_MIDPOINT.iter().find(|p| p.0 == name.as_str()) {
            None => {
                if !(max <= MAX && mean <= MEAN) {
                    failures.push(format!(
                        "{name}: max {max:.6} mean {mean:.6} misses 2 / 0.5"
                    ));
                }
            }
            Some((_, max_pin, mean_pin)) => {
                if !(max <= *max_pin && mean <= *mean_pin) {
                    failures.push(format!(
                        "{name}: max {max:.6} (pinned {max_pin}), mean {mean:.6} (pinned \
                         {mean_pin})"
                    ));
                }
                if max <= MAX && mean <= MEAN {
                    failures.push(format!(
                        "{name}: max {max:.6} mean {mean:.6} now meets 2 / 0.5: unpin it"
                    ));
                }
            }
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

/// Measured on the shipped kernel for the Smoothness rows that miss 2 / 0.5
/// (`take2_smooth.json`), max and mean rounded up in the third decimal.
const PINNED_SMOOTH: &[(&str, f64, f64)] = &[];

#[test]
fn every_smoothness_row_meets_the_bound_or_its_measured_residual() {
    let smooth = fixture("take2_smooth.json");
    let rows: BTreeMap<String, Value> =
        serde_json::from_value(smooth["rows"].clone()).expect("rows");
    assert_eq!(rows.len(), 20, "take 2's 20 Smoothness rows");
    for (name, ..) in PINNED_SMOOTH {
        assert!(
            rows.contains_key(*name),
            "{name}: pinned but not in the fixture"
        );
    }
    let mut failures = Vec::new();
    for (name, row) in &rows {
        assert_eq!(row["item"], "fill", "{name}: a Gradient Fill row");
        let (max, mean) = measure(&smooth, row);
        eprintln!("{name}: max {max:.6} mean {mean:.6}");
        match PINNED_SMOOTH.iter().find(|p| p.0 == name.as_str()) {
            None => {
                if !(max <= MAX && mean <= MEAN) {
                    failures.push(format!(
                        "{name}: max {max:.6} mean {mean:.6} misses 2 / 0.5"
                    ));
                }
            }
            Some((_, max_pin, mean_pin)) => {
                if !(max <= *max_pin && mean <= *mean_pin) {
                    failures.push(format!(
                        "{name}: max {max:.6} (pinned {max_pin}), mean {mean:.6} (pinned \
                         {mean_pin})"
                    ));
                }
                if max <= MAX && mean <= MEAN {
                    failures.push(format!(
                        "{name}: max {max:.6} mean {mean:.6} now meets 2 / 0.5: unpin it"
                    ));
                }
            }
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

/// The law's `t` at pixel `(x, y)` of the canvas box (module doc).
fn oracle_t(g: &Gradient, x: u32, y: u32) -> f64 {
    let (bw, bh) = (f64::from(g.w), f64::from(g.h));
    let (dx, dy) = (f64::from(x) - bw / 2.0, f64::from(y) - bh / 2.0);
    let a = g.angle.to_radians();
    let u = dx * a.cos() - dy * a.sin();
    let across = [bw / a.cos().abs(), bh / a.sin().abs()];
    let extent = across.into_iter().fold(f64::INFINITY, f64::min) * g.scale;
    let t = match g.style {
        "linear" => 0.5 + u / extent,
        "reflected" => (2.0 * u / extent).abs(),
        "radial" => {
            // The integer centre and the integer sample; the radius the
            // whole-pixel vector of Linear's half extent.
            let (ix, iy) = (
                f64::from(x) - f64::from(g.w / 2),
                f64::from(y) - f64::from(g.h / 2),
            );
            let r = extent / 2.0;
            let (rx, ry) = ((r * a.cos().abs()).floor(), (r * a.sin().abs()).floor());
            (ix * ix + iy * iy).sqrt() / (rx * rx + ry * ry).sqrt()
        }
        _ => (a - (-dy).atan2(dx)).rem_euclid(std::f64::consts::TAU) / std::f64::consts::TAU,
    };
    let t = t.clamp(0.0, 1.0);
    if g.reverse {
        1.0 - t
    } else {
        t
    }
}

#[test]
fn shipped_styles_follow_the_described_law_at_8_and_16_bits() {
    let three = vec![(0.0, 0.0), (0.3, 1.0), (1.0, 64.0 / 255.0)];
    let two = vec![(0.0, 0.0), (1.0, 1.0)];
    // The last field is Smoothness: 1 is the Classic ramp, below it
    // the linear and Classic ramps blended (`common::bracket_ramp`).
    let cases = [
        ("linear", 0.0, 1.0, false, three.clone(), (SIZE, SIZE), 1.0),
        ("linear", 30.0, 0.8, true, three.clone(), (SIZE, SIZE), 1.0),
        ("linear", 135.0, 1.5, false, two.clone(), (SIZE, SIZE), 1.0),
        (
            "reflected",
            30.0,
            0.8,
            false,
            three.clone(),
            (SIZE, SIZE),
            1.0,
        ),
        (
            "reflected",
            135.0,
            1.5,
            true,
            two.clone(),
            (SIZE, SIZE),
            1.0,
        ),
        ("angle", 30.0, 1.0, false, three.clone(), (SIZE, SIZE), 1.0),
        (
            "angle",
            135.0,
            1.0,
            true,
            vec![(0.0, 1.0), (1.0, 0.0)],
            (SIZE, SIZE),
            1.0,
        ),
        // Radial off the axes on a non-square box at scales other than 1, on
        // an odd box (the half pixel) and an even one.
        ("radial", 60.0, 0.9, false, three.clone(), (61, 19), 1.0),
        ("radial", 30.0, 0.8, true, three.clone(), (52, 76), 1.0),
        ("radial", 135.0, 1.5, false, two.clone(), (SIZE, SIZE), 1.0),
        // Smoothness below 100 % on each identified style.
        ("linear", 30.0, 0.8, false, three.clone(), (SIZE, SIZE), 0.5),
        ("linear", 0.0, 1.0, false, two.clone(), (SIZE, SIZE), 0.0),
        (
            "reflected",
            30.0,
            0.8,
            true,
            three.clone(),
            (SIZE, SIZE),
            0.25,
        ),
        ("angle", 30.0, 1.0, false, three.clone(), (SIZE, SIZE), 0.75),
        ("radial", 60.0, 0.9, false, three, (61, 19), 0.4),
    ];
    for (style, angle, scale, reverse, stops, (w, h), smoothness) in cases {
        let midpoints = vec![0.5; stops.len()];
        let g = Gradient {
            w,
            h,
            style,
            angle,
            scale,
            reverse,
            stops,
            smoothness,
            midpoints,
        };
        let (eight, sixteen) = (fill8(&g), fill16(&g));
        let (cx, cy) = (f64::from(w) / 2.0, f64::from(h) / 2.0);
        for y in 0..h {
            for x in 0..w {
                // An Angle pixel whose square straddles the seam reads either
                // end on a sub-pixel offset (module doc).
                let near_seam = style == "angle" && {
                    let (dx, dy) = (f64::from(x) + 0.5 - cx, f64::from(y) + 0.5 - cy);
                    let theta = (-dy).atan2(dx).to_degrees();
                    let gap = ((theta - angle) + 180.0).rem_euclid(360.0) - 180.0;
                    gap.abs() * dx.hypot(dy).to_radians() < 1.5
                };
                if near_seam {
                    continue;
                }
                // A Gradient Fill layer of these four styles holds the
                // Classic term to its bracketing stops (the FILL CLAMP).
                let want = bracket_ramp(&g.stops, oracle_t(&g, x, y), g.smoothness);
                let at = (y * w + x) as usize;
                let what = format!(
                    "{style} {angle}° ×{scale} reverse {reverse} smoothness {smoothness} ({x},{y})"
                );
                assert!(
                    (eight[at] - want * 255.0).abs() <= 1.0,
                    "{what}: 8-bit {} against {}",
                    eight[at],
                    want * 255.0
                );
                assert!(
                    (sixteen[at] * 257.0 - want * 65535.0).abs() <= 2.0,
                    "{what}: 16-bit {} against {}",
                    sixteen[at] * 257.0,
                    want * 65535.0
                );
            }
        }
    }
}
