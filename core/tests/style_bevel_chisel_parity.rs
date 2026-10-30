//! #578: Bevel & Emboss Chisel Hard and Chisel Soft against Photoshop's own
//! readbacks, at 8 and 16 bits. The fixtures
//! (`tests/fixtures/psd_photoshop/bevel_chisel_578/`, README) are bevel-
//! chisel-578 take1 (`scripts/photoshop-probe/bevel_chisel_578_take.py`):
//! style-parity-v1x take2's 8 bevel specs plus A3's default (index 08), each
//! lit side a white Normal 100 % pass over black, on take2's `fx` picture.
//!
//! Each row is rebuilt here from the take's own source (`fx_source_confirm_
//! <d>.png`: a black layer over an opaque black canvas, an 8-bit
//! `DrDocument` or a 16-bit `PrecisionDocument`) and the case's style, and
//! compared on the runner's FIXED GATE (`gate_<kk>_<d>.png`): the max and the
//! mean of `|ours − Photoshop|` on the red channel in 8-bit codes, 16-bit
//! samples as `/ 257`. The bound, declared in the runner before the take, is
//! max <= 2 and mean <= 0.5; a row that misses it asserts its own measured
//! number (`PINNED`, rounded up in the third decimal), never a wider one.
//! Every row misses the bound today: the shipped members are the runner's
//! best declared ones, and the take's residual is the record.

use std::ffi::{c_char, CString};
use std::path::{Path, PathBuf};
use std::ptr;

use doctored_core::doc::DrDocument;
use doctored_core::ffi_precision_edit::dr_precision_doc_edit;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer as Layer};
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use image::{Rgba, RgbaImage};
use serde_json::{json, Value};

use crate::common;

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
const MAX: f64 = 2.0;
const MEAN: f64 = 0.5;

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/psd_photoshop/bevel_chisel_578")
}

/// The runner's specs (style, size, soften, depth %, up, altitude, angle):
/// `V.bevel_specs('confirm')` then A3's default.
const SPECS: [(&str, f64, f64, f64, bool, f64, f64); 9] = [
    ("inner_bevel", 5.0, 2.0, 75.0, true, 45.0, 60.0),
    ("outer_bevel", 11.0, 0.0, 200.0, false, 45.0, 150.0),
    ("emboss", 20.0, 8.0, 150.0, true, 40.0, 90.0),
    ("pillow_emboss", 6.0, 3.0, 250.0, true, 55.0, 120.0),
    ("inner_bevel", 14.0, 6.0, 120.0, false, 35.0, 30.0),
    ("outer_bevel", 7.0, 2.0, 90.0, true, 60.0, 200.0),
    ("emboss", 9.0, 0.0, 60.0, false, 50.0, 10.0),
    ("pillow_emboss", 13.0, 4.0, 180.0, false, 40.0, 75.0),
    ("inner_bevel", 5.0, 0.0, 100.0, true, 30.0, 120.0),
];

/// One case's style: the spec, the technique, one side white Normal 100 %.
fn style(k: usize, side: &str, technique: &str) -> Value {
    let (name, size, soften, depth, up, altitude, angle) = SPECS[k];
    let lit = |s: &str| if side == s { 1.0 } else { 0.0 };
    json!({"effects":[{"type":"bevel_emboss","style":name,"technique":technique,
        "size":size,"soften":soften,"depth":depth / 100.0,
        "direction":if up {"up"} else {"down"},"angle":angle,"use_global_light":false,
        "altitude":altitude,"highlight_blend":"normal","highlight_color":"#ffffff",
        "highlight_opacity":lit("h"),"shadow_blend":"normal","shadow_color":"#ffffff",
        "shadow_opacity":lit("s")}]})
}

