//! PSD layer comps (`psd_comps`, `psd_comp_records`) through the FFI.
//!
//! - The committed Photoshop family `layer_comps` (`provenance.json`): its
//!   five comps import as the three that record visibility, and applying HA
//!   then HB gives Photoshop's own visibilities from the take's step table
//!   (`photoshop.json`, `layer_comps[0].steps`), at 8 and 16 bits.
//! - Resource 1065 decoded and re-encoded is Photoshop's 494 bytes.
//! - A document's visibility comps round-trip through PSD at 8 and 16 bits,
//!   PSB and the layered TIFF, bound to the same layers after a rename and a
//!   reorder; this file's own walker reads 1065 and checks every record's
//!   `cmls` `LyrI` against its `lyid`.
//! - The export report names what it narrows or drops.

use std::ffi::{c_char, CStr, CString};
use std::path::Path;
use std::ptr;

use doctored_core::doc::DrDocument;
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_comps::{
    dr_doc_edit_layer_comps, dr_doc_layer_comps_json, dr_precision_doc_layer_comps_json,
};
use doctored_core::ffi_doc::*;
use doctored_core::ffi_group::dr_doc_move_layer_to;
use doctored_core::ffi_precision_conversion::dr_doc_promote_structure;
use doctored_core::ffi_precision_edit::dr_precision_doc_edit;
use doctored_core::ffi_psd::{
    dr_doc_psd_export_report, dr_doc_save_psd, dr_precision_doc_open_psd, dr_precision_doc_save_psd,
};
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::REFERENCE_WORKING_BYTES as LIMIT;
use serde_json::{json, Value};
use tempfile::TempDir;

use crate::common;
use common::psd_photoshop::{fixture as family_file, import_report, open_psd, strings, LAYERS};
use common::{add_layer, cpath, doc_from, solid, take_err_string};

const PSD: i32 = 0;
const PSB: i32 = 1;
const TIFF: i32 = 2;

// ------------------------------------------------------------ plumbing --

fn byte_open(path: &Path) -> *mut DrDocument {
    Box::into_raw(open_psd(path, &import_report(path), LAYERS))
}

