//! The CLIPPING PATH (`doc_path::ClippingPath`, `psd_paths`,
//! `clipping_path_native`), through the FFI.
//!
//! Oracle: Adobe's file format document lists image resource 2999 as the
//! name of the clipping path, a Pascal string, and the `authored_exchange`
//! family's files (Photoshop 26.10.0) show what follows it: no pad, a 16.16
//! flatness (0 for none) and a u16 1. The bodies below are computed from
//! that layout; a file's own tail is carried byte for byte, so the test
//! also writes one of its own and checks it comes back.
//!
//! - A document whose saved path is the clipping path writes 2999 in a PSD,
//!   a PSB and a layered TIFF at 8 bits and a PSD, a PSB and a layered TIFF
//!   at 16, the authored no-flatness tail after the name, and each reopens
//!   with the mark on that path.
//! - Photoshop's own clipping paths (no flatness, 0.5 and 3; names of 4 and
//!   5 bytes; PSD and flat TIFF) are marked, a save writes Photoshop's body
//!   back, and a rename to a name of the other length keeps the flatness.
//! - A file's own 2999 with a tail marks its path, and a save writes the
//!   tail back; a 2999 naming no path of the file is a sentence and no mark.
//! - `.doctored` claims word 114 exactly when a path is the clipping path,
//!   at both depths, keeps the mark and the tail, and refuses the key
//!   without the word and the word without the key.
//! - The list rules, and rename, delete and crop.

use std::ffi::{c_char, CString};
use std::path::Path;
use std::ptr;

use doctored_core::doc::DrDocument;
use doctored_core::doc_path::PathEntry;
use doctored_core::ffi_doc::dr_doc_free;
use doctored_core::ffi_path::{
    dr_doc_paths_json, dr_doc_with_paths_json, dr_precision_doc_paths_json,
    dr_precision_doc_with_paths_json,
};
use doctored_core::ffi_precision_conversion::dr_doc_promote_structure;
use doctored_core::ffi_psd::dr_precision_doc_save_psd;
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::REFERENCE_WORKING_BYTES as LIMIT;
use serde_json::Value;
use tempfile::TempDir;

use crate::common::psd_photoshop::{import_report, strings, LAYERS};
use crate::common::{cpath, take_err_string};
use crate::psd_paths::{byte_open, byte_save, document, resource, typed_open};

/// The tail this file's hand-built 2999 carries after the name.
const TAIL: [u8; 6] = [0x00, 0x01, 0x00, 0x00, 0x00, 0x00];

/// The tail after the name with no flatness set: 16.16 zero, then u16 1.
pub const NO_FLATNESS: [u8; 6] = [0, 0, 0, 0, 0, 1];

/// The tail after the name for a `flatness` in device pixels (16.16).
fn flatness_tail(flatness: f64) -> Vec<u8> {
    let mut out = ((flatness * 65536.0).round() as i32).to_be_bytes().to_vec();
    out.extend_from_slice(&1u16.to_be_bytes());
    out
}

/// Resource 2999's body by the published layout: the name as a Pascal
/// string, then `tail`.
pub fn clipping_body(name: &str, tail: &[u8]) -> Vec<u8> {
    let mut out = vec![name.len() as u8];
    out.extend_from_slice(name.as_bytes());
    out.extend_from_slice(tail);
    out
}

/// A classic TIFF's tag 34377 (the 8BIM run), read from its first
/// directory by the TIFF 6.0 layout, either byte order.
pub fn tiff_run(bytes: &[u8]) -> Vec<u8> {
    let little = &bytes[..2] == b"II";
    let u16_at = |at: usize| {
        let v = [bytes[at], bytes[at + 1]];
        if little {
            u16::from_le_bytes(v)
        } else {
            u16::from_be_bytes(v)
        }
    };
    let u32_at = |at: usize| {
        let v: [u8; 4] = bytes[at..at + 4].try_into().unwrap();
        if little {
            u32::from_le_bytes(v)
        } else {
            u32::from_be_bytes(v)
        }
    };
    let ifd = u32_at(4) as usize;
    for e in 0..usize::from(u16_at(ifd)) {
        let at = ifd + 2 + e * 12;
        if u16_at(at) == 34377 {
            let count = u32_at(at + 4) as usize;
            let value = if count <= 4 {
                at + 8
            } else {
                u32_at(at + 8) as usize
            };
            return bytes[value..value + count].to_vec();
        }
    }
    Vec::new()
}

