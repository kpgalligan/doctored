//! KNOCKOUT and "Blend Interior Effects as Group" — the TYPED FORMAT half of
//! Phase 12E-3b: the stored style's two keys (decision iv's storage rule), the
//! typed `.rz` word **50** with NO table of its own, its two reader sentences
//! evaluated once and unconditionally, and the 16-bit PSD export's per-layer
//! loss.
//!
//! Every claim here is an EQUALITY, not a tolerance: the whole point of word
//! 50 is that a document without the keys is byte-for-byte the file the
//! 12E-1c writer produced, that clearing the last key restores those exact
//! bytes again, and that reopen → resave is identical.
//!
//! # The stored 48-writer file
//!
//! `tests/fixtures/rz_typed_48/style_no_knockout.rz` is a FORMAT ORACLE, as
//! `psd_10c/*.psd` is — not a golden image. It was written by the writer
//! that predates word 50, never by this build, and nothing in this tree
//! regenerates it:
//!
//! - commit: 78cb0bacc4279637a6353c3cb6157cf379b40335 (78cb0ba, `1a69abb^`,
//!   the last typed writer without word 50), checked out in a scratch
//!   `git worktree`;
//! - builder: a throwaway, never-committed `core/tests/gen_rz48.rs` holding
//!   this file's `W`, `H`, `image`, `parse` and `Fixture` verbatim and one
//!   ignored test writing `Fixture::new().with(Some("{\"fill_opacity\":0.5}"))
//!   .encode_native(REFERENCE_WORKING_BYTES, 0, None)` to `$DR_OUT`;
//! - command: `cd <worktree>/core && CARGO_TARGET_DIR=<scratch> DR_OUT=<repo>/core/tests/fixtures/rz_typed_48/style_no_knockout.rz
//!   cargo test --release --test gen_rz48 -- --ignored`;
//! - bytes: 3343; sha256
//!   `5e84f701fcf42ec047e372fa866c98c6cc413785ce13565d5acc9870c65adcc4`.
//!
//! The current writer must reproduce it byte for byte. The planes inside it
//! are encoded by the PNG crate, so a dependency bump that moves the PNG
//! encoder's bytes fails that test ON PURPOSE: the stored file is what an
//! older build wrote, and a writer that no longer reproduces it has changed
//! the format.

use std::ffi::{c_char, CStr, CString};
use std::ptr;

use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_precision_edit::dr_precision_doc_edit;
use doctored_core::ffi_psd::{dr_precision_doc_psd_export_report, dr_precision_doc_save_psd};
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer as Layer};
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES as LIMIT};
use doctored_core::style::{Knockout, LayerStyle};

use crate::identity;

const W: u32 = 6;
const H: u32 = 4;
/// The typed word, spelled out here rather than imported, so a change to the
/// constant cannot quietly change the claim.
const KNOCKOUT_WORD: u32 = 50;

// -------------------------------------------------------------- fixtures --

fn image() -> PrecisionImage {
    PrecisionImage::from_rgba16(
        W,
        H,
        vec![[40000u16, 20000, 10000, 65535]; (W * H) as usize],
        IccProfile::srgb(),
        None,
    )
    .unwrap()
}

fn parse(json: &str) -> LayerStyle {
    LayerStyle::from_json(json).unwrap_or_else(|e| panic!("{json}: {e}"))
}

/// A two-record document whose TOP record carries `style`.
///
/// The two records are built ONCE per `Fixture` and cloned per variant, so
/// every byte comparison below is between files whose LAYER IDS are the same
/// — ids are minted per construction and persisted in `LIDS`, so two
/// independently built documents could never be byte-identical.
struct Fixture {
    bottom: Layer,
    top: Layer,
}

impl Fixture {
    fn new() -> Self {
        Self {
            bottom: Layer::raster("BG", image(), (0, 0), 1.0, true).unwrap(),
            top: Layer::raster("Top", image(), (0, 0), 1.0, true).unwrap(),
        }
    }
    fn with(&self, style: Option<&str>) -> PrecisionDocument {
        let mut top = self.top.clone();
        if let Some(style) = style {
            top = top
                .with_layer_style(Some(&parse(style)), (W, H))
                .unwrap_or_else(|e| panic!("{style}: {e}"));
        }
        PrecisionDocument::from_parts(
            W,
            H,
            IccProfile::srgb(),
            Metadata::default(),
            Resolution::default(),
            vec![self.bottom.clone(), top],
        )
        .unwrap()
    }
}

/// A fresh fixture's variant, for the tests that compare only one file.
fn fixture(style: Option<&str>) -> PrecisionDocument {
    Fixture::new().with(style)
}

