//! The Stroke's Fill Type Pattern in PSD: our 8- and 16-bit
//! exports reopened through our own importer equal the document they came
//! from (`PntT` `Ptrn`, the fill's `Lnkd`, the tile in `Patt` at 8 bits and
//! `Pat2` at 16, and the `fxrp` of a linked stroke whose layer moved after
//! the style), and the PSD Photoshop authored in
//! `scripts/photoshop-probe/stroke_fill_take.py`'s take (PT-O: an 8 px
//! Outside stroke of tile T, `fixtures/psd_photoshop/stroke_fill`) reads as
//! a pattern stroke whose render is Photoshop's own flattened TIFF within
//! one code on the take's judged pixels. The CMYK round trip lives with the
//! CMYK helpers (`precision_stroke_pattern`).

use std::ffi::{c_char, CString};
use std::path::{Path, PathBuf};
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
    let raw = unsafe { dr_doc_open_psd(cpath(path).as_ptr(), 0, token.as_ptr(), &mut error) };
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
    let ok = unsafe { dr_doc_save_psd(doc, ptr::null(), cpath(path).as_ptr(), 0, 0, &mut error) };
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
            &mut error,
        )
    };
    assert!(ok, "save16: {}", take_err_string(error));
}

fn contains(bytes: &[u8], needle: &[u8]) -> bool {
    bytes.windows(needle.len()).any(|w| w == needle)
}

fn base64(bytes: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for i in 0..4 {
            out.push(if i <= chunk.len() {
                T[((n >> (18 - 6 * i)) & 63) as usize] as char
            } else {
                '='
            });
        }
    }
    out
}

/// take1's tile T: 12 x 8, opaque.
fn texel(x: u32, y: u32) -> [u8; 3] {
    [
        (20 * x + 10) as u8,
        (30 * y + 15) as u8,
        if (x + y).is_multiple_of(2) { 200 } else { 60 },
    ]
}

fn tile_t() -> Value {
    let mut rgba = Vec::new();
    for y in 0..8 {
        for x in 0..12 {
            rgba.extend_from_slice(&texel(x, y));
            rgba.push(255);
        }
    }
    json!({"name":"T","width":12,"height":8,"rgba":base64(&rgba)})
}

