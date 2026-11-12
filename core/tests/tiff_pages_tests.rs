//! A multipage TIFF opens every page as a layer at 8, 16 and 32 bits
//! (`tiff_pages`, `doc_tiff_pages`).
//!
//! Every file here is hand-assembled by the writer below — the header, each
//! IFD, each uncompressed strip — so a decode that agrees with an expectation
//! agrees with arithmetic written in this file, not with the `tiff` crate's
//! encoder. Every open goes through the FFI (`dr_doc_open`,
//! `dr_precision_doc_open`, `dr_path_metadata_import_report`); the typed
//! documents' records are read back through the public `PrecisionDocument`
//! accessors.

use crate::cmyk_fixture::press_profile;
use crate::common;
use crate::gray_fixture::gray_icc;
use common::{cpath, layer_dims, layer_name, layer_offset, layer_pixels, take_err_string};

use doctored_core::doc::DrDocument;
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_bitmap::{dr_path_bitmap_source, dr_precision_doc_open_bitmap};
use doctored_core::ffi_cmyk::dr_path_cmyk_source;
use doctored_core::ffi_doc::{
    dr_doc_free, dr_doc_height, dr_doc_layer_count, dr_doc_layer_visible, dr_doc_open, dr_doc_width,
};
use doctored_core::ffi_gray::{dr_path_gray_source, dr_precision_doc_open_gray};
use doctored_core::ffi_indexed::{dr_path_indexed_source, dr_precision_doc_open_indexed};
use doctored_core::ffi_lab::{dr_path_lab_source, dr_precision_doc_open_lab};
use doctored_core::ffi_metadata_report::dr_path_metadata_import_report;
use doctored_core::ffi_precision::{dr_precision_doc_free, dr_precision_doc_open};
use doctored_core::precision_document::PrecisionDocument;

use std::ffi::{c_char, CStr, CString};
use std::path::{Path, PathBuf};
use std::ptr;
use tempfile::TempDir;

const LIMIT: u64 = 2 * 1024 * 1024 * 1024;

// ------------------------------------------------------------- the writer --

/// One IFD as the writer lays it out: an uncompressed single strip unless
/// `compression` says otherwise.
#[derive(Clone)]
struct Page {
    width: u32,
    height: u32,
    bits: u16,
    samples: u16,
    format: u16,
    photometric: u16,
    extras: Vec<u16>,
    data: Vec<u8>,
    new_subfile: Option<u32>,
    icc: Option<Vec<u8>>,
    exif_ifd: bool,
    compression: u16,
    /// ImageWidth and ImageLength as LONG8 (type 16), which only a BigTIFF
    /// may use.
    long8_size: bool,
    /// A palette page's ColorMap (320): 3 · 2^bits SHORTs.
    color_map: Option<Vec<u16>>,
}

impl Page {
    fn new(width: u32, height: u32, bits: u16, samples: u16, format: u16, data: Vec<u8>) -> Self {
        let extras = if samples == 4 { vec![2] } else { vec![] };
        Page {
            width,
            height,
            bits,
            samples,
            format,
            photometric: 2,
            extras,
            data,
            new_subfile: None,
            icc: None,
            exif_ifd: false,
            compression: 1,
            long8_size: false,
            color_map: None,
        }
    }
}

/// One IFD entry before layout: tag, type, count and its value bytes.
struct Field(u16, u16, u64, Vec<u8>);

fn shorts(values: &[u16]) -> Vec<u8> {
    values.iter().flat_map(|v| v.to_le_bytes()).collect()
}

