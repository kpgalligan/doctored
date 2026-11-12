//! PATHS in `.rz` — byte word 74 and typed word 75, the `PATH` table,
//! its refusals, the document anchor cap at the size the table was sized
//! for, and the flat-save loss. Through the C API's save and open.
//!
//! The table's framing is read here by this file's own walk (`path_table`):
//! the tag, the schema, the JSON length and the plane count, located by the
//! one position at which they end exactly where `LIDS` begins. The JSON is
//! compared as a value against the list the document reported, never against
//! text the core printed. The table's VECTOR MASK section — records, planes
//! and their refusals — is `vector_masks.rs`'s, which shares the `pub`
//! helpers here; every table in this file has none, which `path_table`
//! asserts.

use std::ptr;

use doctored_core::doc::DrDocument;
use doctored_core::doc_sampler::ColorSampler;
use doctored_core::ffi_doc::{dr_doc_open, dr_doc_save_native};
use doctored_core::ffi_precision::{dr_precision_doc_open, dr_precision_doc_save_native};
use doctored_core::ffi_preservation::dr_doc_flat_save_losses;
use doctored_core::precision_document::PrecisionDocument;
use serde_json::{json, Value};
use tempfile::TempDir;

use crate::common::{cpath, drdc_word, identity_tail, take_err_string};
use crate::path_geometry::{
    byte, byte_paths, changed, entries, float, ids, polygon, refused, saved, set_byte, set_typed,
    start, typed, typed_paths, with_byte, with_typed, without_ids, LIMIT,
};

const W: u32 = 64;
const H: u32 = 48;

// ---------------------------------------------------------------- helpers --

pub fn save_byte(dir: &TempDir, name: &str, doc: &DrDocument) -> Vec<u8> {
    let path = dir.path().join(name);
    let mut error = ptr::null_mut();
    let saved =
        unsafe { dr_doc_save_native(doc, cpath(&path).as_ptr(), std::ptr::null(), &mut error) };
    assert!(saved, "{}", take_err_string(error));
    std::fs::read(path).expect("read back")
}

pub fn open_byte(dir: &TempDir, name: &str, bytes: &[u8]) -> Result<DrDocument, String> {
    let path = dir.path().join(name);
    std::fs::write(&path, bytes).expect("write");
    let mut error = ptr::null_mut();
    let out = unsafe { dr_doc_open(cpath(&path).as_ptr(), std::ptr::null(), &mut error) };
    if out.is_null() {
        return Err(take_err_string(error));
    }
    Ok(*unsafe { Box::from_raw(out) })
}

fn save_typed(dir: &TempDir, name: &str, doc: &PrecisionDocument) -> Vec<u8> {
    let path = dir.path().join(name);
    let mut error = ptr::null_mut();
    let saved = unsafe {
        dr_precision_doc_save_native(
            doc,
            cpath(&path).as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut error,
        )
    };
    assert!(saved, "{}", take_err_string(error));
    std::fs::read(path).expect("read back")
}

fn open_typed(dir: &TempDir, name: &str, bytes: &[u8]) -> Result<PrecisionDocument, String> {
    let path = dir.path().join(name);
    std::fs::write(&path, bytes).expect("write");
    let mut error = ptr::null_mut();
    let out =
        unsafe { dr_precision_doc_open(cpath(&path).as_ptr(), LIMIT, 0, ptr::null(), &mut error) };
    if out.is_null() {
        return Err(take_err_string(error));
    }
    Ok(*unsafe { Box::from_raw(out) })
}

fn u32_at(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap())
}

/// Where `LIDS` starts in a file of `records` records.
fn lids_at(bytes: &[u8], records: usize) -> usize {
    let tail = bytes.len() - identity_tail(records);
    assert_eq!(&bytes[tail..tail + 4], b"LIDS", "LIDS stays LAST");
    tail
}

