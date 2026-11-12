//! P7-24 phase 3: a Shadows/Highlights layer that reads a guide, under
//! linear-light evaluation, at both depths, full frame and regional.
//!
//! The guide is an input of the kernel, so it is built from the backdrop
//! RE-ENCODED (the adjustment transcode rule applied to the guide): over an
//! opaque lattice backdrop the guided adjustment at full opacity equals the
//! encoded document's result, and at half opacity each pixel is the LINEAR
//! lerp between the backdrop and that full-opacity result. The oracle is
//! `linear_oracle`'s decode and encode; the full-opacity target `E` is read
//! from the encoded document, whose own Shadows/Highlights tests are its
//! oracle.

use doctored_core::doc::DrDocument;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{
    PrecisionDocument, PrecisionDocumentLayer as PrecisionLayer,
};
use doctored_core::precision_image::PrecisionImage;
use image::{Rgba, RgbaImage};

use crate::linear_oracle::{
    assert_within, encode_opaque, flatten, lerp3, lin, linear, pixel, region, typed_code,
    typed_encode_opaque, typed_lin, typed_linear, typed_region, typed_render, typed_within,
    with_meta,
};

const SIDE: u32 = 64;

/// A dark left half and a bright right half: the guide's blur crosses the
/// edge, so pixels near it read a guide neither half alone would give.
fn tone(x: u32) -> [u8; 3] {
    if x < SIDE / 2 {
        [40, 52, 36]
    } else {
        [210, 196, 180]
    }
}

fn backdrop() -> RgbaImage {
    RgbaImage::from_fn(SIDE, SIDE, |x, _| {
        let [r, g, b] = tone(x);
        Rgba([r, g, b, 255])
    })
}

const BYTE_META: &str =
    r#"{"type":"adjust","op":"shadows_highlights","params":{"shadows":{"amount":0.5},"radius":8}}"#;

const TYPED_META: &str = r#"{"type":"adjust","evaluation":"precision_v1","op":"shadows_highlights","params":{"shadows":{"amount":0.5,"tone":0.5},"radius":8.0}}"#;

fn byte_document(opacity: f32) -> DrDocument {
    let doc = DrDocument::from_pixels(backdrop())
        .adding_image_layer(0, RgbaImage::new(1, 1), "Shadows/Highlights")
        .expect("the adjustment entry");
    let doc = with_meta(&doc, 1, BYTE_META);
    if opacity < 1.0 {
        doc.with_layer_opacity(1, opacity).expect("a new opacity")
    } else {
        doc
    }
}

#[test]
fn a_guided_shadows_highlights_matches_the_transcode_at_byte_depth() {
    let encoded = flatten(&byte_document(1.0)).expect("the encoded projection");
    let full = linear(&byte_document(1.0));
    let switched = flatten(&full).expect("the linear projection");
    let backdrop = backdrop();
    let mut moved = 0;
    for i in 0..(SIDE * SIDE) as usize {
        // The kernel's answer is quantized once by the encoded path and
        // decoded then re-encoded by the linear one: phase 1's 1-code gate.
        assert_within(
            pixel(&switched, i),
            pixel(&encoded, i),
            1,
            &format!("full opacity at {i}"),
        );
        moved += usize::from(pixel(&encoded, i) != backdrop.as_raw()[i * 4..i * 4 + 4]);
    }
    assert!(moved > 0, "the fixture's adjustment moves pixels");
    assert!(
        region(&full).expect("the linear region") == switched,
        "the regional render is the full one"
    );

    let half = linear(&byte_document(0.5));
    let faded = flatten(&half).expect("the half-opacity linear projection");
    for (i, b) in backdrop.pixels().enumerate() {
        let target = pixel(&encoded, i);
        let expected = encode_opaque(lerp3(
            [b[0], b[1], b[2]].map(lin),
            [target[0], target[1], target[2]].map(lin),
            0.5,
        ));
        assert_within(pixel(&faded, i), expected, 1, &format!("half at {i}"));
    }
    assert!(
        region(&half).expect("the half-opacity region") == faded,
        "the regional render is the full one at half opacity"
    );
}

fn typed_document(opacity: f32) -> PrecisionDocument {
    let pixels: Vec<[u16; 4]> = backdrop()
        .pixels()
        .map(|p| [typed_code(p[0]), typed_code(p[1]), typed_code(p[2]), 65535])
        .collect();
    let image = PrecisionImage::from_rgba16(SIDE, SIDE, pixels, IccProfile::srgb(), None)
        .expect("the fixture image");
    let base = PrecisionLayer::raster("Base", image, (0, 0), 1.0, true).expect("a raster");
    let adjustment = PrecisionLayer::adjustment(
        "Shadows/Highlights",
        TYPED_META,
        opacity,
        true,
        &IccProfile::srgb(),
    )
    .expect("the typed adjustment");
    PrecisionDocument::from_parts(
        SIDE,
        SIDE,
        IccProfile::srgb(),
        Metadata::default(),
        Resolution::default(),
        vec![base, adjustment],
    )
    .expect("the fixture document")
}

#[test]
fn a_guided_shadows_highlights_matches_the_transcode_at_typed_depth() {
    let encoded = typed_render(&typed_document(1.0)).expect("the encoded typed projection");
    let full = typed_linear(&typed_document(1.0));
    let switched = typed_render(&full).expect("the linear typed projection");
    for (i, (got, want)) in switched.iter().zip(&encoded).enumerate() {
        typed_within(*got, *want, &format!("typed full opacity at {i}"));
    }
    assert!(
        typed_region(&full).expect("the linear typed region") == switched,
        "the typed regional render is the full one"
    );

    let half = typed_linear(&typed_document(0.5));
    let faded = typed_render(&half).expect("the half-opacity typed projection");
    for (i, b) in backdrop().pixels().enumerate() {
        let target = encoded[i];
        let expected = typed_encode_opaque(lerp3(
            [b[0], b[1], b[2]].map(lin),
            [target[0], target[1], target[2]].map(typed_lin),
            0.5,
        ));
        typed_within(faded[i], expected, &format!("typed half at {i}"));
    }
    assert!(
        typed_region(&half).expect("the half-opacity typed region") == faded,
        "the typed regional render is the full one at half opacity"
    );
}
