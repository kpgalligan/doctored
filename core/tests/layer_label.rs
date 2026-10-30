//! Phase 12C-1: per-layer COLOUR LABELS at both depths.
//!
//! What is pinned here, all of it against references this file builds itself:
//! the setter's refusals and its no-op rule; the carriage rules (duplicate,
//! group/ungroup, a deep document copy, and the merge that keeps the LOWEST
//! entry's label); the `.rz` wrapper at both depths — 41 / 42 as the OUTERMOST
//! version word and one `LLBL` table whose bytes this file assembles
//! independently, closing the file after `EVAL` and after `FLOT` — the
//! unlabelled document's bytes being exactly what they were, and the refusals
//! a crafted table earns; and Photoshop's `lclr` in both directions at both
//! depths, read out of the exported bytes by this file's own record walk.

use std::ffi::{c_char, CString};
use std::path::Path;
use std::ptr;
use std::sync::Arc;

use doctored_core::doc::DrDocument;
use doctored_core::ffi_doc::*;
use doctored_core::ffi_evaluation::{dr_doc_evaluation, dr_doc_set_evaluation};
use doctored_core::ffi_group::*;
use doctored_core::ffi_precision::{dr_precision_doc_free, dr_precision_doc_info};
use doctored_core::ffi_precision_edit::dr_precision_doc_edit;
use doctored_core::ffi_psd::{
    dr_doc_open_psd, dr_doc_save_psd, dr_precision_doc_open_psd, dr_precision_doc_save_psd,
    dr_psd_import_report,
};
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer as Layer};
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use image::{Rgba, RgbaImage};
use serde_json::Value;
use tempfile::TempDir;

use crate::common;
use crate::psd16_fixture as psd16;
use crate::psd_mask_fixture as psd8;
use common::{add_layer, cpath, doc_from, opaque_pattern, take_err_string, test_pattern};

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
const LAYERS: i32 = 0;
const LINEAR: std::ffi::c_int = 1;

// ------------------------------------------------- independent references --

/// The `LLBL` table this build must write, assembled here from the model
/// alone: the tag, u32 schema 1, u32 count, then per entry a u32 index and a
/// u32 code. Nothing in this function reads `label_native`.
fn expected_table(entries: &[(u32, u32)]) -> Vec<u8> {
    let mut out = b"LLBL".to_vec();
    out.extend((1u32).to_le_bytes());
    out.extend((entries.len() as u32).to_le_bytes());
    for &(index, code) in entries {
        out.extend(index.to_le_bytes());
        out.extend(code.to_le_bytes());
    }
    out
}

fn word(bytes: &[u8], index: usize) -> u32 {
    let at = 4 + index * 4;
    u32::from_le_bytes(bytes[at..at + 4].try_into().expect("4 bytes"))
}

/// Phase 12E-1's always-written identity wrapper: 45/46 is now the OUTERMOST
/// word and its `LIDS` table closes every file, so the label word sits one
/// slot in and the `LLBL` table is found by its tag rather than by counting
/// back from the end.
const IDENTITY_BYTE: u32 = 45;
const IDENTITY_TYPED: u32 = 46;

fn llbl_at(bytes: &[u8]) -> usize {
    bytes.windows(4).rposition(|w| w == b"LLBL").expect("LLBL")
}

/// The `LLBL` table exactly as it sits in the file.
fn table_in(bytes: &[u8], table: &[u8]) -> bool {
    let at = llbl_at(bytes);
    &bytes[at..at + table.len()] == table
}

/// One record's additional layer blocks: key and verbatim body.
type Blocks = Vec<([u8; 4], Vec<u8>)>;