/// A linked Outside stroke of T at 50 %, 30°, phase (3, 1); the round trips
/// move the layer after the style so its `fxrp` is not zero.
fn pattern_style() -> Value {
    json!({"effects":[{"type":"stroke","size":6,"position":"outside",
        "fill_type":"pattern","pattern_fill":{"pattern":tile_t(),"scale":0.5,
        "angle":30,"phase":[3,1]}}]})
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

/// The `fxrp` body's (x, y) in `bytes`.
fn fxrp(bytes: &[u8]) -> (f64, f64) {
    let at = bytes
        .windows(8)
        .position(|w| w == b"8BIMfxrp")
        .expect("an fxrp block");
    (
        f64::from_be_bytes(bytes[at + 12..at + 20].try_into().unwrap()),
        f64::from_be_bytes(bytes[at + 20..at + 28].try_into().unwrap()),
    )
}

/// `PntT` enum `FrFl` `Ptrn` and a linked `Lnkd`.
fn assert_pattern_stroke_items(bytes: &[u8], what: &str) {
    assert!(
        contains(bytes, b"FrFl\x00\x00\x00\x00Ptrn"),
        "{what}: PntT Ptrn"
    );
    assert!(contains(bytes, b"Lnkdbool\x01"), "{what}: Lnkd true");
}

// ------------------------------------------------------------ round trips --

#[test]
fn our_8_bit_psd_reopens_with_the_pattern_stroke() {
    let dir = TempDir::new().unwrap();
    let doc = styled(
        &rect_layer_doc(CANVAS, RECT, GREEN),
        1,
        &pattern_style().to_string(),
    );
    let doc = doc.with_layer_offset(1, RECT.0 + 7, RECT.1 + 3).unwrap();
    let path = dir.path().join("p.psd");
    save8(&doc, &path);
    let bytes = std::fs::read(&path).unwrap();
    assert!(contains(&bytes, b"8BIMPatt"), "the pattern table");
    assert_pattern_stroke_items(&bytes, "8-bit");
    assert_eq!(fxrp(&bytes), (7.0, 3.0), "offset − anchor");
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
fn our_16_bit_psd_reopens_with_the_pattern_stroke() {
    let dir = TempDir::new().unwrap();
    let mut style = pattern_style();
    // Set at (6, 6), then moved (+7, +3) to RECT: the anchor says so.
    style["effects"][0]["pattern_fill"]["anchor"] = json!([6, 6]);
    let doc = typed_doc(&style);
    let path = dir.path().join("p16.psd");
    save16(&doc, &path);
    let bytes = std::fs::read(&path).unwrap();
    assert!(contains(&bytes, b"8BIMPat2"), "the 16-bit pattern table");
    assert_pattern_stroke_items(&bytes, "16-bit");
    assert_eq!(fxrp(&bytes), (7.0, 3.0), "offset − anchor");
    let reopened = open16(&path);
    assert_eq!(
        reopened.layer(1).unwrap().reported_style(),
        doc.layer(1).unwrap().reported_style(),
        "the style"
    );
    let render = |d: &PrecisionDocument| d.render(LIMIT, 0, None).unwrap().to_rgba16().unwrap();
    assert!(render(&reopened) == render(&doc), "the picture");
}

// ------------------------------------------------- Photoshop's own file --

fn photoshop_fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/psd_photoshop/stroke_fill")
        .join(name)
}

#[test]
fn photoshops_pattern_stroke_file_reads_and_renders_as_its_tiff() {
    let doc = open8(&photoshop_fixture("pt_o_rgb8.psd"));
    let index = doc
        .layers
        .iter()
        .position(|l| l.style.is_some())
        .expect("the styled layer");
    let style: Value =
        serde_json::from_str(&doc.layers[index].style.as_deref().unwrap().to_json()).unwrap();
    let e = style["effects"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["type"] == "stroke")
        .expect("the stroke");
    assert_eq!(e["enabled"], true);
    assert_eq!(e["position"], "outside");
    assert_eq!(e["size"], 8.0);
    assert_eq!(e["fill_type"], "pattern");
    let fill = &e["pattern_fill"];
    assert_eq!(fill["link_with_layer"], true, "Lnkd");
    assert_eq!(fill["angle"], 0.0, "Angl 0");
    assert_eq!(fill["scale"], 1.0);
    assert_eq!(fill["pattern"]["width"], 12);
    assert_eq!(fill["pattern"]["height"], 8);
    let ours = doc.flattened().unwrap();
    let theirs = DrDocument::open(photoshop_fixture("pt_o_rgb8.tif").to_str().unwrap())
        .unwrap()
        .flattened()
        .unwrap();
    // The take's judged pixels: the straight runs more than 9 px from every
    // corner, k = 1..7 px outside the contour.
    let (x0, y0) = (RECT.0, RECT.1);
    let (x1, y1) = (x0 + RECT.2 as i32 - 1, y0 + RECT.3 as i32 - 1);
    let mut pixels = Vec::new();
    for k in 1..=7 {
        for y in y0 + 10..=y1 - 10 {
            pixels.extend([(x0 - k, y), (x1 + k, y)]);
        }
        for x in x0 + 10..=x1 - 10 {
            pixels.extend([(x, y0 - k), (x, y1 + k)]);
        }
    }
    assert!(pixels.len() > 300);
    for (x, y) in pixels {
        let (a, b) = (
            ours.get_pixel(x as u32, y as u32).0,
            theirs.get_pixel(x as u32, y as u32).0,
        );
        for c in 0..3 {
            assert!(
                (i32::from(a[c]) - i32::from(b[c])).abs() <= 1,
                "({x}, {y}): {a:?} against Photoshop {b:?}"
            );
        }
    }
}
