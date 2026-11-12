//! Native files written before smart objects, Stack Mode, live filters and
//! non-RAW retained sources were removed (2026-10-10) still open, at both
//! depths, with every layer's stored pixels.
//!
//! The fixtures (`fixtures/rz_removed_features`, written by commit
//! `3e05aea6`; its README names each file's leading version words) hold one
//! `.rz` per removed feature and depth, and beside each the layers' stored
//! pixels as that build's own reader returned them: the oracle. A record
//! that carried a removed feature (contents, a Stack Mode, a live-filter
//! stack, masked or not) loses its binding and keeps its pixels, as
//! `rasterize` would leave it; a plain or linked placed record and a RAW
//! record with none of them keep their bindings (the app's open pass
//! rasterizes the non-RAW ones).

use std::ffi::{CStr, CString};
use std::path::PathBuf;
use std::ptr;

use doctored_core::doc::DrDocument;
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_doc::dr_doc_open;
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::REFERENCE_WORKING_BYTES;

const LIMIT: u64 = REFERENCE_WORKING_BYTES;

/// What the reader does with the fixture's one bound record.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Binding {
    /// The record carried a removed feature: no layer is bound.
    Dropped,
    /// A plain or linked placed record: still bound, no RAW original.
    Placed,
    /// A RAW record with nothing removed: still bound, with its original.
    Raw,
}

const FIXTURES: [(&str, Binding); 9] = [
    ("smart_object", Binding::Dropped),
    ("float_contents", Binding::Dropped),
    ("stack_mode", Binding::Dropped),
    ("live_filter_placed", Binding::Dropped),
    ("live_filter_masked", Binding::Dropped),
    ("live_filter_raw", Binding::Dropped),
    ("linked_placed", Binding::Placed),
    ("placed", Binding::Placed),
    ("raw", Binding::Raw),
];

fn path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/rz_removed_features")
        .join(name)
}

/// One oracle layer: dimensions, offset and the raw sample bytes.
struct Stored {
    size: (u32, u32),
    offset: (i32, i32),
    samples: Vec<u8>,
}

/// `<name>.pixels`: a u32 layer count, then per layer (bottom first) u32
/// width, u32 height, i32 x, i32 y and `bytes_per_pixel` bytes per pixel.
fn oracle(name: &str, bytes_per_pixel: usize) -> Vec<Stored> {
    let bytes = std::fs::read(path(&format!("{name}.pixels"))).unwrap();
    let word = |at: usize| u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
    let count = word(0);
    let mut at = 4;
    let mut layers = Vec::new();
    for _ in 0..count {
        let (w, h) = (word(at), word(at + 4));
        let (x, y) = (word(at + 8) as i32, word(at + 12) as i32);
        at += 16;
        let length = w as usize * h as usize * bytes_per_pixel;
        layers.push(Stored {
            size: (w, h),
            offset: (x, y),
            samples: bytes[at..at + length].to_vec(),
        });
        at += length;
    }
    assert_eq!(at, bytes.len(), "{name}: the oracle is read whole");
    layers
}

fn open8(name: &str) -> DrDocument {
    let text = CString::new(path(&format!("{name}.rz")).to_str().unwrap()).unwrap();
    let mut error = ptr::null_mut();
    let out = unsafe { dr_doc_open(text.as_ptr(), std::ptr::null(), &mut error) };
    if !error.is_null() {
        let message = unsafe { CStr::from_ptr(error) }
            .to_string_lossy()
            .into_owned();
        unsafe { dr_string_free(error) };
        panic!("{name}: {message}");
    }
    assert!(!out.is_null(), "{name}: opens");
    *unsafe { Box::from_raw(out) }
}

fn open16(name: &str) -> PrecisionDocument {
    let bytes = std::fs::read(path(&format!("{name}.rz"))).unwrap();
    PrecisionDocument::parse_native(&bytes, LIMIT, 0, None)
        .unwrap_or_else(|message| panic!("{name}: {message}"))
}

/// The indexes of the bound layers, and whether each holds a RAW original.
fn check(name: &str, bound: &[(usize, bool)], expected: Binding) {
    match expected {
        Binding::Dropped => assert!(bound.is_empty(), "{name}: bound layers {bound:?}"),
        Binding::Placed => {
            assert_eq!(bound.len(), 1, "{name}: one bound layer, {bound:?}");
            assert!(!bound[0].1, "{name}: a placed record holds no RAW original");
        }
        Binding::Raw => {
            assert_eq!(bound.len(), 1, "{name}: one bound layer, {bound:?}");
            assert!(bound[0].1, "{name}: the RAW original is kept");
        }
    }
}

