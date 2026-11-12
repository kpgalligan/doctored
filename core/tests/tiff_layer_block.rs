//! Which TIFFs carry Photoshop's layer block with layers in it, through the C
//! API (`dr_tiff_has_layer_block`): either byte order, classic or BigTIFF,
//! any colour model, depth and sample format, the files the layered open
//! does not read (`dr_tiff_has_layers` false) included.
//!
//! Oracle: a TIFF builder written here from the TIFF 6.0 layout (an 8-byte
//! header, one directory of 12-byte entries) and the BigTIFF layout (a
//! 16-byte header, a u64 count of 20-byte entries with 8-byte value fields),
//! its tag 37724 holding the block's prefix and `8BIM`-framed entries as
//! Photoshop's notes lay them out: each entry a signature, a key, a u32
//! length and the body padded to 4, the codes byte-reversed in a
//! little-endian file (`MIB8`, `ryaL`). A layer entry's body starts with the
//! i16 record count. None of it shares code with `src/`.
use std::ptr;

use doctored_core::ffi_psd::{dr_tiff_has_layer_block, dr_tiff_has_layers};
use tempfile::TempDir;

use crate::common::cpath;

const PREFIX: &[u8] = b"Adobe Photoshop Document Data Block\0";

// ------------------------------------------------------ the TIFF builder --

/// The first directory's model. Every file is 2 × 2, one uncompressed strip.
#[derive(Clone, Copy)]
struct Shape {
    little: bool,
    /// BigTIFF (version 43) rather than classic (42).
    big: bool,
    photometric: u16,
    samples: u16,
    bits: u16,
    /// SampleFormat on every sample, or absent.
    format: Option<u16>,
}

/// The one shape `dr_tiff_has_layers` reads: classic big-endian unsigned
/// 8-bit RGB.
const RGB8: Shape = Shape {
    little: false,
    big: false,
    photometric: 2,
    samples: 3,
    bits: 8,
    format: None,
};

fn put16(little: bool, value: u16) -> [u8; 2] {
    if little {
        value.to_le_bytes()
    } else {
        value.to_be_bytes()
    }
}

fn put32(little: bool, value: u32) -> [u8; 4] {
    if little {
        value.to_le_bytes()
    } else {
        value.to_be_bytes()
    }
}

fn put64(little: bool, value: u64) -> [u8; 8] {
    if little {
        value.to_le_bytes()
    } else {
        value.to_be_bytes()
    }
}

/// A four-character code as a `little` block stores it.
fn code(little: bool, code: &[u8; 4]) -> [u8; 4] {
    let mut out = *code;
    if little {
        out.reverse();
    }
    out
}

/// Tag 37724's value: the prefix, then each `(key, body)` as one entry of
/// the `little` order, padded to 4.
fn block(little: bool, entries: &[(&[u8; 4], Vec<u8>)]) -> Vec<u8> {
    let mut out = PREFIX.to_vec();
    for (key, body) in entries {
        out.extend_from_slice(&code(little, b"8BIM"));
        out.extend_from_slice(&code(little, key));
        out.extend_from_slice(&put32(little, body.len() as u32));
        out.extend_from_slice(body);
        while !out.len().is_multiple_of(4) {
            out.push(0);
        }
    }
    out
}

/// A layer entry's body holding `count` records: the i16 count in the
/// `little` order, then record bytes nothing here parses.
fn records(little: bool, count: i16) -> Vec<u8> {
    let mut body = put16(little, count as u16).to_vec();
    body.extend_from_slice(&[0x5a; 6]);
    body
}

/// One layer entry keyed `key` holding `count` records, as a file of the
/// `little` order writes it.
fn layers(little: bool, key: &[u8; 4], count: i16) -> Vec<u8> {
    block(little, &[(key, records(little, count))])
}

