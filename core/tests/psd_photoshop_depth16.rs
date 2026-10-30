//! The committed Adobe Photoshop 2025 (26.10.0) 16-bit fixture, through the C API.
//!
//! `core/tests/fixtures/psd_photoshop/depth16/` is a document Photoshop itself
//! authored (`scripts/verify_phase10_photoshop_depth16.py --author`, whose
//! receipt and gates are quoted in `archive/phase-10c-depth-status.md`): a
//! 64 x 64 base raster, a styled raster (drop shadow), a masked raster, a
//! layer set holding one Multiply member, a `curv` adjustment, a `brit`
//! adjustment, a `SoCo` fill and an empty layer — saved at 16 bit
//! (`source16.psd`) and at 8 (`source.psd`, kept for the divergences the two
//! depths actually have).
//!
//! `source16.psd` is the one file in this crate that exercises the form
//! Photoshop really writes at 16 bits: an EMPTY layer-information section,
//! the records in an `Lr16` tail block, and ZIP-with-prediction (compression
//! 3) channels — read by `psd_container::layer_info` and `psd_zip`. The
//! synthetic half of that evidence is `core/tests/psd_zip16.rs`, whose ZIP
//! streams are stored blocks written by an independent encoder; these are
//! Photoshop's own compressed ones.
//!
//! Everything below is a MEASUREMENT of what this file yields, taken
//! 2026-09-16 with the runner's own gates D1-D3 passing. The
//! `SoCo` record is carried as a Doctored solid color fill layer at both
//! depths, its colour asserted against Photoshop's own `RGBC` numbers.

use std::path::{Path, PathBuf};
use std::ptr;

use doctored_core::precision_document::PrecisionDocument;
use serde_json::{json, Value};

use crate::common;
use common::psd_photoshop::{
    assert_solid_description, blocks_in, import_report, open_psd, rgbc_numbers, strings, LAYERS,
};
use common::{cpath, take_err_string};

const CANVAS: (u32, u32) = (64, 64);

fn fixture(name: &str) -> PathBuf {
    common::psd_photoshop::fixture("depth16", name)
}

/// A typed document handle freed through the C API on drop.
struct Owned(*mut PrecisionDocument);
impl Drop for Owned {
    fn drop(&mut self) {
        unsafe { doctored_core::ffi_precision::dr_precision_doc_free(self.0) };
    }
}
impl std::ops::Deref for Owned {
    type Target = PrecisionDocument;
    fn deref(&self) -> &PrecisionDocument {
        unsafe { &*self.0 }
    }
}

fn open_typed(path: &Path, report: &Value) -> Owned {
    let token = std::ffi::CString::new(report["source_fingerprint"].as_str().unwrap()).unwrap();
    let mut error = ptr::null_mut();
    let raw = unsafe {
        doctored_core::ffi_psd::dr_precision_doc_open_psd(
            cpath(path).as_ptr(),
            LAYERS,
            token.as_ptr(),
            doctored_core::precision_image::REFERENCE_WORKING_BYTES,
            0,
            ptr::null::<doctored_core::job_control::DrJobControl>(),
            &mut error,
        )
    };
    assert!(!raw.is_null(), "typed open: {}", take_err_string(error));
    Owned(raw)
}

fn info(document: &PrecisionDocument) -> Value {
    let mut error = ptr::null_mut();
    let json = unsafe { doctored_core::ffi_precision::dr_precision_doc_info(document, &mut error) };
    assert!(!json.is_null(), "{}", take_err_string(error));
    serde_json::from_str(&take_err_string(json)).expect("info JSON")
}

fn layer_pixels(document: &PrecisionDocument, index: usize) -> Vec<[u16; 4]> {
    let image =
        unsafe { doctored_core::ffi_precision::dr_precision_doc_layer_source(document, index) };
    assert!(!image.is_null(), "layer {index} has no raster source");
    let (w, h) = unsafe { (*image).dimensions() };
    let base = unsafe { doctored_core::ffi_precision::dr_precision_image_pixels_rgba16(image) };
    let samples = unsafe { std::slice::from_raw_parts(base, w as usize * h as usize * 4) }.to_vec();
    unsafe { doctored_core::ffi_precision::dr_precision_image_free(image) };
    samples
        .chunks_exact(4)
        .map(|p| [p[0], p[1], p[2], p[3]])
        .collect()
}

