//! #1 Duotone phase 1a in `.rz` (word 109 and its `DUOT` table after
//! `GRAY`): a Duotone document of one to four inks, with blank curve slots
//! and authored overprints, reopens with equal options, descriptor and
//! render, and a re-save is byte-equal to the first file; the reader refuses
//! a bad ink count, a float body under the word and the word without its
//! table by name (byte-patched files, as `indexed_native.rs` does).

use std::path::Path;
use std::ptr;

use doctored_core::ffi_precision::{dr_precision_doc_open, dr_precision_doc_save_native};
use doctored_core::precision_document::PrecisionDocument;
use serde_json::{json, Value};
use tempfile::TempDir;

use crate::common::{cpath, take_err_string};
use crate::duotone_document::{descriptor, duotone, free, gray_doc, ink, options, render16, LIMIT};

fn save(doc: *const PrecisionDocument, path: &Path) {
    let c = cpath(path);
    let mut err = ptr::null_mut();
    assert!(
        unsafe { dr_precision_doc_save_native(doc, c.as_ptr(), LIMIT, 0, ptr::null(), &mut err) },
        "{}",
        take_err_string(err)
    );
}

fn open(path: &Path) -> Result<*mut PrecisionDocument, String> {
    let c = cpath(path);
    let mut err = ptr::null_mut();
    let out = unsafe { dr_precision_doc_open(c.as_ptr(), LIMIT, 0, ptr::null(), &mut err) };
    if out.is_null() {
        Err(take_err_string(err))
    } else {
        Ok(out)
    }
}

/// The options of `n` inks: ink 2's curve with an interior slot set (the
/// rest blank), and for three or more inks two authored overprints.
fn options_of(n: usize) -> Value {
    let mut inks: Vec<Value> = (0..n).map(ink).collect();
    if n >= 2 {
        inks[1]["curve"][6] = json!(42.5);
    }
    if n >= 3 {
        json!({"inks": inks, "overprints": {"1+3": [30, 10, 20], "1+2+3": [12.5, -3, 4]}})
    } else {
        json!({ "inks": inks })
    }
}

/// The offset of the CMYK-slot word: right after `DRDC` and the identity
/// wrapper's 46.
fn word_at(bytes: &[u8]) -> usize {
    assert_eq!(&bytes[..4], b"DRDC");
    assert_eq!(&bytes[4..8], &46u32.to_le_bytes());
    8
}

#[test]
fn a_duotone_document_round_trips_one_to_four_inks() {
    let dir = TempDir::new().unwrap();
    let gray = gray_doc(&[0, 12000, 30000, 52000, 65535], 16, None);
    for n in 1..=4 {
        let doc = duotone(gray, &options_of(n));
        let path = dir.path().join(format!("d{n}.rz"));
        save(doc, &path);
        let bytes = std::fs::read(&path).unwrap();
        let at = word_at(&bytes);
        assert_eq!(&bytes[at..at + 4], &109u32.to_le_bytes());
        let reopened = open(&path).unwrap();
        assert_eq!(options(reopened), options(doc));
        assert_eq!(descriptor(reopened)["duotone"], descriptor(doc)["duotone"]);
        assert_eq!(descriptor(reopened)["color_mode"], "gray");
        assert_eq!(render16(reopened), render16(doc));
        let again = dir.path().join(format!("again{n}.rz"));
        save(reopened, &again);
        assert_eq!(std::fs::read(&again).unwrap(), bytes);
        free(reopened);
        free(doc);
    }
    // An 8-bit Duotone document keeps its depth.
    let eight = gray_doc(&[0, 77, 255], 8, None);
    let doc = duotone(eight, &options_of(2));
    let path = dir.path().join("eight.rz");
    save(doc, &path);
    let reopened = open(&path).unwrap();
    assert_eq!(descriptor(reopened)["gray"]["depth"], 8);
    assert_eq!(options(reopened), options(doc));
    for d in [reopened, doc, eight, gray] {
        free(d);
    }
}

#[test]
fn the_native_reader_refuses_a_bad_duotone_file_by_name() {
    let dir = TempDir::new().unwrap();
    let gray = gray_doc(&[0, 30000, 65535], 16, None);
    let doc = duotone(gray, &options_of(2));
    let path = dir.path().join("d.rz");
    save(doc, &path);
    let bytes = std::fs::read(&path).unwrap();
    // The DUOT table: tag, schema u32 = 1, ink count u8 = 2.
    let at = bytes
        .windows(9)
        .rposition(|w| w == b"DUOT\x01\x00\x00\x00\x02")
        .expect("DUOT table");
    let reopen = |bytes: &[u8], name: &str| -> String {
        let path = dir.path().join(name);
        std::fs::write(&path, bytes).unwrap();
        open(&path).expect_err(name)
    };
    let mut patched = bytes.clone();
    patched[at + 8] = 5;
    assert_eq!(
        reopen(&patched, "count.rz"),
        "native Duotone ink count must be 1 to 4"
    );
    let mut patched = bytes.clone();
    patched[at + 4] = 2;
    assert_eq!(
        reopen(&patched, "schema.rz"),
        "unsupported native Duotone schema"
    );
    // The word without its table: a Gray file whose word says Duotone.
    let gray_path = dir.path().join("g.rz");
    save(gray, &gray_path);
    let mut plain = std::fs::read(&gray_path).unwrap();
    let word = word_at(&plain);
    assert_eq!(&plain[word..word + 4], &106u32.to_le_bytes());
    plain[word..word + 4].copy_from_slice(&109u32.to_le_bytes());
    assert_eq!(
        reopen(&plain, "no-table.rz"),
        "unsupported native Duotone schema"
    );
    // A float body under the word.
    let float = crate::gray_ops::gray_doc(3, 1, 32);
    let float_path = dir.path().join("f.rz");
    save(float, &float_path);
    let mut floated = std::fs::read(&float_path).unwrap();
    let word = word_at(&floated);
    assert_eq!(&floated[word..word + 4], &106u32.to_le_bytes());
    floated[word..word + 4].copy_from_slice(&109u32.to_le_bytes());
    assert_eq!(
        reopen(&floated, "float.rz"),
        "native Duotone document storage must be 8 or 16 bits"
    );
    for d in [float, doc, gray] {
        free(d);
    }
}
