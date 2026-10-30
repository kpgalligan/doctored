//! Phase 12E-1: PERSISTED layer identity in `.rz` at both depths.
//!
//! The oracle is the FORMAT, written out here rather than read off the
//! implementation: **DRDC 45 (byte) / 46 (typed) is the OUTERMOST version
//! word, it is ALWAYS written, and one `LIDS` table — the tag, u32 schema 1,
//! u32 count, then count u64 little-endian ids in index order — closes the
//! file after every prior tail.** This file assembles that table itself.
//!
//! What is pinned: a round trip keeps every id at both depths and
//! reopen-then-resave is byte-identical; a file with no 45/46 word (built
//! here by stripping the outermost word and the `LIDS` tail off a fresh save)
//! still reads and MINTS; a newer word refuses by name; a zero id, a
//! duplicate, a short count, a long count and a trailing word all refuse; a
//! document carrying EVERY other conditional tail at once still round-trips
//! and re-saves byte-identical, which is what catches an intermediate
//! trailing-bytes check that forgot to make room for `LIDS`; a layer created
//! after the read mints ABOVE every id the file carried; the structural ops
//! and `document_clone` still carry ids; and the four new capability families
//! answer at both depths.

use std::ffi::{c_char, CStr, CString};
use std::path::Path;
use std::ptr;

use doctored_core::doc::DrDocument;
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_comps::dr_doc_edit_layer_comps;
use doctored_core::ffi_doc::*;
use doctored_core::ffi_evaluation::dr_doc_set_evaluation;
use doctored_core::ffi_group::*;
use doctored_core::ffi_identity::dr_doc_layer_id;
use doctored_core::ffi_precision_conversion::dr_precision_capability;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer as Layer};
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use doctored_core::precision_mask::{PrecisionMask, PrecisionPlane};
use image::{Rgba, RgbaImage};
use serde_json::Value;
use tempfile::TempDir;

use crate::common;
use common::{add_layer, cpath, doc_from, opaque_pattern, take_err_string, test_pattern};

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
const BYTE_VERSION: u32 = 45;
const PRECISION_VERSION: u32 = 46;
const LINEAR: std::ffi::c_int = 1;

// ------------------------------------------------- independent references --

/// The `LIDS` table this build must write, assembled here from the ids alone:
/// the tag, u32 schema 1, u32 count, then one u64 per id in index order.
/// Nothing in this function reads `identity_native`.
fn expected_table(ids: &[u64]) -> Vec<u8> {
    let mut out = b"LIDS".to_vec();
    out.extend(1u32.to_le_bytes());
    out.extend((ids.len() as u32).to_le_bytes());
    for id in ids {
        out.extend(id.to_le_bytes());
    }
    out
}

fn word(bytes: &[u8], index: usize) -> u32 {
    let at = 4 + index * 4;
    u32::from_le_bytes(bytes[at..at + 4].try_into().expect("4 bytes"))
}

// ----------------------------------------------------------- byte plumbing --

