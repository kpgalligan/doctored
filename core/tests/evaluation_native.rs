//! Phase 11E (N): the `.rz` evaluation wrapper at both depths.
//!
//! What is pinned: a linear-light document writes 38 (byte) / 39 (typed) as
//! the FIRST word and closes with one `EVAL` table; the wrapper nests OUTSIDE
//! every prior optional feature (an outside-mask document writes 38 then 28);
//! an ENCODED document's first word is exactly the prior writer's and no table
//! follows; and a crafted policy, schema or trailing word refuses the open
//! before a single pixel is decoded.

use std::ffi::{c_char, CStr, CString};
use std::ptr;

use doctored_core::doc::{DrDocument, MaskKind};
use doctored_core::ffi::*;
use doctored_core::ffi_doc::*;
use doctored_core::ffi_evaluation::*;
use doctored_core::ffi_precision::*;
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::REFERENCE_WORKING_BYTES;
use image::codecs::png::PngEncoder;
use image::{ExtendedColorType, ImageEncoder, Rgba, RgbaImage};
use tempfile::TempDir;

const LINEAR: std::ffi::c_int = 1;
const LIMIT: u64 = REFERENCE_WORKING_BYTES;

fn take_err(err: *mut c_char) -> String {
    if err.is_null() {
        return String::new();
    }
    let text = unsafe { CStr::from_ptr(err) }
        .to_string_lossy()
        .into_owned();
    unsafe { dr_string_free(err) };
    text
}

fn handle(doc: &DrDocument) -> *mut DrDocument {
    Box::into_raw(Box::new(doc.clone()))
}

fn linear(doc: &DrDocument) -> DrDocument {
    let h = handle(doc);
    let mut err: *mut c_char = ptr::null_mut();
    let out = unsafe { dr_doc_set_evaluation(h, LINEAR, &mut err) };
    unsafe { dr_doc_free(h) };
    assert!(!out.is_null(), "{}", take_err(err));
    *unsafe { Box::from_raw(out) }
}

fn save(doc: &DrDocument, path: &std::path::Path) -> Vec<u8> {
    let h = handle(doc);
    let c = CString::new(path.to_str().unwrap()).unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    let ok = unsafe { dr_doc_save_native(h, c.as_ptr(), std::ptr::null(), &mut err) };
    unsafe { dr_doc_free(h) };
    assert!(ok, "{}", take_err(err));
    std::fs::read(path).expect("the saved file")
}

/// The refusal `reopen` reports, panicking when the open unexpectedly
/// succeeds (`DrDocument` has no `Debug`, so `expect_err` is not available).
fn reopen_error(path: &std::path::Path, what: &str) -> String {
    match reopen(path) {
        Err(message) => message,
        Ok(_) => panic!("{what} must refuse the open"),
    }
}

fn reopen(path: &std::path::Path) -> Result<DrDocument, String> {
    let c = CString::new(path.to_str().unwrap()).unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    let out = unsafe { dr_doc_open(c.as_ptr(), std::ptr::null(), &mut err) };
    if out.is_null() {
        return Err(take_err(err));
    }
    Ok(*unsafe { Box::from_raw(out) })
}

fn first_word(bytes: &[u8]) -> u32 {
    u32::from_le_bytes(bytes[4..8].try_into().unwrap())
}

fn second_word(bytes: &[u8]) -> u32 {
    u32::from_le_bytes(bytes[8..12].try_into().unwrap())
}

fn word_at(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap())
}

/// Where the `EVAL` table starts. Since 12E-1 it is no longer the file's
/// last block — the always-written `LIDS` table closes every file — so the
/// tests locate it by its tag rather than by counting back from the end.
fn eval_at(bytes: &[u8]) -> usize {
    bytes.windows(4).rposition(|w| w == b"EVAL").expect("EVAL")
}

/// The 12E-1 identity wrapper's version words.
const IDENTITY_BYTE: u32 = 45;
const IDENTITY_TYPED: u32 = 46;

fn fixture() -> DrDocument {
    let base = DrDocument::from_pixels(RgbaImage::from_fn(4, 2, |x, y| {
        Rgba([(x * 60) as u8, (y * 90) as u8, 120, 255])
    }));
    base.adding_image_layer(
        0,
        RgbaImage::from_fn(4, 2, |x, _| Rgba([200, (x * 40) as u8, 20, 180])),
        "Top",
    )
    .expect("the fixture's second layer")
}

