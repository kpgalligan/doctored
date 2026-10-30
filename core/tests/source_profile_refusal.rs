//! A placed or replaced ordinary source keeps the core open's
//! profile-refusal sentence on its immutable asset, reports it from source
//! info, and persists it in the conditional `RSPR` table (word 56 byte /
//! 57 typed, the first word inside identity 45/46, the table last before
//! `LIDS`). Every table byte this file checks is assembled from the wire
//! format written out in `source_refusal_native.rs`'s module doc, not read
//! back from the writer.
//!
//! # The stored 53/55-writer files
//!
//! `tests/fixtures/rz_byte_53/placed_source.rz` and
//! `tests/fixtures/rz_typed_55/placed_source.rz` are FORMAT ORACLES, as
//! `psd_10c/*.psd` is — not golden images. They were written by the writers
//! that predate words 56 and 57, never by this build, and nothing in this
//! tree regenerates them:
//!
//! - commit: 8706567f643ff469fb36ef4c3f4ef827c1a257f8 (8706567, the last
//!   writers without the refusal words), checked out in a scratch
//!   `git worktree`;
//! - builder: a throwaway, never-committed `core/tests/gen_rz53.rs` holding
//!   `common/source_refusal_fixture.rs` verbatim (as a `#[path]` module) and
//!   two ignored tests: `byte_document().save_native($DR_OUT)` and
//!   `typed_document().encode_native(LIMIT, 0, None)` written to `$DR_OUT`;
//! - command: `cd <worktree>/core && CARGO_TARGET_DIR=<repo>/core/target DR_OUT=<repo>/core/tests/fixtures/rz_byte_53/placed_source.rz
//!   cargo test --release --test gen_rz53 -- --ignored write_rz53`, and the
//!   same with `rz_typed_55/placed_source.rz` and `write_rz55`;
//! - bytes: 3362, sha256
//!   `116f0982ee09621d06dae070e5aa1798eb00c1dfa42b0f802c10fa3453461ace`
//!   (byte); 6476, sha256
//!   `0bc709a00981e915aa528dc2df43d66940ae6799de0bd54e4d18b214a9bc6d78`
//!   (typed).
//!
//! The current writers must reproduce them byte for byte. The planes inside
//! them are encoded by the PNG crate, so a dependency bump that moves the PNG
//! encoder's bytes fails those tests ON PURPOSE: the stored files are what an
//! older build wrote, and a writer that no longer reproduces them has changed
//! the format.

use crate::identity;
use crate::source_refusal_fixture as fixture;
use crate::warp_fixture as warp;

use doctored_core::doc::DrDocument;
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_precision_edit::dr_precision_doc_edit;
use doctored_core::ffi_retained_source::{
    dr_doc_mixed_source_edit, dr_doc_source_info, dr_precision_doc_mixed_source_edit,
    dr_precision_doc_source_info,
};
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_geometry::GeometryEdit;
use doctored_core::precision_image::PrecisionImage;
use doctored_core::retained_source::SourceEdit;
use doctored_core::style::LayerStyle;
use fixture::{
    byte_base, byte_document, byte_master, place, place_request, typed_base, typed_document,
    typed_master, LIMIT, PLACED,
};
use identity::{drdc_word, without_identity};
use image::{Rgba, RgbaImage};
use serde_json::{json, Value};
use std::ffi::{c_char, CStr, CString};
use std::path::Path;
use std::ptr;
use std::sync::Arc;
use tempfile::TempDir;

/// A sentence of the shape the core open reports.
const SENTENCE: &str = "The embedded ICC profile “Generic Gray Gamma 2.2 Profile” \
    is not a supported RGB matrix/TRC profile; the image was opened as sRGB.";
/// The validator's sentence, restated.
const INVALID: &str = "source profile refusal requires 1–4096 bytes of UTF-8 without NUL";
const BYTE_WORD: u32 = 56;
const TYPED_WORD: u32 = 57;

// ------------------------------------------------------------- plumbing --

unsafe fn text(pointer: *mut c_char) -> String {
    if pointer.is_null() {
        return String::new();
    }
    let result = CStr::from_ptr(pointer).to_string_lossy().into_owned();
    dr_string_free(pointer);
    result
}

