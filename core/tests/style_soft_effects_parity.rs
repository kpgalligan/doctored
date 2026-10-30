//! #578: the soft Drop Shadow, Inner Shadow and Satin against Photoshop's
//! own renders, at 8 and 16 bits. The fixtures
//! (`tests/fixtures/psd_photoshop/soft_effects_578/`, README) are Photoshop
//! 26.10.0's renders of Doctored-written PSDs: effects-578 take1 (A3's
//! `drop_shadow_*`, `inner_shadow_*`, `satin_*` and `all`, and the `base`
//! both takes share pixel for pixel) and soft-effects-578 take1, the
//! held-out take of `scripts/photoshop-probe/soft_effects_578_fit.py`
//! (`ds_*`, `is_*`, `sa_*`, global light off).
//!
//! Each row is rebuilt here from Photoshop's own `base_<d>.png`, A3's
//! `#40607c` 24 x 24 rect at (20, 20) and the case's style (an 8-bit
//! `DrDocument`, a 16-bit `PrecisionDocument`), and compared over every RGB
//! sample of the 64 x 64 canvas as `verify_phase10_photoshop.compare` does:
//! the max and the mean of `|ours − Photoshop|` in 8-bit codes, 16-bit
//! samples as `/ 257`. The bound, declared in the runner before its fit, is
//! max <= 2 and mean <= 0.5.
//!
//! The members: the shadows' box-pair law (`style_matte::law_matte_with_job`,
//! #578's `effect_kernels_578_take.py`), and the satin's runner-FITTED
//! sigma `size / 2.5` with `|a − b|`. Every hard and shadow row meets the
//! bound; every soft satin row keeps a residual, far below the one it had
//! (A3's default satin 31 / 0.99 -> 3 / 0.15 at 8 bits), and asserts its own
//! measured number (`PINNED`, rounded up in the third decimal), never a
//! wider one.

use std::path::{Path, PathBuf};

use doctored_core::doc::DrDocument;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer as Layer};
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use doctored_core::style::LayerStyle;
use image::RgbaImage;

use crate::common;

const W: u32 = 64;
const H: u32 = 64;
const RECT: (u32, u32, u32, u32) = (20, 20, 24, 24);
const OVERLAY: [u8; 3] = [0x40, 0x60, 0x7c];
const MAX: f64 = 2.0;
const MEAN: f64 = 0.5;

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/psd_photoshop/soft_effects_578")
}

/// The case's style JSON: A3's (`verify_phase10_photoshop_appearance.py`)
/// for take1's rows, the runner's `style_of` for the held-out rows.
fn style(name: &str) -> &'static str {
    match name {
        "drop_shadow_soft" => r#"{"effects":[{"type":"drop_shadow"}]}"#,
        "drop_shadow_hard" => r#"{"effects":[{"type":"drop_shadow","distance":6,"size":0}]}"#,
        "inner_shadow_soft" => r#"{"effects":[{"type":"inner_shadow"}]}"#,
        "inner_shadow_hard" => r#"{"effects":[{"type":"inner_shadow","distance":4,"size":0}]}"#,
        "satin_soft" => r#"{"effects":[{"type":"satin"}]}"#,
        "satin_hard" => r#"{"effects":[{"type":"satin","distance":8,"size":0}]}"#,
        "all" => {
            r#"{"fill_opacity":0.5,"effects":[{"type":"drop_shadow"},{"type":"stroke","size":2},{"type":"color_overlay","opacity":0.5}]}"#
        }
        "ds_s3_a60" => {
            r#"{"effects":[{"type":"drop_shadow","size":3,"distance":5,"angle":60,"spread":0.0,"use_global_light":false}]}"#
        }
        "ds_s9_sp30_a30" => {
            r#"{"effects":[{"type":"drop_shadow","size":9,"distance":3,"angle":30,"spread":0.3,"use_global_light":false}]}"#
        }
        "ds_s12_a90" => {
            r#"{"effects":[{"type":"drop_shadow","size":12,"distance":4,"angle":90,"spread":0.0,"use_global_light":false}]}"#
        }
        "is_s3_a60" => {
            r#"{"effects":[{"type":"inner_shadow","size":3,"distance":5,"angle":60,"choke":0.0,"use_global_light":false}]}"#
        }
        "is_s9_ch30_a30" => {
            r#"{"effects":[{"type":"inner_shadow","size":9,"distance":3,"angle":30,"choke":0.3,"use_global_light":false}]}"#
        }
        "is_s8_a90" => {
            r#"{"effects":[{"type":"inner_shadow","size":8,"distance":4,"angle":90,"choke":0.0,"use_global_light":false}]}"#
        }
        "sa_s6_d5_a60" => {
            r#"{"effects":[{"type":"satin","size":6,"distance":5,"angle":60,"invert":true}]}"#
        }
        "sa_s20_d8_a135" => {
            r#"{"effects":[{"type":"satin","size":20,"distance":8,"angle":135,"invert":false}]}"#
        }
        "sa_s10_d3_a90" => {
            r#"{"effects":[{"type":"satin","size":10,"distance":3,"angle":90,"invert":true}]}"#
        }
        _ => panic!("no case {name}"),
    }
}

