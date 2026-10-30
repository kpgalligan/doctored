//! Phase 12E-1b: LAYER COMPS bound by the PERSISTED layer id.
//!
//! `layer_comps.rs` owns the comp ops themselves; this file owns the MIGRATION
//! — that both depths now write `LCMP` schema 2, that a schema-2 entry rebinds
//! across a reorder AND a rename (which schema 1 could not survive), that
//! schema 1 is still read exactly as it was, and the four refusals a crafted
//! table earns. Every table in this file is assembled here, byte by byte, from
//! the payload alone.

use std::ffi::{c_char, CStr, CString};
use std::path::Path;
use std::ptr;

use doctored_core::doc::DrDocument;
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_comps::{
    dr_doc_edit_layer_comps, dr_doc_layer_comps_json, dr_precision_doc_layer_comps_json,
};
use doctored_core::ffi_doc::{
    dr_doc_free, dr_doc_layer_count, dr_doc_layer_visible, dr_doc_open, dr_doc_save_native,
    dr_doc_with_layer_name, dr_doc_with_layer_visible,
};
use doctored_core::ffi_group::{dr_doc_layer_depth, dr_doc_move_layer_to};
use doctored_core::ffi_identity::dr_doc_layer_id;
use doctored_core::ffi_precision_edit::dr_precision_doc_edit;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer as Layer};
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use serde_json::{json, Value};
use tempfile::TempDir;

use crate::common;
use common::{
    add_layer, cpath, doc_from, opaque_pattern, replace_comp_id, take_err_string, test_pattern,
};

const LIMIT: u64 = REFERENCE_WORKING_BYTES;

// -------------------------------------------- the table, assembled by hand --

/// The `LCMP` table for a payload at a given schema: the tag, the u32 schema,
/// the u32 length, then the bytes. Nothing here reads `comps_native`.
fn table(schema: u32, payload: &str) -> Vec<u8> {
    let mut out = b"LCMP".to_vec();
    out.extend(schema.to_le_bytes());
    out.extend((payload.len() as u32).to_le_bytes());
    out.extend(payload.as_bytes());
    out
}

fn lcmp_at(bytes: &[u8]) -> usize {
    bytes.windows(4).rposition(|w| w == b"LCMP").expect("LCMP")
}

fn payload_of(bytes: &[u8]) -> String {
    let at = lcmp_at(bytes);
    let length = u32::from_le_bytes(bytes[at + 8..at + 12].try_into().expect("4 bytes")) as usize;
    String::from_utf8(bytes[at + 12..at + 12 + length].to_vec()).expect("utf-8")
}

fn schema_word(bytes: &[u8]) -> u32 {
    let at = lcmp_at(bytes);
    u32::from_le_bytes(bytes[at + 4..at + 8].try_into().expect("4 bytes"))
}

/// `bytes` with its `LCMP` table replaced, keeping the `LIDS` tail intact.
fn with_table(bytes: &[u8], old: &[u8], new: &[u8]) -> Vec<u8> {
    let at = lcmp_at(bytes);
    let mut out = bytes[..at].to_vec();
    out.extend_from_slice(new);
    out.extend_from_slice(&bytes[at + old.len()..]);
    out
}

/// The same file with the IDENTITY wrapper peeled off — the outermost word
/// and the `LIDS` tail cut away — which is what a file with no persisted ids
/// looks like. A schema-2 payload inside one of those must be refused.
fn without_identity(bytes: &[u8], records: usize) -> Vec<u8> {
    let tail = 12 + 8 * records;
    assert_eq!(
        &bytes[bytes.len() - tail..bytes.len() - tail + 4],
        b"LIDS",
        "the identity table closes every file this build writes"
    );
    let mut out = bytes[..4].to_vec();
    out.extend_from_slice(&bytes[8..bytes.len() - tail]);
    out
}

// ------------------------------------------------------------- byte side --

fn fixture(dir: &TempDir) -> *mut DrDocument {
    let doc = doc_from(dir, "base.png", &opaque_pattern(4, 3));
    let doc = add_layer(dir, "middle.png", doc, 0, &test_pattern(4, 3), "Middle");
    add_layer(dir, "top.png", doc, 1, &test_pattern(4, 3), "Top")
}