/// Every image resource of an 8BIM `run` as (id, Pascal name, body), in
/// order, by Adobe's image resource block layout.
pub fn run_resources(run: &[u8]) -> Vec<(u16, String, Vec<u8>)> {
    let mut out = Vec::new();
    let mut at = 0;
    while at < run.len() {
        assert_eq!(&run[at..at + 4], b"8BIM");
        let id = u16::from_be_bytes([run[at + 4], run[at + 5]]);
        let n = usize::from(run[at + 6]);
        let name = String::from_utf8(run[at + 7..at + 7 + n].to_vec()).unwrap();
        let size_at = at + 6 + (n + 2) / 2 * 2;
        let size = u32::from_be_bytes(run[size_at..size_at + 4].try_into().unwrap()) as usize;
        out.push((id, name, run[size_at + 4..size_at + 4 + size].to_vec()));
        at = size_at + 4 + size + size % 2;
    }
    out
}

/// Resource `id`'s body in a PSD, a PSB or a TIFF's tag 34377.
fn body_of(bytes: &[u8], id: u16) -> Option<Vec<u8>> {
    if bytes.starts_with(b"8BPS") {
        return resource(bytes, id).map(|(_, body)| body);
    }
    run_resources(&tiff_run(bytes))
        .into_iter()
        .find(|(rid, _, _)| *rid == id)
        .map(|(_, _, body)| body)
}

/// The path list JSON of `doc` with `name` made the clipping path (`tail`
/// hex), every other mark cleared.
fn list_with_clipping(json: &str, name: &str, tail: &str) -> String {
    let mut list: Value = serde_json::from_str(json).unwrap();
    for entry in list["paths"].as_array_mut().unwrap() {
        let object = entry.as_object_mut().unwrap();
        object.remove("clipping");
        if object["name"] == name {
            object.insert("clipping".into(), serde_json::json!({ "tail": tail }));
        }
    }
    list.to_string()
}

fn paths_json(doc: *const DrDocument) -> String {
    take_err_string(unsafe { dr_doc_paths_json(doc) })
}

fn typed_json(doc: &PrecisionDocument) -> String {
    take_err_string(unsafe { dr_precision_doc_paths_json(doc) })
}

/// `doc` holding the list `json`, or the core's sentence.
fn set_paths(doc: *const DrDocument, json: &str) -> Result<*mut DrDocument, String> {
    let text = CString::new(json).unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    let out = unsafe { dr_doc_with_paths_json(doc, text.as_ptr(), &mut err) };
    if out.is_null() {
        return Err(take_err_string(err));
    }
    Ok(out)
}

fn typed_set_paths(doc: &PrecisionDocument, json: &str) -> Result<Box<PrecisionDocument>, String> {
    let text = CString::new(json).unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    let out = unsafe { dr_precision_doc_with_paths_json(doc, text.as_ptr(), &mut err) };
    if out.is_null() {
        return Err(take_err_string(err));
    }
    Ok(unsafe { Box::from_raw(out) })
}

/// [`psd_paths::document`]'s paths with "Box" the clipping path.
fn clipped(dir: &TempDir, tail: &str) -> *mut DrDocument {
    let doc = document(dir);
    let marked = set_paths(doc, &list_with_clipping(&paths_json(doc), "Box", tail)).unwrap();
    unsafe { dr_doc_free(doc) };
    marked
}

/// The clipping marks of a list, as (name, tail).
fn marks(paths: &[PathEntry]) -> Vec<(String, Vec<u8>)> {
    paths
        .iter()
        .filter_map(|e| Some((e.name.clone(), e.clipping.as_ref()?.tail.clone())))
        .collect()
}

fn typed_save_in(doc: &PrecisionDocument, path: &Path, container: i32) -> Vec<u8> {
    let projection = doc.render(LIMIT, 0, None).unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    let ok = unsafe {
        dr_precision_doc_save_psd(
            doc,
            &projection,
            cpath(path).as_ptr(),
            container,
            LAYERS,
            LIMIT,
            0,
            std::ptr::null(),
            &mut err,
        )
    };
    assert!(ok, "typed save: {}", take_err_string(err));
    std::fs::read(path).unwrap()
}