/// Lays `pages` out as one little-endian TIFF (BigTIFF when `big`): every
/// strip and out-of-line value first, then the IFDs in chain order, each
/// linking to the next and the last to 0. Returns the file and, per IFD,
/// (its offset, the offset of its next-IFD link).
fn write_tiff(pages: &[Page], big: bool) -> (Vec<u8>, Vec<(usize, usize)>) {
    let (head, entry, field) = if big { (16, 20, 8) } else { (8, 12, 4) };
    let offset_bytes = |v: u64| -> Vec<u8> {
        if big {
            v.to_le_bytes().to_vec()
        } else {
            (v as u32).to_le_bytes().to_vec()
        }
    };
    let mut out = vec![0u8; head];
    // A minimal Exif IFD every exif_ifd page points at: no entries.
    let exif_at = out.len() as u64;
    if big {
        out.extend_from_slice(&0u64.to_le_bytes());
        out.extend_from_slice(&0u64.to_le_bytes());
    } else {
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
    }
    let mut directories: Vec<Vec<Field>> = Vec::new();
    for page in pages {
        while out.len() % 2 != 0 {
            out.push(0);
        }
        let strip = out.len() as u64;
        out.extend_from_slice(&page.data);
        let long = |v: u64| {
            if big {
                (16u16, offset_bytes(v))
            } else {
                (4u16, offset_bytes(v))
            }
        };
        let size = |v: u32| {
            if big && page.long8_size {
                (16u16, u64::from(v).to_le_bytes().to_vec())
            } else {
                (4u16, v.to_le_bytes().to_vec())
            }
        };
        let mut fields = vec![
            Field(256, size(page.width).0, 1, size(page.width).1),
            Field(257, size(page.height).0, 1, size(page.height).1),
            Field(
                258,
                3,
                u64::from(page.samples),
                shorts(&vec![page.bits; usize::from(page.samples)]),
            ),
            Field(259, 3, 1, shorts(&[page.compression])),
            Field(262, 3, 1, shorts(&[page.photometric])),
            Field(273, long(0).0, 1, long(strip).1),
            Field(277, 3, 1, shorts(&[page.samples])),
            Field(278, 4, 1, page.height.to_le_bytes().to_vec()),
            Field(279, long(0).0, 1, long(page.data.len() as u64).1),
            Field(284, 3, 1, shorts(&[1])),
            Field(
                339,
                3,
                u64::from(page.samples),
                shorts(&vec![page.format; usize::from(page.samples)]),
            ),
        ];
        if let Some(kind) = page.new_subfile {
            fields.push(Field(254, 4, 1, kind.to_le_bytes().to_vec()));
        }
        if !page.extras.is_empty() {
            fields.push(Field(
                338,
                3,
                page.extras.len() as u64,
                shorts(&page.extras),
            ));
        }
        if let Some(icc) = &page.icc {
            fields.push(Field(34675, 7, icc.len() as u64, icc.clone()));
        }
        if page.exif_ifd {
            fields.push(Field(34665, 4, 1, (exif_at as u32).to_le_bytes().to_vec()));
        }
        if let Some(map) = &page.color_map {
            fields.push(Field(320, 3, map.len() as u64, shorts(map)));
        }
        fields.sort_by_key(|f| f.0);
        // Out-of-line values go into the body now; their field becomes the
        // offset.
        for f in fields.iter_mut() {
            if f.3.len() > field {
                while out.len() % 2 != 0 {
                    out.push(0);
                }
                let at = out.len() as u64;
                out.extend_from_slice(&f.3);
                f.3 = offset_bytes(at);
            }
        }
        directories.push(fields);
    }
    while out.len() % 2 != 0 {
        out.push(0);
    }
    let mut placed = Vec::new();
    let mut at = out.len();
    let count_bytes = if big { 8 } else { 2 };
    for fields in &directories {
        placed.push((at, at + count_bytes + entry * fields.len()));
        at += count_bytes + entry * fields.len() + field;
    }
    for (i, fields) in directories.iter().enumerate() {
        if big {
            out.extend_from_slice(&(fields.len() as u64).to_le_bytes());
        } else {
            out.extend_from_slice(&(fields.len() as u16).to_le_bytes());
        }
        for f in fields {
            out.extend_from_slice(&f.0.to_le_bytes());
            out.extend_from_slice(&f.1.to_le_bytes());
            if big {
                out.extend_from_slice(&f.2.to_le_bytes());
            } else {
                out.extend_from_slice(&(f.2 as u32).to_le_bytes());
            }
            let mut value = f.3.clone();
            value.resize(field, 0);
            out.extend_from_slice(&value);
        }
        let next = placed.get(i + 1).map_or(0, |p| p.0 as u64);
        out.extend_from_slice(&offset_bytes(next));
    }
    out[0..2].copy_from_slice(b"II");
    if big {
        out[2..4].copy_from_slice(&43u16.to_le_bytes());
        out[4..6].copy_from_slice(&8u16.to_le_bytes());
        out[6..8].copy_from_slice(&0u16.to_le_bytes());
        out[8..16].copy_from_slice(&(placed[0].0 as u64).to_le_bytes());
    } else {
        out[2..4].copy_from_slice(&42u16.to_le_bytes());
        out[4..8].copy_from_slice(&(placed[0].0 as u32).to_le_bytes());
    }
    (out, placed)
}

// ------------------------------------------------------------- the values --

/// Page `p`'s 8-bit sample at (x, y, c): distinct per page, per channel and
/// per pixel, with no zero alpha.
fn v8(p: u32, x: u32, y: u32, c: u32) -> u8 {
    if c == 3 {
        (128 + (x * 13 + y * 7 + p * 5) % 120) as u8
    } else {
        ((x * 31 + y * 17 + c * 53 + p * 71) % 256) as u8
    }
}

fn v16(p: u32, x: u32, y: u32, c: u32) -> u16 {
    if c == 3 {
        (40000 + (x * 997 + y * 431 + p * 89) % 25000) as u16
    } else {
        ((u64::from(x) * 7919
            + u64::from(y) * 104_729
            + u64::from(c) * 1_299_709
            + u64::from(p) * 15_485_863)
            % 65536) as u16
    }
}

/// Above one and below zero, so a clamped or byte-reduced decode shows.
fn vf(p: u32, x: u32, y: u32, c: u32) -> f32 {
    p as f32 * 1.5 + x as f32 * 0.125 - y as f32 * 0.0625 + c as f32 * 0.5 - 0.25
}

fn page8(p: u32, w: u32, h: u32, samples: u16) -> Page {
    let mut data = Vec::new();
    for y in 0..h {
        for x in 0..w {
            for c in 0..u32::from(samples) {
                data.push(v8(p, x, y, c));
            }
        }
    }
    Page::new(w, h, 8, samples, 1, data)
}

fn page16(p: u32, w: u32, h: u32, samples: u16) -> Page {
    let mut data = Vec::new();
    for y in 0..h {
        for x in 0..w {
            for c in 0..u32::from(samples) {
                data.extend_from_slice(&v16(p, x, y, c).to_le_bytes());
            }
        }
    }
    Page::new(w, h, 16, samples, 1, data)
}

fn page32(p: u32, w: u32, h: u32) -> Page {
    let mut data = Vec::new();
    for y in 0..h {
        for x in 0..w {
            for c in 0..3 {
                data.extend_from_slice(&vf(p, x, y, c).to_le_bytes());
            }
        }
    }
    Page::new(w, h, 32, 3, 3, data)
}

fn expected8(p: u32, w: u32, h: u32, samples: u16) -> Vec<u8> {
    let mut out = Vec::new();
    for y in 0..h {
        for x in 0..w {
            for c in 0..4 {
                out.push(if c == 3 && samples == 3 {
                    255
                } else {
                    v8(p, x, y, c)
                });
            }
        }
    }
    out
}

fn expected16(p: u32, w: u32, h: u32, samples: u16) -> Vec<[u16; 4]> {
    let mut out = Vec::new();
    for y in 0..h {
        for x in 0..w {
            let a = if samples == 3 { 65535 } else { v16(p, x, y, 3) };
            out.push([v16(p, x, y, 0), v16(p, x, y, 1), v16(p, x, y, 2), a]);
        }
    }
    out
}

fn expected32(p: u32, w: u32, h: u32) -> Vec<[f32; 4]> {
    let mut out = Vec::new();
    for y in 0..h {
        for x in 0..w {
            out.push([vf(p, x, y, 0), vf(p, x, y, 1), vf(p, x, y, 2), 1.0]);
        }
    }
    out
}

