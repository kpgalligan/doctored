//! Layered TIFF through the C API: the file a layered TIFF export
//! writes, and which TIFFs the layered open claims.
//!
//! Oracles: a TIFF directory reader and a TIFF builder written here from the
//! TIFF 6.0 layout (header, one IFD of 12-byte entries), and
//! `common/psb_fixture.rs` for the layer information and image resources
//! inside Photoshop's tags. None of it shares code with `src/tiff_layers.rs`.
use std::collections::BTreeMap;
use std::ptr;
use std::sync::Arc;

use doctored_core::doc::DrDocument;
use doctored_core::ffi_psd::dr_tiff_has_layers;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer};
use doctored_core::precision_image::PrecisionImage;
use image::RgbaImage;
use serde_json::Value;
use tempfile::TempDir;

use crate::common::{cpath, take_err_string};
use crate::psb_fixture::{self as fixture, two_layers, File, FLATTENED, H, LAYERS, TIFF, W};

const HEADER: &[u8] = b"Adobe Photoshop Document Data Block\0";
const NO_COMPOSITE: &str = "a TIFF's flat image opens through the ordinary TIFF open";

// ------------------------------------------------------- the TIFF reader --

/// One big-endian directory: tag -> (type, count, the entry's four value
/// bytes).
struct Directory<'a> {
    bytes: &'a [u8],
    entries: BTreeMap<u16, (u16, u32, [u8; 4])>,
}

fn directory(bytes: &[u8]) -> Directory<'_> {
    assert_eq!(&bytes[..4], b"MM\0*", "a classic big-endian TIFF");
    let at = u32::from_be_bytes(bytes[4..8].try_into().unwrap()) as usize;
    let count = usize::from(u16::from_be_bytes([bytes[at], bytes[at + 1]]));
    let mut entries = BTreeMap::new();
    let mut previous = 0u16;
    for entry in bytes[at + 2..at + 2 + count * 12].chunks_exact(12) {
        let tag = u16::from_be_bytes([entry[0], entry[1]]);
        assert!(tag > previous, "tags ascend");
        previous = tag;
        entries.insert(
            tag,
            (
                u16::from_be_bytes([entry[2], entry[3]]),
                u32::from_be_bytes(entry[4..8].try_into().unwrap()),
                entry[8..12].try_into().unwrap(),
            ),
        );
    }
    let next = at + 2 + count * 12;
    assert_eq!(&bytes[next..next + 4], &[0; 4], "one directory");
    Directory { bytes, entries }
}

impl Directory<'_> {
    /// A SHORT or LONG scalar.
    fn scalar(&self, tag: u16) -> u32 {
        let (kind, count, value) = self.entries[&tag];
        assert_eq!(count, 1, "tag {tag}");
        match kind {
            3 => u32::from(u16::from_be_bytes([value[0], value[1]])),
            4 => u32::from_be_bytes(value),
            other => panic!("tag {tag} has type {other}"),
        }
    }
    /// An out-of-line value's bytes (`unit` bytes per count).
    fn data(&self, tag: u16, unit: usize) -> &[u8] {
        let (_, count, value) = self.entries[&tag];
        let at = u32::from_be_bytes(value) as usize;
        assert_eq!(at % 2, 0, "tag {tag} starts on a word boundary");
        &self.bytes[at..at + count as usize * unit]
    }
}

/// The body of the `key` block inside tag 37724's value.
fn block<'a>(value: &'a [u8], key: &[u8; 4]) -> &'a [u8] {
    assert_eq!(&value[..HEADER.len()], HEADER);
    let rest = &value[HEADER.len()..];
    assert_eq!(&rest[..4], b"8BIM");
    assert_eq!(&rest[4..8], key);
    let length = u32::from_be_bytes(rest[8..12].try_into().unwrap()) as usize;
    assert_eq!(value.len() % 4, 0, "the block is padded to four");
    &rest[12..12 + length]
}

// ------------------------------------------------------ the TIFF builder --

