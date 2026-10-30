//! The typed `.rz` reader streams a file record by record and the
//! writer streams to its temporary file (`native_stream`). On small files,
//! the streamed reader (`open_native`) and the slice reader (`parse_native`)
//! must agree on every file the writer produces, on every truncation of it
//! and on corrupted bytes, and the streamed reader must refuse a file that
//! shrinks under it and ignore bytes appended after the open. A save and a
//! reopen admit at the plan's figure, which is below the retained bytes plus
//! the file length the former whole-file reader and writer needed.
//!
//! Oracles: the saved file itself (a reopen re-encodes to its exact bytes),
//! the slice reader's answer for the same bytes, and spans located in the
//! file by this test's own search for the bytes it wrote (PNG signatures,
//! the mask and channel samples, the f32 samples).

use std::io::Write;

use doctored_core::doc::DrDocument;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use doctored_core::precision_mask_edit::MaskEdit;
use doctored_core::precision_native::open_native_observed;
use doctored_core::retained_source::{SourceEdit, SourceInput};
use image::{Rgba, RgbaImage};
use serde_json::{json, Value};
use tempfile::TempDir;

use crate::float_fixture;

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
const SIGNATURE: &[u8] = b"\x89PNG\r\n\x1a\n";

/// xorshift64 noise in all four channels.
fn noise(w: u32, h: u32, seed: u64) -> Vec<[u16; 4]> {
    let mut state = seed;
    (0..w as usize * h as usize)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            [
                state as u16,
                (state >> 16) as u16,
                (state >> 32) as u16,
                (state >> 48) as u16,
            ]
        })
        .collect()
}

fn image16(w: u32, h: u32, seed: u64) -> PrecisionDocument {
    PrecisionDocument::from_image(
        PrecisionImage::from_rgba16(w, h, noise(w, h, seed), IccProfile::srgb(), None).unwrap(),
        Metadata::default(),
        Resolution { x: 72., y: 72. },
    )
    .unwrap()
}

fn mask_edit(doc: &PrecisionDocument, request: Value, samples: &[u16]) -> PrecisionDocument {
    doc.mask_edit(
        &MaskEdit::parse(&request.to_string()).unwrap(),
        samples,
        LIMIT,
        0,
        None,
    )
    .unwrap()
    .unwrap()
}

/// Places `input` at `(x, 1)`; a byte or 16-bit original converts its
/// fallback to the document's depth, a float master into a float document
/// needs no conversion.
fn place(doc: &PrecisionDocument, input: SourceInput<'_>, id: &str, x: i32) -> PrecisionDocument {
    let mut request = json!({"op":"place","source_id":id,"name":id,"x":x,"y":1});
    if !doc.is_float() {
        request["depth_conversion"] = json!("convert_fallback");
    }
    doc.source_edit_input(
        Some(input),
        &SourceEdit::parse(&request.to_string()).unwrap(),
        LIMIT,
        0,
        None,
    )
    .unwrap()
    .unwrap()
}

const W: u32 = 24;
const H: u32 = 16;

/// The mask's samples: a ramp no other plane holds.
fn mask_samples() -> Vec<u16> {
    (0..W * H).map(|i| (i * 21 + 7) as u16).collect()
}

/// The channel's samples.
fn channel_samples() -> Vec<u16> {
    (0..W * H).map(|i| 65_535 - (i * 13) as u16).collect()
}

