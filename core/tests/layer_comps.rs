//! Phase 12D-1: LAYER COMPS at both depths.
//!
//! What is pinned here, all of it against references this file builds itself:
//! the four ops and the purity rule; the name, count and capture refusals;
//! the binding rule (stale after a delete, still bound after a rename, still
//! bound after a rename AND a reopen); the three capture-flag subsets
//! applying only their own fields; the lock rule (`LOCK_POSITION` reported
//! and the offset skipped while visibility and appearance still land); the
//! `.rz` wrapper at both depths — 43 / 44 as the OUTERMOST version word and
//! one `LCMP` table whose bytes this file assembles independently, closing
//! the file after `LLBL` — the comp-less document's bytes being exactly what
//! they were, and the refusals a crafted table earns.

use std::ffi::{c_char, CString};
use std::path::Path;
use std::ptr;
use std::sync::Arc;

use doctored_core::doc::DrDocument;
use doctored_core::doc_comps::{CompCaptures, CompRequest, MAX_LAYER_COMPS};
use doctored_core::ffi_comps::{
    dr_doc_edit_layer_comps, dr_doc_layer_comps_json, dr_precision_doc_layer_comps_json,
};
use doctored_core::ffi_doc::*;
use doctored_core::ffi_group::dr_doc_with_layer_locks;
use doctored_core::ffi_identity::dr_doc_layer_id;
use doctored_core::ffi_precision::dr_precision_doc_free;
use doctored_core::ffi_precision_edit::dr_precision_doc_edit;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer as Layer};
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use image::{Rgba, RgbaImage};
use serde_json::{json, Value};
use tempfile::TempDir;

use crate::common;
use common::{
    add_layer, cpath, doc_from, opaque_pattern, replace_comp_id, take_err_string, test_pattern,
};

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
/// `DR_LOCK_POSITION`, restated here rather than read out of the core.
const LOCK_POSITION: u32 = 4;
/// `DrBlendMode`'s Multiply, restated here for the same reason.
const MULTIPLY: i32 = 1;

// ------------------------------------------------- independent references --

/// The `LCMP` table this build must write, assembled here from the payload
/// alone: the tag, u32 schema 2, u32 length, then the bytes. Nothing in this
/// function reads `comps_native`.
fn expected_table(payload: &str) -> Vec<u8> {
    table_with_schema(2, payload)
}

/// The same, for the SCHEMA-1 files this build no longer writes but must go
/// on reading exactly as it always did.
fn table_with_schema(schema: u32, payload: &str) -> Vec<u8> {
    let mut out = b"LCMP".to_vec();
    out.extend(schema.to_le_bytes());
    out.extend((payload.len() as u32).to_le_bytes());
    out.extend(payload.as_bytes());
    out
}

fn word(bytes: &[u8], index: usize) -> u32 {
    let at = 4 + index * 4;
    u32::from_le_bytes(bytes[at..at + 4].try_into().expect("4 bytes"))
}

/// Phase 12E-1's always-written identity wrapper: 45/46 is now the OUTERMOST
/// word and its `LIDS` table closes every file, so the comp word sits one
/// slot in and the `LCMP` table is found by its tag.
const IDENTITY_BYTE: u32 = 45;
const IDENTITY_TYPED: u32 = 46;

fn lcmp_at(bytes: &[u8]) -> usize {
    bytes.windows(4).rposition(|w| w == b"LCMP").expect("LCMP")
}

/// `bytes` with its `LCMP` table replaced by `table`, keeping everything
/// after it (the `LIDS` tail) exactly as it was.
fn with_table(bytes: &[u8], old: &[u8], table: &[u8]) -> Vec<u8> {
    let at = lcmp_at(bytes);
    let mut out = bytes[..at].to_vec();
    out.extend_from_slice(table);
    out.extend_from_slice(&bytes[at + old.len()..]);
    out
}

/// The `LCMP` payload a file carries, as text — found by this file's own
/// scan for the tag rather than by asking the core where it put it.
fn payload_of(bytes: &[u8]) -> String {
    let at = bytes
        .windows(4)
        .position(|w| w == b"LCMP")
        .expect("an LCMP table");
    let length = u32::from_le_bytes(bytes[at + 8..at + 12].try_into().expect("4 bytes")) as usize;
    String::from_utf8(bytes[at + 12..at + 12 + length].to_vec()).expect("utf-8 payload")
}

// ---------------------------------------------------------- byte plumbing --

/// Three raster layers: "Base", "Middle", "Top", bottom first.
fn fixture(dir: &TempDir) -> *mut DrDocument {
    let doc = doc_from(dir, "base.png", &opaque_pattern(4, 3));
    let doc = add_layer(dir, "middle.png", doc, 0, &test_pattern(4, 3), "Middle");
    add_layer(dir, "top.png", doc, 1, &test_pattern(4, 3), "Top")
}

/// One comp request, or the refusal it earned. `Ok(None)` is the exact no-op.
fn edit(doc: *const DrDocument, request: &Value) -> Result<Option<*mut DrDocument>, String> {
    let c = CString::new(request.to_string()).unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    let out = unsafe { dr_doc_edit_layer_comps(doc, c.as_ptr(), &mut err) };
    if out.is_null() {
        if err.is_null() {
            return Ok(None);
        }
        return Err(take_err_string(err));
    }
    Ok(Some(out))
}

/// A comp request that must succeed; frees the old handle.
fn comp(doc: *mut DrDocument, request: Value) -> *mut DrDocument {
    match edit(doc, &request) {
        Ok(Some(out)) => {
            unsafe { dr_doc_free(doc) };
            out
        }
        Ok(None) => panic!("{request} reported no change"),
        Err(message) => panic!("{request}: {message}"),
    }
}

fn refusal(doc: *const DrDocument, request: Value) -> String {
    match edit(doc, &request) {
        Err(message) => message,
        Ok(Some(out)) => {
            unsafe { dr_doc_free(out) };
            panic!("{request} must be refused")
        }
        Ok(None) => panic!("{request} must be refused, not answered as a no-op"),
    }
}

fn save_request(name: &str) -> Value {
    json!({"op": "layer_comp", "action": "save", "name": name})
}

fn apply_request(index: usize) -> Value {
    json!({"op": "layer_comp", "action": "apply", "comp": index})
}

fn comps(doc: *const DrDocument) -> Value {
    let mut err: *mut c_char = ptr::null_mut();
    let json = unsafe { dr_doc_layer_comps_json(doc, &mut err) };
    assert!(!json.is_null(), "{}", take_err_string(err));
    serde_json::from_str(&take_err_string(json)).expect("comp JSON")
}

fn typed_comps(document: &PrecisionDocument) -> Value {
    let mut err: *mut c_char = ptr::null_mut();
    let json = unsafe { dr_precision_doc_layer_comps_json(document, &mut err) };
    assert!(!json.is_null(), "{}", take_err_string(err));
    serde_json::from_str(&take_err_string(json)).expect("comp JSON")
}

