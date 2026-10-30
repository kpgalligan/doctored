//! #1 Indexed Color phase 1c's Image ▸ Mode ▸ Color Table through the FFI:
//! the table read and replaced (every index kept, the one layer re-rendered
//! as `palette[i] × 257`), its plan and refusals, and Adobe's `.act` file
//! against hand-built bytes (768 and 772 bytes, a transparent index and
//! `0xFFFF`), the writer's bytes and the round trip.

use std::ffi::{CStr, CString};
use std::ptr;

use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_indexed::{
    dr_indexed_read_act, dr_indexed_write_act, dr_precision_doc_set_color_table_plan,
};
use serde_json::{json, Value};
use tempfile::TempDir;

use crate::common::{cpath, take_err_string};
use crate::indexed_fixture::{
    color_table, free, indices, layer16, opaque, render16, rgb_doc, set_table, to_indexed,
};

const SHORT: &str =
    "The colour table holds 2 colours, but the image uses index 3; it needs at least 4.";
const TABLE_SIZE: &str = "a colour table holds 2 to 256 colours";

/// Four colours in a 4×1 RGB document, indexed Exact (no dither): palette =
/// the colours in order, index i at pixel i.
const COLORS: [[u8; 3]; 4] = [[200, 10, 10], [10, 200, 10], [10, 10, 200], [90, 90, 90]];

fn indexed_four() -> (
    *mut doctored_core::precision_document::PrecisionDocument,
    *mut doctored_core::precision_document::PrecisionDocument,
) {
    let rgb = rgb_doc(4, 1, &opaque(&COLORS));
    let indexed = to_indexed(rgb, &json!({"palette":"exact","dither":"none"})).unwrap();
    (rgb, indexed)
}

fn plan(
    doc: *const doctored_core::precision_document::PrecisionDocument,
    table: &Value,
) -> Result<Value, String> {
    let text = CString::new(table.to_string()).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe { dr_precision_doc_set_color_table_plan(doc, text.as_ptr(), &mut err) };
    if out.is_null() {
        return Err(take_err_string(err));
    }
    let value = serde_json::from_str(unsafe { CStr::from_ptr(out) }.to_str().unwrap()).unwrap();
    unsafe { dr_string_free(out) };
    Ok(value)
}

fn read_act(bytes: &[u8]) -> Result<Value, String> {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("table.act");
    std::fs::write(&path, bytes).unwrap();
    let c = cpath(&path);
    let mut err = ptr::null_mut();
    let out = unsafe { dr_indexed_read_act(c.as_ptr(), &mut err) };
    if out.is_null() {
        return Err(take_err_string(err));
    }
    let value = serde_json::from_str(unsafe { CStr::from_ptr(out) }.to_str().unwrap()).unwrap();
    unsafe { dr_string_free(out) };
    Ok(value)
}

fn write_act(table: &Value) -> Result<Vec<u8>, String> {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("out.act");
    let c = cpath(&path);
    let text = CString::new(table.to_string()).unwrap();
    let mut err = ptr::null_mut();
    if !unsafe { dr_indexed_write_act(c.as_ptr(), text.as_ptr(), &mut err) } {
        return Err(take_err_string(err));
    }
    Ok(std::fs::read(&path).unwrap())
}

/// A hand-built `.act`: `entries` triples, zero-padded to 768 bytes, then the
/// trailer when given (count, transparent; big-endian).
fn act_bytes(entries: &[[u8; 3]], trailer: Option<(u16, u16)>) -> Vec<u8> {
    let mut out: Vec<u8> = entries.iter().flatten().copied().collect();
    out.resize(768, 0);
    if let Some((count, transparent)) = trailer {
        out.extend_from_slice(&count.to_be_bytes());
        out.extend_from_slice(&transparent.to_be_bytes());
    }
    out
}