/// Two noise rasters, a u16 mask on the upper one, a saved channel, a placed
/// 16-bit original and a placed 8-bit original.
fn doc16() -> PrecisionDocument {
    let two = image16(W, H, 0x1234_5678_9ABC_DEF1)
        .duplicate_layer(0)
        .unwrap()
        .unwrap();
    let masked = mask_edit(
        &two,
        json!({"op":"set_mask","layer":1,"width":W,"height":H,"x":0,"linked":false,
            "density":1.0,"feather":0.0,"enabled":true}),
        &mask_samples(),
    );
    let channeled = mask_edit(
        &masked,
        json!({"op":"set_channel","index":null,"name":"Coverage"}),
        &channel_samples(),
    );
    let original16 = image16(12, 10, 0x0F1E_2D3C_4B5A_6978);
    let placed = place(&channeled, SourceInput::Typed(&original16), "o16", 2);
    let original8 = DrDocument::from_pixels(RgbaImage::from_fn(10, 8, |x, y| {
        Rgba([(x * 23) as u8, (y * 31) as u8, (x * y * 7) as u8, 255])
    }));
    place(&placed, SourceInput::Byte(&original8), "o8", 5)
}

/// The float raster's samples.
fn float_samples(w: u32, h: u32, k: f32) -> Vec<[f32; 4]> {
    (0..w * h)
        .map(|i| {
            let (x, y) = ((i % w) as f32, (i / w) as f32);
            [x / w as f32 * k, -0.25 + y / h as f32, 1.5 + k, 1.0]
        })
        .collect()
}

/// A float raster and a placed f32 master.
fn doc32() -> PrecisionDocument {
    let srgb = IccProfile::srgb();
    let base = float_fixture::float_doc(W, H, &srgb, &[float_samples(W, H, 1.0)]);
    let master = float_fixture::float_doc(9, 7, &srgb, &[float_samples(9, 7, 2.5)]);
    place(&base, SourceInput::Typed(&master), "f32", 3)
}

/// Saves `doc` through `save_native` and answers the file's bytes.
fn saved(doc: &PrecisionDocument, dir: &TempDir, name: &str) -> (String, Vec<u8>) {
    let path = dir.path().join(name);
    let path = path.to_str().unwrap().to_string();
    doc.save_native(&path, LIMIT, 0, None).expect("the save");
    let bytes = std::fs::read(&path).unwrap();
    (path, bytes)
}

fn open(path: &str) -> Result<PrecisionDocument, String> {
    PrecisionDocument::open_native(path, LIMIT, 0, None)
}

fn parse(bytes: &[u8]) -> Result<PrecisionDocument, String> {
    PrecisionDocument::parse_native(bytes, LIMIT, 0, None)
}

fn encoded(doc: &PrecisionDocument) -> Vec<u8> {
    doc.encode_native(LIMIT, 0, None).expect("the re-encode")
}

fn find(hay: &[u8], needle: &[u8]) -> usize {
    hay.windows(needle.len())
        .position(|w| w == needle)
        .expect("the bytes are in the file")
}

/// Every non-overlapping occurrence of `needle` in `hay`.
fn find_all(hay: &[u8], needle: &[u8]) -> Vec<usize> {
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(at) = hay[from..].windows(needle.len()).position(|w| w == needle) {
        out.push(from + at);
        from += at + needle.len();
    }
    out
}

fn u16_bytes(samples: &[u16]) -> Vec<u8> {
    samples.iter().flat_map(|v| v.to_le_bytes()).collect()
}

fn f32_bytes(samples: &[[f32; 4]]) -> Vec<u8> {
    samples
        .iter()
        .flatten()
        .flat_map(|v| v.to_le_bytes())
        .collect()
}

/// A span located in a file: `(start, end, is a PNG)`.
type Span = (usize, usize, bool);

/// Every span in the file: each stored PNG (from its signature, the length
/// word before it), and every occurrence of the given raw bodies.
fn spans(bytes: &[u8], bodies: &[Vec<u8>]) -> Vec<Span> {
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(at) = bytes[from..]
        .windows(SIGNATURE.len())
        .position(|w| w == SIGNATURE)
    {
        let start = from + at;
        let length = u64::from_le_bytes(bytes[start - 8..start].try_into().unwrap()) as usize;
        out.push((start, start + length, true));
        from = start + length;
    }
    for body in bodies {
        for start in find_all(bytes, body) {
            out.push((start, start + body.len(), false));
        }
    }
    out
}

