//! The Stroke's Shape Burst and Dither in PSD: our 8- and
//! 16-bit exports reopened through our own importer equal the document
//! they came from (`GrdT` `shapeburst`, `Dthr` true on a stroke and on a
//! Gradient Overlay), a style without the options writes `Dthr` false and
//! no `shapeburst`, and the PSD Photoshop authored in
//! `scripts/photoshop-probe/stroke_fill_take.py`'s take (SB-C: an 8 px
//! Center Shape Burst stroke, `fixtures/psd_photoshop/stroke_fill`) reads
//! as a Shape Burst stroke whose render stays within the residual the
//! runner recorded for the shipped rule. The CMYK round trip lives with the
//! CMYK helpers (`precision_stroke_fill`).

use std::ffi::{c_char, CString};
use std::path::Path;
use std::ptr;

use doctored_core::doc::DrDocument;
use doctored_core::ffi_precision_edit::dr_precision_doc_edit;
use doctored_core::ffi_psd::{
    dr_doc_open_psd, dr_doc_save_psd, dr_precision_doc_open_psd, dr_precision_doc_save_psd,
    dr_psd_import_report,
};
use doctored_core::icc::IccProfile;
use doctored_core::job_control::DrJobControl;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer as Layer};
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use doctored_core::style::LayerStyle;
use serde_json::{json, Value};
use tempfile::TempDir;

use crate::common::{cpath, rect_layer_doc, styled, take_err_string};

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
const CANVAS: (u32, u32) = (96, 64);
const RECT: (i32, i32, u32, u32) = (13, 9, 50, 40);
const GREEN: [u8; 4] = [0x30, 0xa0, 0x50, 255];

// ---------------------------------------------------------------- plumbing --

fn import_report(path: &Path) -> Value {
    let mut error = ptr::null_mut();
    let raw = unsafe { dr_psd_import_report(cpath(path).as_ptr(), &mut error) };
    assert!(!raw.is_null(), "{}", take_err_string(error));
    serde_json::from_str(&take_err_string(raw)).expect("import report JSON")
}

fn open8(path: &Path) -> DrDocument {
    let report = import_report(path);
    let token = CString::new(report["source_fingerprint"].as_str().unwrap()).unwrap();
    let mut error = ptr::null_mut();
    let raw = unsafe {
        dr_doc_open_psd(
            cpath(path).as_ptr(),
            0,
            token.as_ptr(),
            std::ptr::null(),
            &mut error,
        )
    };
    assert!(!raw.is_null(), "open_psd: {}", take_err_string(error));
    *unsafe { Box::from_raw(raw) }
}

fn open16(path: &Path) -> PrecisionDocument {
    let report = import_report(path);
    let token = CString::new(report["source_fingerprint"].as_str().unwrap()).unwrap();
    let mut error = ptr::null_mut();
    let raw = unsafe {
        dr_precision_doc_open_psd(
            cpath(path).as_ptr(),
            0,
            token.as_ptr(),
            LIMIT,
            0,
            ptr::null::<DrJobControl>(),
            &mut error,
        )
    };
    assert!(!raw.is_null(), "typed open: {}", take_err_string(error));
    let doc = unsafe { (*raw).clone() };
    unsafe { doctored_core::ffi_precision::dr_precision_doc_free(raw) };
    doc
}

fn save8(doc: &DrDocument, path: &Path) {
    let mut error: *mut c_char = ptr::null_mut();
    let ok = unsafe {
        dr_doc_save_psd(
            doc,
            ptr::null(),
            cpath(path).as_ptr(),
            0,
            0,
            std::ptr::null(),
            &mut error,
        )
    };
    assert!(ok, "save: {}", take_err_string(error));
}

fn save16(doc: &PrecisionDocument, path: &Path) {
    let projection = doc.render(LIMIT, 0, None).unwrap();
    let mut error: *mut c_char = ptr::null_mut();
    let ok = unsafe {
        dr_precision_doc_save_psd(
            doc,
            &projection,
            cpath(path).as_ptr(),
            0,
            0,
            LIMIT,
            0,
            std::ptr::null(),
            &mut error,
        )
    };
    assert!(ok, "save16: {}", take_err_string(error));
}

