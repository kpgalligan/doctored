//! BigTIFF input through the preserving open: a 16-bit or float
//! BigTIFF opens exactly as its classic twin does — the same samples, the
//! same ICC profile, the same XMP packet and the same resolution.
//!
//! Both twins come from the `tiff` crate's encoder (`TiffEncoder::new_big`
//! and `TiffEncoder::new`) with identical tags and samples, so the only
//! difference between the two files is the container. The expectation is
//! the classic twin plus the samples written here, never a golden image.
use crate::common;
use common::take_err_string;

use doctored_core::ffi_float::dr_precision_doc_is_float;
use doctored_core::ffi_precision::{
    dr_precision_doc_free, dr_precision_doc_open, dr_precision_doc_render, dr_precision_image_free,
};
use doctored_core::icc::IccProfile;
use doctored_core::precision_document::PrecisionDocument;

use std::ffi::{c_char, CString};
use std::io::Cursor;
use std::ptr;
use tempfile::TempDir;
use tiff::encoder::{colortype, Rational, TiffEncoder};
use tiff::tags::Tag;

const LIMIT: u64 = 2 * 1024 * 1024 * 1024;
const W: u32 = 8;
const H: u32 = 6;
const XMP: &[u8] = b"<x:xmpmeta xmlns:x='adobe:ns:meta/'><rdf:RDF xmlns:rdf='http://www.w3.org/1999/02/22-rdf-syntax-ns#'><rdf:Description xmlns:dc='http://purl.org/dc/elements/1.1/'><dc:creator>BigTIFF twin</dc:creator></rdf:Description></rdf:RDF></x:xmpmeta>";

/// Writes one image with the shared tags — Display P3 ICC (34675), XMP
/// (700) and 300 ppi — through whichever encoder kind `$encoder` is.
macro_rules! write_twin {
    ($encoder:expr, $color:ty, $samples:expr) => {{
        let mut encoder = $encoder;
        let mut image = encoder.new_image::<$color>(W, H).unwrap();
        let tags = image.encoder();
        let p3 = IccProfile::display_p3();
        tags.write_tag(Tag::Unknown(34675), &p3.bytes()[..])
            .unwrap();
        tags.write_tag(Tag::Unknown(700), XMP).unwrap();
        tags.write_tag(Tag::XResolution, Rational { n: 300, d: 1 })
            .unwrap();
        tags.write_tag(Tag::YResolution, Rational { n: 300, d: 1 })
            .unwrap();
        tags.write_tag(Tag::ResolutionUnit, 2u16).unwrap();
        image.write_data($samples).unwrap();
    }};
}

fn rgb16(big: bool, samples: &[u16]) -> Vec<u8> {
    let mut out = Cursor::new(Vec::new());
    if big {
        write_twin!(
            TiffEncoder::new_big(&mut out).unwrap(),
            colortype::RGB16,
            samples
        );
    } else {
        write_twin!(
            TiffEncoder::new(&mut out).unwrap(),
            colortype::RGB16,
            samples
        );
    }
    out.into_inner()
}

fn rgb32f(big: bool, samples: &[f32]) -> Vec<u8> {
    let mut out = Cursor::new(Vec::new());
    if big {
        write_twin!(
            TiffEncoder::new_big(&mut out).unwrap(),
            colortype::RGB32Float,
            samples
        );
    } else {
        write_twin!(
            TiffEncoder::new(&mut out).unwrap(),
            colortype::RGB32Float,
            samples
        );
    }
    out.into_inner()
}

fn open(dir: &TempDir, bytes: &[u8], name: &str) -> *mut PrecisionDocument {
    let path = dir.path().join(name);
    std::fs::write(&path, bytes).expect("write fixture");
    let c = CString::new(path.to_str().unwrap()).expect("path");
    let mut err: *mut c_char = ptr::null_mut();
    let document = unsafe { dr_precision_doc_open(c.as_ptr(), LIMIT, 0, ptr::null(), &mut err) };
    assert!(!document.is_null(), "{name}: {}", take_err_string(err));
    document
}