/// The file's `PATH` table: its offset and its JSON. It is the one `PATH`
/// tag whose schema is 1 and whose length and plane count end exactly at
/// `LIDS` — so finding it also proves it is the last table before `LIDS`.
pub fn path_table(bytes: &[u8], records: usize) -> (usize, Value) {
    let tail = lids_at(bytes, records);
    let found: Vec<usize> = (0..tail.saturating_sub(16))
        .filter(|&at| {
            &bytes[at..at + 4] == b"PATH"
                && u32_at(bytes, at + 4) == 1
                && at + 16 + u32_at(bytes, at + 8) as usize == tail
        })
        .collect();
    assert_eq!(found.len(), 1, "one PATH table, ending where LIDS begins");
    let at = found[0];
    assert_eq!(u32_at(bytes, tail - 4), 0, "no mask plane");
    let json = serde_json::from_slice(&bytes[at + 12..tail - 4]).expect("the table's JSON");
    (at, json)
}

/// `bytes` with its `PATH` table replaced by one holding `text` and `planes`.
pub fn with_table(bytes: &[u8], records: usize, text: &str, planes: u32) -> Vec<u8> {
    let (at, _) = path_table(bytes, records);
    let tail = lids_at(bytes, records);
    let mut out = bytes[..at].to_vec();
    out.extend(b"PATH");
    out.extend(1u32.to_le_bytes());
    out.extend((text.len() as u32).to_le_bytes());
    out.extend(text.as_bytes());
    out.extend(planes.to_le_bytes());
    out.extend(&bytes[tail..]);
    out
}

/// What the table must hold for a document reporting `list`.
fn expected_table(list: &Value) -> Value {
    json!({"paths": without_ids(list)["paths"], "vector_masks": []})
}

// -------------------------------------------------------- the files (T1) --

#[test]
fn byte_rz_writes_word_74_and_the_path_table_only_for_paths() {
    let dir = TempDir::new().unwrap();
    let base = byte(W, H);
    let plain = save_byte(&dir, "plain.rz", &base);
    assert_eq!(drdc_word(&plain, 0), 45);
    assert_ne!(drdc_word(&plain, 1), 74);
    assert!(!plain.windows(4).any(|w| w == b"PATH"));

    let set = with_byte(&base, &start());
    let stored = byte_paths(&set);
    let bytes = save_byte(&dir, "set.rz", &set);
    assert_eq!((drdc_word(&bytes, 0), drdc_word(&bytes, 1)), (45, 74));
    let (_, table) = path_table(&bytes, 1);
    assert_eq!(table, expected_table(&stored), "the list, with no id");
    // Nothing but the word and the table was added.
    let (at, _) = path_table(&bytes, 1);
    let tail = lids_at(&bytes, 1);
    let mut stripped = bytes[..8].to_vec();
    stripped.extend(&bytes[12..at]);
    stripped.extend(&bytes[tail..]);
    assert_eq!(stripped, plain, "a path adds one word and one table");

    let reopened = open_byte(&dir, "again.rz", &bytes).expect("reopen");
    let reread = byte_paths(&reopened);
    assert_eq!(without_ids(&reread), without_ids(&stored));
    assert!(
        ids(&reread).iter().all(|id| *id != 0),
        "ids are minted on load"
    );
    assert_eq!(
        save_byte(&dir, "resave.rz", &reopened),
        bytes,
        "reopen → resave is byte-identical"
    );

    let cleared = with_byte(&set, &json!({"paths": []}));
    assert_eq!(
        save_byte(&dir, "cleared.rz", &cleared),
        plain,
        "deleting the last path restores the path-free bytes"
    );
}