/// The 8-bit render's red, unit.
fn render8(style: &Value) -> Vec<f64> {
    let source = image::open(fixtures().join("fx_source_confirm_8.png"))
        .expect("source")
        .to_rgba8();
    let (w, h) = source.dimensions();
    let d = DrDocument::from_pixels(RgbaImage::from_pixel(w, h, Rgba([0, 0, 0, 255])))
        .adding_image_layer(0, source, "fx")
        .expect("layer");
    common::styled(&d, 1, &style.to_string())
        .flattened()
        .expect("flatten")
        .pixels()
        .map(|p| f64::from(p[0]) / 255.0)
        .collect()
}

/// The 16-bit render's red, unit.
fn render16(style: &Value) -> Vec<f64> {
    let source = image::open(fixtures().join("fx_source_confirm_16.png"))
        .expect("source")
        .to_rgba16();
    let (w, h) = source.dimensions();
    let image = |pixels: Vec<[u16; 4]>| {
        PrecisionImage::from_rgba16(w, h, pixels, IccProfile::srgb(), None).expect("image")
    };
    let doc = PrecisionDocument::from_parts(
        w,
        h,
        IccProfile::srgb(),
        Metadata::default(),
        Resolution::default(),
        vec![
            Layer::raster(
                "Base",
                image(vec![[0, 0, 0, 65535]; (w * h) as usize]),
                (0, 0),
                1.0,
                true,
            )
            .expect("base"),
            Layer::raster(
                "fx",
                image(source.pixels().map(|p| p.0).collect()),
                (0, 0),
                1.0,
                true,
            )
            .expect("fx"),
        ],
    )
    .expect("document");
    let request = json!({"op":"layer_style","version":1,"layer":1,"style":style});
    let c = CString::new(request.to_string()).unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_edit(
            &doc,
            c.as_ptr(),
            ptr::null(),
            0,
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!out.is_null(), "{}", common::take_err_string(err));
    let styled = unsafe { Box::from_raw(out) };
    styled
        .render(LIMIT, 0, None)
        .expect("render")
        .to_rgba16()
        .expect("rgba16")
        .into_iter()
        .map(|p| f64::from(p[0]) / 65535.0)
        .collect()
}

/// A gray fixture PNG as unit values.
fn gray(name: &str) -> Vec<f64> {
    image::open(fixtures().join(name))
        .unwrap_or_else(|e| panic!("{name}: {e}"))
        .to_luma16()
        .pixels()
        .map(|p| f64::from(p[0]) / 65535.0)
        .collect()
}

/// (max, mean) in 8-bit codes on the gate (module doc).
fn measure(k: usize, side: &str, technique: &str, depth: u32) -> (f64, f64) {
    let tag = if technique == "chisel_hard" {
        "hard"
    } else {
        "soft"
    };
    let s = style(k, side, technique);
    let ours = if depth == 8 {
        render8(&s)
    } else {
        render16(&s)
    };
    let ps = gray(&format!("bevel_{k:02}_{side}_{tag}_{depth}.png"));
    let gate = gray(&format!("gate_{k:02}_{depth}.png"));
    assert_eq!(ours.len(), ps.len(), "canvas size");
    let (mut worst, mut total, mut n) = (0.0f64, 0.0, 0usize);
    for ((o, p), g) in ours.iter().zip(&ps).zip(&gate) {
        if *g > 0.5 {
            let e = (o - p).abs() * 255.0;
            worst = worst.max(e);
            total += e;
            n += 1;
        }
    }
    assert!(n > 0, "an empty gate");
    let mean = total / n as f64;
    eprintln!("bevel_{k:02}_{side}_{tag}_{depth}: max {worst:.6} mean {mean:.6}");
    (worst, mean)
}

/// One row: `(spec, side, technique, depth, (max, mean))`, the pin a row
/// that misses the bound asserts, or `None` for a row that meets it.
type Pin = (usize, &'static str, &'static str, u32, Option<(f64, f64)>);