/// Every additional layer block of every record in a classic PSD's layer
/// information section, paired with the record's Pascal name. Written from
/// Adobe's published framing and deliberately depth-agnostic: it never looks
/// at a channel's samples, only at the declared lengths it must skip, so one
/// walk reads the 8-bit and the 16-bit writer's output alike.
fn record_blocks(bytes: &[u8]) -> Vec<(String, Blocks)> {
    let be32 = |at: usize| i32::from_be_bytes(bytes[at..at + 4].try_into().expect("4 bytes"));
    let mut cursor = 26usize;
    cursor += 4 + be32(cursor) as usize; // colour mode data
    cursor += 4 + be32(cursor) as usize; // image resources
    let section_len = be32(cursor) as usize;
    let section = &bytes[cursor + 4..cursor + 4 + section_len];
    let info_len = u32::from_be_bytes(section[..4].try_into().expect("4 bytes")) as usize;
    let mut info = &section[4..4 + info_len];
    if info_len == 0 {
        // Photoshop's 16-bit layout: the records live in an `Lr16` tail block
        // after the global layer mask, every tail block padded to a multiple
        // of four.
        let mut at = 4;
        at += 4 + u32::from_be_bytes(section[at..at + 4].try_into().expect("4 bytes")) as usize;
        while at + 12 <= section.len() {
            let key = &section[at + 4..at + 8];
            let len =
                u32::from_be_bytes(section[at + 8..at + 12].try_into().expect("4 bytes")) as usize;
            if key == b"Lr16" {
                info = &section[at + 12..at + 12 + len];
                break;
            }
            at += 12 + len + (len.wrapping_neg() & 3);
        }
    }
    if info.is_empty() {
        return Vec::new();
    }
    let word_at = |slice: &[u8], at: usize| {
        u32::from_be_bytes(slice[at..at + 4].try_into().expect("4 bytes")) as usize
    };
    let count = i16::from_be_bytes(info[..2].try_into().expect("2 bytes")).unsigned_abs() as usize;
    let mut at = 2usize;
    let mut out = Vec::with_capacity(count);
    for _ in 0..count {
        at += 16; // the record's rectangle
        let channels = u16::from_be_bytes(info[at..at + 2].try_into().expect("2 bytes")) as usize;
        at += 2 + channels * 6;
        at += 12; // signature, blend key, opacity, clipping, flags, filler
        let extra_len = word_at(info, at);
        let extra = &info[at + 4..at + 4 + extra_len];
        at += 4 + extra_len;

        let mut cursor = 0usize;
        cursor += 4 + word_at(extra, cursor); // the mask section
        cursor += 4 + word_at(extra, cursor); // the blending ranges
        let pascal_len = extra[cursor] as usize;
        let name =
            String::from_utf8_lossy(&extra[cursor + 1..cursor + 1 + pascal_len]).into_owned();
        cursor += 1 + pascal_len;
        while !cursor.is_multiple_of(4) {
            cursor += 1;
        }
        let mut blocks = Vec::new();
        while cursor + 12 <= extra.len() {
            assert_eq!(
                &extra[cursor..cursor + 4],
                b"8BIM",
                "additional block signature"
            );
            let key: [u8; 4] = extra[cursor + 4..cursor + 8].try_into().expect("4 bytes");
            let len = word_at(extra, cursor + 8);
            blocks.push((key, extra[cursor + 12..cursor + 12 + len].to_vec()));
            cursor += 12 + len + len % 2;
        }
        out.push((name, blocks));
    }
    out
}

/// The `lclr` code one record carries, or `None` when it carries no such
/// block. The body Photoshop writes is a big-endian u16 code then six
/// reserved bytes.
fn lclr(blocks: &Blocks) -> Option<u16> {
    blocks
        .iter()
        .find(|(key, _)| key == b"lclr")
        .map(|(_, body)| {
            assert_eq!(body.len(), 8, "an lclr body is eight bytes");
            assert_eq!(&body[2..], &[0u8; 6], "the six reserved bytes are zero");
            u16::from_be_bytes(body[..2].try_into().expect("2 bytes"))
        })
}

/// The eight-byte `lclr` body for a code, for the fixtures this file builds.
fn lclr_body(code: u16) -> Vec<u8> {
    let mut out = code.to_be_bytes().to_vec();
    out.extend([0u8; 6]);
    out
}

// ---------------------------------------------------------- byte plumbing --

fn fixture(dir: &TempDir) -> *mut DrDocument {
    let doc = doc_from(dir, "base.png", &opaque_pattern(4, 3));
    add_layer(dir, "top.png", doc, 0, &test_pattern(4, 3), "Top")
}

