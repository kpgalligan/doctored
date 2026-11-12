//! The Background mark (`doc_background`, `psd_background`, `background_native`)
//! through the FFI, at 8 and 16 bits.
//!
//! - The committed Photoshop take `knockout_bottom/deep_over_ordinary_bottom`
//!   (a Deep knockout over an ORDINARY bottom layer, rendered by Photoshop
//!   26.10.0 and matted on white) is matched by a layered import of the same
//!   family's `source.psd` / `source16.psd`. Bound, declared before measuring:
//!   max 2, mean 0.5 codes (16 bits divided by 257), P7-39's.
//! - Import marks record 0: Background for `knockout/source.psd`'s
//!   Photoshop Background record, ordinary for `knockout_bottom`'s.
//! - `.doctored` keeps the mark at both depths under word 113, writes the word
//!   only for a marked document, and refuses a bad `LBKG` role.
//! - Export writes record 0 in the authored Background form (no channel `-1`,
//!   flag bit 0, `lnsr bgnd`, `lspf 0x0d`), read back by this file's own
//!   record walker, for a marked Background and for an unmarked qualifying
//!   bottom under a Deep knockout; an ordinary bottom keeps its `-1`.
//! - Depth conversions and Duplicate Document keep the mark; a merge into
//!   the bottom keeps it and Flatten drops it.

use std::ffi::{c_char, CStr, CString};
use std::path::Path;
use std::ptr;

use doctored_core::doc::DrDocument;
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_background::{dr_doc_background_role, dr_precision_doc_background_role};
use doctored_core::ffi_doc::*;
use doctored_core::ffi_group::dr_doc_duplicate_to_new_document;
use doctored_core::ffi_precision_conversion::{
    dr_doc_promote_structure, dr_precision_doc_narrow_structure_to_u8,
};
use doctored_core::ffi_precision_edit::dr_precision_doc_edit;
use doctored_core::ffi_psd::{
    dr_doc_psd_export_report, dr_doc_save_psd, dr_precision_doc_open_psd, dr_precision_doc_save_psd,
};
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::REFERENCE_WORKING_BYTES as LIMIT;
use serde_json::{json, Value};
use tempfile::TempDir;

use crate::common;
use common::psd_photoshop::{fixture as family_file, import_report, open_psd, strings, LAYERS};
use common::{add_layer, cpath, doc_from, set_style, solid, take_err_string};

const ROLE_NONE: i32 = 0;
const ROLE_BACKGROUND: i32 = 1;
const ROLE_ORDINARY: i32 = 2;

// ------------------------------------------------------------ plumbing --

fn byte_open(path: &Path) -> *mut DrDocument {
    Box::into_raw(open_psd(path, &import_report(path), LAYERS))
}