/// What one hand-built TIFF declares. Every such file carries a tag 37724
/// holding a well-formed block keyed `key` with an empty layer list.
#[derive(Clone, Copy)]
struct Shape {
    little: bool,
    photometric: u16,
    samples: u16,
    bits: u16,
    format: Option<u16>,
    key: [u8; 4],
}

const RGB8: Shape = Shape {
    little: false,
    photometric: 2,
    samples: 3,
    bits: 8,
    format: None,
    key: *b"Layr",
};

fn built(shape: Shape) -> Vec<u8> {
    let u16b = |v: u16| {
        if shape.little {
            v.to_le_bytes()
        } else {
            v.to_be_bytes()
        }
    };
    let u32b = |v: u32| {
        if shape.little {
            v.to_le_bytes()
        } else {
            v.to_be_bytes()
        }
    };
    let mut out = if shape.little {
        b"II*\0".to_vec()
    } else {
        b"MM\0*".to_vec()
    };
    out.extend_from_slice(&[0; 4]);
    let strip = out.len() as u32;
    let strip_len = 2 * 2 * u32::from(shape.samples) * u32::from(shape.bits / 8);
    out.extend(std::iter::repeat_n(7u8, strip_len as usize));
    let array = |out: &mut Vec<u8>, value: u16| -> u32 {
        let at = out.len() as u32;
        for _ in 0..shape.samples {
            out.extend_from_slice(&u16b(value));
        }
        at
    };
    let bits_at = array(&mut out, shape.bits);
    let format_at = shape.format.map(|format| array(&mut out, format));
    let block_at = out.len() as u32;
    out.extend_from_slice(HEADER);
    // Photoshop writes a little-endian file's codes byte-reversed: `MIB8`,
    // `23rL` (take1's `layered32.tif`).
    out.extend_from_slice(if shape.little { b"MIB8" } else { b"8BIM" });
    let mut key = shape.key;
    if shape.little {
        key.reverse();
    }
    out.extend_from_slice(&key);
    out.extend_from_slice(&u32b(2));
    out.extend_from_slice(&[0, 0, 0, 0]); // an empty layer list, padded to 4
    let block_len = out.len() as u32 - block_at;
    let directory = out.len() as u32;
    // Arrays of one or two SHORTs sit in the entry itself.
    let inline = |value: u16| {
        let mut bytes = [0u8; 4];
        for slot in 0..usize::from(shape.samples.min(2)) {
            bytes[slot * 2..slot * 2 + 2].copy_from_slice(&u16b(value));
        }
        bytes
    };
    let short = |value: u16| {
        let b = u16b(value);
        [b[0], b[1], 0, 0]
    };
    let mut entries: Vec<(u16, u16, u32, [u8; 4])> = vec![
        (256, 4, 1, u32b(2)),
        (257, 4, 1, u32b(2)),
        (
            258,
            3,
            u32::from(shape.samples),
            if shape.samples > 2 {
                u32b(bits_at)
            } else {
                inline(shape.bits)
            },
        ),
        (259, 3, 1, short(1)),
        (262, 3, 1, short(shape.photometric)),
        (273, 4, 1, u32b(strip)),
        (277, 3, 1, short(shape.samples)),
        (278, 4, 1, u32b(2)),
        (279, 4, 1, u32b(strip_len)),
    ];
    if let (Some(format), Some(at)) = (shape.format, format_at) {
        entries.push((
            339,
            3,
            u32::from(shape.samples),
            if shape.samples > 2 {
                u32b(at)
            } else {
                inline(format)
            },
        ));
    }
    entries.push((37724, 7, block_len, u32b(block_at)));
    out.extend_from_slice(&u16b(entries.len() as u16));
    for (tag, kind, count, value) in entries {
        out.extend_from_slice(&u16b(tag));
        out.extend_from_slice(&u16b(kind));
        out.extend_from_slice(&u32b(count));
        out.extend_from_slice(&value);
    }
    out.extend_from_slice(&[0; 4]);
    let offset = u32b(directory);
    out[4..8].copy_from_slice(&offset);
    out
}