fn typed_open(path: &Path) -> Box<PrecisionDocument> {
    let report = import_report(path);
    let token = CString::new(report["source_fingerprint"].as_str().unwrap()).unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    let doc = unsafe {
        dr_precision_doc_open_psd(
            cpath(path).as_ptr(),
            LAYERS,
            token.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!doc.is_null(), "typed open: {}", take_err_string(err));
    unsafe { Box::from_raw(doc) }
}

fn comps_json(doc: *const DrDocument) -> Value {
    let mut err: *mut c_char = ptr::null_mut();
    let raw = unsafe { dr_doc_layer_comps_json(doc, &mut err) };
    assert!(!raw.is_null(), "{}", take_err_string(err));
    serde_json::from_str(&take_err_string(raw)).unwrap()
}

fn typed_comps_json(doc: &PrecisionDocument) -> Value {
    let mut err: *mut c_char = ptr::null_mut();
    let raw = unsafe { dr_precision_doc_layer_comps_json(doc, &mut err) };
    assert!(!raw.is_null(), "{}", take_err_string(err));
    serde_json::from_str(&take_err_string(raw)).unwrap()
}

fn comp_edit(doc: *mut DrDocument, request: Value) -> *mut DrDocument {
    let text = CString::new(request.to_string()).unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    let out = unsafe { dr_doc_edit_layer_comps(doc, text.as_ptr(), &mut err) };
    assert!(!out.is_null(), "{request}: {}", take_err_string(err));
    unsafe { dr_doc_free(doc) };
    out
}

fn typed_edit(document: &PrecisionDocument, request: Value) -> Box<PrecisionDocument> {
    let text = CString::new(request.to_string()).unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_edit(
            document,
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

/// One comp as (name, comment, captures, {bound layer index → visible}).
type CompRow = (String, String, Value, Vec<(u64, bool)>);

/// Each comp as a [`CompRow`]. The bound index, not the entry's recorded
/// name: an entry keeps the name the layer had when the comp was saved, and a
/// reopened file names it anew.
fn comp_rows(report: &Value) -> Vec<CompRow> {
    report["comps"]
        .as_array()
        .unwrap()
        .iter()
        .map(|comp| {
            let mut entries: Vec<(u64, bool)> = comp["entries"]
                .as_array()
                .unwrap()
                .iter()
                .map(|e| {
                    assert_eq!(e["stale"], false, "{e}");
                    (
                        e["layer"].as_u64().expect("a bound entry"),
                        e["visible"].as_bool().unwrap(),
                    )
                })
                .collect();
            entries.sort();
            (
                comp["name"].as_str().unwrap().to_string(),
                comp["comment"].as_str().unwrap().to_string(),
                comp["captures"].clone(),
                entries,
            )
        })
        .collect()
}

fn byte_visibility(doc: *const DrDocument, name: &str) -> bool {
    let count = unsafe { &*doc }.layers.len();
    let index = (0..count)
        .find(|&i| common::layer_name(doc, i) == name)
        .unwrap_or_else(|| panic!("{name}"));
    unsafe { &*doc }.layers[index].visible
}

fn typed_visibility(doc: &PrecisionDocument, name: &str) -> bool {
    (0..doc.layer_count())
        .filter_map(|i| doc.layer(i))
        .find(|l| l.name() == name)
        .unwrap_or_else(|| panic!("{name}"))
        .visible()
}

fn byte_save(doc: *const DrDocument, path: &Path, container: i32) -> Vec<u8> {
    let mut err: *mut c_char = ptr::null_mut();
    let ok = unsafe {
        dr_doc_save_psd(
            doc,
            ptr::null(),
            cpath(path).as_ptr(),
            container,
            LAYERS,
            &mut err,
        )
    };
    assert!(ok, "save: {}", take_err_string(err));
    std::fs::read(path).unwrap()
}

fn typed_save(doc: &PrecisionDocument, path: &Path) -> Vec<u8> {
    let projection = doc.render(LIMIT, 0, None).unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    let ok = unsafe {
        dr_precision_doc_save_psd(
            doc,
            &projection,
            cpath(path).as_ptr(),
            PSD,
            LAYERS,
            LIMIT,
            0,
            &mut err,
        )
    };
    assert!(ok, "typed save: {}", take_err_string(err));
    std::fs::read(path).unwrap()
}

fn export_losses(doc: *const DrDocument, mode: i32) -> Vec<String> {
    let mut err: *mut c_char = ptr::null_mut();
    let json = unsafe { dr_doc_psd_export_report(doc, PSD, mode, &mut err) };
    assert!(!json.is_null(), "{}", take_err_string(err));
    let text = unsafe { CStr::from_ptr(json) }
        .to_string_lossy()
        .into_owned();
    unsafe { dr_string_free(json) };
    strings(&serde_json::from_str(&text).unwrap(), "losses")
}

// ------------------------------------------------- this file's walkers --

fn be32(bytes: &[u8], at: usize) -> usize {
    u32::from_be_bytes(bytes[at..at + 4].try_into().unwrap()) as usize
}

/// Image resource `id`'s body in a classic PSD, by Adobe's layout.
fn resource_body(bytes: &[u8], id: u16) -> Option<Vec<u8>> {
    let mut at = 26;
    at += 4 + be32(bytes, at);
    let end = at + 4 + be32(bytes, at);
    at += 4;
    while at < end {
        let rid = u16::from_be_bytes(bytes[at + 4..at + 6].try_into().unwrap());
        let name = usize::from(bytes[at + 6]);
        at += 6 + ((name + 2) & !1);
        let len = be32(bytes, at);
        if rid == id {
            return Some(bytes[at + 4..at + 4 + len].to_vec());
        }
        at += 4 + len + len % 2;
    }
    None
}

/// Every layer record's additional blocks, in FILE order, by Adobe's layout.
fn record_blocks(bytes: &[u8]) -> Vec<Vec<([u8; 4], Vec<u8>)>> {
    let mut at = 26;
    at += 4 + be32(bytes, at);
    at += 4 + be32(bytes, at);
    at += 8;
    let count = i16::from_be_bytes(bytes[at..at + 2].try_into().unwrap()).unsigned_abs();
    at += 2;
    let mut out = Vec::new();
    for _ in 0..count {
        at += 16;
        let channels = usize::from(u16::from_be_bytes(bytes[at..at + 2].try_into().unwrap()));
        at += 2 + 6 * channels + 12;
        let extra_len = be32(bytes, at);
        let extra = &bytes[at + 4..at + 4 + extra_len];
        at += 4 + extra_len;
        let mut cursor = 4 + be32(extra, 0);
        cursor += 4 + be32(extra, cursor);
        cursor += (usize::from(extra[cursor]) + 4) & !3;
        let mut blocks = Vec::new();
        while cursor + 12 <= extra.len() {
            let key: [u8; 4] = extra[cursor + 4..cursor + 8].try_into().unwrap();
            let len = be32(extra, cursor + 8);
            blocks.push((key, extra[cursor + 12..cursor + 12 + len].to_vec()));
            cursor += 12 + len + len % 2;
        }
        out.push(blocks);
    }
    out
}

/// The `LyrI` long of a `cmls` item inside a `shmd` body.
fn lyr_i(shmd: &[u8]) -> i32 {
    let at = shmd
        .windows(8)
        .position(|w| w == b"LyrIlong")
        .expect("LyrI");
    i32::from_be_bytes(shmd[at + 8..at + 12].try_into().unwrap())
}

/// Every comp name of a 1065 body: each `Nm  TEXT` unicode string.
fn names_in_1065(body: &[u8]) -> Vec<String> {
    let mut out = Vec::new();
    let mut at = 0;
    while let Some(found) = body[at..].windows(8).position(|w| w == b"Nm  TEXT") {
        let start = at + found + 8;
        let units = be32(body, start);
        let text: Vec<u16> = (0..units)
            .map(|i| u16::from_be_bytes([body[start + 4 + 2 * i], body[start + 5 + 2 * i]]))
            .filter(|&u| u != 0)
            .collect();
        out.push(String::from_utf16(&text).unwrap());
        at = start;
    }
    out
}

// ---------------------------------------------------- the authored file --

/// `photoshop.json`'s visibility after step `step`, as (name, visible).
fn step_after(step: &str) -> Vec<(String, bool)> {
    let take: Value = serde_json::from_str(
        &std::fs::read_to_string(family_file("layer_comps", "photoshop.json")).unwrap(),
    )
    .unwrap();
    take["layer_comps"][0]["steps"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["step"] == step)
        .unwrap()["after"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| {
            (
                row[0].as_str().unwrap().to_string(),
                row[1].as_bool().unwrap(),
            )
        })
        .collect()
}

#[test]
fn photoshops_comps_import_and_apply_as_photoshop_applied_them() {
    let path = family_file("layer_comps", "source.psd");
    let losses = strings(&import_report(&path), "verified_losses");
    for name in ["PosOnly", "AppOnly"] {
        assert!(
            losses.iter().any(|l| l.starts_with(&format!("Layer comp “{name}” records no layer visibility"))),
            "{name}: {losses:?}"
        );
    }
    assert!(
        losses
            .iter()
            .any(|l| l == "Layer comp “All” also records layer position and appearance, which is not imported; it is imported with its visibility."),
        "{losses:?}"
    );

    let doc = byte_open(&path);
    let names: Vec<String> = comp_rows(&comps_json(doc))
        .into_iter()
        .map(|c| c.0)
        .collect();
    assert_eq!(names, ["All", "HA", "HB"]);
    let mut doc = doc;
    for (comp, step) in [(1, "apply_ha"), (2, "apply_hb")] {
        doc = comp_edit(
            doc,
            json!({"op": "layer_comp", "action": "apply", "comp": comp}),
        );
        for (name, visible) in step_after(step) {
            assert_eq!(
                byte_visibility(doc, &name),
                visible,
                "8 bits, {step}: {name}"
            );
        }
    }
    unsafe { dr_doc_free(doc) };

    // At 16 bits: the authored file is 8-bit, which only the byte reader
    // opens, so its promotion is written by the 16-bit writer and read back
    // by the 16-bit reader, and the same applies must hold.
    let doc = byte_open(&path);
    let mut err: *mut c_char = ptr::null_mut();
    let wide = unsafe { dr_doc_promote_structure(doc, LIMIT, 0, ptr::null(), &mut err) };
    assert!(!wide.is_null(), "{}", take_err_string(err));
    unsafe { dr_doc_free(doc) };
    let wide = unsafe { Box::from_raw(wide) };
    let path16 = TempDir::new().unwrap();
    let file16 = path16.path().join("comps16.psd");
    typed_save(&wide, &file16);
    let mut typed = typed_open(&file16);
    assert_eq!(
        comp_rows(&typed_comps_json(&typed))
            .into_iter()
            .map(|c| c.0)
            .collect::<Vec<_>>(),
        ["All", "HA", "HB"]
    );
    for (comp, step) in [(1, "apply_ha"), (2, "apply_hb")] {
        typed = typed_edit(
            &typed,
            json!({"op": "layer_comp", "action": "apply", "comp": comp}),
        );
        for (name, visible) in step_after(step) {
            assert_eq!(
                typed_visibility(&typed, &name),
                visible,
                "16 bits, {step}: {name}"
            );
        }
    }
}

#[test]
fn resource_1065_decodes_and_reencodes_to_photoshops_bytes() {
    let bytes = std::fs::read(family_file("layer_comps", "source.psd")).unwrap();
    let body = resource_body(&bytes, 1065).expect("1065");
    assert_eq!(body.len(), 494);
    assert_eq!(
        doctored_core::psd_comp_records::rewritten_comps(&body).unwrap(),
        body
    );
}

// ------------------------------------------------------- the writers --

/// Bottom, A and B, then two visibility comps (one with a comment), then B
/// renamed and moved below A: the entries must still name the same layers.
fn comp_document(dir: &TempDir) -> *mut DrDocument {
    let mut doc = doc_from(dir, "bottom.png", &solid(6, 4, [10, 20, 30, 255]));
    doc = add_layer(dir, "a.png", doc, 0, &solid(6, 4, [200, 0, 0, 255]), "A");
    doc = add_layer(dir, "b.png", doc, 1, &solid(6, 4, [0, 200, 0, 255]), "B");
    doc = comp_edit(
        doc,
        json!({"op": "layer_comp", "action": "save", "name": "Both",
               "visibility": true, "position": false, "appearance": false}),
    );
    let hidden = unsafe { dr_doc_with_layer_visible(doc, 2, false) };
    unsafe { dr_doc_free(doc) };
    doc = comp_edit(
        hidden,
        json!({"op": "layer_comp", "action": "save", "name": "No B", "comment": "B is off",
               "visibility": true, "position": false, "appearance": false}),
    );
    let name = CString::new("B renamed").unwrap();
    let renamed = unsafe { dr_doc_with_layer_name(doc, 2, name.as_ptr()) };
    assert!(!renamed.is_null());
    unsafe { dr_doc_free(doc) };
    let moved = unsafe { dr_doc_move_layer_to(renamed, 2, 1, 0) };
    assert!(!moved.is_null());
    unsafe { dr_doc_free(renamed) };
    moved
}

#[test]
fn visibility_comps_round_trip_through_every_rgb_container() {
    let dir = TempDir::new().unwrap();
    let doc = comp_document(&dir);
    let want = comp_rows(&comps_json(doc));
    assert_eq!(want.len(), 2);
    for (container, name) in [(PSD, "c.psd"), (PSB, "c.psb"), (TIFF, "c.tif")] {
        let path = dir.path().join(name);
        let bytes = byte_save(doc, &path, container);
        let reopened = byte_open(&path);
        assert_eq!(comp_rows(&comps_json(reopened)), want, "{name}");
        unsafe { dr_doc_free(reopened) };
        if container == PSD {
            let body = resource_body(&bytes, 1065).expect("1065 is written");
            assert_eq!(names_in_1065(&body), ["Both", "No B"]);
            for blocks in record_blocks(&bytes) {
                let lyid = blocks.iter().find(|(k, _)| k == b"lyid").expect("lyid");
                let lyid = i32::from_be_bytes(lyid.1[..4].try_into().unwrap());
                if let Some((_, shmd)) = blocks.iter().find(|(k, _)| k == b"shmd") {
                    assert_eq!(lyr_i(shmd), lyid, "each record's LyrI is its lyid");
                }
            }
        }
    }

    // 16 bits: the promoted document writes and reopens the same comps.
    let mut err: *mut c_char = ptr::null_mut();
    let wide = unsafe { dr_doc_promote_structure(doc, LIMIT, 0, ptr::null(), &mut err) };
    assert!(!wide.is_null(), "{}", take_err_string(err));
    let wide = unsafe { Box::from_raw(wide) };
    let path = dir.path().join("c16.psd");
    typed_save(&wide, &path);
    assert_eq!(
        comp_rows(&typed_comps_json(&typed_open(&path))),
        want,
        "16 bits"
    );
    unsafe { dr_doc_free(doc) };
}

#[test]
fn the_export_report_names_what_it_narrows_and_drops() {
    let dir = TempDir::new().unwrap();
    let mut doc = doc_from(&dir, "p.png", &solid(4, 4, [1, 2, 3, 255]));
    doc = comp_edit(
        doc,
        json!({"op": "layer_comp", "action": "save", "name": "All three"}),
    );
    doc = comp_edit(
        doc,
        json!({"op": "layer_comp", "action": "save", "name": "Where",
               "visibility": false, "position": true, "appearance": false}),
    );
    let layered = export_losses(doc, LAYERS);
    assert!(
        layered.contains(&"1 layer comp records no layer visibility and is not written; this export writes a layer comp's visibility only.".to_string()),
        "{layered:?}"
    );
    assert!(
        layered.contains(&"1 layer comp is written with its visibility only; the layer position and appearance it records are not written.".to_string()),
        "{layered:?}"
    );
    let flattened = export_losses(doc, 1);
    assert!(
        flattened.contains(
            &"2 layer comps are not written; this export stores no Photoshop layer comps."
                .to_string()
        ),
        "{flattened:?}"
    );
    unsafe { dr_doc_free(doc) };
}
