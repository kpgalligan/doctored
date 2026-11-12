//! Photoshop's `lspf` layer locks, both directions and both depths.
//!
//! Every expectation is written here from Adobe's table and psd-tools'
//! `ProtectedSetting` (1 transparency, 2 pixels, 4 position, 8 nesting,
//! `0x80000000` complete), never read back out of `psd_locks`: the import
//! side builds its own files and reads the FFI lock bits and report
//! sentences, the export side walks the written bytes with this file's own
//! record walk.
//!
//! - the decode table at 8 and 16 bits, duplicates included;
//! - header flag bit 0, which is never read as a lock;
//! - ImageMagick's `samples/layered.psd` and an alpha-less two-record file;
//! - a locked folder (whose lock covers its contents), a locked closing
//!   divider and an unrecognized one;
//! - a locked folder dissolved past the 10-level cap;
//! - Photoshop's own Background (`psd_photoshop/knockout`) and a synthesized
//!   one;
//! - round trips at 8 and 16 bits, the unlocked writer's bytes, the export
//!   reports and the flattened mode.

use std::ffi::{c_char, CStr, CString};
use std::path::Path;
use std::ptr;

use doctored_core::doc::DrDocument;
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_doc::*;
use doctored_core::ffi_group::{
    dr_doc_group_layers, dr_doc_layer_locks, dr_doc_lock_block, dr_doc_with_layer_locks,
};
use doctored_core::ffi_precision::{dr_precision_doc_free, dr_precision_doc_info};
use doctored_core::ffi_precision_edit::dr_precision_doc_edit;
use doctored_core::ffi_psd::{
    dr_doc_open_psd, dr_doc_psd_export_report, dr_doc_save_psd, dr_precision_doc_open_psd,
    dr_precision_doc_psd_export_report, dr_precision_doc_save_psd, dr_psd_import_report,
};
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer as TLayer};
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES as LIMIT};
use serde_json::Value;
use tempfile::TempDir;

use crate::common;
use crate::psd16_fixture as psd16;
use crate::psd_mask_fixture as psd8;

use common::{add_layer, adjust_meta, apply, cpath, doc_from, set_meta, solid, take_err_string};

const LAYERS: i32 = 0;
const FLATTENED: i32 = 1;

// Doctored's lock bits, restated from `doc_lock`'s table.
const TRANSPARENCY: u32 = 1;
const PIXELS: u32 = 2;
const POSITION: u32 = 4;
const ALL: u32 = 7;

const PROTECTION_LOSS: &str = "nonzero or unrecognized layer-protection";
const ZERO_OBSERVATION: &str = "has zero layer-protection flags";
const BACKGROUND_OBSERVATION: &str = "is Photoshop's Background layer";

// ------------------------------------------------------------- the reports --

fn strings(value: &Value, key: &str) -> Vec<String> {
    value[key]
        .as_array()
        .unwrap_or_else(|| panic!("{key} array in {value}"))
        .iter()
        .map(|v| v.as_str().expect("string").to_string())
        .collect()
}

fn mentions(value: &Value, key: &str, fragment: &str) -> bool {
    strings(value, key).iter().any(|s| s.contains(fragment))
}

fn import_report(path: &Path) -> Value {
    let mut err = ptr::null_mut();
    let json = unsafe { dr_psd_import_report(cpath(path).as_ptr(), &mut err) };
    assert!(!json.is_null(), "{}", take_err_string(err));
    serde_json::from_str(&take_err_string(json)).expect("import report JSON")
}

/// An 8-bit layered import: its report and the lock bits of every entry,
/// bottom-first.
fn import8(path: &Path) -> (Value, Vec<u32>) {
    let report = import_report(path);
    let token = CString::new(report["source_fingerprint"].as_str().unwrap()).unwrap();
    let mut err = ptr::null_mut();
    let doc = unsafe {
        dr_doc_open_psd(
            cpath(path).as_ptr(),
            LAYERS,
            token.as_ptr(),
            std::ptr::null(),
            &mut err,
        )
    };
    assert!(!doc.is_null(), "open_psd: {}", take_err_string(err));
    let locks = byte_locks(doc);
    unsafe { dr_doc_free(doc) };
    (report, locks)
}

fn byte_locks(doc: *const DrDocument) -> Vec<u32> {
    (0..unsafe { dr_doc_layer_count(doc) })
        .map(|i| unsafe { dr_doc_layer_locks(doc, i) })
        .collect()
}

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

fn open16(path: &Path, report: &Value) -> Owned {
    let token = CString::new(report["source_fingerprint"].as_str().unwrap()).unwrap();
    let mut err = ptr::null_mut();
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
    assert!(!doc.is_null(), "typed open_psd: {}", take_err_string(err));
    Owned(doc)
}