fn edit8(
    doc: &DrDocument,
    source8: Option<&DrDocument>,
    source16: Option<&PrecisionDocument>,
    value: Value,
) -> DrDocument {
    let request = CString::new(value.to_string()).unwrap();
    let mut error = ptr::null_mut();
    let out = unsafe {
        dr_doc_mixed_source_edit(
            doc,
            source8.map_or(ptr::null(), |s| s),
            source16.map_or(ptr::null(), |s| s),
            request.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut error,
        )
    };
    assert!(error.is_null(), "{}", unsafe { text(error) });
    assert!(!out.is_null());
    *unsafe { Box::from_raw(out) }
}

fn edit16(
    doc: &PrecisionDocument,
    source8: Option<&DrDocument>,
    source16: Option<&PrecisionDocument>,
    value: Value,
) -> PrecisionDocument {
    let request = CString::new(value.to_string()).unwrap();
    let mut error = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_mixed_source_edit(
            doc,
            source8.map_or(ptr::null(), |s| s),
            source16.map_or(ptr::null(), |s| s),
            request.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut error,
        )
    };
    assert!(error.is_null(), "{}", unsafe { text(error) });
    assert!(!out.is_null());
    *unsafe { Box::from_raw(out) }
}

fn info8(doc: &DrDocument, index: usize) -> Value {
    let mut error = ptr::null_mut();
    let out = unsafe { dr_doc_source_info(doc, index, &mut error) };
    assert!(error.is_null(), "{}", unsafe { text(error) });
    serde_json::from_str(&unsafe { text(out) }).unwrap()
}

fn info16(doc: &PrecisionDocument, index: usize) -> Value {
    let mut error = ptr::null_mut();
    let out = unsafe { dr_precision_doc_source_info(doc, index, &mut error) };
    assert!(error.is_null(), "{}", unsafe { text(error) });
    serde_json::from_str(&unsafe { text(out) }).unwrap()
}

fn second_byte_master() -> DrDocument {
    DrDocument::from_pixels(RgbaImage::from_pixel(5, 4, Rgba([13, 25, 37, 255])))
}

fn second_typed_master() -> PrecisionDocument {
    PrecisionDocument::from_image(
        PrecisionImage::from_rgba16(
            5,
            4,
            vec![[13, 25, 37, 65535]; 20],
            IccProfile::srgb(),
            None,
        )
        .unwrap(),
        Metadata::default(),
        Resolution::default(),
    )
    .unwrap()
}

fn byte_with(refusal: Option<&str>) -> DrDocument {
    byte_base()
        .source_edit(Some(&byte_master()), &place(refusal), LIMIT, 0, None)
        .unwrap()
        .unwrap()
}

fn typed_with(refusal: Option<&str>) -> PrecisionDocument {
    typed_base()
        .source_edit(Some(&typed_master()), &place(refusal), LIMIT, 0, None)
        .unwrap()
        .unwrap()
}

fn save8(doc: &DrDocument, path: &Path) -> Vec<u8> {
    doc.save_native(path.to_str().unwrap()).unwrap();
    std::fs::read(path).unwrap()
}

fn save16(doc: &PrecisionDocument, path: &Path) -> Vec<u8> {
    doc.save_native(path.to_str().unwrap(), LIMIT, 0, None)
        .unwrap();
    std::fs::read(path).unwrap()
}

fn open8(path: &Path) -> DrDocument {
    DrDocument::open(path.to_str().unwrap()).unwrap()
}

fn open16(path: &Path) -> PrecisionDocument {
    PrecisionDocument::open_native(path.to_str().unwrap(), LIMIT, 0, None).unwrap()
}

/// The RSPR table this file expects, assembled from the module-doc format.
fn table(entries: &[(u32, &str)]) -> Vec<u8> {
    let mut out = b"RSPR".to_vec();
    out.extend_from_slice(&1_u32.to_le_bytes());
    out.extend_from_slice(&(entries.len() as u32).to_le_bytes());
    for (index, text) in entries {
        out.extend_from_slice(&index.to_le_bytes());
        out.extend_from_slice(&(text.len() as u32).to_le_bytes());
        out.extend_from_slice(text.as_bytes());
    }
    out
}

fn position(bytes: &[u8], tag: &[u8]) -> Option<usize> {
    bytes.windows(4).position(|w| w == tag)
}

fn occurrences(bytes: &[u8], tag: &[u8]) -> usize {
    bytes.windows(4).filter(|w| *w == tag).count()
}