fn comp(doc: *mut DrDocument, request: Value) -> *mut DrDocument {
    let c = CString::new(request.to_string()).unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    let out = unsafe { dr_doc_edit_layer_comps(doc, c.as_ptr(), &mut err) };
    assert!(!out.is_null(), "{request}: {}", take_err_string(err));
    unsafe { dr_doc_free(doc) };
    out
}

fn comps(doc: *const DrDocument) -> Value {
    let mut err: *mut c_char = ptr::null_mut();
    let json = unsafe { dr_doc_layer_comps_json(doc, &mut err) };
    assert!(!json.is_null(), "{}", take_err_string(err));
    serde_json::from_str(&take_err_string(json)).expect("comp JSON")
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

fn write(dir: &TempDir, name: &str, bytes: &[u8]) -> std::path::PathBuf {
    let path = dir.path().join(name);
    std::fs::write(&path, bytes).unwrap();
    path
}

// ------------------------------------------------------------ typed side --

fn typed_fixture() -> PrecisionDocument {
    let image = |code: u16| {
        PrecisionImage::from_rgba16(
            2,
            2,
            vec![[code, code, code, 65535]; 4],
            IccProfile::srgb(),
            None,
        )
        .expect("an image")
    };
    PrecisionDocument::from_parts(
        2,
        2,
        IccProfile::srgb(),
        Metadata::default(),
        Resolution::default(),
        vec![
            Layer::raster("Base", image(1000), (0, 0), 1.0, true).unwrap(),
            Layer::raster("Middle", image(2000), (0, 0), 1.0, true).unwrap(),
            Layer::raster("Top", image(3000), (0, 0), 1.0, true).unwrap(),
        ],
    )
    .expect("a document")
}

fn take_error(error: *mut c_char) -> Option<String> {
    if error.is_null() {
        return None;
    }
    unsafe {
        let text = CStr::from_ptr(error).to_string_lossy().into_owned();
        dr_string_free(error);
        Some(text)
    }
}

fn typed_edit(document: &PrecisionDocument, request: Value) -> Box<PrecisionDocument> {
    let c = CString::new(request.to_string()).unwrap();
    let mut error = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_edit(
            document,
            c.as_ptr(),
            ptr::null(),
            0,
            LIMIT,
            0,
            ptr::null(),
            &mut error,
        )
    };
    if let Some(message) = take_error(error) {
        panic!("{request}: {message}");
    }
    assert!(!out.is_null(), "{request} reported no change");
    unsafe { Box::from_raw(out) }
}

fn typed_comps(document: &PrecisionDocument) -> Value {
    let mut err: *mut c_char = ptr::null_mut();
    let json = unsafe { dr_precision_doc_layer_comps_json(document, &mut err) };
    assert!(!json.is_null(), "{}", take_err_string(err));
    serde_json::from_str(&take_err_string(json)).expect("comp JSON")
}

// ---------------------------------------------------------------- tests --

#[test]
fn both_depths_write_schema_two() {
    let dir = TempDir::new().unwrap();
    let doc = fixture(&dir);
    let doc = comp(
        doc,
        json!({"op": "layer_comp", "action": "save", "name": "A"}),
    );
    let bytes = save(doc, &dir.path().join("byte.rz"));
    assert_eq!(schema_word(&bytes), 2, "the byte writer emits schema 2");
    let payload: Value = serde_json::from_str(&payload_of(&bytes)).expect("payload JSON");
    assert_eq!(payload["schema"], json!(2), "and so does its JSON");
    for (i, entry) in payload["comps"][0]["entries"]
        .as_array()
        .expect("entries")
        .iter()
        .enumerate()
    {
        assert_eq!(
            entry["id"],
            json!(unsafe { dr_doc_layer_id(doc, i) }),
            "entry {i} carries the layer's own id"
        );
        assert!(entry.get("layer").is_none(), "and no index key at all");
    }
    unsafe { dr_doc_free(doc) };

    let typed = typed_fixture();
    let typed = typed_edit(
        &typed,
        json!({"op": "layer_comp", "action": "save", "name": "A"}),
    );
    let bytes = typed.encode_native(LIMIT, 0, None).expect("typed bytes");
    assert_eq!(schema_word(&bytes), 2, "the typed writer emits schema 2");
    let payload: Value = serde_json::from_str(&payload_of(&bytes)).expect("payload JSON");
    assert_eq!(payload["schema"], json!(2));
    for (i, entry) in payload["comps"][0]["entries"]
        .as_array()
        .expect("entries")
        .iter()
        .enumerate()
    {
        assert_eq!(
            entry["id"],
            json!(typed.layer(i).expect("a record").id()),
            "typed entry {i} carries the record's own id"
        );
        assert!(entry.get("layer").is_none());
    }
}