#[test]
fn hand_built_act_files_parse_to_the_tables_they_hold() {
    // 768 bytes: all 256 entries, no transparent index.
    let full: Vec<[u8; 3]> = (0..=255u8).map(|i| [i, 255 - i, i / 2]).collect();
    let table = read_act(&act_bytes(&full, None)).unwrap();
    assert_eq!(table["colors"].as_array().unwrap().len(), 256);
    assert_eq!(table["colors"][0], json!([0, 255, 0]));
    assert_eq!(table["colors"][255], json!([255, 0, 127]));
    assert_eq!(table["colors"], json!(full));
    assert!(table["transparent_index"].is_null());
    // 772 bytes: count 3, transparent index 1; the bytes past the count are
    // not part of the table.
    let three = [[1, 2, 3], [4, 5, 6], [7, 8, 9], [99, 99, 99]];
    let table = read_act(&act_bytes(&three, Some((3, 1)))).unwrap();
    assert_eq!(
        table,
        json!({"colors":[[1,2,3],[4,5,6],[7,8,9]],"transparent_index":1})
    );
    // 0xFFFF is no transparent index.
    let table = read_act(&act_bytes(&three, Some((4, 0xFFFF)))).unwrap();
    assert_eq!(
        table,
        json!({"colors":[[1,2,3],[4,5,6],[7,8,9],[99,99,99]],"transparent_index":null})
    );
    // 256 with a trailer.
    let table = read_act(&act_bytes(&full, Some((256, 255)))).unwrap();
    assert_eq!(table["transparent_index"], 255);
}

#[test]
fn bad_act_files_refuse() {
    for length in [0usize, 767, 769, 771, 773, 1024] {
        let error = read_act(&vec![0u8; length]).unwrap_err();
        assert!(
            error.contains(&format!("is 768 or 772 bytes, not {length}")),
            "{length}: {error}"
        );
    }
    let three = [[1, 2, 3], [4, 5, 6], [7, 8, 9]];
    for count in [0u16, 1, 257, 0xFFFF] {
        let error = read_act(&act_bytes(&three, Some((count, 0xFFFF)))).unwrap_err();
        assert!(
            error.contains(&format!("holds {count} colours")),
            "{count}: {error}"
        );
    }
    let error = read_act(&act_bytes(&three, Some((3, 3)))).unwrap_err();
    assert!(
        error.contains("transparent index 3 is outside its 3 colours"),
        "{error}"
    );
    // A missing file names its path.
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("missing.act");
    let c = cpath(&path);
    let mut err = ptr::null_mut();
    assert!(unsafe { dr_indexed_read_act(c.as_ptr(), &mut err) }.is_null());
    let error = take_err_string(err);
    assert!(error.starts_with("failed to read "), "{error}");
    assert!(error.contains("missing.act"), "{error}");
}

#[test]
fn the_writer_writes_772_bytes_and_round_trips() {
    let table = json!({"colors":[[1,2,3],[4,5,6],[250,251,252]],"transparent_index":2});
    let bytes = write_act(&table).unwrap();
    let expected = act_bytes(&[[1, 2, 3], [4, 5, 6], [250, 251, 252]], Some((3, 2)));
    assert_eq!(bytes, expected);
    assert_eq!(read_act(&bytes).unwrap(), table);
    // No transparent index: 0xFFFF; a full table round trips too.
    let full: Vec<[u8; 3]> = (0..=255u8).map(|i| [i, i, 255 - i]).collect();
    let table = json!({"colors": full, "transparent_index": null});
    let bytes = write_act(&table).unwrap();
    assert_eq!(bytes, act_bytes(&full, Some((256, 0xFFFF))));
    assert_eq!(read_act(&bytes).unwrap(), table);
    // An absent transparent_index is none.
    let bytes = write_act(&json!({"colors":[[0,0,0],[255,255,255]]})).unwrap();
    assert_eq!(&bytes[768..], &[0, 2, 0xFF, 0xFF]);
    // Malformed tables refuse before writing.
    assert_eq!(
        write_act(&json!({"colors":[[0,0,0]]})).unwrap_err(),
        TABLE_SIZE
    );
    assert!(write_act(&json!({"colors":[[0,0,0],[1,1,1]],"count":2}))
        .unwrap_err()
        .contains("unknown key \"count\""));
    assert!(write_act(&json!({"colors":[[0,0,0],[1,1,256]]}))
        .unwrap_err()
        .contains("[r, g, b] codes 0 to 255"));
    assert!(
        write_act(&json!({"colors":[[0,0,0],[1,1,1]],"transparent_index":2}))
            .unwrap_err()
            .contains("outside the table")
    );
}