fn typed_open(path: &Path) -> Box<PrecisionDocument> {
    let report = import_report(path);
    let token = CString::new(report["source_fingerprint"].as_str().unwrap()).unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    let doc = unsafe {
        dr_precision_doc_open_psd(
            cpath(path).as_ptr(),
            LAYERS,
            token.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!doc.is_null(), "typed open: {}", take_err_string(err));
    unsafe { Box::from_raw(doc) }
}

fn typed_edit(document: &PrecisionDocument, request: Value) -> Box<PrecisionDocument> {
    let text = CString::new(request.to_string()).unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_edit(
            document,
            text.as_ptr(),
            ptr::null(),
            0,
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!out.is_null(), "{request}: {}", take_err_string(err));
    unsafe { Box::from_raw(out) }
}

fn byte_index(doc: *const DrDocument, name: &str) -> usize {
    let count = unsafe { &*doc }.layers.len();
    (0..count)
        .find(|&i| common::layer_name(doc, i) == name)
        .unwrap_or_else(|| panic!("{name} is present"))
}

fn typed_index(document: &PrecisionDocument, name: &str) -> usize {
    (0..document.layer_count())
        .find(|&i| document.layer(i).is_some_and(|l| l.name() == name))
        .unwrap_or_else(|| panic!("{name} is present"))
}

fn byte_hidden(doc: *mut DrDocument, name: &str) -> *mut DrDocument {
    let index = byte_index(doc, name);
    let out = unsafe { dr_doc_with_layer_visible(doc, index, false) };
    assert!(!out.is_null(), "hide {name}");
    unsafe { dr_doc_free(doc) };
    out
}

fn role(doc: *const DrDocument) -> i32 {
    unsafe { dr_doc_background_role(doc) }
}

fn typed_role(doc: &PrecisionDocument) -> i32 {
    unsafe { dr_precision_doc_background_role(doc) }
}

/// A straight-alpha pixel over white, in the pixel's own scale.
fn over_white(c: f64, a: f64, max: f64) -> f64 {
    c * a / max + max * (1.0 - a / max)
}

/// (max, mean) absolute difference of two equally long sample lists.
fn residual(got: &[f64], want: &[f64]) -> (f64, f64) {
    assert_eq!(got.len(), want.len());
    let diffs: Vec<f64> = got.iter().zip(want).map(|(g, w)| (g - w).abs()).collect();
    let max = diffs.iter().copied().fold(0.0, f64::max);
    (max, diffs.iter().sum::<f64>() / diffs.len() as f64)
}

fn byte_matted(doc: *const DrDocument) -> Vec<f64> {
    common::flat_pixels(doc)
        .chunks(4)
        .flat_map(|p| (0..3).map(move |k| over_white(f64::from(p[k]), f64::from(p[3]), 255.0)))
        .collect()
}

fn typed_matted(doc: &PrecisionDocument) -> Vec<f64> {
    doc.render(LIMIT, 0, None)
        .expect("the typed projection")
        .to_rgba16()
        .expect("RGBA16")
        .iter()
        .flat_map(|p| {
            (0..3).map(move |k| over_white(f64::from(p[k]), f64::from(p[3]), 65535.0) / 257.0)
        })
        .collect()
}

fn take8(name: &str) -> Vec<f64> {
    image::open(family_file("knockout_bottom", name))
        .unwrap()
        .to_rgb8()
        .pixels()
        .flat_map(|p| p.0.map(f64::from))
        .collect()
}

fn take16(name: &str) -> Vec<f64> {
    image::open(family_file("knockout_bottom", name))
        .unwrap()
        .to_rgb16()
        .pixels()
        .flat_map(|p| p.0.map(|c| f64::from(c) / 257.0))
        .collect()
}

fn write_psd(doc: *const DrDocument, path: &Path) -> Vec<u8> {
    let mut err: *mut c_char = ptr::null_mut();
    let ok =
        unsafe { dr_doc_save_psd(doc, ptr::null(), cpath(path).as_ptr(), 0, LAYERS, &mut err) };
    assert!(ok, "save_psd: {}", take_err_string(err));
    std::fs::read(path).unwrap()
}

fn typed_write_psd(doc: &PrecisionDocument, path: &Path) -> Vec<u8> {
    let projection = doc.render(LIMIT, 0, None).expect("the typed projection");
    let mut err: *mut c_char = ptr::null_mut();
    let ok = unsafe {
        dr_precision_doc_save_psd(
            doc,
            &projection,
            cpath(path).as_ptr(),
            0,
            LAYERS,
            LIMIT,
            0,
            &mut err,
        )
    };
    assert!(ok, "typed save_psd: {}", take_err_string(err));
    std::fs::read(path).unwrap()
}

fn export_losses(doc: *const DrDocument) -> Vec<String> {
    let mut err: *mut c_char = ptr::null_mut();
    let json = unsafe { dr_doc_psd_export_report(doc, 0, LAYERS, &mut err) };
    assert!(!json.is_null(), "export report: {}", take_err_string(err));
    let text = unsafe { CStr::from_ptr(json) }
        .to_string_lossy()
        .into_owned();
    unsafe { dr_string_free(json) };
    strings(&serde_json::from_str(&text).unwrap(), "losses")
}

// ------------------------------------------------- the record walker --

/// FILE record 0 of a classic PSD, read by this test's own walk of Adobe's
/// layout: its channel ids, its flags byte and its additional blocks.
struct FirstRecord {
    channels: Vec<i16>,
    flags: u8,
    blocks: Vec<([u8; 4], Vec<u8>)>,
}

fn be32(bytes: &[u8], at: usize) -> usize {
    u32::from_be_bytes(bytes[at..at + 4].try_into().unwrap()) as usize
}

fn first_record(bytes: &[u8]) -> FirstRecord {
    assert_eq!(&bytes[..4], b"8BPS");
    let mut at = 26;
    at += 4 + be32(bytes, at); // colour-mode data
    at += 4 + be32(bytes, at); // image resources
    at += 4; // layer and mask section length
    at += 4; // layer information length
    let count = i16::from_be_bytes(bytes[at..at + 2].try_into().unwrap());
    assert!(count != 0, "the file holds records");
    at += 2 + 16;
    let n = u16::from_be_bytes(bytes[at..at + 2].try_into().unwrap()) as usize;
    at += 2;
    let channels = (0..n)
        .map(|i| i16::from_be_bytes(bytes[at + 6 * i..at + 6 * i + 2].try_into().unwrap()))
        .collect();
    at += 6 * n;
    assert_eq!(&bytes[at..at + 4], b"8BIM");
    let flags = bytes[at + 10];
    let extra_len = be32(bytes, at + 12);
    let extra = &bytes[at + 16..at + 16 + extra_len];
    let mut cursor = 4 + be32(extra, 0); // mask data
    cursor += 4 + be32(extra, cursor); // blending ranges
    cursor += (usize::from(extra[cursor]) + 4) & !3; // Pascal name, padded to 4
    let mut blocks = Vec::new();
    while cursor + 12 <= extra.len() {
        assert_eq!(&extra[cursor..cursor + 4], b"8BIM");
        let key: [u8; 4] = extra[cursor + 4..cursor + 8].try_into().unwrap();
        let len = be32(extra, cursor + 8);
        blocks.push((key, extra[cursor + 12..cursor + 12 + len].to_vec()));
        cursor += 12 + len + len % 2;
    }
    FirstRecord {
        channels,
        flags,
        blocks,
    }
}

impl FirstRecord {
    fn block(&self, key: &[u8; 4]) -> Option<&[u8]> {
        self.blocks
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, body)| body.as_slice())
    }

    /// Photoshop's authored Background form (`knockout/source.psd` record 0).
    fn is_background(&self) -> bool {
        !self.channels.contains(&-1)
            && self.flags & 1 == 1
            && self.block(b"lnsr") == Some(b"bgnd".as_slice())
            && self.block(b"lspf") == Some([0u8, 0, 0, 0x0d].as_slice())
    }
}

