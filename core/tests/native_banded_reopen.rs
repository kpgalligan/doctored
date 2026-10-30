//! The typed native reopen decodes each RGBA16 PNG plane row by row
//! straight into tiles (`precision_png_rows`), and the reopen envelope
//! charges that banded figure (`precision_native_plan::reopen_scratch`: the
//! file, the planes and one band plus a quarter, with no cap at the 27 B/px
//! whole-plane figure).
//!
//! Oracles: the samples the document held before it was saved
//! (`to_rgba16()`), the reopen figure written out here from the plan's own
//! terms, and the PNG IHDR layout (signature, length, type, 13 data bytes,
//! CRC-32 over type and data) written out here.

use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_geometry::GeometryEdit;
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use doctored_core::retained_source::SourceEdit;
use serde_json::json;

const BOOKKEEPING: u64 = 1024 * 1024;
const CODEC_FIXED: u64 = 4 * 1024 * 1024;

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

fn document(w: u32, h: u32, pixels: Vec<[u16; 4]>) -> PrecisionDocument {
    PrecisionDocument::from_image(
        PrecisionImage::from_rgba16(w, h, pixels, IccProfile::srgb(), None).unwrap(),
        Metadata::default(),
        Resolution { x: 72., y: 72. },
    )
    .unwrap()
}