// ---------------------------------------------------------------- opening --

fn save(dir: &TempDir, name: &str, bytes: &[u8]) -> PathBuf {
    let path = dir.path().join(name);
    std::fs::write(&path, bytes).expect("write fixture");
    path
}

fn open_byte(path: &Path) -> Result<*mut DrDocument, String> {
    let c = cpath(path);
    let mut err: *mut c_char = ptr::null_mut();
    let doc = unsafe { dr_doc_open(c.as_ptr(), &mut err) };
    if doc.is_null() {
        Err(take_err_string(err))
    } else {
        Ok(doc)
    }
}

fn open_typed(path: &Path) -> Result<*mut PrecisionDocument, String> {
    let c = cpath(path);
    let mut err: *mut c_char = ptr::null_mut();
    let doc = unsafe { dr_precision_doc_open(c.as_ptr(), LIMIT, 0, ptr::null(), &mut err) };
    if doc.is_null() {
        Err(take_err_string(err))
    } else {
        Ok(doc)
    }
}

fn byte_layers(doc: *const DrDocument) -> Vec<(String, bool)> {
    let count = unsafe { dr_doc_layer_count(doc) };
    (0..count)
        .map(|i| (layer_name(doc, i), unsafe { dr_doc_layer_visible(doc, i) }))
        .collect()
}

fn typed_layers(doc: *const PrecisionDocument) -> Vec<(String, bool)> {
    let doc = unsafe { &*doc };
    (0..doc.layer_count())
        .map(|i| {
            let layer = doc.layer(i).expect("record");
            (layer.name().to_owned(), layer.visible())
        })
        .collect()
}

fn typed_warnings(doc: *const PrecisionDocument) -> Vec<String> {
    let info = unsafe { &*doc }.info().expect("info");
    info["import_warnings"]
        .as_array()
        .expect("warnings")
        .iter()
        .map(|v| v.as_str().unwrap_or_default().to_owned())
        .collect()
}

fn report(path: &Path) -> serde_json::Value {
    let c = cpath(path);
    let mut err: *mut c_char = ptr::null_mut();
    let raw = unsafe { dr_path_metadata_import_report(c.as_ptr(), &mut err) };
    assert!(!raw.is_null(), "{}", take_err_string(err));
    let text = unsafe { CStr::from_ptr(raw) }
        .to_string_lossy()
        .into_owned();
    unsafe { dr_string_free(raw) };
    serde_json::from_str(&text).expect("report json")
}

fn report_warnings(path: &Path) -> Vec<String> {
    report(path)["warnings"]
        .as_array()
        .expect("warnings")
        .iter()
        .map(|v| v.as_str().unwrap_or_default().to_owned())
        .collect()
}

fn names(list: &[(&str, bool)]) -> Vec<(String, bool)> {
    list.iter().map(|(n, v)| ((*n).to_owned(), *v)).collect()
}

const OPENED: &str = "opened as layers named by page number";
/// The metadata scan's first-image-only sentence, which a file whose every
/// page opens no longer carries.
const FIRST_ONLY: &str = "Only the first TIFF image is imported";

// ------------------------------------------------------------------ tests --

#[test]
fn three_pages_open_as_three_layers_at_8_bits() {
    let dir = TempDir::new().unwrap();
    let (bytes, _) = write_tiff(
        &[page8(1, 6, 4, 3), page8(2, 6, 4, 4), page8(3, 3, 2, 3)],
        false,
    );
    let path = save(&dir, "three8.tif", &bytes);
    let doc = open_byte(&path).expect("open");
    unsafe {
        assert_eq!((dr_doc_width(doc), dr_doc_height(doc)), (6, 4));
    }
    assert_eq!(
        byte_layers(doc),
        names(&[("Page 1", true), ("Page 2", false), ("Page 3", false)])
    );
    assert_eq!(layer_pixels(doc, 0), expected8(1, 6, 4, 3));
    assert_eq!(layer_pixels(doc, 1), expected8(2, 6, 4, 4));
    // A smaller page keeps its own size at the canvas's top-left.
    assert_eq!(layer_dims(doc, 2), (3, 2));
    assert_eq!(layer_offset(doc, 2), (0, 0));
    assert_eq!(layer_pixels(doc, 2), expected8(3, 3, 2, 3));
    // The byte open's channel is the import report.
    let warnings = report_warnings(&path);
    assert!(warnings.iter().any(|w| w.contains(OPENED)), "{warnings:?}");
    assert!(
        warnings
            .iter()
            .any(|w| w.contains("Page 3") && w.contains("top-left")),
        "{warnings:?}"
    );
    assert_eq!(report(&path)["tiff_pages"], 3);
    assert!(
        !warnings.iter().any(|w| w.starts_with(FIRST_ONLY)),
        "{warnings:?}"
    );
    unsafe { dr_doc_free(doc) };
}

#[test]
fn three_pages_open_as_three_layers_at_16_bits() {
    let dir = TempDir::new().unwrap();
    let (bytes, _) = write_tiff(
        &[page16(1, 5, 3, 3), page16(2, 5, 3, 4), page16(3, 5, 3, 3)],
        false,
    );
    let path = save(&dir, "three16.tif", &bytes);
    let doc = open_typed(&path).expect("open");
    let typed = unsafe { &*doc };
    assert!(!typed.is_float());
    assert_eq!(typed.dimensions(), (5, 3));
    assert_eq!(
        typed_layers(doc),
        names(&[("Page 1", true), ("Page 2", false), ("Page 3", false)])
    );
    for (index, (page, samples)) in [(1, 3), (2, 4), (3, 3)].into_iter().enumerate() {
        let source = typed.layer(index).unwrap().source().expect("raster");
        assert_eq!(
            source.to_rgba16().unwrap(),
            expected16(page, 5, 3, samples),
            "page {page}"
        );
    }
    let warnings = typed_warnings(doc);
    assert!(warnings.iter().any(|w| w.contains(OPENED)), "{warnings:?}");
    assert!(
        !warnings.iter().any(|w| w.starts_with(FIRST_ONLY)),
        "{warnings:?}"
    );
    unsafe { dr_precision_doc_free(doc) };
}

