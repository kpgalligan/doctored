//! The hand-assembled metadata fixtures more than one test binary needs: an
//! EXIF TIFF stream with every patchable field's offset reported, and one
//! 8BIM IPTC run. Both were written for `metadata_tests.rs` /
//! `metadata_tiff_tests.rs` and moved here unchanged when
//! `export_production.rs` needed the same bytes — a fixture, like an
//! algorithm, has exactly one home. The byte-stream oracles, the PNG
//! assemblers and the FFI helpers below moved here unchanged from
//! `metadata_tests.rs` when `metadata_packet_framing.rs` needed them too.
#![allow(dead_code)]

use std::ffi::{c_char, c_int};
use std::path::Path;
use std::ptr;

use doctored_core::doc::DrDocument;
use doctored_core::ffi_color::*;
use doctored_core::ffi_doc::*;
use image::{ExtendedColorType, ImageEncoder, RgbaImage};
use tempfile::TempDir;

use super::{apply, cpath, take_err_string};

/// What an assembled EXIF block contains and where its patchable fields
/// live, so a test can assert that NOTHING ELSE moved.
pub struct Exif {
    pub bytes: Vec<u8>,
    /// Offset of the Orientation entry's 4-byte value field.
    pub orientation_field: usize,
    /// Offset of IFD0's next-IFD link.
    pub link: usize,
    /// Offset of the two resolution RATIONALs (16 bytes).
    pub rationals: usize,
    /// Offset of the Exif sub-IFD's ColorSpace value field, when the block
    /// has a sub-IFD at all.
    pub color_space_field: Option<usize>,
}

/// Assembles an EXIF TIFF stream by hand — the layout the ICC/metadata map
/// verified against `samples/portrait-exif6.jpg`, generalized.
pub fn build_exif(
    big_endian: bool,
    orientation: u16,
    resolution: (u32, u32),
    dimensions: Option<(u32, u32)>,
    thumbnail: bool,
) -> Exif {
    let u16b = |v: u16| {
        if big_endian {
            v.to_be_bytes()
        } else {
            v.to_le_bytes()
        }
    };
    let u32b = |v: u32| {
        if big_endian {
            v.to_be_bytes()
        } else {
            v.to_le_bytes()
        }
    };
    // Entry count and the layout that follows IFD0.
    let mut count = 4u16; // orientation, x res, y res, unit
    if dimensions.is_some() {
        count += 3; // width, length, Exif sub-IFD pointer
    }
    let ifd0 = 8usize;
    let link = ifd0 + 2 + 12 * usize::from(count);
    let mut cursor = link + 4;
    let rationals = cursor;
    cursor += 16;
    let sub_ifd = cursor;
    if dimensions.is_some() {
        // ColorSpace, PixelXDimension, PixelYDimension.
        cursor += 2 + 36 + 4;
    }
    let ifd1 = cursor;
    let thumb = ifd1 + 2 + 24 + 4;
    let end = if thumbnail { thumb + 8 } else { cursor };

    let mut b = vec![0u8; end];
    b[0..2].copy_from_slice(if big_endian { b"MM" } else { b"II" });
    b[2..4].copy_from_slice(&u16b(42));
    b[4..8].copy_from_slice(&u32b(ifd0 as u32));
    b[ifd0..ifd0 + 2].copy_from_slice(&u16b(count));

    let mut at = ifd0 + 2;
    let put = |b: &mut Vec<u8>, at: &mut usize, tag: u16, kind: u16, value: [u8; 4]| {
        b[*at..*at + 2].copy_from_slice(&u16b(tag));
        b[*at + 2..*at + 4].copy_from_slice(&u16b(kind));
        b[*at + 4..*at + 8].copy_from_slice(&u32b(1));
        b[*at + 8..*at + 12].copy_from_slice(&value);
        let field = *at + 8;
        *at += 12;
        field
    };
    // Entries must be sorted ascending by tag.
    if let Some((w, _)) = dimensions {
        put(&mut b, &mut at, 0x0100, 4, u32b(w));
    }
    if let Some((_, h)) = dimensions {
        put(&mut b, &mut at, 0x0101, 4, u32b(h));
    }
    // A SHORT's value is left-justified in the 4-byte field.
    let orientation_field = {
        let mut v = [0u8; 4];
        v[0..2].copy_from_slice(&u16b(orientation));
        put(&mut b, &mut at, 0x0112, 3, v)
    };
    put(&mut b, &mut at, 0x011A, 5, u32b(rationals as u32));
    put(&mut b, &mut at, 0x011B, 5, u32b((rationals + 8) as u32));
    {
        let mut v = [0u8; 4];
        v[0..2].copy_from_slice(&u16b(2)); // inch
        put(&mut b, &mut at, 0x0128, 3, v);
    }
    if dimensions.is_some() {
        put(&mut b, &mut at, 0x8769, 4, u32b(sub_ifd as u32));
    }
    b[link..link + 4].copy_from_slice(&u32b(if thumbnail { ifd1 as u32 } else { 0 }));

    b[rationals..rationals + 4].copy_from_slice(&u32b(resolution.0));
    b[rationals + 4..rationals + 8].copy_from_slice(&u32b(1));
    b[rationals + 8..rationals + 12].copy_from_slice(&u32b(resolution.1));
    b[rationals + 12..rationals + 16].copy_from_slice(&u32b(1));

    let mut color_space_field = None;
    if let Some((w, h)) = dimensions {
        b[sub_ifd..sub_ifd + 2].copy_from_slice(&u16b(3));
        let mut at = sub_ifd + 2;
        // ColorSpace 1 = sRGB, an inline SHORT of count 1 — what every
        // camera and phone writes.
        color_space_field = Some({
            let mut v = [0u8; 4];
            v[0..2].copy_from_slice(&u16b(1));
            put(&mut b, &mut at, 0xA001, 3, v)
        });
        put(&mut b, &mut at, 0xA002, 4, u32b(w));
        put(&mut b, &mut at, 0xA003, 4, u32b(h));
        b[at..at + 4].copy_from_slice(&u32b(0));
    }
    if thumbnail {
        b[ifd1..ifd1 + 2].copy_from_slice(&u16b(2));
        let mut at = ifd1 + 2;
        put(&mut b, &mut at, 0x0201, 4, u32b(thumb as u32));
        put(&mut b, &mut at, 0x0202, 4, u32b(8));
        b[at..at + 4].copy_from_slice(&u32b(0));
        b[thumb..thumb + 8].copy_from_slice(b"THUMBJPG");
    }
    Exif {
        bytes: b,
        orientation_field,
        link,
        rationals,
        color_space_field,
    }
}