/// RSPR appears once and ends exactly where LIDS begins.
fn assert_table_before_ids(bytes: &[u8], expected: &[u8]) {
    assert_eq!(occurrences(bytes, b"RSPR"), 1);
    let at = position(bytes, b"RSPR").unwrap();
    assert_eq!(&bytes[at..at + expected.len()], expected);
    assert_eq!(
        &bytes[at + expected.len()..at + expected.len() + 4],
        b"LIDS"
    );
}

// ---------------------------------------------------------------- parse --

#[test]
fn place_and_replace_accept_the_key_and_the_validator_bounds_it() {
    let long = "x".repeat(4097);
    let exact = "é".repeat(2048);
    assert_eq!(exact.len(), 4096);
    for (op, base) in [
        ("place", place_request(None)),
        (
            "replace",
            json!({"op":"replace","layer":1,"revision_id":"second"}),
        ),
    ] {
        for accepted in [SENTENCE, exact.as_str()] {
            let mut request = base.clone();
            request["profile_refusal"] = json!(accepted);
            assert!(
                SourceEdit::parse(&request.to_string()).is_ok(),
                "{op} accepts the key"
            );
        }
        let mut absent = base.clone();
        absent["profile_refusal"] = Value::Null;
        assert!(SourceEdit::parse(&absent.to_string()).is_ok());
        for refused in ["", long.as_str(), "before\0after"] {
            let mut request = base.clone();
            request["profile_refusal"] = json!(refused);
            assert_eq!(
                SourceEdit::parse(&request.to_string()).unwrap_err(),
                INVALID,
                "{op} refuses {} bytes",
                refused.len()
            );
        }
    }
    for request in [
        json!({"op":"capture_raster","layer":0,"source_id":"capture",
            "source_transform":[1,0,0,1,0,0],"profile_refusal":SENTENCE}),
        json!({"op":"retain","source_id":"retained","profile_refusal":SENTENCE}),
    ] {
        let error = SourceEdit::parse(&request.to_string()).unwrap_err();
        assert!(error.starts_with("invalid source request"), "{error}");
    }
}

// ------------------------------------------------------------------ FFI --

#[test]
fn the_sentence_rides_the_asset_through_edits_at_the_byte_depth() {
    let placed = edit8(
        &byte_base(),
        Some(&byte_master()),
        None,
        place_request(Some(SENTENCE)),
    );
    assert_eq!(info8(&placed, PLACED)["profile_refusal"], SENTENCE);
    let plain = edit8(
        &byte_base(),
        Some(&byte_master()),
        None,
        place_request(None),
    );
    assert!(info8(&plain, PLACED)["profile_refusal"].is_null());
    let moved = edit8(
        &placed,
        None,
        None,
        json!({"op":"transform","layer":PLACED,"matrix":[1,0,0,1,1,1],"filter":"lanczos3"}),
    );
    assert_eq!(info8(&moved, PLACED)["profile_refusal"], SENTENCE);
    assert_eq!(info8(&moved, PLACED)["embedded"], true);
    let replaced = edit8(
        &moved,
        Some(&second_byte_master()),
        None,
        json!({"op":"replace","layer":PLACED,"revision_id":"second"}),
    );
    assert_eq!(info8(&replaced, PLACED)["revision_id"], "second");
    assert!(info8(&replaced, PLACED)["profile_refusal"].is_null());
    let refused_again = edit8(
        &replaced,
        Some(&byte_master()),
        None,
        json!({"op":"replace","layer":PLACED,"revision_id":"third","profile_refusal":"Third."}),
    );
    assert_eq!(info8(&refused_again, PLACED)["profile_refusal"], "Third.");
    let rasterized = edit8(
        &refused_again,
        None,
        None,
        json!({"op":"rasterize","layer":PLACED}),
    );
    assert_eq!(info8(&rasterized, PLACED), json!({"valid":false}));
}