#[test]
fn typed_rz_writes_word_75_at_16_bits_and_at_float() {
    let dir = TempDir::new().unwrap();
    for (name, base) in [("16", typed(W, H)), ("float", float(W, H))] {
        let records = base.layer_count();
        let plain = save_typed(&dir, &format!("plain-{name}.rz"), &base);
        assert_eq!(drdc_word(&plain, 0), 46, "{name}");
        assert_ne!(drdc_word(&plain, 1), 75, "{name}");

        let set = with_typed(&base, &start());
        let stored = typed_paths(&set);
        let bytes = save_typed(&dir, &format!("set-{name}.rz"), &set);
        assert_eq!(
            (drdc_word(&bytes, 0), drdc_word(&bytes, 1)),
            (46, 75),
            "{name}"
        );
        let (at, table) = path_table(&bytes, records);
        assert_eq!(table, expected_table(&stored), "{name}");
        let tail = lids_at(&bytes, records);
        let mut stripped = bytes[..8].to_vec();
        stripped.extend(&bytes[12..at]);
        stripped.extend(&bytes[tail..]);
        assert_eq!(stripped, plain, "{name}: one word and one table");

        let reopened = open_typed(&dir, &format!("again-{name}.rz"), &bytes).expect("reopen");
        assert_eq!(reopened.is_float(), name == "float");
        assert_eq!(
            without_ids(&typed_paths(&reopened)),
            without_ids(&stored),
            "{name}"
        );
        assert_eq!(
            save_typed(&dir, &format!("resave-{name}.rz"), &reopened),
            bytes,
            "{name}: reopen → resave is byte-identical"
        );
        let cleared = with_typed(&set, &json!({"paths": []}));
        assert_eq!(
            save_typed(&dir, &format!("cleared-{name}.rz"), &cleared),
            plain,
            "{name}"
        );
        // A crafted table refuses the typed open too.
        let bad = with_table(&bytes, records, &expected_table(&stored).to_string(), 1);
        let message = open_typed(&dir, &format!("bad-{name}.rz"), &bad)
            .err()
            .expect("refused");
        assert!(message.contains("vector mask section"), "{name}: {message}");
    }
}

#[test]
fn the_paths_word_leads_the_other_conditional_words() {
    let dir = TempDir::new().unwrap();
    let sampler = [ColorSampler {
        x: 3,
        y: 4,
        sample_size: 3,
    }];
    // Byte: 45, 74, 59 and … CSMP → PATH → LIDS.
    let b = with_byte(&byte(W, H), &start())
        .with_color_samplers(&sampler)
        .expect("a sampler");
    let bytes = save_byte(&dir, "both.rz", &b);
    assert_eq!(
        (
            drdc_word(&bytes, 0),
            drdc_word(&bytes, 1),
            drdc_word(&bytes, 2)
        ),
        (45, 74, 59)
    );
    let (at, _) = path_table(&bytes, 1);
    assert_eq!(&bytes[at - 21..at - 17], b"CSMP", "CSMP → PATH → LIDS");
    let reopened = open_byte(&dir, "both-again.rz", &bytes).expect("reopen");
    assert_eq!(reopened.color_samplers(), &sampler);
    assert_eq!(
        without_ids(&byte_paths(&reopened)),
        without_ids(&byte_paths(&b))
    );

    // Typed: 46, 75, 60.
    let t = with_typed(&typed(W, H), &start())
        .with_color_samplers(&sampler)
        .expect("a sampler");
    let bytes = save_typed(&dir, "both16.rz", &t);
    assert_eq!(
        (
            drdc_word(&bytes, 0),
            drdc_word(&bytes, 1),
            drdc_word(&bytes, 2)
        ),
        (46, 75, 60)
    );
    let (at, _) = path_table(&bytes, 1);
    assert_eq!(&bytes[at - 21..at - 17], b"CSMP");
    let reopened = open_typed(&dir, "both16-again.rz", &bytes).expect("reopen");
    assert_eq!(reopened.color_samplers(), &sampler);
    assert_eq!(
        without_ids(&typed_paths(&reopened)),
        without_ids(&typed_paths(&t))
    );
}

// --------------------------------------------------- crafted tables (T2) --