#[test]
fn byte_files_open_with_their_stored_pixels() {
    for (feature, expected) in FIXTURES {
        let name = format!("{feature}_byte");
        let doc = open8(&name);
        let stored = oracle(&name, 4);
        assert_eq!(doc.layers.len(), stored.len(), "{name}: layer count");
        for (i, (layer, want)) in doc.layers.iter().zip(&stored).enumerate() {
            let image = layer.pixels.contiguous_image();
            assert_eq!(image.dimensions(), want.size, "{name} layer {i}: size");
            assert_eq!(layer.offset, want.offset, "{name} layer {i}: offset");
            assert!(
                image.as_raw() == &want.samples,
                "{name} layer {i}: stored pixels"
            );
        }
        let bound: Vec<(usize, bool)> = (0..doc.layers.len())
            .filter(|i| doc.has_retained_source(*i))
            .map(|i| (i, doc.source_info(i).unwrap()["raw"].is_object()))
            .collect();
        check(&name, &bound, expected);
    }
}

#[test]
fn typed_files_open_with_their_stored_pixels() {
    for (feature, expected) in FIXTURES {
        let name = format!("{feature}_typed");
        let doc = open16(&name);
        let stored = oracle(&name, 8);
        assert_eq!(doc.layer_count(), stored.len(), "{name}: layer count");
        for (i, want) in stored.iter().enumerate() {
            let image = doc.layer_source(i).unwrap();
            assert_eq!(image.dimensions(), want.size, "{name} layer {i}: size");
            assert_eq!(
                doc.layer(i).unwrap().offset(),
                want.offset,
                "{name} layer {i}: offset"
            );
            let samples: Vec<u8> = image
                .to_rgba16()
                .unwrap()
                .iter()
                .flat_map(|p| p.iter().flat_map(|v| v.to_le_bytes()))
                .collect();
            assert!(samples == want.samples, "{name} layer {i}: stored pixels");
        }
        let bound: Vec<(usize, bool)> = (0..doc.layer_count())
            .filter(|i| doc.has_retained_source(*i))
            .map(|i| (i, doc.source_info(i).unwrap()["raw"].is_object()))
            .collect();
        check(&name, &bound, expected);
    }
}

fn word(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap())
}

/// `bytes` with the u32 at `at` replaced.
fn set(bytes: &[u8], at: usize, value: u32) -> Vec<u8> {
    let mut out = bytes.to_vec();
    out[at..at + 4].copy_from_slice(&value.to_le_bytes());
    out
}

/// The offset of a table tag, which the fixture holds exactly once.
fn tag(bytes: &[u8], tag: &[u8; 4]) -> usize {
    let found: Vec<usize> = (0..bytes.len() - 3)
        .filter(|at| &bytes[*at..*at + 4] == tag)
        .collect();
    assert_eq!(found.len(), 1, "one {:?} tag", std::str::from_utf8(tag));
    found[0]
}

/// `bytes` without one of its leading version words (the README's column).
fn without_word(bytes: &[u8], value: u32) -> Vec<u8> {
    let at = (4..24)
        .step_by(4)
        .find(|at| word(bytes, *at) == value)
        .unwrap_or_else(|| panic!("leading word {value}"));
    [&bytes[..at], &bytes[at + 4..]].concat()
}

/// The reader's refusal of `bytes` at one depth; an accepted file fails.
fn refusal(dir: &std::path::Path, bytes: &[u8], typed: bool, case: &str) -> String {
    if typed {
        return match PrecisionDocument::parse_native(bytes, LIMIT, 0, None) {
            Ok(_) => panic!("typed, {case}: the file opened"),
            Err(message) => message,
        };
    }
    let file = dir.join("corrupt.rz");
    std::fs::write(&file, bytes).unwrap();
    let text = CString::new(file.to_str().unwrap()).unwrap();
    let mut error = ptr::null_mut();
    let out = unsafe { dr_doc_open(text.as_ptr(), std::ptr::null(), &mut error) };
    if !out.is_null() {
        drop(unsafe { Box::from_raw(out) });
        panic!("byte, {case}: the file opened");
    }
    assert!(!error.is_null(), "byte, {case}: a refusal has a message");
    let message = unsafe { CStr::from_ptr(error) }
        .to_string_lossy()
        .into_owned();
    unsafe { dr_string_free(error) };
    message
}