#[test]
fn the_sentence_rides_the_asset_through_edits_at_the_typed_depth() {
    let placed = edit16(
        &typed_base(),
        None,
        Some(&typed_master()),
        place_request(Some(SENTENCE)),
    );
    assert_eq!(info16(&placed, PLACED)["profile_refusal"], SENTENCE);
    let plain = edit16(
        &typed_base(),
        None,
        Some(&typed_master()),
        place_request(None),
    );
    assert!(info16(&plain, PLACED)["profile_refusal"].is_null());
    let moved = edit16(
        &placed,
        None,
        None,
        json!({"op":"transform","layer":PLACED,"matrix":[1,0,0,1,1,1],"filter":"lanczos3"}),
    );
    assert_eq!(info16(&moved, PLACED)["profile_refusal"], SENTENCE);
    let replaced = edit16(
        &moved,
        None,
        Some(&second_typed_master()),
        json!({"op":"replace","layer":PLACED,"revision_id":"second"}),
    );
    assert_eq!(info16(&replaced, PLACED)["revision_id"], "second");
    assert!(info16(&replaced, PLACED)["profile_refusal"].is_null());
    let rasterized = edit16(
        &replaced,
        None,
        None,
        json!({"op":"rasterize","layer":PLACED}),
    );
    assert_eq!(info16(&rasterized, PLACED), json!({"valid":false}));
}

// ----------------------------------------------------------- round trip --

#[test]
fn a_refusal_round_trips_inside_identity_at_both_depths() {
    let dir = TempDir::new().unwrap();
    let exact = "é".repeat(2048);
    for sentence in [SENTENCE, exact.as_str()] {
        let expected = table(&[(0, sentence)]);

        let path = dir.path().join("byte.rz");
        let bytes = save8(&byte_with(Some(sentence)), &path);
        assert_eq!(drdc_word(&bytes, 0), 45);
        assert_eq!(drdc_word(&bytes, 1), BYTE_WORD);
        assert_table_before_ids(&bytes, &expected);
        let reopened = open8(&path);
        assert_eq!(info8(&reopened, PLACED)["profile_refusal"], sentence);
        assert_eq!(save8(&reopened, &path), bytes);

        let path = dir.path().join("typed.rz");
        let bytes = save16(&typed_with(Some(sentence)), &path);
        assert_eq!(drdc_word(&bytes, 0), 46);
        assert_eq!(drdc_word(&bytes, 1), TYPED_WORD);
        assert_table_before_ids(&bytes, &expected);
        let reopened = open16(&path);
        assert_eq!(info16(&reopened, PLACED)["profile_refusal"], sentence);
        assert_eq!(save16(&reopened, &path), bytes);
    }
}

/// A perspective set on the placed layer: the projective (51/52) request.
fn perspective_set() -> GeometryEdit {
    perspective_on(PLACED)
}

/// The same perspective set on `layer`.
fn perspective_on(layer: usize) -> GeometryEdit {
    GeometryEdit::parse(
        &json!({"op":"perspective_set","layers":[layer],
        "source_rect":[2,1,5,4],"quad":[2,1,5.5,1,5.5,4,2,5],
        "filter":"bilinear"})
        .to_string(),
    )
    .unwrap()
}

/// A deep knockout on byte `layer`: the style-knockout (49) word.
fn knockout8(doc: &DrDocument, layer: usize) -> DrDocument {
    let style = LayerStyle::from_json("{\"knockout\":\"deep\"}").unwrap();
    doc.set_layer_style(layer, Some(Arc::new(style))).unwrap()
}

#[test]
fn the_refusal_word_nests_outside_the_projective_word() {
    let dir = TempDir::new().unwrap();
    let perspective = perspective_set();
    let byte = byte_with(Some(SENTENCE))
        .geometry_edit(&perspective, LIMIT, 0, None)
        .unwrap()
        .unwrap();
    assert_eq!(info8(&byte, PLACED)["transform_kind"], "projective");
    let path = dir.path().join("byte.rz");
    let bytes = save8(&byte, &path);
    assert_eq!([0, 1, 2].map(|i| drdc_word(&bytes, i)), [45, BYTE_WORD, 51]);
    assert!(position(&bytes, b"RSPJ").unwrap() < position(&bytes, b"RSPR").unwrap());
    assert_table_before_ids(&bytes, &table(&[(0, SENTENCE)]));
    let reopened = open8(&path);
    assert_eq!(info8(&reopened, PLACED)["profile_refusal"], SENTENCE);
    assert_eq!(info8(&reopened, PLACED)["transform_kind"], "projective");
    assert_eq!(save8(&reopened, &path), bytes);

    let typed = typed_with(Some(SENTENCE))
        .geometry(&perspective, LIMIT, 0, None)
        .unwrap()
        .unwrap();
    let path = dir.path().join("typed.rz");
    let bytes = save16(&typed, &path);
    assert_eq!(
        [0, 1, 2].map(|i| drdc_word(&bytes, i)),
        [46, TYPED_WORD, 52]
    );
    assert_table_before_ids(&bytes, &table(&[(0, SENTENCE)]));
    let reopened = open16(&path);
    assert_eq!(info16(&reopened, PLACED)["profile_refusal"], SENTENCE);
    assert_eq!(info16(&reopened, PLACED)["transform_kind"], "projective");
    assert_eq!(save16(&reopened, &path), bytes);
}