fn has_layers(directory: &TempDir, name: &str, bytes: &[u8]) -> bool {
    let path = directory.path().join(name);
    std::fs::write(&path, bytes).unwrap();
    unsafe { dr_tiff_has_layers(cpath(&path).as_ptr()) }
}

// ------------------------------------------------------------- documents --

fn bytes8(pixels: &[[u16; 4]]) -> Vec<u8> {
    pixels.iter().flatten().map(|v| *v as u8).collect()
}

fn byte_document(file: &File) -> DrDocument {
    let image = |index: usize, w: u32, h: u32| {
        RgbaImage::from_raw(w, h, bytes8(&file.layers[index].rgba)).unwrap()
    };
    let mut document = DrDocument::from_pixels(image(0, W, H))
        .adding_image_layer(0, image(1, 4, 2), "Upper")
        .expect("add layer");
    document.layers[0].name = "Lower".into();
    document.layers[1].offset = (3, 1);
    document
}

fn typed_document(file: &File) -> PrecisionDocument {
    let profile = IccProfile::srgb();
    let image = |index: usize, w: u32, h: u32| {
        PrecisionImage::from_rgba16(
            w,
            h,
            file.layers[index].rgba.clone(),
            Arc::clone(&profile),
            None,
        )
        .unwrap()
    };
    PrecisionDocument::from_parts(
        W,
        H,
        Arc::clone(&profile),
        Metadata::default(),
        Resolution::default(),
        vec![
            PrecisionDocumentLayer::raster("Lower", image(0, W, H), (0, 0), 1.0, true).unwrap(),
            PrecisionDocumentLayer::raster("Upper", image(1, 4, 2), (3, 1), 1.0, true).unwrap(),
        ],
    )
    .unwrap()
}

/// The written file against the TIFF layout and Photoshop's three tags.
fn check_written(bytes: &[u8], file: &File, depth: u16) {
    let tiff = directory(bytes);
    let sample_bytes = usize::from(depth / 8);
    assert_eq!((tiff.scalar(256), tiff.scalar(257)), (W, H));
    assert_eq!(tiff.scalar(259), 1, "uncompressed");
    assert_eq!(tiff.scalar(262), 2, "RGB");
    assert_eq!(tiff.scalar(277), 4, "four samples");
    assert_eq!(tiff.scalar(284), 1, "chunky");
    assert_eq!(tiff.scalar(338), 2, "unassociated alpha");
    assert_eq!(tiff.scalar(278), H, "one strip");
    let bits: Vec<u16> = tiff
        .data(258, 2)
        .chunks_exact(2)
        .map(|b| u16::from_be_bytes([b[0], b[1]]))
        .collect();
    assert_eq!(bits, [depth; 4]);
    // The strip is the flat picture: straight RGBA, big-endian samples.
    let strip = tiff.scalar(273) as usize;
    let length = tiff.scalar(279) as usize;
    let expected: Vec<u8> = file
        .composite
        .iter()
        .flatten()
        .flat_map(|v| {
            if sample_bytes == 1 {
                vec![*v as u8]
            } else {
                v.to_be_bytes().to_vec()
            }
        })
        .collect();
    assert_eq!(&bytes[strip..strip + length], expected.as_slice());
    assert_eq!(tiff.data(34675, 1), &IccProfile::srgb().bytes()[..]);
    // 72 pixels per inch as a RATIONAL.
    let x = tiff.data(282, 8);
    let ratio = f64::from(u32::from_be_bytes(x[..4].try_into().unwrap()))
        / f64::from(u32::from_be_bytes(x[4..].try_into().unwrap()));
    assert_eq!(ratio, f64::from(Resolution::default().x));

    let resources = fixture::parse_resources(tiff.data(34377, 1));
    let ids: Vec<u16> = resources.iter().map(|(id, _)| *id).collect();
    assert_eq!(ids, [1039, 1005], "the profile and the resolution");
    let key = if depth == 8 { b"Layr" } else { b"Lr16" };
    let layers = fixture::parse_layer_info(block(tiff.data(37724, 1), key), false);
    assert_eq!(layers.len(), 2);
    for (written, expected) in layers.iter().zip(&file.layers) {
        assert_eq!(written.name, expected.name);
        assert_eq!(written.bounds, expected.bounds);
        assert_eq!(written.rgba(sample_bytes).as_ref(), Some(&expected.rgba));
    }
}