/// One PNG beside the fixture, as straight RGBA8. The decoder is the crate's
/// own image loader, which shares nothing with the PSD channel paths under
/// test here.
fn png_rgba(path: &Path) -> (u32, u32, Vec<u8>) {
    let mut error = ptr::null_mut();
    let image = unsafe { doctored_core::ffi::dr_image_open(cpath(path).as_ptr(), &mut error) };
    assert!(!image.is_null(), "{}", take_err_string(error));
    let (w, h) = unsafe {
        (
            doctored_core::ffi::dr_image_width(image),
            doctored_core::ffi::dr_image_height(image),
        )
    };
    let base = unsafe { doctored_core::ffi::dr_image_pixels_rgba(image) };
    let pixels = unsafe { std::slice::from_raw_parts(base, w as usize * h as usize * 4) }.to_vec();
    unsafe { doctored_core::ffi::dr_image_free(image) };
    (w, h, pixels)
}

/// Photoshop's own 8 -> 16 promotion, measured over the whole base picture in
/// Run 3 and reproduced here as the INDEPENDENT oracle for the
/// ZIP-with-prediction decode: its 15+1-bit internals hold `round(k * 65535 /
/// 32768)`, so a byte `v` becomes `round(round(v * 32768 / 255) * 65535 /
/// 32768)`.
fn ps16(value: u8) -> u16 {
    let internal = (f64::from(value) * 32768.0 / 255.0).round();
    (internal * 65535.0 / 32768.0).round() as u16
}

/// The `RGBC` numbers (0…255) of `name`'s own `SoCo` block, as Photoshop
/// wrote them.
fn soco_numbers(name: &str) -> [f64; 3] {
    let bytes = std::fs::read(fixture(name)).unwrap();
    let blocks = blocks_in(&bytes, b"SoCo");
    assert_eq!(blocks.len(), 1, "{name}: one SoCo");
    rgbc_numbers(&blocks[0])
}

// ----------------------------------------------------------------- 16 bit --

#[test]
fn the_sixteen_bit_file_reports_the_layered_facts_the_runner_gated() {
    let report = import_report(&fixture("source16.psd"));
    assert_eq!(report["layered_available"], Value::Bool(true), "{report}");
    assert_eq!(report["layered_refusal"], Value::Null);
    assert_eq!(report["import_precision"], Value::from("rgba16"));
    assert_eq!(report["depth"], Value::from(16));
    // Eight records that are not section dividers, in one folder.
    assert_eq!(report["layer_count"], Value::from(8), "{report}");
    assert_eq!(report["group_count"], Value::from(1));

    // The WHOLE loss list is empty: the Brightness/Contrast record is
    // carried, and so is the Solid Color Fill (below).
    let losses = strings(&report, "verified_losses");
    assert!(losses.is_empty(), "{losses:?}");

    let observations = strings(&report, "observations");
    for wanted in [
        "(\u{201c}Curves\u{201d}) is a Photoshop Curves adjustment layer (curv); layered import carries it",
        "(\u{201c}Brightness\u{201d}) is a Photoshop Brightness/Contrast adjustment layer (brit); layered import carries it",
        "(\u{201c}Masked\u{201d}): ordinary raster mask samples, absolute placement",
        "(\u{201c}Shadowed\u{201d}) has Photoshop layer effects (drop_shadow",
        "(\u{201c}Empty\u{201d}) has no pixels; layered import creates it as an empty canvas-sized layer.",
        "(\u{201c}Fill\u{201d}) is a Photoshop Solid Color Fill layer (SoCo) with no pixels; layered import carries it as a Doctored solid color fill layer",
    ] {
        assert!(
            observations.iter().any(|o| o.contains(wanted)),
            "missing observation {wanted}: {observations:?}"
        );
    }
}

