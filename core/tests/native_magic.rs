//! The native format's two magics (the Rasterize → Doctored rename): every
//! file this build writes starts `DRDC`, and every reader takes `DRDC` and
//! the legacy `RZDC` alike, at both depths, through the FFI.
//!
//! The oracle for the legacy side is the committed fixtures themselves: each
//! starts `RZDC`, and a twin that differs only in its first four bytes must
//! open to exactly what the original opens to — the same flattened pixels,
//! or the same refusal.

use std::ffi::c_char;
use std::path::{Path, PathBuf};
use std::ptr;

use doctored_core::doc::DrDocument;
use doctored_core::doc_color::metadata_walked;
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_doc::{dr_doc_free, dr_doc_layer_count, dr_doc_open, dr_doc_save_native};
use doctored_core::ffi_metadata_report::dr_path_metadata_import_report;
use doctored_core::ffi_precision::{
    dr_precision_doc_free, dr_precision_doc_open, dr_precision_doc_save_native,
};
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use image::{Rgba, RgbaImage};
use serde_json::Value;
use tempfile::TempDir;

use crate::common::{
    add_layer, cpath, doc_from, flat_pixels, layer_dims, layer_offset, layer_pixels, solid,
    take_err_string, test_pattern,
};

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
const NEW: &[u8; 4] = b"DRDC";
const LEGACY: &[u8; 4] = b"RZDC";

/// Every committed native fixture, relative to `tests/fixtures`.
fn fixtures() -> Vec<PathBuf> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let mut found = Vec::new();
    for entry in std::fs::read_dir(&root).unwrap() {
        let dir = entry.unwrap().path();
        if !dir.is_dir() {
            continue;
        }
        for file in std::fs::read_dir(&dir).unwrap() {
            let file = file.unwrap().path();
            if file.extension().is_some_and(|e| e == "rz") {
                found.push(file);
            }
        }
    }
    found.sort();
    found
}

/// `bytes` with its first four bytes replaced by `magic`.
fn with_magic(bytes: &[u8], magic: &[u8; 4]) -> Vec<u8> {
    let mut out = bytes.to_vec();
    out[..4].copy_from_slice(magic);
    out
}

/// The byte open's result: the flattened pixels with the layer count, or
/// the refusal with the file's own path taken out.
fn byte_result(path: &Path) -> Result<(usize, Vec<u8>), String> {
    let c = cpath(path);
    let mut err: *mut c_char = ptr::null_mut();
    let doc = unsafe { dr_doc_open(c.as_ptr(), &mut err) };
    if doc.is_null() {
        return Err(take_err_string(err).replace(path.to_str().unwrap(), "<path>"));
    }
    let out = (unsafe { dr_doc_layer_count(doc) }, flat_pixels(doc));
    unsafe { dr_doc_free(doc) };
    Ok(out)
}

/// The typed open's result: every layer's stored samples and the render's
/// u16 projection, or the refusal with the file's own path taken out.
type Typed = (Vec<Vec<[u16; 4]>>, Vec<[u16; 4]>);
fn typed_result(path: &Path) -> Result<Typed, String> {
    let c = cpath(path);
    let mut err: *mut c_char = ptr::null_mut();
    let doc = unsafe { dr_precision_doc_open(c.as_ptr(), LIMIT, 0, ptr::null(), &mut err) };
    if doc.is_null() {
        return Err(take_err_string(err).replace(path.to_str().unwrap(), "<path>"));
    }
    let document = unsafe { &*doc };
    let layers = (0..document.layer_count())
        .map(|i| {
            document
                .layer_source(i)
                .and_then(|image| image.to_rgba16().ok())
                .unwrap_or_default()
        })
        .collect();
    let render = document
        .render(LIMIT, 0, None)
        .ok()
        .and_then(|image| image.to_rgba16().ok())
        .unwrap_or_default();
    unsafe { dr_precision_doc_free(doc) };
    Ok((layers, render))
}

