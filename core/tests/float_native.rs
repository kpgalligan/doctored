//! Phase 11F (N): the `.rz` float wrapper.
//!
//! What is pinned: a float document writes 40 as its FIRST word and 39 as its
//! SECOND, closes with one `FLOT` table, and reopens BIT-EXACT with the same
//! tone map, the same retained accounting and the same refusal wording; a 40
//! word followed by anything else, a storage tag that disagrees with the
//! wrapper, a bad schema/operator/white and a trailing word all refuse before
//! a pixel is decoded; and a u16 document's bytes are exactly the prior
//! writer's.

use doctored_core::doc_evaluation::Evaluation;
use doctored_core::float_document::ToneMap;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::{PrecisionImage, PrecisionStorage, REFERENCE_WORKING_BYTES};
use tempfile::TempDir;

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
const FIXTURE: &str = "tests/fixtures/float_11f/scene.tiff";

fn fixture_document() -> PrecisionDocument {
    PrecisionDocument::open_preserving(FIXTURE, LIMIT, 0, None).expect("the float fixture opens")
}

fn word(bytes: &[u8], index: usize) -> u32 {
    let at = 4 + index * 4;
    u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
}

/// Phase 12E-1's always-written identity wrapper: 46 is the OUTERMOST typed
/// word and its `LIDS` table closes every file, so the float word sits one
/// slot in and `FLOT` is located by its tag.
const IDENTITY_TYPED: u32 = 46;

fn flot_at(bytes: &[u8]) -> usize {
    bytes.windows(4).rposition(|w| w == b"FLOT").expect("FLOT")
}

fn u16_document() -> PrecisionDocument {
    let image = PrecisionImage::from_rgba16(
        3,
        2,
        (0..6)
            .map(|i| [1000 + i * 7, 2000 + i * 3, 3000 + i, 65535])
            .collect(),
        IccProfile::srgb(),
        None,
    )
    .unwrap();
    PrecisionDocument::from_image(image, Metadata::default(), Resolution::default()).unwrap()
}

#[test]
fn a_float_document_writes_forty_then_thirty_nine_and_reopens_bit_exact() {
    let document = fixture_document();
    let bytes = document.encode_native(LIMIT, 0, None).unwrap();
    assert_eq!(&bytes[..4], b"DRDC");
    assert_eq!(word(&bytes, 0), IDENTITY_TYPED);
    assert_eq!(word(&bytes, 1), 40);
    assert_eq!(word(&bytes, 2), 39);
    // The body header's storage tag is 2, and FLOT precedes the identity
    // table that now closes the file.
    assert_eq!(word(&bytes, 4), 2);
    assert_eq!(&bytes[flot_at(&bytes)..flot_at(&bytes) + 4], b"FLOT");

    let reopened = PrecisionDocument::parse_native(&bytes, LIMIT, 0, None).unwrap();
    assert!(reopened.is_float());
    assert_eq!(reopened.evaluation(), Evaluation::LinearLight);
    assert_eq!(reopened.tone_map(), Some(ToneMap::Clip));
    let before = document.layer_source(0).unwrap().to_rgba32().ok().unwrap();
    let after = reopened.layer_source(0).unwrap().to_rgba32().ok().unwrap();
    assert_eq!(before.len(), after.len());
    for (i, (a, b)) in before.iter().zip(after.iter()).enumerate() {
        for c in 0..4 {
            assert_eq!(a[c].to_bits(), b[c].to_bits(), "sample {i}:{c}");
        }
    }
    // Parity with the freshly opened TIFF, which is what "reopen is the same
    // document" has to mean for admission and for the host's guards.
    assert_eq!(reopened.retained_bytes(), document.retained_bytes());
    assert_eq!(reopened.float_refusal(), document.float_refusal());
    assert_eq!(reopened.dimensions(), document.dimensions());
    assert_eq!(
        reopened.profile().bytes(),
        document.profile().bytes(),
        "the embedded profile survives"
    );
    // And writing it again is byte-identical.
    assert_eq!(reopened.encode_native(LIMIT, 0, None).unwrap(), bytes);
}

