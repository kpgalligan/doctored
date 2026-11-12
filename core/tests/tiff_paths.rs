//! Paths in a FLAT TIFF (`tiff_paths`), through the FFI: the saved paths,
//! the work path and the clipping path as image resources 2000…, 1025 and
//! 2999 in tag 34377, the same blocks a PSD's resource section holds.
//!
//! Oracles: this file's own TIFF tag and 8BIM readers
//! (`psd_clipping_path::tiff_run`, `run_resources`), the 26-byte records
//! computed from Adobe's layout (`psd_paths::expected_records`) and the
//! Pascal-string 2999 body (`psd_clipping_path::clipping_body`).
//!
//! - A byte RGB save, a 16-bit RGB save and a CMYK save write every path
//!   and the clipping path, and each reopens with them.
//! - A TIFF that brought its paths writes the document's current ones, once:
//!   cropped, they are the cropped records; deleted, none is written.
//! - The flat save's "paths" loss is gone for a TIFF that writes every path
//!   and stays for PNG, JPEG, a path past 128 canvases and a Grayscale TIFF.
//! - A file stored rotated reads no path and keeps none, at 8 and 16 bits,
//!   and the report says why.
//! - A run whose paths no open read (a JPEG's APP13) passes through a TIFF
//!   of a document with no path, gives way to one with paths, and loses its
//!   paths at every whole-canvas edit but Image Size, at both depths, even a
//!   flip that moves no pixel.

use std::ffi::{c_char, CString};
use std::path::Path;
use std::ptr;
use std::sync::Arc;

use doctored_core::doc::DrDocument;
use doctored_core::doc_path::PathEntry;
use doctored_core::ffi_cmyk::dr_precision_doc_save_cmyk;
use doctored_core::ffi_color::dr_doc_save_image;
use doctored_core::ffi_doc::dr_doc_open;
use doctored_core::ffi_metadata_report::dr_path_metadata_import_report;
use doctored_core::ffi_path::{dr_doc_with_paths_json, dr_precision_doc_with_paths_json};
use doctored_core::ffi_precision::dr_precision_doc_open;
use doctored_core::ffi_precision_conversion::dr_doc_promote_structure;
use doctored_core::ffi_precision_edit::dr_precision_doc_edit;
use doctored_core::ffi_precision_export::dr_precision_doc_save_projection16;
use doctored_core::ffi_preservation::{
    dr_doc_flat_save_losses, dr_precision_doc_flat_state_losses,
};
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES as LIMIT};
use image::{Rgba, RgbaImage};
use serde_json::{json, Value};
use tempfile::TempDir;

use crate::cmyk_fixture::press_profile;
use crate::common::{byte_geometry_edit, cpath, take_err_string};
use crate::psd_clipping_path::{clipping_body, run_resources, tiff_run};
use crate::psd_paths::{assert_paths_close, expected_records};

const W: u32 = 60;
const H: u32 = 40;
const PNG: i32 = 0;
const JPEG: i32 = 1;
const TIFF: i32 = 2;

/// A work path, a curved saved path and "Box", the clipping path.
fn path_list() -> Value {
    json!({"paths": [
        {"kind": "work", "fill_rule": "nonzero", "subpaths": [
            {"closed": true, "anchors": [[5, 5], [20, 5], [20, 15]]}]},
        {"kind": "saved", "name": "Curve", "fill_rule": "evenodd", "subpaths": [
            {"closed": true, "anchors": [
                {"x": 10, "y": 10, "in": [8, 14], "out": [12, 6], "smooth": true},
                [40, 10],
                {"x": 40, "y": 30, "in": [44, 26], "out": [36, 34]}]},
            {"closed": false, "anchors": [[2.5, 37.25], [57.75, 1.5]]}]},
        {"kind": "saved", "name": "Box", "fill_rule": "nonzero", "clipping": {"tail": ""},
         "subpaths": [{"closed": true, "anchors": [[30, 2], [58, 2], [58, 20], [30, 20]]}]}
    ]})
}

fn with_list(doc: DrDocument, list: &Value) -> Box<DrDocument> {
    let text = CString::new(list.to_string()).unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    let out = unsafe { dr_doc_with_paths_json(&doc, text.as_ptr(), &mut err) };
    assert!(!out.is_null(), "{}", take_err_string(err));
    unsafe { Box::from_raw(out) }
}