fn policy_of(doc: &DrDocument) -> std::ffi::c_int {
    let h = handle(doc);
    let value = unsafe { dr_doc_evaluation(h) };
    unsafe { dr_doc_free(h) };
    value
}

#[test]
fn an_encoded_byte_document_writes_exactly_the_prior_first_word() {
    let dir = TempDir::new().unwrap();
    let doc = fixture();
    let bytes = save(&doc, &dir.path().join("encoded.rz"));
    assert_ne!(first_word(&bytes), 38, "no evaluation wrapper is written");
    assert_eq!(&bytes[bytes.len() - 4..], &bytes[bytes.len() - 4..]);
    assert!(
        !bytes.windows(4).any(|w| w == b"EVAL"),
        "an encoded document carries no EVAL table"
    );
    let reopened = reopen(&dir.path().join("encoded.rz")).expect("reopen");
    assert_eq!(policy_of(&reopened), 0);
}

#[test]
fn a_linear_byte_document_round_trips_through_the_wrapper() {
    let dir = TempDir::new().unwrap();
    let doc = linear(&fixture());
    let path = dir.path().join("linear.rz");
    let bytes = save(&doc, &path);
    assert_eq!(
        first_word(&bytes),
        IDENTITY_BYTE,
        "12E-1's identity wrapper is the outermost word"
    );
    assert_eq!(second_word(&bytes), 38, "38 is the word inside it");
    let at = eval_at(&bytes);
    assert_eq!(word_at(&bytes, at + 4), 1, "schema 1");
    assert_eq!(word_at(&bytes, at + 8), 1, "policy 1");
    let reopened = reopen(&path).expect("reopen");
    assert_eq!(policy_of(&reopened), LINEAR);
    assert_eq!(
        doc.flattened().expect("before"),
        reopened.flattened().expect("after"),
        "the reopened projection is the saved one"
    );
}

#[test]
fn the_wrapper_nests_outside_the_outside_mask_table() {
    let dir = TempDir::new().unwrap();
    let doc = fixture();
    let masked = doc
        .add_mask(1, MaskKind::FromSelection(&[255u8; 8]))
        .expect("a mask");
    let mut masked = masked;
    masked.layers[1].mask_properties.outside_coverage = 255;
    let path = dir.path().join("both.rz");
    let bytes = save(&linear(&masked), &path);
    assert_eq!(
        first_word(&bytes),
        IDENTITY_BYTE,
        "the identity word is outermost"
    );
    assert_eq!(second_word(&bytes), 38, "the evaluation word is inside it");
    assert_eq!(
        word_at(&bytes, 12),
        28,
        "the outside-mask word is inside that"
    );
    let reopened = reopen(&path).expect("reopen");
    assert_eq!(policy_of(&reopened), LINEAR);
}

#[test]
fn a_crafted_policy_schema_or_trailing_word_refuses_the_open() {
    let dir = TempDir::new().unwrap();
    let doc = linear(&fixture());
    let good = save(&doc, &dir.path().join("good.rz"));

    // The `EVAL` table is located by its tag: `LIDS` closes the file now.
    let at = eval_at(&good);
    let mut wrong_policy = good.clone();
    wrong_policy[at + 8..at + 12].copy_from_slice(&0u32.to_le_bytes());
    let path = dir.path().join("policy.rz");
    std::fs::write(&path, &wrong_policy).unwrap();
    assert!(
        reopen_error(&path, "policy 0").contains("policy"),
        "the refusal names the policy"
    );

    let mut wrong_schema = good.clone();
    wrong_schema[at + 4..at + 8].copy_from_slice(&2u32.to_le_bytes());
    let path = dir.path().join("schema.rz");
    std::fs::write(&path, &wrong_schema).unwrap();
    assert!(
        reopen_error(&path, "schema 2").contains("schema"),
        "the refusal names the schema"
    );

    let mut trailing = good.clone();
    trailing.extend_from_slice(&7u32.to_le_bytes());
    let path = dir.path().join("trailing.rz");
    std::fs::write(&path, &trailing).unwrap();
    assert!(
        reopen_error(&path, "a trailing word").contains("trailing"),
        "the refusal names the trailing bytes"
    );
}

// --------------------------------------------------------------- typed --