#[test]
fn a_reinhard_map_round_trips_through_the_flot_table() {
    let document = fixture_document()
        .with_tone_map(ToneMap::Reinhard { white: 2.5 })
        .unwrap()
        .unwrap();
    let bytes = document.encode_native(LIMIT, 0, None).unwrap();
    let reopened = PrecisionDocument::parse_native(&bytes, LIMIT, 0, None).unwrap();
    assert_eq!(reopened.tone_map(), Some(ToneMap::Reinhard { white: 2.5 }));
}

#[test]
fn a_forty_word_refuses_everything_but_a_thirty_nine_behind_it() {
    let mut bytes = fixture_document().encode_native(LIMIT, 0, None).unwrap();
    bytes[12..16].copy_from_slice(&36u32.to_le_bytes());
    let error = PrecisionDocument::parse_native(&bytes, LIMIT, 0, None)
        .err()
        .expect("a float file must declare linear-light evaluation");
    assert!(error.contains("linear-light evaluation"), "{error}");
}

#[test]
fn a_float_storage_tag_is_admitted_only_under_forty() {
    // Storage tag 2 with the word 40 removed: the reader must not accept the
    // raw f32 payload under an ordinary typed version.
    let bytes = fixture_document().encode_native(LIMIT, 0, None).unwrap();
    // The 40 word alone is removed; the identity wrapper stays where the
    // writer puts it.
    let mut stripped = Vec::new();
    stripped.extend_from_slice(&bytes[..8]);
    stripped.extend_from_slice(&bytes[12..]);
    assert!(PrecisionDocument::parse_native(&stripped, LIMIT, 0, None).is_err());
    // And a float file that claims tag 1 refuses too.
    let mut retagged = bytes.clone();
    retagged[20..24].copy_from_slice(&1u32.to_le_bytes());
    let error = PrecisionDocument::parse_native(&retagged, LIMIT, 0, None)
        .err()
        .expect("tag 1 under 40 refuses");
    assert!(error.contains("storage tag"), "{error}");
}

#[test]
fn a_bad_flot_table_or_a_trailing_word_refuses() {
    let bytes = fixture_document().encode_native(LIMIT, 0, None).unwrap();
    let table = flot_at(&bytes);
    let mut schema = bytes.clone();
    schema[table + 4..table + 8].copy_from_slice(&2u32.to_le_bytes());
    assert!(PrecisionDocument::parse_native(&schema, LIMIT, 0, None)
        .err()
        .expect("schema 2 refuses")
        .contains("float schema"));
    let mut operator = bytes.clone();
    operator[table + 8..table + 12].copy_from_slice(&7u32.to_le_bytes());
    assert!(PrecisionDocument::parse_native(&operator, LIMIT, 0, None)
        .err()
        .expect("operator 7 refuses")
        .contains("tone map operator"));
    let mut white = bytes.clone();
    white[table + 8..table + 12].copy_from_slice(&1u32.to_le_bytes());
    white[table + 12..table + 16].copy_from_slice(&0.5f32.to_le_bytes());
    assert!(PrecisionDocument::parse_native(&white, LIMIT, 0, None)
        .err()
        .expect("a white point below one refuses")
        .contains("white point"));
    let mut trailing = bytes.clone();
    trailing.extend_from_slice(&1u32.to_le_bytes());
    assert!(PrecisionDocument::parse_native(&trailing, LIMIT, 0, None).is_err());
    let mut truncated = bytes.clone();
    truncated.truncate(truncated.len() - 4);
    assert!(PrecisionDocument::parse_native(&truncated, LIMIT, 0, None).is_err());
}