/// The ordinary flat open of `path`, as straight RGBA8.
fn flat_open(path: &std::path::Path) -> Vec<u8> {
    let mut error = ptr::null_mut();
    let image = unsafe { doctored_core::ffi::dr_image_open(cpath(path).as_ptr(), &mut error) };
    assert!(!image.is_null(), "{}", take_err_string(error));
    let pixels = unsafe {
        let (w, h) = (
            doctored_core::ffi::dr_image_width(image),
            doctored_core::ffi::dr_image_height(image),
        );
        std::slice::from_raw_parts(
            doctored_core::ffi::dr_image_pixels_rgba(image),
            w as usize * h as usize * 4,
        )
        .to_vec()
    };
    unsafe { doctored_core::ffi::dr_image_free(image) };
    pixels
}

// ----------------------------------------------------------------- tests --

#[test]
fn an_eight_bit_layered_tiff_is_a_flat_tiff_that_also_opens_as_its_layers() {
    let directory = TempDir::new().unwrap();
    let file = two_layers(1, 8, 0);
    let document = byte_document(&file);
    let report = fixture::export_report8(&document, TIFF, LAYERS);
    assert_eq!(report["format"], Value::from("TIFF"));
    assert_eq!(report["container"], Value::from("TIFF"));
    assert_eq!(report["available"], Value::Bool(true), "{report}");
    assert!(fixture::strings(&report, "general_limitations")[0].contains("8-bit RGB TIFF"));
    let path = directory.path().join("out.tif");
    fixture::save8(&document, &path, TIFF, LAYERS).expect("save");
    let bytes = std::fs::read(&path).unwrap();
    check_written(&bytes, &file, 8);

    // Anything that ignores Photoshop's tags sees the flat picture.
    assert_eq!(flat_open(&path), bytes8(&file.composite));

    assert!(unsafe { dr_tiff_has_layers(cpath(&path).as_ptr()) });
    let notice = fixture::import_report(&path);
    assert_eq!(notice["format"], Value::from("TIFF"), "{notice}");
    assert_eq!(notice["layered_available"], Value::Bool(true), "{notice}");
    assert_eq!(notice["layer_count"], Value::from(2));
    assert_eq!(notice["stored_composite_available"], Value::Bool(false));
    assert_eq!(
        notice["stored_composite_refusal"],
        Value::from(NO_COMPOSITE)
    );
    assert_eq!(notice["profile"], Value::from(IccProfile::srgb().name()));

    let layered = fixture::open8(&path, LAYERS).expect("layers");
    let names: Vec<&str> = layered.layers.iter().map(|l| l.name.as_str()).collect();
    assert_eq!(names, ["Lower", "Upper"]);
    assert_eq!(layered.layers[1].offset, (3, 1));
    assert_eq!(
        layered.flattened().unwrap().into_raw(),
        bytes8(&file.composite)
    );
    let refused = fixture::open8(&path, FLATTENED)
        .err()
        .expect("a TIFF has no stored composite");
    assert!(refused.contains(NO_COMPOSITE), "{refused}");

    // Flattened mode: the same file with one record.
    let flat = directory.path().join("flat.tif");
    fixture::save8(&document, &flat, TIFF, FLATTENED).expect("save flattened");
    let bytes = std::fs::read(&flat).unwrap();
    let layers = fixture::parse_layer_info(block(directory_block(&bytes), b"Layr"), false);
    assert_eq!(layers.len(), 1);
    assert_eq!(layers[0].rgba(1).unwrap(), file.composite);
}