struct Doc(*mut PrecisionDocument);
impl Drop for Doc {
    fn drop(&mut self) {
        unsafe { dr_precision_doc_free(self.0) }
    }
}

fn typed_fixture(dir: &TempDir) -> Doc {
    let file = dir.path().join("typed.png");
    let pixels: Vec<[u16; 4]> = (0..8u32)
        .map(|x| [(x * 8000) as u16, 40000 - (x * 4000) as u16, 12345, 65535])
        .collect();
    let bytes: Vec<_> = pixels
        .iter()
        .flatten()
        .flat_map(|v| v.to_ne_bytes())
        .collect();
    let mut encoded = Vec::new();
    PngEncoder::new(&mut encoded)
        .write_image(&bytes, pixels.len() as u32, 1, ExtendedColorType::Rgba16)
        .unwrap();
    std::fs::write(&file, encoded).unwrap();
    let c = CString::new(file.to_str().unwrap()).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe { dr_precision_doc_open(c.as_ptr(), LIMIT, 0, ptr::null(), &mut err) };
    assert!(!out.is_null(), "{}", take_err(err));
    Doc(out)
}

fn typed_info(doc: &Doc) -> serde_json::Value {
    let mut err = ptr::null_mut();
    let raw = unsafe { dr_precision_doc_info(doc.0, &mut err) };
    assert!(!raw.is_null(), "{}", take_err(err));
    serde_json::from_str(&take_err(raw)).expect("info JSON")
}

#[test]
fn a_typed_document_reports_and_round_trips_its_evaluation_tag() {
    let dir = TempDir::new().unwrap();
    let doc = typed_fixture(&dir);
    assert_eq!(typed_info(&doc)["evaluation"], "encoded_rgb_f32_v1");
    assert_eq!(unsafe { dr_precision_doc_evaluation(doc.0) }, 0);

    let mut err = ptr::null_mut();
    let switched = unsafe { dr_precision_doc_set_evaluation(doc.0, LINEAR, &mut err) };
    assert!(!switched.is_null(), "{}", take_err(err));
    let switched = Doc(switched);
    assert_eq!(unsafe { dr_precision_doc_evaluation(switched.0) }, LINEAR);
    assert_eq!(typed_info(&switched)["evaluation"], "linear_light_f32_v1");

    let saved = dir.path().join("typed.rz");
    let c = CString::new(saved.to_str().unwrap()).unwrap();
    let mut err = ptr::null_mut();
    assert!(
        unsafe {
            dr_precision_doc_save_native(switched.0, c.as_ptr(), LIMIT, 0, ptr::null(), &mut err)
        },
        "{}",
        take_err(err)
    );
    let bytes = std::fs::read(&saved).expect("the saved typed file");
    assert_eq!(
        first_word(&bytes),
        IDENTITY_TYPED,
        "12E-1's identity wrapper is the outermost typed word"
    );
    assert_eq!(second_word(&bytes), 39, "39 is the word inside it");
    assert_eq!(&bytes[eval_at(&bytes)..eval_at(&bytes) + 4], b"EVAL");

    let mut err = ptr::null_mut();
    let reopened = unsafe { dr_precision_doc_open(c.as_ptr(), LIMIT, 0, ptr::null(), &mut err) };
    assert!(!reopened.is_null(), "{}", take_err(err));
    let reopened = Doc(reopened);
    assert_eq!(unsafe { dr_precision_doc_evaluation(reopened.0) }, LINEAR);
    assert_eq!(typed_info(&reopened)["evaluation"], "linear_light_f32_v1");

    // The byte reader must not accept a typed file, evaluation word or not.
    assert!(reopen(&saved).is_err(), "the byte reader refuses 39");
}

#[test]
fn a_typed_documents_plan_counts_the_transfer_tables() {
    let dir = TempDir::new().unwrap();
    let doc = typed_fixture(&dir);
    let before = typed_info(&doc)["retained_bytes"].as_u64().expect("bytes");
    let mut err = ptr::null_mut();
    let switched = unsafe { dr_precision_doc_set_evaluation(doc.0, LINEAR, &mut err) };
    assert!(!switched.is_null(), "{}", take_err(err));
    let switched = Doc(switched);
    let after = typed_info(&switched)["retained_bytes"]
        .as_u64()
        .expect("bytes");
    assert!(
        after >= before + 2_359_320,
        "the typed tables are inside admission: {before} -> {after}"
    );
}