/// The two documents, their files and their spans.
fn fixtures(dir: &TempDir) -> Vec<(String, Vec<u8>, Vec<Span>)> {
    let (path16, bytes16) = saved(&doc16(), dir, "d16.rz");
    let spans16 = spans(
        &bytes16,
        &[u16_bytes(&mask_samples()), u16_bytes(&channel_samples())],
    );
    // Four rasters (two noise layers, two placed fallbacks) and the 16-bit
    // and 8-bit originals are PNGs; then the mask and the channel.
    assert_eq!(
        spans16.iter().filter(|s| s.2).count(),
        6,
        "the 16-bit document's PNGs"
    );
    assert_eq!(spans16.len(), 8, "the 16-bit document's spans");
    let (path32, bytes32) = saved(&doc32(), dir, "d32.rz");
    let spans32 = spans(
        &bytes32,
        &[
            f32_bytes(&float_samples(W, H, 1.0)),
            f32_bytes(&float_samples(9, 7, 2.5)),
        ],
    );
    // The base raster, and the master's samples in its body (and in its
    // fallback raster, when the place kept them exactly).
    assert!(spans32.len() >= 2, "the float document's spans");
    vec![(path16, bytes16, spans16), (path32, bytes32, spans32)]
}

#[test]
fn a_saved_file_reopens_streamed_and_re_encodes_to_its_bytes() {
    let dir = TempDir::new().unwrap();
    for (path, bytes, _) in fixtures(&dir) {
        let streamed = open(&path).expect("the streamed reopen");
        assert!(encoded(&streamed) == bytes, "{path}: the reopen re-encodes");
        let sliced = parse(&bytes).expect("the slice reopen");
        assert!(
            encoded(&sliced) == bytes,
            "{path}: the slice reopen re-encodes"
        );
    }
    // The 16-bit document kept both originals and the mask and channel.
    let back = open(&dir.path().join("d16.rz").to_string_lossy()).unwrap();
    assert_eq!(back.layer_count(), 4);
    let depths: Vec<_> = (0..4)
        .filter_map(|i| back.layer(i).unwrap().effective_source())
        .map(|s| s.asset.depth())
        .collect();
    assert_eq!(depths, vec![16, 8]);
    // The float document kept its f32 master.
    let back = open(&dir.path().join("d32.rz").to_string_lossy()).unwrap();
    assert!(back.is_float());
    let master = back.layer(1).unwrap().effective_source().unwrap();
    assert_eq!(master.asset.depth(), 32);
}

/// `open_native` of `bytes` written to `path`, beside `parse_native`.
fn both(path: &str, bytes: &[u8]) -> (Result<Vec<u8>, String>, Result<Vec<u8>, String>) {
    std::fs::write(path, bytes).unwrap();
    (
        open(path).map(|d| encoded(&d)),
        parse(bytes).map(|d| encoded(&d)),
    )
}

#[test]
fn every_truncation_refuses_the_same_way_streamed_and_sliced() {
    let dir = TempDir::new().unwrap();
    let probe = dir.path().join("probe.rz");
    let probe = probe.to_str().unwrap();
    for (path, bytes, spans) in fixtures(&dir) {
        let len = bytes.len();
        let mut cuts: Vec<usize> = (0..len.min(4096)).collect();
        for (start, end, _) in spans {
            for at in [start - 1, start, start + 1, end - 1, end, end + 1] {
                cuts.push(at);
            }
        }
        cuts.push(len - 1);
        cuts.retain(|&at| at < len);
        cuts.sort_unstable();
        cuts.dedup();
        for at in cuts {
            let (streamed, sliced) = both(probe, &bytes[..at]);
            let streamed = streamed.expect_err("a truncated file refuses");
            let sliced = sliced.expect_err("a truncated buffer refuses");
            assert_eq!(streamed, sliced, "{path} cut at {at}");
        }
    }
}