/// A typed structure edit through the one request surface.
fn structure16(doc: &PrecisionDocument, value: Value) -> PrecisionDocument {
    let request = CString::new(value.to_string()).unwrap();
    let mut error = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_edit(
            doc,
            request.as_ptr(),
            ptr::null(),
            0,
            LIMIT,
            0,
            ptr::null(),
            &mut error,
        )
    };
    assert!(error.is_null(), "{}", unsafe { text(error) });
    assert!(!out.is_null());
    *unsafe { Box::from_raw(out) }
}

#[test]
fn the_refusal_word_nests_outside_the_group_open_word() {
    let dir = TempDir::new().unwrap();
    // Postorder after grouping the base: Base 0, Group 1, Placed 2.
    let grouped = structure16(
        &typed_with(Some(SENTENCE)),
        json!({"op":"group_layers","layers":[0],"name":"Group","pass_through":false}),
    );
    let closed = structure16(&grouped, json!({"op":"layer_open","layer":1,"open":false}));
    let placed = 2;
    assert_eq!(info16(&closed, placed)["profile_refusal"], SENTENCE);
    let path = dir.path().join("typed.rz");
    let bytes = save16(&closed, &path);
    assert_eq!(
        [0, 1, 2].map(|i| drdc_word(&bytes, i)),
        [46, TYPED_WORD, 55]
    );
    assert!(position(&bytes, b"LOPN").unwrap() < position(&bytes, b"RSPR").unwrap());
    assert_table_before_ids(&bytes, &table(&[(0, SENTENCE)]));
    let reopened = open16(&path);
    assert_eq!(info16(&reopened, placed)["profile_refusal"], SENTENCE);
    assert_eq!(save16(&reopened, &path), bytes);
}

#[test]
fn the_refusal_word_nests_outside_the_warp_word() {
    let dir = TempDir::new().unwrap();
    // The warp binds the base (layer 0); the refusing source is layer 1.
    let request = warp::edit(warp::field(3, 3, "bilinear"));
    let byte = byte_with(Some(SENTENCE))
        .warp_edit(&request, LIMIT, 0, None)
        .unwrap()
        .unwrap();
    let path = dir.path().join("byte.rz");
    let bytes = save8(&byte, &path);
    assert_eq!([0, 1, 2].map(|i| drdc_word(&bytes, i)), [45, BYTE_WORD, 53]);
    assert!(position(&bytes, b"WARP").unwrap() < position(&bytes, b"RSPR").unwrap());
    assert_table_before_ids(&bytes, &table(&[(0, SENTENCE)]));
    let reopened = open8(&path);
    assert_eq!(info8(&reopened, PLACED)["profile_refusal"], SENTENCE);
    assert!(reopened.warp_info(0).unwrap()["field"].is_object());
    assert_eq!(save8(&reopened, &path), bytes);

    let typed = typed_with(Some(SENTENCE))
        .warp_edit(&request, LIMIT, 0, None)
        .unwrap()
        .unwrap();
    let path = dir.path().join("typed.rz");
    let bytes = save16(&typed, &path);
    assert_eq!(
        [0, 1, 2].map(|i| drdc_word(&bytes, i)),
        [46, TYPED_WORD, 54]
    );
    assert!(position(&bytes, b"WARP").unwrap() < position(&bytes, b"RSPR").unwrap());
    assert_table_before_ids(&bytes, &table(&[(0, SENTENCE)]));
    let reopened = open16(&path);
    assert_eq!(info16(&reopened, PLACED)["profile_refusal"], SENTENCE);
    assert!(reopened.warp_info(0).unwrap()["field"].is_object());
    assert_eq!(save16(&reopened, &path), bytes);
}

/// A second placed source (asset 1, layer 2), optionally refusing.
fn second(refusal: Option<&str>) -> SourceEdit {
    let mut request = json!({"op":"place","source_id":"second-master",
        "revision_id":"first","name":"Second","x":0,"y":0});
    if let Some(text) = refusal {
        request["profile_refusal"] = json!(text);
    }
    SourceEdit::parse(&request.to_string()).unwrap()
}