/// A 16-bit layered import: its report and each postorder record's locks.
fn import16(path: &Path) -> (Value, Vec<u32>) {
    let report = import_report(path);
    assert_eq!(report["import_precision"], "rgba16", "{report}");
    let doc = open16(path, &report);
    (report, typed_locks(&doc))
}

fn info(document: &PrecisionDocument) -> Value {
    let mut err = ptr::null_mut();
    let json = unsafe { dr_precision_doc_info(document, &mut err) };
    assert!(!json.is_null(), "{}", take_err_string(err));
    serde_json::from_str(&take_err_string(json)).expect("info JSON")
}

fn typed_locks(document: &PrecisionDocument) -> Vec<u32> {
    info(document)["layers"]
        .as_array()
        .expect("layers")
        .iter()
        .map(|row| row["locks"].as_u64().expect("a locks row") as u32)
        .collect()
}

// --------------------------------------------------------------- fixtures --

fn be(word: u32) -> Vec<u8> {
    word.to_be_bytes().to_vec()
}

/// A one-record 8-bit file whose raster carries `blocks` and `flags`.
fn file8(blocks: Vec<u8>, flags: u8) -> Vec<u8> {
    let rgba: Vec<u8> = (0..4u8).flat_map(|i| [i * 30, 90, 150, 255]).collect();
    let mut record = psd8::Record::raster([0, 0, 2, 2], &rgba, false);
    record.name = "Only";
    record.blocks = blocks;
    record.flags = flags;
    psd8::fixture(&[record], [2, 2], false)
}

fn channels16() -> Vec<(i16, Vec<u8>)> {
    let samples: Vec<[u16; 4]> = (0..4u16)
        .map(|i| [1000 + i * 900, 22222, 33333, 65535])
        .collect();
    let plane = |c: usize| psd16::raw_plane(&samples.iter().map(|p| p[c]).collect::<Vec<_>>());
    vec![(-1, plane(3)), (0, plane(0)), (1, plane(1)), (2, plane(2))]
}

/// The 16-bit twin of [`file8`].
fn file16(blocks: Vec<u8>, flags: u8) -> Vec<u8> {
    let mut record = psd16::Record::raster("Only", [0, 0, 2, 2], channels16());
    record.blocks = blocks;
    record.flags = flags;
    psd16::fixture(
        (2, 2),
        &[record],
        IccProfile::srgb().bytes(),
        (72.0, 72.0),
        &[[1000, 22222, 33333, 65535]; 4],
        false,
        false,
    )
}

fn write(dir: &TempDir, name: &str, bytes: &[u8]) -> std::path::PathBuf {
    let path = dir.path().join(name);
    std::fs::write(&path, bytes).unwrap();
    path
}

// ----------------------------------------------------- the decode table --

/// `(body, expected locks, protection loss, zero observation)`.
fn decode_table() -> Vec<(Vec<u8>, u32, bool, bool)> {
    vec![
        (be(0x0), 0, false, true),
        (be(0x1), TRANSPARENCY, false, false),
        (be(0x2), PIXELS, false, false),
        (be(0x4), POSITION, false, false),
        // Transparency | pixels | position is Doctored's Lock All.
        (be(0x7), ALL, false, false),
        // The nesting bit is dropped; the other three are still all of them.
        (be(0xF), ALL, false, false),
        // psd-tools' `complete`.
        (be(0x8000_0000), ALL, false, false),
        (be(0x8000_000D), ALL, false, false),
        // Nesting alone: nothing Doctored can hold, and nothing lost either.
        (be(0x8), 0, false, false),
        // Bits no source documents, and bodies that are not one word.
        (be(0x10), 0, true, false),
        (be(0x4000_0000), 0, true, false),
        (vec![0, 2], 0, true, false),
        (vec![0, 0, 0, 2, 0, 0, 0, 2], 0, true, false),
    ]
}

fn check_decode(depth: u16, report: &Value, locks: &[u32], expected: (u32, bool, bool)) {
    let (lock, loss, zero) = expected;
    assert_eq!(locks, [lock], "{depth}-bit: {report}");
    assert_eq!(
        mentions(report, "verified_losses", PROTECTION_LOSS),
        loss,
        "{depth}-bit: {report}"
    );
    assert_eq!(
        mentions(report, "observations", ZERO_OBSERVATION),
        zero,
        "{depth}-bit: {report}"
    );
    if !loss && !zero {
        assert!(
            !mentions(report, "observations", "layer-protection"),
            "{depth}-bit: a carried word adds no observation: {report}"
        );
    }
}