#[test]
fn a_clipping_path_crosses_psd_psb_and_the_layered_tiff_as_resource_2999() {
    let dir = TempDir::new().unwrap();
    let doc = clipped(&dir, "");
    let want = vec![("Box".to_string(), Vec::new())];
    assert_eq!(marks(unsafe { &*doc }.paths()), want);
    for (container, name) in [(0, "c.psd"), (1, "c.psb"), (2, "c.tif")] {
        let path = dir.path().join(name);
        let bytes = byte_save(doc, &path, container);
        assert_eq!(
            body_of(&bytes, 2999),
            Some(clipping_body("Box", &NO_FLATNESS)),
            "{name}: resource 2999"
        );
        let back = byte_open(&path);
        assert_eq!(marks(unsafe { &*back }.paths()), want, "{name}: reopened");
        unsafe { dr_doc_free(back) };
    }
    let mut err: *mut c_char = ptr::null_mut();
    let wide = unsafe { dr_doc_promote_structure(doc, LIMIT, 0, ptr::null(), &mut err) };
    assert!(!wide.is_null(), "{}", take_err_string(err));
    let wide = unsafe { Box::from_raw(wide) };
    assert_eq!(marks(wide.paths()), want, "promoted");
    for (container, name) in [(0, "c16.psd"), (1, "c16.psb"), (2, "c16.tif")] {
        let path = dir.path().join(name);
        let bytes = typed_save_in(&wide, &path, container);
        assert_eq!(
            body_of(&bytes, 2999),
            Some(clipping_body("Box", &NO_FLATNESS)),
            "{name}: resource 2999"
        );
        assert_eq!(marks(typed_open(&path).paths()), want, "{name}: reopened");
    }
    unsafe { dr_doc_free(doc) };
}

/// `psd` (a classic PSD) with `block` appended to its image resources.
fn with_resource(psd: &[u8], block: &[u8]) -> Vec<u8> {
    let be32 = |at: usize| u32::from_be_bytes(psd[at..at + 4].try_into().unwrap()) as usize;
    let section = 26 + 4 + be32(26);
    let length = be32(section);
    let end = section + 4 + length;
    let mut out = psd[..section].to_vec();
    out.extend_from_slice(&((length + block.len()) as u32).to_be_bytes());
    out.extend_from_slice(&psd[section + 4..end]);
    out.extend_from_slice(block);
    out.extend_from_slice(&psd[end..]);
    out
}

/// An unnamed image resource block: `8BIM`, the id, an empty Pascal name,
/// the body padded even.
fn block(id: u16, body: &[u8]) -> Vec<u8> {
    let mut out = b"8BIM".to_vec();
    out.extend_from_slice(&id.to_be_bytes());
    out.extend_from_slice(&[0, 0]);
    out.extend_from_slice(&(body.len() as u32).to_be_bytes());
    out.extend_from_slice(body);
    if body.len() % 2 == 1 {
        out.push(0);
    }
    out
}

#[test]
fn photoshops_clipping_paths_cross_with_their_flatness() {
    use crate::common::psd_photoshop::fixture;
    let dir = TempDir::new().unwrap();
    for (name, path_name, flatness) in [
        ("clip_a.psd", "Clip", 0.0),
        ("clip_b.psd", "Clip5", 0.5),
        ("clip_c.tif", "Clip", 3.0),
    ] {
        let source = fixture("authored_exchange", name);
        let bytes = std::fs::read(&source).unwrap();
        let tail = flatness_tail(flatness);
        let authored = clipping_body(path_name, &tail);
        assert_eq!(
            body_of(&bytes, 2999),
            Some(authored.clone()),
            "{name}: the layout"
        );
        // Read through the flat TIFF's own open or the PSD's.
        let doc = if name.ends_with(".tif") {
            let mut err: *mut c_char = ptr::null_mut();
            let doc = unsafe {
                doctored_core::ffi_doc::dr_doc_open(cpath(&source).as_ptr(), ptr::null(), &mut err)
            };
            assert!(!doc.is_null(), "{}", take_err_string(err));
            doc
        } else {
            byte_open(&source)
        };
        // No flatness is kept as a mark made here is: no tail.
        let kept = if flatness == 0.0 {
            Vec::new()
        } else {
            tail.clone()
        };
        assert_eq!(
            marks(unsafe { &*doc }.paths()),
            vec![(path_name.to_string(), kept)],
            "{name}: marked"
        );
        let again = byte_save(doc, &dir.path().join(format!("{name}.psd")), 0);
        assert_eq!(
            body_of(&again, 2999),
            Some(authored),
            "{name}: written back"
        );
        // A rename to a name of the other length keeps the flatness after it.
        let renamed_name = if path_name.len() == 4 {
            "Clip55"
        } else {
            "Clp"
        };
        let json =
            paths_json(doc).replace(&format!("\"{path_name}\""), &format!("\"{renamed_name}\""));
        let renamed = set_paths(doc, &json).unwrap();
        let bytes = byte_save(renamed, &dir.path().join(format!("{name}-renamed.psd")), 0);
        assert_eq!(
            body_of(&bytes, 2999),
            Some(clipping_body(renamed_name, &tail)),
            "{name}: renamed"
        );
        unsafe {
            dr_doc_free(renamed);
            dr_doc_free(doc);
        }
    }
}