fn report(path: &Path) -> Value {
    let c = cpath(path);
    let mut err: *mut c_char = ptr::null_mut();
    let json = unsafe { dr_path_metadata_import_report(c.as_ptr(), &mut err) };
    assert!(!json.is_null(), "report: {}", take_err_string(err));
    let text = unsafe { std::ffi::CStr::from_ptr(json) }
        .to_string_lossy()
        .into_owned();
    unsafe { dr_string_free(json) };
    serde_json::from_str(&text).unwrap()
}

fn byte_document(dir: &TempDir) -> *mut DrDocument {
    let doc = doc_from(dir, "bg.png", &test_pattern(6, 5));
    add_layer(
        dir,
        "top.png",
        doc,
        0,
        &solid(3, 2, [200, 40, 9, 180]),
        "Top",
    )
}

fn typed_document() -> PrecisionDocument {
    let image = PrecisionImage::from_rgba16(
        4,
        3,
        (0..12)
            .map(|i| [1000 + i * 911, 40_000 - i * 77, 123 * i, 65535 - i * 9])
            .collect(),
        IccProfile::srgb(),
        None,
    )
    .unwrap();
    PrecisionDocument::from_image(image, Metadata::default(), Resolution::default()).unwrap()
}

#[test]
fn a_byte_document_writes_drdc_and_reopens_equal() {
    let dir = TempDir::new().unwrap();
    let doc = byte_document(&dir);
    let path = dir.path().join("byte.doctored");
    let c = cpath(&path);
    let mut err: *mut c_char = ptr::null_mut();
    assert!(
        unsafe { dr_doc_save_native(doc, c.as_ptr(), &mut err) },
        "save: {}",
        take_err_string(err)
    );
    let bytes = std::fs::read(&path).unwrap();
    assert_eq!(&bytes[..4], NEW, "the writer emits the new magic");

    let mut err: *mut c_char = ptr::null_mut();
    let back = unsafe { dr_doc_open(c.as_ptr(), &mut err) };
    assert!(!back.is_null(), "reopen: {}", take_err_string(err));
    assert_eq!(unsafe { dr_doc_layer_count(back) }, 2);
    for i in 0..2 {
        assert_eq!(layer_pixels(back, i), layer_pixels(doc, i), "layer {i}");
        assert_eq!(layer_dims(back, i), layer_dims(doc, i), "layer {i}");
        assert_eq!(layer_offset(back, i), layer_offset(doc, i), "layer {i}");
    }
    assert_eq!(flat_pixels(back), flat_pixels(doc), "flattened");
    unsafe { dr_doc_free(back) };
    unsafe { dr_doc_free(doc) };
}

#[test]
fn a_typed_document_writes_drdc_and_reopens_equal() {
    let dir = TempDir::new().unwrap();
    let doc = Box::into_raw(Box::new(typed_document()));
    let path = dir.path().join("typed.doctored");
    let c = cpath(&path);
    let mut err: *mut c_char = ptr::null_mut();
    let saved =
        unsafe { dr_precision_doc_save_native(doc, c.as_ptr(), LIMIT, 0, ptr::null(), &mut err) };
    assert!(saved, "save: {}", take_err_string(err));
    let bytes = std::fs::read(&path).unwrap();
    assert_eq!(&bytes[..4], NEW, "the typed writer emits the new magic");

    let original = unsafe { &*doc };
    let (layers, render) = typed_result(&path).expect("the typed file reopens");
    assert_eq!(layers.len(), original.layer_count());
    for (i, samples) in layers.iter().enumerate() {
        let want = original.layer_source(i).unwrap().to_rgba16().unwrap();
        assert_eq!(samples, &want, "layer {i}");
    }
    let want = original
        .render(LIMIT, 0, None)
        .unwrap()
        .to_rgba16()
        .unwrap();
    assert_eq!(render, want, "flattened");
    unsafe { dr_precision_doc_free(doc) };
}