#[test]
fn a_reopened_comp_survives_a_reorder_and_a_rename_at_both_depths() {
    // This is the whole point of schema 2: index + name could survive one or
    // the other, never both.
    let dir = TempDir::new().unwrap();
    let doc = fixture(&dir);
    let doc = comp(
        doc,
        json!({"op": "layer_comp", "action": "save", "name": "A"}),
    );
    let ids: Vec<u64> = (0..unsafe { dr_doc_layer_count(doc) })
        .map(|i| unsafe { dr_doc_layer_id(doc, i) })
        .collect();
    let bytes = save(doc, &dir.path().join("before.rz"));
    unsafe { dr_doc_free(doc) };

    let reopened = open(&write(&dir, "carry.rz", &bytes)).expect("it reopens");
    // Move the bottom layer to the top AND rename it. Under schema 1 the
    // index moved and the name changed, so the entry would have gone stale.
    let moved = unsafe { dr_doc_move_layer_to(reopened, 0, 2, dr_doc_layer_depth(reopened, 2)) };
    assert!(!moved.is_null(), "the move is admitted");
    unsafe { dr_doc_free(reopened) };
    let name = CString::new("Renamed base").unwrap();
    let renamed = unsafe { dr_doc_with_layer_name(moved, 2, name.as_ptr()) };
    assert!(!renamed.is_null());
    unsafe { dr_doc_free(moved) };

    let listing = comps(renamed);
    assert_eq!(listing["comps"][0]["stale_count"], 0, "nothing went stale");
    let entries = listing["comps"][0]["entries"].as_array().unwrap();
    assert_eq!(
        entries[0]["id"],
        json!(ids[0]),
        "the entry still names the same layer"
    );
    assert_eq!(
        entries[0]["layer"],
        json!(2),
        "…which now sits at index 2, where it was moved to"
    );

    // Saving and reopening again keeps it bound through both changes.
    let bytes = save(renamed, &dir.path().join("after.rz"));
    unsafe { dr_doc_free(renamed) };
    let again = open(&write(&dir, "after2.rz", &bytes)).expect("it reopens again");
    let listing = comps(again);
    assert_eq!(listing["comps"][0]["stale_count"], 0);
    assert_eq!(listing["comps"][0]["entries"][0]["layer"], json!(2));
    unsafe { dr_doc_free(again) };

    // The typed depth, same claim, through the typed structural move.
    let typed = typed_fixture();
    let typed = typed_edit(
        &typed,
        json!({"op": "layer_comp", "action": "save", "name": "A"}),
    );
    let base_id = typed.layer(0).expect("a record").id();
    let bytes = typed.encode_native(LIMIT, 0, None).expect("bytes");
    let reopened = PrecisionDocument::parse_native(&bytes, LIMIT, 0, None).expect("reopen");
    let moved = typed_edit(
        &reopened,
        json!({"op": "move_layer_to", "from": 0, "to": 2, "depth": 0}),
    );
    let renamed = typed_edit(
        &moved,
        json!({"op": "rename_layer", "layer": 2, "name": "Renamed base"}),
    );
    let listing = typed_comps(&renamed);
    assert_eq!(listing["comps"][0]["stale_count"], 0);
    let entries = listing["comps"][0]["entries"].as_array().unwrap();
    assert_eq!(entries[0]["id"], json!(base_id));
    assert_eq!(entries[0]["layer"], json!(2));
}

