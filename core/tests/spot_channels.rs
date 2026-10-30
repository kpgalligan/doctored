//! #547 spot channels, phase 1a-1 (`spot_channel`, `spot_overprint`,
//! `spot_channel_native`): the overprint kernel against hand-worked values
//! of its rule (Multiply at coverage, mixed toward the ink by Solidity); the
//! ink's range refusals; the byte add/set/duplicate/crop
//! and their refusals; the typed add on RGB 16, CMYK, Gray, Lab and Duotone
//! and its Indexed and Bitmap refusals by sentence; the conversions into
//! Indexed and Bitmap refused while a spot channel exists, every other
//! carrying the ink; and `.rz` word 112 with its `SPOT` table at both depths,
//! CHPR beside it, and the reader's refusals of cut, forged and bad files.

use doctored_core::doc::DrDocument;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_channel_edit::ChannelEdit;
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::PrecisionImage;
use doctored_core::precision_mask_edit::MaskEdit;
use doctored_core::precision_quantize::PrecisionDither;
use doctored_core::spot_channel::{
    SpotInk, BITMAP_REFUSAL, INDEXED_REFUSAL, NOT_SPOT_REFUSAL, TO_BITMAP_REFUSAL,
    TO_INDEXED_REFUSAL,
};
use doctored_core::spot_overprint::{overprint_rgba16, overprint_rgba8};
use image::{Rgba, RgbaImage};
use serde_json::{json, Value};
use tempfile::TempDir;

use crate::cmyk_fixture::press_profile;
use crate::duotone_document::{duotone, monotone_black};

const LIMIT: u64 = 2 * 1024 * 1024 * 1024;
const WORD: u32 = 112;

fn ink(lab: [f64; 3], solidity: u8) -> SpotInk {
    SpotInk::new(lab, solidity).unwrap()
}

fn word(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap())
}

fn write(dir: &TempDir, name: &str, bytes: &[u8]) -> String {
    let path = dir.path().join(name);
    std::fs::write(&path, bytes).unwrap();
    path.to_str().unwrap().to_string()
}

// ------------------------------------------------------------- overprint --