#[test]
fn three_pages_open_as_three_layers_at_32_bits() {
    let dir = TempDir::new().unwrap();
    let (bytes, _) = write_tiff(&[page32(1, 4, 3), page32(2, 4, 3), page32(3, 4, 3)], false);
    let path = save(&dir, "three32.tif", &bytes);
    let doc = open_typed(&path).expect("open");
    let typed = unsafe { &*doc };
    assert!(typed.is_float());
    assert_eq!(
        typed_layers(doc),
        names(&[("Page 1", true), ("Page 2", false), ("Page 3", false)])
    );
    for (index, page) in [1, 2, 3].into_iter().enumerate() {
        let source = typed.layer(index).unwrap().source().expect("raster");
        assert_eq!(
            source.to_rgba32().unwrap(),
            expected32(page, 4, 3),
            "page {page}"
        );
    }
    unsafe { dr_precision_doc_free(doc) };
}

#[test]
fn a_bigtiff_opens_every_page() {
    let dir = TempDir::new().unwrap();
    let (bytes, _) = write_tiff(&[page16(1, 4, 2, 3), page16(2, 4, 2, 3)], true);
    assert_eq!(&bytes[..4], b"II\x2b\0", "the fixture is a BigTIFF");
    let path = save(&dir, "big16.tif", &bytes);
    let doc = open_typed(&path).expect("open");
    assert_eq!(
        typed_layers(doc),
        names(&[("Page 1", true), ("Page 2", false)])
    );
    let source = unsafe { &*doc }.layer(1).unwrap().source().unwrap();
    assert_eq!(source.to_rgba16().unwrap(), expected16(2, 4, 2, 3));
    unsafe { dr_precision_doc_free(doc) };
    let byte = open_byte(&path).expect("byte open");
    assert_eq!(
        byte_layers(byte),
        names(&[("Page 1", true), ("Page 2", false)])
    );
    unsafe { dr_doc_free(byte) };
}

#[test]
fn a_reduced_resolution_image_is_not_a_page_and_no_longer_refuses() {
    let dir = TempDir::new().unwrap();
    let mut thumb16 = page16(9, 2, 1, 3);
    thumb16.new_subfile = Some(1);
    let (bytes, _) = write_tiff(&[page16(1, 4, 2, 3), thumb16], false);
    let path = save(&dir, "reduced16.tif", &bytes);
    let doc = open_typed(&path).expect("a thumbnail IFD is not a second page");
    assert_eq!(typed_layers(doc), names(&[("Background", true)]));
    let source = unsafe { &*doc }.layer(0).unwrap().source().unwrap();
    assert_eq!(source.to_rgba16().unwrap(), expected16(1, 4, 2, 3));
    unsafe { dr_precision_doc_free(doc) };

    let mut thumb32 = page32(9, 2, 1);
    thumb32.new_subfile = Some(1);
    let (bytes, _) = write_tiff(&[page32(1, 4, 2), thumb32], false);
    let path = save(&dir, "reduced32.tif", &bytes);
    let doc = open_typed(&path).expect("a thumbnail IFD is not a second page");
    assert_eq!(typed_layers(doc), names(&[("Background", true)]));
    unsafe { dr_precision_doc_free(doc) };
}

#[test]
fn an_exif_thumbnail_is_not_a_page() {
    let dir = TempDir::new().unwrap();
    let mut first = page8(1, 8, 6, 3);
    first.exif_ifd = true;
    let (bytes, _) = write_tiff(&[first, page8(2, 4, 3, 3)], false);
    let path = save(&dir, "exif8.tif", &bytes);
    let doc = open_byte(&path).expect("open");
    assert_eq!(byte_layers(doc), names(&[("Background", true)]));
    unsafe { dr_doc_free(doc) };
    assert!(report(&path).get("tiff_pages").is_none());
}

#[test]
fn a_page_unlike_page_one_is_skipped_and_named() {
    let dir = TempDir::new().unwrap();
    let mut cmyk = page16(3, 5, 3, 4);
    cmyk.photometric = 5;
    cmyk.extras = vec![];
    let (bytes, _) = write_tiff(
        &[
            page16(1, 5, 3, 3),
            page8(2, 5, 3, 3),
            cmyk,
            page16(4, 5, 3, 3),
        ],
        false,
    );
    let path = save(&dir, "mixed16.tif", &bytes);
    let doc = open_typed(&path).expect("open");
    assert_eq!(
        typed_layers(doc),
        names(&[("Page 1", true), ("Page 4", false)])
    );
    let source = unsafe { &*doc }.layer(1).unwrap().source().unwrap();
    assert_eq!(source.to_rgba16().unwrap(), expected16(4, 5, 3, 3));
    let warnings = typed_warnings(doc);
    assert!(
        warnings
            .iter()
            .any(|w| w.starts_with("Page 2 was not opened") && w.contains("8-bit")),
        "{warnings:?}"
    );
    assert!(
        warnings
            .iter()
            .any(|w| w.starts_with("Page 3 was not opened") && w.contains("CMYK")),
        "{warnings:?}"
    );
    unsafe { dr_precision_doc_free(doc) };

    // The byte open names the same pages in its report.
    let (bytes, _) = write_tiff(&[page8(1, 5, 3, 3), page16(2, 5, 3, 3)], false);
    let path = save(&dir, "mixed8.tif", &bytes);
    let doc = open_byte(&path).expect("open");
    assert_eq!(byte_layers(doc), names(&[("Background", true)]));
    unsafe { dr_doc_free(doc) };
    let warnings = report_warnings(&path);
    assert!(
        warnings
            .iter()
            .any(|w| w.starts_with("Page 2 was not opened") && w.contains("16-bit")),
        "{warnings:?}"
    );
    assert_eq!(report(&path)["tiff_pages"], 2);
}