#[test]
fn a_crafted_path_table_refuses_before_it_is_believed() {
    let dir = TempDir::new().unwrap();
    let set = with_byte(&byte(W, H), &start());
    let good = save_byte(&dir, "good.rz", &set);
    let (at, table) = path_table(&good, 1);
    let tail = lids_at(&good, 1);
    let patched = |offset: usize, value: &[u8]| {
        let mut bytes = good.clone();
        bytes[at + offset..at + offset + value.len()].copy_from_slice(value);
        bytes
    };
    let rebuilt = |json: Value, planes: u32| with_table(&good, 1, &json.to_string(), planes);
    let corner = json!([polygon(&[[1.0, 1.0]], false)]);
    let many: Vec<Value> = (0..257)
        .map(|i| saved(&format!("p{i}"), json!([])))
        .collect();
    let mut without_word = good[..8].to_vec();
    without_word.extend(&good[12..]);
    let mut without_table = good[..at].to_vec();
    without_table.extend(&good[tail..]);
    let cases: Vec<(Vec<u8>, &str, &str)> = vec![
        (patched(0, b"XXXX"), "a bad tag", "tag or schema"),
        (
            patched(4, &2u32.to_le_bytes()),
            "a bad schema",
            "tag or schema",
        ),
        (patched(8, &0u32.to_le_bytes()), "a zero length", "length"),
        (
            patched(8, &(16 * 1024 * 1024 + 1u32).to_le_bytes()),
            "a length past the cap",
            "length",
        ),
        (
            with_table(&good, 1, "{not json", 0),
            "malformed JSON",
            "path table json",
        ),
        (
            rebuilt(table.clone(), 1),
            "a mask plane",
            "vector mask section",
        ),
        (
            rebuilt(json!({"paths": table["paths"], "vector_masks": [{}]}), 0),
            "a vector mask record",
            "vector mask section",
        ),
        (
            rebuilt(json!({"paths": table["paths"]}), 0),
            "no vector_masks array",
            "vector_masks",
        ),
        (
            rebuilt(json!({"vector_masks": []}), 0),
            "no paths array",
            "paths array",
        ),
        (
            rebuilt(json!({"paths": [], "vector_masks": []}), 0),
            "an empty table",
            "empty",
        ),
        (
            rebuilt(
                json!({"paths": [saved("x", json!([{"anchors": []}]))], "vector_masks": []}),
                0,
            ),
            "an anchorless subpath",
            "at least one anchor",
        ),
        (
            rebuilt(
                json!({"paths": [saved("x", json!([polygon(&[[1e7, 0.0]], false)]))],
                       "vector_masks": []}),
                0,
            ),
            "a coordinate past the wall",
            "outside",
        ),
        (
            rebuilt(
                json!({"paths": [{"kind": "work", "subpaths": []},
                                 {"kind": "work", "subpaths": []}],
                       "vector_masks": []}),
                0,
            ),
            "two work paths",
            "at most one work path",
        ),
        (
            rebuilt(
                json!({"paths": [saved("Twin", corner.clone()), saved("Twin", corner.clone())],
                       "vector_masks": []}),
                0,
            ),
            "a repeated name",
            "two paths are named",
        ),
        (
            rebuilt(json!({"paths": many, "vector_masks": []}), 0),
            "257 paths",
            "too many paths",
        ),
        (without_table, "the word with no table", "tag or schema"),
    ];
    for (bytes, what, sentence) in cases {
        let message = match open_byte(&dir, "bad.rz", &bytes) {
            Err(message) => message,
            Ok(_) => panic!("{what} must refuse"),
        };
        assert!(message.contains(sentence), "{what}: {message}");
    }
    assert!(
        open_byte(&dir, "wordless.rz", &without_word).is_err(),
        "a table with no word is not skipped"
    );

    // An id in the file is not adopted, and a file's order is canonicalized:
    // the work path leads whatever the JSON says.
    let foreign = 4_242_424_242_424_242u64;
    let crafted = rebuilt(
        json!({"paths": [
            {"kind": "saved", "name": "Kept", "id": foreign, "subpaths": corner},
            {"kind": "work", "name": "ignored", "subpaths": []},
        ], "vector_masks": []}),
        0,
    );
    let opened = byte_paths(&open_byte(&dir, "foreign.rz", &crafted).expect("opens"));
    assert!(!ids(&opened).contains(&foreign));
    assert_eq!(entries(&opened)[0]["name"], "Work Path");
    assert_eq!(entries(&opened)[1]["name"], "Kept");
}

// ---------------------------------------------------- the anchor cap (T3) --

/// 65,536 anchors in the form that prints longest: every coordinate at
/// −999999.9999 and every anchor in a subpath of its own (16 paths of 4,096
/// one-anchor subpaths).
fn longest_list() -> Vec<Value> {
    let v = -999999.9999;
    let subpaths = vec![
        json!({"closed": false, "anchors": [
            {"x": v, "y": v, "in": [v, v], "out": [v, v], "smooth": false},
        ]});
        4096
    ];
    (0..16)
        .map(|i| saved(&format!("path {i}"), json!(subpaths)))
        .collect()
}