/// A whole TIFF of `shape` whose tag 37724 holds `value` (absent for
/// `None`). Values longer than the entry's value field go out of line,
/// word-aligned, before the directory.
fn tiff(shape: Shape, value: Option<&[u8]>) -> Vec<u8> {
    let little = shape.little;
    let field = if shape.big { 8 } else { 4 };
    let mut out = if little {
        b"II".to_vec()
    } else {
        b"MM".to_vec()
    };
    if shape.big {
        out.extend_from_slice(&put16(little, 43));
        out.extend_from_slice(&put16(little, 8));
        out.extend_from_slice(&put16(little, 0));
        out.extend_from_slice(&[0; 8]);
    } else {
        out.extend_from_slice(&put16(little, 42));
        out.extend_from_slice(&[0; 4]);
    }
    let strip = out.len() as u32;
    let strip_len = 2 * 2 * u32::from(shape.samples) * u32::from(shape.bits / 8);
    out.extend(std::iter::repeat_n(7u8, strip_len as usize));
    let shorts = |value: u16, count: u16| -> Vec<u8> {
        (0..count).flat_map(|_| put16(little, value)).collect()
    };
    // (tag, type, count, the value's bytes)
    let mut fields: Vec<(u16, u16, u64, Vec<u8>)> = vec![
        (256, 3, 1, shorts(2, 1)),
        (257, 3, 1, shorts(2, 1)),
        (
            258,
            3,
            u64::from(shape.samples),
            shorts(shape.bits, shape.samples),
        ),
        (259, 3, 1, shorts(1, 1)),
        (262, 3, 1, shorts(shape.photometric, 1)),
        (273, 4, 1, put32(little, strip).to_vec()),
        (277, 3, 1, shorts(shape.samples, 1)),
        (278, 3, 1, shorts(2, 1)),
        (279, 4, 1, put32(little, strip_len).to_vec()),
    ];
    if let Some(format) = shape.format {
        fields.push((
            339,
            3,
            u64::from(shape.samples),
            shorts(format, shape.samples),
        ));
    }
    if let Some(value) = value {
        fields.push((37724, 7, value.len() as u64, value.to_vec()));
    }
    // The value field of each entry: its bytes inline, padded, or the offset
    // of its out-of-line copy.
    let mut slots = Vec::new();
    for (_, _, _, bytes) in &fields {
        if bytes.len() <= field {
            let mut slot = bytes.clone();
            slot.resize(field, 0);
            slots.push(slot);
        } else {
            if !out.len().is_multiple_of(2) {
                out.push(0);
            }
            let at = out.len() as u64;
            out.extend_from_slice(bytes);
            slots.push(if shape.big {
                put64(little, at).to_vec()
            } else {
                put32(little, at as u32).to_vec()
            });
        }
    }
    if !out.len().is_multiple_of(2) {
        out.push(0);
    }
    let directory = out.len() as u64;
    if shape.big {
        out.extend_from_slice(&put64(little, fields.len() as u64));
    } else {
        out.extend_from_slice(&put16(little, fields.len() as u16));
    }
    for ((tag, kind, count, _), slot) in fields.iter().zip(&slots) {
        out.extend_from_slice(&put16(little, *tag));
        out.extend_from_slice(&put16(little, *kind));
        if shape.big {
            out.extend_from_slice(&put64(little, *count));
        } else {
            out.extend_from_slice(&put32(little, *count as u32));
        }
        out.extend_from_slice(slot);
    }
    out.extend(std::iter::repeat_n(0u8, field));
    if shape.big {
        out[8..16].copy_from_slice(&put64(little, directory));
    } else {
        out[4..8].copy_from_slice(&put32(little, directory as u32));
    }
    out
}

/// What the two queries answer for `bytes` written to `name`:
/// `(dr_tiff_has_layers, dr_tiff_has_layer_block)`.
fn answers(directory: &TempDir, name: &str, bytes: &[u8]) -> (bool, bool) {
    let path = directory.path().join(name);
    std::fs::write(&path, bytes).unwrap();
    let path = cpath(&path);
    unsafe {
        (
            dr_tiff_has_layers(path.as_ptr()),
            dr_tiff_has_layer_block(path.as_ptr()),
        )
    }
}

// ----------------------------------------------------------------- cases --

#[test]
fn a_layer_block_the_layered_open_does_not_read_still_holds_layers() {
    let directory = TempDir::new().unwrap();
    let little8 = Shape {
        little: true,
        ..RGB8
    };
    let little16 = Shape {
        bits: 16,
        ..little8
    };
    let gray32 = Shape {
        photometric: 1,
        samples: 1,
        bits: 32,
        format: Some(3),
        ..RGB8
    };
    let cases: [(&str, Shape, &[u8; 4]); 9] = [
        ("little-endian RGB at 8 bits", little8, b"Layr"),
        ("little-endian RGB at 16 bits", little16, b"Lr16"),
        ("32-bit float Grayscale, big-endian", gray32, b"Lr32"),
        (
            "32-bit float Grayscale, little-endian",
            Shape {
                little: true,
                ..gray32
            },
            b"Lr32",
        ),
        (
            "Palette",
            Shape {
                photometric: 3,
                samples: 1,
                ..RGB8
            },
            b"Layr",
        ),
        (
            "16-bit float RGB",
            Shape {
                bits: 16,
                format: Some(3),
                ..RGB8
            },
            b"Lr16",
        ),
        ("32-bit unsigned RGB", Shape { bits: 32, ..RGB8 }, b"Lr32"),
        ("BigTIFF, big-endian", Shape { big: true, ..RGB8 }, b"Layr"),
        (
            "BigTIFF, little-endian",
            Shape {
                big: true,
                ..little8
            },
            b"Layr",
        ),
    ];
    for (index, (name, shape, key)) in cases.into_iter().enumerate() {
        let value = layers(shape.little, key, 1);
        let (read, held) = answers(
            &directory,
            &format!("unread{index}.tif"),
            &tiff(shape, Some(&value)),
        );
        assert!(!read, "{name}: the layered open does not read this file");
        assert!(held, "{name}: its layer block holds a layer");
    }
}