#[test]
fn only_refusing_assets_are_listed_in_asset_order() {
    let dir = TempDir::new().unwrap();
    // Asset 0 refuses nothing, asset 1 does: the table names index 1 only.
    let byte = byte_with(None)
        .source_edit(
            Some(&second_byte_master()),
            &second(Some("Second.")),
            LIMIT,
            0,
            None,
        )
        .unwrap()
        .unwrap();
    let path = dir.path().join("byte.rz");
    let bytes = save8(&byte, &path);
    assert_table_before_ids(&bytes, &table(&[(1, "Second.")]));
    let reopened = open8(&path);
    assert!(info8(&reopened, PLACED)["profile_refusal"].is_null());
    assert_eq!(info8(&reopened, 2)["profile_refusal"], "Second.");

    let typed = typed_with(Some(SENTENCE))
        .source_edit(
            Some(&second_typed_master()),
            &second(Some("Second.")),
            LIMIT,
            0,
            None,
        )
        .unwrap()
        .unwrap();
    let path = dir.path().join("typed.rz");
    let bytes = save16(&typed, &path);
    assert_table_before_ids(&bytes, &table(&[(0, SENTENCE), (1, "Second.")]));
    let reopened = open16(&path);
    assert_eq!(info16(&reopened, PLACED)["profile_refusal"], SENTENCE);
    assert_eq!(info16(&reopened, 2)["profile_refusal"], "Second.");
}

// ----------------------------------------------------------- corruption --

/// `projective_source_native.rs`'s `reject_bad_table`: poison the first
/// fallback PNG's payload so a reader that skipped the table check would
/// fail in decode instead, and require a refusal that never got there.
fn reject_bad_table(mut bytes: Vec<u8>, typed: bool, case: &str, path: &Path) {
    let png = position(&bytes, b"IDAT").unwrap();
    bytes[png + 4] ^= 0x7f;
    std::fs::write(path, bytes).unwrap();
    let error = if typed {
        match PrecisionDocument::open_native(path.to_str().unwrap(), LIMIT, 0, None) {
            Err(error) => error,
            Ok(_) => panic!("typed malformed {case} reopened"),
        }
    } else {
        match DrDocument::open(path.to_str().unwrap()) {
            Err(error) => error,
            Ok(_) => panic!("byte malformed {case} reopened"),
        }
    };
    let shown = error.replace(&path.to_string_lossy().into_owned(), "<path>");
    assert!(
        !shown.to_lowercase().contains("png") && !shown.contains("decode"),
        "{case} reached fallback decode before table refusal: {error}"
    );
}