fn typed_with_list(doc: &PrecisionDocument, list: &Value) -> Box<PrecisionDocument> {
    let text = CString::new(list.to_string()).unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    let out = unsafe { dr_precision_doc_with_paths_json(doc, text.as_ptr(), &mut err) };
    assert!(!out.is_null(), "{}", take_err_string(err));
    unsafe { Box::from_raw(out) }
}

fn byte_doc() -> Box<DrDocument> {
    let base = DrDocument::from_pixels(RgbaImage::from_fn(W, H, |x, y| {
        Rgba([(x * 4) as u8, (y * 6) as u8, 90, 255])
    }));
    with_list(base, &path_list())
}

fn promoted(doc: &DrDocument) -> Box<PrecisionDocument> {
    let mut err: *mut c_char = ptr::null_mut();
    let wide = unsafe { dr_doc_promote_structure(doc, LIMIT, 0, ptr::null(), &mut err) };
    assert!(!wide.is_null(), "{}", take_err_string(err));
    unsafe { Box::from_raw(wide) }
}

fn cmyk_doc(doc: &DrDocument) -> PrecisionDocument {
    promoted(doc)
        .convert_to_cmyk(
            Some(&press_profile("Press Test")),
            r#"{"depth":16}"#,
            LIMIT,
            0,
            None,
        )
        .unwrap()
}

fn save_byte(doc: &DrDocument, path: &Path, format: i32) -> Vec<u8> {
    let mut err = ptr::null_mut();
    let ok = unsafe {
        dr_doc_save_image(
            doc,
            ptr::null(),
            cpath(path).as_ptr(),
            format,
            90,
            true,
            false,
            ptr::null_mut(),
            &mut err,
        )
    };
    assert!(ok, "{}: {}", path.display(), take_err_string(err));
    std::fs::read(path).unwrap()
}

/// The typed RGB document's flat TIFF, as its Save writes it.
fn save_typed(doc: &PrecisionDocument, path: &Path) -> Vec<u8> {
    let projection = doc.render(LIMIT, 0, None).unwrap();
    let mut err = ptr::null_mut();
    let ok = unsafe {
        dr_precision_doc_save_projection16(
            doc,
            &projection,
            cpath(path).as_ptr(),
            1,
            true,
            false,
            LIMIT,
            0,
            ptr::null_mut(),
            &mut err,
        )
    };
    assert!(ok, "{}: {}", path.display(), take_err_string(err));
    std::fs::read(path).unwrap()
}