#[test]
fn the_decode_table_holds_at_both_depths() {
    let dir = TempDir::new().unwrap();
    for (index, (body, lock, loss, zero)) in decode_table().into_iter().enumerate() {
        let path = write(
            &dir,
            &format!("decode{index}.psd"),
            &file8(psd8::block(b"lspf", &body), 8),
        );
        let (report, locks) = import8(&path);
        check_decode(8, &report, &locks, (lock, loss, zero));

        let path = write(
            &dir,
            &format!("decode{index}-16.psd"),
            &file16(psd16::block(b"lspf", &body), 8),
        );
        let (report, locks) = import16(&path);
        check_decode(16, &report, &locks, (lock, loss, zero));
    }
}

#[test]
fn duplicate_words_carry_when_equal_and_are_lost_when_they_disagree() {
    let dir = TempDir::new().unwrap();
    let equal = [psd8::block(b"lspf", &be(2)), psd8::block(b"lspf", &be(2))].concat();
    let (report, locks) = import8(&write(&dir, "equal.psd", &file8(equal.clone(), 8)));
    assert_eq!(locks, [PIXELS], "{report}");
    assert!(!mentions(&report, "verified_losses", PROTECTION_LOSS));
    let (report, locks) = import16(&write(&dir, "equal16.psd", &file16(equal, 8)));
    assert_eq!(locks, [PIXELS], "{report}");
    assert!(!mentions(&report, "verified_losses", PROTECTION_LOSS));

    let differ = [psd8::block(b"lspf", &be(2)), psd8::block(b"lspf", &be(4))].concat();
    let (report, locks) = import8(&write(&dir, "differ.psd", &file8(differ.clone(), 8)));
    assert_eq!(locks, [0], "{report}");
    assert!(mentions(&report, "verified_losses", PROTECTION_LOSS));
    let (report, locks) = import16(&write(&dir, "differ16.psd", &file16(differ, 8)));
    assert_eq!(locks, [0], "{report}");
    assert!(mentions(&report, "verified_losses", PROTECTION_LOSS));
}

// ------------------------------------------------------- header flag bit 0 --

#[test]
fn header_bit_zero_is_never_a_lock() {
    let dir = TempDir::new().unwrap();
    // (lspf body, expected locks, protection loss)
    let cases: [(Option<u32>, u32, bool); 6] = [
        (None, 0, true),
        (Some(0), 0, true),
        // Position alone does not cover what the header bit claims.
        (Some(0x4), POSITION, true),
        (Some(0x1), TRANSPARENCY, false),
        (Some(0x2), PIXELS, false),
        (Some(0x8000_0000), ALL, false),
    ];
    for (index, (word, lock, loss)) in cases.into_iter().enumerate() {
        let blocks = word.map_or(Vec::new(), |w| psd8::block(b"lspf", &be(w)));
        let path = write(&dir, &format!("header{index}.psd"), &file8(blocks, 8 | 1));
        let (report, locks) = import8(&path);
        assert_eq!(locks, [lock], "{word:?}: {report}");
        assert_eq!(
            mentions(&report, "verified_losses", PROTECTION_LOSS),
            loss,
            "{word:?}: {report}"
        );
    }
}

#[test]
fn an_alpha_less_bottom_with_the_header_bit_carries_nothing() {
    let dir = TempDir::new().unwrap();
    // ImageMagick's own layered PSD: record 0 has no `-1` channel and sets
    // header bit 0, with no `lspf` at all.
    let sample = Path::new(env!("CARGO_MANIFEST_DIR")).join("../samples/layered.psd");
    if sample.exists() {
        let (report, locks) = import8(&sample);
        assert!(!locks.is_empty());
        assert!(locks.iter().all(|&l| l == 0), "{report}");
        assert!(
            !mentions(&report, "observations", BACKGROUND_OBSERVATION),
            "{report}"
        );
    }
    let rgba: Vec<u8> = (0..4u8).flat_map(|i| [i * 30, 90, 150, 255]).collect();
    let mut bottom = psd8::Record::raster([0, 0, 2, 2], &rgba, false);
    bottom.channels.retain(|(id, _)| *id != -1);
    bottom.name = "Bottom";
    bottom.flags = 1;
    let mut top = psd8::Record::raster([0, 0, 2, 2], &rgba, false);
    top.name = "Top";
    let path = write(
        &dir,
        "alphaless.psd",
        &psd8::fixture(&[bottom, top], [2, 2], false),
    );
    let (report, locks) = import8(&path);
    assert_eq!(locks, [0, 0], "{report}");
    assert!(
        !mentions(&report, "observations", BACKGROUND_OBSERVATION),
        "{report}"
    );
}