#[test]
fn the_overprint_matches_the_hand_worked_multiply() {
    // The rule: w = coverage / max; B = (1 − s)·cb·ink + s·ink; then the
    // W3C step, which over an opaque backdrop is c = (1 − w)·cb + w·B.
    // Solidity 0 at full coverage is the plain product, cb·ink:
    // R 200·0.2 = 40, G 100·0.6 = 60, B 50·1.0 = 50.
    let mut px = [200u8, 100, 50, 255];
    assert!(overprint_rgba8(&mut px, &[255], [0.2, 0.6, 1.0], 0.0));
    assert_eq!(px, [40, 60, 50, 255]);
    // Solidity 0.25 at full coverage: c = 0.75·cb·ink + 0.25·ink (×255).
    // R 0.75·40 + 0.25·51 = 42.75 → 43; G 0.75·60 + 0.25·153 = 83.25 → 83;
    // B 0.75·50 + 0.25·255 = 101.25 → 101.
    let mut quarter = [200u8, 100, 50, 255];
    assert!(overprint_rgba8(&mut quarter, &[255], [0.2, 0.6, 1.0], 0.25));
    assert_eq!(quarter, [43, 83, 101, 255]);
    // Coverage 51 (w = 0.2), solidity 0: c = 0.8·cb + 0.2·cb·ink.
    // R 160 + 8 = 168; G 80 + 12 = 92; B 40 + 10 = 50.
    let mut light = [200u8, 100, 50, 255];
    assert!(overprint_rgba8(&mut light, &[51], [0.2, 0.6, 1.0], 0.0));
    assert_eq!(light, [168, 92, 50, 255]);
    // White ink at solidity 100 over black at full coverage covers it.
    let mut black = [0u8, 0, 0, 255];
    assert!(overprint_rgba8(&mut black, &[255], [1.0, 1.0, 1.0], 1.0));
    assert_eq!(black, [255, 255, 255, 255]);
    // A transparent backdrop takes the ink at the coverage's alpha, at any
    // solidity: αo = 0.2 → 51, colour = cs → (51, 153, 255).
    let mut clear = [0u8, 0, 0, 0];
    assert!(overprint_rgba8(&mut clear, &[51], [0.2, 0.6, 1.0], 0.0));
    assert_eq!(clear, [51, 153, 255, 51]);
    // Coverage 0 leaves the pixel exactly as it was at any solidity.
    let mut untouched = [200u8, 100, 50, 255];
    for solidity in [0.0, 0.5, 1.0] {
        assert!(overprint_rgba8(
            &mut untouched,
            &[0],
            [0.2, 0.6, 1.0],
            solidity
        ));
    }
    assert_eq!(untouched, [200, 100, 50, 255]);
    // 16 bits at w = 1, solidity 0: the plain product, cb·ink.
    let mut wide = [40000u16, 20000, 10000, 65535];
    assert!(overprint_rgba16(&mut wide, &[65535], [0.5, 0.25, 1.0], 0.0));
    assert_eq!(wide, [20000, 5000, 10000, 65535]);
    // 16 bits at w = 1, solidity 1: the ink itself, 0.2·65535 = 13107.
    let mut covered = [40000u16, 20000, 10000, 65535];
    assert!(overprint_rgba16(
        &mut covered,
        &[65535],
        [0.2, 0.4, 1.0],
        1.0
    ));
    assert_eq!(covered, [13107, 26214, 65535, 65535]);
    // A length mismatch writes nothing.
    let mut two = [1u8, 2, 3, 4, 5, 6, 7, 8];
    assert!(!overprint_rgba8(&mut two, &[255], [0.0, 0.0, 0.0], 1.0));
    assert_eq!(two, [1, 2, 3, 4, 5, 6, 7, 8]);
    let mut one = [1u16, 2, 3, 4];
    assert!(!overprint_rgba16(&mut one, &[1, 2], [0.0, 0.0, 0.0], 1.0));
    assert_eq!(one, [1, 2, 3, 4]);
}

#[test]
fn an_ink_out_of_range_is_refused_by_sentence() {
    let lab =
        "the spot ink colour must be [L, a, b] with L from 0 to 100 and a and b from -128 to 127";
    for bad in [
        [100.5, 0.0, 0.0],
        [-0.1, 0.0, 0.0],
        [50.0, -128.5, 0.0],
        [50.0, 0.0, 127.5],
        [f64::NAN, 0.0, 0.0],
    ] {
        assert_eq!(SpotInk::new(bad, 50).unwrap_err(), lab, "{bad:?}");
    }
    assert_eq!(
        SpotInk::new([50.0, 0.0, 0.0], 101).unwrap_err(),
        "solidity must be from 0 to 100 %"
    );
    let ok = ink([0.0, -128.0, 127.0], 100);
    assert_eq!((ok.lab(), ok.solidity()), ([0.0, -128.0, 127.0], 100));
}

// ------------------------------------------------------------------ byte --

fn byte_doc() -> DrDocument {
    DrDocument::from_pixels(RgbaImage::from_fn(4, 3, |x, y| {
        Rgba([(x * 60) as u8, (y * 80) as u8, 120, 255])
    }))
}

fn coverage8(seed: u8) -> Vec<u8> {
    (0..12u8)
        .map(|i| i.wrapping_mul(21).wrapping_add(seed))
        .collect()
}