/// The legacy words and tables are skipped, not trusted: a file whose
/// contents, float-contents or Stack Mode word lacks its companion, or whose
/// `SOCT` / `SOSM` table breaks the framing the writer laid out, is refused by
/// name at both depths and never panics. Each case changes one word of a
/// fixture; the layouts are `native_sources::skip_contents_table`'s and
/// `skip_stack_mode_table`'s doc comments: `SOCT`, schema, count, then per
/// entry asset, kind and a u64 length before the nested bytes; `SOSM`,
/// schema, count, then per entry record and mode.
#[test]
fn corrupt_legacy_words_and_tables_refuse() {
    const WORD: &str = "invalid native smart object contents word";
    const KIND: &str = "unsupported smart object contents kind";
    let dir = tempfile::TempDir::new().unwrap();
    for typed in [false, true] {
        let depth = if typed { "typed" } else { "byte" };
        let read = |feature: &str| std::fs::read(path(&format!("{feature}_{depth}.rz"))).unwrap();
        // The contents, float-contents and Stack Mode words of this depth.
        let (contents, float, stack) = if typed { (66, 68, 70) } else { (65, 73, 69) };
        let stacked = read("stack_mode");
        let floated = read("float_contents");
        let (soct, sosm) = (tag(&stacked, b"SOCT"), tag(&stacked, b"SOSM"));
        let float_soct = tag(&floated, b"SOCT");
        assert!((4..24).step_by(4).any(|at| word(&stacked, at) == stack));
        assert_eq!(word(&floated, float_soct + 16), 3, "{depth}: a float entry");
        let cases: Vec<(&str, Vec<u8>, &str)> = vec![
            (
                "a Stack Mode word without the contents word",
                without_word(&stacked, contents),
                WORD,
            ),
            (
                "a float-contents word without the contents word",
                without_word(&floated, contents),
                WORD,
            ),
            (
                "a float-contents word over a table with no float entry",
                set(&floated, float_soct + 16, 2),
                WORD,
            ),
            (
                "a float entry without the float-contents word",
                without_word(&floated, float),
                KIND,
            ),
            (
                "a SOCT schema other than 1",
                set(&stacked, soct + 4, 2),
                "invalid smart object contents table tag or schema",
            ),
            (
                "a SOCT count of zero",
                set(&stacked, soct + 8, 0),
                "invalid smart object contents table count",
            ),
            (
                "a SOCT asset index past the asset table",
                set(&stacked, soct + 12, u32::MAX),
                "invalid or duplicate smart object contents asset",
            ),
            ("an unknown SOCT kind", set(&stacked, soct + 16, 9), KIND),
            (
                "a SOCT length past the end of the file",
                set(&stacked, soct + 24, 1),
                "smart object contents length is out of range",
            ),
            (
                "SOCT bytes that are not a document",
                set(&stacked, soct + 28, 0),
                "smart object contents are not a DRDC document",
            ),
            (
                "a SOCT alias to an asset with no entry",
                set(&stacked, soct + 16, 4),
                "invalid smart object contents alias",
            ),
            (
                "a SOSM schema other than 1",
                set(&stacked, sosm + 4, 2),
                "invalid stack mode table tag or schema",
            ),
            (
                "a SOSM count of zero",
                set(&stacked, sosm + 8, 0),
                "invalid stack mode table count",
            ),
            (
                "a SOSM record index past the records",
                set(&stacked, sosm + 12, u32::MAX),
                "invalid or duplicate stack mode record",
            ),
            (
                "a SOSM mode of zero",
                set(&stacked, sosm + 16, 0),
                "invalid stack mode code",
            ),
        ];
        for (case, bytes, expected) in cases {
            let message = refusal(dir.path(), &bytes, typed, case);
            assert!(
                message.contains(expected),
                "{depth}, {case}: refused with {message:?}, expected {expected:?}"
            );
        }
    }
}

/// A file that carried a removed feature saves again as an ordinary file the
/// reader takes back with the same pixels: nothing of the removed record is
/// written.
#[test]
fn a_reopened_file_saves_and_reopens_with_the_same_pixels() {
    let dir = tempfile::TempDir::new().unwrap();
    for (feature, _) in FIXTURES {
        let name = format!("{feature}_byte");
        let doc = open8(&name);
        let out = dir.path().join(format!("{name}.rz"));
        doc.save_native(out.to_str().unwrap()).unwrap();
        let text = CString::new(out.to_str().unwrap()).unwrap();
        let mut error = ptr::null_mut();
        let back = unsafe { dr_doc_open(text.as_ptr(), std::ptr::null(), &mut error) };
        assert!(error.is_null() && !back.is_null(), "{name}: reopens");
        let back = *unsafe { Box::from_raw(back) };
        assert_eq!(back.layers.len(), doc.layers.len(), "{name}");
        for (a, b) in doc.layers.iter().zip(&back.layers) {
            assert!(
                a.pixels.contiguous_image().as_raw() == b.pixels.contiguous_image().as_raw(),
                "{name}: resaved pixels"
            );
        }

        let name = format!("{feature}_typed");
        let doc = open16(&name);
        let bytes = doc.encode_native(LIMIT, 0, None).unwrap();
        let back = PrecisionDocument::parse_native(&bytes, LIMIT, 0, None)
            .unwrap_or_else(|message| panic!("{name}: {message}"));
        assert_eq!(back.layer_count(), doc.layer_count(), "{name}");
        for i in 0..doc.layer_count() {
            assert!(
                doc.layer_source(i).unwrap().to_rgba16().unwrap()
                    == back.layer_source(i).unwrap().to_rgba16().unwrap(),
                "{name}: resaved pixels"
            );
        }
    }
}