// ------------------------------------------------------------------ folders --

#[test]
fn a_locked_folder_locks_its_contents_and_dividers_carry_nothing() {
    let dir = TempDir::new().unwrap();
    let rgba: Vec<u8> = (0..4u8).flat_map(|i| [i * 30, 90, 150, 255]).collect();
    let build = |divider_word: Option<Vec<u8>>, folder_word: Option<u32>| {
        let mut closer = psd8::Record::folder(3, *b"norm");
        closer.name = "</Layer group>";
        if let Some(body) = divider_word {
            closer.blocks = psd8::block(b"lspf", &body);
        }
        let mut inner = psd8::Record::raster([0, 0, 2, 2], &rgba, false);
        inner.name = "Inner";
        let mut folder = psd8::Record::folder(1, *b"norm");
        folder.name = "Folder";
        if let Some(word) = folder_word {
            folder.blocks = psd8::block(b"lspf", &be(word));
        }
        psd8::fixture(&[closer, inner, folder], [2, 2], false)
    };

    // Bottom-first: the child, then its folder. The lock is stored on the
    // group alone, and it covers the child: the child's Pixels
    // edits are blocked by the group's bit.
    let folder_path = write(&dir, "folder.psd", &build(None, Some(0x2)));
    let (report, locks) = import8(&folder_path);
    assert_eq!(locks, [0, PIXELS], "{report}");
    assert!(!mentions(&report, "verified_losses", PROTECTION_LOSS));
    let token = CString::new(report["source_fingerprint"].as_str().unwrap()).unwrap();
    let mut err = ptr::null_mut();
    let doc = unsafe {
        dr_doc_open_psd(
            cpath(&folder_path).as_ptr(),
            LAYERS,
            token.as_ptr(),
            std::ptr::null(),
            &mut err,
        )
    };
    assert!(!doc.is_null(), "open_psd: {}", take_err_string(err));
    assert_eq!(
        unsafe { dr_doc_lock_block(doc, 0, 0) },
        PIXELS,
        "the group's lock blocks it"
    );
    unsafe { dr_doc_free(doc) };

    let (report, locks) = import8(&write(&dir, "closer.psd", &build(Some(be(0x2)), None)));
    assert_eq!(locks, [0, 0], "a divider's word is attached to nothing");
    assert!(
        !mentions(&report, "verified_losses", PROTECTION_LOSS),
        "{report}"
    );

    let (report, locks) = import8(&write(&dir, "bad-closer.psd", &build(Some(be(0x10)), None)));
    assert_eq!(locks, [0, 0]);
    assert!(
        strings(&report, "verified_losses")
            .iter()
            .any(|s| s.starts_with("PSD bounding-divider metadata")
                && s.contains("layer-protection")),
        "{report}"
    );
}

const DISSOLVED_LOCK_LOSS: &str = "its locks are added to each layer it held";

#[test]
fn a_locked_folder_dissolved_past_the_cap_locks_its_contents() {
    let dir = TempDir::new().unwrap();
    let rgba: Vec<u8> = (0..4u8).flat_map(|i| [i * 30, 90, 150, 255]).collect();
    let mut records = vec![psd8::Record::folder(3, *b"norm"); 11];
    let mut art = psd8::Record::raster([0, 0, 2, 2], &rgba, false);
    art.name = "Art";
    records.push(art);
    for level in 0..11 {
        let mut folder = psd8::Record::folder(1, *b"norm");
        folder.name = "Folder";
        // The innermost folder: ten ancestors above it.
        if level == 0 {
            folder.blocks = psd8::block(b"lspf", &be(0x2));
        }
        records.push(folder);
    }
    let path = write(
        &dir,
        "dissolved.psd",
        &psd8::fixture(&records, [2, 2], false),
    );
    let (report, locks) = import8(&path);
    assert!(
        mentions(&report, "verified_losses", "deeper than 10"),
        "{report}"
    );
    assert!(
        mentions(&report, "verified_losses", DISSOLVED_LOCK_LOSS),
        "{report}"
    );
    // Bottom-first: the layer the dissolved folder held, then the ten kept
    // folders. The folder's Pixels bit moved onto its layer.
    let mut expected = vec![PIXELS];
    expected.extend([0; 10]);
    assert_eq!(locks, expected, "{report}");

    let token = CString::new(report["source_fingerprint"].as_str().unwrap()).unwrap();
    let mut err = ptr::null_mut();
    let doc = unsafe {
        dr_doc_open_psd(
            cpath(&path).as_ptr(),
            LAYERS,
            token.as_ptr(),
            std::ptr::null(),
            &mut err,
        )
    };
    assert!(!doc.is_null(), "open_psd: {}", take_err_string(err));
    assert_eq!(
        unsafe { dr_doc_lock_block(doc, 0, 0) },
        PIXELS,
        "the moved Pixels bit blocks it"
    );
    unsafe { dr_doc_free(doc) };
}