#[test]
fn the_typed_open_keeps_every_construct_photoshop_wrote_at_sixteen_bits() {
    let report = import_report(&fixture("source16.psd"));
    let document = open_typed(&fixture("source16.psd"), &report);
    let info = info(&document);
    let layers = info["layers"].as_array().expect("layers");
    let names: Vec<&str> = layers.iter().map(|l| l["name"].as_str().unwrap()).collect();
    // Eight roots, with the set's one member listed before it (the info view
    // is postorder). The fill is carried.
    assert_eq!(
        names,
        [
            "Base",
            "Shadowed",
            "Masked",
            "Inner",
            "Group",
            "Curves",
            "Brightness",
            "Fill",
            "Empty"
        ]
    );
    let depths: Vec<u64> = layers
        .iter()
        .map(|l| l["depth"].as_u64().unwrap())
        .collect();
    assert_eq!(depths, [0, 0, 0, 1, 0, 0, 0, 0, 0]);
    assert_eq!(layers[4]["kind"], Value::from("pass_through_group"));

    // The set's member: Multiply at the stored opacity byte 153.
    assert_eq!(
        layers[3]["blend_mode"],
        Value::from(doctored_core::doc::BlendMode::Multiply.to_c())
    );
    assert_eq!(layers[3]["opacity"], Value::from(153.0f32 / 255.0));
    assert_eq!(layers[3]["offset"], json!([30, 30]));
    assert_eq!(layers[3]["dimensions"], json!([20, 20]));

    // Photoshop stores a 16-bit mask UNTRIMMED over the whole canvas, where
    // the 8-bit twin trims it to the mask's own extent.
    let mask = document
        .layer(2)
        .and_then(|layer| layer.mask())
        .expect("the Masked mask");
    assert_eq!(mask.plane().dimensions(), CANVAS);
    assert_eq!(mask.properties().offset, (0, 0));
    assert_eq!(mask.properties().outside_coverage, 0);
    assert!(mask.enabled());
    assert!(!mask.properties().linked, "imported masks are independent");
    // The reveal-selection rectangle is white inside and black outside.
    let inside = (24 * 64 + 28) as usize;
    assert_eq!(mask.plane().samples()[inside], 65535);
    assert_eq!(mask.plane().samples()[0], 0);

    // The styled record's `lfx2` carries one ENABLED drop shadow with the
    // authored parameters; Photoshop's own four disabled slots come with it.
    let style: Value = serde_json::from_str(
        &document
            .layer(1)
            .and_then(|layer| layer.reported_style())
            .expect("the Shadowed style"),
    )
    .expect("style JSON");
    let effects = style["effects"].as_array().expect("effects");
    let shadow = effects
        .iter()
        .find(|effect| effect["type"] == "drop_shadow")
        .expect("a drop shadow");
    assert_eq!(shadow["enabled"], Value::Bool(true));
    assert_eq!(shadow["opacity"], Value::from(0.75));
    assert_eq!(shadow["angle"], Value::from(120.0));
    assert_eq!(shadow["distance"], Value::from(6.0));
    assert_eq!(shadow["size"], Value::from(0.0));
    assert_eq!(shadow["blend"], Value::from("multiply"));
    assert_eq!(shadow["color"], Value::from("#000000"));
    assert!(
        effects
            .iter()
            .filter(|effect| effect["enabled"] == Value::Bool(true))
            .count()
            == 1,
        "only the authored effect is enabled: {effects:?}"
    );

    // The carried adjustment is Run 2's own `curv` meta plus the typed
    // `evaluation` key, and it keeps Photoshop's canvas-sized white mask.
    assert_eq!(
        document.adjustment_description(5),
        Some(
            json!({"type": "adjust", "evaluation": "precision_v1", "op": "curves",
                   "params": {"rgb": [[0, 0], [64, 80], [255, 255]]}})
            .to_string()
            .as_str()
        )
    );
    let curves_mask = document
        .layer(5)
        .and_then(|layer| layer.mask())
        .expect("the Curves mask");
    assert_eq!(curves_mask.plane().dimensions(), CANVAS);
    assert!(curves_mask.plane().samples().iter().all(|s| *s == 65535));

    // Photoshop's own Brightness/Contrast record: `brit` is eight zero
    // bytes and the values are in `CgEd` — Brgh 30, Cntr 20, useLegacy false —
    // which is brightness 30/150 and contrast 20/100 on Doctored's modern form.
    assert_eq!(
        document.adjustment_description(6),
        Some(
            json!({"type": "adjust", "evaluation": "precision_v1", "op": "bcs",
                   "params": {"brightness": 0.2, "contrast": 0.2, "legacy": false}})
            .to_string()
            .as_str()
        )
    );

    // The pixel-less `SoCo` is a solid color fill layer: Photoshop's colour,
    // rendered over the canvas at the origin. The file's profile is
    // Photoshop's sRGB, within `MatrixTrc::approx_eq` of Doctored's but not
    // exactly it, so the colour reads in as sRGB unchanged (the authored-
    // colour rule) and renders through the file's own exact curves: its
    // 16-bit samples land a few codes off n · 257 (6 at most, measured) and
    // each is exactly Photoshop's 8-bit code n — the bound a round trip is
    // held to.
    let numbers = soco_numbers("source16.psd");
    assert_solid_description(
        document
            .artwork_description(7)
            .expect("the fill's description"),
        numbers,
        CANVAS,
        "source16",
    );
    assert_eq!(document.layer(7).unwrap().offset(), (0, 0));
    let fill = layer_pixels(&document, 7);
    assert_eq!(fill.len(), 64 * 64);
    for pixel in &fill {
        for c in 0..3 {
            let code = (u32::from(pixel[c]) + 128) / 257;
            assert_eq!(
                f64::from(code),
                numbers[c].round(),
                "{pixel:?} vs {numbers:?}"
            );
        }
        assert_eq!(pixel[3], 65535);
    }

    // The empty record becomes one canvas-sized transparent raster.
    let empty = layer_pixels(&document, 8);
    assert_eq!(empty.len(), 64 * 64);
    assert!(empty.iter().all(|pixel| *pixel == [0, 0, 0, 0]));

    // And the ZIP-with-prediction decode itself: Base's plane is exactly
    // Photoshop's promotion of the base picture beside the fixture.
    let (width, height, base) = png_rgba(&fixture("base.png"));
    assert_eq!((width, height), CANVAS);
    let expected: Vec<[u16; 4]> = base
        .chunks_exact(4)
        .map(|p| [ps16(p[0]), ps16(p[1]), ps16(p[2]), ps16(p[3])])
        .collect();
    assert_eq!(layer_pixels(&document, 0), expected);
}