/// `doc`'s only layer bound to a retained source: a placed image.
fn retained(doc: &PrecisionDocument) -> PrecisionDocument {
    doc.source_edit(
        None,
        &SourceEdit::parse(r#"{"op":"retain","source_id":"banded"}"#).unwrap(),
        REFERENCE_WORKING_BYTES,
        0,
        None,
    )
    .unwrap()
    .unwrap()
}

fn reopened(doc: &PrecisionDocument) -> PrecisionDocument {
    let bytes = doc.encode_native(REFERENCE_WORKING_BYTES, 0, None).unwrap();
    PrecisionDocument::parse_native(&bytes, REFERENCE_WORKING_BYTES, 0, None).unwrap()
}

fn layer_samples(doc: &PrecisionDocument, index: usize) -> Vec<[u16; 4]> {
    doc.layer_source(index).unwrap().to_rgba16().unwrap()
}

fn original_samples(doc: &PrecisionDocument, index: usize) -> Vec<[u16; 4]> {
    doc.layer(index)
        .unwrap()
        .effective_source()
        .unwrap()
        .asset
        .rgba16()
        .unwrap()
        .to_rgba16()
        .unwrap()
}

#[test]
fn noise_planes_round_trip_bit_exact() {
    for (i, (w, h)) in [(1, 1), (257, 1), (256, 256), (515, 300)]
        .into_iter()
        .enumerate()
    {
        let doc = document(w, h, noise(w, h, 0x9E37_79B9_7F4A_7C15 + i as u64));
        let oracle = layer_samples(&doc, 0);
        let back = reopened(&doc);
        assert_eq!(back.layer_source(0).unwrap().dimensions(), (w, h));
        assert!(layer_samples(&back, 0) == oracle, "{w}x{h}");
    }
    let placed = retained(&document(515, 300, noise(515, 300, 0x0123_4567_89AB_CDEF)));
    let (raster, original) = (layer_samples(&placed, 0), original_samples(&placed, 0));
    let back = reopened(&placed);
    assert!(layer_samples(&back, 0) == raster);
    assert!(original_samples(&back, 0) == original);
}

#[test]
fn a_uniform_plane_reopens_filled() {
    let doc = document(600, 600, vec![[1234, 5678, 9012, 65535]; 600 * 600]);
    let back = reopened(&doc);
    // At most four tile allocations (interior, right edge, bottom edge,
    // corner, each at most 256×256 RGBA16), the 3×3 index and bookkeeping.
    let bound = 4 * 256 * 256 * 8 + 9 * 8 + BOOKKEEPING;
    let image = back.layer_source(0).unwrap();
    assert!(
        image.retained_bytes() <= bound,
        "{}",
        image.retained_bytes()
    );
    assert!(layer_samples(&back, 0) == vec![[1234, 5678, 9012, 65535]; 600 * 600]);
}

#[test]
fn an_interlaced_native_plane_is_refused() {
    let doc = document(64, 48, noise(64, 48, 7));
    let mut bytes = doc.encode_native(REFERENCE_WORKING_BYTES, 0, None).unwrap();
    let signature = b"\x89PNG\r\n\x1a\n";
    let at = bytes
        .windows(signature.len())
        .position(|w| w == signature)
        .expect("the layer PNG");
    assert_eq!(&bytes[at + 12..at + 16], b"IHDR");
    // IHDR data: width, height, depth, colour type, compression, filter,
    // interlace; the CRC covers the type and the data.
    assert_eq!(bytes[at + 28], 0);
    bytes[at + 28] = 1;
    let crc = crc32fast::hash(&bytes[at + 12..at + 29]);
    bytes[at + 29..at + 33].copy_from_slice(&crc.to_be_bytes());
    let error = PrecisionDocument::parse_native(&bytes, REFERENCE_WORKING_BYTES, 0, None)
        .err()
        .expect("an interlaced native plane is refused");
    assert_eq!(error, "high-depth native: layer PNG must not be interlaced");
}

/// The stored length of a native RGBA16 PNG (`native_png_bound.rs`'s
/// formula): row bytes with a filter byte each, the zlib stream's stored
/// blocks and framing, then signature, IHDR, one IDAT and IEND.
fn png_len(w: u32, h: u32) -> u64 {
    let r = u64::from(h) * (8 * u64::from(w) + 1);
    let z = 2 + r + 5 * r.div_ceil(65_535) + 4;
    8 + (12 + 13) + z + 12 * z.div_ceil((1 << 31) - 1) + 12
}

/// The streamed reader's window (`native_stream`).
const STREAM_WINDOW: u64 = 4 * 1024 * 1024;

#[test]
fn the_reopen_scratch_is_the_banded_figure() {
    // The reader holds the file less its spans (the raster's and
    // the source's stored PNGs, both 2000×1500) plus its window.
    let doc = retained(&document(2000, 1500, noise(2000, 1500, 11)));
    let plan = doc.native_plan().unwrap();
    let banded = 16 * (8 * 2000 + 1) + CODEC_FIXED;
    let held = plan.encoded_bound - 2 * png_len(2000, 1500) + STREAM_WINDOW;
    let base = plan.reopen_retained_bytes + held + banded + BOOKKEEPING;
    assert_eq!(
        plan.decode_scratch_bytes,
        base + base.div_ceil(4) - plan.reopen_retained_bytes
    );
}

#[test]
fn a_turned_placed_image_reopens_at_its_banded_envelope() {
    let turned = retained(&document(320, 240, noise(320, 240, 13)))
        .geometry(
            &GeometryEdit::parse(
                &json!({"op":"rotate_canvas","degrees":17,"filter":"bicubic"}).to_string(),
            )
            .unwrap(),
            REFERENCE_WORKING_BYTES,
            0,
            None,
        )
        .unwrap()
        .unwrap();
    let plan = turned.native_plan().unwrap();
    let (w, h) = turned.layer_source(0).unwrap().dimensions();
    // The whole-plane figure the banded one must undercut here, over the
    // same held bytes (the file less the raster's and the 320×240 source's
    // stored PNGs, plus the window).
    let held = plan.encoded_bound - png_len(w, h) - png_len(320, 240) + STREAM_WINDOW;
    let whole = held + 27 * u64::from(w) * u64::from(h) + CODEC_FIXED + BOOKKEEPING;
    assert!(plan.decode_scratch_bytes < whole);
    let bytes = turned
        .encode_native(REFERENCE_WORKING_BYTES, 0, None)
        .unwrap();
    let limit = plan.reopen_retained_bytes + plan.decode_scratch_bytes;
    let back = PrecisionDocument::parse_native(&bytes, limit, 0, None).unwrap();
    assert!(back.retained_bytes() <= plan.reopen_retained_bytes);
    assert!(layer_samples(&back, 0) == layer_samples(&turned, 0));
}