#[test]
fn corrupt_refusal_table_corpus_refuses_before_fallback_at_both_depths() {
    let dir = TempDir::new().unwrap();
    let second = SourceEdit::parse(
        &json!({"op":"place","source_id":"second-master","revision_id":"first",
            "name":"Second","x":0,"y":0,"profile_refusal":"Second."})
        .to_string(),
    )
    .unwrap();
    for typed in [false, true] {
        let path = dir.path().join(if typed { "typed.rz" } else { "byte.rz" });
        let original = if typed {
            save16(
                &typed_with(Some("First."))
                    .source_edit(Some(&second_typed_master()), &second, LIMIT, 0, None)
                    .unwrap()
                    .unwrap(),
                &path,
            )
        } else {
            save8(
                &byte_with(Some("First."))
                    .source_edit(Some(&second_byte_master()), &second, LIMIT, 0, None)
                    .unwrap()
                    .unwrap(),
                &path,
            )
        };
        let expected = table(&[(0, "First."), (1, "Second.")]);
        assert_table_before_ids(&original, &expected);
        let at = position(&original, b"RSPR").unwrap();
        let length = expected.len();
        // Offsets of the two entries: index, length, then six/seven bytes.
        let first = at + 12;
        let second_entry = first + 8 + "First.".len();
        let mut cases: Vec<(&str, Vec<u8>)> = Vec::new();
        let mut mutate = |name: &'static str, change: &dyn Fn(&mut Vec<u8>)| {
            let mut bytes = original.clone();
            change(&mut bytes);
            cases.push((name, bytes));
        };
        mutate("bad tag", &|b| b[at] = b'X');
        mutate("bad schema", &|b| {
            b[at + 4..at + 8].copy_from_slice(&2_u32.to_le_bytes())
        });
        mutate("zero count", &|b| {
            b[at + 8..at + 12].copy_from_slice(&0_u32.to_le_bytes())
        });
        mutate("count past the assets", &|b| {
            b[at + 8..at + 12].copy_from_slice(&3_u32.to_le_bytes())
        });
        mutate("out of range asset", &|b| {
            b[second_entry..second_entry + 4].copy_from_slice(&2_u32.to_le_bytes())
        });
        mutate("duplicate asset", &|b| {
            b[second_entry..second_entry + 4].copy_from_slice(&0_u32.to_le_bytes())
        });
        mutate("unsorted assets", &|b| {
            b[first..first + 4].copy_from_slice(&1_u32.to_le_bytes());
            b[second_entry..second_entry + 4].copy_from_slice(&0_u32.to_le_bytes());
        });
        mutate("zero length", &|b| {
            b[first + 4..first + 8].copy_from_slice(&0_u32.to_le_bytes())
        });
        mutate("length past the cap", &|b| {
            b[first + 4..first + 8].copy_from_slice(&4097_u32.to_le_bytes())
        });
        mutate("not UTF-8", &|b| b[first + 8] = 0xff);
        mutate("NUL", &|b| b[first + 8] = 0);
        mutate("missing table", &|b| {
            b.drain(at..at + length);
        });
        mutate("extra table", &|b| {
            let copy = b[at..at + length].to_vec();
            b.splice(at + length..at + length, copy);
        });
        mutate("trailing bytes", &|b| b.extend_from_slice(b"TAIL"));
        for (case, bytes) in cases {
            reject_bad_table(bytes, typed, case, &path);
        }
    }
}

#[test]
fn a_refusal_word_over_a_source_free_body_refuses() {
    let dir = TempDir::new().unwrap();
    for typed in [false, true] {
        let path = dir.path().join(if typed { "typed.rz" } else { "byte.rz" });
        let mut bytes = if typed {
            save16(&typed_base(), &path)
        } else {
            save8(&byte_base(), &path)
        };
        assert!(position(&bytes, b"RSRC").is_none());
        let ids = bytes.windows(4).rposition(|w| w == b"LIDS").unwrap();
        bytes.splice(ids..ids, table(&[(0, "Orphan.")]));
        let word = if typed { TYPED_WORD } else { BYTE_WORD };
        bytes.splice(8..8, word.to_le_bytes());
        reject_bad_table(bytes, typed, "source-free body", &path);
    }
}

// -------------------------------------------------------------- fixtures --

#[test]
fn stored_previous_writer_files_reopen_and_resave_byte_for_byte() {
    let dir = TempDir::new().unwrap();
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");

    let stored = std::fs::read(root.join("rz_byte_53/placed_source.rz")).unwrap();
    assert_eq!(&stored[..4], b"RZDC");
    // This build rewrites it under its own magic (`DRDC`).
    let stored = [b"DRDC".as_slice(), &stored[4..]].concat();
    assert_eq!(drdc_word(&stored, 0), 45);
    assert_ne!(drdc_word(&stored, 1), BYTE_WORD);
    let opened =
        DrDocument::open(root.join("rz_byte_53/placed_source.rz").to_str().unwrap()).unwrap();
    assert!(info8(&opened, PLACED)["profile_refusal"].is_null());
    let path = dir.path().join("byte.rz");
    assert_eq!(save8(&opened, &path), stored);
    let fresh = save8(&byte_document(), &path);
    assert_eq!(without_identity(&fresh, 2), without_identity(&stored, 2));
    let cleared = edit8(
        &byte_with(Some(SENTENCE)),
        Some(&second_byte_master()),
        None,
        json!({"op":"replace","layer":PLACED,"revision_id":"second"}),
    );
    let bytes = save8(&cleared, &path);
    assert_ne!(drdc_word(&bytes, 1), BYTE_WORD);
    assert!(position(&bytes, b"RSPR").is_none());

    let stored = std::fs::read(root.join("rz_typed_55/placed_source.rz")).unwrap();
    assert_eq!(&stored[..4], b"RZDC");
    // This build rewrites it under its own magic (`DRDC`).
    let stored = [b"DRDC".as_slice(), &stored[4..]].concat();
    assert_eq!(drdc_word(&stored, 0), 46);
    assert_ne!(drdc_word(&stored, 1), TYPED_WORD);
    let opened = PrecisionDocument::open_native(
        root.join("rz_typed_55/placed_source.rz").to_str().unwrap(),
        LIMIT,
        0,
        None,
    )
    .unwrap();
    assert!(info16(&opened, PLACED)["profile_refusal"].is_null());
    let path = dir.path().join("typed.rz");
    assert_eq!(save16(&opened, &path), stored);
    let fresh = save16(&typed_document(), &path);
    assert_eq!(without_identity(&fresh, 2), without_identity(&stored, 2));
    let cleared = edit16(
        &typed_with(Some(SENTENCE)),
        None,
        Some(&second_typed_master()),
        json!({"op":"replace","layer":PLACED,"revision_id":"second"}),
    );
    let bytes = save16(&cleared, &path);
    assert_ne!(drdc_word(&bytes, 1), TYPED_WORD);
    assert!(position(&bytes, b"RSPR").is_none());
}