fn ids(doc: *const DrDocument) -> Vec<u64> {
    (0..unsafe { dr_doc_layer_count(doc) })
        .map(|i| unsafe { dr_doc_layer_id(doc, i) })
        .collect()
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

fn open_err(dir: &TempDir, name: &str, bytes: &[u8], what: &str) -> String {
    let path = dir.path().join(name);
    std::fs::write(&path, bytes).unwrap();
    match open(&path) {
        Err(message) => message,
        Ok(doc) => {
            unsafe { dr_doc_free(doc) };
            panic!("{what} must refuse the open")
        }
    }
}

fn byte_fixture(dir: &TempDir) -> *mut DrDocument {
    let doc = doc_from(dir, "base.png", &opaque_pattern(4, 3));
    let doc = add_layer(dir, "mid.png", doc, 0, &test_pattern(4, 3), "Mid");
    add_layer(dir, "top.png", doc, 1, &test_pattern(4, 3), "Top")
}

// ---------------------------------------------------------- typed plumbing --

fn typed_bytes(doc: &PrecisionDocument) -> Vec<u8> {
    doc.encode_native(LIMIT, 0, None).unwrap()
}

fn typed_ids(doc: &PrecisionDocument) -> Vec<u64> {
    (0..doc.layer_count())
        .map(|i| doc.layer_id(i).unwrap())
        .collect()
}

fn image(pixels: Vec<[u16; 4]>) -> PrecisionImage {
    PrecisionImage::from_rgba16(pixels.len() as u32, 1, pixels, IccProfile::srgb(), None).unwrap()
}

fn typed_layer(name: &str, offset: i32) -> Layer {
    let px: Vec<[u16; 4]> = (0..4u16)
        .map(|i| [1001 + i, 2003 + i, 3007 + i, 65535])
        .collect();
    Layer::raster(name, image(px), (offset, 0), 1.0, true).unwrap()
}

fn typed_document(layers: Vec<Layer>) -> PrecisionDocument {
    PrecisionDocument::from_parts(
        4,
        1,
        IccProfile::srgb(),
        Metadata::default(),
        Resolution::default(),
        layers,
    )
    .unwrap()
}

fn mask(values: Vec<u16>) -> PrecisionMask {
    PrecisionMask::new(
        PrecisionPlane::from_u16(values.len() as u32, 1, values, None).unwrap(),
        Default::default(),
        true,
    )
    .unwrap()
}

fn typed_edit(doc: &PrecisionDocument, request: Value) -> PrecisionDocument {
    use doctored_core::ffi_precision_edit::dr_precision_doc_edit;
    let text = CString::new(request.to_string()).unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_edit(
            doc,
            text.as_ptr(),
            ptr::null(),
            0,
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!out.is_null(), "{}", take_err_string(err));
    *unsafe { Box::from_raw(out) }
}

// ------------------------------------------------------ (a) the round trip --

#[test]
fn a_byte_round_trip_keeps_every_id_and_resaves_byte_identical() {
    let dir = TempDir::new().unwrap();
    let doc = byte_fixture(&dir);
    let before = ids(doc);
    assert_eq!(before.len(), 3);

    let path = dir.path().join("ids.rz");
    let bytes = save(doc, &path);
    assert_eq!(&bytes[..4], b"DRDC");
    assert_eq!(word(&bytes, 0), BYTE_VERSION, "45 is the outermost word");
    let table = expected_table(&before);
    assert_eq!(
        &bytes[bytes.len() - table.len()..],
        &table[..],
        "the table this file assembled closes the file"
    );

    let reopened = open_ok(&path);
    assert_eq!(ids(reopened), before, "a reopen adopts the file's ids");
    assert_eq!(
        save(reopened, &dir.path().join("twice.rz")),
        bytes,
        "reopen then resave is byte-identical"
    );
    unsafe { dr_doc_free(reopened) };
    unsafe { dr_doc_free(doc) };
}

#[test]
fn a_typed_round_trip_keeps_every_id_and_resaves_byte_identical() {
    let doc = typed_document(vec![
        typed_layer("B", 0),
        Layer::pass_through(
            "G",
            vec![typed_layer("Inner", 1), typed_layer("Member", -1)],
            1.0,
            true,
        )
        .unwrap(),
        typed_layer("Top", 0),
    ]);
    let before = typed_ids(&doc);
    assert_eq!(
        before.len(),
        5,
        "postorder: two children, the group, two roots"
    );

    let data = typed_bytes(&doc);
    assert_eq!(
        word(&data, 0),
        PRECISION_VERSION,
        "46 is the outermost word"
    );
    let table = expected_table(&before);
    assert_eq!(&data[data.len() - table.len()..], &table[..]);

    let reopened = PrecisionDocument::parse_native(&data, LIMIT, 0, None).unwrap();
    assert_eq!(
        typed_ids(&reopened),
        before,
        "a reopen adopts the file's ids"
    );
    assert_eq!(typed_bytes(&reopened), data, "resave is byte-identical");
}

// ------------------------------------ (b) a file WITHOUT the word still reads --

/// Strips the outermost version word and the `LIDS` tail off a fresh save, so
/// what is left is byte-for-byte what the previous writer produced.
fn without_ids(bytes: &[u8], ids: usize) -> Vec<u8> {
    let tail = 12 + 8 * ids;
    let mut out = bytes[..4].to_vec();
    out.extend_from_slice(&bytes[8..bytes.len() - tail]);
    out
}

#[test]
fn a_byte_file_with_no_identity_word_reads_as_before_and_mints() {
    let dir = TempDir::new().unwrap();
    let doc = byte_fixture(&dir);
    let before = ids(doc);
    let bytes = save(doc, &dir.path().join("full.rz"));
    let older = without_ids(&bytes, before.len());
    assert_eq!(
        word(&older, 0),
        10,
        "what is left is the ordinary body word"
    );

    let path = dir.path().join("older.rz");
    std::fs::write(&path, &older).unwrap();
    let reopened = open_ok(&path);
    let after = ids(reopened);
    assert_eq!(after.len(), before.len());
    for id in &after {
        assert!(
            !before.contains(id),
            "a file without the table mints ({after:?} vs {before:?})"
        );
    }
    unsafe { dr_doc_free(reopened) };
    unsafe { dr_doc_free(doc) };
}

#[test]
fn a_typed_file_with_no_identity_word_reads_as_before_and_mints() {
    let doc = typed_document(vec![typed_layer("B", 0), typed_layer("Top", 0)]);
    let before = typed_ids(&doc);
    let data = typed_bytes(&doc);
    let older = without_ids(&data, before.len());
    let reopened = PrecisionDocument::parse_native(&older, LIMIT, 0, None).unwrap();
    let after = typed_ids(&reopened);
    assert_eq!(after.len(), before.len());
    for id in &after {
        assert!(!before.contains(id), "{after:?} vs {before:?}");
    }
}

// ------------------------------------------------------------ (c) refusals --

#[test]
fn a_newer_outermost_word_refuses_by_name() {
    let dir = TempDir::new().unwrap();
    let doc = byte_fixture(&dir);
    let mut bytes = save(doc, &dir.path().join("future.rz"));
    bytes[4..8].copy_from_slice(&47u32.to_le_bytes());
    let message = open_err(&dir, "future-read.rz", &bytes, "a version-47 file");
    assert!(
        message.contains("unsupported DRDC version 47"),
        "unexpected refusal: {message}"
    );
    unsafe { dr_doc_free(doc) };
}

#[test]
fn a_crafted_identity_table_refuses_before_any_pixel_decodes() {
    let dir = TempDir::new().unwrap();
    let doc = byte_fixture(&dir);
    let live = ids(doc);
    let bytes = save(doc, &dir.path().join("good.rz"));
    let body = &bytes[..bytes.len() - expected_table(&live).len()];

    let rebuilt = |ids: &[u64]| {
        let mut out = body.to_vec();
        out.extend(expected_table(ids));
        out
    };
    // A zero id: 0 is "no layer" everywhere this crate carries an id.
    let message = open_err(
        &dir,
        "zero.rz",
        &rebuilt(&[live[0], 0, live[2]]),
        "a zero id",
    );
    assert!(
        message.contains("invalid native layer identity value"),
        "{message}"
    );
    // A duplicate: two records would answer one `layer_index_by_id`.
    let message = open_err(
        &dir,
        "dup.rz",
        &rebuilt(&[live[0], live[0], live[2]]),
        "a duplicate id",
    );
    assert!(
        message.contains("invalid native layer identity value"),
        "{message}"
    );
    // Short and long counts are both a claim about the record count.
    let message = open_err(&dir, "short.rz", &rebuilt(&live[..2]), "a short table");
    assert!(
        message.contains("invalid native layer identity count"),
        "{message}"
    );
    let mut long = live.clone();
    long.push(live[2] + 7);
    let message = open_err(&dir, "long.rz", &rebuilt(&long), "a long table");
    assert!(
        message.contains("invalid native layer identity count"),
        "{message}"
    );
    // A trailing word after a well-formed table.
    let mut trailing = rebuilt(&live);
    trailing.extend(1u32.to_le_bytes());
    let message = open_err(&dir, "trailing.rz", &trailing, "a trailing word");
    assert!(
        message.contains("native layer ids have trailing bytes"),
        "{message}"
    );
    // A bad tag.
    let mut tag = rebuilt(&live);
    let at = tag.len() - expected_table(&live).len();
    tag[at..at + 4].copy_from_slice(b"XIDS");
    let message = open_err(&dir, "tag.rz", &tag, "a bad tag");
    assert!(
        message.contains("unsupported native layer identity schema"),
        "{message}"
    );
    unsafe { dr_doc_free(doc) };
}

#[test]
fn a_crafted_typed_identity_table_refuses_too() {
    let doc = typed_document(vec![typed_layer("B", 0), typed_layer("Top", 0)]);
    let live = typed_ids(&doc);
    let data = typed_bytes(&doc);
    let body = &data[..data.len() - expected_table(&live).len()];
    let rebuilt = |ids: &[u64]| {
        let mut out = body.to_vec();
        out.extend(expected_table(ids));
        out
    };
    for (ids, what) in [
        (vec![0, live[1]], "a zero id"),
        (vec![live[0], live[0]], "a duplicate id"),
        (vec![live[0]], "a short table"),
    ] {
        let message = match PrecisionDocument::parse_native(&rebuilt(&ids), LIMIT, 0, None) {
            Err(message) => message,
            Ok(_) => panic!("{what} must refuse the open"),
        };
        assert!(
            message.contains("invalid native layer identity"),
            "{what}: {message}"
        );
    }
}

// --------------------------------- (d) every conditional tail at once --

#[test]
fn a_byte_document_carrying_every_other_tail_still_round_trips_exactly() {
    // An outside-coverage mask, linear-light evaluation, a colour label AND a
    // layer comp, so `MOUT`, `EVAL`, `LLBL` and `LCMP` all precede `LIDS`.
    // This is the fixture an intermediate `remaining() != 0` check that
    // forgot the identity table fails on.
    let dir = TempDir::new().unwrap();
    let base = DrDocument::from_pixels(RgbaImage::from_fn(4, 2, |x, y| {
        Rgba([(x * 60) as u8, (y * 90) as u8, 120, 255])
    }));
    let mut model = base
        .adding_image_layer(
            0,
            RgbaImage::from_fn(4, 2, |x, _| Rgba([200, (x * 40) as u8, 20, 180])),
            "Top",
        )
        .expect("a second layer")
        .add_mask(1, doctored_core::doc::MaskKind::FromSelection(&[255u8; 8]))
        .expect("a mask");
    model.layers[1].mask_properties.outside_coverage = 255;
    let handle = Box::into_raw(Box::new(model));

    let mut err: *mut c_char = ptr::null_mut();
    let doc = unsafe { dr_doc_set_evaluation(handle, LINEAR, &mut err) };
    assert!(!doc.is_null(), "{}", take_err_string(err));
    unsafe { dr_doc_free(handle) };
    let labelled = unsafe { dr_doc_with_layer_label(doc, 1, 5) };
    assert!(!labelled.is_null(), "the label");
    unsafe { dr_doc_free(doc) };
    let request =
        CString::new(r#"{"op":"layer_comp","action":"save","name":"Everything"}"#).unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    let doc = unsafe { dr_doc_edit_layer_comps(labelled, request.as_ptr(), &mut err) };
    assert!(!doc.is_null(), "{}", take_err_string(err));
    unsafe { dr_doc_free(labelled) };

    let live = ids(doc);
    let path = dir.path().join("everything.rz");
    let bytes = save(doc, &path);
    assert_eq!(word(&bytes, 0), BYTE_VERSION, "45 is outermost");
    assert_eq!(word(&bytes, 1), 43, "the comp word is inside it");
    assert_eq!(word(&bytes, 2), 41, "then the label word");
    assert_eq!(word(&bytes, 3), 38, "then the evaluation word");
    assert_eq!(word(&bytes, 4), 28, "then the outside-mask word");
    // MOUT, EVAL, LLBL, LCMP, LIDS, in that order and nothing after.
    let positions: Vec<usize> = [&b"MOUT"[..], b"EVAL", b"LLBL", b"LCMP", b"LIDS"]
        .iter()
        .map(|tag| {
            bytes
                .windows(4)
                .rposition(|w| w == *tag)
                .unwrap_or_else(|| panic!("{:?} is present", std::str::from_utf8(tag)))
        })
        .collect();
    assert!(positions.windows(2).all(|w| w[0] < w[1]), "{positions:?}");
    let table = expected_table(&live);
    assert_eq!(&bytes[bytes.len() - table.len()..], &table[..]);

    let reopened = open_ok(&path);
    assert_eq!(ids(reopened), live);
    assert_eq!(save(reopened, &dir.path().join("again.rz")), bytes);
    unsafe { dr_doc_free(reopened) };
    unsafe { dr_doc_free(doc) };
}

#[test]
fn a_typed_document_carrying_every_other_tail_still_round_trips_exactly() {
    // A style, Blend If, a clipping member, an outside-coverage mask,
    // linear-light evaluation, a label and a comp: LSTY, LBIF, LCLP, MOUT,
    // EVAL, LLBL and LCMP all precede LIDS. This is the fixture a typed
    // count/trailing check placed before the identity table fails on.
    let doc = typed_document(vec![
        typed_layer("B", 0),
        typed_layer("Mid", 1),
        typed_layer("Top", 0).with_mask(Some(mask(vec![0, 123, 32769, 65535]))),
    ]);
    // The outside-coverage flag is stored mask state, set through the same
    // public setter `precision_clipping`'s own fixture uses.
    let target = doc.layer(2).unwrap().mask().unwrap();
    let mut properties = target.properties().clone();
    properties.outside_coverage = 255;
    let doc = doc
        .set_layer_mask(
            2,
            Some(
                target
                    .with_properties(properties, target.enabled())
                    .unwrap(),
            ),
            LIMIT,
            0,
            None,
        )
        .unwrap()
        .unwrap();
    let doc = typed_edit(
        &doc,
        serde_json::json!({"op":"layer_style","version":1,"layer":0,
                           "style":{"fill_opacity":0.5,"effects":[{"type":"drop_shadow"}]}}),
    );
    let doc = typed_edit(
        &doc,
        serde_json::json!({"op":"layer_blend_if","version":1,"layer":1,
                           "blend_if":{"channel":"red","this_layer":[10,20,240,255],
                                       "underlying":[0,0,200,220]}}),
    );
    let doc = typed_edit(
        &doc,
        serde_json::json!({"op":"layer_clipped","layer":1,"clipped":true}),
    );
    // NOT linear light: 11E's refusal walk declines a layer STYLE under that
    // policy, so a style and `EVAL` cannot co-exist on one typed document.
    // The evaluation tail is covered on its own below.
    let doc = typed_edit(
        &doc,
        serde_json::json!({"op":"layer_label","layer":1,"label":5}),
    );
    let doc = typed_edit(
        &doc,
        serde_json::json!({"op":"layer_comp","action":"save","name":"Everything"}),
    );

    let live = typed_ids(&doc);
    let data = typed_bytes(&doc);
    assert_eq!(word(&data, 0), PRECISION_VERSION, "46 is outermost");
    // Every conditional tail, then LIDS, and nothing after it.
    let positions: Vec<usize> = [
        &b"MOUT"[..],
        b"LCLP",
        b"LBIF",
        b"LSTY",
        b"LLBL",
        b"LCMP",
        b"LIDS",
    ]
    .iter()
    .map(|tag| {
        data.windows(4)
            .rposition(|w| w == *tag)
            .unwrap_or_else(|| panic!("{:?} is present", std::str::from_utf8(tag)))
    })
    .collect();
    assert!(positions.windows(2).all(|w| w[0] < w[1]), "{positions:?}");
    let table = expected_table(&live);
    assert_eq!(&data[data.len() - table.len()..], &table[..]);

    let reopened = PrecisionDocument::parse_native(&data, LIMIT, 0, None).unwrap();
    assert_eq!(typed_ids(&reopened), live);
    assert_eq!(typed_bytes(&reopened), data);
}

#[test]
fn a_linear_light_typed_document_still_closes_with_the_identity_table() {
    // The evaluation tail's own case, separate because 11E refuses a layer
    // style under `linear_light`: EVAL must sit before LIDS, and the round
    // trip must still be byte-identical.
    let doc = typed_document(vec![typed_layer("B", 0), typed_layer("Top", 0)])
        .with_evaluation(doctored_core::doc_evaluation::Evaluation::LinearLight)
        .unwrap()
        .unwrap();
    let live = typed_ids(&doc);
    let data = typed_bytes(&doc);
    assert_eq!(word(&data, 0), PRECISION_VERSION);
    let eval = data.windows(4).rposition(|w| w == b"EVAL").expect("EVAL");
    let lids = data.windows(4).rposition(|w| w == b"LIDS").expect("LIDS");
    assert!(eval < lids, "LIDS closes the file: {eval} {lids}");
    let reopened = PrecisionDocument::parse_native(&data, LIMIT, 0, None).unwrap();
    assert_eq!(typed_ids(&reopened), live);
    assert_eq!(typed_bytes(&reopened), data);
}

// --------------------------------------------- (e) minting above the read --

#[test]
fn a_layer_created_after_a_read_mints_above_every_id_the_file_carried() {
    let dir = TempDir::new().unwrap();
    let doc = byte_fixture(&dir);
    let live = ids(doc);
    let highest = *live.iter().max().unwrap();
    // A crafted file whose ids sit far above anything this process has
    // minted: the counter must be raised past them on read.
    let bytes = save(doc, &dir.path().join("high.rz"));
    let body = &bytes[..bytes.len() - expected_table(&live).len()];
    let high: Vec<u64> = (0..live.len() as u64)
        .map(|i| highest + 1_000_000 + i)
        .collect();
    let mut crafted = body.to_vec();
    crafted.extend(expected_table(&high));
    let path = dir.path().join("high-read.rz");
    std::fs::write(&path, &crafted).unwrap();

    let reopened = open_ok(&path);
    assert_eq!(ids(reopened), high);
    let grown = add_layer(&dir, "extra.png", reopened, 0, &test_pattern(4, 3), "Extra");
    let after = ids(grown);
    let minted: Vec<u64> = after
        .iter()
        .copied()
        .filter(|id| !high.contains(id))
        .collect();
    assert_eq!(minted.len(), 1, "one new entry");
    assert!(
        minted[0] > *high.iter().max().unwrap(),
        "a fresh mint is above every read id ({minted:?} vs {high:?})"
    );
    unsafe { dr_doc_free(grown) };
    unsafe { dr_doc_free(doc) };

    // The typed reader raises the same counter.
    let typed = typed_document(vec![typed_layer("B", 0)]);
    let data = typed_bytes(&typed);
    let live = typed_ids(&typed);
    let body = &data[..data.len() - expected_table(&live).len()];
    let high = vec![live[0] + 2_000_000];
    let mut crafted = body.to_vec();
    crafted.extend(expected_table(&high));
    let read = PrecisionDocument::parse_native(&crafted, LIMIT, 0, None).unwrap();
    assert_eq!(typed_ids(&read), high);
    let grown = read.duplicate_layer(0).unwrap().unwrap();
    let minted: Vec<u64> = typed_ids(&grown)
        .into_iter()
        .filter(|id| !high.contains(id))
        .collect();
    assert_eq!(minted.len(), 1);
    assert!(minted[0] > high[0], "{minted:?} vs {high:?}");
}

// ----------------------------------------------- (f) carriage still holds --

#[test]
fn a_clone_and_the_structural_ops_still_carry_ids_through_a_round_trip() {
    let dir = TempDir::new().unwrap();
    let doc = byte_fixture(&dir);
    let path = dir.path().join("carry.rz");
    save(doc, &path);
    let reopened = open_ok(&path);
    let live = ids(reopened);

    let cloned = unsafe { dr_doc_clone(reopened) };
    assert!(!cloned.is_null(), "the deep copy");
    assert_eq!(ids(cloned), live, "a deep copy carries every id");
    unsafe { dr_doc_free(cloned) };

    // A reorder carries; grouping mints for the container alone.
    let moved = unsafe { dr_doc_move_layer_to(reopened, 2, 0, 0) };
    assert!(!moved.is_null(), "the move");
    let mut expected = live.clone();
    let top = expected.remove(2);
    expected.insert(0, top);
    assert_eq!(ids(moved), expected, "a reorder carries ids");
    unsafe { dr_doc_free(moved) };

    let removed = unsafe { dr_doc_removing_layer(reopened, 1) };
    assert!(!removed.is_null(), "the delete");
    assert_eq!(ids(removed), vec![live[0], live[2]], "a delete carries ids");
    unsafe { dr_doc_free(removed) };
    unsafe { dr_doc_free(reopened) };
    unsafe { dr_doc_free(doc) };
}

// ------------------------------------------- (g) the new capability rows --

#[test]
fn the_four_structure_families_answer_at_both_typed_depths() {
    fn capability(operation: &str, target: &str, depth: u32) -> Value {
        let op = CString::new(operation).unwrap();
        let target = CString::new(target).unwrap();
        let mut err: *mut c_char = ptr::null_mut();
        let out = unsafe { dr_precision_capability(op.as_ptr(), target.as_ptr(), depth, &mut err) };
        assert!(!out.is_null());
        let text = unsafe { CStr::from_ptr(out) }
            .to_string_lossy()
            .into_owned();
        unsafe { dr_string_free(out) };
        serde_json::from_str(&text).unwrap()
    }
    for depth in [16u32, 32] {
        for (operation, targets) in [
            ("layer_move", &["raster", "group", "adjustment"][..]),
            ("layer_group", &["raster", "group", "adjustment"][..]),
            ("layer_ungroup", &["group"][..]),
            ("layer_remove", &["raster", "group", "adjustment"][..]),
        ] {
            for target in targets {
                let report = capability(operation, target, depth);
                assert_eq!(
                    report["supported"],
                    Value::Bool(true),
                    "{operation}/{target} at {depth}: {report}"
                );
            }
        }
    }
    // The gate must derive a RECORD target, never "document": the core
    // refuses that pairing by name, which is what makes a Swift gate that
    // forgets the index-mapped list fail loudly rather than silently.
    let report = capability("layer_move", "document", 16);
    assert_eq!(report["supported"], Value::Bool(false));
    assert_eq!(
        report["reason"].as_str().unwrap(),
        "high-depth layer_move does not support target document"
    );
    // And ungrouping admits GROUP records only.
    let report = capability("layer_ungroup", "raster", 16);
    assert_eq!(report["supported"], Value::Bool(false));
    assert_eq!(
        report["reason"].as_str().unwrap(),
        "high-depth layer_ungroup does not support target raster"
    );
}
