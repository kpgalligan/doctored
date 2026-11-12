//! The TYPED group disclosure — the byte `Layer::open` twin.
//!
//! Every rule checked here is the byte one (`doc_group_ops::with_layer_open`):
//! groups only, the same state is a no-op, no pixel changes. The persisted
//! form is checked against a table this file assembles itself — `LOPN`, u32
//! schema 1, u32 count, then the closed groups' postorder indices — and the
//! all-open file against the bytes of a document that was never touched, so
//! the conditional word 55 is proven absent rather than assumed.
//!
//! # The stored 54-writer file
//!
//! `tests/fixtures/rz_typed_54/nested_all_open.rz` is a FORMAT ORACLE, as
//! `psd_10c/*.psd` is — not a golden image. It was written by the writer
//! that predates word 55, never by this build, and nothing in this tree
//! regenerates it:
//!
//! - commit: ec795d5a03da7c77a22f358d56a8e1ee72864980 (ec795d5, the last
//!   writer without word 55), checked out in a scratch `git worktree`;
//! - builder: a throwaway, never-committed `core/tests/gen_rz54.rs` holding
//!   this file's `image`, `raster` and `nested` verbatim and one ignored test
//!   writing `nested().encode_native(REFERENCE_WORKING_BYTES, 0, None)` to
//!   `$DR54_OUT`;
//! - command: `cd <worktree>/core && CARGO_TARGET_DIR=<scratch> DR54_OUT=<repo>/core/tests/fixtures/rz_typed_54/nested_all_open.rz
//!   cargo test --release --test gen_rz54 -- --ignored`;
//! - bytes: 3266; sha256
//!   `0b67418b4e4fc95865118bf9f48b29771545f50a361498e1610bb67ac29e6838`.
//!
//! The current writer must reproduce it byte for byte. The planes inside it
//! are encoded by the PNG crate, so a dependency bump that moves the PNG
//! encoder's bytes fails that test ON PURPOSE: the stored file is what an
//! older build wrote, and a writer that no longer reproduces it has changed
//! the format.

use std::ffi::{c_char, CStr, CString};
use std::path::Path;
use std::ptr;

use doctored_core::doc::{DrDocument, LayerKind};
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_precision_edit::dr_precision_doc_edit;
use doctored_core::ffi_psd::{
    dr_precision_doc_open_psd, dr_precision_doc_save_psd, dr_psd_import_report,
};
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer as Layer};
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use doctored_core::precision_quantize::PrecisionDither;
use image::RgbaImage;
use serde_json::{json, Value};
use tempfile::TempDir;

use crate::identity;
use crate::psd16_fixture as fixture;

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
/// `LOCK_ALL`, restated here rather than read out of the core.
const LOCK_ALL: u32 = 7;
const WORD: u32 = 55;

// ------------------------------------------------------------- plumbing --

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

fn edit(
    document: &PrecisionDocument,
    request: Value,
) -> Result<Option<Box<PrecisionDocument>>, String> {
    let request = CString::new(request.to_string()).unwrap();
    let mut error = ptr::null_mut();
    let result = unsafe {
        dr_precision_doc_edit(
            document,
            request.as_ptr(),
            ptr::null(),
            0,
            LIMIT,
            0,
            ptr::null(),
            &mut error,
        )
    };
    if let Some(error) = take_error(error) {
        return Err(error);
    }
    Ok((!result.is_null()).then(|| unsafe { Box::from_raw(result) }))
}

fn must(document: &PrecisionDocument, request: Value) -> Box<PrecisionDocument> {
    match edit(document, request.clone()) {
        Ok(Some(out)) => out,
        Ok(None) => panic!("{request} reported no change"),
        Err(message) => panic!("{request}: {message}"),
    }
}

fn refusal(document: &PrecisionDocument, request: Value) -> String {
    match edit(document, request.clone()) {
        Err(message) => message,
        Ok(Some(_)) => panic!("{request} must be refused"),
        Ok(None) => panic!("{request} must be refused, not answered as a no-op"),
    }
}

fn close(document: &PrecisionDocument, layer: usize) -> Box<PrecisionDocument> {
    must(
        document,
        json!({"op": "layer_open", "layer": layer, "open": false}),
    )
}

fn rows(document: &PrecisionDocument) -> Vec<Value> {
    let value: Value =
        serde_json::from_str(&document.info().expect("info").to_string()).expect("info JSON");
    value["layers"].as_array().expect("layer rows").clone()
}