#[test]
fn a_u16_documents_native_bytes_are_unchanged_by_this_phase() {
    let document = u16_document();
    let bytes = document.encode_native(LIMIT, 0, None).unwrap();
    // No 40 word, no float tag, and the prior writer's body word behind the
    // always-written identity wrapper.
    assert_eq!(word(&bytes, 0), IDENTITY_TYPED);
    assert_eq!(word(&bytes, 1), 13);
    assert_eq!(word(&bytes, 2), 1);
    assert!(!bytes.windows(4).any(|w| w == b"FLOT"));
    let reopened = PrecisionDocument::parse_native(&bytes, LIMIT, 0, None).unwrap();
    assert!(!reopened.is_float());
    assert_eq!(reopened.evaluation(), Evaluation::Encoded);
    assert_eq!(
        reopened.layer_source(0).unwrap().to_rgba16().ok(),
        document.layer_source(0).unwrap().to_rgba16().ok()
    );
    assert_eq!(reopened.encode_native(LIMIT, 0, None).unwrap(), bytes);
}

#[test]
fn a_float_document_saves_and_opens_through_the_file_path() {
    let directory = TempDir::new().unwrap();
    let path = directory.path().join("scene.rz");
    let document = fixture_document();
    document
        .save_native(path.to_str().unwrap(), LIMIT, 0, None)
        .unwrap();
    let reopened =
        PrecisionDocument::open_preserving(path.to_str().unwrap(), LIMIT, 0, None).unwrap();
    assert!(reopened.is_float());
    assert_eq!(
        reopened.layer_source(0).unwrap().storage(),
        PrecisionStorage::Rgba32Float
    );
    let before = document.layer_source(0).unwrap().to_rgba32().ok().unwrap();
    let after = reopened.layer_source(0).unwrap().to_rgba32().ok().unwrap();
    for (a, b) in before.iter().zip(after.iter()) {
        assert_eq!(a, b);
    }
    // The native plan charges the raw payload, not a PNG bound.
    let plan = reopened.native_plan().unwrap();
    assert!(plan.serialized_pixel_bytes >= 48 * 32 * 16);
}

/// The float `.rz` reopen and the float TIFF open decode
/// straight into tiles (a contiguous plane shares no tile with itself).
#[test]
fn a_reopened_float_raster_is_tiled() {
    let document = fixture_document();
    let opened = document.layer_source(0).unwrap();
    let (width, height) = opened.dimensions();
    let tiles = (width.div_ceil(256) * height.div_ceil(256)) as usize;
    assert_eq!(opened.shared_tile_count(opened), tiles, "the TIFF open");
    let bytes = document.encode_native(LIMIT, 0, None).unwrap();
    let reopened = PrecisionDocument::parse_native(&bytes, LIMIT, 0, None).unwrap();
    let image = reopened.layer_source(0).unwrap();
    assert_eq!(image.dimensions(), (width, height));
    assert_eq!(image.shared_tile_count(image), tiles, "the reopen");
}

/// The tiled reopen still checks every sample against the storage
/// invariant: a NaN written over sample 0's red refuses.
#[test]
fn a_reopened_raster_outside_the_stored_range_refuses() {
    let document = fixture_document();
    let first = document.layer_source(0).unwrap().sample_at(0, 0).unwrap();
    let pattern: Vec<u8> = first.iter().flat_map(|v| v.to_le_bytes()).collect();
    let mut bytes = document.encode_native(LIMIT, 0, None).unwrap();
    let at = bytes
        .windows(16)
        .position(|w| w == pattern.as_slice())
        .expect("sample 0's bytes are in the file");
    bytes[at..at + 4].copy_from_slice(&f32::NAN.to_le_bytes());
    let error = PrecisionDocument::parse_native(&bytes, LIMIT, 0, None)
        .err()
        .expect("a NaN sample refuses");
    assert!(
        error.contains("32-bit float samples must be finite within ±65504"),
        "{error}"
    );
}