fn directory_block(bytes: &[u8]) -> &[u8] {
    let tiff = directory(bytes);
    let (_, count, value) = tiff.entries[&37724];
    let at = u32::from_be_bytes(value) as usize;
    &bytes[at..at + count as usize]
}

#[test]
fn a_sixteen_bit_layered_tiff_round_trips_as_a_typed_document() {
    let directory = TempDir::new().unwrap();
    let file = two_layers(1, 16, 0);
    let document = typed_document(&file);
    let report = fixture::export_report16(&document, TIFF, LAYERS);
    assert_eq!(report["format"], Value::from("TIFF"));
    assert_eq!(report["depth"], Value::from(16));
    assert_eq!(report["available"], Value::Bool(true), "{report}");
    let path = directory.path().join("out16.tif");
    fixture::save16(&document, &path, TIFF, LAYERS).expect("save");
    check_written(&std::fs::read(&path).unwrap(), &file, 16);

    assert!(unsafe { dr_tiff_has_layers(cpath(&path).as_ptr()) });
    let notice = fixture::import_report(&path);
    assert_eq!(notice["format"], Value::from("TIFF"));
    assert_eq!(notice["import_precision"], Value::from("rgba16"));
    assert_eq!(notice["layered_available"], Value::Bool(true), "{notice}");
    assert_eq!(notice["stored_composite_available"], Value::Bool(false));
    let layered = fixture::open16(&path, LAYERS).expect("layers");
    assert_eq!(fixture::rendered16(&layered), file.composite);
    let refused = fixture::open16(&path, FLATTENED)
        .err()
        .expect("a TIFF has no stored composite");
    assert!(refused.contains(NO_COMPOSITE), "{refused}");
}

#[test]
fn only_a_big_endian_tiff_at_eight_or_sixteen_bits_or_a_float_rgb_tiff_at_32_has_layers() {
    let directory = TempDir::new().unwrap();
    // The builder's own positive cases: the refusals below are about the
    // declared colour model, not about how the file is laid out.
    assert!(has_layers(&directory, "rgb8.tif", &built(RGB8)));
    assert!(has_layers(
        &directory,
        "rgb16.tif",
        &built(Shape {
            bits: 16,
            key: *b"Lr16",
            ..RGB8
        })
    ));
    assert!(has_layers(
        &directory,
        "rgba8.tif",
        &built(Shape {
            samples: 4,
            format: Some(1),
            ..RGB8
        })
    ));
    // #547 phase 3: a separated (CMYK, InkSet absent) first directory's
    // records are a layered CMYK file's.
    assert!(has_layers(
        &directory,
        "cmyk8.tif",
        &built(Shape {
            photometric: 5,
            samples: 4,
            ..RGB8
        })
    ));
    // #547 phase 3a: a BlackIsZero first directory's records are a layered
    // Grayscale file's, with or without its alpha sample.
    assert!(has_layers(
        &directory,
        "gray.tif",
        &built(Shape {
            photometric: 1,
            samples: 1,
            ..RGB8
        })
    ));
    assert!(has_layers(
        &directory,
        "gray_alpha.tif",
        &built(Shape {
            photometric: 1,
            samples: 2,
            ..RGB8
        })
    ));
    // #547 Lab phase 3a: a CIELab first directory's records are a layered
    // Lab file's.
    assert!(has_layers(
        &directory,
        "lab.tif",
        &built(Shape {
            photometric: 8,
            ..RGB8
        })
    ));
    // A 32-bit IEEE-float RGB file's `Lr32` records, in either byte
    // order (Photoshop's own `layered32.tif` is little-endian).
    let float32 = Shape {
        bits: 32,
        format: Some(3),
        key: *b"Lr32",
        ..RGB8
    };
    assert!(has_layers(&directory, "float32_mm.tif", &built(float32)));
    assert!(has_layers(
        &directory,
        "float32_ii.tif",
        &built(Shape {
            little: true,
            ..float32
        })
    ));

    for (name, shape) in [
        // A separated file short of its four inks.
        (
            "cmyk3",
            Shape {
                photometric: 5,
                samples: 3,
                ..RGB8
            },
        ),
        (
            "palette",
            Shape {
                photometric: 3,
                samples: 1,
                ..RGB8
            },
        ),
        (
            "little",
            Shape {
                little: true,
                ..RGB8
            },
        ),
        // A 32-bit float file whose block is another depth's, and a 32-bit
        // float gray file: the float form is RGB `Lr32` only.
        (
            "float_lr16",
            Shape {
                bits: 32,
                format: Some(3),
                key: *b"Lr16",
                ..RGB8
            },
        ),
        (
            "float_gray",
            Shape {
                photometric: 1,
                samples: 1,
                bits: 32,
                format: Some(3),
                key: *b"Lr32",
                ..RGB8
            },
        ),
        (
            "little16",
            Shape {
                little: true,
                bits: 16,
                key: *b"Lr16",
                ..RGB8
            },
        ),
        (
            "float16",
            Shape {
                bits: 16,
                format: Some(3),
                key: *b"Lr16",
                ..RGB8
            },
        ),
        (
            "signed",
            Shape {
                format: Some(2),
                ..RGB8
            },
        ),
        (
            "rgb32",
            Shape {
                bits: 32,
                key: *b"Lr32",
                ..RGB8
            },
        ),
        // The block for the OTHER depth is not this file's layers.
        (
            "wrong_key",
            Shape {
                key: *b"Lr16",
                ..RGB8
            },
        ),
    ] {
        assert!(
            !has_layers(&directory, &format!("{name}.tif"), &built(shape)),
            "{name} must open flat"
        );
    }

    // No tag 37724 at all, a truncated file, a missing file, and NULL.
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
    }
    assert!(!unsafe { dr_tiff_has_layers(cpath(&plain).as_ptr()) });
    let whole = built(RGB8);
    assert!(!has_layers(
        &directory,
        "cut.tif",
        &whole[..whole.len() - 20]
    ));
    assert!(!has_layers(&directory, "empty.tif", &[]));
    assert!(!unsafe { dr_tiff_has_layers(cpath(&directory.path().join("absent.tif")).as_ptr()) });
    assert!(!unsafe { dr_tiff_has_layers(ptr::null()) });
}