#[test]
fn a_locked_folder_dissolved_past_the_cap_locks_its_contents_at_sixteen_bits() {
    let dir = TempDir::new().unwrap();
    let mut records: Vec<psd16::Record> = (0..11)
        .map(|_| psd16::Record::folder("</Layer group>", 3, *b"norm"))
        .collect();
    records.push(psd16::Record::raster("Art", [0, 0, 2, 2], channels16()));
    for level in 0..11 {
        let mut folder = psd16::Record::folder("Folder", 1, *b"norm");
        // The innermost folder: ten ancestors above it.
        if level == 0 {
            folder.blocks = psd16::block(b"lspf", &be(0x2));
        }
        records.push(folder);
    }
    let path = write(
        &dir,
        "dissolved16.psd",
        &psd16::fixture(
            (2, 2),
            &records,
            IccProfile::srgb().bytes(),
            (72.0, 72.0),
            &[[1000, 22222, 33333, 65535]; 4],
            false,
            false,
        ),
    );
    let (report, locks) = import16(&path);
    assert!(
        mentions(&report, "verified_losses", "deeper than 10"),
        "{report}"
    );
    assert!(
        mentions(&report, "verified_losses", DISSOLVED_LOCK_LOSS),
        "{report}"
    );
    // Postorder: the layer the dissolved folder held, then the ten kept
    // folders.
    let mut expected = vec![PIXELS];
    expected.extend([0; 10]);
    assert_eq!(locks, expected, "{report}");
}

// --------------------------------------------------------------- Background --

fn photoshop(name: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/psd_photoshop/knockout")
        .join(name)
}

#[test]
fn photoshops_background_implicit_locks_are_not_carried() {
    let (report, locks) = import8(&photoshop("source.psd"));
    assert_eq!(locks[0], 0, "the Background is entry 0: {report}");
    assert!(locks.iter().all(|&l| l == 0), "{locks:?}");
    assert!(
        mentions(&report, "observations", BACKGROUND_OBSERVATION),
        "{report}"
    );
    assert!(
        !mentions(&report, "verified_losses", "layer-protection"),
        "{report}"
    );

    let (report, locks) = import16(&photoshop("source16.psd"));
    assert_eq!(locks[0], 0, "the Background is record 0: {report}");
    assert!(locks.iter().all(|&l| l == 0), "{locks:?}");
    assert!(
        mentions(&report, "observations", BACKGROUND_OBSERVATION),
        "{report}"
    );
    assert!(
        !mentions(&report, "verified_losses", "layer-protection"),
        "{report}"
    );
}

#[test]
fn a_synthesized_background_keeps_only_what_the_user_added() {
    let dir = TempDir::new().unwrap();
    let rgba: Vec<u8> = (0..4u8).flat_map(|i| [i * 30, 90, 150, 255]).collect();
    for (word, lock, observed) in [(0x0Fu32, PIXELS, true), (0x8000_000D, ALL, false)] {
        let mut background = psd8::Record::raster([0, 0, 2, 2], &rgba, false);
        background.channels.retain(|(id, _)| *id != -1);
        background.name = "Background";
        background.flags = 0x09;
        background.blocks = psd8::block(b"lspf", &be(word));
        let path = write(
            &dir,
            &format!("background{word:x}.psd"),
            &psd8::fixture(&[background], [2, 2], false),
        );
        let (report, locks) = import8(&path);
        assert_eq!(locks, [lock], "{word:#x}: {report}");
        assert_eq!(
            mentions(&report, "observations", BACKGROUND_OBSERVATION),
            observed,
            "{word:#x}: {report}"
        );
    }
}

// --------------------------------------------------------------- the writer --

/// One record of a written PSD: its Pascal name, flags byte and every
/// additional block in file order.
struct Written {
    name: String,
    flags: u8,
    blocks: Vec<([u8; 4], Vec<u8>)>,
}

impl Written {
    fn lspf(&self) -> Option<&[u8]> {
        self.blocks
            .iter()
            .find(|(key, _)| key == b"lspf")
            .map(|(_, body)| body.as_slice())
    }
    fn position(&self, key: &[u8; 4]) -> Option<usize> {
        self.blocks.iter().position(|(k, _)| k == key)
    }
}

