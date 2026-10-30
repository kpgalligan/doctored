//! Gradient Smoothness (`Intr`) and a Gradient Fill layer's Align with
//! Layer (`Algn`) in PSD.
//!
//! A gradient fill layer at Smoothness 50 % writes `Intr` 2048 in its `GdFl`
//! and reads back as the same description and pixels; a Gradient Overlay and
//! a Stroke Gradient at 25 % write `Intr` 1024 in their effect and read back
//! their `smoothness`. An in-app ALIGNED gradient fill over a canvas-sized
//! mask made from a selection — the host's own render over the box
//! `dr_doc_fill_align_box` answers — is exported (its record rectangle the
//! stored mask rectangle, the canvas) and reopened: the importer finds the
//! same box from the mask's coverage, so the layer keeps its box and its
//! pixels within a code, at 8 and at 16 bits, and `Algn` is written true.
//!
//! The `Intr` and `Algn` numbers are read from the written file by
//! `psd_gradient_fill_layer`'s own descriptor walker (never the core's
//! reader) or found as raw `Intrdoub` bytes in an effect block.

use std::ffi::CString;
use std::ptr;

use doctored_core::doc::{DrDocument, MaskKind};
use doctored_core::doc_mask::MaskProperties;
use doctored_core::ffi_artwork::{dr_doc_artwork, dr_precision_doc_artwork};
use doctored_core::ffi_fill_align::{dr_doc_fill_align_box, dr_precision_doc_fill_align_box};
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer};
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use doctored_core::precision_mask::{PrecisionMask, PrecisionPlane};
use serde_json::{json, Value};
use tempfile::TempDir;

use crate::common::{rect_layer_doc, solid, styled};
use crate::psd_gradient_fill_layer::{
    gdfl_of, gradient, n, near_samples, parse, render, stop_hex, unit_colors, H, W,
};
use crate::psd_pattern_fill_layer::{import_report, open16, open8, owned, save16, save8};

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
/// The selection the mask is made from, `[x, y, w, h]`.
const RECT: [i32; 4] = [14, 9, 50, 30];

/// The gradient object with `smoothness` and `align_with_layer`.
fn fill_gradient(smoothness: f64, align: bool) -> Value {
    let mut g = gradient("linear", 30.0, 0.8, &stop_hex());
    if smoothness != 1.0 {
        g["smoothness"] = json!(smoothness);
    }
    g["align_with_layer"] = json!(align);
    g
}

/// The render components (with `frame` as the gradient's `box`) and the
/// description of a gradient fill of `g`.
fn fill(g: &Value, frame: Option<[i32; 4]>) -> (String, String) {
    let mut c = json!({"kind": "gradient", "gradient": g, "colors": unit_colors()});
    if let Some(frame) = frame {
        c["box"] = json!(frame);
    }
    let d = json!({"type": "fill", "version": 1, "fill": "gradient", "gradient": g,
        "canvas": [W, H]});
    (json!([c]).to_string(), d.to_string())
}