fn open_of(document: &PrecisionDocument, index: usize) -> bool {
    rows(document)[index]["open"]
        .as_bool()
        .expect("every row carries a bool open")
}

fn index_named(document: &PrecisionDocument, name: &str) -> usize {
    rows(document)
        .iter()
        .position(|row| row["name"] == name)
        .unwrap_or_else(|| panic!("no record named {name}"))
}

fn render(document: &PrecisionDocument) -> Vec<[u16; 4]> {
    document
        .render(LIMIT, 0, None)
        .expect("render")
        .to_rgba16()
        .expect("u16 projection")
}

fn word(bytes: &[u8], index: usize) -> u32 {
    let at = 4 + index * 4;
    u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap())
}

fn u32_at(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap())
}

// ------------------------------------------------------------- fixtures --

fn image(seed: u16) -> PrecisionImage {
    let pixels = (0..4)
        .map(|i| {
            [
                seed.wrapping_mul(97).wrapping_add(i * 911),
                40_000,
                9_000 + i * 3,
                50_000,
            ]
        })
        .collect();
    PrecisionImage::from_rgba16(2, 2, pixels, IccProfile::srgb(), None).expect("an image")
}

fn raster(name: &str, seed: u16) -> Layer {
    Layer::raster(name, image(seed), (0, 0), 0.8, true).expect("a raster")
}

/// `Base`, then group `G` holding `A` and `B`, then `Top`. Postorder: Base 0,
/// A 1, B 2, G 3, Top 4.
fn nested() -> PrecisionDocument {
    let group = Layer::isolated_normal("G", vec![raster("A", 2), raster("B", 3)], 0.9, true)
        .expect("a group");
    PrecisionDocument::from_parts(
        2,
        2,
        IccProfile::srgb(),
        Metadata::default(),
        Resolution::default(),
        vec![raster("Base", 1), group, raster("Top", 4)],
    )
    .expect("a document")
}

/// `nested` with a second group `H` holding `C`: postorder Base 0, A 1, B 2,
/// G 3, C 4, H 5, Top 6.
fn two_groups() -> PrecisionDocument {
    let g = Layer::isolated_normal("G", vec![raster("A", 2), raster("B", 3)], 0.9, true)
        .expect("a group");
    let h = Layer::pass_through("H", vec![raster("C", 5)], 1.0, true).expect("a group");
    PrecisionDocument::from_parts(
        2,
        2,
        IccProfile::srgb(),
        Metadata::default(),
        Resolution::default(),
        vec![raster("Base", 1), g, h, raster("Top", 4)],
    )
    .expect("a document")
}

// ------------------------------------------------------------ (a) edit --

#[test]
fn close_reopen_and_no_op_answer_as_the_byte_setter_does() {
    let base = nested();
    assert!(
        rows(&base).iter().all(|row| row["open"] == json!(true)),
        "every row, group or not, starts open"
    );
    let before = render(&base);

    let closed = close(&base, 3);
    assert!(!open_of(&closed, 3));
    assert_eq!(render(&closed), before, "the disclosure changes no pixel");
    assert!(
        edit(
            &closed,
            json!({"op": "layer_open", "layer": 3, "open": false})
        )
        .expect("not an error")
        .is_none(),
        "the same state is the exact no-op"
    );
    let reopened = must(
        &closed,
        json!({"op": "layer_open", "layer": 3, "open": true}),
    );
    assert!(open_of(&reopened, 3));
    assert_eq!(render(&reopened), before);

    // A raster and an out-of-range index refuse by name.
    assert_eq!(
        refusal(
            &base,
            json!({"op": "layer_open", "layer": 0, "open": false})
        ),
        "high-depth layer open state applies to groups only"
    );
    assert!(refusal(
        &base,
        json!({"op": "layer_open", "layer": 9, "open": false})
    )
    .contains("out of range"));

    // Lock-exempt: a fully locked group still toggles.
    let locked = must(
        &base,
        json!({"op": "layer_lock", "layer": 3, "locks": LOCK_ALL}),
    );
    let closed = close(&locked, 3);
    assert!(!open_of(&closed, 3));
    assert_eq!(rows(&closed)[3]["locks"], json!(LOCK_ALL));
}

// ------------------------------------------------------- (b) survival --