#[test]
fn a_looped_page_list_opens_the_pages_before_the_loop() {
    let dir = TempDir::new().unwrap();
    let (mut bytes, placed) = write_tiff(&[page8(1, 4, 2, 3), page8(2, 4, 2, 3)], false);
    // The second IFD links back to the first.
    let (first, _) = placed[0];
    let (_, link) = placed[1];
    bytes[link..link + 4].copy_from_slice(&(first as u32).to_le_bytes());
    let path = save(&dir, "loop8.tif", &bytes);
    let doc = open_byte(&path).expect("open");
    assert_eq!(
        byte_layers(doc),
        names(&[("Page 1", true), ("Page 2", false)])
    );
    unsafe { dr_doc_free(doc) };
    let warnings = report_warnings(&path);
    assert!(
        warnings.iter().any(|w| w.contains("damaged")),
        "{warnings:?}"
    );
    // The same file at 16 bits opens too.
    let (mut bytes, placed) = write_tiff(&[page16(1, 4, 2, 3), page16(2, 4, 2, 3)], false);
    let (first, _) = placed[0];
    let (_, link) = placed[1];
    bytes[link..link + 4].copy_from_slice(&(first as u32).to_le_bytes());
    let path = save(&dir, "loop16.tif", &bytes);
    let doc = open_typed(&path).expect("open");
    assert_eq!(
        typed_layers(doc),
        names(&[("Page 1", true), ("Page 2", false)])
    );
    assert!(typed_warnings(doc).iter().any(|w| w.contains("damaged")));
    unsafe { dr_precision_doc_free(doc) };
}

#[test]
fn a_later_page_that_fails_to_decode_fails_the_open_naming_it() {
    let dir = TempDir::new().unwrap();
    let mut broken = page8(2, 4, 2, 3);
    broken.compression = 99;
    let (bytes, _) = write_tiff(&[page8(1, 4, 2, 3), broken], false);
    let path = save(&dir, "broken8.tif", &bytes);
    let error = open_byte(&path).expect_err("a page that cannot decode");
    assert!(error.starts_with("page 2 of "), "{error}");

    let mut broken = page16(2, 4, 2, 3);
    broken.compression = 99;
    let (bytes, _) = write_tiff(&[page16(1, 4, 2, 3), broken], false);
    let path = save(&dir, "broken16.tif", &bytes);
    let error = open_typed(&path).expect_err("a page that cannot decode");
    assert!(error.starts_with("page 2 of "), "{error}");
}

#[test]
fn a_one_page_tiff_reports_no_pages() {
    let dir = TempDir::new().unwrap();
    let (bytes, _) = write_tiff(&[page8(1, 4, 2, 3)], false);
    let path = save(&dir, "one8.tif", &bytes);
    let doc = open_byte(&path).expect("open");
    assert_eq!(byte_layers(doc), names(&[("Background", true)]));
    assert_eq!(layer_pixels(doc, 0), expected8(1, 4, 2, 3));
    unsafe { dr_doc_free(doc) };
    assert!(report(&path).get("tiff_pages").is_none());
    assert!(!report_warnings(&path).iter().any(|w| w.contains("page")));
}

/// A BigTIFF may store ImageWidth and ImageLength as LONG8: the page rule
/// reads those sizes too, so a smaller later page is named as one.
#[test]
fn a_bigtiff_page_size_stored_as_long8_is_read() {
    let dir = TempDir::new().unwrap();
    let mut first = page8(1, 6, 4, 3);
    first.long8_size = true;
    let mut second = page8(2, 3, 2, 3);
    second.long8_size = true;
    let (bytes, _) = write_tiff(&[first, second], true);
    let path = save(&dir, "long8.tif", &bytes);
    let warnings = report_warnings(&path);
    assert!(
        warnings
            .iter()
            .any(|w| w.starts_with("Page 2 is a different size from page 1 (6 × 4, the canvas)")),
        "{warnings:?}"
    );
    let doc = open_byte(&path).expect("open");
    assert_eq!(
        byte_layers(doc),
        names(&[("Page 1", true), ("Page 2", false)])
    );
    assert_eq!(layer_dims(doc, 1), (3, 2));
    assert_eq!(layer_pixels(doc, 1), expected8(2, 3, 2, 3));
    unsafe { dr_doc_free(doc) };
}

/// A typed open's result, or its error.
fn typed(doc: *mut PrecisionDocument, err: *mut c_char) -> Result<*mut PrecisionDocument, String> {
    if doc.is_null() {
        Err(take_err_string(err))
    } else {
        Ok(doc)
    }
}

/// `opened` holds page 1 alone and a warning starting with `sentence`.
fn assert_page_one_alone(
    opened: Result<*mut PrecisionDocument, String>,
    what: &str,
    sentence: &str,
) {
    let doc = opened.unwrap_or_else(|e| panic!("{what}: {e}"));
    assert_eq!(unsafe { &*doc }.layer_count(), 1, "{what}");
    let warnings = typed_warnings(doc);
    assert!(
        warnings.iter().any(|w| w.starts_with(sentence)),
        "{what}: {warnings:?}"
    );
    unsafe { dr_precision_doc_free(doc) };
}

/// Two pages of `page`'s shape, each with `photometric`.
fn two_pages(page: impl Fn(u32) -> Page, photometric: u16) -> Vec<u8> {
    let pages: Vec<Page> = (1..=2)
        .map(|p| {
            let mut one = page(p);
            one.photometric = photometric;
            one
        })
        .collect();
    write_tiff(&pages, false).0
}

/// The working Gray the Grayscale opens here assign to an untagged file.
const WORKING_GRAY: &str = "gray_gamma_22";