#[test]
fn corrupted_bytes_answer_the_same_streamed_and_sliced() {
    let dir = TempDir::new().unwrap();
    let probe = dir.path().join("probe.rz");
    let probe = probe.to_str().unwrap();
    let mut refused = 0;
    for (path, bytes, spans) in fixtures(&dir) {
        let lids = find(&bytes, b"LIDS");
        let mut places = vec![
            ("header", 12),
            ("LIDS tag", lids + 1),
            ("LIDS id", bytes.len() - 3),
        ];
        for &(start, end, png) in &spans {
            places.push(("body", start + 5));
            if png {
                // IHDR's width; the IDAT's first stored data; its Adler-32.
                places.push(("IHDR", start + 17));
                places.push(("IDAT", start + 33 + 8 + 2 + 5 + 3));
                places.push(("IDAT end", end - 20));
            }
        }
        for (name, at) in places {
            let mut bad = bytes.clone();
            bad[at] ^= 0x5A;
            let (streamed, sliced) = both(probe, &bad);
            refused += usize::from(streamed.is_err());
            assert_eq!(streamed, sliced, "{path}: {name} at {at}");
        }
    }
    assert!(refused >= 5, "corruptions refuse: {refused}");
}

#[test]
fn a_file_that_shrinks_during_the_read_refuses_and_one_that_grows_reads() {
    let dir = TempDir::new().unwrap();
    for (path, bytes, _) in fixtures(&dir) {
        // Truncated after the preflight, before any pixel decodes.
        let target = path.clone();
        let error = open_native_observed(&path, LIMIT, 0, None, &mut || {
            std::fs::OpenOptions::new()
                .write(true)
                .open(&target)
                .unwrap()
                .set_len(64)
                .unwrap();
        })
        .map(|_| ())
        .expect_err("a shrunk file refuses");
        assert_eq!(error, "high-depth native file shrank during read", "{path}");

        // Grown after the open: the bytes past the length seen at open are
        // never read.
        std::fs::write(&path, &bytes).unwrap();
        let target = path.clone();
        let grown = open_native_observed(&path, LIMIT, 0, None, &mut || {
            let mut file = std::fs::OpenOptions::new()
                .append(true)
                .open(&target)
                .unwrap();
            file.write_all(&[0xA5; 4096]).unwrap();
        })
        .expect("a grown file reads as it was");
        assert!(
            encoded(&grown) == bytes,
            "{path}: the open read the original"
        );
    }
}

/// Three 1024² noise rasters: about 25 MB of stored PNGs.
fn large() -> PrecisionDocument {
    image16(1024, 1024, 0x2545_F491_4F6C_DD1D)
        .duplicate_layer(0)
        .unwrap()
        .unwrap()
        .duplicate_layer(0)
        .unwrap()
        .unwrap()
}

#[test]
fn a_save_and_a_reopen_admit_at_the_plan_below_the_file_length() {
    let doc = large();
    let plan = doc.native_plan().unwrap();
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("large.rz");
    let path = path.to_str().unwrap();

    let save = doc.retained_bytes() + plan.write_scratch_bytes;
    doc.save_native(path, save, 0, None)
        .expect("the save admits at its plan");
    let length = std::fs::metadata(path).unwrap().len();
    assert!(
        plan.write_scratch_bytes < length,
        "the save charges less than the file: {} ≥ {length}",
        plan.write_scratch_bytes
    );
    assert!(doc.save_native(path, save - 1, 0, None).is_err());

    let reopen = plan.reopen_retained_bytes + plan.decode_scratch_bytes;
    assert!(
        plan.decode_scratch_bytes < length,
        "the reopen charges less than the file: {} ≥ {length}",
        plan.decode_scratch_bytes
    );
    let back = PrecisionDocument::open_native(path, reopen, 0, None)
        .expect("the streamed reopen admits at the plan");
    let bytes = std::fs::read(path).unwrap();
    assert!(encoded(&back) == bytes);
    // The slice twin admits the same figure (its buffer is the caller's).
    PrecisionDocument::parse_native(&bytes, reopen, 0, None)
        .expect("the slice reopen admits at the plan");
}