fn encode(doc: &PrecisionDocument) -> Vec<u8> {
    doc.encode_native(LIMIT, 0, None).unwrap()
}

fn open(bytes: &[u8]) -> Result<PrecisionDocument, String> {
    PrecisionDocument::parse_native(bytes, LIMIT, 0, None)
}

/// `bytes` with a four-byte word inserted at word index `at` (counting the
/// `DRDC` tag as index −1, so 0 is the outermost version word).
fn insert_word(bytes: &[u8], at: usize, word: u32) -> Vec<u8> {
    let cut = 4 + at * 4;
    let mut out = bytes[..cut].to_vec();
    out.extend_from_slice(&word.to_le_bytes());
    out.extend_from_slice(&bytes[cut..]);
    out
}

/// `bytes` with the four-byte word at word index `at` removed.
fn remove_word(bytes: &[u8], at: usize) -> Vec<u8> {
    let cut = 4 + at * 4;
    let mut out = bytes[..cut].to_vec();
    out.extend_from_slice(&bytes[cut + 4..]);
    out
}

// ------------------------------------------------------- the stored style --

#[test]
fn a_knockout_only_typed_style_is_stored_and_a_lone_flag_is_not() {
    // Decision (iv): a typed style is stored iff it is non-identity WITH
    // BLEND IF REMOVED — `fill < 1 || an enabled effect || knockout != none`.
    let doc = fixture(Some("{\"knockout\":\"deep\"}"));
    let style = doc
        .layer(1)
        .unwrap()
        .style()
        .expect("a knockout-only style");
    assert_eq!(style.knockout, Knockout::Deep);
    assert!(doc
        .layer(1)
        .unwrap()
        .reported_style()
        .unwrap()
        .contains("\"knockout\":\"deep\""));

    // D2's identity rule one depth up: the defaults are an identity and are
    // never stored.
    let doc = fixture(Some(
        "{\"knockout\":\"none\",\"blend_interior_as_group\":false}",
    ));
    assert!(doc.layer(1).unwrap().style().is_none(), "an identity style");

    // The stated DROP: a lone flag with nothing else non-default renders
    // nothing at either depth.
    let doc = fixture(Some("{\"blend_interior_as_group\":true}"));
    assert!(
        doc.layer(1).unwrap().style().is_none(),
        "a lone flag is not stored"
    );
}

// --------------------------------------------------------------- word 50 --

#[test]
fn word_50_sits_inside_46_only_when_a_style_carries_a_key_and_clearing_restores_the_prior_bytes() {
    let f = Fixture::new();
    // No key: the file is exactly what the 12E-1c writer made — 46 first, and
    // the next word is NOT 50.
    let plain = encode(&f.with(Some("{\"fill_opacity\":0.5}")));
    assert_eq!(identity::drdc_word(&plain, 0), identity::IDENTITY_TYPED);
    assert_ne!(
        identity::drdc_word(&plain, 1),
        KNOCKOUT_WORD,
        "no 50 without a key"
    );

    // With a key: 46 then 50, and nothing else moves.
    let knocked = encode(&f.with(Some("{\"fill_opacity\":0.5,\"knockout\":\"deep\"}")));
    assert_eq!(identity::drdc_word(&knocked, 0), identity::IDENTITY_TYPED);
    assert_eq!(
        identity::drdc_word(&knocked, 1),
        KNOCKOUT_WORD,
        "46 -> 50 -> the prior words"
    );
    // NO TABLE: the file still CLOSES with the identity tail, so word 50 added
    // a version word and nothing else.
    let tail = identity::identity_tail(2);
    assert_eq!(
        &knocked[knocked.len() - tail..knocked.len() - tail + 4],
        b"LIDS",
        "word 50 appends no tail of its own"
    );
    assert_eq!(identity::identity_records(&knocked), 2);
    // And the word is the ONLY thing the key added: the bytes with it removed
    // differ from the key-free file only inside the style JSON the `LSTY`
    // record already carried.
    assert_eq!(
        remove_word(&knocked, 1).len(),
        plain.len() + "\"knockout\":\"deep\",".len(),
        "word 50 adds four bytes and the JSON adds its own key"
    );

    // The GROUPED flag alone is not enough to keep a style, so pair it with a
    // fill opacity: the word still appears.
    let flagged = encode(&f.with(Some(
        "{\"fill_opacity\":0.5,\"blend_interior_as_group\":true}",
    )));
    assert_eq!(identity::drdc_word(&flagged, 1), KNOCKOUT_WORD);

    // Clearing the last key restores the 12E-1c writer's EXACT bytes.
    assert!(
        encode(&f.with(Some("{\"fill_opacity\":0.5}"))) == plain,
        "clearing the last key restores the prior bytes"
    );
}