/// Every record of a classic PSD's layer information, from Adobe's framing
/// and depth-agnostic: channel data is only skipped by declared length.
fn walk(bytes: &[u8]) -> Vec<Written> {
    let be32 = |s: &[u8], at: usize| u32::from_be_bytes(s[at..at + 4].try_into().unwrap()) as usize;
    let mut cursor = 26usize;
    cursor += 4 + be32(bytes, cursor);
    cursor += 4 + be32(bytes, cursor);
    let section_len = be32(bytes, cursor);
    let section = &bytes[cursor + 4..cursor + 4 + section_len];
    let info_len = be32(section, 0);
    let mut info = &section[4..4 + info_len];
    if info_len == 0 {
        let mut at = 4;
        at += 4 + be32(section, at);
        while at + 12 <= section.len() {
            let len = be32(section, at + 8);
            if &section[at + 4..at + 8] == b"Lr16" {
                info = &section[at + 12..at + 12 + len];
                break;
            }
            at += 12 + len + (len.wrapping_neg() & 3);
        }
    }
    if info.is_empty() {
        return Vec::new();
    }
    let count = i16::from_be_bytes(info[..2].try_into().unwrap()).unsigned_abs() as usize;
    let mut at = 2usize;
    let mut out = Vec::new();
    for _ in 0..count {
        at += 16;
        let channels = u16::from_be_bytes(info[at..at + 2].try_into().unwrap()) as usize;
        at += 2 + channels * 6;
        let flags = info[at + 10];
        at += 12;
        let extra_len = be32(info, at);
        let extra = &info[at + 4..at + 4 + extra_len];
        at += 4 + extra_len;
        let mut c = 0usize;
        c += 4 + be32(extra, c);
        c += 4 + be32(extra, c);
        let pascal = extra[c] as usize;
        let name = String::from_utf8_lossy(&extra[c + 1..c + 1 + pascal]).into_owned();
        c += 1 + pascal;
        while !c.is_multiple_of(4) {
            c += 1;
        }
        let mut blocks = Vec::new();
        while c + 12 <= extra.len() {
            assert_eq!(&extra[c..c + 4], b"8BIM");
            let key: [u8; 4] = extra[c + 4..c + 8].try_into().unwrap();
            let len = be32(extra, c + 8);
            blocks.push((key, extra[c + 12..c + 12 + len].to_vec()));
            c += 12 + len + len % 2;
        }
        out.push(Written {
            name,
            flags,
            blocks,
        });
    }
    out
}

/// The written file's expectations, by record name: the `lspf` body or none.
fn check_written(records: &[Written], expected: &[(&str, Option<u32>)], labelled: &[&str]) {
    for record in records {
        assert_eq!(record.flags & 1, 0, "header bit 0 is never written");
        if record.name == "</Layer group>" {
            assert!(record.lspf().is_none(), "no lspf on a closing divider");
            continue;
        }
        let want = expected
            .iter()
            .find(|(name, _)| *name == record.name)
            .unwrap_or_else(|| panic!("unexpected record {}", record.name))
            .1;
        assert_eq!(
            record.lspf().map(|b| b.to_vec()),
            want.map(be),
            "{}",
            record.name
        );
        if labelled.contains(&record.name.as_str()) {
            let lspf = record.position(b"lspf").expect("lspf");
            let lclr = record.position(b"lclr").expect("lclr");
            assert_eq!(
                lspf + 1,
                lclr,
                "{}: lspf sits immediately before lclr",
                record.name
            );
        }
    }
}

fn byte_export_report(doc: *const DrDocument, mode: i32) -> Value {
    let mut err: *mut c_char = ptr::null_mut();
    let json = unsafe { dr_doc_psd_export_report(doc, 0, mode, &mut err) };
    assert!(!json.is_null(), "export report: {}", take_err_string(err));
    let text = unsafe { CStr::from_ptr(json) }
        .to_string_lossy()
        .into_owned();
    unsafe { dr_string_free(json) };
    serde_json::from_str(&text).expect("export report JSON")
}

fn typed_export_report(doc: &PrecisionDocument, mode: i32) -> Value {
    let mut err: *mut c_char = ptr::null_mut();
    let json = unsafe { dr_precision_doc_psd_export_report(doc, 0, mode, &mut err) };
    assert!(
        !json.is_null(),
        "typed export report: {}",
        take_err_string(err)
    );
    let text = unsafe { CStr::from_ptr(json) }
        .to_string_lossy()
        .into_owned();
    unsafe { dr_string_free(json) };
    serde_json::from_str(&text).expect("typed export report JSON")
}