fn save_cmyk(doc: &PrecisionDocument, path: &Path) -> Vec<u8> {
    let request = CString::new(r#"{"format":"tiff"}"#).unwrap();
    let mut err = ptr::null_mut();
    let ok = unsafe {
        dr_precision_doc_save_cmyk(
            doc,
            cpath(path).as_ptr(),
            request.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(ok, "{}: {}", path.display(), take_err_string(err));
    std::fs::read(path).unwrap()
}

fn open_byte(path: &Path) -> Box<DrDocument> {
    let mut err = ptr::null_mut();
    let doc = unsafe { dr_doc_open(cpath(path).as_ptr(), &mut err) };
    assert!(!doc.is_null(), "{}", take_err_string(err));
    unsafe { Box::from_raw(doc) }
}

fn open_typed(path: &Path) -> Box<PrecisionDocument> {
    let mut err = ptr::null_mut();
    let doc =
        unsafe { dr_precision_doc_open(cpath(path).as_ptr(), LIMIT, 0, ptr::null(), &mut err) };
    assert!(!doc.is_null(), "{}", take_err_string(err));
    unsafe { Box::from_raw(doc) }
}

fn is_path_id(id: u16) -> bool {
    id == 1025 || id == 2999 || (2000..=2997).contains(&id)
}

/// The path resources of a TIFF's tag 34377, in order, as (id, name, body).
fn path_resources(file: &[u8]) -> Vec<(u16, String, Vec<u8>)> {
    run_resources(&tiff_run(file))
        .into_iter()
        .filter(|(id, _, _)| is_path_id(*id))
        .collect()
}

/// The path resources a TIFF of `paths` on a `canvas` must hold, computed
/// from the published layouts: the work path, each saved path by its name,
/// then the clipping path's name.
fn expected_resources(paths: &[PathEntry], canvas: (u32, u32)) -> Vec<(u16, String, Vec<u8>)> {
    let size = (f64::from(canvas.0), f64::from(canvas.1));
    let mut saved = 2000;
    let mut out = Vec::new();
    for entry in paths {
        let records = expected_records(&entry.shape, size);
        if entry.name == "Work Path" {
            out.push((1025, String::new(), records));
        } else {
            out.push((saved, entry.name.clone(), records));
            saved += 1;
        }
    }
    if let Some(entry) = paths.iter().find(|e| e.clipping.is_some()) {
        out.push((2999, String::new(), clipping_body(&entry.name, &[])));
    }
    out
}

fn clipping_names(paths: &[PathEntry]) -> Vec<String> {
    paths
        .iter()
        .filter(|e| e.clipping.is_some())
        .map(|e| e.name.clone())
        .collect()
}

#[test]
fn rgb_and_cmyk_flat_tiffs_carry_their_paths_and_clipping_path() {
    let dir = TempDir::new().unwrap();
    let doc = byte_doc();
    let want = doc.paths().to_vec();
    assert_eq!(clipping_names(&want), ["Box"]);

    let path = dir.path().join("byte.tif");
    let file = save_byte(&doc, &path, TIFF);
    assert_eq!(
        path_resources(&file),
        expected_resources(&want, (W, H)),
        "8 bits"
    );
    let back = open_byte(&path);
    assert_paths_close(back.paths(), &want, "8-bit reopen");
    assert_eq!(clipping_names(back.paths()), ["Box"], "8-bit reopen");

    let wide = promoted(&doc);
    let path = dir.path().join("rgb16.tif");
    let file = save_typed(&wide, &path);
    assert_eq!(
        path_resources(&file),
        expected_resources(&want, (W, H)),
        "16 bits"
    );
    let back = open_typed(&path);
    assert_paths_close(back.paths(), &want, "16-bit reopen");
    assert_eq!(clipping_names(back.paths()), ["Box"], "16-bit reopen");

    let cmyk = cmyk_doc(&doc);
    assert!(cmyk.is_cmyk());
    let path = dir.path().join("cmyk.tif");
    let file = save_cmyk(&cmyk, &path);
    assert_eq!(
        path_resources(&file),
        expected_resources(&want, (W, H)),
        "CMYK"
    );
    let back = open_typed(&path);
    assert!(back.is_cmyk(), "reopened as CMYK");
    assert_paths_close(back.paths(), &want, "CMYK reopen");
    assert_eq!(clipping_names(back.paths()), ["Box"], "CMYK reopen");
}

#[test]
fn a_reopened_tiff_writes_its_current_paths_once() {
    let dir = TempDir::new().unwrap();
    let first = dir.path().join("first.tif");
    save_byte(&byte_doc(), &first, TIFF);
    let opened = open_byte(&first);
    assert_eq!(opened.paths().len(), 3, "the open reads the paths");
    let run = opened.metadata.iptc.as_deref().unwrap_or_default();
    assert!(
        run_resources(run).iter().all(|(id, _, _)| !is_path_id(*id)),
        "the open takes them out of the document's run"
    );

    // Cropped: the file holds one set, the cropped document's.
    let cropped = opened.crop(5, 5, 40, 30).unwrap();
    let path = dir.path().join("cropped.tif");
    let file = save_byte(&cropped, &path, TIFF);
    assert_eq!(
        path_resources(&file),
        expected_resources(cropped.paths(), (40, 30)),
        "the cropped records, once"
    );

    // Deleted: none comes back from the inherited run.
    let cleared = cropped.with_paths(Vec::new()).unwrap();
    let path = dir.path().join("cleared.tif");
    let file = save_byte(&cleared, &path, TIFF);
    assert!(path_resources(&file).is_empty(), "no path resource");
    assert!(open_byte(&path).paths().is_empty());

    // The same at 16 bits and in CMYK: the run a reopened file brought is
    // not written beside the document's own.
    let wide_first = dir.path().join("wide-first.tif");
    save_typed(&promoted(&byte_doc()), &wide_first);
    let wide = open_typed(&wide_first).with_paths(Vec::new()).unwrap();
    let file = save_typed(&wide, &dir.path().join("wide.tif"));
    assert!(
        path_resources(&file).is_empty(),
        "16 bits: no path resource"
    );
    let cmyk = cmyk_doc(&opened);
    let cmyk_path = dir.path().join("cmyk.tif");
    save_cmyk(&cmyk, &cmyk_path);
    let cmyk = open_typed(&cmyk_path).with_paths(Vec::new()).unwrap();
    let file = save_cmyk(&cmyk, &dir.path().join("cmyk-cleared.tif"));
    assert!(path_resources(&file).is_empty(), "CMYK: no path resource");
}

fn byte_losses(doc: &DrDocument, format: i32) -> Vec<String> {
    let mut err = ptr::null_mut();
    let losses = unsafe { dr_doc_flat_save_losses(doc, format, true, false, &mut err) };
    assert!(!losses.is_null(), "{}", take_err_string(err));
    serde_json::from_str(&take_err_string(losses)).unwrap()
}

fn typed_losses(doc: &PrecisionDocument, tiff: bool) -> Vec<String> {
    let mut err = ptr::null_mut();
    let losses = unsafe { dr_precision_doc_flat_state_losses(doc, false, tiff, &mut err) };
    assert!(!losses.is_null(), "{}", take_err_string(err));
    serde_json::from_str(&take_err_string(losses)).unwrap()
}

/// An image resource block by Adobe's layout: `8BIM`, the id, the Pascal
/// name padded even, the body padded even.
fn block(id: u16, name: &str, body: &[u8]) -> Vec<u8> {
    let mut out = b"8BIM".to_vec();
    out.extend_from_slice(&id.to_be_bytes());
    out.push(name.len() as u8);
    out.extend_from_slice(name.as_bytes());
    if name.len().is_multiple_of(2) {
        out.push(0);
    }
    out.extend_from_slice(&(body.len() as u32).to_be_bytes());
    out.extend_from_slice(body);
    if body.len() % 2 == 1 {
        out.push(0);
    }
    out
}

/// The records of "Clip", the one path of [`foreign_run`], on the W × H
/// canvas.
fn clip_records() -> Vec<u8> {
    let records = byte_doc().paths()[1].shape.clone();
    expected_records(&records, (f64::from(W), f64::from(H)))
}

/// A run a file whose open reads no path brought (a JPEG's APP13): an IPTC
/// caption (1028), then the saved path "Clip" (2000).
fn foreign_run() -> Arc<[u8]> {
    let caption = block(1028, "", &[0x1c, 0x02, 0x00, 0x00, 0x00]);
    [caption, block(2000, "Clip", &clip_records())]
        .concat()
        .into()
}

/// The ids of a document's 8BIM run, in order.
fn held(run: Option<&[u8]>) -> Vec<u16> {
    run_resources(run.unwrap_or_default())
        .into_iter()
        .map(|(id, _, _)| id)
        .collect()
}

/// A run a file whose open reads no path brought (a JPEG's APP13): its
/// path resources pass through a TIFF of a document with no path of its
/// own, and a document that has one writes its own in their place and
/// names the loss.
#[test]
fn a_run_whose_paths_the_open_did_not_read_passes_through_until_the_document_has_paths() {
    let dir = TempDir::new().unwrap();
    let mut doc = DrDocument::from_pixels(RgbaImage::from_pixel(W, H, Rgba([9, 9, 9, 255])));
    doc.metadata.iptc = Some(foreign_run());
    let file = save_byte(&doc, &dir.path().join("through.tif"), TIFF);
    assert_eq!(
        path_resources(&file),
        vec![(2000, "Clip".to_string(), clip_records())],
        "passed through"
    );
    assert!(!byte_losses(&doc, TIFF).contains(&"paths".to_string()));

    let doc = with_list(doc, &path_list());
    let file = save_byte(&doc, &dir.path().join("own.tif"), TIFF);
    assert_eq!(
        path_resources(&file),
        expected_resources(doc.paths(), (W, H)),
        "the document's own paths, once"
    );
    assert!(
        run_resources(&tiff_run(&file))
            .iter()
            .any(|(id, _, _)| *id == 1028),
        "the run's other resources stay"
    );
    assert!(
        byte_losses(&doc, TIFF).contains(&"paths".to_string()),
        "the run's path is lost"
    );
}

#[test]
fn a_flat_tiff_that_writes_every_path_does_not_list_paths_as_lost() {
    let doc = byte_doc();
    let paths = "paths".to_string();
    assert!(!byte_losses(&doc, TIFF).contains(&paths), "TIFF");
    assert!(byte_losses(&doc, PNG).contains(&paths), "PNG");
    assert!(byte_losses(&doc, JPEG).contains(&paths), "JPEG");
    // A path past 128 canvases is not written, so the TIFF loses it.
    let mut far = path_list();
    far["paths"].as_array_mut().unwrap().push(json!({
        "kind": "saved", "name": "Far", "subpaths": [
            {"closed": false, "anchors": [[0, 0], [10000, 0]]}]}));
    let base = DrDocument::from_pixels(RgbaImage::from_pixel(W, H, Rgba([1, 2, 3, 255])));
    let far_doc = with_list(base, &far);
    assert!(
        byte_losses(&far_doc, TIFF).contains(&paths),
        "a path past 128 canvases"
    );

    let cmyk = cmyk_doc(&doc);
    assert!(!typed_losses(&cmyk, true).contains(&paths), "CMYK TIFF");
    assert!(typed_losses(&cmyk, false).contains(&paths), "CMYK JPEG");
    let gray = promoted(&doc)
        .convert_to_gray(
            None,
            r#"{"space":"gray_gamma_22","depth":16}"#,
            LIMIT,
            0,
            None,
        )
        .unwrap();
    let gray = typed_with_list(&gray, &path_list());
    assert!(gray.is_gray());
    assert!(
        typed_losses(&gray, true).contains(&paths),
        "a Grayscale TIFF"
    );
}

/// `file`, one of this crate's TIFFs, with its Orientation (one SHORT,
/// value 1) made 6.
fn rotated(mut file: Vec<u8>) -> Vec<u8> {
    assert_eq!(&file[..2], b"II", "this writer is little-endian");
    let ifd = u32::from_le_bytes(file[4..8].try_into().unwrap()) as usize;
    let entries = usize::from(u16::from_le_bytes([file[ifd], file[ifd + 1]]));
    let at = (0..entries)
        .map(|e| ifd + 2 + e * 12)
        .find(|&at| u16::from_le_bytes([file[at], file[at + 1]]) == 274)
        .expect("an Orientation entry");
    assert_eq!(&file[at + 8..at + 10], &1u16.to_le_bytes());
    file[at + 8..at + 10].copy_from_slice(&6u16.to_le_bytes());
    file
}

const ROTATED: &str =
    "Paths in this TIFF are not read or kept: it is stored rotated (Orientation 6).";

/// A file stored rotated opens upright, where its records name other
/// places: no path is read, none stays in the run, so a TIFF saved from it
/// (written upright, Orientation 1) reopens with none; the report and the
/// typed open's warnings say so.
#[test]
fn a_tiff_stored_rotated_reads_and_keeps_no_path_and_says_so() {
    let dir = TempDir::new().unwrap();
    let upright = dir.path().join("upright.tif");
    let file = save_byte(&byte_doc(), &upright, TIFF);
    let path = dir.path().join("rotated.tif");
    std::fs::write(&path, rotated(file)).unwrap();
    let mut err = ptr::null_mut();
    let report = unsafe { dr_path_metadata_import_report(cpath(&path).as_ptr(), &mut err) };
    assert!(!report.is_null(), "{}", take_err_string(err));
    let report: Value = serde_json::from_str(&take_err_string(report)).unwrap();
    let warnings: Vec<&str> = report["warnings"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(Value::as_str)
        .collect();
    assert!(warnings.contains(&ROTATED), "{warnings:?}");
    let opened = open_byte(&path);
    assert!(opened.paths().is_empty());
    assert!(
        !held(opened.metadata.iptc.as_deref())
            .iter()
            .any(|id| is_path_id(*id)),
        "8 bits: the run keeps no path"
    );
    let again = dir.path().join("again.tif");
    let file = save_byte(&opened, &again, TIFF);
    assert!(path_resources(&file).is_empty(), "8 bits: none written");
    assert!(open_byte(&again).paths().is_empty(), "8 bits: none read");
    let doc = open_byte(&upright);
    assert_eq!(doc.paths().len(), 3, "the upright file reads all three");

    let file = save_typed(&promoted(&byte_doc()), &dir.path().join("wide.tif"));
    let path = dir.path().join("wide-rotated.tif");
    std::fs::write(&path, rotated(file)).unwrap();
    let opened = open_typed(&path);
    assert!(opened.paths().is_empty());
    assert!(
        !held(opened.metadata().iptc.as_deref())
            .iter()
            .any(|id| is_path_id(*id)),
        "16 bits: the run keeps no path"
    );
    let info = opened.info().unwrap();
    assert!(
        info["import_warnings"]
            .as_array()
            .unwrap()
            .contains(&json!(ROTATED)),
        "{}",
        info["import_warnings"]
    );
    let again = dir.path().join("wide-again.tif");
    let file = save_typed(&opened, &again);
    assert!(path_resources(&file).is_empty(), "16 bits: none written");
    assert!(open_typed(&again).paths().is_empty(), "16 bits: none read");
}

/// A byte document whose 8BIM run is `run`: a gradient, so every turn and
/// flip changes the pixels, or one colour, so a flip changes none.
fn byte_canvas(gradient: bool, run: Option<Arc<[u8]>>) -> DrDocument {
    let mut doc = DrDocument::from_pixels(RgbaImage::from_fn(W, H, |x, y| {
        if gradient {
            Rgba([(x * 4) as u8, (y * 6) as u8, 90, 255])
        } else {
            Rgba([9, 9, 9, 255])
        }
    }));
    doc.metadata.iptc = run;
    doc
}

/// [`byte_canvas`] at 16 bits.
fn typed_canvas(gradient: bool, run: Option<Arc<[u8]>>) -> PrecisionDocument {
    let pixels = (0..W * H)
        .map(|i| {
            if gradient {
                [
                    ((i % W) * 1000) as u16,
                    ((i / W) * 1500) as u16,
                    30000,
                    65535,
                ]
            } else {
                [2313, 2313, 2313, 65535]
            }
        })
        .collect();
    let image = PrecisionImage::from_rgba16(W, H, pixels, IccProfile::srgb(), None).unwrap();
    let metadata = Metadata {
        iptc: run,
        ..Metadata::default()
    };
    PrecisionDocument::from_image(image, metadata, Resolution::default()).unwrap()
}

fn byte_edit(doc: &DrDocument, request: &Value) -> Box<DrDocument> {
    let (out, error) = byte_geometry_edit(doc, request);
    assert_eq!(error, None, "{request}");
    assert!(!out.is_null(), "{request} changed nothing");
    unsafe { Box::from_raw(out) }
}

fn typed_edit(doc: &PrecisionDocument, request: &Value) -> Box<PrecisionDocument> {
    let text = CString::new(request.to_string()).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_edit(
            doc,
            text.as_ptr(),
            ptr::null(),
            0,
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!out.is_null(), "{request}: {}", take_err_string(err));
    unsafe { Box::from_raw(out) }
}

/// Every whole-canvas edit but Image Size: the crop, Canvas Size, the
/// turns and flips, Rotate Canvas, a Straighten over the whole canvas (no
/// crop after it) and Perspective Crop.
fn canvas_moves() -> Vec<Value> {
    let (cx, cy) = (f64::from(W) / 2.0, f64::from(H) / 2.0);
    let (s, c) = 3f64.to_radians().sin_cos();
    let m = [c, s, -s, c, cx - cx * c + cy * s, cy - cx * s - cy * c];
    vec![
        json!({"op": "crop", "x": 5, "y": 4, "width": 40, "height": 30}),
        json!({"op": "canvas_size", "width": 70, "height": 50, "x": 5, "y": 5}),
        json!({"op": "rotate90"}),
        json!({"op": "rotate180"}),
        json!({"op": "rotate270"}),
        json!({"op": "flip_horizontal"}),
        json!({"op": "flip_vertical"}),
        json!({"op": "rotate_canvas", "degrees": 10, "filter": "bilinear"}),
        json!({"op": "straighten", "matrix": m, "filter": "bilinear",
               "x": 0, "y": 0, "width": W, "height": H}),
        json!({"op": "perspective_crop", "quad": [5.3, 3.7, 46.1, 2.2, 49.4, 31.6, 2.8, 29.9],
               "width": 30, "height": 20, "filter": "bilinear"}),
    ]
}

/// The records of a run whose paths no open read are fractions of the
/// canvas the file stored: Image Size keeps them right, every other
/// whole-canvas edit leaves them naming other places, so it takes them out
/// of the run (the run's other resources stay) and a TIFF saved after it
/// reopens with no path, where it once read "Clip" moved.
#[test]
fn a_canvas_edit_but_image_size_takes_a_runs_unread_paths_out() {
    let dir = TempDir::new().unwrap();
    let doc = byte_canvas(true, Some(foreign_run()));
    let typed = typed_canvas(true, Some(foreign_run()));
    // Every op is checked before the verdict, so a miss names each site.
    let mut kept = Vec::new();
    for request in canvas_moves() {
        let byte = held(byte_edit(&doc, &request).metadata.iptc.as_deref());
        if byte != [1028] {
            kept.push(format!("8 bits {request}: {byte:?}"));
        }
        let wide = held(typed_edit(&typed, &request).metadata().iptc.as_deref());
        if wide != [1028] {
            kept.push(format!("16 bits {request}: {wide:?}"));
        }
    }
    assert!(kept.is_empty(), "{kept:#?}");

    let cropped = byte_edit(&doc, &canvas_moves()[0]);
    let path = dir.path().join("cropped.tif");
    let file = save_byte(&cropped, &path, TIFF);
    assert!(path_resources(&file).is_empty(), "cropped: none written");
    assert!(open_byte(&path).paths().is_empty(), "cropped: none read");

    let resize = json!({"op": "resize", "width": 30, "height": 20, "filter": "bilinear"});
    let resized = byte_edit(&doc, &resize);
    assert_eq!(
        held(resized.metadata.iptc.as_deref()),
        [1028, 2000],
        "8 bits resize"
    );
    let resized16 = typed_edit(&typed, &resize);
    assert_eq!(
        held(resized16.metadata().iptc.as_deref()),
        [1028, 2000],
        "16 bits resize"
    );
    let path = dir.path().join("resized.tif");
    let file = save_byte(&resized, &path, TIFF);
    assert_eq!(
        path_resources(&file),
        vec![(2000, "Clip".to_string(), clip_records())],
        "resized: the same fractions"
    );
    let back = open_byte(&path);
    assert_eq!(back.paths().len(), 1, "resized: Clip is read");
    assert_eq!(back.paths()[0].name, "Clip");
}

/// A flip that moves no pixel of a one-colour picture (its ruler origin at
/// the centre) changes nothing, so it is no edit; with a run holding a path
/// record it still moves that record, so it is an edit that takes it out,
/// at both depths.
#[test]
fn a_flip_that_moves_no_pixel_still_takes_a_runs_unread_paths_out() {
    let (cx, cy) = (f64::from(W) / 2.0, f64::from(H) / 2.0);
    let byte = |run| byte_canvas(false, run).set_ruler_origin(cx, cy).unwrap();
    let typed = |run| typed_canvas(false, run).set_ruler_origin(cx, cy).unwrap();
    let flip = json!({"op": "flip_horizontal"});
    let (out, error) = byte_geometry_edit(&byte(None), &flip);
    assert_eq!(error, None);
    assert!(out.is_null(), "8 bits: no run, no edit");
    let moved = byte_edit(&byte(Some(foreign_run())), &flip);
    assert_eq!(held(moved.metadata.iptc.as_deref()), [1028], "8 bits");

    let text = CString::new(flip.to_string()).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_edit(
            &typed(None),
            text.as_ptr(),
            ptr::null(),
            0,
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(err.is_null(), "{}", take_err_string(err));
    assert!(out.is_null(), "16 bits: no run, no edit");
    let moved = typed_edit(&typed(Some(foreign_run())), &flip);
    assert_eq!(held(moved.metadata().iptc.as_deref()), [1028], "16 bits");
}