#[test]
fn a_files_clipping_path_keeps_its_tail_and_one_naming_no_path_is_a_sentence() {
    let dir = TempDir::new().unwrap();
    let doc = document(&dir);
    let plain = byte_save(doc, &dir.path().join("plain.psd"), 0);
    unsafe { dr_doc_free(doc) };
    assert_eq!(resource(&plain, 2999), None, "no clipping path, no 2999");
    let path = dir.path().join("tail.psd");
    let body = clipping_body("Curve", &TAIL);
    std::fs::write(&path, with_resource(&plain, &block(2999, &body))).unwrap();
    let losses = strings(&import_report(&path), "verified_losses");
    assert!(
        !losses.iter().any(|l| l.contains("clipping path")),
        "{losses:?}"
    );
    let back = byte_open(&path);
    assert_eq!(
        marks(unsafe { &*back }.paths()),
        vec![("Curve".to_string(), TAIL.to_vec())]
    );
    let again = byte_save(back, &dir.path().join("again.psd"), 0);
    assert_eq!(resource(&again, 2999).map(|(_, b)| b), Some(body.clone()));
    // 16 bits: the same resource on a 16-bit file is read and written back.
    let mut err: *mut c_char = ptr::null_mut();
    let wide = unsafe { dr_doc_promote_structure(back, LIMIT, 0, ptr::null(), &mut err) };
    assert!(!wide.is_null(), "{}", take_err_string(err));
    let wide = unsafe { Box::from_raw(wide) };
    let plain16 = {
        let unmarked =
            typed_set_paths(&wide, &list_with_clipping(&typed_json(&wide), "", "")).unwrap();
        typed_save_in(&unmarked, &dir.path().join("plain16.psd"), 0)
    };
    assert_eq!(resource(&plain16, 2999), None, "16 bits: no mark, no 2999");
    let path16 = dir.path().join("tail16.psd");
    std::fs::write(&path16, with_resource(&plain16, &block(2999, &body))).unwrap();
    let back16 = typed_open(&path16);
    assert_eq!(
        marks(back16.paths()),
        vec![("Curve".to_string(), TAIL.to_vec())]
    );
    let again16 = typed_save_in(&back16, &dir.path().join("again16.psd"), 0);
    assert_eq!(resource(&again16, 2999).map(|(_, b)| b), Some(body.clone()));
    unsafe { dr_doc_free(back) };

    let path = dir.path().join("missing.psd");
    let body = clipping_body("Missing", &[]);
    std::fs::write(&path, with_resource(&plain, &block(2999, &body))).unwrap();
    let losses = strings(&import_report(&path), "verified_losses");
    assert!(
        losses.contains(&"The clipping path names “Missing”, which is not one of the file's imported paths; no clipping path is set.".to_string()),
        "{losses:?}"
    );
    let back = byte_open(&path);
    assert!(marks(unsafe { &*back }.paths()).is_empty());
    unsafe { dr_doc_free(back) };
}

#[test]
fn the_native_file_claims_word_114_exactly_when_a_path_is_the_clipping_path() {
    let dir = TempDir::new().unwrap();
    let tail = "0001000000ff";
    let doc = clipped(&dir, tail);
    let want = vec![("Box".to_string(), vec![0, 1, 0, 0, 0, 0xff])];
    let path = dir.path().join("c.doctored");
    unsafe { &*doc }
        .save_native(path.to_str().unwrap())
        .unwrap();
    let bytes = std::fs::read(&path).unwrap();
    assert_eq!(&bytes[4..8], &45u32.to_le_bytes());
    assert_eq!(
        &bytes[8..12],
        &114u32.to_le_bytes(),
        "word 114 first inside 45"
    );
    let back = DrDocument::open(path.to_str().unwrap()).unwrap();
    assert_eq!(marks(back.paths()), want, "byte reopen");
    // The key without the word, and the word without the key.
    let mut unclaimed = bytes.clone();
    unclaimed.drain(8..12);
    let refused = dir.path().join("unclaimed.doctored");
    std::fs::write(&refused, &unclaimed).unwrap();
    let error = DrDocument::open(refused.to_str().unwrap()).err().unwrap();
    assert!(
        error.contains("native clipping path requires DRDC 114"),
        "{error}"
    );
    let plain = document(&dir);
    let plain_path = dir.path().join("plain.doctored");
    unsafe { &*plain }
        .save_native(plain_path.to_str().unwrap())
        .unwrap();
    let mut claimed = std::fs::read(&plain_path).unwrap();
    assert_ne!(
        &claimed[8..12],
        &114u32.to_le_bytes(),
        "no word without a mark"
    );
    claimed.splice(8..8, 114u32.to_le_bytes());
    std::fs::write(&refused, &claimed).unwrap();
    let error = DrDocument::open(refused.to_str().unwrap()).err().unwrap();
    assert!(
        error.contains("invalid native clipping path word"),
        "{error}"
    );
    unsafe { dr_doc_free(plain) };

    // The typed file: the word right after the CMYK slot (none here).
    let mut err: *mut c_char = ptr::null_mut();
    let wide = unsafe { dr_doc_promote_structure(doc, LIMIT, 0, ptr::null(), &mut err) };
    assert!(!wide.is_null(), "{}", take_err_string(err));
    let wide = unsafe { Box::from_raw(wide) };
    let bytes = wide.encode_native(LIMIT, 0, None).unwrap();
    assert_eq!(&bytes[4..8], &46u32.to_le_bytes());
    assert_eq!(&bytes[8..12], &114u32.to_le_bytes(), "typed word 114");
    let back = PrecisionDocument::parse_native(&bytes, LIMIT, 0, None).unwrap();
    assert_eq!(marks(back.paths()), want, "typed reopen");
    let mut unclaimed = bytes.clone();
    unclaimed.drain(8..12);
    let error = PrecisionDocument::parse_native(&unclaimed, LIMIT, 0, None)
        .err()
        .unwrap();
    assert!(
        error.contains("native clipping path requires DRDC 114"),
        "{error}"
    );
    unsafe { dr_doc_free(doc) };
}