/// The document's own visibility / offset / opacity table, read back through
/// the FFI — the state a comp is measured against.
fn state(doc: *const DrDocument) -> Vec<(bool, i32, i32, f32)> {
    (0..unsafe { dr_doc_layer_count(doc) })
        .map(|i| unsafe {
            (
                dr_doc_layer_visible(doc, i),
                dr_doc_layer_offset_x(doc, i),
                dr_doc_layer_offset_y(doc, i),
                dr_doc_layer_opacity(doc, i),
            )
        })
        .collect()
}

fn save(doc: *const DrDocument, path: &Path) -> Vec<u8> {
    let c = cpath(path);
    let mut err: *mut c_char = ptr::null_mut();
    let ok = unsafe { dr_doc_save_native(doc, c.as_ptr(), &mut err) };
    assert!(ok, "{}", take_err_string(err));
    std::fs::read(path).expect("the saved file")
}

fn open(path: &Path) -> Result<*mut DrDocument, String> {
    let c = cpath(path);
    let mut err: *mut c_char = ptr::null_mut();
    let out = unsafe { dr_doc_open(c.as_ptr(), &mut err) };
    if out.is_null() {
        return Err(take_err_string(err));
    }
    Ok(out)
}

fn open_ok(path: &Path) -> *mut DrDocument {
    open(path).expect("the document reopens")
}

fn open_err(path: &Path, what: &str) -> String {
    match open(path) {
        Err(message) => message,
        Ok(doc) => {
            unsafe { dr_doc_free(doc) };
            panic!("{what} must refuse the open")
        }
    }
}

// ------------------------------------------------------- the four byte ops --

#[test]
fn save_apply_update_and_delete_round_trip_with_the_no_op_rule() {
    let dir = TempDir::new().unwrap();
    let doc = fixture(&dir);

    // State A: everything visible, unmoved, opaque.
    let doc = comp(doc, save_request("A"));
    let listing = comps(doc);
    assert_eq!(listing["count"], 1);
    assert_eq!(listing["comps"][0]["name"], "A");
    assert_eq!(listing["comps"][0]["index"], 0);
    assert_eq!(listing["comps"][0]["stale_count"], 0);
    assert_eq!(listing["comps"][0]["locked_count"], 0);
    assert_eq!(
        listing["comps"][0]["entries"].as_array().unwrap().len(),
        3,
        "a comp records EVERY layer"
    );
    let a_state = state(doc);

    // Applying the comp that was just saved changes nothing — the purity
    // rule, reported as NULL with no error.
    assert!(
        edit(doc, &apply_request(0)).expect("admitted").is_none(),
        "applying the state the document is already in is an exact no-op"
    );

    // State B: hide the top layer, move the middle one, halve its opacity.
    let doc = unsafe { must(dr_doc_with_layer_visible(doc, 2, false), doc) };
    let doc = unsafe { must(dr_doc_with_layer_offset(doc, 1, 40, 30), doc) };
    let doc = unsafe { must(dr_doc_with_layer_opacity(doc, 1, 0.5), doc) };
    let b_state = state(doc);
    assert_ne!(a_state, b_state);
    let doc = comp(doc, save_request("B"));
    assert_eq!(comps(doc)["count"], 2);

    // A restores A exactly; B restores B exactly.
    let back = comp(doc, apply_request(0));
    assert_eq!(state(back), a_state, "comp A restores state A");
    let forward = comp(back, apply_request(1));
    assert_eq!(state(forward), b_state, "comp B restores state B");

    // Update renames without touching the recording.
    let renamed = comp(
        forward,
        json!({"op": "layer_comp", "action": "update", "comp": 0,
               "name": "A prime", "recapture": false}),
    );
    let listing = comps(renamed);
    assert_eq!(listing["comps"][0]["name"], "A prime");
    assert_eq!(listing["comps"][1]["name"], "B");
    // …and the same update again is an exact no-op.
    assert!(
        edit(
            renamed,
            &json!({"op": "layer_comp", "action": "update", "comp": 0,
                    "name": "A prime", "recapture": false})
        )
        .expect("admitted")
        .is_none(),
        "an update that changes nothing is an exact no-op"
    );
    // Applying A prime still restores A: a rename does not touch the table.
    let restored = comp(renamed, apply_request(0));
    assert_eq!(state(restored), a_state);

    // Delete removes exactly that comp.
    let deleted = comp(
        restored,
        json!({"op": "layer_comp", "action": "delete", "comp": 0}),
    );
    let listing = comps(deleted);
    assert_eq!(listing["count"], 1);
    assert_eq!(listing["comps"][0]["name"], "B");
    assert_eq!(listing["comps"][0]["index"], 0);
    unsafe { dr_doc_free(deleted) };
}

/// `apply(op(doc))`, asserting the op succeeded and freeing the old handle.
unsafe fn must(out: *mut DrDocument, old: *mut DrDocument) -> *mut DrDocument {
    assert!(!out.is_null(), "the property setter refused");
    unsafe { dr_doc_free(old) };
    out
}

#[test]
fn the_refusals_are_named() {
    let dir = TempDir::new().unwrap();
    let mut doc = fixture(&dir);

    doc = comp(doc, save_request("A"));
    assert!(
        refusal(doc, save_request("A")).contains("already exists"),
        "a duplicate name is refused by name"
    );
    assert!(refusal(doc, save_request("   ")).contains("needs a name"));
    assert!(
        refusal(doc, save_request(&"x".repeat(129))).contains("128"),
        "an oversize name names the limit"
    );
    assert!(
        refusal(
            doc,
            json!({"op": "layer_comp", "action": "save", "name": "Q",
                   "comment": "c".repeat(1025)})
        )
        .contains("1024"),
        "an oversize comment names the limit"
    );
    assert!(
        refusal(
            doc,
            json!({"op": "layer_comp", "action": "save", "name": "Q",
                   "visibility": false, "position": false, "appearance": false})
        )
        .contains("at least one attribute"),
        "a comp that captures nothing is refused"
    );
    assert!(refusal(doc, apply_request(9)).contains("out of range"));
    assert!(refusal(
        doc,
        json!({"op": "layer_comp", "action": "delete", "comp": 9})
    )
    .contains("out of range"));
    assert!(refusal(
        doc,
        json!({"op": "layer_comp", "action": "wiggle", "comp": 0})
    )
    .contains("wiggle"));
    assert!(
        refusal(doc, json!({"op": "layer_comp", "action": "apply"})).contains("comp index"),
        "an apply with no comp index is refused by name"
    );
    assert!(refusal(
        doc,
        json!({"op": "layer_label", "action": "apply", "comp": 0})
    )
    .contains("layer_comp"));

    // The 65th comp is refused; the 64th is not.
    for i in 1..MAX_LAYER_COMPS {
        doc = comp(doc, save_request(&format!("C{i}")));
    }
    assert_eq!(comps(doc)["count"], MAX_LAYER_COMPS);
    assert!(
        refusal(doc, save_request("one too many")).contains(&MAX_LAYER_COMPS.to_string()),
        "the 65th comp names the limit"
    );

    // NULL tolerance on both exports.
    assert!(unsafe { dr_doc_layer_comps_json(ptr::null(), ptr::null_mut()) }.is_null());
    assert!(
        unsafe { dr_doc_edit_layer_comps(ptr::null(), ptr::null(), ptr::null_mut()) }.is_null()
    );
    assert!(unsafe { dr_doc_edit_layer_comps(doc, ptr::null(), ptr::null_mut()) }.is_null());
    unsafe { dr_doc_free(doc) };
}