fn contains(bytes: &[u8], needle: &[u8]) -> bool {
    bytes.windows(needle.len()).any(|w| w == needle)
}

/// A Shape Burst Center stroke with Dither under a dithered Gradient
/// Overlay — every option the round trip carries.
fn options_style() -> Value {
    json!({"effects":[
        {"type":"gradient_overlay","opacity":0.5,
         "gradient":{"style":"radial","dither":true}},
        {"type":"stroke","size":5,"position":"center","fill_type":"gradient",
         "gradient":{"style":"shape_burst","dither":true,"stops":[
            {"position":0,"color":"#102030","opacity":1},
            {"position":1,"color":"#f0e0d0","opacity":1}]}}]})
}

fn byte_doc(style: &Value) -> DrDocument {
    styled(&rect_layer_doc(CANVAS, RECT, GREEN), 1, &style.to_string())
}

fn typed_doc(style: &Value) -> PrecisionDocument {
    let image = |w: u32, h: u32, px: [u8; 4]| {
        PrecisionImage::from_rgba16(
            w,
            h,
            vec![px.map(|v| u16::from(v) * 257); (w * h) as usize],
            IccProfile::srgb(),
            None,
        )
        .unwrap()
    };
    let doc = PrecisionDocument::from_parts(
        CANVAS.0,
        CANVAS.1,
        IccProfile::srgb(),
        Metadata::default(),
        Resolution::default(),
        vec![
            Layer::raster(
                "Base",
                image(CANVAS.0, CANVAS.1, [255; 4]),
                (0, 0),
                1.0,
                true,
            )
            .unwrap(),
            Layer::raster(
                "Rect",
                image(RECT.2, RECT.3, GREEN),
                (RECT.0, RECT.1),
                1.0,
                true,
            )
            .unwrap(),
        ],
    )
    .unwrap();
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
    assert!(!out.is_null(), "{}", take_err_string(err));
    *unsafe { Box::from_raw(out) }
}

// ------------------------------------------------------------ round trips --

#[test]
fn our_8_bit_psd_reopens_with_shape_burst_and_dither() {
    let dir = TempDir::new().unwrap();
    let doc = byte_doc(&options_style());
    let path = dir.path().join("o.psd");
    save8(&doc, &path);
    let bytes = std::fs::read(&path).unwrap();
    assert!(
        contains(&bytes, b"GrdT\x00\x00\x00\x0ashapeburst"),
        "GrdT shapeburst"
    );
    assert!(contains(&bytes, b"Dthrbool\x01"), "Dthr true");
    let reopened = open8(&path);
    assert_eq!(
        reopened.layers[1].style.as_deref().map(LayerStyle::to_json),
        doc.layers[1].style.as_deref().map(LayerStyle::to_json),
        "the style"
    );
    assert_eq!(
        reopened.flattened().unwrap(),
        doc.flattened().unwrap(),
        "the picture"
    );
}

#[test]
fn our_16_bit_psd_reopens_with_shape_burst_and_dither() {
    let dir = TempDir::new().unwrap();
    let doc = typed_doc(&options_style());
    let path = dir.path().join("o16.psd");
    save16(&doc, &path);
    let bytes = std::fs::read(&path).unwrap();
    assert!(
        contains(&bytes, b"GrdT\x00\x00\x00\x0ashapeburst"),
        "GrdT shapeburst"
    );
    assert!(contains(&bytes, b"Dthrbool\x01"), "Dthr true");
    let reopened = open16(&path);
    assert_eq!(
        reopened.layer(1).unwrap().reported_style(),
        doc.layer(1).unwrap().reported_style(),
        "the style"
    );
    let render = |d: &PrecisionDocument| d.render(LIMIT, 0, None).unwrap().to_rgba16().unwrap();
    assert!(render(&reopened) == render(&doc), "the picture");
}