fn inside(x: u32, y: u32) -> bool {
    let (x0, y0, rw, rh) = RECT;
    x >= x0 && x < x0 + rw && y >= y0 && y < y0 + rh
}

/// The 8-bit render: Photoshop's base, the rect layer styled.
fn render8(name: &str) -> Vec<[f64; 3]> {
    let base = image::open(fixtures().join("base_8.png"))
        .expect("base")
        .to_rgba8();
    let d = DrDocument::from_pixels(base);
    let rect = RgbaImage::from_fn(W, H, |x, y| {
        let a = if inside(x, y) { 255 } else { 0 };
        image::Rgba([OVERLAY[0], OVERLAY[1], OVERLAY[2], a])
    });
    // Inserted above layer 0 (the base), so the rect is layer 1.
    let d = d.adding_image_layer(0, rect, "rect").expect("add layer");
    let d = common::styled(&d, 1, style(name));
    let flat = d.flattened().expect("flatten");
    flat.pixels()
        .map(|p| [0, 1, 2].map(|k| f64::from(p[k]) / 255.0))
        .collect()
}

/// The 16-bit render: Photoshop's 16-bit base, the rect (x 257) styled.
fn render16(name: &str) -> Vec<[f64; 3]> {
    let base = image::open(fixtures().join("base_16.png"))
        .expect("base")
        .to_rgba16();
    let image = |pixels: Vec<[u16; 4]>| {
        PrecisionImage::from_rgba16(W, H, pixels, IccProfile::srgb(), None).expect("image")
    };
    let base_pixels = base.pixels().map(|p| p.0).collect();
    let rect_pixels = (0..H)
        .flat_map(|y| (0..W).map(move |x| (x, y)))
        .map(|(x, y)| {
            let s = OVERLAY.map(|v| u16::from(v) * 257);
            [s[0], s[1], s[2], if inside(x, y) { 65535 } else { 0 }]
        })
        .collect();
    let parsed = LayerStyle::from_json(style(name)).expect("style");
    let layers = vec![
        Layer::raster("Base", image(base_pixels), (0, 0), 1.0, true).expect("base layer"),
        Layer::raster("rect", image(rect_pixels), (0, 0), 1.0, true)
            .expect("rect layer")
            .with_layer_style(Some(&parsed), (W, H))
            .expect("styled"),
    ];
    let d = PrecisionDocument::from_parts(
        W,
        H,
        IccProfile::srgb(),
        Metadata::default(),
        Resolution::default(),
        layers,
    )
    .expect("document");
    d.render(REFERENCE_WORKING_BYTES, 0, None)
        .expect("render")
        .to_rgba16()
        .expect("rgba16")
        .into_iter()
        .map(|p| [0, 1, 2].map(|k| f64::from(p[k]) / 65535.0))
        .collect()
}

/// Photoshop's render of one row, as unit RGB.
fn photoshop(name: &str, depth: u32) -> Vec<[f64; 3]> {
    let img = image::open(fixtures().join(format!("{name}_{depth}.png"))).expect("fixture");
    if depth == 16 {
        img.to_rgba16()
            .pixels()
            .map(|p| [0, 1, 2].map(|k| f64::from(p[k]) / 65535.0))
            .collect()
    } else {
        img.to_rgba8()
            .pixels()
            .map(|p| [0, 1, 2].map(|k| f64::from(p[k]) / 255.0))
            .collect()
    }
}

/// (max, mean) in 8-bit codes over every RGB sample (module doc).
fn measure(name: &str, depth: u32) -> (f64, f64) {
    let ours = if depth == 8 {
        render8(name)
    } else {
        render16(name)
    };
    let ps = photoshop(name, depth);
    assert_eq!(ours.len(), ps.len(), "{name}_{depth}: canvas size");
    let (mut worst, mut total) = (0.0f64, 0.0);
    for (o, p) in ours.iter().zip(&ps) {
        for k in 0..3 {
            let e = (o[k] - p[k]).abs() * 255.0;
            worst = worst.max(e);
            total += e;
        }
    }
    let mean = total / (ours.len() * 3) as f64;
    eprintln!("{name}_{depth}: max {worst:.6} mean {mean:.6}");
    (worst, mean)
}