#[test]
fn the_byte_spot_channel_adds_sets_duplicates_and_crops() {
    let doc = byte_doc();
    let warm = ink([60.0, 40.0, 30.0], 70);
    let cool = ink([45.0, -20.0, -35.0], 25);
    assert!(
        doc.add_spot_channel("PANTONE", warm, &[0; 11]).is_none(),
        "a wrong length"
    );
    let spot = doc
        .add_spot_channel("PANTONE", warm, &coverage8(3))
        .unwrap();
    assert_eq!(spot.channels.len(), 1);
    assert_eq!(spot.channels[0].name, "PANTONE");
    assert_eq!(spot.channels[0].spot, Some(warm));
    assert_eq!(spot.channel_plane(0).unwrap(), coverage8(3).as_slice());
    assert!(doc.channels.is_empty(), "the input is never mutated");

    assert!(
        spot.set_spot_channel(1, None, cool).is_none(),
        "out of range"
    );
    assert!(
        spot.set_spot_channel(0, Some("PANTONE"), warm).is_none(),
        "nothing changes"
    );
    let set = spot.set_spot_channel(0, None, cool).unwrap();
    assert_eq!(
        (set.channels[0].name.as_str(), set.channels[0].spot),
        ("PANTONE", Some(cool))
    );
    let renamed = set.set_spot_channel(0, Some("Varnish"), cool).unwrap();
    assert_eq!(renamed.channels[0].name, "Varnish");

    let alpha = spot
        .add_channel("Alpha 1", &coverage8(9), 4, 3, [255, 0, 0], 0.5)
        .unwrap();
    assert!(alpha.channels[1].spot.is_none());
    assert!(
        alpha.set_spot_channel(1, None, cool).is_none(),
        "an alpha channel is not a spot channel"
    );

    let dup = alpha.duplicate_channel(0).unwrap();
    assert_eq!(dup.channels[1].name, "PANTONE copy");
    assert_eq!(dup.channels[1].spot, Some(warm));
    assert!(dup.channels[2].spot.is_none());

    let cropped = alpha.crop(1, 1, 2, 2).unwrap();
    assert_eq!(cropped.channels[0].spot, Some(warm));
    assert_eq!(cropped.channel_plane(0).unwrap().len(), 4);
    assert!(cropped.channels[1].spot.is_none());
}

// ----------------------------------------------------------------- typed --

fn rgb16(width: u32, height: u32) -> PrecisionDocument {
    let pixels = (0..width * height)
        .map(|i| [(i * 3000) as u16, 40000, (60000 - i * 2000) as u16, 65535])
        .collect();
    let image =
        PrecisionImage::from_rgba16(width, height, pixels, IccProfile::srgb(), None).unwrap();
    PrecisionDocument::from_image(image, Metadata::default(), Resolution::default()).unwrap()
}

fn rgb8(width: u32, height: u32) -> PrecisionDocument {
    let pixels = (0..width * height)
        .map(|i| [[250u16, 10, 10, 255], [10, 250, 10, 255]][(i % 2) as usize].map(|c| c * 257))
        .collect();
    let image =
        PrecisionImage::from_rgba16(width, height, pixels, IccProfile::srgb(), None).unwrap();
    PrecisionDocument::from_image(image, Metadata::default(), Resolution::default()).unwrap()
}

fn coverage16(doc: &PrecisionDocument, seed: u16) -> Vec<u16> {
    let (w, h) = doc.dimensions();
    (0..w * h)
        .map(|i| (i as u16).wrapping_mul(4099).wrapping_add(seed))
        .collect()
}

fn add(doc: &PrecisionDocument, name: &str, spot: SpotInk) -> PrecisionDocument {
    doc.add_spot_channel(name, spot, &coverage16(doc, 7), LIMIT, 0, None)
        .unwrap()
}

fn spot_of(doc: &PrecisionDocument, index: usize) -> Option<SpotInk> {
    doc.channels()[index].metadata().spot
}