#[test]
fn a_style_without_the_options_writes_dither_false_and_no_shape_burst() {
    let dir = TempDir::new().unwrap();
    let doc = byte_doc(&json!({"effects":[
        {"type":"stroke","size":5,"fill_type":"gradient","gradient":{"style":"radial"}}]}));
    let path = dir.path().join("plain.psd");
    save8(&doc, &path);
    let bytes = std::fs::read(&path).unwrap();
    assert!(!contains(&bytes, b"shapeburst"));
    assert!(contains(&bytes, b"Dthrbool\x00"), "Dthr false, as before");
    assert!(!contains(&bytes, b"Dthrbool\x01"));
}

// ------------------------------------------------- Photoshop's own file --

fn photoshop_fixture(name: &str) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/psd_photoshop/stroke_fill")
        .join(name)
}

/// SB-C under the shipped Shape Burst law (one ramp over `size + 2` across
/// the contour, the Classic ramp): (max, mean) codes over its judged pixels —
/// the straight runs more than 9 px from every rect corner, k = 1..3 px on
/// each side of the contour. Measured 1 / 0.333 through this file's
/// composite, pinned there (lowered from the old rule's 17.188 / 14.333).
const SB_C_RESIDUAL: (f64, f64) = (1.0, 0.334);

#[test]
fn photoshops_shape_burst_file_reads_within_the_recorded_residual() {
    let doc = open8(&photoshop_fixture("sb_c_rgb8.psd"));
    let layer = doc
        .layers
        .iter()
        .find(|l| l.style.is_some())
        .expect("the styled layer");
    let style: Value = serde_json::from_str(&layer.style.as_deref().unwrap().to_json()).unwrap();
    // Photoshop's `lfx2` keeps its other effects' slots (disabled).
    let e = style["effects"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["type"] == "stroke")
        .expect("the stroke");
    assert_eq!(e["enabled"], true);
    assert_eq!(e["position"], "center");
    assert_eq!(e["size"], 8.0);
    assert_eq!(e["fill_type"], "gradient");
    assert_eq!(e["gradient"]["style"], "shape_burst", "GrdT shapeburst");
    assert!(e["gradient"].get("dither").is_none(), "Dthr false");
    let ours = doc.flattened().unwrap();
    let theirs = DrDocument::open(photoshop_fixture("sb_c_rgb8.tif").to_str().unwrap())
        .unwrap()
        .flattened()
        .unwrap();
    let (x0, y0) = (RECT.0, RECT.1);
    let (x1, y1) = (x0 + RECT.2 as i32 - 1, y0 + RECT.3 as i32 - 1);
    let rows = y0 + 10..=y1 - 10;
    let cols = x0 + 10..=x1 - 10;
    let mut pixels = Vec::new();
    for k in 1..=3 {
        for y in rows.clone() {
            pixels.extend([(x0 - k, y), (x1 + k, y), (x0 + k - 1, y), (x1 - k + 1, y)]);
        }
        for x in cols.clone() {
            pixels.extend([(x, y0 - k), (x, y1 + k), (x, y0 + k - 1), (x, y1 - k + 1)]);
        }
    }
    let (mut worst, mut total) = (0f64, 0f64);
    for &(x, y) in &pixels {
        let (a, b) = (
            ours.get_pixel(x as u32, y as u32).0,
            theirs.get_pixel(x as u32, y as u32).0,
        );
        let d = (0..3)
            .map(|c| f64::from((i32::from(a[c]) - i32::from(b[c])).abs()))
            .fold(0.0, f64::max);
        worst = worst.max(d);
        total += d;
    }
    let mean = total / pixels.len() as f64;
    assert_eq!(pixels.len(), 600, "the judged set");
    assert!(
        worst <= SB_C_RESIDUAL.0 + 2.0 && mean <= SB_C_RESIDUAL.1 + 0.5,
        "SB-C: max {worst} mean {mean:.3} against the recorded {SB_C_RESIDUAL:?}"
    );
    // The fill well inside the band reads as Photoshop's.
    for (x, y) in [(38, 29), (30, 20), (50, 40)] {
        let (a, b) = (ours.get_pixel(x, y).0, theirs.get_pixel(x, y).0);
        for c in 0..3 {
            assert!(
                (i32::from(a[c]) - i32::from(b[c])).abs() <= 2,
                "the fill at ({x}, {y})"
            );
        }
    }
}