// ------------------------------------------------------ the take match --

/// `knockout_bottom`: Bottom (ordinary, channel `-1`), Mid, DeepKnock (Deep
/// at fill 0), then ClipBase and ClipMember, which this take hides.
#[test]
fn deep_over_an_ordinary_bottom_matches_photoshop_at_both_depths() {
    let doc = byte_open(&family_file("knockout_bottom", "source.psd"));
    assert_eq!(role(doc), ROLE_ORDINARY, "record 0 is an ordinary layer");
    let doc = byte_hidden(byte_hidden(doc, "ClipBase"), "ClipMember");
    let (max, mean) = residual(&byte_matted(doc), &take8("deep_over_ordinary_bottom_8.png"));
    assert!(max <= 2.0 && mean <= 0.5, "8 bits: max {max}, mean {mean}");
    unsafe { dr_doc_free(doc) };

    let typed = typed_open(&family_file("knockout_bottom", "source16.psd"));
    assert_eq!(typed_role(&typed), ROLE_ORDINARY);
    let typed = typed_edit(
        &typed,
        json!({"op": "layer_visible", "layer": typed_index(&typed, "ClipBase"), "visible": false}),
    );
    let typed = typed_edit(
        &typed,
        json!({"op": "layer_visible", "layer": typed_index(&typed, "ClipMember"), "visible": false}),
    );
    let (max, mean) = residual(
        &typed_matted(&typed),
        &take16("deep_over_ordinary_bottom_16.png"),
    );
    assert!(max <= 2.0 && mean <= 0.5, "16 bits: max {max}, mean {mean}");
}

#[test]
fn a_photoshop_background_record_imports_as_the_background_at_both_depths() {
    let doc = byte_open(&family_file("knockout", "source.psd"));
    assert_eq!(role(doc), ROLE_BACKGROUND);
    unsafe { dr_doc_free(doc) };
    let typed = typed_open(&family_file("knockout", "source16.psd"));
    assert_eq!(typed_role(&typed), ROLE_BACKGROUND);
    // NULL tolerance of both queries.
    assert_eq!(role(ptr::null()), ROLE_NONE);
    assert_eq!(
        unsafe { dr_precision_doc_background_role(ptr::null()) },
        ROLE_NONE
    );
}

// ------------------------------------------------------------- native --

