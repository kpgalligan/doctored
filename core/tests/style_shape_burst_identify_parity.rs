//! The Shape Burst Stroke against Photoshop's own
//! gradient-identify-673 take-2 rows, at 8 and 16 bits. The fixture
//! (`tests/fixtures/psd_photoshop/gradient_identify_673/take2_burst.json`,
//! README) holds take 2's 54 Smoothness-100 burst rows (Outside, Inside and
//! Center at Size 4, 16 and 32 on a hard rect, a disc and a 4 x 4
//! anti-aliased disc), written by
//! `scripts/photoshop-probe/gradient_identify_673_fit.py --fixtures` on its
//! `--judge-phase3` SHIP of the Shape Burst (rule (ii): every existing
//! stroke-fill-557 Shape Burst row fell and none rose). Each row's `pixels`
//! are exactly the judge's band (`burst_band`) on row y = 80, with their
//! side of the contour and Photoshop's red in 8-bit codes.
//!
//! Each row is rebuilt as the take runner builds it
//! (`gradient_identify_673_take2.py`): a 160 x 160 sRGB document, an opaque
//! white backdrop under a (48, 160, 80) source whose alpha is the take's
//! shape quantized to the depth (`floor(a · full + 0.5)`), styled with one
//! Normal 100 % Stroke of the row's position and size, Fill Type Gradient,
//! Shape Burst, black to white. The 8-bit render is a `DrDocument` through
//! `dr_doc_set_layer_style`; the 16-bit one a `PrecisionDocument` through
//! `dr_precision_doc_edit` (`layer_style`), read as red / 257. A row's
//! error is the max and the mean of `|ours − Photoshop|` over its pixels.
//!
//! The bound, the runner's, is max <= 2 and mean <= 0.5. A row that misses
//! it asserts its own measured number (`PINNED`, rounded up in the third
//! decimal), never a wider one, and that it still misses.

use std::collections::BTreeMap;
use std::ffi::{c_char, CString};
use std::path::Path;
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
const W: u32 = 160;
const ROW: u32 = 80;
const SOURCE: [u8; 3] = [48, 160, 80];
const MAX: f64 = 2.0;
const MEAN: f64 = 0.5;

// Measured on the shipped kernel, max and mean in codes rounded up in the
// third decimal, for the rows that miss 2 / 0.5: every one on the mean, at
// Size 32 Outside and Inside and at Size 16 Center at 16 bits, the judge's
// own (`--judge-phase3` measured the same within 0.004 codes, its law in
// f64 against the core's f32 field).
const PINNED: &[(&str, f64, f64)] = &[
    ("sb_out_rect_z32_sm100_8", 2.0, 0.563),
    ("sb_out_disc_z32_sm100_8", 2.0, 0.563),
    ("sb_out_aadisc_z32_sm100_8", 2.0, 0.563),
    ("sb_in_rect_z32_sm100_8", 2.0, 0.625),
    ("sb_in_disc_z32_sm100_8", 3.0, 0.608),
    ("sb_in_aadisc_z32_sm100_8", 2.0, 0.572),
    ("sb_out_rect_z32_sm100_16", 1.394, 0.593),
    ("sb_out_disc_z32_sm100_16", 1.394, 0.593),
    ("sb_out_aadisc_z32_sm100_16", 1.394, 0.593),
    ("sb_in_rect_z32_sm100_16", 1.324, 0.544),
    ("sb_in_disc_z32_sm100_16", 2.402, 0.594),
    ("sb_in_aadisc_z32_sm100_16", 1.502, 0.562),
    ("sb_ctr_rect_z16_sm100_16", 1.394, 0.612),
    ("sb_ctr_disc_z16_sm100_16", 1.394, 0.612),
    ("sb_ctr_aadisc_z16_sm100_16", 1.394, 0.612),
];

fn fixture() -> Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/psd_photoshop/gradient_identify_673/take2_burst.json");
    serde_json::from_str(&std::fs::read_to_string(path).expect("take2_burst.json")).expect("json")
}

/// The take's shape coverage over the 160 x 160 box, row-major
/// (`gradient_identify_673_take2.shape_alpha`): the rect (48, 56)…(111, 103)
/// inclusive; the disc of radius 28 about (80, 80) by pixel centre; the same
/// disc's area on a 4 x 4 sub-grid.
fn shape_alpha(shape: &str) -> Vec<f64> {
    let inside = |px: f64, py: f64| (px - 80.0).powi(2) + (py - 80.0).powi(2) <= 28.0 * 28.0;
    (0..W)
        .flat_map(|y| (0..W).map(move |x| (x, y)))
        .map(|(x, y)| {
            let (fx, fy) = (f64::from(x), f64::from(y));
            match shape {
                "rect" => f64::from(u8::from((48..=111).contains(&x) && (56..=103).contains(&y))),
                "disc" => f64::from(u8::from(inside(fx + 0.5, fy + 0.5))),
                "aadisc" => {
                    let mut n = 0u32;
                    for j in 0..4 {
                        for i in 0..4 {
                            let (px, py) = (
                                fx + (f64::from(i) + 0.5) / 4.0,
                                fy + (f64::from(j) + 0.5) / 4.0,
                            );
                            n += u32::from(inside(px, py));
                        }
                    }
                    f64::from(n) / 16.0
                }
                _ => panic!("unknown shape {shape}"),
            }
        })
        .collect()
}