const CAP_SENTENCE: &str = "too many path anchors in the document (max 65536)";

#[test]
fn a_document_at_the_anchor_cap_saves_and_reopens_and_one_more_anchor_refuses() {
    let dir = TempDir::new().unwrap();
    let full = longest_list();
    let mut over = full.clone();
    over.push(saved("one more", json!([polygon(&[[0.0, 0.0]], false)])));
    let full = json!({ "paths": full });
    let over = json!({ "paths": over });

    let b = with_byte(&byte(4, 4), &full);
    assert_eq!(
        b.paths()
            .iter()
            .map(|p| p.shape.anchor_count())
            .sum::<usize>(),
        65_536
    );
    let stored = byte_paths(&b);
    let bytes = save_byte(&dir, "full.rz", &b);
    let (at, table) = path_table(&bytes, 1);
    assert_eq!(table, expected_table(&stored));
    // This is the size the cap was chosen for: 117 bytes of anchor and 29 of
    // subpath wrapper before any separator, inside the 16 MiB length slot,
    // and inside the writer's own plan.
    let length = u32_at(&bytes, at + 8) as usize;
    assert!(length >= 65_536 * 146, "{length}");
    assert!(length <= 16 * 1024 * 1024, "{length}");
    assert!(b.native_save_plan().expect("a plan").encoded_bound >= bytes.len() as u64);
    let reopened = open_byte(&dir, "full-again.rz", &bytes).expect("reopen");
    assert_eq!(without_ids(&byte_paths(&reopened)), without_ids(&stored));
    assert_eq!(refused(set_byte(&byte(4, 4), &over)), CAP_SENTENCE);
    // One more anchor on the full document refuses the same way.
    let mut grown = stored.clone();
    grown["paths"][0]["subpaths"][0]["anchors"]
        .as_array_mut()
        .unwrap()
        .push(json!([1.0, 2.0]));
    assert_eq!(refused(set_byte(&b, &grown)), CAP_SENTENCE);

    let t = with_typed(&typed(4, 4), &full);
    let stored16 = typed_paths(&t);
    let bytes = save_typed(&dir, "full16.rz", &t);
    assert_eq!(path_table(&bytes, 1).1, expected_table(&stored16));
    let reopened = open_typed(&dir, "full16-again.rz", &bytes).expect("reopen");
    assert_eq!(without_ids(&typed_paths(&reopened)), without_ids(&stored16));
    assert_eq!(refused(set_typed(&typed(4, 4), &over)), CAP_SENTENCE);

    // A geometry op keeps every one of them, and the result still saves.
    let mut turned = changed(Ok::<_, String>(b.rotate90()));
    assert_eq!(turned.paths().len(), 16);
    // The turn also maps the layer's reference point ((0, 0) -> (4, 0)),
    // whose `LREF` table would sit between `PATH` and `LIDS`; put it back so
    // `path_table` still finds `PATH` last.
    assert_eq!(turned.layers[0].reference, (4, 0));
    turned.layers[0].reference = (0, 0);
    let turned_bytes = save_byte(&dir, "full-turned.rz", &turned);
    assert_eq!(
        path_table(&turned_bytes, 1).1,
        expected_table(&byte_paths(&turned))
    );
}

// ---------------------------------------------------- the flat save (T4) --

#[test]
fn a_flat_save_names_the_paths_only_when_there_are_some() {
    let losses = |doc: &DrDocument| -> Vec<String> {
        let mut error = ptr::null_mut();
        let text = unsafe { dr_doc_flat_save_losses(doc, 0, true, false, &mut error) };
        assert!(!text.is_null(), "{}", take_err_string(error));
        serde_json::from_str(&take_err_string(text)).expect("a JSON array")
    };
    let plain = byte(W, H);
    assert!(!losses(&plain).iter().any(|l| l == "paths"));
    let set = with_byte(&plain, &start());
    assert!(losses(&set).iter().any(|l| l == "paths"));
}