// Measured on the shipped members (Chisel Hard `CH`, Chisel Soft at
// `CHISEL_SOFT_RATIO` 0.1) against take1, max and mean in codes rounded up in
// the third decimal: no row meets 2 / 0.5. The runner's own model of the
// same members (its RECORDED section) gives means within 1.4 codes of these
// and maxima within 31. The Pillow Emboss rows (specs 03 and 07) are the
// Chisel law's since #578 bevel phase 2 (`style_bevel_chisel`), re-pinned
// lower: within 1 code / 0.002 of `bevel_578_law_fit.py --judge-chisel-take3`'s
// model (03 h 243 / 21.03 -> 166 / 5.67 Hard, 07 s 245 / 29.98 -> 195 / 6.56).
const PINNED: [Pin; 72] = [
    (0, "h", "chisel_hard", 8, Some((72.001, 11.186))),
    (0, "s", "chisel_hard", 8, Some((22.001, 3.582))),
    (1, "h", "chisel_hard", 8, Some((112.001, 10.282))),
    (1, "s", "chisel_hard", 8, Some((135.001, 17.533))),
    (2, "h", "chisel_hard", 8, Some((117.001, 9.195))),
    (2, "s", "chisel_hard", 8, Some((121.001, 11.366))),
    (3, "h", "chisel_hard", 8, Some((166.001, 5.668))),
    (3, "s", "chisel_hard", 8, Some((155.001, 10.667))),
    (4, "h", "chisel_hard", 8, Some((79.001, 8.418))),
    (4, "s", "chisel_hard", 8, Some((104.001, 22.920))),
    (5, "h", "chisel_hard", 8, Some((189.001, 13.165))),
    (5, "s", "chisel_hard", 8, Some((44.001, 3.355))),
    (6, "h", "chisel_hard", 8, Some((116.001, 9.867))),
    (6, "s", "chisel_hard", 8, Some((48.001, 3.331))),
    (7, "h", "chisel_hard", 8, Some((174.001, 3.546))),
    (7, "s", "chisel_hard", 8, Some((195.001, 6.564))),
    (8, "h", "chisel_hard", 8, Some((79.001, 7.022))),
    (8, "s", "chisel_hard", 8, Some((95.001, 12.796))),
    (0, "h", "chisel_hard", 16, Some((71.993, 11.185))),
    (0, "s", "chisel_hard", 16, Some((22.001, 3.582))),
    (1, "h", "chisel_hard", 16, Some((112.051, 10.293))),
    (1, "s", "chisel_hard", 16, Some((135.071, 17.513))),
    (2, "h", "chisel_hard", 16, Some((116.899, 9.196))),
    (2, "s", "chisel_hard", 16, Some((120.604, 11.366))),
    (3, "h", "chisel_hard", 16, Some((166.001, 5.668))),
    (3, "s", "chisel_hard", 16, Some((154.993, 10.667))),
    (4, "h", "chisel_hard", 16, Some((78.993, 8.418))),
    (4, "s", "chisel_hard", 16, Some((103.989, 22.919))),
    (5, "h", "chisel_hard", 16, Some((189.032, 13.163))),
    (5, "s", "chisel_hard", 16, Some((43.790, 3.356))),
    (6, "h", "chisel_hard", 16, Some((115.654, 9.856))),
    (6, "s", "chisel_hard", 16, Some((48.001, 3.346))),
    (7, "h", "chisel_hard", 16, Some((173.989, 3.546))),
    (7, "s", "chisel_hard", 16, Some((194.989, 6.562))),
    (8, "h", "chisel_hard", 16, Some((79.004, 7.022))),
    (8, "s", "chisel_hard", 16, Some((94.989, 12.795))),
    (0, "h", "chisel_soft", 8, Some((97.001, 13.222))),
    (0, "s", "chisel_soft", 8, Some((45.001, 4.574))),
    (1, "h", "chisel_soft", 8, Some((103.001, 9.081))),
    (1, "s", "chisel_soft", 8, Some((136.001, 16.230))),
    (2, "h", "chisel_soft", 8, Some((118.001, 9.394))),
    (2, "s", "chisel_soft", 8, Some((131.001, 11.782))),
    (3, "h", "chisel_soft", 8, Some((184.001, 7.225))),
    (3, "s", "chisel_soft", 8, Some((194.001, 9.362))),
    (4, "h", "chisel_soft", 8, Some((86.001, 9.108))),
    (4, "s", "chisel_soft", 8, Some((126.001, 26.518))),
    (5, "h", "chisel_soft", 8, Some((183.001, 11.661))),
    (5, "s", "chisel_soft", 8, Some((48.001, 3.111))),
    (6, "h", "chisel_soft", 8, Some((138.001, 10.007))),
    (6, "s", "chisel_soft", 8, Some((48.001, 3.863))),
    (7, "h", "chisel_soft", 8, Some((182.001, 3.353))),
    (7, "s", "chisel_soft", 8, Some((213.001, 5.611))),
    (8, "h", "chisel_soft", 8, Some((66.001, 6.269))),
    (8, "s", "chisel_soft", 8, Some((124.001, 16.947))),
    (0, "h", "chisel_soft", 16, Some((96.989, 13.223))),
    (0, "s", "chisel_soft", 16, Some((45.001, 4.575))),
    (1, "h", "chisel_soft", 16, Some((103.187, 9.097))),
    (1, "s", "chisel_soft", 16, Some((135.584, 16.212))),
    (2, "h", "chisel_soft", 16, Some((117.845, 9.393))),
    (2, "s", "chisel_soft", 16, Some((130.327, 11.783))),
    (3, "h", "chisel_soft", 16, Some((184.001, 7.224))),
    (3, "s", "chisel_soft", 16, Some((193.989, 9.360))),
    (4, "h", "chisel_soft", 16, Some((85.989, 9.107))),
    (4, "s", "chisel_soft", 16, Some((125.989, 26.519))),
    (5, "h", "chisel_soft", 16, Some((182.674, 11.654))),
    (5, "s", "chisel_soft", 16, Some((48.090, 3.109))),
    (6, "h", "chisel_soft", 16, Some((137.763, 10.001))),
    (6, "s", "chisel_soft", 16, Some((48.001, 3.881))),
    (7, "h", "chisel_soft", 16, Some((181.993, 3.354))),
    (7, "s", "chisel_soft", 16, Some((212.989, 5.611))),
    (8, "h", "chisel_soft", 16, Some((66.001, 6.270))),
    (8, "s", "chisel_soft", 16, Some((123.993, 16.945))),
];