#[test]
fn an_entry_goes_stale_when_its_layer_is_deleted_and_survives_a_rename() {
    let dir = TempDir::new().unwrap();
    let doc = fixture(&dir);
    let doc = comp(doc, save_request("A"));

    // A RENAME keeps the entry bound — the id carries through it — and the
    // recorded name is refreshed at the next write, not here.
    let name = CString::new("Middle renamed").unwrap();
    let renamed = unsafe { must(dr_doc_with_layer_name(doc, 1, name.as_ptr()), doc) };
    let listing = comps(renamed);
    let entries = listing["comps"][0]["entries"].as_array().unwrap();
    assert!(
        entries.iter().all(|e| e["stale"] == json!(false)),
        "a rename leaves every entry bound: {entries:?}"
    );
    assert_eq!(entries[1]["layer"], json!(1), "still bound to record 1");

    // A DELETE makes exactly that entry stale, reported under its recorded
    // name, and the surviving entries renumber.
    let deleted = unsafe { must(dr_doc_removing_layer(renamed, 1), renamed) };
    let listing = comps(deleted);
    assert_eq!(listing["comps"][0]["stale_count"], 1);
    let entries = listing["comps"][0]["entries"].as_array().unwrap();
    assert_eq!(entries[0]["layer"], json!(0));
    assert_eq!(entries[1]["layer"], Value::Null, "the deleted entry");
    assert_eq!(entries[1]["stale"], json!(true));
    assert_eq!(entries[1]["name"], "Middle", "reported under its old name");
    assert_eq!(entries[2]["layer"], json!(1), "the top layer renumbered");

    // Applying it still restores every entry it CAN.
    let moved = unsafe { must(dr_doc_with_layer_visible(deleted, 1, false), deleted) };
    let applied = comp(moved, apply_request(0));
    assert!(
        unsafe { dr_doc_layer_visible(applied, 1) },
        "the bound entry still applies around the stale one"
    );
    unsafe { dr_doc_free(applied) };
}

#[test]
fn each_capture_flag_applies_only_its_own_fields() {
    let dir = TempDir::new().unwrap();
    let base = fixture(&dir);

    for (flag, expect_visible, expect_offset, expect_opacity) in [
        ("visibility", true, false, false),
        ("position", false, true, false),
        ("appearance", false, false, true),
    ] {
        let mut request = json!({"op": "layer_comp", "action": "save", "name": "Only"});
        for key in ["visibility", "position", "appearance"] {
            request[key] = json!(key == flag);
        }
        let doc = unsafe { dr_doc_clone(base) };
        let doc = comp(doc, request);
        let before = state(doc);

        // Perturb all three on the middle layer, then apply the comp.
        let doc = unsafe { must(dr_doc_with_layer_visible(doc, 1, false), doc) };
        let doc = unsafe { must(dr_doc_with_layer_offset(doc, 1, 7, 9), doc) };
        let doc = unsafe { must(dr_doc_with_layer_opacity(doc, 1, 0.25), doc) };
        let doc = comp(doc, apply_request(0));
        let after = state(doc);

        assert_eq!(after[1].0 == before[1].0, expect_visible, "{flag}: visible");
        assert_eq!(
            (after[1].1, after[1].2) == (before[1].1, before[1].2),
            expect_offset,
            "{flag}: offset"
        );
        assert_eq!(after[1].3 == before[1].3, expect_opacity, "{flag}: opacity");

        // Only the captured key is present in the report.
        let entry = &comps(doc)["comps"][0]["entries"][1];
        assert_eq!(entry.get("visible").is_some(), expect_visible);
        assert_eq!(entry.get("offset").is_some(), expect_offset);
        assert_eq!(entry.get("opacity").is_some(), expect_opacity);
        assert_eq!(entry.get("blend").is_some(), expect_opacity);
        unsafe { dr_doc_free(doc) };
    }
    unsafe { dr_doc_free(base) };
}

#[test]
fn a_position_locked_entry_is_reported_and_its_offset_skipped() {
    let dir = TempDir::new().unwrap();
    let doc = fixture(&dir);
    // Record a comp with the middle layer moved, hidden and faded.
    let doc = unsafe { must(dr_doc_with_layer_offset(doc, 1, 40, 30), doc) };
    let doc = unsafe { must(dr_doc_with_layer_visible(doc, 1, false), doc) };
    let doc = unsafe { must(dr_doc_with_layer_opacity(doc, 1, 0.5), doc) };
    let doc = comp(doc, save_request("Moved"));

    // Put the layer back and LOCK its position.
    let doc = unsafe { must(dr_doc_with_layer_offset(doc, 1, 0, 0), doc) };
    let doc = unsafe { must(dr_doc_with_layer_visible(doc, 1, true), doc) };
    let doc = unsafe { must(dr_doc_with_layer_opacity(doc, 1, 1.0), doc) };
    let doc = unsafe { must(dr_doc_with_layer_locks(doc, 1, LOCK_POSITION), doc) };

    let listing = comps(doc);
    assert_eq!(listing["comps"][0]["locked_count"], 1);
    let entries = listing["comps"][0]["entries"].as_array().unwrap();
    assert_eq!(entries[1]["locked"], json!(true), "the locked entry");
    assert_eq!(entries[0]["locked"], json!(false));
    assert_eq!(entries[2]["locked"], json!(false));

    let applied = comp(doc, apply_request(0));
    assert_eq!(
        (unsafe { dr_doc_layer_offset_x(applied, 1) }, unsafe {
            dr_doc_layer_offset_y(applied, 1)
        }),
        (0, 0),
        "the lock skipped the move"
    );
    assert!(
        !unsafe { dr_doc_layer_visible(applied, 1) },
        "visibility is lock-exempt and still applied"
    );
    assert_eq!(
        unsafe { dr_doc_layer_opacity(applied, 1) },
        0.5,
        "appearance is lock-exempt and still applied"
    );
    unsafe { dr_doc_free(applied) };
}

// ----------------------------------------------------- the byte .rz table --

#[test]
fn a_comp_less_byte_document_writes_exactly_the_prior_writers_bytes() {
    let dir = TempDir::new().unwrap();
    let doc = fixture(&dir);
    let plain = save(doc, &dir.path().join("plain.rz"));
    assert_ne!(word(&plain, 0), 43, "no comp wrapper is written");
    assert!(
        !plain.windows(4).any(|w| w == b"LCMP"),
        "a comp-less document carries no table"
    );

    // Saving a comp and deleting it again reproduces those exact bytes.
    let doc = comp(doc, save_request("A"));
    let doc = comp(
        doc,
        json!({"op": "layer_comp", "action": "delete", "comp": 0}),
    );
    let again = save(doc, &dir.path().join("again.rz"));
    assert_eq!(again, plain, "deleting the last comp restores the bytes");
    unsafe { dr_doc_free(doc) };
}