#[test]
fn a_closed_group_stays_closed_through_structure_edits_around_it() {
    let closed = close(&nested(), 3);

    let renamed = must(
        &closed,
        json!({"op": "rename_layer", "layer": 1, "name": "A2"}),
    );
    assert!(!open_of(&renamed, index_named(&renamed, "G")));

    // Move `Base` into the group, below `A`.
    let moved = must(
        &closed,
        json!({"op": "move_layer_to", "from": 0, "to": 0, "depth": 1}),
    );
    let g = index_named(&moved, "G");
    assert_eq!(
        rows(&moved)[g]["subtree_start"],
        json!(0),
        "Base moved inside G"
    );
    assert!(!open_of(&moved, g));

    let hidden = must(
        &closed,
        json!({"op": "layer_visible", "layer": 2, "visible": false}),
    );
    assert!(!open_of(&hidden, 3));

    let wrapped = must(
        &closed,
        json!({"op": "group_layers", "layers": [3], "name": "Outer"}),
    );
    assert!(!open_of(&wrapped, index_named(&wrapped, "G")));
    assert!(
        open_of(&wrapped, index_named(&wrapped, "Outer")),
        "a new group opens"
    );
}

// ------------------------------------------------------------ (c) native --

fn native_round_trip(base: &PrecisionDocument, group: usize) {
    let plain = base.encode_native(LIMIT, 0, None).expect("plain bytes");
    assert!(!plain.windows(4).any(|w| w == b"LOPN"));
    assert_ne!(word(&plain, 1), WORD);

    let closed = close(base, group);
    let reopened_open = must(
        &closed,
        json!({"op": "layer_open", "layer": group, "open": true}),
    );
    assert_eq!(
        reopened_open
            .encode_native(LIMIT, 0, None)
            .expect("reopened"),
        plain,
        "open → close → open writes the untouched document's exact bytes"
    );

    let bytes = closed.encode_native(LIMIT, 0, None).expect("closed bytes");
    assert_eq!(&bytes[..4], b"DRDC");
    assert_eq!(word(&bytes, 0), 46, "the identity word stays outermost");
    assert_eq!(word(&bytes, 1), WORD, "55 is the first word inside 46");

    let mut table = b"LOPN".to_vec();
    table.extend(1u32.to_le_bytes());
    table.extend(1u32.to_le_bytes());
    table.extend((group as u32).to_le_bytes());
    let tail = bytes.len() - (12 + 8 * closed.layer_count());
    assert_eq!(&bytes[tail..tail + 4], b"LIDS", "LIDS stays LAST");
    assert_eq!(
        &bytes[tail - table.len()..tail],
        &table[..],
        "LOPN, as this file assembles it, sits immediately before LIDS"
    );

    let reopened = PrecisionDocument::parse_native(&bytes, LIMIT, 0, None).expect("reopen");
    assert!(!open_of(&reopened, group), "the group reopens closed");
    assert_eq!(reopened.is_float(), base.is_float());
    assert_eq!(
        reopened.encode_native(LIMIT, 0, None).expect("again"),
        bytes,
        "reopen then resave is byte-identical"
    );
}

#[test]
fn the_disclosure_round_trips_the_native_file_and_all_open_is_the_54_writer() {
    native_round_trip(&nested(), 3);
}

/// Byte equality that reports where it breaks: the first differing offset,
/// as `psd_export.rs`'s `compare_golden` does.
fn assert_same_bytes(what: &str, wrote: &[u8], stored: &[u8]) {
    // The stored file predates the rename and starts `RZDC`; this build
    // writes `DRDC`. Every byte after the magic must match.
    assert_eq!(&wrote[..4], b"DRDC", "{what}: this build's magic");
    assert_eq!(&stored[..4], b"RZDC", "{what}: the stored file's magic");
    if wrote[4..] == stored[4..] {
        return;
    }
    let offset = wrote
        .iter()
        .zip(stored)
        .position(|(a, b)| a != b)
        .unwrap_or(wrote.len().min(stored.len()));
    panic!(
        "{what}: wrote {} bytes, the stored 54-writer file has {} bytes; first mismatch at offset {offset}",
        wrote.len(),
        stored.len()
    );
}