/// One 8BIM IPTC run holding a single IIM 2:25 keyword — the smallest run
/// that survives `iptc_filtered`, which is what makes it a usable fixture
/// for "the IPTC packet was written".
pub const IIM: &[u8] = b"\x1c\x02\x19\0\x04Test";

pub fn iptc_run() -> Vec<u8> {
    let mut run = b"8BIM\x04\x04\0\0".to_vec();
    run.extend((IIM.len() as u32).to_be_bytes());
    run.extend(IIM);
    if !IIM.len().is_multiple_of(2) {
        run.push(0)
    }
    run
}

/// CRC-32 with the PNG polynomial, written out here so the walk below
/// verifies chunks against an oracle rather than against the core's copy.
pub fn crc32(bytes: &[u8]) -> u32 {
    let mut c: u32 = 0xFFFF_FFFF;
    for &b in bytes {
        c ^= u32::from(b);
        for _ in 0..8 {
            c = if c & 1 != 0 {
                0xEDB8_8320 ^ (c >> 1)
            } else {
                c >> 1
            };
        }
    }
    c ^ 0xFFFF_FFFF
}

/// Every JPEG marker segment as (marker, payload), stopping at SOS — an
/// independent walker, deliberately not the core's.
pub fn jpeg_segments(bytes: &[u8]) -> Vec<(u8, Vec<u8>)> {
    assert_eq!(&bytes[..2], &[0xFF, 0xD8], "SOI");
    let mut out = Vec::new();
    let mut i = 2usize;
    while i + 4 <= bytes.len() {
        assert_eq!(bytes[i], 0xFF, "a marker at {i}");
        let marker = bytes[i + 1];
        if marker == 0xDA || marker == 0xD9 {
            out.push((marker, Vec::new()));
            break;
        }
        let len = usize::from(u16::from_be_bytes([bytes[i + 2], bytes[i + 3]]));
        out.push((marker, bytes[i + 4..i + 2 + len].to_vec()));
        i += 2 + len;
    }
    out
}

