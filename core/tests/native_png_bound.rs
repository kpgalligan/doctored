//! The native RGBA16 PNG bound is the exact stored length the writer
//! produces (`precision_png`'s `# Encoded bound`), not a fitted
//! 10 B/px + 1 MiB. Each saved PNG is walked by an oracle written here —
//! signature, IHDR, IDAT+, IEND, then the zlib stream's stored blocks — and
//! its length is checked against this file's own formula, on flat and noise
//! pixels alike. The plan's whole-file figure is then exact up to one
//! per-shape header constant at every size, linear light included.

use doctored_core::doc_evaluation::Evaluation;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use doctored_core::retained_source::SourceEdit;

const SIGNATURE: &[u8] = b"\x89PNG\r\n\x1a\n";

/// The sizes the bound must be exact at: one pixel, R = h·(8w+1) of exactly
/// one full stored block (65,535), one 257-byte row over it (R = 65,536 is
/// unreachable: every row is 8w + 1 bytes), exactly two full blocks, and a
/// photo-like plane.
const SIZES: [(u32, u32); 7] = [
    (1, 1),
    (2, 3855),
    (32, 255),
    (32, 256),
    (32, 510),
    (546, 15),
    (1000, 700),
];

/// An RGBA16 sRGB plane: flat, or xorshift64 noise in all four channels.
fn plane(w: u32, h: u32, noise: bool) -> PrecisionImage {
    let mut state: u64 = 0x2545_F491_4F6C_DD1D;
    let pixels = (0..w as usize * h as usize)
        .map(|_| {
            if !noise {
                return [12_000, 34_000, 56_000, 65_535];
            }
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
        .collect();
    PrecisionImage::from_rgba16(w, h, pixels, IccProfile::srgb(), None).unwrap()
}

fn document(w: u32, h: u32, noise: bool) -> PrecisionDocument {
    PrecisionDocument::from_image(
        plane(w, h, noise),
        Metadata::default(),
        Resolution { x: 72., y: 72. },
    )
    .unwrap()
}

/// `document` whose only layer is bound to a retained source: a placed
/// image, which writes the layer raster and the source as two PNGs.
fn placed(w: u32, h: u32, noise: bool) -> PrecisionDocument {
    document(w, h, noise)
        .source_edit(
            None,
            &SourceEdit::parse(r#"{"op":"retain","source_id":"bound392"}"#).unwrap(),
            REFERENCE_WORKING_BYTES,
            0,
            None,
        )
        .unwrap()
        .unwrap()
}

/// This file's own length formula for a stored RGBA16 PNG: R row bytes
/// (a filter byte per row), Z the zlib stream (2-byte header, 5 bytes per
/// stored block of ≤ 65,535, Adler-32), then signature, IHDR, one IDAT per
/// 2^31 − 1 bytes, IEND.
fn expected_len(w: u32, h: u32) -> u64 {
    let r = u64::from(h) * (8 * u64::from(w) + 1);
    let z = 2 + r + 5 * r.div_ceil(65_535) + 4;
    8 + (12 + 13) + z + 12 * z.div_ceil((1 << 31) - 1) + 12
}

/// Every PNG in a native file: the writer frames each one with its length as
/// a u64 LE in the eight bytes before its signature.
fn pngs(file: &[u8]) -> Vec<&[u8]> {
    let mut out = Vec::new();
    let mut at = 8;
    while let Some(offset) = file[at..]
        .windows(SIGNATURE.len())
        .position(|w| w == SIGNATURE)
    {
        let start = at + offset;
        let len = u64::from_le_bytes(file[start - 8..start].try_into().unwrap()) as usize;
        out.push(&file[start..start + len]);
        at = start + len;
    }
    out
}

/// Walks one PNG: signature, IHDR (RGBA16), IDAT+, IEND and nothing else,
/// by chunk lengths alone; then the IDAT stream: zlib header `78 01`, stored
/// blocks (BTYPE 00) with LEN = !NLEN and LEN ≤ 65,535, every non-final one
/// full, then the Adler-32. Returns (w, h) and the summed LEN.
fn walk(png: &[u8]) -> ((u32, u32), u64) {
    assert_eq!(&png[..8], SIGNATURE);
    let (mut at, mut kinds, mut zlib) = (8, Vec::new(), Vec::new());
    let mut size = (0, 0);
    while at < png.len() {
        let len = u32::from_be_bytes(png[at..at + 4].try_into().unwrap()) as usize;
        let kind = &png[at + 4..at + 8];
        let data = &png[at + 8..at + 8 + len];
        match kind {
            b"IHDR" => {
                assert_eq!(len, 13);
                size = (
                    u32::from_be_bytes(data[..4].try_into().unwrap()),
                    u32::from_be_bytes(data[4..8].try_into().unwrap()),
                );
                assert_eq!(&data[8..10], &[16, 6], "RGBA16");
            }
            b"IDAT" => zlib.extend_from_slice(data),
            b"IEND" => assert_eq!(len, 0),
            other => panic!("unexpected chunk {other:?}"),
        }
        kinds.push(kind.to_vec());
        at += 12 + len;
    }
    assert_eq!(at, png.len(), "chunks end exactly at the PNG's end");
    assert_eq!(kinds.first().unwrap(), b"IHDR");
    assert_eq!(kinds.last().unwrap(), b"IEND");
    assert!(kinds.len() >= 3);
    assert!(kinds[1..kinds.len() - 1].iter().all(|k| k == b"IDAT"));
    assert_eq!(&zlib[..2], &[0x78, 0x01]);
    let (mut at, mut sum) = (2, 0_u64);
    loop {
        let header = zlib[at];
        assert_eq!(header & !1, 0, "a stored block (BTYPE 00)");
        let len = u16::from_le_bytes([zlib[at + 1], zlib[at + 2]]);
        let nlen = u16::from_le_bytes([zlib[at + 3], zlib[at + 4]]);
        assert_eq!(len, !nlen);
        at += 5 + usize::from(len);
        sum += u64::from(len);
        if header & 1 == 1 {
            break;
        }
        assert_eq!(len, 65_535, "every non-final block is full");
    }
    assert_eq!(at + 4, zlib.len(), "the Adler-32 closes the stream");
    (size, sum)
}

/// Checks one PNG against the oracle and the formula; returns its length.
fn check(png: &[u8], (w, h): (u32, u32)) -> u64 {
    let (size, sum) = walk(png);
    assert_eq!(size, (w, h));
    assert_eq!(sum, u64::from(h) * (8 * u64::from(w) + 1), "{w}×{h} R");
    assert_eq!(png.len() as u64, expected_len(w, h), "{w}×{h} L");
    png.len() as u64
}

#[test]
fn a_native_png_is_exactly_its_stored_bound() {
    // The documented figures: one pixel is 77 bytes, one full block 65,603.
    assert_eq!(expected_len(1, 1), 77);
    assert_eq!(expected_len(32, 255), 65_603);
    for (w, h) in SIZES {
        let [flat, noise] = [false, true].map(|noise| {
            let file = document(w, h, noise)
                .encode_native(REFERENCE_WORKING_BYTES, 0, None)
                .unwrap();
            let found = pngs(&file);
            assert_eq!(found.len(), 1, "{w}×{h}");
            check(found[0], (w, h))
        });
        assert_eq!(flat, noise, "{w}×{h}: the length never depends on pixels");
    }
}

#[test]
fn a_placed_document_s_source_png_is_exactly_its_stored_bound() {
    for (w, h) in [(32, 255), (1000, 700)] {
        for noise in [false, true] {
            let file = placed(w, h, noise)
                .encode_native(REFERENCE_WORKING_BYTES, 0, None)
                .unwrap();
            let found = pngs(&file);
            assert_eq!(found.len(), 2, "{w}×{h}: the layer raster and its source");
            for png in found {
                check(png, (w, h));
            }
        }
    }
}

/// `encoded_bound − bytes.len()` of `doc`'s plan and file.
fn slack(doc: &PrecisionDocument) -> (u64, u64) {
    let plan = doc.native_plan().unwrap();
    let file = doc.encode_native(REFERENCE_WORKING_BYTES, 0, None).unwrap();
    let len = file.len() as u64;
    assert!(plan.encoded_bound >= len, "the plan covers the file");
    (plan.encoded_bound - len, len)
}

#[test]
fn the_plan_s_png_term_is_exact_at_every_size() {
    // With the exact PNG term, whatever the plan charges beyond the file is
    // header estimate alone, so it is one constant at every size of a shape.
    for shape in [document, placed] {
        let slacks: Vec<_> = SIZES
            .iter()
            .map(|&(w, h)| slack(&shape(w, h, false)).0)
            .collect();
        assert!(slacks.iter().all(|s| *s == slacks[0]), "{slacks:?}");
    }
    // Linear light: the plan counts the version word 39
    // and the 12-byte EVAL table the writer writes, byte for byte, so the
    // planned header grows exactly as the written one does.
    for (w, h) in SIZES {
        let encoded = document(w, h, false);
        let linear = encoded
            .with_evaluation(Evaluation::LinearLight)
            .unwrap()
            .unwrap();
        let (encoded_slack, encoded_len) = slack(&encoded);
        let (linear_slack, linear_len) = slack(&linear);
        assert_eq!(linear_len, encoded_len + 16, "{w}×{h}");
        assert_eq!(linear_slack, encoded_slack, "{w}×{h}");
    }
}