/// The document's RGBA16 samples, through the shipped renderer.
fn pixels(document: *const PrecisionDocument) -> Vec<[u16; 4]> {
    let mut err: *mut c_char = ptr::null_mut();
    let image = unsafe { dr_precision_doc_render(document, LIMIT, 0, ptr::null(), &mut err) };
    assert!(!image.is_null(), "{}", take_err_string(err));
    let samples = unsafe { &*image }.to_rgba16().expect("rgba16 storage");
    unsafe { dr_precision_image_free(image) };
    samples
}

/// What the twins must agree on besides the samples.
fn sidecar(document: *const PrecisionDocument) -> (Vec<u8>, Option<Vec<u8>>, (f32, f32)) {
    let document = unsafe { &*document };
    let resolution = document.resolution();
    (
        document.profile().bytes().to_vec(),
        document.metadata().xmp.as_deref().map(<[u8]>::to_vec),
        (resolution.x, resolution.y),
    )
}

#[test]
fn a_16_bit_bigtiff_opens_exactly_as_its_classic_twin() {
    let samples: Vec<u16> = (0..(W * H * 3)).map(|i| (i * 1361 + 17) as u16).collect();
    let big = rgb16(true, &samples);
    assert_eq!(&big[..4], b"II\x2b\0", "the fixture is a BigTIFF");
    let classic = rgb16(false, &samples);
    assert_eq!(&classic[..4], b"II\x2a\0");
    let dir = TempDir::new().unwrap();
    let from_big = open(&dir, &big, "big.tif");
    let from_classic = open(&dir, &classic, "classic.tif");
    unsafe {
        assert!(!dr_precision_doc_is_float(from_big));
        assert!(!dr_precision_doc_is_float(from_classic));
    }
    let rendered = pixels(from_big);
    assert_eq!(rendered, pixels(from_classic));
    let expected: Vec<[u16; 4]> = samples
        .chunks_exact(3)
        .map(|p| [p[0], p[1], p[2], 65535])
        .collect();
    assert_eq!(rendered, expected);
    let (profile, xmp, resolution) = sidecar(from_big);
    assert_eq!(
        (profile.clone(), xmp.clone(), resolution),
        sidecar(from_classic)
    );
    assert_eq!(profile, IccProfile::display_p3().bytes().to_vec());
    assert_eq!(xmp.as_deref(), Some(XMP));
    assert_eq!(resolution, (300.0, 300.0));
    unsafe {
        dr_precision_doc_free(from_big);
        dr_precision_doc_free(from_classic);
    }
}

#[test]
fn a_float_bigtiff_opens_as_float_with_its_classic_twins_packets() {
    let samples: Vec<f32> = (0..(W * H * 3))
        .map(|i| (i as f32) / (W * H * 3) as f32)
        .collect();
    let big = rgb32f(true, &samples);
    assert_eq!(&big[..4], b"II\x2b\0", "the fixture is a BigTIFF");
    let classic = rgb32f(false, &samples);
    let dir = TempDir::new().unwrap();
    let from_big = open(&dir, &big, "big-float.tif");
    let from_classic = open(&dir, &classic, "classic-float.tif");
    unsafe {
        assert!(dr_precision_doc_is_float(from_big));
        assert!(dr_precision_doc_is_float(from_classic));
    }
    let (_, xmp, resolution) = sidecar(from_big);
    let (_, classic_xmp, classic_resolution) = sidecar(from_classic);
    assert_eq!(xmp, classic_xmp);
    assert_eq!(xmp.as_deref(), Some(XMP));
    assert_eq!(resolution, classic_resolution);
    assert_eq!(resolution, (300.0, 300.0));
    unsafe {
        dr_precision_doc_free(from_big);
        dr_precision_doc_free(from_classic);
    }
}