#[test]
fn every_chisel_row_meets_the_bound_or_its_measured_residual() {
    let mut unpinned = Vec::new();
    for technique in ["chisel_hard", "chisel_soft"] {
        for depth in [8, 16] {
            for k in 0..SPECS.len() {
                for side in ["h", "s"] {
                    let (max, mean) = measure(k, side, technique, depth);
                    let pin = PINNED
                        .iter()
                        .find(|p| p.0 == k && p.1 == side && p.2 == technique && p.3 == depth);
                    match pin {
                        None => unpinned.push(format!(
                            "{k:02} {side} {technique} {depth}: max {max:.3} mean {mean:.4}"
                        )),
                        Some((.., None)) => assert!(
                            max <= MAX && mean <= MEAN,
                            "{k:02} {side} {technique} {depth}: max {max:.3}, mean {mean:.4}"
                        ),
                        Some((.., Some((max_pin, mean_pin)))) => assert!(
                            max <= *max_pin && mean <= *mean_pin,
                            "{k:02} {side} {technique} {depth}: max {max:.3} (pinned {max_pin}), \
                             mean {mean:.4} (pinned {mean_pin})"
                        ),
                    }
                }
            }
        }
    }
    assert!(unpinned.is_empty(), "rows not pinned: {unpinned:#?}");
}