#[test]
fn a_knockout_document_reopens_with_its_keys_and_resaves_byte_identically() {
    let doc = fixture(Some(
        "{\"knockout\":\"shallow\",\"blend_interior_as_group\":true,\"fill_opacity\":0.25}",
    ));
    let first = encode(&doc);
    let before = doc.layer(1).unwrap().reported_style();

    let reopened = open(&first).expect("reopen");
    assert_eq!(
        reopened.layer(1).unwrap().reported_style(),
        before,
        "both keys survive the file"
    );
    assert_eq!(encode(&reopened), first, "reopen -> resave is identical");
}

#[test]
fn a_hand_patched_typed_file_refuses_with_each_of_the_two_sentences() {
    // 50 with no such style anywhere — the word sits after 46 (and after any
    // 47/48 this document would carry, which a plain fixture does not).
    let plain = encode(&fixture(Some("{\"fill_opacity\":0.5}")));
    let forged = insert_word(&plain, 1, KNOCKOUT_WORD);
    let message = open(&forged).err().expect("50 with no key must refuse");
    assert!(
        message.ends_with("invalid native knockout word"),
        "{message}"
    );

    // Such a style with no 50.
    let knocked = encode(&fixture(Some("{\"knockout\":\"deep\"}")));
    assert_eq!(identity::drdc_word(&knocked, 1), KNOCKOUT_WORD);
    let stripped = remove_word(&knocked, 1);
    let message = open(&stripped).err().expect("a key with no 50 must refuse");
    assert!(
        message.ends_with("native knockout styles require DRDC 50"),
        "{message}"
    );
}

#[test]
fn a_style_free_file_carrying_word_50_refuses_too() {
    // The check runs ONCE and UNCONDITIONALLY, outside the optional `LSTY`
    // attach block — so a file with NO style table at all is checked as well.
    // Without that, `46 -> 50 -> …` over a style-free body would open, and
    // resave without the word.
    let plain = encode(&fixture(None));
    assert_ne!(identity::drdc_word(&plain, 1), KNOCKOUT_WORD);
    let forged = insert_word(&plain, 1, KNOCKOUT_WORD);
    let message = open(&forged)
        .err()
        .expect("50 over a style-free body must refuse");
    assert!(
        message.ends_with("invalid native knockout word"),
        "{message}"
    );
}

#[test]
fn a_misplaced_50_and_an_unknown_word_in_that_slot_refuse_by_name() {
    let knocked = encode(&fixture(Some("{\"knockout\":\"deep\"}")));

    // A BARE 50 — not nested inside 46 — is refused by name, which is also
    // what every older build does with the nested one.
    let mut bare = knocked[..4].to_vec();
    bare.extend_from_slice(&KNOCKOUT_WORD.to_le_bytes());
    bare.extend_from_slice(&knocked[8..]);
    let message = open(&bare).err().expect("a bare 50 must refuse");
    assert!(
        message.contains("not a supported version"),
        "a misplaced 50 lands on the terminal version sentence: {message}"
    );

    // The OLDER-READER proxy: an unknown word in the same slot reaches the
    // reader's terminal "not a supported version" sentence, which is exactly
    // what a build that predates 50 does with 50 itself.
    let mut unknown = knocked[..8].to_vec();
    unknown.extend_from_slice(&9999u32.to_le_bytes());
    unknown.extend_from_slice(&knocked[12..]);
    let message = open(&unknown).err().expect("an unknown word must refuse");
    assert!(
        message.contains("not a supported version"),
        "the terminal version sentence, which is what a pre-50 build says \
         about 50 itself: {message}"
    );
}

// ------------------------------------------------ the stored 48 file --

/// Byte equality that reports where it breaks: the first differing offset.
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
        "{what}: wrote {} bytes, the stored 48-writer file has {} bytes; first mismatch at offset {offset}",
        wrote.len(),
        stored.len()
    );
}