#[test]
fn a_byte_document_round_trips_through_an_independently_assembled_table() {
    let dir = TempDir::new().unwrap();
    let doc = fixture(&dir);
    // A fixture with values this file chose, all exact in binary.
    let doc = unsafe { must(dr_doc_with_layer_visible(doc, 2, false), doc) };
    let doc = unsafe { must(dr_doc_with_layer_offset(doc, 1, 40, 30), doc) };
    let doc = unsafe { must(dr_doc_with_layer_opacity(doc, 1, 0.5), doc) };
    let doc = unsafe { must(dr_doc_with_layer_blend_mode(doc, 1, MULTIPLY), doc) };
    let doc = comp(
        doc,
        json!({"op": "layer_comp", "action": "save", "name": "Hero",
               "comment": "the one with the sky"}),
    );
    let path = dir.path().join("comps.rz");
    let bytes = save(doc, &path);

    assert_eq!(&bytes[..4], b"DRDC");
    assert_eq!(
        word(&bytes, 0),
        IDENTITY_BYTE,
        "the identity word is outermost"
    );
    // The fixture MOVED "Middle" to (40, 30), so its reference point is
    // written: word 91 first inside 45 (`reference_native`), then 43.
    assert_eq!(word(&bytes, 1), 91, "the moved layer's reference word");
    assert_eq!(word(&bytes, 2), 43, "43 is the next word inside it");

    // The payload this file expects, written out by hand — SCHEMA 2, so an
    // entry addresses its layer by the PERSISTED id and carries no "layer"
    // key at all. The ids are minted at run time, so they are read back
    // through the FFI rather than spelled as literals; nothing else here
    // comes from the core.
    let id = |i: usize| unsafe { dr_doc_layer_id(doc, i) };
    let expected = format!(
        concat!(
            r#"{{"schema":2,"comps":[{{"name":"Hero","comment":"the one with the sky","#,
            r#""captures":{{"visibility":true,"position":true,"appearance":true}},"entries":["#,
            r#"{{"id":{0},"name":"Background","visible":true,"offset":[0,0],"opacity":1.0,"blend":0}},"#,
            r#"{{"id":{1},"name":"Middle","visible":true,"offset":[40,30],"opacity":0.5,"blend":1}},"#,
            r#"{{"id":{2},"name":"Top","visible":false,"offset":[0,0],"opacity":1.0,"blend":0}}"#,
            r#"]}}]}}"#,
        ),
        id(0),
        id(1),
        id(2)
    );
    let expected = expected.as_str();
    assert_eq!(
        payload_of(&bytes),
        expected,
        "the payload this file expects"
    );
    let table = expected_table(expected);
    let at = lcmp_at(&bytes);
    assert_eq!(
        &bytes[at..at + table.len()],
        &table[..],
        "the table this file assembled sits at LCMP"
    );
    let llbl_free = !bytes.windows(4).any(|w| w == b"LLBL");
    assert!(llbl_free, "no label table in this fixture");

    let reopened = open_ok(&path);
    let listing = comps(reopened);
    assert_eq!(listing["count"], 1);
    assert_eq!(listing["comps"][0]["comment"], "the one with the sky");
    let entries = listing["comps"][0]["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 3);
    for (i, entry) in entries.iter().enumerate() {
        assert_eq!(entry["layer"], json!(i), "entry {i} rebound by its id");
        assert_eq!(entry["stale"], json!(false));
        assert_eq!(
            entry["id"],
            json!(unsafe { dr_doc_layer_id(reopened, i) }),
            "entry {i} carries the id the reopened record kept"
        );
    }
    // And writing it again is byte-identical.
    assert_eq!(save(reopened, &dir.path().join("twice.rz")), bytes);
    unsafe { dr_doc_free(reopened) };
    unsafe { dr_doc_free(doc) };
}

#[test]
fn an_in_session_rename_survives_save_and_reopen() {
    let dir = TempDir::new().unwrap();
    let doc = fixture(&dir);
    let doc = comp(doc, save_request("A"));
    // Renamed AFTER the comp was saved: the id keeps the entry bound in
    // session, and the writer records the CURRENT name, so it reopens bound.
    let name = CString::new("Middle renamed").unwrap();
    let doc = unsafe { must(dr_doc_with_layer_name(doc, 1, name.as_ptr()), doc) };
    assert_eq!(comps(doc)["comps"][0]["entries"][1]["layer"], json!(1));

    let path = dir.path().join("renamed.rz");
    let bytes = save(doc, &path);
    assert!(
        payload_of(&bytes).contains("Middle renamed"),
        "the writer records the CURRENT name"
    );
    let reopened = open_ok(&path);
    let entries = comps(reopened)["comps"][0]["entries"].clone();
    assert_eq!(entries[1]["layer"], json!(1), "still bound after reopen");
    assert_eq!(entries[1]["stale"], json!(false));
    assert_eq!(entries[1]["name"], "Middle renamed");
    unsafe { dr_doc_free(reopened) };
    unsafe { dr_doc_free(doc) };
}

#[test]
fn a_stale_entry_is_written_and_reopens_stale() {
    let dir = TempDir::new().unwrap();
    // Deleting layer 2 of 3 — the middle one — and layer 0 of 3 both have to
    // reopen, because the writer must never emit a file its reader refuses.
    for victim in [1usize, 0] {
        let doc = fixture(&dir);
        let names: Vec<String> = (0..3)
            .map(|i| {
                let c = unsafe { dr_doc_layer_name(doc, i) };
                take_err_string(c)
            })
            .collect();
        let doc = comp(doc, save_request("A"));
        let doc = unsafe { must(dr_doc_removing_layer(doc, victim), doc) };

        let path = dir.path().join(format!("stale{victim}.rz"));
        let bytes = save(doc, &path);
        let payload = payload_of(&bytes);
        assert!(
            !payload.contains(r#""layer""#),
            "schema 2 writes no index key at all: {payload}"
        );
        let reopened = open_ok(&path);
        let listing = comps(reopened);
        assert_eq!(listing["comps"][0]["stale_count"], 1);
        let entries = listing["comps"][0]["entries"].as_array().unwrap();
        assert_eq!(entries[victim]["layer"], Value::Null);
        assert_eq!(entries[victim]["stale"], json!(true));
        assert_eq!(entries[victim]["name"], names[victim].as_str());
        // The DELETED entry's id was written verbatim, and it is an id no
        // live record carries — which is exactly what "stale" means now.
        let stale_id = payload_of(&bytes);
        let live: Vec<u64> = (0..unsafe { dr_doc_layer_count(reopened) })
            .map(|i| unsafe { dr_doc_layer_id(reopened, i) })
            .collect();
        let written: Vec<u64> = serde_json::from_str::<Value>(&stale_id).expect("payload JSON")
            ["comps"][0]["entries"]
            .as_array()
            .expect("entries")
            .iter()
            .map(|e| e["id"].as_u64().expect("an id key"))
            .collect();
        assert!(
            written[victim] != 0 && !live.contains(&written[victim]),
            "the stale entry wrote a dangling id {written:?} against {live:?}"
        );
        let survivors: Vec<&Value> = entries
            .iter()
            .enumerate()
            .filter(|(i, _)| *i != victim)
            .map(|(_, e)| e)
            .collect();
        for entry in survivors {
            assert_eq!(entry["stale"], json!(false), "{entry:?}");
        }
        unsafe { dr_doc_free(reopened) };
        unsafe { dr_doc_free(doc) };
    }
}

#[test]
fn a_crafted_comp_table_refuses_the_open() {
    let dir = TempDir::new().unwrap();
    let doc = fixture(&dir);
    let doc = comp(doc, save_request("A"));
    let good = save(doc, &dir.path().join("good.rz"));
    unsafe { dr_doc_free(doc) };
    let payload = payload_of(&good);
    // The `LCMP` table is located by its tag: `LIDS` closes the file since
    // 12E-1, so counting back from the end names the wrong block.
    let head = lcmp_at(&good);
    let old_table = expected_table(&payload);

    let write = |name: &str, bytes: &[u8]| -> std::path::PathBuf {
        let path = dir.path().join(name);
        std::fs::write(&path, bytes).unwrap();
        path
    };
    let rebuilt = |text: &str| -> Vec<u8> { with_table(&good, &old_table, &expected_table(text)) };

    // A version word this build does not know. 45 IS known now (it is the
    // identity wrapper), so the unknown one moved up.
    let mut future = good.clone();
    future[4..8].copy_from_slice(&47u32.to_le_bytes());
    assert!(
        open_err(&write("v47.rz", &future), "version 47").contains("unsupported DRDC version"),
        "an unknown outermost word is refused by name"
    );

    // A schema this build does not know at all.
    let mut schema = good.clone();
    schema[head + 4..head + 8].copy_from_slice(&3u32.to_le_bytes());
    assert!(
        open_err(&write("schema.rz", &schema), "schema 3").contains("layer comp schema"),
        "the refusal names the schema"
    );

    // The binary word and the JSON key are ONE statement: a file claiming
    // binary 2 over a payload claiming 1 — the way a crafted file would pick
    // the looser rule — is refused.
    let mismatch = with_table(
        &good,
        &old_table,
        &table_with_schema(2, &payload.replace(r#""schema":2"#, r#""schema":1"#)),
    );
    assert!(
        open_err(&write("mismatch.rz", &mismatch), "a schema mismatch")
            .contains("layer comp schema"),
        "the refusal names the schema"
    );

    let mut zero_table = b"LCMP".to_vec();
    zero_table.extend(2u32.to_le_bytes());
    zero_table.extend(0u32.to_le_bytes());
    let zero = with_table(&good, &old_table, &zero_table);
    assert!(
        open_err(&write("zero.rz", &zero), "length 0").contains("payload length"),
        "the refusal names the length"
    );

    let mut huge = good.clone();
    huge[head + 8..head + 12].copy_from_slice(&((16u32 << 20) + 1).to_le_bytes());
    assert!(
        open_err(&write("huge.rz", &huge), "an oversize length").contains("payload length"),
        "the refusal names the length"
    );

    assert!(
        open_err(&write("json.rz", &rebuilt("{ not json")), "bad JSON")
            .contains("layer comp payload"),
        "the refusal names the payload"
    );

    // SCHEMA 2 addresses by ID, so the two structural refusals are about
    // ids: a REPEATED non-zero id within one comp refuses (two rows would
    // address one record), while an id NO record carries is simply stale —
    // that is what a deleted layer leaves behind and refusing it would make
    // an ordinary file unopenable.
    let ids: Vec<u64> = serde_json::from_str::<Value>(&payload).expect("payload JSON")["comps"][0]
        ["entries"]
        .as_array()
        .expect("entries")
        .iter()
        .map(|e| e["id"].as_u64().expect("an id key"))
        .collect();
    let repeated = replace_comp_id(&payload, ids[2], &format!(r#""id":{}"#, ids[1]));
    assert_ne!(repeated, payload);
    assert!(
        open_err(&write("repeat.rz", &rebuilt(&repeated)), "a repeated id")
            .contains("layer comp reference"),
        "the refusal names the reference"
    );

    let unknown = replace_comp_id(&payload, ids[2], r#""id":999999999"#);
    assert_ne!(unknown, payload);
    let stale = open_ok(&write("unknown.rz", &rebuilt(&unknown)));
    assert_eq!(
        comps(stale)["comps"][0]["stale_count"],
        1,
        "an id no record carries reopens STALE, not refused"
    );
    unsafe { dr_doc_free(stale) };

    // A `layer` key inside a schema-2 payload is the schema-1 address, which
    // this schema does not admit.
    let mixed = replace_comp_id(&payload, ids[0], &format!(r#""id":{},"layer":0"#, ids[0]));
    assert_ne!(mixed, payload);
    assert!(
        open_err(&write("mixed.rz", &rebuilt(&mixed)), "a layer key")
            .contains("layer comp reference"),
        "the refusal names the reference"
    );

    // The one comp object, spelled out twice — this file's own slicing, not
    // a trim, because the object itself ends in the same two characters the
    // wrapper does.
    let opening = r#"{"schema":2,"comps":["#;
    let object = &payload[opening.len()..payload.len() - 2];
    let duplicate = format!("{opening}{object},{object}]}}");
    assert!(
        open_err(&write("dup.rz", &rebuilt(&duplicate)), "a duplicate name")
            .contains("duplicate native layer comp name"),
        "the refusal names the duplicate"
    );

    let mut trailing = good.clone();
    trailing.extend(7u32.to_le_bytes());
    assert!(
        open_err(&write("trailing.rz", &trailing), "a trailing word").contains("trailing"),
        "the refusal names the trailing bytes"
    );

    // A non-finite opacity cannot be written by serde_json, so it is spelled
    // into the payload directly, which is exactly how a crafted file would
    // carry one.
    let infinite = payload.replace(r#""opacity":1.0"#, r#""opacity":1e400"#);
    assert_ne!(infinite, payload);
    assert!(
        open_err(&write("inf.rz", &rebuilt(&infinite)), "an infinite opacity")
            .contains("layer comp"),
        "the refusal names the comp table"
    );
}

#[test]
fn the_comp_wrapper_nests_outside_the_label_table() {
    let dir = TempDir::new().unwrap();
    let doc = fixture(&dir);
    let doc = unsafe { must(dr_doc_with_layer_label(doc, 1, 5), doc) };
    let doc = comp(doc, save_request("A"));
    let path = dir.path().join("both.rz");
    let bytes = save(doc, &path);
    assert_eq!(
        word(&bytes, 0),
        IDENTITY_BYTE,
        "the identity word is outermost"
    );
    assert_eq!(word(&bytes, 1), 43, "the comp word is inside it");
    assert_eq!(word(&bytes, 2), 41, "then the label word");
    let llbl = bytes.windows(4).position(|w| w == b"LLBL").expect("LLBL");
    let lcmp = bytes.windows(4).position(|w| w == b"LCMP").expect("LCMP");
    assert!(llbl < lcmp, "LCMP follows LLBL: {llbl} {lcmp}");

    let reopened = open_ok(&path);
    assert_eq!(unsafe { dr_doc_layer_label(reopened, 1) }, 5);
    assert_eq!(comps(reopened)["count"], 1);
    unsafe { dr_doc_free(reopened) };
    unsafe { dr_doc_free(doc) };
}

// ------------------------------------------------------------ typed depth --

struct Owned(*mut PrecisionDocument);
impl Drop for Owned {
    fn drop(&mut self) {
        unsafe { dr_precision_doc_free(self.0) };
    }
}
impl std::ops::Deref for Owned {
    type Target = PrecisionDocument;
    fn deref(&self) -> &PrecisionDocument {
        unsafe { &*self.0 }
    }
}

fn typed_fixture() -> PrecisionDocument {
    let profile = IccProfile::srgb();
    let samples = |seed: u16| -> Vec<[u16; 4]> {
        (0..6u16)
            .map(|i| [seed + i * 11, 20000 + i * 7, 30000 + i, 65535])
            .collect()
    };
    let image = |seed: u16| {
        PrecisionImage::from_rgba16(3, 2, samples(seed), Arc::clone(&profile), None).unwrap()
    };
    PrecisionDocument::from_parts(
        3,
        2,
        Arc::clone(&profile),
        Metadata::default(),
        Resolution::default(),
        vec![
            Layer::raster("Base", image(2000), (0, 0), 1.0, true).unwrap(),
            Layer::raster("Middle", image(3000), (0, 0), 1.0, true).unwrap(),
            Layer::raster("Top", image(4000), (0, 0), 1.0, true).unwrap(),
        ],
    )
    .unwrap()
}

fn typed_edit(document: &PrecisionDocument, request: &Value) -> Result<Option<Owned>, String> {
    let c = CString::new(request.to_string()).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_edit(
            document,
            c.as_ptr(),
            ptr::null(),
            0,
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    if out.is_null() {
        if err.is_null() {
            return Ok(None);
        }
        return Err(take_err_string(err));
    }
    Ok(Some(Owned(out)))
}

fn typed_comp(document: &PrecisionDocument, request: Value) -> Owned {
    match typed_edit(document, &request) {
        Ok(Some(out)) => out,
        Ok(None) => panic!("{request} reported no change"),
        Err(message) => panic!("{request}: {message}"),
    }
}

fn typed_state(document: &PrecisionDocument) -> Vec<(bool, (i32, i32), f32)> {
    (0..document.layer_count())
        .filter_map(|i| document.layer(i))
        .map(|l| (l.visible(), l.offset(), l.opacity()))
        .collect()
}

#[test]
fn the_typed_ops_round_trip_and_honour_the_no_op_rule() {
    let document = typed_fixture();
    assert_eq!(typed_comps(&document)["count"], 0);

    let a = typed_comp(&document, save_request("A"));
    let listing = typed_comps(&a);
    assert_eq!(listing["count"], 1);
    assert_eq!(listing["comps"][0]["entries"].as_array().unwrap().len(), 3);
    assert_eq!(
        listing["comps"][0]["locked_count"], 0,
        "a typed layer carries no lock, so nothing is ever reported locked"
    );
    let a_state = typed_state(&a);
    assert!(
        typed_edit(&a, &apply_request(0))
            .expect("admitted")
            .is_none(),
        "applying the state the document is already in is an exact no-op"
    );

    // Perturb through the typed edits the host actually uses, then save B.
    let moved = typed_comp(
        &a,
        json!({"op": "layer_visible", "layer": 2, "visible": false}),
    );
    let faded = moved
        .set_layer_opacity(1, 0.5)
        .expect("opacity")
        .expect("a change");
    let b_state = typed_state(&faded);
    assert_ne!(a_state, b_state);
    let b = typed_comp(&faded, save_request("B"));

    let back = typed_comp(&b, apply_request(0));
    assert_eq!(typed_state(&back), a_state, "comp A restores state A");
    let forward = typed_comp(&back, apply_request(1));
    assert_eq!(typed_state(&forward), b_state, "comp B restores state B");

    let Err(message) = typed_edit(&forward, &save_request("A")) else {
        panic!("a duplicate name must refuse");
    };
    assert!(message.contains("already exists"), "{message}");
    let deleted = typed_comp(
        &forward,
        json!({"op": "layer_comp", "action": "delete", "comp": 0}),
    );
    assert_eq!(typed_comps(&deleted)["count"], 1);
    assert_eq!(typed_comps(&deleted)["comps"][0]["name"], "B");
}

#[test]
fn a_typed_document_round_trips_through_an_independently_assembled_table() {
    let document = typed_fixture();
    let plain = document.encode_native(LIMIT, 0, None).expect("plain bytes");
    assert_eq!(
        word(&plain, 0),
        IDENTITY_TYPED,
        "the identity word is outermost"
    );
    assert_ne!(word(&plain, 1), 44, "no comp wrapper is written");
    assert!(!plain.windows(4).any(|w| w == b"LCMP"));

    let hidden = typed_comp(
        &document,
        json!({"op": "layer_visible", "layer": 2, "visible": false}),
    );
    let saved = typed_comp(&hidden, save_request("Hero"));
    let bytes = saved.encode_native(LIMIT, 0, None).expect("bytes");
    assert_eq!(
        word(&bytes, 0),
        IDENTITY_TYPED,
        "the identity word is outermost"
    );
    assert_eq!(word(&bytes, 1), 44, "44 is the word inside it");

    // SCHEMA 2: the entry addresses its record by the PERSISTED id and
    // carries no "layer" key. The ids are minted at run time, so they come
    // back through the layer info rather than as literals.
    let id = |i: usize| saved.layer(i).expect("a record").id();
    let expected = format!(
        concat!(
            r#"{{"schema":2,"comps":[{{"name":"Hero","comment":"","#,
            r#""captures":{{"visibility":true,"position":true,"appearance":true}},"entries":["#,
            r#"{{"id":{0},"name":"Base","visible":true,"offset":[0,0],"opacity":1.0,"blend":0}},"#,
            r#"{{"id":{1},"name":"Middle","visible":true,"offset":[0,0],"opacity":1.0,"blend":0}},"#,
            r#"{{"id":{2},"name":"Top","visible":false,"offset":[0,0],"opacity":1.0,"blend":0}}"#,
            r#"]}}]}}"#,
        ),
        id(0),
        id(1),
        id(2)
    );
    let expected = expected.as_str();
    assert_eq!(payload_of(&bytes), expected);
    let table = expected_table(expected);
    let at = lcmp_at(&bytes);
    assert_eq!(&bytes[at..at + table.len()], &table[..]);

    let reopened = PrecisionDocument::parse_native(&bytes, LIMIT, 0, None).expect("reopen");
    let listing = typed_comps(&reopened);
    assert_eq!(listing["count"], 1);
    for (i, entry) in listing["comps"][0]["entries"]
        .as_array()
        .unwrap()
        .iter()
        .enumerate()
    {
        assert_eq!(entry["layer"], json!(i), "entry {i} rebound by its id");
        assert_eq!(entry["stale"], json!(false));
        assert_eq!(
            entry["id"],
            json!(reopened.layer(i).expect("a record").id()),
            "entry {i} carries the id the reopened record kept"
        );
    }
    assert_eq!(
        reopened.encode_native(LIMIT, 0, None).expect("again"),
        bytes,
        "writing the reopened document is byte-identical"
    );

    // Deleting the last comp restores the prior writer's exact bytes — the
    // comp was saved on `hidden`, so that is the document to compare with.
    let cleared = typed_comp(
        &saved,
        json!({"op": "layer_comp", "action": "delete", "comp": 0}),
    );
    assert_eq!(
        cleared.encode_native(LIMIT, 0, None).expect("cleared"),
        hidden.encode_native(LIMIT, 0, None).expect("hidden"),
    );
}

#[test]
fn a_typed_stale_entry_reopens_stale() {
    // A typed document has no delete-record op in the core (12E owns typed
    // structure edits), so the STALE case is built the only honest way: the
    // writer's own payload with one entry's index turned into the `null` the
    // byte writer is proven to emit for a deleted layer, reparsed.
    let document = typed_fixture();
    let saved = typed_comp(&document, save_request("A"));
    let bytes = saved.encode_native(LIMIT, 0, None).expect("bytes");
    let payload = payload_of(&bytes);
    let middle = saved.layer(1).expect("a record").id();
    let stale = replace_comp_id(&payload, middle, r#""id":0"#);
    assert_ne!(stale, payload);
    let table = expected_table(&payload);
    let crafted = with_table(&bytes, &table, &expected_table(&stale));

    let reopened = PrecisionDocument::parse_native(&crafted, LIMIT, 0, None).expect("reopen");
    let listing = typed_comps(&reopened);
    assert_eq!(listing["comps"][0]["stale_count"], 1);
    let entries = listing["comps"][0]["entries"].as_array().unwrap();
    assert_eq!(entries[1]["layer"], Value::Null);
    assert_eq!(entries[1]["stale"], json!(true));
    assert_eq!(entries[1]["name"], "Middle", "reported under its own name");
    assert_eq!(
        entries[0]["layer"],
        json!(0),
        "the other entries stay bound"
    );
    assert_eq!(entries[2]["layer"], json!(2));

    // …and it WRITES back as stale, so the round trip is closed.
    let again = reopened.encode_native(LIMIT, 0, None).expect("again");
    assert!(payload_of(&again).contains(r#""id":0"#));
    assert_eq!(
        payload_of(&again),
        stale,
        "the stale entry is written verbatim"
    );

    assert!(
        unsafe { dr_precision_doc_layer_comps_json(ptr::null(), ptr::null_mut()) }.is_null(),
        "the typed export tolerates NULL"
    );
}

#[test]
fn a_crafted_typed_comp_table_refuses_the_open() {
    let document = typed_fixture();
    let saved = typed_comp(&document, save_request("A"));
    let good = saved.encode_native(LIMIT, 0, None).expect("bytes");
    let payload = payload_of(&good);
    let head = lcmp_at(&good);
    let old_table = expected_table(&payload);
    let rebuilt = |text: &str| -> Vec<u8> { with_table(&good, &old_table, &expected_table(text)) };
    let refuse = |bytes: &[u8], what: &str| -> String {
        match PrecisionDocument::parse_native(bytes, LIMIT, 0, None) {
            Err(message) => message,
            Ok(_) => panic!("{what} must refuse the typed open"),
        }
    };

    let mut schema = good.clone();
    schema[head + 4..head + 8].copy_from_slice(&3u32.to_le_bytes());
    assert!(refuse(&schema, "schema 3").contains("layer comp schema"));

    // Binary 2 over a payload claiming 1 — the two must agree.
    let mismatch = with_table(
        &good,
        &old_table,
        &table_with_schema(2, &payload.replace(r#""schema":2"#, r#""schema":1"#)),
    );
    assert!(refuse(&mismatch, "a schema mismatch").contains("layer comp schema"));

    let top = saved.layer(2).expect("a record").id();
    let middle = saved.layer(1).expect("a record").id();
    let repeated = replace_comp_id(&payload, top, &format!(r#""id":{middle}"#));
    assert_ne!(repeated, payload);
    assert!(refuse(&rebuilt(&repeated), "a repeated id").contains("layer comp reference"));

    // A schema-1 address inside a schema-2 payload.
    let mixed = replace_comp_id(&payload, top, &format!(r#""id":{top},"layer":2"#));
    assert_ne!(mixed, payload);
    assert!(refuse(&rebuilt(&mixed), "a layer key").contains("layer comp reference"));

    let mut trailing = good.clone();
    trailing.extend(7u32.to_le_bytes());
    assert!(refuse(&trailing, "a trailing word").contains("trailing"));

    let mut future = good.clone();
    future[4..8].copy_from_slice(&99u32.to_le_bytes());
    assert!(
        !refuse(&future, "an unknown outermost word").is_empty(),
        "an unknown outermost word is refused"
    );
}

// ----------------------------------------------------------------- reorder --

fn reorder_request(comp: usize, to: usize) -> Value {
    json!({"op": "layer_comp", "action": "reorder", "comp": comp, "to": to})
}

/// The comp names in list order, and each comp's reported `index`.
fn order(listing: &Value) -> (Vec<String>, Vec<u64>) {
    let rows = listing["comps"].as_array().expect("comps");
    (
        rows.iter()
            .map(|c| c["name"].as_str().expect("name").to_string())
            .collect(),
        rows.iter()
            .map(|c| c["index"].as_u64().expect("index"))
            .collect(),
    )
}

/// Each comp's `entries`, keyed by comp name, so a reorder can be shown to
/// move the recordings verbatim.
fn entries_by_name(listing: &Value) -> Vec<(String, Value)> {
    let mut rows: Vec<(String, Value)> = listing["comps"]
        .as_array()
        .expect("comps")
        .iter()
        .map(|c| {
            (
                c["name"].as_str().expect("name").to_string(),
                c["entries"].clone(),
            )
        })
        .collect();
    rows.sort_by(|a, b| a.0.cmp(&b.0));
    rows
}

fn names(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| s.to_string()).collect()
}

#[test]
fn reorder_moves_a_comp_to_its_final_index_and_keeps_its_recording() {
    let dir = TempDir::new().unwrap();
    let doc = fixture(&dir);

    // Three comps recorded in three different states.
    let doc = comp(doc, save_request("A"));
    let a_state = state(doc);
    let doc = unsafe { must(dr_doc_with_layer_visible(doc, 2, false), doc) };
    let doc = comp(doc, save_request("B"));
    let doc = unsafe { must(dr_doc_with_layer_offset(doc, 1, 7, 5), doc) };
    let doc = unsafe { must(dr_doc_with_layer_opacity(doc, 0, 0.25), doc) };
    let doc = comp(doc, save_request("C"));
    assert_ne!(state(doc), a_state);
    let before = comps(doc);
    assert_eq!(order(&before), (names(&["A", "B", "C"]), vec![0, 1, 2]));

    // `to` is the FINAL index: A to 2 is [B, C, A].
    let doc = comp(doc, reorder_request(0, 2));
    let listing = comps(doc);
    assert_eq!(order(&listing), (names(&["B", "C", "A"]), vec![0, 1, 2]));
    assert_eq!(
        entries_by_name(&listing),
        entries_by_name(&before),
        "a reorder moves each recording verbatim"
    );

    // …and C to 0 is [C, B, A].
    let doc = comp(doc, reorder_request(1, 0));
    let listing = comps(doc);
    assert_eq!(order(&listing), (names(&["C", "B", "A"]), vec![0, 1, 2]));
    assert_eq!(entries_by_name(&listing), entries_by_name(&before));

    // The same index is the exact no-op.
    assert!(
        edit(doc, &reorder_request(1, 1))
            .expect("admitted")
            .is_none(),
        "a comp moved to its own index is an exact no-op"
    );

    // Applying A at its NEW index restores A's state.
    let applied = comp(doc, apply_request(2));
    assert_eq!(state(applied), a_state, "A applies from its new index");

    // A `.rz` round trip keeps the order.
    let path = dir.path().join("reordered.rz");
    save(applied, &path);
    let reopened = open_ok(&path);
    assert_eq!(
        order(&comps(reopened)),
        (names(&["C", "B", "A"]), vec![0, 1, 2])
    );
    unsafe {
        dr_doc_free(reopened);
        dr_doc_free(applied);
    }
}

#[test]
fn reorder_refusals_are_named() {
    let dir = TempDir::new().unwrap();
    let doc = comp(fixture(&dir), save_request("A"));

    // A one-comp list has exactly one destination.
    assert_eq!(
        refusal(doc, reorder_request(0, 1)),
        "layer comp destination index 1 is out of range (0..0)"
    );
    assert!(
        edit(doc, &reorder_request(0, 0))
            .expect("admitted")
            .is_none(),
        "a one-comp list reordered to 0 is an exact no-op"
    );

    let doc = comp(doc, save_request("B"));
    let doc = comp(doc, save_request("C"));
    assert_eq!(
        refusal(doc, reorder_request(3, 0)),
        "layer comp index is out of range"
    );
    assert_eq!(
        refusal(doc, reorder_request(0, 3)),
        "layer comp destination index 3 is out of range (0..2)"
    );
    assert_eq!(
        refusal(
            doc,
            json!({"op": "layer_comp", "action": "reorder", "comp": 0})
        ),
        "layer comp reorder requires a destination index"
    );
    assert_eq!(
        refusal(
            doc,
            json!({"op": "layer_comp", "action": "reorder", "to": 0})
        ),
        "layer comp reorder requires a comp index"
    );
    unsafe { dr_doc_free(doc) };
}

#[test]
fn the_typed_reorder_matches_the_byte_rule() {
    let document = typed_fixture();
    let a = typed_comp(&document, save_request("A"));
    let b = typed_comp(&a, save_request("B"));
    let c = typed_comp(&b, save_request("C"));

    let moved = typed_comp(&c, reorder_request(0, 2));
    assert_eq!(
        order(&typed_comps(&moved)),
        (names(&["B", "C", "A"]), vec![0, 1, 2])
    );
    assert_eq!(
        entries_by_name(&typed_comps(&moved)),
        entries_by_name(&typed_comps(&c))
    );
    assert!(
        typed_edit(&moved, &reorder_request(2, 2))
            .expect("admitted")
            .is_none(),
        "a comp moved to its own index is an exact no-op"
    );
    let Err(message) = typed_edit(&moved, &reorder_request(0, 3)) else {
        panic!("a destination past the end must refuse");
    };
    assert_eq!(
        message,
        "layer comp destination index 3 is out of range (0..2)"
    );

    // The typed `.rz` round trip keeps the order too.
    let bytes = moved.encode_native(LIMIT, 0, None).expect("bytes");
    let reopened = PrecisionDocument::parse_native(&bytes, LIMIT, 0, None).expect("reopens");
    assert_eq!(
        order(&typed_comps(&reopened)),
        (names(&["B", "C", "A"]), vec![0, 1, 2])
    );
}

#[test]
fn replace_comp_id_leaves_prefixed_ids_alone() {
    // Ids come from the process-wide counter the parallel tests share,
    // so `"id":1` must not also rewrite `"id":12`.
    let payload = r#"{"entries":[{"id":1,"name":"A"},{"id":12,"name":"B"},{"id":1}]}"#;
    assert_eq!(
        replace_comp_id(payload, 1, r#""id":7"#),
        r#"{"entries":[{"id":7,"name":"A"},{"id":12,"name":"B"},{"id":7}]}"#
    );
    assert_eq!(
        replace_comp_id(payload, 12, r#""id":1,"layer":0"#),
        r#"{"entries":[{"id":1,"name":"A"},{"id":1,"layer":0,"name":"B"},{"id":1}]}"#
    );
    assert_eq!(replace_comp_id(payload, 2, r#""id":9"#), payload);
}

/// The payload cap is a property of the LIST, so it has to be reachable with
/// a cap small enough to test — an 8 MiB comp list is 64 comps over
/// thousands of layers. The `#[doc(hidden)] pub` seam
/// `edit_layer_comps_capped` takes the cap as a parameter; the cap the
/// Update meets is the length of the payload the saved file carries, read by
/// this file's own `payload_of`.
#[test]
fn the_payload_cap_refuses_save_and_update_by_name() {
    /// `doc_comps`'s payload-cap refusal, restated here rather than read out
    /// of the core: the byte and the typed op word it identically.
    const PAYLOAD_CAP_REFUSAL: &str = "layer comps exceed the .doctored payload cap";
    let dir = TempDir::new().unwrap();
    let doc = DrDocument::from_pixels(RgbaImage::from_pixel(2, 2, Rgba([10, 20, 30, 255])));
    let save = CompRequest::Save {
        name: "Hero".into(),
        comment: String::new(),
        captures: CompCaptures::default(),
    };
    // `DrDocument` is deliberately not `Debug`, so the refusal is taken
    // apart with a `let else` rather than with `expect_err`.
    let Err(message) = doc.edit_layer_comps_capped(&save, 64) else {
        panic!("a 64-byte cap must refuse the first comp");
    };
    assert_eq!(message, PAYLOAD_CAP_REFUSAL);

    // The same refusal on Update, reached with a cap that admits the saved
    // comp and not the renamed one.
    let saved = doc
        .edit_layer_comps(&save)
        .expect("the save is admitted")
        .expect("a new document");
    let path = dir.path().join("saved.rz");
    saved
        .save_native(path.to_str().expect("a UTF-8 path"))
        .expect("the document saves");
    let payload = payload_of(&std::fs::read(&path).expect("the saved file"));
    let update = CompRequest::Update {
        comp: 0,
        name: Some("Hero with a very much longer name than the first one".into()),
        comment: None,
        captures: None,
        recapture: true,
    };
    let Err(message) = saved.edit_layer_comps_capped(&update, payload.len() as u64) else {
        panic!("a longer name must not fit the cap the short one filled");
    };
    assert_eq!(message, PAYLOAD_CAP_REFUSAL);
}
