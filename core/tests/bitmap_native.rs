//! #1 Bitmap phase 1a in `.rz` (word 110, its GRAY table then its `BTMP`
//! table): a Bitmap document reopens as Bitmap with the same plane and
//! render, and a re-save is byte-equal to the first file; the reader refuses
//! a bad schema, a pad bit set past the canvas and a plane that disagrees
//! with the body by name (byte-patched files, as `indexed_native.rs` does).

use std::path::{Path, PathBuf};
use std::ptr;

use doctored_core::ffi_precision::{dr_precision_doc_open, dr_precision_doc_save_native};
use doctored_core::precision_document::PrecisionDocument;
use serde_json::json;
use tempfile::TempDir;

use crate::bitmap_mode::{bits, gray_doc, to_bitmap, to_gray};
use crate::common::{cpath, take_err_string};
use crate::indexed_fixture::{descriptor, free, render16, LIMIT};

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

/// A 5×3 Bitmap document (width 5: three pad bits a row).
fn bitmap() -> (*mut PrecisionDocument, *mut PrecisionDocument) {
    let codes: Vec<u8> = (0..15u8)
        .map(|i| if i % 3 == 0 { 255 } else { 0 })
        .collect();
    let gray = gray_doc(5, 3, &codes);
    let bitmap = to_bitmap(gray, &json!({"method":"threshold"})).unwrap();
    (gray, bitmap)
}

#[test]
fn a_bitmap_document_round_trips() {
    let dir = TempDir::new().unwrap();
    let (gray, doc) = bitmap();
    let path = dir.path().join("b.rz");
    save(doc, &path);
    let reopened = open(&path).unwrap();
    let r = descriptor(reopened);
    assert_eq!(
        (
            r["color_mode"].clone(),
            r["gray"]["depth"].clone(),
            r["layer_count"].clone()
        ),
        (json!("bitmap"), json!(8), json!(1))
    );
    assert_eq!(bits(reopened), bits(doc));
    assert_eq!(render16(reopened), render16(doc));
    let again = dir.path().join("again.rz");
    save(reopened, &again);
    assert_eq!(
        std::fs::read(&again).unwrap(),
        std::fs::read(&path).unwrap()
    );
    // …and it converts back to Grayscale as the original does.
    let back = to_gray(reopened, &json!({})).unwrap();
    assert_eq!(descriptor(back)["color_mode"], json!("gray"));
    // A plain Gray document still writes the Gray word and reads as Gray.
    let gray_path = dir.path().join("g.rz");
    save(back, &gray_path);
    let gray_again = open(&gray_path).unwrap();
    assert_eq!(descriptor(gray_again)["color_mode"], json!("gray"));
    for d in [gray_again, back, reopened, doc, gray] {
        free(d);
    }
}

#[test]
fn the_native_reader_refuses_a_btmp_table_that_disagrees_with_the_body() {
    let dir = TempDir::new().unwrap();
    let (gray, doc) = bitmap();
    let path = dir.path().join("r.rz");
    save(doc, &path);
    free(doc);
    free(gray);
    let bytes = std::fs::read(&path).unwrap();
    // The BTMP table: tag, schema u32 = 1, width u32 = 5, height u32 = 3.
    let at = bytes
        .windows(16)
        .rposition(|w| w == b"BTMP\x01\x00\x00\x00\x05\x00\x00\x00\x03\x00\x00\x00")
        .expect("BTMP table");
    let plane = at + 16;
    // Row 0 is 1 0 0 1 0: 0b1001_0000.
    assert_eq!(bytes[plane], 0b1001_0000);
    let reopen = |patch: &dyn Fn(&mut Vec<u8>), name: &str| -> String {
        let mut patched = bytes.clone();
        patch(&mut patched);
        let path: PathBuf = dir.path().join(name);
        std::fs::write(&path, &patched).unwrap();
        open(&path).expect_err(name)
    };
    assert_eq!(
        reopen(&|b| b[at + 4] = 2, "schema.rz"),
        "unsupported native Bitmap schema"
    );
    assert_eq!(
        reopen(&|b| b[plane] |= 0b0000_0100, "pad.rz"),
        "native Bitmap document: the Bitmap plane sets a bit past the canvas"
    );
    // A flipped bit inside the canvas: a valid plane that is not the body's.
    assert_eq!(
        reopen(&|b| b[plane] ^= 0b0100_0000, "drift.rz"),
        "native Bitmap document pixels are not its plane's"
    );
}