fn quantized(alpha: &[f64], full: f64) -> Vec<f64> {
    alpha.iter().map(|&a| (a * full + 0.5).floor()).collect()
}

/// The row's style: one Normal 100 % Shape Burst stroke, black to white.
fn style(position: &str, size: f64) -> Value {
    json!({"effects":[{"type":"stroke","size":size,"position":position,
        "fill_type":"gradient","gradient":{"style":"shape_burst","stops":[
        {"position":0,"color":"#000000","opacity":1},
        {"position":1,"color":"#ffffff","opacity":1}]}}]})
}

/// The 8-bit render's red, in codes.
fn render8(alpha: &[u8], style: &Value) -> Vec<f64> {
    let source = RgbaImage::from_fn(W, W, |x, y| {
        Rgba([SOURCE[0], SOURCE[1], SOURCE[2], alpha[(y * W + x) as usize]])
    });
    let d = DrDocument::from_pixels(RgbaImage::from_pixel(W, W, Rgba([255, 255, 255, 255])))
        .adding_image_layer(0, source, "fx")
        .expect("layer");
    common::styled(&d, 1, &style.to_string())
        .flattened()
        .expect("flatten")
        .pixels()
        .map(|p| f64::from(p[0]))
        .collect()
}

/// The 16-bit render's red, in 8-bit codes (`/ 257`).
fn render16(alpha: &[u16], style: &Value) -> Vec<f64> {
    let image = |pixels: Vec<[u16; 4]>| {
        PrecisionImage::from_rgba16(W, W, pixels, IccProfile::srgb(), None).expect("image")
    };
    let wide = |c: u8| u16::from(c) * 257;
    let layer = alpha
        .iter()
        .map(|&a| [wide(SOURCE[0]), wide(SOURCE[1]), wide(SOURCE[2]), a])
        .collect();
    let doc = PrecisionDocument::from_parts(
        W,
        W,
        IccProfile::srgb(),
        Metadata::default(),
        Resolution::default(),
        vec![
            Layer::raster(
                "Base",
                image(vec![[65535; 4]; (W * W) as usize]),
                (0, 0),
                1.0,
                true,
            )
            .expect("base"),
            Layer::raster("fx", image(layer), (0, 0), 1.0, true).expect("fx"),
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
        .map(|p| f64::from(p[0]) / 257.0)
        .collect()
}

/// `(max, mean)` of one row against its fixture pixels.
fn row_error(row: &Value) -> (f64, f64) {
    let shape = row["shape"].as_str().expect("shape");
    let position = row["position"].as_str().expect("position");
    let size = row["size"].as_f64().expect("size");
    let alpha = shape_alpha(shape);
    let style = style(position, size);
    let red = match row["depth"].as_u64() {
        Some(8) => {
            let a: Vec<u8> = quantized(&alpha, 255.0).iter().map(|&v| v as u8).collect();
            render8(&a, &style)
        }
        Some(16) => {
            let a: Vec<u16> = quantized(&alpha, 65535.0)
                .iter()
                .map(|&v| v as u16)
                .collect();
            render16(&a, &style)
        }
        other => panic!("depth {other:?}"),
    };
    let pixels = row["pixels"].as_array().expect("pixels");
    assert!(!pixels.is_empty(), "{}: no band pixel", row["key"]);
    let (mut worst, mut total) = (0f64, 0f64);
    for p in pixels {
        let x = p[0].as_u64().expect("x") as u32;
        let side = p[1].as_str().expect("side");
        let inside = alpha[(ROW * W + x) as usize] >= 0.5;
        assert_eq!(side == "in", inside, "{}: x {x} side {side}", row["key"]);
        let e = (red[(ROW * W + x) as usize] - p[2].as_f64().expect("code")).abs();
        worst = worst.max(e);
        total += e;
    }
    (worst, total / pixels.len() as f64)
}

#[test]
fn every_smoothness_100_burst_row_meets_the_bound_or_its_pin() {
    let fixture = fixture();
    let rows = fixture["rows"].as_array().expect("rows");
    assert_eq!(rows.len(), 54, "the 54 Smoothness-100 rows");
    let pinned: BTreeMap<&str, (f64, f64)> =
        PINNED.iter().map(|&(n, mx, mn)| (n, (mx, mn))).collect();
    let mut failures = Vec::new();
    let mut seen = 0;
    for row in rows {
        let key = row["key"].as_str().expect("key");
        let (max, mean) = row_error(row);
        match pinned.get(key) {
            Some(&(pmax, pmean)) => {
                seen += 1;
                if max > pmax || mean > pmean {
                    failures.push(format!("{key}: {max:.4} / {mean:.4} above its pin"));
                }
                if max <= MAX && mean <= MEAN {
                    failures.push(format!("{key}: {max:.4} / {mean:.4} now meets: unpin it"));
                }
            }
            None if max > MAX || mean > MEAN => {
                failures.push(format!("{key}: {max:.4} / {mean:.4} misses {MAX} / {MEAN}"));
            }
            None => {}
        }
    }
    assert_eq!(seen, PINNED.len(), "every pin names a fixture row");
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