/// The all-open file is pinned against a file the 54 writer (ec795d5)
/// wrote, not just against this build's own output.
#[test]
fn an_all_open_document_rewrites_the_stored_54_file_byte_for_byte() {
    const G: &[u8] = include_bytes!("fixtures/rz_typed_54/nested_all_open.rz");

    // (a) The stored file is a 54-writer file: identity word 46 outermost,
    // no word 55, no LOPN table.
    assert_eq!(&G[..4], b"RZDC");
    assert_eq!(word(G, 0), 46, "the identity word is outermost");
    assert_ne!(word(G, 1), WORD, "the 54 writer never wrote word 55");
    assert!(!G.windows(4).any(|w| w == b"LOPN"), "and no LOPN table");

    // (b) It opens with every row open and rewrites to itself.
    let stored = PrecisionDocument::parse_native(G, LIMIT, 0, None).expect("the stored file opens");
    assert!(
        rows(&stored).iter().all(|row| row["open"] == json!(true)),
        "every row of a pre-55 file opens"
    );
    assert_same_bytes(
        "reopen then resave",
        &stored.encode_native(LIMIT, 0, None).expect("resave"),
        G,
    );

    // (c) Closing G turns word 55 on; reopening G turns it off again and
    // writes the stored bytes.
    let closed = close(&stored, 3);
    let closed_bytes = closed.encode_native(LIMIT, 0, None).expect("closed bytes");
    assert_eq!(
        word(&closed_bytes, 1),
        WORD,
        "a closed group writes word 55"
    );
    let reopened = must(
        &closed,
        json!({"op": "layer_open", "layer": 3, "open": true}),
    );
    assert_same_bytes(
        "close then reopen",
        &reopened
            .encode_native(LIMIT, 0, None)
            .expect("reopened bytes"),
        G,
    );

    // (d) A freshly built document writes the same file up to its identity
    // wrapper: layer ids come from a process-global counter, so only (b) and
    // (c) compare the LIDS table.
    let fresh = nested().encode_native(LIMIT, 0, None).expect("fresh bytes");
    assert_same_bytes(
        "a fresh document, without its identity wrapper",
        &identity::without_identity(&fresh, 5),
        &identity::without_identity(G, 5),
    );
}

#[test]
fn the_disclosure_round_trips_a_float_native_file() {
    let document =
        PrecisionDocument::open_preserving("tests/fixtures/float_11f/scene.tiff", LIMIT, 0, None)
            .expect("the float fixture opens");
    assert!(document.is_float());
    let grouped = must(
        &document,
        json!({"op": "group_layers", "layers": [0], "name": "Float group"}),
    );
    let group = index_named(&grouped, "Float group");
    native_round_trip(&grouped, group);
}

// ------------------------------------------------------ (d) malformed --

#[test]
fn a_crafted_group_open_table_refuses_the_open() {
    // Two closed groups, so the ordering claim can be broken.
    let closed = close(&close(&two_groups(), 3), 5);
    let good = closed.encode_native(LIMIT, 0, None).expect("bytes");
    let at = good
        .windows(4)
        .position(|w| w == b"LOPN")
        .expect("an LOPN table");
    assert_eq!(u32_at(&good, at + 8), 2);
    assert_eq!((u32_at(&good, at + 12), u32_at(&good, at + 16)), (3, 5));
    let reopened = PrecisionDocument::parse_native(&good, LIMIT, 0, None).expect("reopen");
    assert!(!open_of(&reopened, 3) && !open_of(&reopened, 5));

    let refuse = |bytes: &[u8], what: &str| -> String {
        match PrecisionDocument::parse_native(bytes, LIMIT, 0, None) {
            Err(message) => message,
            Ok(_) => panic!("{what} must refuse the open"),
        }
    };
    let patched = |offset: usize, value: &[u8]| {
        let mut bytes = good.clone();
        bytes[at + offset..at + offset + value.len()].copy_from_slice(value);
        bytes
    };

    assert_eq!(
        refuse(&patched(0, b"XXXX"), "a bad tag"),
        "unsupported native group open schema"
    );
    assert_eq!(
        refuse(&patched(4, &2u32.to_le_bytes()), "a bad schema"),
        "unsupported native group open schema"
    );
    assert_eq!(
        refuse(&patched(8, &0u32.to_le_bytes()), "an empty table"),
        "invalid native group open table"
    );
    assert_eq!(
        refuse(&patched(12, &5u32.to_le_bytes()), "a repeated index"),
        "invalid native group open table"
    );
    let mut swapped = good.clone();
    swapped[at + 12..at + 16].copy_from_slice(&5u32.to_le_bytes());
    swapped[at + 16..at + 20].copy_from_slice(&3u32.to_le_bytes());
    assert_eq!(
        refuse(&swapped, "an unordered table"),
        "invalid native group open table"
    );
    assert_eq!(
        refuse(
            &patched(16, &7u32.to_le_bytes()),
            "an index past the records"
        ),
        "invalid native group open table"
    );
    assert_eq!(
        refuse(&patched(16, &4u32.to_le_bytes()), "a raster index"),
        "invalid native group open index"
    );
}