#[test]
fn a_typed_spot_channel_adds_in_every_admitted_mode() {
    let warm = ink([60.0, 40.0, 30.0], 70);
    let rgb = rgb16(4, 2);
    let profile = press_profile("Press Test");
    let cmyk = rgb
        .convert_to_cmyk(Some(&profile), "{}", LIMIT, 0, None)
        .unwrap();
    let gray = rgb
        .convert_to_gray(None, r#"{"space":"gray_gamma_22"}"#, LIMIT, 0, None)
        .unwrap();
    let lab = rgb
        .convert_to_lab(r#"{"depth":16}"#, LIMIT, 0, None)
        .unwrap();
    let gray_ptr = Box::into_raw(Box::new(gray.clone()));
    let duo_ptr = duotone(gray_ptr, &monotone_black());
    let duo = unsafe { Box::from_raw(duo_ptr) };
    drop(unsafe { Box::from_raw(gray_ptr) });
    for (mode, doc) in [
        ("rgb", &rgb),
        ("cmyk", &cmyk),
        ("gray", &gray),
        ("lab", &lab),
        ("duotone", &*duo),
    ] {
        let out = add(doc, "PANTONE", warm);
        assert_eq!(out.channels().len(), 1, "{mode}");
        assert_eq!(out.channels()[0].name(), "PANTONE", "{mode}");
        assert_eq!(spot_of(&out, 0), Some(warm), "{mode}");
        assert_eq!(
            out.channels()[0].plane().samples(),
            coverage16(doc, 7).as_slice(),
            "{mode}"
        );
        assert!(doc.channels().is_empty(), "{mode}: the input is untouched");
    }
}

#[test]
fn the_typed_set_edits_only_a_spot_channel() {
    let warm = ink([60.0, 40.0, 30.0], 70);
    let cool = ink([45.0, -20.0, -35.0], 25);
    let doc = add(&rgb16(4, 2), "PANTONE", warm);
    assert!(doc
        .set_spot_channel(0, None, warm, LIMIT, 0)
        .unwrap()
        .is_none());
    let set = doc
        .set_spot_channel(0, Some("Varnish"), cool, LIMIT, 0)
        .unwrap()
        .unwrap();
    assert_eq!(
        (set.channels()[0].name(), spot_of(&set, 0)),
        ("Varnish", Some(cool))
    );
    assert_eq!(
        doc.set_spot_channel(1, None, cool, LIMIT, 0).err().unwrap(),
        "typed channel index is out of range"
    );
    let edit = MaskEdit::SetChannel {
        index: None,
        name: "Alpha 1".into(),
    };
    let alpha = doc
        .mask_edit(&edit, &coverage16(&doc, 1), LIMIT, 0, None)
        .unwrap()
        .unwrap();
    assert_eq!(spot_of(&alpha, 0), Some(warm), "an append keeps the ink");
    assert_eq!(
        alpha
            .set_spot_channel(1, None, cool, LIMIT, 0)
            .err()
            .unwrap(),
        NOT_SPOT_REFUSAL
    );
    let long = "n".repeat(70_000);
    assert_eq!(
        doc.set_spot_channel(0, Some(&long), cool, LIMIT, 0)
            .err()
            .unwrap(),
        "typed channel name exceeds native limit"
    );
}

fn indexed_doc() -> PrecisionDocument {
    rgb8(4, 2)
        .convert_to_indexed(r#"{"palette":"exact","dither":"none"}"#, LIMIT, 0, None)
        .unwrap()
}

fn gray8() -> *mut PrecisionDocument {
    crate::bitmap_mode::gray_doc(4, 2, &[0, 40, 80, 120, 160, 200, 240, 255])
}

#[test]
fn indexed_and_bitmap_refuse_a_spot_channel_by_name() {
    let warm = ink([60.0, 40.0, 30.0], 70);
    let indexed = indexed_doc();
    assert!(indexed.is_indexed());
    assert_eq!(
        indexed
            .add_spot_channel("PANTONE", warm, &coverage16(&indexed, 0), LIMIT, 0, None)
            .err()
            .unwrap(),
        INDEXED_REFUSAL
    );
    let gray = gray8();
    let bitmap_ptr = crate::bitmap_mode::to_bitmap(gray, &json!({"method":"threshold"})).unwrap();
    let bitmap = unsafe { Box::from_raw(bitmap_ptr) };
    assert!(bitmap.is_bitmap());
    assert_eq!(
        bitmap
            .add_spot_channel("PANTONE", warm, &coverage16(&bitmap, 0), LIMIT, 0, None)
            .err()
            .unwrap(),
        BITMAP_REFUSAL
    );

    // Converting INTO either mode refuses while a spot channel exists, in
    // the plan and the conversion alike.
    let options = r#"{"palette":"exact","dither":"none"}"#;
    let spotted = add(&rgb8(4, 2), "PANTONE", warm);
    let plan: Value = spotted.convert_to_indexed_plan(options).unwrap();
    assert_eq!(plan["refusal"], TO_INDEXED_REFUSAL);
    assert_eq!(
        spotted
            .convert_to_indexed(options, LIMIT, 0, None)
            .err()
            .unwrap(),
        TO_INDEXED_REFUSAL
    );
    let gray_spotted = add(unsafe { &*gray }, "PANTONE", warm);
    let plan: Value = gray_spotted.convert_to_bitmap_plan("{}").unwrap();
    assert_eq!(plan["refusal"], TO_BITMAP_REFUSAL);
    assert_eq!(
        gray_spotted
            .convert_to_bitmap("{}", LIMIT, 0, None)
            .err()
            .unwrap(),
        TO_BITMAP_REFUSAL
    );
    drop(unsafe { Box::from_raw(gray) });
}

#[test]
fn every_other_conversion_carries_the_ink() {
    let warm = ink([60.0, 40.0, 30.0], 70);
    let byte = byte_doc()
        .add_spot_channel("PANTONE", warm, &coverage8(5))
        .unwrap();
    let typed = byte.promote_structure(LIMIT, 0, None).unwrap();
    assert_eq!(spot_of(&typed, 0), Some(warm));
    assert_eq!(typed.channels()[0].name(), "PANTONE");
    let back = typed
        .narrow_structure_to_u8(PrecisionDither::None, LIMIT, 0, None)
        .unwrap();
    assert_eq!(back.channels[0].spot, Some(warm));
    assert_eq!(back.channel_plane(0).unwrap(), coverage8(5).as_slice());

    let rgb = add(&rgb16(4, 2), "PANTONE", warm);
    let profile = press_profile("Press Test");
    let cmyk = rgb
        .convert_to_cmyk(Some(&profile), "{}", LIMIT, 0, None)
        .unwrap();
    assert_eq!(spot_of(&cmyk, 0), Some(warm));
    let gray = rgb
        .convert_to_gray(None, r#"{"space":"gray_gamma_22"}"#, LIMIT, 0, None)
        .unwrap();
    assert_eq!(spot_of(&gray, 0), Some(warm));
}

// -------------------------------------------------------------------- .rz --

fn byte_save(dir: &TempDir, name: &str, doc: &DrDocument) -> Vec<u8> {
    let path = dir.path().join(name);
    doc.save_native(path.to_str().unwrap()).unwrap();
    std::fs::read(path).unwrap()
}

fn typed_save(dir: &TempDir, name: &str, doc: &PrecisionDocument) -> Vec<u8> {
    let path = dir.path().join(name);
    doc.save_native(path.to_str().unwrap(), LIMIT, 0, None)
        .unwrap();
    std::fs::read(path).unwrap()
}

fn typed_open(dir: &TempDir, name: &str, bytes: &[u8]) -> Result<PrecisionDocument, String> {
    PrecisionDocument::open_native(&write(dir, name, bytes), LIMIT, 0, None)
}

/// The offset of the `SPOT` tag: the last occurrence in the file.
fn spot_table(bytes: &[u8]) -> usize {
    bytes.windows(4).rposition(|w| w == b"SPOT").unwrap()
}

/// A byte document with an alpha channel between two spot channels.
fn byte_spots() -> DrDocument {
    byte_doc()
        .add_spot_channel("Warm", ink([60.0, 40.0, 30.0], 70), &coverage8(1))
        .unwrap()
        .add_channel("Alpha", &coverage8(2), 4, 3, [0, 0, 255], 0.25)
        .unwrap()
        .add_spot_channel("Cool", ink([45.5, -20.25, -35.0], 0), &coverage8(3))
        .unwrap()
}

#[test]
fn a_byte_file_writes_word_112_first_and_round_trips() {
    let dir = TempDir::new().unwrap();
    let plain = byte_doc()
        .add_channel("Alpha", &coverage8(2), 4, 3, [0, 0, 255], 0.25)
        .unwrap();
    let legacy = byte_save(&dir, "legacy.rz", &plain);
    assert_eq!(word(&legacy, 4), 45);
    assert_ne!(word(&legacy, 8), WORD, "no spot channel writes no word");
    assert!(!legacy.windows(4).any(|w| w == b"SPOT"));

    let doc = byte_spots();
    let bytes = byte_save(&dir, "spot.rz", &doc);
    assert_eq!(word(&bytes, 4), 45);
    assert_eq!(word(&bytes, 8), WORD, "the first word inside 45");
    assert_eq!(word(&bytes, 12), word(&legacy, 8), "then the prior words");
    let back = DrDocument::open(&write(&dir, "again.rz", &bytes)).unwrap();
    assert_eq!(back.channels.len(), 3);
    for i in 0..3 {
        assert_eq!(back.channels[i].name, doc.channels[i].name);
        assert_eq!(back.channels[i].spot, doc.channels[i].spot);
        assert_eq!(back.channel_plane(i), doc.channel_plane(i));
    }
    assert_eq!(back.channels[1].overlay_color, [0, 0, 255]);

    let mut cut = bytes.clone();
    cut.drain(8..12);
    assert!(DrDocument::open(&write(&dir, "cut.rz", &cut)).is_err());
    let mut forged = legacy.clone();
    forged.splice(8..8, WORD.to_le_bytes());
    let message = DrDocument::open(&write(&dir, "forged.rz", &forged))
        .err()
        .unwrap();
    assert!(
        message.contains("unsupported native spot channel schema"),
        "{message}"
    );
}

#[test]
fn a_bad_spot_table_is_refused_by_name() {
    let dir = TempDir::new().unwrap();
    let bytes = byte_save(&dir, "spot.rz", &byte_spots());
    let at = spot_table(&bytes);
    // Layout: tag, schema, count, then (index, L, a, b, solidity) × count.
    let first = at + 12;
    let second = first + 29;
    // (sentence, offset, replacement bytes)
    let cases: [(&str, usize, Vec<u8>); 6] = [
        (
            "unsupported native spot channel schema",
            at + 4,
            2u32.to_le_bytes().to_vec(),
        ),
        (
            "invalid native spot channel count",
            at + 8,
            0u32.to_le_bytes().to_vec(),
        ),
        (
            "invalid native spot channel order or index",
            second,
            0u32.to_le_bytes().to_vec(),
        ),
        (
            "invalid native spot channel order or index",
            second,
            3u32.to_le_bytes().to_vec(),
        ),
        ("invalid native spot channel ink", first + 28, vec![101]),
        (
            "invalid native spot channel ink",
            first + 4,
            100.5f64.to_le_bytes().to_vec(),
        ),
    ];
    for (n, (sentence, offset, patch)) in cases.iter().enumerate() {
        let mut bad = bytes.clone();
        bad[*offset..*offset + patch.len()].copy_from_slice(patch);
        let message = DrDocument::open(&write(&dir, &format!("bad{n}.rz"), &bad))
            .err()
            .unwrap();
        assert!(message.contains(sentence), "{n}: {message}");
    }
}

#[test]
fn a_typed_file_writes_word_112_after_46_and_round_trips_beside_chpr() {
    let dir = TempDir::new().unwrap();
    let warm = ink([60.0, 40.0, 30.0], 70);
    let base = rgb16(4, 2);
    let edit = MaskEdit::SetChannel {
        index: None,
        name: "Alpha".into(),
    };
    let alpha = base
        .mask_edit(&edit, &coverage16(&base, 3), LIMIT, 0, None)
        .unwrap()
        .unwrap();
    let legacy = typed_save(&dir, "legacy.rz", &alpha);
    assert_eq!(word(&legacy, 4), 46);
    assert_ne!(word(&legacy, 8), WORD, "no spot channel writes no word");

    // Channel 0 gets a non-default overlay, so CHPR is written too.
    let props = ChannelEdit::ChannelProperties {
        index: 0,
        name: None,
        overlay_color: Some([0, 200, 40]),
        overlay_opacity: Some(0.75),
        color_indicates_selected: None,
        provenance: None,
    };
    let coloured = alpha
        .channel_edit(&props, &[], LIMIT, 0, None)
        .unwrap()
        .unwrap();
    let doc = add(&coloured, "PANTONE", warm);
    let bytes = typed_save(&dir, "spot.rz", &doc);
    assert_eq!(word(&bytes, 4), 46);
    assert_eq!(word(&bytes, 8), WORD, "right after 46");
    let back = typed_open(&dir, "again.rz", &bytes).unwrap();
    assert_eq!(back.channels().len(), 2);
    assert_eq!(spot_of(&back, 0), None);
    assert_eq!(back.channels()[0].metadata().overlay_color, [0, 200, 40]);
    assert_eq!(back.channels()[0].metadata().overlay_opacity, 0.75);
    assert_eq!(spot_of(&back, 1), Some(warm));
    assert_eq!(back.channels()[1].name(), "PANTONE");
    assert_eq!(
        back.channels()[1].plane().samples(),
        doc.channels()[1].plane().samples()
    );
    // A spot-only typed document writes no CHPR (its ink is SPOT's).
    let only = add(&base, "PANTONE", warm);
    let only_bytes = typed_save(&dir, "only.rz", &only);
    assert!(!only_bytes.windows(4).any(|w| w == b"CHPR"));
    let back = typed_open(&dir, "only-again.rz", &only_bytes).unwrap();
    assert_eq!(spot_of(&back, 0), Some(warm));

    let mut cut = bytes.clone();
    cut.drain(8..12);
    assert!(typed_open(&dir, "cut.rz", &cut).is_err());
    let mut forged = legacy.clone();
    forged.splice(8..8, WORD.to_le_bytes());
    let message = typed_open(&dir, "forged.rz", &forged).err().unwrap();
    assert!(
        message.contains("unsupported native spot channel schema"),
        "{message}"
    );
}

#[test]
fn a_cmyk_file_writes_word_112_after_its_rung() {
    let dir = TempDir::new().unwrap();
    let warm = ink([60.0, 40.0, 30.0], 70);
    let profile = press_profile("Press Test");
    let cmyk = rgb16(4, 2)
        .convert_to_cmyk(Some(&profile), "{}", LIMIT, 0, None)
        .unwrap();
    let doc = add(&cmyk, "PANTONE", warm);
    let bytes = typed_save(&dir, "cmyk.rz", &doc);
    assert!((82..=88).contains(&word(&bytes, 8)), "the CMYK rung");
    assert_eq!(word(&bytes, 12), WORD, "112 after the rung");
    let back = typed_open(&dir, "cmyk-again.rz", &bytes).unwrap();
    assert!(back.is_cmyk());
    assert_eq!(spot_of(&back, 0), Some(warm));
    assert_eq!(
        back.channels()[0].plane().samples(),
        doc.channels()[0].plane().samples()
    );
}