/// Writes the files `scripts/test_dr_probe_readers.py` reads with the
/// independent Python `.rz` readers. Run by hand:
/// `DR_OUT=<existing dir> make t F=source_profile_refusal
/// T=write_refusal_samples_for_python_readers A='--ignored --exact'`.
#[test]
#[ignore = "writes .doctored samples for the Python probe readers into $DR_OUT"]
fn write_refusal_samples_for_python_readers() {
    let out = std::path::PathBuf::from(std::env::var("DR_OUT").expect("DR_OUT"));
    assert!(out.is_dir(), "DR_OUT must be an existing directory");
    save8(&byte_with(Some(SENTENCE)), &out.join("byte.rz"));
    save16(&typed_with(Some(SENTENCE)), &out.join("typed.rz"));
    let perspective = perspective_set();
    let byte = byte_with(Some(SENTENCE))
        .geometry_edit(&perspective, LIMIT, 0, None)
        .unwrap()
        .unwrap();
    save8(&byte, &out.join("byte_projective.rz"));
    let typed = typed_with(Some(SENTENCE))
        .geometry(&perspective, LIMIT, 0, None)
        .unwrap()
        .unwrap();
    save16(&typed, &out.join("typed_projective.rz"));
    let grouped = structure16(
        &typed_with(Some(SENTENCE)),
        json!({"op":"group_layers","layers":[0],"name":"Group","pass_through":false}),
    );
    let closed = structure16(&grouped, json!({"op":"layer_open","layer":1,"open":false}));
    save16(&closed, &out.join("typed_closed_group.rz"));
    let byte = byte_with(None)
        .source_edit(
            Some(&second_byte_master()),
            &second(Some("Second.")),
            LIMIT,
            0,
            None,
        )
        .unwrap()
        .unwrap();
    save8(&byte, &out.join("byte_second_only.rz"));
    let typed = typed_with(Some(SENTENCE))
        .source_edit(
            Some(&second_typed_master()),
            &second(Some("Second.")),
            LIMIT,
            0,
            None,
        )
        .unwrap()
        .unwrap();
    save16(&typed, &out.join("typed_two.rz"));
    // The warp (53/54), knockout (49) and all-words samples; the warp
    // binds the base (layer 0).
    let warp = warp::edit(warp::field(3, 3, "bilinear"));
    let byte = byte_with(Some(SENTENCE))
        .warp_edit(&warp, LIMIT, 0, None)
        .unwrap()
        .unwrap();
    save8(&byte, &out.join("byte_warp.rz"));
    let byte = byte
        .geometry_edit(&perspective_on(PLACED), LIMIT, 0, None)
        .unwrap()
        .unwrap();
    save8(&knockout8(&byte, 0), &out.join("byte_all.rz"));
    let typed = typed_with(Some(SENTENCE))
        .warp_edit(&warp, LIMIT, 0, None)
        .unwrap()
        .unwrap();
    save16(&typed, &out.join("typed_warp.rz"));
    save8(
        &knockout8(&byte_with(None), 0),
        &out.join("byte_knockout.rz"),
    );
    // Postorder after grouping the base: Base 0, Group 1, Placed 2.
    let typed = closed
        .warp_edit(&warp, LIMIT, 0, None)
        .unwrap()
        .unwrap()
        .geometry(&perspective_on(2), LIMIT, 0, None)
        .unwrap()
        .unwrap();
    save16(&typed, &out.join("typed_all.rz"));
}