// ------------------------------------------------------------------ 8 bit --

#[test]
fn the_eight_bit_twin_holds_the_same_stack_in_photoshops_other_forms() {
    let report = import_report(&fixture("source.psd"));
    assert_eq!(report["layered_available"], Value::Bool(true), "{report}");
    assert_eq!(report["import_precision"], Value::from("rgba8"));
    assert_eq!(report["layer_count"], Value::from(8), "{report}");
    assert_eq!(report["group_count"], Value::from(1));

    // The divergence that matters: at 8 bits `SoCo` is a canvas-sized RLE
    // RASTER with flags bit 4 — Photoshop's form — and is carried as the
    // same solid color fill layer, costing no loss.
    let losses = strings(&report, "verified_losses");
    assert!(losses.is_empty(), "{losses:?}");
    let observations = strings(&report, "observations");
    assert!(
        observations.iter().any(|o| o.contains(
            "(\u{201c}Fill\u{201d}) is a Photoshop Solid Color Fill layer (SoCo) in Photoshop's form"
        )),
        "{observations:?}"
    );

    let doc = open_psd(&fixture("source.psd"), &report, LAYERS);
    let names: Vec<&str> = doc.layers.iter().map(|l| l.name.as_str()).collect();
    assert_eq!(
        names,
        [
            "Base",
            "Shadowed",
            "Masked",
            "Inner",
            "Group",
            "Curves",
            "Brightness",
            "Fill",
            "Empty"
        ],
        "the 8-bit stack keeps the fill record"
    );
    let fill = doc.layers.iter().find(|l| l.name == "Fill").unwrap();
    assert_eq!(fill.pixels.dimensions(), CANVAS);
    assert_eq!(fill.offset, (0, 0));
    let numbers = soco_numbers("source.psd");
    assert_solid_description(
        fill.meta.as_deref().expect("the fill's description"),
        numbers,
        CANVAS,
        "source",
    );
    let want = numbers.map(|n| n.round() as u8);
    assert!(
        fill.pixels
            .to_contiguous()
            .unwrap()
            .pixels()
            .all(|p| p.0 == [want[0], want[1], want[2], 255]),
        "the fill is Photoshop's colour {want:?}"
    );

    // Photoshop TRIMS an 8-bit mask to its nonzero extent (Run 3's finding),
    // where the 16-bit twin above stores it over the whole canvas.
    let masked = doc.layers.iter().find(|l| l.name == "Masked").unwrap();
    let mask = masked.mask.as_ref().expect("the Masked mask");
    assert_eq!(mask.dimensions(), (32, 36));
    assert_eq!(masked.mask_properties.offset, (28, 24));
    assert_eq!(masked.mask_properties.outside_coverage, 0);
    assert!(masked.mask_enabled);
}