// --------------------------------------------------- (e) conversion --

fn byte_group_document(open: bool) -> DrDocument {
    let mut doc =
        DrDocument::from_pixels(RgbaImage::from_pixel(3, 2, image::Rgba([9, 80, 200, 255])));
    doc.layers[0].name = "Background".into();
    let mut child =
        DrDocument::from_pixels(RgbaImage::from_pixel(3, 2, image::Rgba([200, 10, 30, 180])))
            .layers
            .remove(0);
    child.name = "Child".into();
    child.depth = 1;
    let mut group = DrDocument::from_pixels(RgbaImage::new(1, 1))
        .layers
        .remove(0);
    group.name = "Folder".into();
    group.kind = LayerKind::Group;
    group.open = open;
    doc.layers.extend([child, group]);
    doc
}

#[test]
fn conversion_carries_the_disclosure_in_both_directions() {
    let plan = byte_group_document(false)
        .promote_structure_plan()
        .expect("plan")
        .json();
    let preserved = plan["preserved"].as_array().expect("preserved");
    assert!(preserved.contains(&json!("group_open_state")));
    assert!(!plan["discarded"]
        .as_array()
        .expect("discarded")
        .contains(&json!("group_open_state")));

    let typed = byte_group_document(false)
        .promote_structure(LIMIT, 0, None)
        .expect("promote");
    assert!(!open_of(&typed, 2), "a closed byte group lands closed");
    assert!(open_of(
        &byte_group_document(true)
            .promote_structure(LIMIT, 0, None)
            .expect("promote"),
        2
    ));

    let narrowed = typed
        .narrow_structure_to_u8(PrecisionDither::None, LIMIT, 0, None)
        .expect("narrow");
    assert_eq!(narrowed.layers[2].kind, LayerKind::Group);
    assert!(
        !narrowed.layers[2].open,
        "a closed typed group lands closed"
    );
    let reopened = must(
        &typed,
        json!({"op": "layer_open", "layer": 2, "open": true}),
    );
    assert!(
        reopened
            .narrow_structure_to_u8(PrecisionDither::None, LIMIT, 0, None)
            .expect("narrow")
            .layers[2]
            .open
    );
}

// ------------------------------------------------------------ (f) PSD --

fn cpath(path: &Path) -> CString {
    CString::new(path.to_str().expect("utf-8 path")).expect("no NUL")
}

#[test]
fn a_sixteen_bit_psd_writes_and_reads_a_closed_folder() {
    let directory = TempDir::new().expect("tempdir");
    let path = directory.path().join("closed16.psd");
    let closed = close(&nested(), 3);
    let projection = closed.render(LIMIT, 0, None).expect("projection");
    let mut error = ptr::null_mut();
    let saved = unsafe {
        dr_precision_doc_save_psd(
            &*closed,
            &projection,
            cpath(&path).as_ptr(),
            0,
            0,
            LIMIT,
            0,
            std::ptr::null(),
            &mut error,
        )
    };
    assert!(saved, "{:?}", take_error(error));

    let file = fixture::read(&std::fs::read(&path).expect("read back"));
    let folder = file
        .records
        .iter()
        .find(|record| record.name == "G")
        .expect("the folder record");
    assert_eq!(folder.divider, Some(2), "a closed folder writes divider 2");

    let mut error = ptr::null_mut();
    let report = unsafe { dr_psd_import_report(cpath(&path).as_ptr(), &mut error) };
    let report: Value = serde_json::from_str(&take_error(report).expect("a report"))
        .unwrap_or_else(|_| panic!("{:?}", take_error(error)));
    let fingerprint = CString::new(report["source_fingerprint"].as_str().unwrap()).unwrap();
    let mut error = ptr::null_mut();
    let opened = unsafe {
        dr_precision_doc_open_psd(
            cpath(&path).as_ptr(),
            0,
            fingerprint.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut error,
        )
    };
    assert!(!opened.is_null(), "{:?}", take_error(error));
    let opened = unsafe { Box::from_raw(opened) };
    let g = index_named(&opened, "G");
    assert!(!open_of(&opened, g), "the reimported folder is closed");
}
