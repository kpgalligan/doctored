//! P7-24 phase 3: the six non-separable modes and Dissolve under linear-light
//! evaluation, at both depths, against `linear_oracle` (the W3C pseudocode in
//! f64 on analytic-decoded components, re-encoded by the counted threshold
//! rule at 8 bits and the analytic inverse at 16).
//!
//! The modes run the `blend.rs` masters on LINEAR components with the W3C
//! weights 0.3 / 0.59 / 0.11 and ClipColor at [0, 1]: only what enters the
//! accumulator is decoded. Dissolve's threshold reads the canvas position and
//! the effective alpha only, and a shown pixel is the source's own colour, so
//! a linear-light Dissolve equals the encoded one.

use doctored_core::doc::{BlendMode, DrDocument};
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{
    PrecisionDocument, PrecisionDocumentLayer as PrecisionLayer,
};
use doctored_core::precision_image::PrecisionImage;
use image::{Rgba, RgbaImage};

use crate::linear_oracle::{
    assert_within, encode, flatten, lin, linear, lum, nonseparable, over_blended, pixel,
    typed_blend, typed_code, typed_encode, typed_linear, typed_render, typed_within, with_meta,
};

const MODES: [BlendMode; 6] = [
    BlendMode::DarkerColor,
    BlendMode::LighterColor,
    BlendMode::Hue,
    BlendMode::Saturation,
    BlendMode::Color,
    BlendMode::Luminosity,
];

/// Opaque backdrops across the toe, the knee and the shoulder.
const BACKDROP: [[u8; 3]; 8] = [
    [16, 239, 128],
    [64, 191, 100],
    [96, 159, 60],
    [128, 127, 200],
    [160, 95, 30],
    [200, 55, 210],
    [230, 25, 90],
    [250, 5, 150],
];

/// Chromatic sources, so SetSat, SetLum and ClipColor all have work to do.
const SOURCE: [[u8; 3]; 8] = [
    [200, 70, 20],
    [150, 120, 240],
    [90, 180, 44],
    [40, 220, 165],
    [220, 35, 75],
    [70, 200, 130],
    [180, 90, 250],
    [20, 235, 10],
];

fn row(colors: &[[u8; 3]; 8], alpha: u8) -> RgbaImage {
    RgbaImage::from_fn(8, 1, |x, _| {
        let c = colors[x as usize];
        Rgba([c[0], c[1], c[2], alpha])
    })
}

fn two_layer(alpha: u8) -> DrDocument {
    DrDocument::from_pixels(row(&BACKDROP, 255))
        .adding_image_layer(0, row(&SOURCE, alpha), "Top")
        .expect("the fixture's second layer")
}

/// The oracle's straight linear result at pixel `x`, source alpha `sa`.
fn expected_linear(mode: BlendMode, x: usize, sa: f64) -> [f64; 4] {
    let cb = BACKDROP[x].map(lin);
    let cs = SOURCE[x].map(lin);
    // The two picks are discontinuous where the luminosities meet; the
    // fixture keeps every pixel well away from that seam.
    assert!(
        (lum(cs) - lum(cb)).abs() > 0.01,
        "fixture pixel {x} sits on the pick seam"
    );
    over_blended(
        [cb[0], cb[1], cb[2], 1.0],
        cs,
        nonseparable(mode, cb, cs),
        sa,
    )
}

#[test]
fn every_non_separable_mode_matches_the_linear_oracle_at_byte_depth() {
    for alpha in [255u8, 128] {
        let base = two_layer(alpha);
        for mode in MODES {
            let doc = base.with_layer_blend_mode(1, mode).expect("a new mode");
            let bytes = flatten(&linear(&doc)).expect("a non-separable mode projects");
            for x in 0..8usize {
                let out = expected_linear(mode, x, f64::from(alpha) / 255.0);
                let expected = [
                    encode(out[0]),
                    encode(out[1]),
                    encode(out[2]),
                    (out[3] * 255.0).round() as u8,
                ];
                assert_within(
                    pixel(&bytes, x),
                    expected,
                    1,
                    &format!("{mode:?} alpha {alpha} at {x}"),
                );
            }
        }
    }
}

fn typed_row(rows: &[[u8; 3]; 8], alpha: u16) -> Vec<[u16; 4]> {
    rows.iter()
        .map(|c| [typed_code(c[0]), typed_code(c[1]), typed_code(c[2]), alpha])
        .collect()
}

fn typed_raster(name: &str, pixels: Vec<[u16; 4]>, w: u32, h: u32) -> PrecisionLayer {
    let image = PrecisionImage::from_rgba16(w, h, pixels, IccProfile::srgb(), None)
        .expect("the fixture image");
    PrecisionLayer::raster(name, image, (0, 0), 1.0, true).expect("a raster")
}

fn typed_document(w: u32, h: u32, layers: Vec<PrecisionLayer>) -> PrecisionDocument {
    PrecisionDocument::from_parts(
        w,
        h,
        IccProfile::srgb(),
        Metadata::default(),
        Resolution::default(),
        layers,
    )
    .expect("the fixture document")
}