fn label(doc: *const DrDocument, idx: usize) -> u32 {
    unsafe { dr_doc_layer_label(doc, idx) }
}

/// Sets a label, asserting the call succeeded and freeing the old handle.
fn set_label(doc: *mut DrDocument, idx: usize, code: u32) -> *mut DrDocument {
    let out = unsafe { dr_doc_with_layer_label(doc, idx, code) };
    assert!(!out.is_null(), "setting label {code} on {idx}");
    unsafe { dr_doc_free(doc) };
    out
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

// ------------------------------------------------------------ the setter --

#[test]
fn the_setter_reads_back_and_refuses_what_it_should() {
    let dir = TempDir::new().unwrap();
    let doc = fixture(&dir);
    assert_eq!(label(doc, 0), 0, "a fresh layer is unlabelled");
    assert_eq!(label(doc, 1), 0);

    let doc = set_label(doc, 1, 4);
    assert_eq!(label(doc, 1), 4);
    assert_eq!(label(doc, 0), 0, "only the named entry changes");

    // Every code the palette names round-trips.
    for code in 1..=7u32 {
        let out = unsafe { dr_doc_with_layer_label(doc, 0, code) };
        assert!(!out.is_null(), "code {code} is legal");
        assert_eq!(label(out, 0), code);
        unsafe { dr_doc_free(out) };
    }

    assert!(
        unsafe { dr_doc_with_layer_label(doc, 1, 8) }.is_null(),
        "8 is past the palette"
    );
    assert!(
        unsafe { dr_doc_with_layer_label(doc, 1, u32::MAX) }.is_null(),
        "an absurd code is refused, not truncated"
    );
    assert!(
        unsafe { dr_doc_with_layer_label(doc, 9, 1) }.is_null(),
        "an out-of-range index is refused"
    );
    assert!(
        unsafe { dr_doc_with_layer_label(doc, 1, 4) }.is_null(),
        "the label the entry already carries is a no-op, so no undo step"
    );
    assert!(
        unsafe { dr_doc_with_layer_label(ptr::null(), 0, 1) }.is_null(),
        "a NULL document is tolerated"
    );
    assert_eq!(
        label(ptr::null(), 0),
        0,
        "a NULL document reads as unlabelled"
    );
    assert_eq!(label(doc, 9), 0, "a bad index reads as unlabelled");

    // Clearing is an ordinary set.
    let doc = set_label(doc, 1, 0);
    assert_eq!(label(doc, 1), 0);
    unsafe { dr_doc_free(doc) };
}

#[test]
fn duplicate_copies_a_label_and_a_merge_keeps_the_lowest() {
    let dir = TempDir::new().unwrap();
    let doc = fixture(&dir);
    let doc = set_label(doc, 0, 2);
    let doc = set_label(doc, 1, 5);

    let duplicated = unsafe { dr_doc_duplicating_layer(doc, 1) };
    assert!(!duplicated.is_null());
    assert_eq!(unsafe { dr_doc_layer_count(duplicated) }, 3);
    assert_eq!(label(duplicated, 0), 2, "the untouched entry keeps its own");
    assert_eq!(label(duplicated, 1), 5, "the original keeps its own");
    assert_eq!(label(duplicated, 2), 5, "the copy carries the label");
    unsafe { dr_doc_free(duplicated) };

    // The merge keeps the LOWEST merged entry's label — `doc_identity`'s rule.
    let indices = [0usize, 1];
    let merged = unsafe { dr_doc_merge_layers(doc, indices.as_ptr(), indices.len()) };
    assert!(!merged.is_null(), "the merge succeeds");
    assert_eq!(unsafe { dr_doc_layer_count(merged) }, 1);
    assert_eq!(label(merged, 0), 2, "the lowest entry's label survives");
    unsafe { dr_doc_free(merged) };

    // Flatten makes ONE new picture, so its entry is unlabelled.
    let flat = unsafe { dr_doc_flattening(doc) };
    assert!(!flat.is_null());
    assert_eq!(
        label(flat, 0),
        0,
        "a flatten produces a fresh, unmarked entry"
    );
    unsafe { dr_doc_free(flat) };
    unsafe { dr_doc_free(doc) };
}

#[test]
fn group_ungroup_and_a_document_copy_carry_labels() {
    let dir = TempDir::new().unwrap();
    let doc = fixture(&dir);
    let doc = set_label(doc, 0, 1);
    let doc = set_label(doc, 1, 6);

    let indices = [0usize, 1];
    let name = CString::new("Folder").unwrap();
    let mut group = usize::MAX;
    let grouped = unsafe {
        dr_doc_group_layers(
            doc,
            indices.as_ptr(),
            indices.len(),
            name.as_ptr(),
            &mut group,
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            0,
        )
    };
    assert!(!grouped.is_null(), "the group succeeds");
    assert_eq!(unsafe { dr_doc_layer_count(grouped) }, 3);
    assert_eq!(label(grouped, 0), 1, "a member keeps its label");
    assert_eq!(label(grouped, 1), 6);
    assert_eq!(label(grouped, group), 0, "a new container is unlabelled");

    // A container can be labelled too, and the flag survives the round trip.
    let grouped = set_label(grouped, group, 7);
    assert_eq!(label(grouped, group), 7);

    let copy = unsafe { dr_doc_clone(grouped) };
    assert!(!copy.is_null());
    for idx in 0..3 {
        assert_eq!(label(copy, idx), label(grouped, idx), "copy of entry {idx}");
    }
    unsafe { dr_doc_free(copy) };

    let ungrouped =
        unsafe { dr_doc_ungroup_layer(grouped, group, ptr::null_mut(), ptr::null_mut(), 0) };
    assert!(!ungrouped.is_null(), "the ungroup succeeds");
    assert_eq!(unsafe { dr_doc_layer_count(ungrouped) }, 2);
    assert_eq!(label(ungrouped, 0), 1, "the children keep their labels");
    assert_eq!(label(ungrouped, 1), 6);
    unsafe { dr_doc_free(ungrouped) };
    unsafe { dr_doc_free(grouped) };
    unsafe { dr_doc_free(doc) };
}

// ----------------------------------------------------- the byte .rz table --

#[test]
fn an_unlabelled_byte_document_writes_exactly_the_prior_writers_bytes() {
    let dir = TempDir::new().unwrap();
    let doc = fixture(&dir);
    let plain = save(doc, &dir.path().join("plain.rz"));
    assert_ne!(word(&plain, 0), 41, "no label wrapper is written");
    assert!(
        !plain.windows(4).any(|w| w == b"LLBL"),
        "an unlabelled document carries no table"
    );

    // Setting a label and clearing it again reproduces those exact bytes,
    // which is the round-trip form of "the prior writer's file is unchanged".
    let doc = set_label(doc, 1, 3);
    let doc = set_label(doc, 1, 0);
    let again = save(doc, &dir.path().join("again.rz"));
    assert_eq!(again, plain, "releasing the last label restores the bytes");
    unsafe { dr_doc_free(doc) };
}

#[test]
fn a_labelled_byte_document_round_trips_through_an_independent_table() {
    let dir = TempDir::new().unwrap();
    let doc = fixture(&dir);
    let doc = set_label(doc, 0, 2);
    let doc = set_label(doc, 1, 7);
    let path = dir.path().join("labelled.rz");
    let bytes = save(doc, &path);

    assert_eq!(&bytes[..4], b"DRDC");
    assert_eq!(
        word(&bytes, 0),
        IDENTITY_BYTE,
        "the identity word is outermost"
    );
    assert_eq!(word(&bytes, 1), 41, "41 is the word inside it");
    let table = expected_table(&[(0, 2), (1, 7)]);
    assert!(
        table_in(&bytes, &table),
        "the table this file assembled sits at LLBL"
    );

    let reopened = open_ok(&path);
    assert_eq!(unsafe { dr_doc_layer_count(reopened) }, 2);
    assert_eq!(label(reopened, 0), 2);
    assert_eq!(label(reopened, 1), 7);
    // And writing it again is byte-identical.
    assert_eq!(save(reopened, &dir.path().join("twice.rz")), bytes);
    unsafe { dr_doc_free(reopened) };
    unsafe { dr_doc_free(doc) };
}

#[test]
fn the_label_wrapper_nests_outside_the_outside_mask_and_evaluation_tables() {
    let dir = TempDir::new().unwrap();
    // The outside-mask feature is stored state on the model, exactly as
    // `evaluation_native`'s own test sets it.
    let base = DrDocument::from_pixels(RgbaImage::from_fn(4, 2, |x, y| {
        Rgba([(x * 60) as u8, (y * 90) as u8, 120, 255])
    }));
    let mut doc = base
        .adding_image_layer(
            0,
            RgbaImage::from_fn(4, 2, |x, _| Rgba([200, (x * 40) as u8, 20, 180])),
            "Top",
        )
        .expect("a second layer")
        .add_mask(1, doctored_core::doc::MaskKind::FromSelection(&[255u8; 8]))
        .expect("a mask");
    doc.layers[1].mask_properties.outside_coverage = 255;
    let handle = Box::into_raw(Box::new(doc));

    let mut err: *mut c_char = ptr::null_mut();
    let linear = unsafe { dr_doc_set_evaluation(handle, LINEAR, &mut err) };
    assert!(!linear.is_null(), "{}", take_err_string(err));
    unsafe { dr_doc_free(handle) };
    let linear = set_label(linear, 1, 5);

    let path = dir.path().join("all.rz");
    let bytes = save(linear, &path);
    assert_eq!(
        word(&bytes, 0),
        IDENTITY_BYTE,
        "the identity word is outermost"
    );
    assert_eq!(word(&bytes, 1), 41, "the label word is inside it");
    assert_eq!(word(&bytes, 2), 38, "then the evaluation word");
    assert_eq!(word(&bytes, 3), 28, "then the outside-mask word");

    // MOUT, then EVAL, then LLBL, in that order.
    let table = expected_table(&[(1, 5)]);
    assert!(table_in(&bytes, &table));
    let mout = bytes.windows(4).position(|w| w == b"MOUT").expect("MOUT");
    let eval = bytes.windows(4).position(|w| w == b"EVAL").expect("EVAL");
    let llbl = bytes.windows(4).position(|w| w == b"LLBL").expect("LLBL");
    assert!(
        mout < eval && eval < llbl,
        "MOUT {mout} EVAL {eval} LLBL {llbl}"
    );

    let reopened = open_ok(&path);
    assert_eq!(label(reopened, 1), 5);
    assert_eq!(unsafe { dr_doc_evaluation(reopened) }, LINEAR);
    unsafe { dr_doc_free(reopened) };
    unsafe { dr_doc_free(linear) };
}

#[test]
fn a_crafted_label_table_refuses_the_open() {
    let dir = TempDir::new().unwrap();
    let doc = fixture(&dir);
    let doc = set_label(doc, 1, 3);
    let good = save(doc, &dir.path().join("good.rz"));
    unsafe { dr_doc_free(doc) };
    let table = expected_table(&[(1, 3)]);
    // `LIDS` closes the file since 12E-1, so the table is located by its tag.
    let head = llbl_at(&good);
    let tail = head + table.len();

    let write = |name: &str, bytes: &[u8]| -> std::path::PathBuf {
        let path = dir.path().join(name);
        std::fs::write(&path, bytes).unwrap();
        path
    };

    let mut zero_code = good.clone();
    zero_code[tail - 4..tail].copy_from_slice(&0u32.to_le_bytes());
    assert!(
        open_err(&write("code0.rz", &zero_code), "code 0").contains("label code"),
        "the refusal names the code"
    );

    let mut big_code = good.clone();
    big_code[tail - 4..tail].copy_from_slice(&8u32.to_le_bytes());
    assert!(
        open_err(&write("code8.rz", &big_code), "code 8").contains("label code"),
        "the refusal names the code"
    );

    let mut bad_index = good.clone();
    bad_index[tail - 8..tail - 4].copy_from_slice(&9u32.to_le_bytes());
    assert!(
        open_err(&write("index.rz", &bad_index), "a bad index").contains("order or index"),
        "the refusal names the index"
    );

    let mut empty = good[..head].to_vec();
    empty.extend(expected_table(&[]));
    empty.extend_from_slice(&good[tail..]);
    assert!(
        open_err(&write("empty.rz", &empty), "an empty table").contains("label count"),
        "the refusal names the count"
    );

    let mut schema = good.clone();
    schema[head + 4..head + 8].copy_from_slice(&2u32.to_le_bytes());
    assert!(
        open_err(&write("schema.rz", &schema), "schema 2").contains("label schema"),
        "the refusal names the schema"
    );

    let mut trailing = good.clone();
    trailing.extend(&7u32.to_le_bytes());
    assert!(
        open_err(&write("trailing.rz", &trailing), "a trailing word").contains("trailing"),
        "the refusal names the trailing bytes"
    );
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
    let inner = Layer::raster("Inner", image(1000), (0, 0), 1.0, true).unwrap();
    let group = Layer::isolated_normal("Folder", vec![inner], 1.0, true).unwrap();
    PrecisionDocument::from_parts(
        3,
        2,
        Arc::clone(&profile),
        Metadata::default(),
        Resolution::default(),
        vec![
            Layer::raster("Base", image(2000), (0, 0), 1.0, true).unwrap(),
            group,
        ],
    )
    .unwrap()
}

fn info(document: &PrecisionDocument) -> Value {
    let mut err = ptr::null_mut();
    let json = unsafe { dr_precision_doc_info(document, &mut err) };
    assert!(!json.is_null(), "{}", take_err_string(err));
    serde_json::from_str(&take_err_string(json)).expect("info JSON")
}

fn typed_labels(document: &PrecisionDocument) -> Vec<u64> {
    info(document)["layers"]
        .as_array()
        .expect("layers")
        .iter()
        .map(|row| row["label"].as_u64().expect("a label row"))
        .collect()
}

/// The `layer_label` edit, or the error it reported. A NULL result with no
/// error is the exact no-op the core answers for an unchanged value.
fn edit_label(
    document: &PrecisionDocument,
    layer: usize,
    code: u8,
) -> Result<Option<Owned>, String> {
    let request = format!(r#"{{"op":"layer_label","layer":{layer},"label":{code}}}"#);
    let c = CString::new(request).unwrap();
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

fn set_typed_label(document: &PrecisionDocument, layer: usize, code: u8) -> Owned {
    match edit_label(document, layer, code) {
        Ok(Some(out)) => out,
        Ok(None) => panic!("label {code} on record {layer} reported no change"),
        Err(message) => panic!("label {code} on record {layer}: {message}"),
    }
}

#[test]
fn the_typed_edit_reads_back_and_refuses_what_it_should() {
    let document = typed_fixture();
    assert_eq!(typed_labels(&document), vec![0, 0, 0]);

    let labelled = set_typed_label(&document, 0, 3);
    assert_eq!(typed_labels(&labelled), vec![3, 0, 0]);
    let both = set_typed_label(&labelled, 2, 6);
    assert_eq!(
        typed_labels(&both),
        vec![3, 0, 6],
        "postorder record indices"
    );

    assert!(
        edit_label(&both, 0, 3).expect("admitted").is_none(),
        "the same value is an exact no-op"
    );
    assert!(
        edit_label(&both, 0, 8).is_err(),
        "a code past the palette refuses"
    );
    assert!(
        edit_label(&both, 9, 1).is_err(),
        "an out-of-range record refuses"
    );
}

#[test]
fn a_labelled_typed_document_round_trips_through_an_independent_table() {
    let document = typed_fixture();
    let plain = document.encode_native(LIMIT, 0, None).expect("plain bytes");
    assert_eq!(
        word(&plain, 0),
        IDENTITY_TYPED,
        "the identity word is outermost"
    );
    assert_ne!(word(&plain, 1), 42, "no label wrapper is written");
    assert!(!plain.windows(4).any(|w| w == b"LLBL"));

    let labelled = set_typed_label(&document, 1, 4);
    let labelled = set_typed_label(&labelled, 2, 1);
    let bytes = labelled.encode_native(LIMIT, 0, None).expect("bytes");
    assert_eq!(
        word(&bytes, 0),
        IDENTITY_TYPED,
        "the identity word is outermost"
    );
    assert_eq!(word(&bytes, 1), 42, "42 is the word inside it");
    let table = expected_table(&[(1, 4), (2, 1)]);
    assert!(
        table_in(&bytes, &table),
        "the independently assembled table sits at LLBL"
    );

    let reopened = PrecisionDocument::parse_native(&bytes, LIMIT, 0, None).expect("reopen");
    assert_eq!(typed_labels(&reopened), vec![0, 4, 1]);
    assert_eq!(
        reopened.encode_native(LIMIT, 0, None).expect("again"),
        bytes,
        "writing the reopened document is byte-identical"
    );

    // Releasing the last label restores the prior writer's exact bytes.
    let cleared = set_typed_label(&labelled, 1, 0);
    let cleared = set_typed_label(&cleared, 2, 0);
    assert_eq!(
        cleared.encode_native(LIMIT, 0, None).expect("cleared"),
        plain
    );
}

#[test]
fn a_labelled_float_document_writes_the_table_after_flot() {
    let document =
        PrecisionDocument::open_preserving("tests/fixtures/float_11f/scene.tiff", LIMIT, 0, None)
            .expect("the float fixture opens");
    assert!(document.is_float());
    let labelled = set_typed_label(&document, 0, 2);
    let bytes = labelled.encode_native(LIMIT, 0, None).expect("bytes");
    assert_eq!(
        word(&bytes, 0),
        IDENTITY_TYPED,
        "the identity word is outermost"
    );
    assert_eq!(word(&bytes, 1), 42, "the label word wraps the float word");
    assert_eq!(word(&bytes, 2), 40);
    assert_eq!(word(&bytes, 3), 39);
    let table = expected_table(&[(0, 2)]);
    assert!(table_in(&bytes, &table));
    let flot = bytes.windows(4).position(|w| w == b"FLOT").expect("FLOT");
    let llbl = bytes.windows(4).position(|w| w == b"LLBL").expect("LLBL");
    assert!(flot < llbl, "LLBL follows FLOT: {flot} {llbl}");

    let reopened = PrecisionDocument::parse_native(&bytes, LIMIT, 0, None).expect("reopen");
    assert!(reopened.is_float());
    assert_eq!(typed_labels(&reopened)[0], 2);
}

// ------------------------------------------------------------------- PSD --

#[test]
fn an_eight_bit_psd_carries_lclr_in_both_directions() {
    let dir = TempDir::new().unwrap();
    // IMPORT: a file this test builds, with `lclr` on one of two records.
    let rgba: Vec<u8> = (0..4u8).flat_map(|i| [i * 9, 40, 80, 255]).collect();
    let mut marked = psd8::Record::raster([0, 0, 2, 2], &rgba, false);
    marked.name = "Marked";
    marked.blocks = psd8::block(b"lclr", &lclr_body(3));
    let mut plain = psd8::Record::raster([0, 0, 2, 2], &rgba, false);
    plain.name = "Plain";
    let bytes = psd8::fixture(&[marked, plain], [2, 2], false);
    let path = dir.path().join("labelled.psd");
    std::fs::write(&path, &bytes).unwrap();

    let mut err = ptr::null_mut();
    let json = unsafe { dr_psd_import_report(cpath(&path).as_ptr(), &mut err) };
    assert!(!json.is_null(), "{}", take_err_string(err));
    let notice: Value = serde_json::from_str(&take_err_string(json)).expect("report");
    let fingerprint = CString::new(notice["source_fingerprint"].as_str().unwrap()).unwrap();
    let mut err = ptr::null_mut();
    let imported = unsafe {
        dr_doc_open_psd(
            cpath(&path).as_ptr(),
            LAYERS,
            fingerprint.as_ptr(),
            &mut err,
        )
    };
    assert!(!imported.is_null(), "{}", take_err_string(err));
    assert_eq!(unsafe { dr_doc_layer_count(imported) }, 2);
    // PSD records are top-down; the document stack is bottom-first.
    // A PSD stores its layer records bottom-first, so the first record built
    // above is the document's entry 0.
    assert_eq!(
        label(imported, 0),
        3,
        "the marked record imported its label"
    );
    assert_eq!(label(imported, 1), 0, "the plain record imported none");

    // EXPORT: the writer's own bytes, read back by this file's record walk.
    let exported = dir.path().join("exported.psd");
    let mut err = ptr::null_mut();
    let ok = unsafe {
        dr_doc_save_psd(
            imported,
            ptr::null(),
            cpath(&exported).as_ptr(),
            0,
            LAYERS,
            &mut err,
        )
    };
    assert!(ok, "{}", take_err_string(err));
    unsafe { dr_doc_free(imported) };
    let written = std::fs::read(&exported).expect("the exported file");
    let records = record_blocks(&written);
    assert_eq!(records.len(), 2, "two records");
    let codes: Vec<Option<u16>> = records.iter().map(|(_, blocks)| lclr(blocks)).collect();
    assert_eq!(
        codes,
        vec![Some(3), None],
        "one eight-byte lclr per labelled record and none otherwise"
    );
}

#[test]
fn a_sixteen_bit_psd_carries_lclr_in_both_directions() {
    let dir = TempDir::new().unwrap();
    let samples: Vec<[u16; 4]> = (0..4u16)
        .map(|i| [1000 + i * 500, 22222, 33333, 65535])
        .collect();
    let plane = |component: usize| {
        psd16::raw_plane(&samples.iter().map(|p| p[component]).collect::<Vec<_>>())
    };
    let channels = vec![
        (-1i16, plane(3)),
        (0, plane(0)),
        (1, plane(1)),
        (2, plane(2)),
    ];
    let mut marked = psd16::Record::raster("Marked", [0, 0, 2, 2], channels.clone());
    marked.blocks = psd16::block(b"lclr", &lclr_body(6));
    let plain = psd16::Record::raster("Plain", [0, 0, 2, 2], channels);
    let composite: Vec<[u16; 4]> = samples.clone();
    let bytes = psd16::fixture(
        (2, 2),
        &[marked, plain],
        IccProfile::srgb().bytes(),
        (72.0, 72.0),
        &composite,
        false,
        false,
    );
    let path = dir.path().join("labelled16.psd");
    std::fs::write(&path, &bytes).unwrap();

    let mut err = ptr::null_mut();
    let json = unsafe { dr_psd_import_report(cpath(&path).as_ptr(), &mut err) };
    assert!(!json.is_null(), "{}", take_err_string(err));
    let notice: Value = serde_json::from_str(&take_err_string(json)).expect("report");
    assert_eq!(notice["import_precision"], "rgba16");
    let fingerprint = CString::new(notice["source_fingerprint"].as_str().unwrap()).unwrap();
    let mut err = ptr::null_mut();
    let imported = unsafe {
        dr_precision_doc_open_psd(
            cpath(&path).as_ptr(),
            LAYERS,
            fingerprint.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!imported.is_null(), "{}", take_err_string(err));
    let imported = Owned(imported);
    // Postorder records, bottom-first: a PSD's first record is the bottom.
    assert_eq!(typed_labels(&imported), vec![6, 0]);

    let projection = imported.render(LIMIT, 0, None).expect("projection");
    let exported = dir.path().join("exported16.psd");
    let mut err = ptr::null_mut();
    let ok = unsafe {
        dr_precision_doc_save_psd(
            imported.0,
            &projection,
            cpath(&exported).as_ptr(),
            0,
            LAYERS,
            LIMIT,
            0,
            &mut err,
        )
    };
    assert!(ok, "{}", take_err_string(err));
    let written = std::fs::read(&exported).expect("the exported file");
    let records = record_blocks(&written);
    assert_eq!(records.len(), 2);
    let codes: Vec<Option<u16>> = records.iter().map(|(_, blocks)| lclr(blocks)).collect();
    assert_eq!(
        codes,
        vec![Some(6), None],
        "one eight-byte lclr per labelled record and none otherwise"
    );
}