#[test]
fn the_table_is_read_and_replaced_keeping_every_index() {
    let (rgb, indexed) = indexed_four();
    assert_eq!(
        color_table(indexed),
        json!({"colors": COLORS, "transparent_index": null})
    );
    let before = indices(indexed);
    assert_eq!(before, vec![0, 1, 2, 3]);
    // Entry 0 → (1, 2, 3): the plan says it changes, the result keeps the
    // plane and re-renders palette[i] × 257.
    let mut colors = COLORS.to_vec();
    colors[0] = [1, 2, 3];
    let table = json!({"colors": colors, "transparent_index": null});
    let answer = plan(indexed, &table).unwrap();
    assert_eq!(answer["changes"], true);
    assert!(answer["refusal"].is_null());
    assert!(answer["working_memory_plan_bytes"].as_u64().unwrap() > 4 * 41);
    let edited = set_table(indexed, &table).unwrap().unwrap();
    assert_eq!(color_table(edited), table);
    assert_eq!(indices(edited), before);
    let expected: Vec<[u16; 4]> = colors
        .iter()
        .map(|c| [c[0], c[1], c[2]].map(|v| u16::from(v) * 257))
        .map(|c| [c[0], c[1], c[2], 65535])
        .collect();
    assert_eq!(layer16(edited), expected);
    assert_eq!(render16(edited), expected);
    // The source is unchanged (purity).
    assert_eq!(color_table(indexed)["colors"][0], json!([200, 10, 10]));
    // A transparent index: its pixels' alpha is 0, the colour kept in the layer.
    let clear = json!({"colors": colors, "transparent_index": 2});
    let cleared = set_table(edited, &clear).unwrap().unwrap();
    assert_eq!(color_table(cleared), clear);
    let layer = layer16(cleared);
    assert_eq!(layer[2], [10 * 257, 10 * 257, 200 * 257, 0]);
    assert_eq!(layer[1][3], 65535);
    // The same table again is NULL with no error, and the plan says no change.
    assert_eq!(set_table(cleared, &clear).unwrap(), None);
    assert_eq!(plan(cleared, &clear).unwrap()["changes"], false);
    // A longer table keeps every index too.
    let mut longer = colors.clone();
    longer.push([5, 5, 5]);
    let grown = set_table(edited, &json!({"colors": longer}))
        .unwrap()
        .unwrap();
    assert_eq!(indices(grown), before);
    assert_eq!(color_table(grown)["colors"].as_array().unwrap().len(), 5);
    for doc in [grown, cleared, edited, indexed, rgb] {
        free(doc);
    }
}

#[test]
fn a_short_table_and_a_non_indexed_document_refuse() {
    let (rgb, indexed) = indexed_four();
    let short = json!({"colors":[[0,0,0],[255,255,255]]});
    assert_eq!(set_table(indexed, &short).unwrap_err(), SHORT);
    let answer = plan(indexed, &short).unwrap();
    assert_eq!(answer["refusal"], SHORT);
    assert_eq!(answer["changes"], false);
    // The plane uses index 3, so four entries are enough.
    let four = json!({"colors":[[0,0,0],[1,1,1],[2,2,2],[3,3,3]]});
    free(set_table(indexed, &four).unwrap().unwrap());
    // Size and shape.
    assert_eq!(
        set_table(indexed, &json!({"colors":[[0,0,0]]})).unwrap_err(),
        TABLE_SIZE
    );
    let too_many: Vec<[u8; 3]> = vec![[0, 0, 0]; 257];
    assert_eq!(
        set_table(indexed, &json!({ "colors": too_many })).unwrap_err(),
        TABLE_SIZE
    );
    assert!(plan(indexed, &json!({"colors":[[0,0,0]]})).is_err());
    // An RGB document.
    assert_eq!(
        set_table(rgb, &four).unwrap_err(),
        "the document is not Indexed Color"
    );
    assert_eq!(
        plan(rgb, &four).unwrap()["refusal"],
        "the document is not Indexed Color"
    );
    let mut err = ptr::null_mut();
    let out =
        unsafe { doctored_core::ffi_indexed::dr_precision_doc_indexed_color_table(rgb, &mut err) };
    assert!(out.is_null());
    assert_eq!(take_err_string(err), "the document is not Indexed Color");
    free(indexed);
    free(rgb);
}

#[test]
fn a_loaded_act_replaces_the_table() {
    // The file's table, through JSON, is what the document takes.
    let (rgb, indexed) = indexed_four();
    let entries = [[9, 8, 7], [6, 5, 4], [3, 2, 1], [0, 0, 0], [255, 255, 255]];
    let table = read_act(&act_bytes(&entries, Some((5, 4)))).unwrap();
    let loaded = set_table(indexed, &table).unwrap().unwrap();
    assert_eq!(color_table(loaded), table);
    assert_eq!(
        layer16(loaded)[0],
        [9 * 257, 8 * 257, 7 * 257, 65535],
        "pixel 0 keeps index 0"
    );
    // And the document's table saves to the same bytes.
    assert_eq!(
        write_act(&color_table(loaded)).unwrap(),
        act_bytes(&entries, Some((5, 4)))
    );
    free(loaded);
    free(indexed);
    free(rgb);
}