#[test]
fn every_non_separable_mode_matches_the_linear_oracle_at_typed_depth() {
    for alpha in [255u8, 128] {
        let base = typed_document(
            8,
            1,
            vec![
                typed_raster("Base", typed_row(&BACKDROP, 65535), 8, 1),
                typed_raster("Top", typed_row(&SOURCE, typed_code(alpha)), 8, 1),
            ],
        );
        for mode in MODES {
            let doc = typed_blend(&base, 1, mode);
            let samples =
                typed_render(&typed_linear(&doc)).expect("a non-separable typed mode renders");
            for (x, sample) in samples.iter().enumerate() {
                let out = expected_linear(mode, x, f64::from(alpha) / 255.0);
                let expected = [
                    typed_encode(out[0]),
                    typed_encode(out[1]),
                    typed_encode(out[2]),
                    (out[3] * 65535.0).round() as u16,
                ];
                typed_within(
                    *sample,
                    expected,
                    &format!("typed {mode:?} alpha {alpha} at {x}"),
                );
            }
        }
    }
}

const SIDE: u32 = 16;

fn dissolve_backdrop() -> RgbaImage {
    RgbaImage::from_fn(SIDE, SIDE, |x, y| {
        Rgba([(x * 15) as u8, (y * 15) as u8, 128, 255])
    })
}

fn dissolve_source(alpha: u8) -> RgbaImage {
    RgbaImage::from_fn(SIDE, SIDE, |x, y| {
        Rgba([200, (x * 13 + y * 3) as u8, 40, alpha])
    })
}

/// How many of the projection's pixels equal the source's own codes: the
/// dither's shown share, so a Dissolve fixture is never vacuous.
fn shown(bytes: &[u8], source: &RgbaImage) -> usize {
    source
        .pixels()
        .enumerate()
        .filter(|(i, p)| pixel(bytes, *i)[..3] == p.0[..3])
        .count()
}

#[test]
fn dissolve_is_domain_independent_at_byte_depth() {
    let total = (SIDE * SIDE) as usize;
    for alpha in [77u8, 179] {
        let source = dissolve_source(alpha);
        let doc = DrDocument::from_pixels(dissolve_backdrop())
            .adding_image_layer(0, source.clone(), "Dissolve")
            .expect("the dissolve layer")
            .with_layer_blend_mode(1, BlendMode::Dissolve)
            .expect("a new mode");
        let encoded = flatten(&doc).expect("the encoded projection");
        let switched = flatten(&linear(&doc)).expect("the linear projection");
        assert_eq!(switched, encoded, "Dissolve at alpha {alpha}");
        let count = shown(&switched, &source);
        assert!(
            count > 0 && count < total,
            "alpha {alpha} shows {count} of {total}"
        );
    }

    // A Dissolve ADJUSTMENT shows its kernel's lattice answer or nothing.
    let doc = DrDocument::from_pixels(dissolve_backdrop())
        .adding_image_layer(0, RgbaImage::new(1, 1), "Invert")
        .expect("the adjustment entry");
    let doc = with_meta(&doc, 1, r#"{"type":"adjust","op":"invert"}"#)
        .with_layer_blend_mode(1, BlendMode::Dissolve)
        .expect("a new mode")
        .with_layer_opacity(1, 0.3)
        .expect("a new opacity");
    let encoded = flatten(&doc).expect("the encoded projection");
    let switched = flatten(&linear(&doc)).expect("the linear projection");
    assert_eq!(switched, encoded, "a Dissolve adjustment");
    let backdrop = flatten(&DrDocument::from_pixels(dissolve_backdrop())).expect("the backdrop");
    assert!(switched != backdrop, "the adjustment shows somewhere");
}

#[test]
fn dissolve_is_domain_independent_at_typed_depth() {
    let backdrop: Vec<[u16; 4]> = dissolve_backdrop()
        .pixels()
        .map(|p| [typed_code(p[0]), typed_code(p[1]), typed_code(p[2]), 65535])
        .collect();
    for alpha in [77u8, 179] {
        let source: Vec<[u16; 4]> = dissolve_source(alpha)
            .pixels()
            .map(|p| {
                [
                    typed_code(p[0]),
                    typed_code(p[1]),
                    typed_code(p[2]),
                    typed_code(alpha),
                ]
            })
            .collect();
        let doc = typed_document(
            SIDE,
            SIDE,
            vec![
                typed_raster("Base", backdrop.clone(), SIDE, SIDE),
                typed_raster("Dissolve", source, SIDE, SIDE),
            ],
        );
        let doc = typed_blend(&doc, 1, BlendMode::Dissolve);
        let encoded = typed_render(&doc).expect("the encoded typed projection");
        let switched = typed_render(&typed_linear(&doc)).expect("the linear typed projection");
        for (i, (got, want)) in switched.iter().zip(&encoded).enumerate() {
            typed_within(*got, *want, &format!("typed Dissolve alpha {alpha} at {i}"));
        }
    }

    let meta = r#"{"type":"adjust","op":"curves","params":{"rgb":[[0,12],[128,158],[255,255]]},"evaluation":"precision_v1"}"#;
    let adjustment = PrecisionLayer::adjustment("Curves", meta, 0.3, true, &IccProfile::srgb())
        .expect("the typed adjustment entry");
    let doc = typed_document(
        SIDE,
        SIDE,
        vec![
            typed_raster("Base", backdrop.clone(), SIDE, SIDE),
            adjustment,
        ],
    );
    let doc = typed_blend(&doc, 1, BlendMode::Dissolve);
    let encoded = typed_render(&doc).expect("the encoded typed projection");
    let switched = typed_render(&typed_linear(&doc)).expect("the linear typed projection");
    for (i, (got, want)) in switched.iter().zip(&encoded).enumerate() {
        typed_within(*got, *want, &format!("typed Dissolve adjustment at {i}"));
    }
    assert!(
        encoded.iter().zip(&backdrop).any(|(a, b)| a != b),
        "the adjustment shows somewhere"
    );
}