#[test]
fn a_tiff_the_locator_rejects_is_not_a_layered_file_to_the_importer() {
    let directory = TempDir::new().unwrap();
    let path = directory.path().join("palette.tif");
    std::fs::write(
        &path,
        built(Shape {
            photometric: 3,
            samples: 1,
            ..RGB8
        }),
    )
    .unwrap();
    let mut error = ptr::null_mut();
    let json =
        unsafe { doctored_core::ffi_psd::dr_psd_import_report(cpath(&path).as_ptr(), &mut error) };
    assert!(json.is_null());
    assert!(
        take_err_string(error).contains("this TIFF has no readable Photoshop layer block"),
        "a palette TIFF is refused by name rather than imported as RGB"
    );

    // #547 Lab phase 3a: a layered Lab TIFF is a Lab file to the importer.
    let path = directory.path().join("lab.tif");
    std::fs::write(
        &path,
        built(Shape {
            photometric: 8,
            ..RGB8
        }),
    )
    .unwrap();
    let notice = fixture::import_report(&path);
    assert_eq!(notice["import_precision"], Value::from("lab"), "{notice}");

    // A layered RGB TIFF whose block retains nothing has no layered choice
    // either: its report says so, and the ordinary open is the way in.
    let path = directory.path().join("empty.tif");
    std::fs::write(&path, built(RGB8)).unwrap();
    let notice = fixture::import_report(&path);
    assert_eq!(notice["format"], Value::from("TIFF"));
    assert_eq!(notice["stored_composite_available"], Value::Bool(false));
    assert_eq!(notice["layered_available"], Value::Bool(false), "{notice}");
    assert_eq!(notice["layered_refusal"], Value::from(NO_COMPOSITE));
}