#[test]
fn the_list_rules_hold_one_saved_clipping_path_that_follows_its_path() {
    let dir = TempDir::new().unwrap();
    let doc = clipped(&dir, "");
    let json = paths_json(doc);
    let mut list: Value = serde_json::from_str(&json).unwrap();
    let entries = list["paths"].as_array_mut().unwrap();
    // Two clipping paths.
    let mut two = entries.clone();
    two[1]["clipping"] = serde_json::json!({ "tail": "" });
    let error = set_paths(doc, &serde_json::json!({ "paths": two }).to_string())
        .err()
        .unwrap();
    assert!(
        error.contains("a document holds at most one clipping path"),
        "{error}"
    );
    // The work path.
    let mut work = entries.clone();
    work[2].as_object_mut().unwrap().remove("clipping");
    work[0]["clipping"] = serde_json::json!({});
    let error = set_paths(doc, &serde_json::json!({ "paths": work }).to_string())
        .err()
        .unwrap();
    assert!(
        error.contains("the work path cannot be the clipping path"),
        "{error}"
    );
    // A tail past 64 bytes.
    let mut long = entries.clone();
    long[2]["clipping"] = serde_json::json!({ "tail": "00".repeat(65) });
    let error = set_paths(doc, &serde_json::json!({ "paths": long }).to_string())
        .err()
        .unwrap();
    assert!(error.contains("longer than 64 bytes"), "{error}");
    // The typed twin answers the same sentence.
    let mut err: *mut c_char = ptr::null_mut();
    let wide = unsafe { dr_doc_promote_structure(doc, LIMIT, 0, ptr::null(), &mut err) };
    let wide = unsafe { Box::from_raw(wide) };
    let error = typed_set_paths(&wide, &serde_json::json!({ "paths": two }).to_string())
        .err()
        .unwrap();
    assert!(
        error.contains("a document holds at most one clipping path"),
        "{error}"
    );
    // A rename keeps the mark; a crop keeps it; a delete removes it.
    let mut renamed = entries.clone();
    renamed[2]["name"] = "Frame".into();
    let renamed = set_paths(doc, &serde_json::json!({ "paths": renamed }).to_string()).unwrap();
    assert_eq!(
        marks(unsafe { &*renamed }.paths()),
        vec![("Frame".to_string(), Vec::new())]
    );
    let cropped = unsafe { &*renamed }.crop(4, 4, 30, 20).unwrap();
    assert_eq!(
        marks(cropped.paths()),
        vec![("Frame".to_string(), Vec::new())]
    );
    let mut deleted = entries.clone();
    deleted.remove(2);
    let deleted = set_paths(doc, &serde_json::json!({ "paths": deleted }).to_string()).unwrap();
    assert!(marks(unsafe { &*deleted }.paths()).is_empty());
    let listed: Value = serde_json::from_str(&paths_json(deleted)).unwrap();
    assert!(listed["paths"]
        .as_array()
        .unwrap()
        .iter()
        .all(|e| e.get("clipping").is_none()));
    unsafe {
        dr_doc_free(renamed);
        dr_doc_free(deleted);
        dr_doc_free(doc);
    }
}