/// Each committed fixture starts with the legacy magic, and its `DRDC` twin
/// opens at both depths to the original's own result.
#[test]
fn every_committed_fixture_opens_the_same_under_either_magic() {
    let fixtures = fixtures();
    assert_eq!(fixtures.len(), 27, "the committed .rz fixtures");
    let dir = TempDir::new().unwrap();
    let mut opened = 0;
    for (n, fixture) in fixtures.iter().enumerate() {
        let name = fixture.display();
        let bytes = std::fs::read(fixture).unwrap();
        assert_eq!(&bytes[..4], LEGACY, "{name}: a legacy file");
        let legacy = dir.path().join(format!("legacy-{n}"));
        let renamed = dir.path().join(format!("renamed-{n}"));
        std::fs::create_dir(&legacy).unwrap();
        std::fs::create_dir(&renamed).unwrap();
        let legacy = legacy.join("file.rz");
        let renamed = renamed.join("file.rz");
        std::fs::write(&legacy, &bytes).unwrap();
        std::fs::write(&renamed, with_magic(&bytes, NEW)).unwrap();

        let byte = byte_result(&legacy);
        assert_eq!(byte, byte_result(&renamed), "{name}: byte open");
        let typed = typed_result(&legacy);
        assert_eq!(typed, typed_result(&renamed), "{name}: typed open");
        if byte.is_ok() || typed.is_ok() {
            opened += 1;
        }
    }
    assert_eq!(opened, fixtures.len(), "every fixture opens at one depth");
}

/// A hand-built version-1 file (canvas, one layer, its PNG) opens under
/// either magic with the same layer.
#[test]
fn a_crafted_version_one_file_opens_under_both_magics() {
    let dir = TempDir::new().unwrap();
    let pixels = RgbaImage::from_fn(2, 2, |x, y| Rgba([x as u8 * 90, y as u8 * 70, 5, 255]));
    let mut png = Vec::new();
    pixels
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .unwrap();
    for magic in [LEGACY, NEW] {
        let mut crafted = magic.to_vec();
        for word in [1u32, 2, 2, 1, 1] {
            crafted.extend_from_slice(&word.to_le_bytes()); // version, w, h, count, name len
        }
        crafted.push(b'X');
        crafted.extend_from_slice(&0i32.to_le_bytes());
        crafted.extend_from_slice(&0i32.to_le_bytes());
        crafted.extend_from_slice(&1.0f32.to_le_bytes());
        crafted.extend_from_slice(&0u32.to_le_bytes()); // normal
        crafted.push(1); // visible
        crafted.extend_from_slice(&(png.len() as u32).to_le_bytes());
        crafted.extend_from_slice(&png);
        let path = dir
            .path()
            .join(format!("v1-{}.rz", String::from_utf8_lossy(magic)));
        std::fs::write(&path, &crafted).unwrap();
        let (count, flat) = byte_result(&path).expect("the crafted file opens");
        assert_eq!(count, 1);
        assert_eq!(flat, pixels.as_raw().clone(), "{magic:?}");
    }
}

/// The metadata report and the walked-container query sniff both magics as
/// the native container, and neither reports an embedded-profile sentence.
#[test]
fn the_metadata_sniffs_accept_both_magics() {
    let dir = TempDir::new().unwrap();
    let doc = byte_document(&dir);
    let path = dir.path().join("saved.doctored");
    let c = cpath(&path);
    let mut err: *mut c_char = ptr::null_mut();
    assert!(unsafe { dr_doc_save_native(doc, c.as_ptr(), &mut err) });
    unsafe { dr_doc_free(doc) };
    let bytes = std::fs::read(&path).unwrap();
    let legacy = dir.path().join("legacy.rz");
    std::fs::write(&legacy, with_magic(&bytes, LEGACY)).unwrap();
    let neither = dir.path().join("neither.rz");
    std::fs::write(&neither, with_magic(&bytes, b"XXDC")).unwrap();
    for file in [&path, &legacy] {
        let report = report(file);
        assert_eq!(report["container"], "native", "{file:?}: {report}");
        assert!(report.get("profile_refusal").is_none(), "{report}");
        assert!(metadata_walked(file.to_str().unwrap()), "{file:?}");
    }
    assert_ne!(report(&neither)["container"], "native");
    assert!(!metadata_walked(neither.to_str().unwrap()));
}