#[test]
fn a_schema_one_file_still_reads_by_index_and_name() {
    // The back-compatibility guarantee, exercised on a table this file
    // assembles in the OLD form — which this build no longer writes.
    let dir = TempDir::new().unwrap();
    let doc = fixture(&dir);
    let doc = comp(
        doc,
        json!({"op": "layer_comp", "action": "save", "name": "A"}),
    );
    let modern = save(doc, &dir.path().join("modern.rz"));
    unsafe { dr_doc_free(doc) };
    let payload = payload_of(&modern);
    let modern_table = table(2, &payload);

    // Every `"id":N` replaced by the `"layer":i` the old writer emitted.
    let mut old = serde_json::from_str::<Value>(&payload).expect("payload JSON");
    old["schema"] = json!(1);
    for (i, entry) in old["comps"][0]["entries"]
        .as_array_mut()
        .expect("entries")
        .iter_mut()
        .enumerate()
    {
        let object = entry.as_object_mut().expect("an entry object");
        object.remove("id");
        object.insert("layer".into(), json!(i));
    }
    // Key ORDER does not matter to the reader, only to a byte comparison,
    // and this file never compares these bytes with the writer's.
    let old_payload = serde_json::to_string(&old).expect("schema-1 payload");
    let crafted = with_table(&modern, &modern_table, &table(1, &old_payload));

    let reopened = open(&write(&dir, "schema1.rz", &crafted)).expect("a schema-1 file reopens");
    let listing = comps(reopened);
    assert_eq!(listing["comps"][0]["stale_count"], 0, "bound by index+name");
    for (i, entry) in listing["comps"][0]["entries"]
        .as_array()
        .unwrap()
        .iter()
        .enumerate()
    {
        assert_eq!(entry["layer"], json!(i));
    }
    // …and resaving it writes SCHEMA 2, so the migration happens on the next
    // save rather than needing a separate step.
    let resaved = save(reopened, &dir.path().join("migrated.rz"));
    assert_eq!(
        schema_word(&resaved),
        2,
        "a reopened schema-1 file resaves as 2"
    );
    unsafe { dr_doc_free(reopened) };

    // A schema-1 entry whose layer was RENAMED goes stale, which is exactly
    // the old rule this file must not have changed.
    let stale_payload = old_payload.replace(r#""name":"Middle""#, r#""name":"Was Middle""#);
    assert_ne!(stale_payload, old_payload);
    let crafted = with_table(&modern, &modern_table, &table(1, &stale_payload));
    let reopened = open(&write(&dir, "schema1stale.rz", &crafted)).expect("it reopens");
    assert_eq!(
        comps(reopened)["comps"][0]["stale_count"],
        1,
        "schema 1 still compares the NAME"
    );
    unsafe { dr_doc_free(reopened) };

    // A bad or repeated schema-1 INDEX still refuses.
    for (name, bad) in [
        ("index", old_payload.replace(r#""layer":2"#, r#""layer":9"#)),
        (
            "repeat",
            old_payload.replace(r#""layer":2"#, r#""layer":1"#),
        ),
    ] {
        assert_ne!(bad, old_payload, "{name}");
        let crafted = with_table(&modern, &modern_table, &table(1, &bad));
        let message = open(&write(&dir, &format!("schema1{name}.rz"), &crafted))
            .err()
            .unwrap_or_else(|| panic!("a bad schema-1 {name} must refuse"));
        assert!(message.contains("layer comp reference"), "{message}");
    }

    // …and an `id` key inside a schema-1 payload is the schema-2 address,
    // which schema 1 does not admit.
    let mixed = old_payload.replace(r#""layer":0"#, r#""layer":0,"id":7"#);
    assert_ne!(mixed, old_payload);
    let crafted = with_table(&modern, &modern_table, &table(1, &mixed));
    let message = open(&write(&dir, "schema1id.rz", &crafted))
        .expect_err("an id key in a schema-1 payload must refuse");
    assert!(message.contains("layer comp reference"), "{message}");
}

#[test]
fn schema_two_without_an_identity_table_refuses() {
    // A crafted file could otherwise carry ids no record has and bind by
    // coincidence, so the reader requires the LIDS table the ids come from.
    let dir = TempDir::new().unwrap();
    let doc = fixture(&dir);
    let doc = comp(
        doc,
        json!({"op": "layer_comp", "action": "save", "name": "A"}),
    );
    let records = unsafe { dr_doc_layer_count(doc) };
    let bytes = save(doc, &dir.path().join("good.rz"));
    unsafe { dr_doc_free(doc) };

    let peeled = without_identity(&bytes, records);
    let message = open(&write(&dir, "noids.rz", &peeled))
        .expect_err("schema 2 with no identity table must refuse");
    assert!(
        message.contains("require a layer identity table"),
        "the refusal names what is missing: {message}"
    );

    // The same peeled file with a SCHEMA-1 payload reads perfectly, which is
    // what every file an older build wrote looks like.
    let payload = payload_of(&peeled);
    let mut old = serde_json::from_str::<Value>(&payload).expect("payload JSON");
    old["schema"] = json!(1);
    for (i, entry) in old["comps"][0]["entries"]
        .as_array_mut()
        .expect("entries")
        .iter_mut()
        .enumerate()
    {
        let object = entry.as_object_mut().expect("an entry object");
        object.remove("id");
        object.insert("layer".into(), json!(i));
    }
    let old_payload = serde_json::to_string(&old).expect("schema-1 payload");
    let crafted = with_table(&peeled, &table(2, &payload), &table(1, &old_payload));
    let reopened = open(&write(&dir, "oldfile.rz", &crafted))
        .expect("a pre-12E-1 file with schema-1 comps reopens");
    assert_eq!(comps(reopened)["comps"][0]["stale_count"], 0);
    unsafe { dr_doc_free(reopened) };
}

#[test]
fn the_schema_word_and_the_payload_key_must_agree_at_both_depths() {
    let dir = TempDir::new().unwrap();
    let doc = fixture(&dir);
    let doc = comp(
        doc,
        json!({"op": "layer_comp", "action": "save", "name": "A"}),
    );
    let bytes = save(doc, &dir.path().join("good.rz"));
    unsafe { dr_doc_free(doc) };
    let payload = payload_of(&bytes);
    let good_table = table(2, &payload);

    for (name, word, text) in [
        ("binary1", 1u32, payload.clone()),
        (
            "json1",
            2,
            payload.replace(r#""schema":2"#, r#""schema":1"#),
        ),
    ] {
        let crafted = with_table(&bytes, &good_table, &table(word, &text));
        let message = open(&write(&dir, &format!("{name}.rz"), &crafted))
            .err()
            .unwrap_or_else(|| panic!("{name} must refuse"));
        assert!(message.contains("layer comp schema"), "{name}: {message}");
    }

    // The typed reader makes exactly the same two refusals.
    let typed = typed_fixture();
    let typed = typed_edit(
        &typed,
        json!({"op": "layer_comp", "action": "save", "name": "A"}),
    );
    let bytes = typed.encode_native(LIMIT, 0, None).expect("bytes");
    let payload = payload_of(&bytes);
    let good_table = table(2, &payload);
    for (name, word, text) in [
        ("binary1", 1u32, payload.clone()),
        (
            "json1",
            2,
            payload.replace(r#""schema":2"#, r#""schema":1"#),
        ),
    ] {
        let crafted = with_table(&bytes, &good_table, &table(word, &text));
        let message = PrecisionDocument::parse_native(&crafted, LIMIT, 0, None)
            .err()
            .unwrap_or_else(|| panic!("typed {name} must refuse"));
        assert!(
            message.contains("layer comp schema"),
            "typed {name}: {message}"
        );
    }
}

#[test]
fn a_dangling_id_is_never_reminted_onto_a_new_layer() {
    // An id a comp row still names but no record carries must never come
    // back on some later layer, which would silently resurrect the row
    // against the wrong picture.
    let dir = TempDir::new().unwrap();
    let doc = fixture(&dir);
    let doc = comp(
        doc,
        json!({"op": "layer_comp", "action": "save", "name": "A"}),
    );
    let bytes = save(doc, &dir.path().join("good.rz"));
    unsafe { dr_doc_free(doc) };
    let payload = payload_of(&bytes);
    let good_table = table(2, &payload);

    // A very large id the process counter has certainly not reached.
    let huge = 9_000_000_000_000_000_000u64;
    let ids: Vec<u64> = serde_json::from_str::<Value>(&payload).expect("payload JSON")["comps"][0]
        ["entries"]
        .as_array()
        .expect("entries")
        .iter()
        .map(|e| e["id"].as_u64().expect("an id"))
        .collect();
    let crafted_payload = replace_comp_id(&payload, ids[1], &format!(r#""id":{huge}"#));
    assert_ne!(crafted_payload, payload);
    let crafted = with_table(&bytes, &good_table, &table(2, &crafted_payload));

    let reopened = open(&write(&dir, "dangling.rz", &crafted)).expect("it reopens");
    assert_eq!(
        comps(reopened)["comps"][0]["stale_count"],
        1,
        "the dangling id reopens stale"
    );
    // Every id minted from here on is ABOVE the one the file carried.
    let doc = fixture(&dir);
    for i in 0..unsafe { dr_doc_layer_count(doc) } {
        assert!(
            unsafe { dr_doc_layer_id(doc, i) } > huge,
            "the counter was raised past the file's largest id"
        );
    }
    unsafe { dr_doc_free(doc) };
    unsafe { dr_doc_free(reopened) };
}

// ------------------------------------------------- unrecorded on schema 1 --

/// `payload` (schema 2, one comp) rewritten in the schema-1 form: each `id`
/// becomes the `layer` index the old writer emitted, the row at `drop` (if
/// any) is removed, and the row at `rename` (if any) carries the name given.
fn schema_one(payload: &str, drop: Option<usize>, rename: Option<(usize, &str)>) -> String {
    let mut old = serde_json::from_str::<Value>(payload).expect("payload JSON");
    old["schema"] = json!(1);
    let entries = old["comps"][0]["entries"].as_array_mut().expect("entries");
    for (i, entry) in entries.iter_mut().enumerate() {
        let object = entry.as_object_mut().expect("an entry object");
        object.remove("id");
        object.insert("layer".into(), json!(i));
        if let Some((at, name)) = rename {
            if at == i {
                object.insert("name".into(), json!(name));
            }
        }
    }
    if let Some(at) = drop {
        entries.remove(at);
    }
    serde_json::to_string(&old).expect("schema-1 payload")
}

/// The one comp's `unrecorded` rows as (layer, id, name).
fn unrecorded_rows(listing: &Value) -> Vec<(u64, u64, String)> {
    listing["comps"][0]["unrecorded"]
        .as_array()
        .expect("unrecorded")
        .iter()
        .map(|row| {
            (
                row["layer"].as_u64().expect("a layer"),
                row["id"].as_u64().expect("an id"),
                row["name"].as_str().expect("a name").to_string(),
            )
        })
        .collect()
}

fn hide(doc: *mut DrDocument, idx: usize) -> *mut DrDocument {
    let out = unsafe { dr_doc_with_layer_visible(doc, idx, false) };
    assert!(!out.is_null(), "hiding {idx}");
    unsafe { dr_doc_free(doc) };
    out
}

fn shown(doc: *const DrDocument) -> Vec<bool> {
    (0..unsafe { dr_doc_layer_count(doc) })
        .map(|i| unsafe { dr_doc_layer_visible(doc, i) })
        .collect()
}

fn typed_shown(document: &PrecisionDocument) -> Vec<bool> {
    (0..document.layer_count())
        .map(|i| document.layer(i).expect("a record").visible())
        .collect()
}

const APPLY: &str = r#"{"op":"layer_comp","action":"apply","comp":0}"#;

#[test]
fn a_schema_one_table_with_no_row_for_a_layer_hides_it_on_apply() {
    // A layer no row names is UNRECORDED, so applying the comp
    // hides it, whatever schema the comp came from.
    let dir = TempDir::new().unwrap();
    let doc = comp(
        fixture(&dir),
        json!({"op": "layer_comp", "action": "save", "name": "A"}),
    );
    let modern = save(doc, &dir.path().join("modern.rz"));
    unsafe { dr_doc_free(doc) };
    let payload = payload_of(&modern);
    let crafted = with_table(
        &modern,
        &table(2, &payload),
        &table(1, &schema_one(&payload, Some(2), None)),
    );
    let reopened = open(&write(&dir, "norow.rz", &crafted)).expect("it reopens");
    let top = unsafe { dr_doc_layer_id(reopened, 2) };
    assert_eq!(
        unrecorded_rows(&comps(reopened)),
        vec![(2, top, "Top".to_string())]
    );
    let applied = comp(reopened, serde_json::from_str(APPLY).unwrap());
    assert_eq!(shown(applied), vec![true, true, false], "Top is hidden");
    unsafe { dr_doc_free(applied) };

    let typed = typed_edit(
        &typed_fixture(),
        json!({"op": "layer_comp", "action": "save", "name": "A"}),
    );
    let bytes = typed.encode_native(LIMIT, 0, None).expect("bytes");
    let payload = payload_of(&bytes);
    let crafted = with_table(
        &bytes,
        &table(2, &payload),
        &table(1, &schema_one(&payload, Some(2), None)),
    );
    let reopened = PrecisionDocument::parse_native(&crafted, LIMIT, 0, None).expect("reopen");
    let top = reopened.layer(2).expect("a record").id();
    assert_eq!(
        unrecorded_rows(&typed_comps(&reopened)),
        vec![(2, top, "Top".to_string())]
    );
    let applied = typed_edit(&reopened, serde_json::from_str(APPLY).unwrap());
    assert_eq!(typed_shown(&applied), vec![true, true, false]);
}

#[test]
fn a_schema_one_layer_renamed_after_save_stays_recorded_and_is_never_hidden() {
    // The renamed layer's row goes stale exactly as
    // before, but the layer itself is still RECORDED (a HOLD row with no
    // recorded keys), so apply leaves its visibility alone, before and after
    // a resave.
    let dir = TempDir::new().unwrap();
    let doc = comp(
        fixture(&dir),
        json!({"op": "layer_comp", "action": "save", "name": "A"}),
    );
    let modern = save(doc, &dir.path().join("modern.rz"));
    unsafe { dr_doc_free(doc) };
    let payload = payload_of(&modern);
    let crafted = with_table(
        &modern,
        &table(2, &payload),
        &table(1, &schema_one(&payload, None, Some((1, "Was Middle")))),
    );
    let reopened = open(&write(&dir, "renamed.rz", &crafted)).expect("it reopens");
    let middle = unsafe { dr_doc_layer_id(reopened, 1) };
    let listing = comps(reopened);
    assert_eq!(listing["comps"][0]["stale_count"], 1, "the old rule holds");
    assert_eq!(unrecorded_rows(&listing), vec![], "Middle is recorded");
    let hold = listing["comps"][0]["entries"]
        .as_array()
        .expect("entries")
        .iter()
        .find(|e| e["stale"] == json!(false) && e["id"] == json!(middle))
        .expect("a bound row names Middle");
    assert_eq!(hold["layer"], json!(1));
    assert!(hold.get("visible").is_none() && hold.get("opacity").is_none());

    let hidden = hide(hide(reopened, 0), 1);
    let applied = comp(hidden, serde_json::from_str(APPLY).unwrap());
    assert_eq!(
        shown(applied),
        vec![true, false, true],
        "Base comes back, Middle is left as it is"
    );

    let resaved = save(applied, &dir.path().join("resaved.rz"));
    unsafe { dr_doc_free(applied) };
    let again = open(&write(&dir, "resaved2.rz", &resaved)).expect("it reopens again");
    let listing = comps(again);
    assert_eq!(listing["comps"][0]["stale_count"], 1);
    assert_eq!(unrecorded_rows(&listing), vec![]);
    let applied = comp(hide(again, 0), serde_json::from_str(APPLY).unwrap());
    assert_eq!(shown(applied), vec![true, false, true]);
    unsafe { dr_doc_free(applied) };

    let typed = typed_edit(
        &typed_fixture(),
        json!({"op": "layer_comp", "action": "save", "name": "A"}),
    );
    let bytes = typed.encode_native(LIMIT, 0, None).expect("bytes");
    let payload = payload_of(&bytes);
    let crafted = with_table(
        &bytes,
        &table(2, &payload),
        &table(1, &schema_one(&payload, None, Some((1, "Was Middle")))),
    );
    let reopened = PrecisionDocument::parse_native(&crafted, LIMIT, 0, None).expect("reopen");
    let listing = typed_comps(&reopened);
    assert_eq!(listing["comps"][0]["stale_count"], 1);
    assert_eq!(unrecorded_rows(&listing), vec![]);
    let hidden = typed_edit(
        &reopened,
        json!({"op": "layer_visible", "layer": 0, "visible": false}),
    );
    let hidden = typed_edit(
        &hidden,
        json!({"op": "layer_visible", "layer": 1, "visible": false}),
    );
    let applied = typed_edit(&hidden, serde_json::from_str(APPLY).unwrap());
    assert_eq!(typed_shown(&applied), vec![true, false, true]);
    let bytes = applied.encode_native(LIMIT, 0, None).expect("resave");
    let again = PrecisionDocument::parse_native(&bytes, LIMIT, 0, None).expect("reopen");
    let listing = typed_comps(&again);
    assert_eq!(listing["comps"][0]["stale_count"], 1);
    assert_eq!(unrecorded_rows(&listing), vec![]);
    let hidden = typed_edit(
        &again,
        json!({"op": "layer_visible", "layer": 0, "visible": false}),
    );
    let applied = typed_edit(&hidden, serde_json::from_str(APPLY).unwrap());
    assert_eq!(typed_shown(&applied), vec![true, false, true]);
}