#[test]
fn the_mark_survives_doctored_at_both_depths_under_word_113() {
    let dir = TempDir::new().unwrap();
    for (family, want) in [
        ("knockout", ROLE_BACKGROUND),
        ("knockout_bottom", ROLE_ORDINARY),
    ] {
        let doc = byte_open(&family_file(family, "source.psd"));
        let path = dir.path().join(format!("{family}.doctored"));
        unsafe { &*doc }
            .save_native(path.to_str().unwrap())
            .unwrap();
        let bytes = std::fs::read(&path).unwrap();
        assert_eq!(common::drdc_word(&bytes, 1), 113, "{family}: word 113");
        let reopened = DrDocument::open(path.to_str().unwrap()).unwrap();
        assert_eq!(role(&reopened), want, "{family}: byte role kept");
        assert_eq!(
            common::flat_pixels(&reopened),
            common::flat_pixels(doc),
            "{family}: the picture is kept"
        );
        unsafe { dr_doc_free(doc) };

        let typed = typed_open(&family_file(family, "source16.psd"));
        let bytes = typed.encode_native(LIMIT, 0, None).unwrap();
        let reopened = PrecisionDocument::parse_native(&bytes, LIMIT, 0, None).unwrap();
        assert_eq!(typed_role(&reopened), want, "{family}: typed role kept");
    }

    // A document with no mark never writes the word.
    let plain = doc_from(&dir, "plain.png", &solid(4, 3, [9, 8, 7, 255]));
    let path = dir.path().join("plain.doctored");
    unsafe { &*plain }
        .save_native(path.to_str().unwrap())
        .unwrap();
    assert_ne!(common::drdc_word(&std::fs::read(&path).unwrap(), 1), 113);
    unsafe { dr_doc_free(plain) };

    // A bad role refuses by name.
    let doc = byte_open(&family_file("knockout_bottom", "source.psd"));
    let path = dir.path().join("bad.doctored");
    unsafe { &*doc }
        .save_native(path.to_str().unwrap())
        .unwrap();
    let mut bytes = std::fs::read(&path).unwrap();
    let at = bytes.windows(4).position(|w| w == b"LBKG").expect("LBKG");
    bytes[at + 12..at + 16].copy_from_slice(&7u32.to_le_bytes());
    std::fs::write(&path, &bytes).unwrap();
    let refused = DrDocument::open(path.to_str().unwrap()).err().unwrap();
    assert!(
        refused.contains("invalid native background mark role"),
        "{refused}"
    );
    unsafe { dr_doc_free(doc) };
}

// ------------------------------------------------------------- export --

#[test]
fn export_writes_the_background_record_and_keeps_an_ordinary_bottom_ordinary() {
    let dir = TempDir::new().unwrap();
    let background = byte_open(&family_file("knockout", "source.psd"));
    let path = dir.path().join("background.psd");
    let record = first_record(&write_psd(background, &path));
    assert!(
        record.is_background(),
        "channels {:?}, flags {:#x}",
        record.channels,
        record.flags
    );
    let reopened = byte_open(&path);
    assert_eq!(role(reopened), ROLE_BACKGROUND);
    unsafe { dr_doc_free(reopened) };
    unsafe { dr_doc_free(background) };

    let typed = typed_open(&family_file("knockout", "source16.psd"));
    let path = dir.path().join("background16.psd");
    let record = first_record(&typed_write_psd(&typed, &path));
    assert!(record.is_background(), "16 bits: {:?}", record.channels);
    assert_eq!(typed_role(&typed_open(&path)), ROLE_BACKGROUND);

    let ordinary = byte_open(&family_file("knockout_bottom", "source.psd"));
    let path = dir.path().join("ordinary.psd");
    let record = first_record(&write_psd(ordinary, &path));
    assert!(record.channels.contains(&-1) && !record.is_background());
    let reopened = byte_open(&path);
    assert_eq!(role(reopened), ROLE_ORDINARY);
    assert_eq!(
        common::flat_pixels(reopened),
        common::flat_pixels(ordinary),
        "the punched hole survives the round trip"
    );
    unsafe { dr_doc_free(reopened) };
    unsafe { dr_doc_free(ordinary) };
}