/// The 8/16-bit Grayscale open (`dr_precision_doc_open_gray`) of a file its
/// source test claims, as the app asks it before the RGB opens.
fn open_gray(path: &Path) -> Result<*mut PrecisionDocument, String> {
    let c = cpath(path);
    assert_ne!(unsafe { dr_path_gray_source(c.as_ptr()) }, 0, "{path:?}");
    let working = CString::new(WORKING_GRAY).unwrap();
    let mut err = ptr::null_mut();
    let doc = unsafe {
        dr_precision_doc_open_gray(
            c.as_ptr(),
            working.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    typed(doc, err)
}

/// The Lab Color open (`dr_precision_doc_open_lab`) of a file its source
/// test claims.
fn open_lab(path: &Path) -> Result<*mut PrecisionDocument, String> {
    let c = cpath(path);
    assert_ne!(unsafe { dr_path_lab_source(c.as_ptr()) }, 0, "{path:?}");
    let mut err = ptr::null_mut();
    let doc = unsafe { dr_precision_doc_open_lab(c.as_ptr(), LIMIT, 0, ptr::null(), &mut err) };
    typed(doc, err)
}

/// A gray page `p` (BlackIsZero) at `bits`: the gray sample is channel 0's
/// value and, when `samples` is 2, the unassociated alpha channel 3's, so
/// no alpha is zero.
fn gray_page(p: u32, w: u32, h: u32, bits: u16, samples: u16) -> Page {
    let mut data = Vec::new();
    for y in 0..h {
        for x in 0..w {
            for c in [0, 3].into_iter().take(usize::from(samples)) {
                if bits == 8 {
                    data.push(v8(p, x, y, c));
                } else {
                    data.extend_from_slice(&v16(p, x, y, c).to_le_bytes());
                }
            }
        }
    }
    let mut page = Page::new(w, h, bits, samples, 1, data);
    page.photometric = 1;
    page.extras = if samples == 2 { vec![2] } else { vec![] };
    page
}

/// [`gray_page`]'s pixels as a Grayscale document stores them: the code in
/// R = G = B (8-bit ×257) and the alpha, or opaque.
fn gray_expected(p: u32, w: u32, h: u32, bits: u16, samples: u16) -> Vec<[u16; 4]> {
    let code = |x: u32, y: u32, c: u32| {
        if bits == 8 {
            u16::from(v8(p, x, y, c)) * 257
        } else {
            v16(p, x, y, c)
        }
    };
    let mut out = Vec::new();
    for y in 0..h {
        for x in 0..w {
            let g = code(x, y, 0);
            let a = if samples == 2 { code(x, y, 3) } else { 65535 };
            out.push([g, g, g, a]);
        }
    }
    out
}

/// A three-sample Lab page `p` at `bits` with `photometric` 8 (CIELab) or
/// 9 (ICCLab).
fn lab_page(p: u32, w: u32, h: u32, bits: u16, photometric: u16) -> Page {
    let mut page = if bits == 8 {
        page8(p, w, h, 3)
    } else {
        page16(p, w, h, 3)
    };
    page.photometric = photometric;
    page
}

/// A four-ink CMYK page `p` at `bits` under `profile`.
fn cmyk_page(p: u32, w: u32, h: u32, bits: u16, profile: &[u8]) -> Page {
    let mut page = if bits == 8 {
        page8(p, w, h, 4)
    } else {
        page16(p, w, h, 4)
    };
    page.photometric = 5;
    page.extras = vec![];
    page.icc = Some(profile.to_vec());
    page
}

/// [`cmyk_page`]'s samples as a CMYK document stores them: each ink the
/// written value (8-bit ×257), opaque.
fn cmyk_expected(p: u32, w: u32, h: u32, bits: u16) -> Vec<[u16; 5]> {
    let ink = |x: u32, y: u32, c: u32| {
        if bits == 8 {
            u16::from(v8(p, x, y, c)) * 257
        } else {
            v16(p, x, y, c)
        }
    };
    let mut out = Vec::new();
    for y in 0..h {
        for x in 0..w {
            out.push([
                ink(x, y, 0),
                ink(x, y, 1),
                ink(x, y, 2),
                ink(x, y, 3),
                65535,
            ]);
        }
    }
    out
}

/// The 8/16-bit Grayscale open takes every page that matches page 1, each
/// as a hidden layer "Page n" holding its own samples, a smaller one at
/// the canvas's top-left; the first-image sentence is gone.
#[test]
fn a_gray_tiff_opens_every_page_as_layers() {
    let dir = TempDir::new().unwrap();
    for bits in [8u16, 16] {
        let shapes = [(1, 5, 3, 1), (2, 5, 3, 2), (3, 2, 2, 1)];
        let pages: Vec<Page> = shapes
            .iter()
            .map(|&(p, w, h, s)| gray_page(p, w, h, bits, s))
            .collect();
        let path = save(
            &dir,
            &format!("gray{bits}.tif"),
            &write_tiff(&pages, false).0,
        );
        let doc = open_gray(&path).expect("open");
        let typed = unsafe { &*doc };
        assert_eq!(typed.info().unwrap()["color_mode"], "gray", "{bits}-bit");
        assert_eq!(typed.dimensions(), (5, 3), "{bits}-bit");
        assert_eq!(
            typed_layers(doc),
            names(&[("Page 1", true), ("Page 2", false), ("Page 3", false)]),
            "{bits}-bit"
        );
        for (index, &(p, w, h, s)) in shapes.iter().enumerate() {
            let layer = typed.layer(index).unwrap();
            assert_eq!(layer.offset(), (0, 0), "{bits}-bit page {p}");
            assert_eq!(
                layer.source().expect("raster").to_rgba16().unwrap(),
                gray_expected(p, w, h, bits, s),
                "{bits}-bit page {p}"
            );
        }
        let warnings = typed_warnings(doc);
        assert!(warnings.iter().any(|w| w.contains(OPENED)), "{warnings:?}");
        assert!(
            warnings
                .iter()
                .any(|w| w.starts_with("Page 3 is a different size")),
            "{warnings:?}"
        );
        assert!(
            !warnings.iter().any(|w| w.starts_with(FIRST_ONLY)),
            "{warnings:?}"
        );
        unsafe { dr_precision_doc_free(doc) };
    }
}

/// The Lab Color open takes every page that matches page 1. Each layer
/// holds what the same page opens to as a one-page file, so the Lab
/// decode's own oracle (the Lab suite) covers the samples.
#[test]
fn a_lab_tiff_opens_every_page_as_layers() {
    let dir = TempDir::new().unwrap();
    for bits in [8u16, 16] {
        let pages = [lab_page(1, 4, 3, bits, 8), lab_page(2, 4, 3, bits, 8)];
        let path = save(
            &dir,
            &format!("lab{bits}.tif"),
            &write_tiff(&pages, false).0,
        );
        let doc = open_lab(&path).expect("open");
        let typed = unsafe { &*doc };
        assert_eq!(typed.info().unwrap()["color_mode"], "lab", "{bits}-bit");
        assert_eq!(
            typed_layers(doc),
            names(&[("Page 1", true), ("Page 2", false)]),
            "{bits}-bit"
        );
        let mut layers = Vec::new();
        for (index, page) in pages.iter().enumerate() {
            let one = save(
                &dir,
                &format!("lab{bits}-page{}.tif", index + 1),
                &write_tiff(std::slice::from_ref(page), false).0,
            );
            let alone = open_lab(&one).expect("one page");
            let want = unsafe { &*alone }.layer(0).unwrap().source().unwrap();
            let got = typed.layer(index).unwrap().source().expect("raster");
            let got = got.to_rgba16().unwrap();
            assert_eq!(
                got,
                want.to_rgba16().unwrap(),
                "{bits}-bit page {}",
                index + 1
            );
            layers.push(got);
            unsafe { dr_precision_doc_free(alone) };
        }
        assert_ne!(layers[0], layers[1], "{bits}-bit: the pages differ");
        let warnings = typed_warnings(doc);
        assert!(warnings.iter().any(|w| w.contains(OPENED)), "{warnings:?}");
        assert!(
            !warnings.iter().any(|w| w.starts_with(FIRST_ONLY)),
            "{warnings:?}"
        );
        unsafe { dr_precision_doc_free(doc) };
    }
}

/// A CMYK document holds layers, so a multipage TIFF whose page 1 opens as
/// one takes every matching page as a CMYK layer of its written ink.
#[test]
fn a_cmyk_tiff_opens_every_page_as_layers() {
    let dir = TempDir::new().unwrap();
    let profile = press_profile("Multipage Press");
    for bits in [8u16, 16] {
        let pages = [
            cmyk_page(1, 4, 2, bits, &profile),
            cmyk_page(2, 4, 2, bits, &profile),
        ];
        let path = save(
            &dir,
            &format!("cmyk{bits}.tif"),
            &write_tiff(&pages, false).0,
        );
        assert_eq!(unsafe { dr_path_cmyk_source(cpath(&path).as_ptr()) }, 1);
        let doc = open_typed(&path).expect("open");
        let typed = unsafe { &*doc };
        assert!(typed.is_cmyk(), "{bits}-bit");
        assert_eq!(
            typed_layers(doc),
            names(&[("Page 1", true), ("Page 2", false)]),
            "{bits}-bit"
        );
        for (index, p) in [1, 2].into_iter().enumerate() {
            assert_eq!(
                typed.cmyk_layer_samples(index).expect("cmyk raster"),
                cmyk_expected(p, 4, 2, bits),
                "{bits}-bit page {p}"
            );
        }
        let warnings = typed_warnings(doc);
        assert!(warnings.iter().any(|w| w.contains(OPENED)), "{warnings:?}");
        assert!(
            !warnings.iter().any(|w| w.starts_with("Only page 1")),
            "{warnings:?}"
        );
        unsafe { dr_precision_doc_free(doc) };
    }
}

/// The header rule holds in the colour modes: a page whose colour model
/// differs from page 1's is named and left out, and the three Lab
/// encodings are told apart by name.
#[test]
fn a_colour_mode_page_unlike_page_one_is_skipped_and_named() {
    let dir = TempDir::new().unwrap();
    let pages = [
        gray_page(1, 4, 2, 8, 1),
        page8(2, 4, 2, 3),
        gray_page(3, 4, 2, 8, 1),
    ];
    let path = save(&dir, "gray-rgb-gray.tif", &write_tiff(&pages, false).0);
    let doc = open_gray(&path).expect("open");
    assert_eq!(
        typed_layers(doc),
        names(&[("Page 1", true), ("Page 3", false)])
    );
    let source = unsafe { &*doc }.layer(1).unwrap().source().unwrap();
    assert_eq!(source.to_rgba16().unwrap(), gray_expected(3, 4, 2, 8, 1));
    let warnings = typed_warnings(doc);
    assert!(
        warnings
            .iter()
            .any(|w| w.starts_with("Page 2 was not opened") && w.contains("RGB")),
        "{warnings:?}"
    );
    unsafe { dr_precision_doc_free(doc) };

    let pages = [lab_page(1, 4, 2, 8, 9), lab_page(2, 4, 2, 8, 8)];
    let path = save(&dir, "icclab-cielab.tif", &write_tiff(&pages, false).0);
    let doc = open_lab(&path).expect("open");
    assert_eq!(unsafe { &*doc }.layer_count(), 1);
    let warnings = typed_warnings(doc);
    assert!(
        warnings
            .iter()
            .any(|w| w.starts_with("Page 2 was not opened")
                && w.contains("CIE Lab")
                && w.contains("ICC Lab")),
        "{warnings:?}"
    );
    unsafe { dr_precision_doc_free(doc) };
}

/// A later page that passes the header rule but that the mode's own open
/// refuses fails the open, naming the page, as the RGB opens do.
#[test]
fn a_colour_mode_page_that_fails_to_decode_fails_the_open_naming_it() {
    let dir = TempDir::new().unwrap();
    let mut broken = gray_page(2, 4, 2, 8, 1);
    broken.compression = 99;
    let pages = [gray_page(1, 4, 2, 8, 1), broken];
    let path = save(&dir, "broken-gray.tif", &write_tiff(&pages, false).0);
    let error = open_gray(&path).expect_err("a page that cannot decode");
    assert!(error.starts_with("page 2 of "), "{error}");

    let mut broken = lab_page(2, 4, 2, 8, 8);
    broken.compression = 99;
    let pages = [lab_page(1, 4, 2, 8, 8), broken];
    let path = save(&dir, "broken-lab.tif", &write_tiff(&pages, false).0);
    let error = open_lab(&path).expect_err("a page that cannot decode");
    assert!(error.starts_with("page 2 of "), "{error}");
}

/// The Grayscale and Lab opens, and the app's source tests, read the
/// file's first image. When that image is not a page (here a
/// reduced-resolution copy before the page), the open keeps reading it
/// alone, and the first-image sentence says so.
#[test]
fn a_first_image_that_is_not_a_page_keeps_the_one_directory_open() {
    let dir = TempDir::new().unwrap();
    let mut reduced = gray_page(1, 4, 2, 8, 1);
    reduced.new_subfile = Some(1);
    let pages = [reduced, gray_page(2, 4, 2, 8, 1)];
    let path = save(&dir, "reduced-first.tif", &write_tiff(&pages, false).0);
    let doc = open_gray(&path).expect("open");
    let typed = unsafe { &*doc };
    assert_eq!(typed.layer_count(), 1);
    let source = typed.layer(0).unwrap().source().unwrap();
    assert_eq!(source.to_rgba16().unwrap(), gray_expected(1, 4, 2, 8, 1));
    let warnings = typed_warnings(doc);
    assert!(
        warnings.iter().any(|w| w.starts_with(FIRST_ONLY)),
        "{warnings:?}"
    );
    unsafe { dr_precision_doc_free(doc) };

    let mut reduced = lab_page(1, 4, 2, 8, 8);
    reduced.new_subfile = Some(1);
    let first = save(
        &dir,
        "lab-first-alone.tif",
        &write_tiff(&[reduced.clone()], false).0,
    );
    let pages = [reduced, lab_page(2, 4, 2, 8, 8)];
    let path = save(&dir, "reduced-first-lab.tif", &write_tiff(&pages, false).0);
    let doc = open_lab(&path).expect("open");
    let alone = open_lab(&first).expect("open the first image alone");
    let typed = unsafe { &*doc };
    assert_eq!(typed.layer_count(), 1);
    assert_eq!(
        typed
            .layer(0)
            .unwrap()
            .source()
            .unwrap()
            .to_rgba16()
            .unwrap(),
        unsafe { &*alone }
            .layer(0)
            .unwrap()
            .source()
            .unwrap()
            .to_rgba16()
            .unwrap()
    );
    let warnings = typed_warnings(doc);
    assert!(
        warnings.iter().any(|w| w.starts_with(FIRST_ONLY)),
        "{warnings:?}"
    );
    unsafe { dr_precision_doc_free(alone) };
    unsafe { dr_precision_doc_free(doc) };
}

/// The Indexed Color and Bitmap opens read a TIFF's first directory alone,
/// as each document holds one layer. A multipage TIFF whose page 1 one of
/// them claims opens page 1 alone, and the metadata scan's first-image
/// sentence says so.
#[test]
fn an_indexed_or_bitmap_first_page_opens_alone_and_says_so() {
    let dir = TempDir::new().unwrap();
    let working = CString::new(WORKING_GRAY).unwrap();

    let palette = |p: u32| {
        let mut page = page8(p, 4, 2, 1);
        page.color_map = Some((0..768u32).map(|i| (i * 85) as u16).collect());
        page
    };
    let path = save(&dir, "indexed8.tif", &two_pages(palette, 3));
    let c = cpath(&path);
    assert_ne!(unsafe { dr_path_indexed_source(c.as_ptr()) }, 0);
    let mut err = ptr::null_mut();
    let doc = unsafe { dr_precision_doc_open_indexed(c.as_ptr(), LIMIT, 0, ptr::null(), &mut err) };
    assert_page_one_alone(typed(doc, err), "indexed", FIRST_ONLY);

    let one_bit = |p: u32| Page::new(8, 2, 1, 1, 1, vec![0b1010_1010 ^ p as u8, 0b0101_0101]);
    let path = save(&dir, "bitmap1.tif", &two_pages(one_bit, 1));
    let c = cpath(&path);
    assert_ne!(unsafe { dr_path_bitmap_source(c.as_ptr()) }, 0);
    let mut err = ptr::null_mut();
    let doc = unsafe {
        dr_precision_doc_open_bitmap(
            c.as_ptr(),
            working.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert_page_one_alone(typed(doc, err), "bitmap", FIRST_ONLY);
}

/// A one-sample float TIFF under a Gray profile is no file of the 8/16-bit
/// Grayscale open: the float route opens it as a 32-bit Grayscale document,
/// and that route takes every page.
#[test]
fn a_32_bit_gray_tiff_opens_every_page_as_layers() {
    let dir = TempDir::new().unwrap();
    let icc = gray_icc("Pages Gray 2.2", 2.2);
    let gray = |p: u32| {
        let data = (0..8u32)
            .flat_map(|i| (p as f32 * 0.25 + i as f32 * 0.0625).to_le_bytes())
            .collect();
        let mut page = Page::new(4, 2, 32, 1, 3, data);
        page.icc = Some(icc.clone());
        page
    };
    let path = save(&dir, "gray32.tif", &two_pages(gray, 1));
    assert_eq!(unsafe { dr_path_gray_source(cpath(&path).as_ptr()) }, 0);
    let doc = open_typed(&path).expect("open");
    assert_eq!(unsafe { &*doc }.info().unwrap()["color_mode"], "gray");
    assert_eq!(
        typed_layers(doc),
        names(&[("Page 1", true), ("Page 2", false)])
    );
    let warnings = typed_warnings(doc);
    assert!(warnings.iter().any(|w| w.contains(OPENED)), "{warnings:?}");
    unsafe { dr_precision_doc_free(doc) };
}