#[test]
fn a_readable_block_holds_layers_only_when_its_count_is_not_zero() {
    let directory = TempDir::new().unwrap();
    // The same file the layered open reads, with one record, none, and a
    // negative count (the first alpha channel is the merged transparency).
    for (count, expected) in [(1i16, true), (0, false), (-2, true)] {
        let (read, held) = answers(
            &directory,
            &format!("readable{count}.tif"),
            &tiff(RGB8, Some(&layers(false, b"Layr", count))),
        );
        assert!(read, "count {count}: the builder's file is a layered TIFF");
        assert_eq!(held, expected, "count {count}");
    }
    // An unread little-endian 16-bit file with no records holds no layers.
    let little16 = Shape {
        little: true,
        bits: 16,
        ..RGB8
    };
    let (_, held) = answers(
        &directory,
        "empty16.tif",
        &tiff(little16, Some(&layers(true, b"Lr16", 0))),
    );
    assert!(!held, "an empty little-endian list");
}

#[test]
fn the_walk_reaches_layers_after_other_entries_in_either_code_order() {
    let directory = TempDir::new().unwrap();
    // An odd-length entry first, so the next head sits past its padding,
    // then an empty list for the other depth, then this depth's records.
    for little in [false, true] {
        let value = block(
            little,
            &[
                (b"cinf", vec![1, 2, 3, 4, 5]),
                (b"Lr16", records(little, 0)),
                (b"Layr", records(little, 3)),
            ],
        );
        let shape = Shape { little, ..RGB8 };
        let (_, held) = answers(
            &directory,
            &format!("walk{little}.tif"),
            &tiff(shape, Some(&value)),
        );
        assert!(held, "little-endian {little}: the third entry holds layers");
    }
    // A little-endian file whose entries are spelled big-endian (`8BIM`,
    // lengths and count big-endian): each entry's signature names its order.
    let shape = Shape {
        little: true,
        ..RGB8
    };
    let (_, held) = answers(
        &directory,
        "spelled-big.tif",
        &tiff(shape, Some(&layers(false, b"Layr", 2))),
    );
    assert!(held, "a big-endian block in a little-endian file");
    // Only entries that are not layer lists: no layers.
    let value = block(false, &[(b"Patt", vec![9; 12]), (b"cinf", vec![1; 3])]);
    let (_, held) = answers(&directory, "patterns.tif", &tiff(RGB8, Some(&value)));
    assert!(!held, "patterns and cinf are not layers");
}

#[test]
fn a_block_whose_entries_cannot_be_walked_answers_true() {
    let directory = TempDir::new().unwrap();
    // Another signature after the prefix.
    let mut value = PREFIX.to_vec();
    value.extend_from_slice(b"XXXXLayr\0\0\0\x08");
    value.extend_from_slice(&[0, 1, 0, 0, 0, 0, 0, 0]);
    let (_, held) = answers(&directory, "signature.tif", &tiff(RGB8, Some(&value)));
    assert!(held, "an unknown signature");
    // An entry whose length runs past the value.
    let mut value = layers(false, b"Lr16", 0);
    let at = PREFIX.len() + 8;
    value[at..at + 4].copy_from_slice(&0x7fff_0000u32.to_be_bytes());
    let (_, held) = answers(&directory, "overrun.tif", &tiff(RGB8, Some(&value)));
    assert!(held, "a length past the value");
}

#[test]
fn a_file_with_no_layer_block_answers_false() {
    let directory = TempDir::new().unwrap();
    let held = |name: &str, bytes: &[u8]| answers(&directory, name, bytes).1;
    // No tag 37724: a TIFF this crate writes.
    let plain = directory.path().join("plain.tif");
    let pixels = [9u8; 3 * 3 * 4];
    let mut error = ptr::null_mut();
    unsafe {
        let image = doctored_core::ffi::dr_image_from_rgba(pixels.as_ptr(), 3, 3);
        assert!(doctored_core::ffi::dr_image_save(
            image,
            cpath(&plain).as_ptr(),
            2,
            90,
            &mut error
        ));
        doctored_core::ffi::dr_image_free(image);
        assert!(!dr_tiff_has_layer_block(cpath(&plain).as_ptr()));
    }
    assert!(!held("untagged.tif", &tiff(RGB8, None)));
    // A tag 37724 without the prefix, and one short enough to sit inline.
    let mut other = b"Another Program's Private Data Block\0".to_vec();
    other.extend_from_slice(&records(false, 1));
    assert!(!held("other.tif", &tiff(RGB8, Some(&other))));
    assert!(!held("inline.tif", &tiff(RGB8, Some(b"Adob"))));
    // A file cut inside its directory, an empty file, a PNG signature.
    let whole = tiff(RGB8, Some(&layers(false, b"Layr", 1)));
    assert!(!held("cut.tif", &whole[..whole.len() - 20]));
    assert!(!held("empty.tif", &[]));
    assert!(!held("png.tif", b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR"));
    // An absent path and NULL.
    let absent = cpath(&directory.path().join("absent.tif"));
    assert!(!unsafe { dr_tiff_has_layer_block(absent.as_ptr()) });
    assert!(!unsafe { dr_tiff_has_layer_block(ptr::null()) });
}