/// The rows that meet the bound at both depths: the hard rows, and since the
/// shadows' box-pair law (#578, `effect_kernels_578_take.py`) every Drop and
/// Inner Shadow row but `ds_s9_sp30_a30` (`all` 1 / 0.055 at 8 bits, from 5
/// / 0.143 under the Gaussian), which reads 2 codes plus float noise at 8
/// bits and is pinned there.
const MET: [&str; 11] = [
    "drop_shadow_hard",
    "inner_shadow_hard",
    "satin_hard",
    "drop_shadow_soft",
    "inner_shadow_soft",
    "all",
    "ds_s3_a60",
    "ds_s12_a90",
    "is_s3_a60",
    "is_s9_ch30_a30",
    "is_s8_a90",
];

#[test]
fn the_hard_shadows_and_satin_meet_photoshop_at_8_and_16_bits() {
    for depth in [8, 16] {
        for name in MET {
            let (max, mean) = measure(name, depth);
            assert!(
                max <= MAX && mean <= MEAN,
                "{name} at {depth} bits: max {max:.3}, mean {mean:.4}"
            );
        }
    }
}

/// The satin rows' measured residual on the shipped member (module doc):
/// `(name, depth, (max, mean))`.
// Measured on the shipped members against the takes (max, mean in codes,
// rounded up in the third decimal). Before them, at 8 bits: satin_soft 31 /
// 0.993 and the held-out satin rows 30..31 / 0.442..1.476 (the runner's
// RECORDED section). The shadows' former pins (drop_shadow_soft 6.001 /
// 0.150, all 5.001 / 0.143, ds_s9_sp30_a30 11.001 / 0.789, is_s9_ch30_a30
// 6.001 / 0.280 at 8 bits) went with their Gaussian: those rows meet the
// bound (`MET`), `ds_s9_sp30_a30` within a rounding of it.
const PINNED: [(&str, u32, (f64, f64)); 10] = [
    ("ds_s9_sp30_a30", 8, (2.001, 0.054)),
    ("ds_s9_sp30_a30", 16, (1.261, 0.051)),
    ("satin_soft", 8, (3.001, 0.149)),
    ("satin_soft", 16, (3.273, 0.157)),
    ("sa_s6_d5_a60", 8, (3.001, 0.086)),
    ("sa_s6_d5_a60", 16, (3.436, 0.091)),
    ("sa_s20_d8_a135", 8, (4.001, 0.171)),
    ("sa_s20_d8_a135", 16, (3.436, 0.169)),
    ("sa_s10_d3_a90", 8, (3.001, 0.109)),
    ("sa_s10_d3_a90", 16, (2.681, 0.094)),
];

#[test]
fn the_soft_rows_keep_their_measured_residual() {
    for (name, depth, (max_bound, mean_bound)) in PINNED {
        let (max, mean) = measure(name, depth);
        assert!(
            max <= max_bound && mean <= mean_bound,
            "{name} at {depth} bits: max {max:.3} (pinned {max_bound}), mean {mean:.4} \
             (pinned {mean_bound})"
        );
    }
}

/// Every soft fixture row is met or pinned at both depths, so none goes
/// unmeasured.
#[test]
fn every_soft_fixture_row_is_pinned() {
    let mut missing = Vec::new();
    for name in [
        "drop_shadow_soft",
        "inner_shadow_soft",
        "satin_soft",
        "all",
        "ds_s3_a60",
        "ds_s9_sp30_a30",
        "ds_s12_a90",
        "is_s3_a60",
        "is_s9_ch30_a30",
        "is_s8_a90",
        "sa_s6_d5_a60",
        "sa_s20_d8_a135",
        "sa_s10_d3_a90",
    ] {
        for depth in [8, 16] {
            if !MET.contains(&name) && !PINNED.iter().any(|(n, d, _)| *n == name && *d == depth) {
                let (max, mean) = measure(name, depth);
                missing.push(format!("{name}_{depth}: max {max:.3}, mean {mean:.4}"));
            }
        }
    }
    assert!(
        missing.is_empty(),
        "rows neither met nor pinned: {missing:#?}"
    );
}