fn save8(doc: *const DrDocument, path: &Path) -> Vec<u8> {
    let mut err: *mut c_char = ptr::null_mut();
    let ok = unsafe {
        dr_doc_save_psd(
            doc,
            ptr::null(),
            cpath(path).as_ptr(),
            0,
            LAYERS,
            std::ptr::null(),
            &mut err,
        )
    };
    assert!(ok, "save_psd: {}", take_err_string(err));
    std::fs::read(path).unwrap()
}

fn save16(doc: &PrecisionDocument, path: &Path) -> Vec<u8> {
    let projection = doc.render(LIMIT, 0, None).expect("projection");
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
            std::ptr::null(),
            &mut err,
        )
    };
    assert!(ok, "typed save_psd: {}", take_err_string(err));
    std::fs::read(path).unwrap()
}

fn check_layered_report(report: &Value) {
    assert!(
        !strings(report, "losses")
            .iter()
            .any(|s| s.contains("is locked")),
        "{report}"
    );
    assert!(
        !strings(report, "general_limitations")
            .iter()
            .any(|s| s.contains("layer locks")),
        "{report}"
    );
}

const W: u32 = 4;
const H: u32 = 3;

/// Base (Pixels, labelled), Adj (an Invert adjustment, Position), and a
/// Folder (Lock All, labelled) holding an unlocked Inner — bottom-first.
fn byte_document(dir: &TempDir) -> *mut DrDocument {
    let mut doc = doc_from(dir, "base.png", &solid(W, H, [20, 40, 60, 255]));
    doc = apply(doc, |d| unsafe {
        dr_doc_with_layer_name(d, 0, c"Base".as_ptr())
    });
    doc = add_layer(dir, "adj.png", doc, 0, &solid(W, H, [0, 0, 0, 0]), "Adj");
    doc = set_meta(doc, 1, &adjust_meta("invert", "{}"));
    doc = add_layer(
        dir,
        "inner.png",
        doc,
        1,
        &solid(W, H, [200, 40, 40, 255]),
        "Inner",
    );
    let indices = [2usize];
    let (mut group, mut cleared, mut cleared_len, mut reordered, mut reordered_len) =
        (0usize, [0usize; 8], 0usize, [0usize; 8], 0usize);
    let name = CString::new("Folder").unwrap();
    doc = apply(doc, |d| unsafe {
        dr_doc_group_layers(
            d,
            indices.as_ptr(),
            indices.len(),
            name.as_ptr(),
            &mut group,
            cleared.as_mut_ptr(),
            &mut cleared_len,
            reordered.as_mut_ptr(),
            &mut reordered_len,
            8,
        )
    });
    assert_eq!(group, 3, "the folder sits above its child");
    doc = apply(doc, |d| unsafe { dr_doc_with_layer_locks(d, 0, PIXELS) });
    doc = apply(doc, |d| unsafe { dr_doc_with_layer_locks(d, 1, POSITION) });
    doc = apply(doc, |d| unsafe { dr_doc_with_layer_locks(d, 3, ALL) });
    doc = apply(doc, |d| unsafe { dr_doc_with_layer_label(d, 0, 3) });
    doc = apply(doc, |d| unsafe { dr_doc_with_layer_label(d, 3, 5) });
    doc
}

const EXPECTED: [(&str, Option<u32>); 4] = [
    ("Base", Some(0x2)),
    ("Adj", Some(0x4)),
    ("Inner", None),
    ("Folder", Some(0x8000_0000)),
];

#[test]
fn an_eight_bit_round_trip_writes_and_rereads_lspf() {
    let dir = TempDir::new().unwrap();
    let doc = byte_document(&dir);
    let locks = byte_locks(doc);
    assert_eq!(locks, [PIXELS, POSITION, 0, ALL]);
    check_layered_report(&byte_export_report(doc, LAYERS));

    let path = dir.path().join("locked.psd");
    let bytes = save8(doc, &path);
    unsafe { dr_doc_free(doc) };
    let records = walk(&bytes);
    assert_eq!(records.len(), 5, "four entries and one closing divider");
    check_written(&records, &EXPECTED, &["Base", "Folder"]);

    let (report, reread) = import8(&path);
    assert_eq!(reread, locks, "{report}");
    assert!(!mentions(&report, "verified_losses", "layer-protection"));

    // An unlocked document writes no `lspf` at all.
    let plain = doc_from(&dir, "plain.png", &solid(W, H, [20, 40, 60, 255]));
    let bytes = save8(plain, &dir.path().join("plain.psd"));
    assert!(walk(&bytes).iter().all(|r| r.lspf().is_none()));
    assert!(
        !mentions(
            &byte_export_report(plain, FLATTENED),
            "losses",
            "The flattened mode writes one composited raster layer"
        ),
        "an unlocked canvas-sized raster flattens losslessly"
    );
    let locked = apply(plain, |d| unsafe { dr_doc_with_layer_locks(d, 0, PIXELS) });
    assert!(
        mentions(
            &byte_export_report(locked, FLATTENED),
            "losses",
            "The flattened mode writes one composited raster layer"
        ),
        "flattening drops the lock"
    );
    unsafe { dr_doc_free(locked) };
}

