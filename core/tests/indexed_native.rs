//! #547 Indexed Color phase 1a in `.rz` (word 108 and its `INDX` table): an
//! Indexed document with a transparent index reopens with its palette size,
//! transparent index, every index and the same render, and a re-save is
//! byte-equal to the first file; the reader refuses a bad schema, a
//! transparent index outside the palette and a plane that disagrees with the
//! body by name (byte-patched files, as `lab_native.rs` does).

use std::path::{Path, PathBuf};
use std::ptr;

use doctored_core::ffi_precision::{dr_precision_doc_open, dr_precision_doc_save_native};
use doctored_core::precision_document::PrecisionDocument;
use serde_json::json;
use tempfile::TempDir;

use crate::common::{cpath, take_err_string};
use crate::indexed_fixture::{descriptor, free, indices, render16, rgb_doc, to_indexed, LIMIT};

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

/// A 4×3 Indexed document: three colours and a clear pixel.
fn indexed() -> *mut PrecisionDocument {
    let mut pixels = Vec::new();
    for i in 0..12u8 {
        pixels.push(match i % 4 {
            0 => [250, 10, 10, 255],
            1 => [10, 250, 10, 255],
            2 => [0, 0, 0, 0],
            _ => [10, 10, 250, 255],
        });
    }
    let rgb = rgb_doc(4, 3, &pixels);
    let out = to_indexed(rgb, &json!({"palette":"exact","dither":"none"})).unwrap();
    free(rgb);
    out
}

#[test]
fn an_indexed_document_round_trips() {
    let dir = TempDir::new().unwrap();
    let doc = indexed();
    let d = descriptor(doc);
    assert_eq!(d["indexed"], json!({"colors": 4, "transparent_index": 3}));
    let path = dir.path().join("i.rz");
    save(doc, &path);
    let reopened = open(&path).unwrap();
    let r = descriptor(reopened);
    assert_eq!(
        (
            r["color_mode"].clone(),
            r["indexed"].clone(),
            r["layer_count"].clone()
        ),
        (json!("indexed"), d["indexed"].clone(), json!(1))
    );
    assert_eq!(indices(reopened), indices(doc));
    assert_eq!(render16(reopened), render16(doc));
    let again = dir.path().join("again.rz");
    save(reopened, &again);
    assert_eq!(
        std::fs::read(&again).unwrap(),
        std::fs::read(&path).unwrap()
    );
    free(reopened);
    free(doc);
}

#[test]
fn the_native_reader_refuses_an_indx_table_that_disagrees_with_the_body() {
    let dir = TempDir::new().unwrap();
    let doc = indexed();
    let path = dir.path().join("r.rz");
    save(doc, &path);
    free(doc);
    let bytes = std::fs::read(&path).unwrap();
    // The INDX table: tag, schema u32 = 1, count u16 = 4.
    let at = bytes
        .windows(10)
        .rposition(|w| w == b"INDX\x01\x00\x00\x00\x04\x00")
        .expect("INDX table");
    let transparent = at + 10 + 4 * 3;
    assert_eq!(&bytes[transparent..transparent + 2], &3i16.to_le_bytes());
    let plane = transparent + 2 + 8;
    let reopen = |patch: &dyn Fn(&mut Vec<u8>), name: &str| -> String {
        let mut patched = bytes.clone();
        patch(&mut patched);
        let path: PathBuf = dir.path().join(name);
        std::fs::write(&path, &patched).unwrap();
        open(&path).expect_err(name)
    };
    assert_eq!(
        reopen(&|b| b[at + 4] = 2, "schema.rz"),
        "unsupported native Indexed Color schema"
    );
    assert_eq!(
        reopen(&|b| b[transparent] = 9, "transparent.rz"),
        "native Indexed Color transparent index is outside the palette"
    );
    assert_eq!(
        reopen(&|b| b[plane] = 9, "outside.rz"),
        "native Indexed Color document: an Indexed Color index is outside the palette"
    );
    // A valid index that is not the body's pixel there.
    assert_eq!(
        reopen(&|b| b[plane] = 1, "drift.rz"),
        "native Indexed Color document pixels are not its palette's"
    );
}