/// Every PNG chunk as (type, data), with every CRC verified.
pub fn png_chunks(bytes: &[u8]) -> Vec<([u8; 4], Vec<u8>)> {
    assert_eq!(
        &bytes[..8],
        &[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]
    );
    let mut out = Vec::new();
    let mut i = 8usize;
    while i + 12 <= bytes.len() {
        let len = u32::from_be_bytes(bytes[i..i + 4].try_into().unwrap()) as usize;
        let kind: [u8; 4] = bytes[i + 4..i + 8].try_into().unwrap();
        let data = bytes[i + 8..i + 8 + len].to_vec();
        let stored = u32::from_be_bytes(bytes[i + 8 + len..i + 12 + len].try_into().unwrap());
        assert_eq!(
            crc32(&bytes[i + 4..i + 8 + len]),
            stored,
            "CRC of {:?}",
            std::str::from_utf8(&kind)
        );
        out.push((kind, data));
        i += 12 + len;
        if &kind == b"IEND" {
            break;
        }
    }
    out
}

/// One 8BIM image-resource block, with the padding rules the format's own
/// layout demands.
pub fn bim(id: u16, name: &[u8], data: &[u8]) -> Vec<u8> {
    let mut b = b"8BIM".to_vec();
    b.extend_from_slice(&id.to_be_bytes());
    b.push(name.len() as u8);
    b.extend_from_slice(name);
    // The WHOLE name field, length byte included, is padded to even.
    if !(1 + name.len()).is_multiple_of(2) {
        b.push(0);
    }
    b.extend_from_slice(&(data.len() as u32).to_be_bytes());
    b.extend_from_slice(data);
    if !data.len().is_multiple_of(2) {
        b.push(0);
    }
    b
}

pub fn png_chunk_bytes(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
    let mut c = (data.len() as u32).to_be_bytes().to_vec();
    c.extend_from_slice(kind);
    c.extend_from_slice(data);
    let crc = crc32(&c[4..]);
    c.extend_from_slice(&crc.to_be_bytes());
    c
}

/// A real encoded PNG with `chunks` inserted straight after `IHDR`.
pub fn png_with(pixels: &RgbaImage, chunks: &[Vec<u8>]) -> Vec<u8> {
    let mut base = Vec::new();
    image::codecs::png::PngEncoder::new(&mut base)
        .write_image(
            pixels.as_raw(),
            pixels.width(),
            pixels.height(),
            ExtendedColorType::Rgba8,
        )
        .unwrap();
    let ihdr_end = 8 + 8 + 13 + 4;
    let mut out = base[..ihdr_end].to_vec();
    for c in chunks {
        out.extend_from_slice(c);
    }
    out.extend_from_slice(&base[ihdr_end..]);
    out
}

pub fn open_file(dir: &TempDir, name: &str, bytes: &[u8]) -> *mut DrDocument {
    let path = dir.path().join(name);
    std::fs::write(&path, bytes).unwrap();
    let c = cpath(&path);
    let mut err: *mut c_char = ptr::null_mut();
    let doc = unsafe { dr_doc_open(c.as_ptr(), &mut err) };
    assert!(!doc.is_null(), "open {name}: {}", take_err_string(err));
    doc
}

pub fn packet(doc: *const DrDocument, kind: c_int) -> Option<Vec<u8>> {
    let len = unsafe { dr_doc_metadata_len(doc, kind) };
    if len == 0 {
        return None;
    }
    let mut out = vec![0u8; len];
    assert!(unsafe { dr_doc_metadata(doc, kind, out.as_mut_ptr(), len) });
    Some(out)
}

/// Sets a packet, FREEING the input — for linear chains.
pub fn set_packet(doc: *mut DrDocument, kind: c_int, bytes: &[u8]) -> *mut DrDocument {
    apply(doc, |d| unsafe {
        dr_doc_set_metadata(d, kind, bytes.as_ptr(), bytes.len(), ptr::null_mut())
    })
}

/// Saves and returns (the file's bytes, the carried bits).
pub fn save(
    doc: *const DrDocument,
    path: &Path,
    format: c_int,
    embed_profile: bool,
    strip_metadata: bool,
) -> (Vec<u8>, u32) {
    let c = cpath(path);
    let mut err: *mut c_char = ptr::null_mut();
    let mut carried: u32 = 0;
    assert!(
        unsafe {
            dr_doc_save_image(
                doc,
                ptr::null(),
                c.as_ptr(),
                format,
                92,
                embed_profile,
                strip_metadata,
                &mut carried,
                &mut err,
            )
        },
        "save: {}",
        take_err_string(err)
    );
    (std::fs::read(path).unwrap(), carried)
}