// ------------------------------------------------------------ typed depth --

fn typed_image(seed: u16) -> PrecisionImage {
    PrecisionImage::from_rgba16(
        W,
        H,
        vec![[seed, 20000, 30000, 65535]; (W * H) as usize],
        IccProfile::srgb(),
        None,
    )
    .unwrap()
}

fn typed_document(layers: Vec<TLayer>) -> PrecisionDocument {
    PrecisionDocument::from_parts(
        W,
        H,
        IccProfile::srgb(),
        Metadata::default(),
        Resolution::default(),
        layers,
    )
    .unwrap()
}

fn edit(document: &PrecisionDocument, request: &str) -> Owned {
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
    assert!(!out.is_null(), "{request}: {}", take_err_string(err));
    Owned(out)
}

#[test]
fn a_sixteen_bit_round_trip_writes_and_rereads_lspf() {
    let dir = TempDir::new().unwrap();
    let invert =
        serde_json::json!({"type":"adjust","op":"invert","params":{},"evaluation":"precision_v1"})
            .to_string();
    let inner = TLayer::raster("Inner", typed_image(9000), (0, 0), 1.0, true).unwrap();
    let document = typed_document(vec![
        TLayer::raster("Base", typed_image(1000), (0, 0), 1.0, true).unwrap(),
        TLayer::adjustment("Adj", &invert, 1.0, true, &IccProfile::srgb()).unwrap(),
        TLayer::isolated_normal("Folder", vec![inner], 1.0, true).unwrap(),
    ]);
    // Postorder: Base 0, Adj 1, Inner 2, Folder 3.
    let document = edit(&document, r#"{"op":"layer_lock","layer":0,"locks":2}"#);
    let document = edit(&document, r#"{"op":"layer_lock","layer":1,"locks":4}"#);
    let document = edit(&document, r#"{"op":"layer_lock","layer":3,"locks":7}"#);
    let document = edit(&document, r#"{"op":"layer_label","layer":0,"label":3}"#);
    let document = edit(&document, r#"{"op":"layer_label","layer":3,"label":5}"#);
    let locks = typed_locks(&document);
    assert_eq!(locks, [PIXELS, POSITION, 0, ALL]);
    check_layered_report(&typed_export_report(&document, LAYERS));

    let path = dir.path().join("locked16.psd");
    let records = walk(&save16(&document, &path));
    assert_eq!(records.len(), 5, "four entries and one closing divider");
    check_written(&records, &EXPECTED, &["Base", "Folder"]);

    let (report, reread) = import16(&path);
    assert_eq!(reread, locks, "{report}");
    assert!(!mentions(&report, "verified_losses", "layer-protection"));

    // An unlocked document writes no `lspf`, and flattens losslessly.
    let plain = typed_document(vec![TLayer::raster(
        "Plain",
        typed_image(1000),
        (0, 0),
        1.0,
        true,
    )
    .unwrap()]);
    let bytes = save16(&plain, &dir.path().join("plain16.psd"));
    assert!(walk(&bytes).iter().all(|r| r.lspf().is_none()));
    assert!(!mentions(
        &typed_export_report(&plain, FLATTENED),
        "losses",
        "The flattened mode writes one composited raster layer"
    ));
    let locked = edit(&plain, r#"{"op":"layer_lock","layer":0,"locks":2}"#);
    assert!(mentions(
        &typed_export_report(&locked, FLATTENED),
        "losses",
        "The flattened mode writes one composited raster layer"
    ));

    // A 16-bit link group is disclosed as the loss it is.
    let pair = typed_document(vec![
        TLayer::raster("A", typed_image(1000), (0, 0), 1.0, true).unwrap(),
        TLayer::raster("B", typed_image(2000), (0, 0), 1.0, true).unwrap(),
    ]);
    let linked = edit(&pair, r#"{"op":"link_layers","layers":[0,1]}"#);
    assert!(
        mentions(
            &typed_export_report(&linked, LAYERS),
            "losses",
            "belongs to a link group"
        ),
        "{}",
        typed_export_report(&linked, LAYERS)
    );
}