/// The top record's style through the app's own typed edit export.
fn restyle(document: &PrecisionDocument, style: &str) -> PrecisionDocument {
    let request = CString::new(format!(
        "{{\"op\":\"layer_style\",\"version\":1,\"layer\":1,\"style\":{style}}}"
    ))
    .unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    let result = unsafe {
        dr_precision_doc_edit(
            document,
            request.as_ptr(),
            ptr::null(),
            0,
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    if !err.is_null() {
        let message = unsafe { CStr::from_ptr(err) }
            .to_string_lossy()
            .into_owned();
        unsafe { dr_string_free(err) };
        panic!("{style}: {message}");
    }
    assert!(!result.is_null(), "{style}: the edit was a no-op");
    *unsafe { Box::from_raw(result) }
}

/// The key-free file is pinned against a file the 48 writer (78cb0ba)
/// wrote, not just against this build's own output.
#[test]
fn a_key_free_document_rewrites_the_stored_48_file_byte_for_byte() {
    const G: &[u8] = include_bytes!("fixtures/rz_typed_48/style_no_knockout.rz");

    // (a) The stored file is a 48-writer file: identity word 46 outermost, no
    // word 50, and this build refuses a 50 forced into it.
    assert_eq!(&G[..4], b"RZDC");
    assert_eq!(identity::drdc_word(G, 0), identity::IDENTITY_TYPED);
    assert_ne!(
        identity::drdc_word(G, 1),
        KNOCKOUT_WORD,
        "the 48 writer never wrote word 50"
    );
    let message = open(&insert_word(G, 1, KNOCKOUT_WORD))
        .err()
        .expect("50 with no key must refuse");
    assert!(
        message.ends_with("invalid native knockout word"),
        "{message}"
    );

    // (b) It opens and rewrites to itself.
    let stored = open(G).expect("the stored file opens");
    assert_same_bytes("reopen then resave", &encode(&stored), G);

    // (c) A knockout key turns word 50 on; clearing it writes the stored
    // bytes again.
    let knocked = restyle(&stored, "{\"fill_opacity\":0.5,\"knockout\":\"deep\"}");
    assert_eq!(
        identity::drdc_word(&encode(&knocked), 1),
        KNOCKOUT_WORD,
        "a knockout key writes word 50"
    );
    let cleared = restyle(&knocked, "{\"fill_opacity\":0.5}");
    assert_same_bytes("set then clear", &encode(&cleared), G);

    // (d) A freshly built document writes the same file up to its identity
    // wrapper: layer ids come from a process-global counter, so only (b) and
    // (c) compare the LIDS table.
    let fresh = encode(&fixture(Some("{\"fill_opacity\":0.5}")));
    assert_same_bytes(
        "a fresh document, without its identity wrapper",
        &identity::without_identity(&fresh, 2),
        &identity::without_identity(G, 2),
    );
}

// ---------------------------------------------------------- the PSD loss --

#[test]
fn the_16_bit_psd_export_writes_the_knockout_blocks_and_the_old_loss_sentence_is_gone() {
    let dir = tempfile::TempDir::new().unwrap();
    let report = |doc: &PrecisionDocument| -> String {
        let mut err: *mut c_char = ptr::null_mut();
        let json = unsafe { dr_precision_doc_psd_export_report(doc, 0, 0, &mut err) };
        assert!(!json.is_null(), "the 16-bit export report refused");
        let text = unsafe { CStr::from_ptr(json) }
            .to_string_lossy()
            .into_owned();
        unsafe { dr_string_free(json) };
        text
    };
    let write = |doc: &PrecisionDocument, path: &std::path::Path| {
        let projection = doc.render(LIMIT, 0, None).unwrap();
        let cpath = CString::new(path.to_str().unwrap()).unwrap();
        let mut err: *mut c_char = ptr::null_mut();
        assert!(
            unsafe {
                dr_precision_doc_save_psd(
                    doc,
                    &projection,
                    cpath.as_ptr(),
                    0,
                    0,
                    LIMIT,
                    0,
                    &mut err,
                )
            },
            "the 16-bit PSD write refused"
        );
    };
    let plain = fixture(Some("{\"fill_opacity\":0.5}"));
    let knocked = fixture(Some("{\"fill_opacity\":0.5,\"knockout\":\"deep\"}"));

    let text = report(&knocked);
    assert!(
        !text.contains("uses knockout or Blend Interior Effects as Group"),
        "the 12E-3 sentence is gone at 16 bits too: {text}"
    );
    assert!(
        text.contains("written as Photoshop's own knko record"),
        "the 16-bit export names the record it writes: {text}"
    );
    assert!(
        !report(&plain).contains("uses knockout"),
        "and only when a key is actually carried"
    );

    // The block IS in the 16-bit file now, in Photoshop's own framing.
    let a = dir.path().join("plain.psd");
    let b = dir.path().join("knocked.psd");
    write(&plain, &a);
    write(&knocked, &b);
    let plain_bytes = std::fs::read(&a).unwrap();
    let knocked_bytes = std::fs::read(&b).unwrap();
    assert!(!plain_bytes.windows(4).any(|w| w == b"knko"));
    let mut block = b"8BIMknko".to_vec();
    block.extend_from_slice(&4u32.to_be_bytes());
    block.extend_from_slice(&[2, 0, 0, 0]);
    assert!(
        knocked_bytes
            .windows(block.len())
            .any(|window| window == block),
        "the exact `knko` block for Deep at 16 bits"
    );
}