/// An unmarked Doctored document whose bottom D5 treats as the Background:
/// written as a Background record when a written knockout reaches it and it
/// is opaque, so neither side punches; with one transparent pixel it stays an
/// ordinary record and the export says so.
#[test]
fn an_unmarked_qualifying_bottom_under_a_deep_knockout_is_written_as_the_background() {
    let dir = TempDir::new().unwrap();
    let mut doc = doc_from(&dir, "b.png", &solid(8, 6, [20, 40, 60, 255]));
    assert_eq!(role(doc), ROLE_NONE);
    doc = add_layer(
        &dir,
        "t.png",
        doc,
        0,
        &solid(8, 6, [200, 40, 40, 255]),
        "Top",
    );
    doc = set_style(doc, 1, "{\"knockout\":\"deep\",\"fill_opacity\":0}");
    assert!(
        !export_losses(doc)
            .iter()
            .any(|t| t.contains("which Doctored treats as the Background")),
        "nothing to say"
    );
    let path = dir.path().join("unmarked.psd");
    assert!(first_record(&write_psd(doc, &path)).is_background());
    let reopened = byte_open(&path);
    assert_eq!(role(reopened), ROLE_BACKGROUND);
    assert_eq!(common::flat_pixels(reopened), common::flat_pixels(doc));
    unsafe { dr_doc_free(reopened) };
    unsafe { dr_doc_free(doc) };

    let mut holed = solid(8, 6, [20, 40, 60, 255]);
    holed.put_pixel(0, 0, image::Rgba([20, 40, 60, 0]));
    let mut doc = doc_from(&dir, "h.png", &holed);
    doc = add_layer(
        &dir,
        "t2.png",
        doc,
        0,
        &solid(8, 6, [200, 40, 40, 255]),
        "Top",
    );
    doc = set_style(doc, 1, "{\"knockout\":\"deep\",\"fill_opacity\":0}");
    assert!(
        export_losses(doc)
            .iter()
            .any(|t| t.contains("so a PSD cannot store it as a Photoshop Background layer")),
        "the transparent bottom is said"
    );
    let path = dir.path().join("holed.psd");
    let record = first_record(&write_psd(doc, &path));
    assert!(record.channels.contains(&-1) && !record.is_background());
    let reopened = byte_open(&path);
    assert_eq!(role(reopened), ROLE_ORDINARY);
    unsafe { dr_doc_free(reopened) };
    unsafe { dr_doc_free(doc) };

    // A bottom no knockout reaches keeps the prior writer's record.
    let plain = doc_from(&dir, "p.png", &solid(8, 6, [20, 40, 60, 255]));
    let path = dir.path().join("plain.psd");
    assert!(first_record(&write_psd(plain, &path))
        .channels
        .contains(&-1));
    unsafe { dr_doc_free(plain) };
}

// ---------------------------------------------- what keeps the mark --

#[test]
fn conversions_duplicates_and_merges_keep_the_mark_and_flatten_drops_it() {
    let doc = byte_open(&family_file("knockout_bottom", "source.psd"));
    let before = common::flat_pixels(doc);

    // 8 → 16 → 8: the ordinary bottom stays ordinary, and the hole with it.
    let mut err: *mut c_char = ptr::null_mut();
    let wide = unsafe { dr_doc_promote_structure(doc, LIMIT, 0, ptr::null(), &mut err) };
    assert!(!wide.is_null(), "promote: {}", take_err_string(err));
    assert_eq!(typed_role(unsafe { &*wide }), ROLE_ORDINARY);
    let narrow = unsafe {
        dr_precision_doc_narrow_structure_to_u8(wide, 0, LIMIT, 0, ptr::null(), &mut err)
    };
    assert!(!narrow.is_null(), "narrow: {}", take_err_string(err));
    assert_eq!(role(narrow), ROLE_ORDINARY);
    assert_eq!(
        common::flat_pixels(narrow),
        before,
        "the hole survives 8 → 16 → 8"
    );
    unsafe { dr_doc_free(narrow) };
    unsafe { doctored_core::ffi_precision::dr_precision_doc_free(wide) };

    // Duplicate Document copies the bottom under a new id, mark included.
    let indices: Vec<usize> = (0..unsafe { &*doc }.layers.len()).collect();
    let copy = unsafe { dr_doc_duplicate_to_new_document(doc, indices.as_ptr(), indices.len()) };
    assert!(!copy.is_null());
    assert_eq!(role(copy), ROLE_ORDINARY);
    unsafe { dr_doc_free(copy) };

    // Merge Down of Mid into the bottom keeps the lowest entry's id, and the
    // mark with it; Flatten mints a new bottom, which no mark names.
    let merged = unsafe { dr_doc_merging_down(doc, 1) };
    assert!(!merged.is_null());
    assert_eq!(role(merged), ROLE_ORDINARY);
    unsafe { dr_doc_free(merged) };
    let flat = unsafe { dr_doc_flattening(doc) };
    assert!(!flat.is_null());
    assert_eq!(role(flat), ROLE_NONE);
    unsafe { dr_doc_free(flat) };
    unsafe { dr_doc_free(doc) };
}