/// `dr_doc_artwork` of `fill` at layer `index`, `replacing` it or above it.
fn publish8(
    doc: &DrDocument,
    index: usize,
    replacing: bool,
    fill: &(String, String),
) -> DrDocument {
    let image = render(&doc.profile, &fill.0, (W, H));
    let (name, text) = (
        CString::new("Grad").unwrap(),
        CString::new(fill.1.clone()).unwrap(),
    );
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_doc_artwork(
            doc,
            index,
            replacing,
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
    *unsafe { Box::from_raw(out) }
}

fn publish16(
    doc: &PrecisionDocument,
    index: usize,
    replacing: bool,
    fill: &(String, String),
) -> PrecisionDocument {
    let image = render(&IccProfile::srgb(), &fill.0, (W, H));
    let (name, text) = (
        CString::new("Grad").unwrap(),
        CString::new(fill.1.clone()).unwrap(),
    );
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_artwork(
            doc,
            index,
            replacing,
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
    *unsafe { Box::from_raw(out) }
}

fn selection() -> Vec<u8> {
    (0..H)
        .flat_map(|y| (0..W).map(move |x| (x as i32, y as i32)))
        .map(|(x, y)| {
            let inside =
                x >= RECT[0] && x < RECT[0] + RECT[2] && y >= RECT[1] && y < RECT[1] + RECT[3];
            if inside {
                255
            } else {
                0
            }
        })
        .collect()
}

fn byte_pixels(doc: &DrDocument, index: usize) -> Vec<[u16; 4]> {
    doc.layers[index]
        .pixels
        .to_contiguous()
        .unwrap()
        .pixels()
        .map(|p| p.0.map(u16::from))
        .collect()
}

#[test]
fn a_gradient_fills_smoothness_crosses_as_intr_at_8_bits() {
    let dir = TempDir::new().unwrap();
    let base = DrDocument::from_pixels(solid(W, H, [255; 4]));
    let doc = publish8(&base, 0, false, &fill(&fill_gradient(0.5, false), None));
    let path = dir.path().join("smooth8.psd");
    save8(&doc, &path);
    let body = gdfl_of(&std::fs::read(&path).unwrap(), "Grad");
    assert_eq!(n(&body, "Grad/Intr"), 2048.0, "Intr = Smoothness · 4096");
    assert_eq!(n(&body, "Algn"), 0.0, "Algn as described");
    let report = import_report(&path);
    assert!(
        !report["verified_losses"].to_string().contains("Grad"),
        "{report}"
    );
    let back = open8(&path);
    let d = parse(back.layers[1].meta.as_deref().expect("a description"));
    assert_eq!(d["gradient"]["smoothness"], json!(0.5), "{d}");
    near_samples(&byte_pixels(&back, 1), &byte_pixels(&doc, 1), 1, "smooth8");
    // Smoothness 100 % writes Photoshop's default 4096 and no key.
    let plain = publish8(&base, 0, false, &fill(&fill_gradient(1.0, false), None));
    let path = dir.path().join("plain8.psd");
    save8(&plain, &path);
    let body = gdfl_of(&std::fs::read(&path).unwrap(), "Grad");
    assert_eq!(n(&body, "Grad/Intr"), 4096.0);
    let d = parse(open8(&path).layers[1].meta.as_deref().unwrap());
    assert!(d["gradient"].get("smoothness").is_none(), "{d}");
}

/// The f64 after each `Intrdoub` in `bytes`.
fn intr_values(bytes: &[u8]) -> Vec<f64> {
    bytes
        .windows(8)
        .enumerate()
        .filter(|(_, w)| *w == b"Intrdoub")
        .map(|(i, _)| f64::from_be_bytes(bytes[i + 8..i + 16].try_into().unwrap()))
        .collect()
}

#[test]
fn an_overlays_and_a_strokes_smoothness_cross_as_intr() {
    let dir = TempDir::new().unwrap();
    let stops = json!([{"position": 0, "color": "#102030", "opacity": 1},
        {"position": 1, "color": "#f0e0d0", "opacity": 1}]);
    for (name, effect) in [
        (
            "overlay",
            json!({"type": "gradient_overlay", "gradient": {"stops": stops, "smoothness": 0.25}}),
        ),
        (
            "stroke",
            json!({"type": "stroke", "size": 3, "fill_type": "gradient",
                "gradient": {"stops": stops, "smoothness": 0.25}}),
        ),
    ] {
        let doc = rect_layer_doc((40, 30), (8, 6, 20, 14), [200, 40, 40, 255]);
        let doc = styled(&doc, 1, &json!({"effects": [effect]}).to_string());
        let path = dir.path().join(format!("{name}.psd"));
        save8(&doc, &path);
        assert_eq!(
            intr_values(&std::fs::read(&path).unwrap()),
            vec![1024.0],
            "{name}: Intr = 0.25 · 4096"
        );
        let back = open8(&path);
        let style: Value =
            serde_json::from_str(&back.layers[1].style.as_deref().expect("a style").to_json())
                .unwrap();
        assert_eq!(
            style["effects"][0]["gradient"]["smoothness"],
            json!(0.25),
            "{name}: {style}"
        );
    }
}

#[test]
fn an_in_app_aligned_fill_keeps_its_box_through_psd_at_8_bits() {
    let dir = TempDir::new().unwrap();
    let base = DrDocument::from_pixels(solid(W, H, [255; 4]));
    let g = fill_gradient(1.0, true);
    let doc = publish8(&base, 0, false, &fill(&g, None));
    let doc = doc
        .add_mask(1, MaskKind::FromSelection(&selection()))
        .unwrap();
    let mut frame = [0i32; 4];
    assert!(unsafe { dr_doc_fill_align_box(&doc, 1, frame.as_mut_ptr()) });
    assert_eq!(frame, RECT, "the selection's rectangle");
    // The host's re-render over its mask (`commitNewFillLayer`).
    let doc = publish8(&doc, 1, true, &fill(&g, Some(frame)));
    assert!(doc.layers[1].mask.is_some(), "the mask is kept");
    let path = dir.path().join("aligned8.psd");
    save8(&doc, &path);
    let body = gdfl_of(&std::fs::read(&path).unwrap(), "Grad");
    assert_eq!(n(&body, "Algn"), 1.0, "Algn true");
    let back = open8(&path);
    let d = parse(back.layers[1].meta.as_deref().expect("a description"));
    assert_eq!(d["gradient"]["align_with_layer"], json!(true), "{d}");
    let mut again = [0i32; 4];
    assert!(unsafe { dr_doc_fill_align_box(&back, 1, again.as_mut_ptr()) });
    assert_eq!(again, RECT, "the reopened box");
    near_samples(&byte_pixels(&back, 1), &byte_pixels(&doc, 1), 1, "aligned8");
    assert_ne!(
        byte_pixels(&doc, 1),
        byte_pixels(&publish8(&base, 0, false, &fill(&g, None)), 1),
        "the box moves the gradient"
    );
}

#[test]
fn an_in_app_aligned_fill_keeps_its_box_through_psd_at_16_bits() {
    let dir = TempDir::new().unwrap();
    let white = PrecisionImage::from_rgba16(
        W,
        H,
        vec![[65535; 4]; (W * H) as usize],
        IccProfile::srgb(),
        None,
    )
    .unwrap();
    let base = PrecisionDocument::from_parts(
        W,
        H,
        IccProfile::srgb(),
        Metadata::default(),
        Resolution::default(),
        vec![PrecisionDocumentLayer::raster("Base", white, (0, 0), 1.0, true).unwrap()],
    )
    .unwrap();
    let g = fill_gradient(0.4, true);
    let doc = publish16(&base, 0, false, &fill(&g, None));
    let samples: Vec<u16> = selection().iter().map(|&v| u16::from(v) * 257).collect();
    let plane = PrecisionPlane::from_u16(W, H, samples, None).unwrap();
    let mask = PrecisionMask::new(plane, MaskProperties::at((0, 0)), true).unwrap();
    let layers: Vec<PrecisionDocumentLayer> = vec![
        doc.layer(0).unwrap().clone(),
        doc.layer(1).unwrap().clone().with_mask(Some(mask)),
    ];
    let doc = PrecisionDocument::from_parts(
        W,
        H,
        IccProfile::srgb(),
        Metadata::default(),
        Resolution::default(),
        layers,
    )
    .unwrap();
    let mut frame = [0i32; 4];
    assert!(unsafe { dr_precision_doc_fill_align_box(&doc, 1, frame.as_mut_ptr()) });
    assert_eq!(frame, RECT);
    let doc = publish16(&doc, 1, true, &fill(&g, Some(frame)));
    assert!(doc.layer(1).unwrap().mask().is_some(), "the mask is kept");
    let path = dir.path().join("aligned16.psd");
    save16(&doc, &path);
    let body = gdfl_of(&std::fs::read(&path).unwrap(), "Grad");
    assert_eq!(n(&body, "Algn"), 1.0, "Algn true");
    assert!((n(&body, "Grad/Intr") - 0.4 * 4096.0).abs() < 1e-3, "Intr");
    let back = open16(&path);
    let d = parse(back.artwork_description(1).expect("a description"));
    assert_eq!(d["gradient"]["align_with_layer"], json!(true), "{d}");
    assert_eq!(d["gradient"]["smoothness"], json!(0.4), "{d}");
    let mut again = [0i32; 4];
    assert!(unsafe { dr_precision_doc_fill_align_box(&back, 1, again.as_mut_ptr()) });
    assert_eq!(again, RECT, "the reopened box");
    let samples =
        |d: &PrecisionDocument| d.layer(1).unwrap().source().unwrap().to_rgba16().unwrap();
    near_samples(&samples(&back), &samples(&doc), 257, "aligned16");
}
