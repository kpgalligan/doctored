//! Genuine 16-bit RGB PSD import and export through the C API. Every
//! expectation is checked against `common::psd16_fixture`, an independent
//! builder/reader with its own PackBits codec and its own matte arithmetic;
//! nothing here is compared against the writer or decoder under test.
use doctored_core::doc::BlendMode;
use doctored_core::doc_mask::MaskProperties;
use doctored_core::ffi_psd::{
    dr_doc_open_psd, dr_precision_doc_open_psd, dr_precision_doc_psd_export_report,
    dr_precision_doc_save_psd, dr_psd_import_report,
};
use doctored_core::icc::IccProfile;
use doctored_core::job_control::DrJobControl;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer as Layer};
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use doctored_core::precision_mask::{PrecisionMask, PrecisionPlane};
use doctored_core::psd_container::unpack_packbits;
use serde_json::{json, Value};
use std::ffi::{c_char, CString};
use std::path::Path;
use std::ptr;
use std::sync::Arc;
use tempfile::TempDir;

use crate::common;
use crate::psd16_fixture as fixture;
use common::{cpath, take_err_string};
use fixture::Record;

const LAYERS: i32 = 0;
const FLATTENED: i32 = 1;
const LIMIT: u64 = REFERENCE_WORKING_BYTES;

// ------------------------------------------------------------ C plumbing --

/// A typed document handle freed through the C API on drop.
#[derive(Debug)]
struct Owned(*mut PrecisionDocument);
impl Drop for Owned {
    fn drop(&mut self) {
        unsafe { doctored_core::ffi_precision::dr_precision_doc_free(self.0) };
    }
}
impl std::ops::Deref for Owned {
    type Target = PrecisionDocument;
    fn deref(&self) -> &PrecisionDocument {
        unsafe { &*self.0 }
    }
}

fn report(path: &Path) -> Value {
    let mut error = ptr::null_mut();
    let json = unsafe { dr_psd_import_report(cpath(path).as_ptr(), &mut error) };
    assert!(!json.is_null(), "{}", take_err_string(error));
    serde_json::from_str(&take_err_string(json)).expect("report JSON")
}

fn open16(
    path: &Path,
    mode: i32,
    fingerprint: &str,
    control: *const DrJobControl,
) -> Result<Owned, String> {
    let expected = CString::new(fingerprint).unwrap();
    let mut error = ptr::null_mut();
    let document = unsafe {
        dr_precision_doc_open_psd(
            cpath(path).as_ptr(),
            mode,
            expected.as_ptr(),
            LIMIT,
            0,
            control,
            &mut error,
        )
    };
    if document.is_null() {
        Err(take_err_string(error))
    } else {
        assert!(error.is_null(), "err_out set on success");
        Ok(Owned(document))
    }
}

fn open_typed(path: &Path, mode: i32) -> Owned {
    let notice = report(path);
    open16(
        path,
        mode,
        notice["source_fingerprint"].as_str().unwrap(),
        ptr::null(),
    )
    .expect("typed open")
}

fn info(document: &PrecisionDocument) -> Value {
    let mut error = ptr::null_mut();
    let json = unsafe { doctored_core::ffi_precision::dr_precision_doc_info(document, &mut error) };
    assert!(!json.is_null(), "{}", take_err_string(error));
    serde_json::from_str(&take_err_string(json)).expect("info JSON")
}

fn export_report(document: &PrecisionDocument, mode: i32) -> Value {
    let mut error = ptr::null_mut();
    let json = unsafe { dr_precision_doc_psd_export_report(document, 0, mode, &mut error) };
    assert!(!json.is_null(), "{}", take_err_string(error));
    serde_json::from_str(&take_err_string(json)).expect("export report JSON")
}

fn save16(
    document: &PrecisionDocument,
    projection: &PrecisionImage,
    path: &Path,
    mode: i32,
) -> Result<(), String> {
    let mut error = ptr::null_mut();
    if unsafe {
        dr_precision_doc_save_psd(
            document,
            projection,
            cpath(path).as_ptr(),
            0,
            mode,
            LIMIT,
            0,
            std::ptr::null(),
            &mut error,
        )
    } {
        assert!(error.is_null(), "err_out set on success");
        Ok(())
    } else {
        Err(take_err_string(error))
    }
}

fn strings(value: &Value, key: &str) -> Vec<String> {
    value[key]
        .as_array()
        .expect(key)
        .iter()
        .map(|v| v.as_str().expect("string").to_string())
        .collect()
}

/// A layer's own samples, read back through the borrowed typed pixel export.
fn layer_pixels(document: &PrecisionDocument, index: usize) -> Vec<[u16; 4]> {
    let image =
        unsafe { doctored_core::ffi_precision::dr_precision_doc_layer_source(document, index) };
    assert!(!image.is_null(), "layer {index} has no raster source");
    let pixels = pixels_of(unsafe { &*image });
    unsafe { doctored_core::ffi_precision::dr_precision_image_free(image) };
    pixels
}

fn pixels_of(image: &PrecisionImage) -> Vec<[u16; 4]> {
    let (w, h) = image.dimensions();
    let base = unsafe { doctored_core::ffi_precision::dr_precision_image_pixels_rgba16(image) };
    assert!(!base.is_null());
    let samples = unsafe { std::slice::from_raw_parts(base, w as usize * h as usize * 4) }.to_vec();
    samples
        .chunks_exact(4)
        .map(|p| [p[0], p[1], p[2], p[3]])
        .collect()
}

// --------------------------------------------------------------- fixtures --

fn ramp16(w: usize, h: usize, seed: u16) -> Vec<[u16; 4]> {
    (0..w * h)
        .map(|i| {
            let v = i as u16;
            [
                v.wrapping_mul(1013).wrapping_add(seed),
                v.wrapping_mul(3571).wrapping_add(seed * 7),
                v.wrapping_mul(7919).wrapping_add(seed * 13),
                65535,
            ]
        })
        .collect()
}

/// Partly transparent samples that KEEP their colour under alpha 0 — what a
/// straight 16-bit layer plane preserves and a matted composite cannot.
fn hidden16(w: usize, h: usize) -> Vec<[u16; 4]> {
    (0..w * h)
        .map(|i| {
            let alpha = [0u16, 1, 16384, 32767, 32768, 60000, 65535][i % 7];
            [4097 + i as u16, 51234 - i as u16, 60000, alpha]
        })
        .collect()
}

fn planes(rgba: &[[u16; 4]], width: usize, rle: bool) -> Vec<(i16, Vec<u8>)> {
    [(-1i16, 3usize), (0, 0), (1, 1), (2, 2)]
        .into_iter()
        .map(|(id, component)| {
            let samples: Vec<u16> = rgba.iter().map(|p| p[component]).collect();
            (
                id,
                if rle {
                    fixture::rle_plane(&samples, width)
                } else {
                    fixture::raw_plane(&samples)
                },
            )
        })
        .collect()
}

fn coverage(count: usize, step: u16) -> Vec<u16> {
    (0..count).map(|i| (i as u16).wrapping_mul(step)).collect()
}

struct Nested {
    bytes: Vec<u8>,
    ghost_mask: Vec<u16>,
    outer_mask: Vec<u16>,
    top_mask: Vec<u16>,
    base: Vec<[u16; 4]>,
    clip: Vec<[u16; 4]>,
    ghost: Vec<[u16; 4]>,
    deep: Vec<[u16; 4]>,
    top: Vec<[u16; 4]>,
    composite: Vec<[u16; 4]>,
}

/// Nested Pass Through / Multiply folders, an empty group, Raw and RLE
/// rasters, a negative offset, hidden RGB under alpha 0, samples one code
/// apart, an RLE mask outside 255, a disabled Raw mask outside 0, a group
/// mask, a clipped layer, a `hue ` layer, Display P3, 300 x 240 PPI and a
/// merged-alpha composite.
fn nested() -> Nested {
    let (w, h) = (8usize, 6usize);
    let base = ramp16(w, h, 11);
    let clip = ramp16(4, 3, 909);
    let ghost = hidden16(3, 3);
    let deep = vec![
        [1000, 2000, 3000, 65535],
        [1001, 2000, 3000, 65535],
        [1000, 2001, 3000, 65535],
        [1000, 2000, 3001, 65535],
    ];
    let top = ramp16(w, h, 5000);
    let ghost_mask = coverage(9, 7281);
    let outer_mask = coverage(w * h, 1234);
    let top_mask = coverage(w * h, 991);
    let composite: Vec<[u16; 4]> = (0..w * h)
        .map(|i| {
            let alpha = [0u16, 65535, 32768, 3, 65534, 12345][i % 6];
            [
                (i as u16).wrapping_mul(517),
                (i as u16).wrapping_mul(2029),
                (i as u16).wrapping_mul(37),
                alpha,
            ]
        })
        .collect();

    let closer = Record::folder("</Layer group>", 3, *b"norm");
    let mut clip_record = Record::raster("Clip", [1, 2, 4, 6], planes(&clip, 4, true));
    clip_record.clipping = 1;
    let mut ghost_record = Record::raster("Ghost", [-2, -1, 1, 2], planes(&ghost, 3, false)).mask(
        [1, 2, 4, 5],
        0,
        false,
        fixture::raw_plane(&ghost_mask),
    );
    ghost_record.flags = 8 | 2;
    let mut deep_record = Record::raster("Deep", [4, 5, 6, 7], planes(&deep, 2, false));
    deep_record.blend = *b"hue ";
    let mut inner = Record::folder("Inner", 2, *b"mul ");
    inner.opacity = 128;
    let outer = Record::folder("Outer", 1, *b"pass").mask(
        [0, 0, h as i32, w as i32],
        255,
        true,
        fixture::rle_plane(&outer_mask, w),
    );
    let top_record = Record::raster("Top", [0, 0, h as i32, w as i32], planes(&top, w, true)).mask(
        [0, 0, h as i32, w as i32],
        255,
        true,
        fixture::rle_plane(&top_mask, w),
    );
    let records = vec![
        Record::raster("Base", [0, 0, h as i32, w as i32], planes(&base, w, false)),
        clip_record,
        ghost_record,
        closer.clone(),
        closer.clone(),
        deep_record,
        inner,
        closer,
        Record::folder("Empty", 1, *b"norm"),
        outer,
        top_record,
    ];
    let bytes = fixture::fixture(
        (w as u32, h as u32),
        &records,
        IccProfile::display_p3().bytes(),
        (300.0, 240.0),
        &composite,
        true,
        true,
    );
    Nested {
        bytes,
        ghost_mask,
        outer_mask,
        top_mask,
        base,
        clip,
        ghost,
        deep,
        top,
        composite,
    }
}

/// `DR_PSD16_FIXTURES=<dir>` also writes the files these tests build, for
/// external review and for driving the built app (the `export_fixture` pattern
/// in `psd_export.rs`).
fn export_fixture(name: &str, bytes: &[u8]) {
    if let Ok(directory) = std::env::var("DR_PSD16_FIXTURES") {
        let directory = std::path::Path::new(&directory);
        std::fs::create_dir_all(directory).unwrap();
        std::fs::write(directory.join(format!("{name}.psd")), bytes).unwrap();
    }
}

fn write(directory: &TempDir, name: &str, bytes: &[u8]) -> std::path::PathBuf {
    let path = directory.path().join(name);
    std::fs::write(&path, bytes).expect("write fixture");
    path
}

// ------------------------------------------------------------------ tests --

#[test]
fn the_in_crate_packbits_decoder_inverts_an_independent_packer() {
    for row_bytes in [2usize, 256, 258, 514] {
        let mut cases: Vec<Vec<u8>> = Vec::new();
        // All-literal, one long run, and runs that deliberately span the
        // boundary between two big-endian sample pairs.
        cases.push((0..row_bytes).map(|i| (i * 7 % 251) as u8).collect());
        cases.push(vec![0xAB; row_bytes]);
        let mut spanning = vec![0u8; row_bytes];
        for (index, byte) in spanning.iter_mut().enumerate() {
            *byte = match index % 9 {
                0..=4 => 0x5A,
                5 => 0x01,
                _ => (index % 251) as u8,
            };
        }
        cases.push(spanning);
        let mut long_run = (0..row_bytes).map(|i| (i % 251) as u8).collect::<Vec<_>>();
        for byte in long_run.iter_mut().take(row_bytes.min(200)).skip(1) {
            *byte = 0x7F;
        }
        cases.push(long_run);
        for row in cases {
            let packed = fixture::pack(&row);
            assert_eq!(
                unpack_packbits(&packed, row_bytes).expect("decode"),
                row,
                "row of {row_bytes} bytes"
            );
            assert!(
                unpack_packbits(&packed, row_bytes + 1).is_err(),
                "a row that decodes to the wrong width must refuse"
            );
        }
    }
}

#[test]
fn a_sixteen_bit_file_reports_its_depth_and_opens_only_as_a_typed_document() {
    let directory = TempDir::new().expect("tempdir");
    let source = nested();
    let path = write(&directory, "nested16.psd", &source.bytes);

    let notice = report(&path);
    assert_eq!(notice["depth"], Value::from(16));
    assert_eq!(notice["import_precision"], Value::from("rgba16"));
    assert_eq!(notice["color_mode"], Value::from("RGB"));
    assert_eq!(notice["layered_available"], Value::Bool(true));
    assert_eq!(notice["stored_composite_available"], Value::Bool(true));
    let losses = strings(&notice, "verified_losses");
    assert!(
        losses.is_empty(),
        "a 16-bit document retains clipping and Hue: {losses:?}"
    );
    assert!(strings(&notice, "observations")
        .iter()
        .any(|o| o.contains("opens as a 16-bit document")));
    // A 16-bit document keeps the folder disclosure, so the report no
    // longer claims it is lost.
    assert!(!strings(&notice, "observations")
        .iter()
        .any(|o| o.contains("no folder open/closed state")));

    // The byte path never converts a depth implicitly.
    let fingerprint = CString::new(notice["source_fingerprint"].as_str().unwrap()).unwrap();
    let mut error = ptr::null_mut();
    let byte_document = unsafe {
        dr_doc_open_psd(
            cpath(&path).as_ptr(),
            LAYERS,
            fingerprint.as_ptr(),
            std::ptr::null(),
            &mut error,
        )
    };
    assert!(byte_document.is_null());
    let message = take_err_string(error);
    assert!(
        message.contains("16-bit PSD opens only as a 16-bit document"),
        "{message}"
    );
}

#[test]
fn layered_typed_import_retains_the_whole_admitted_stack() {
    let directory = TempDir::new().expect("tempdir");
    let source = nested();
    let path = write(&directory, "nested16.psd", &source.bytes);
    let notice = report(&path);
    let fingerprint = notice["source_fingerprint"].as_str().unwrap().to_string();

    // One LIVE single-use token, begun once for the whole open.
    let token = unsafe { doctored_core::ffi_jobs::dr_job_control_create() };
    let document = open16(&path, LAYERS, &fingerprint, token).expect("typed layered open");
    assert_eq!(
        unsafe { doctored_core::ffi_jobs::dr_job_control_scratch_bytes(token) },
        0,
        "the reservation is released when the open finishes"
    );

    let info = info(&document);
    let layers = info["layers"].as_array().expect("layers");
    let names: Vec<&str> = layers.iter().map(|l| l["name"].as_str().unwrap()).collect();
    assert_eq!(
        names,
        ["Base", "Clip", "Ghost", "Deep", "Inner", "Empty", "Outer", "Top"]
    );
    let depths: Vec<u64> = layers
        .iter()
        .map(|l| l["depth"].as_u64().unwrap())
        .collect();
    assert_eq!(depths, [0, 0, 0, 2, 1, 1, 0, 0]);
    assert_eq!(layers[1]["clipped"], Value::Bool(true), "the clipping byte");
    assert_eq!(layers[0]["clipped"], Value::Bool(false));
    assert_eq!(
        layers[4]["blend_mode"],
        Value::from(BlendMode::Multiply.to_c())
    );
    assert_eq!(layers[4]["opacity"], Value::from(128.0f32 / 255.0));
    assert_eq!(layers[6]["kind"], Value::from("pass_through_group"));
    assert_eq!(layers[3]["blend_mode"], Value::from(BlendMode::Hue.to_c()));
    assert_eq!(layers[2]["visible"], Value::Bool(false));
    assert_eq!(layers[2]["offset"], json!([-1, -2]));
    assert_eq!(layers[0]["offset"], json!([0, 0]));
    assert_eq!(layers[1]["offset"], json!([2, 1]));
    assert_eq!(layers[3]["offset"], json!([5, 4]));

    assert_eq!(layer_pixels(&document, 0), source.base);
    assert_eq!(layer_pixels(&document, 1), source.clip);
    assert_eq!(layer_pixels(&document, 2), source.ghost, "hidden RGB kept");
    assert_eq!(layer_pixels(&document, 3), source.deep);
    assert_eq!(layer_pixels(&document, 7), source.top);

    for (index, samples, offset, outside, enabled, size) in [
        (2usize, &source.ghost_mask, (2, 1), 0u8, false, (3u32, 3u32)),
        (6, &source.outer_mask, (0, 0), 255, true, (8, 6)),
        (7, &source.top_mask, (0, 0), 255, true, (8, 6)),
    ] {
        let mask = document
            .layer(index)
            .and_then(|layer| layer.mask())
            .unwrap_or_else(|| panic!("mask on layer {index}"));
        assert_eq!(mask.plane().samples(), &samples[..], "mask {index} samples");
        assert_eq!(mask.plane().dimensions(), size);
        assert_eq!(mask.properties().offset, offset);
        assert_eq!(mask.properties().outside_coverage, outside);
        assert_eq!(mask.enabled(), enabled);
        assert!(!mask.properties().linked, "imported masks are independent");
        assert_eq!(mask.properties().density, 1.0);
        assert_eq!(mask.properties().feather, 0.0);
    }
    assert_eq!(document.profile().bytes(), IccProfile::display_p3().bytes());
    assert_eq!(info["resolution"], json!({"x_ppi": 300.0, "y_ppi": 240.0}));

    // The consumed token cannot open a second document, and a cancelled one
    // refuses before any pixels are read.
    let again = open16(&path, LAYERS, &fingerprint, token);
    assert!(!again.expect_err("consumed token").is_empty());
    unsafe { doctored_core::ffi_jobs::dr_job_control_free(token) };
    let cancelled = unsafe { doctored_core::ffi_jobs::dr_job_control_create() };
    unsafe { doctored_core::ffi_jobs::dr_job_control_cancel(cancelled) };
    assert!(!open16(&path, LAYERS, &fingerprint, cancelled)
        .expect_err("cancelled token")
        .is_empty());
    unsafe { doctored_core::ffi_jobs::dr_job_control_free(cancelled) };
}

#[test]
fn the_stored_composite_matches_the_independent_un_matte_oracle() {
    let directory = TempDir::new().expect("tempdir");
    let source = nested();
    let path = write(&directory, "nested16.psd", &source.bytes);
    let document = open_typed(&path, FLATTENED);
    let expected: Vec<[u16; 4]> = source
        .composite
        .iter()
        .map(|p| {
            let stored = [
                fixture::matte(p[0], p[3]),
                fixture::matte(p[1], p[3]),
                fixture::matte(p[2], p[3]),
                p[3],
            ];
            [
                fixture::unmatte(stored[0], p[3]),
                fixture::unmatte(stored[1], p[3]),
                fixture::unmatte(stored[2], p[3]),
                p[3],
            ]
        })
        .collect();
    assert_eq!(layer_pixels(&document, 0), expected);
}

#[test]
fn an_empty_layer_section_offers_layers_and_opens_the_stored_composite() {
    let directory = TempDir::new().expect("tempdir");
    let composite: Vec<[u16; 4]> = (0..12)
        .map(|i| [i * 1000, 65535 - i * 900, i * 37, 65535])
        .collect();
    let bytes = fixture::fixture(
        (4, 3),
        &[],
        IccProfile::srgb().bytes(),
        (72.0, 72.0),
        &composite,
        false,
        false,
    );
    let path = write(&directory, "flat16.psd", &bytes);
    let notice = report(&path);
    assert_eq!(notice["import_precision"], Value::from("rgba16"));
    assert_eq!(notice["layered_available"], Value::Bool(true));
    let layered = open_typed(&path, LAYERS);
    let stored = open_typed(&path, FLATTENED);
    assert_eq!(layer_pixels(&layered, 0), composite);
    assert_eq!(layer_pixels(&layered, 0), layer_pixels(&stored, 0));
}

// ------------------------------------------------------------- exporting --

const LONG_NAME_LEN: usize = 360;

struct Built {
    document: PrecisionDocument,
    base: Vec<[u16; 4]>,
    hidden: Vec<[u16; 4]>,
    mask: Vec<u16>,
    name: String,
}

fn built() -> Built {
    let profile = IccProfile::display_p3();
    let base = ramp16(4, 3, 77);
    let hidden = hidden16(2, 2);
    let mask = coverage(4, 9001);
    let name = "n".repeat(LONG_NAME_LEN);
    let image = |w: u32, h: u32, pixels: Vec<[u16; 4]>| {
        PrecisionImage::from_rgba16(w, h, pixels, Arc::clone(&profile), None).unwrap()
    };
    let mut properties = MaskProperties::at((-1, 1));
    properties.linked = false;
    properties.outside_coverage = 255;
    let inner = Layer::raster(
        &name,
        image(2, 2, hidden.clone()),
        (-1, 1),
        128.0 / 255.0,
        false,
    )
    .unwrap()
    .with_blend_mode(BlendMode::Multiply)
    .unwrap()
    .with_mask(Some(
        PrecisionMask::new(
            PrecisionPlane::from_u16(2, 2, mask.clone(), None).unwrap(),
            properties,
            false,
        )
        .unwrap(),
    ));
    let group = Layer::pass_through("Group", vec![inner], 1.0, true).unwrap();
    let document = PrecisionDocument::from_parts(
        4,
        3,
        Arc::clone(&profile),
        Metadata::default(),
        Resolution { x: 300.0, y: 240.0 },
        vec![
            Layer::raster("Base", image(4, 3, base.clone()), (0, 0), 1.0, true).unwrap(),
            group,
        ],
    )
    .unwrap();
    Built {
        document,
        base,
        hidden,
        mask,
        name,
    }
}

#[test]
fn a_typed_document_round_trips_through_its_own_sixteen_bit_export() {
    let directory = TempDir::new().expect("tempdir");
    let source = built();
    let projection = source.document.render(LIMIT, 0, None).expect("projection");
    let path = directory.path().join("typed.psd");

    let report = export_report(&source.document, LAYERS);
    assert_eq!(report["depth"], Value::from(16));
    assert_eq!(report["available"], Value::Bool(true));
    assert_eq!(report["layer_count"], Value::from(2));
    assert_eq!(report["group_count"], Value::from(1));
    assert!(strings(&report, "general_limitations")[0].contains("Only 16-bit RGB classic PSD"));
    assert!(strings(&report, "observations")
        .iter()
        .any(|o| o.contains("Display P3") && o.contains("300")));

    save16(&source.document, &projection, &path, LAYERS).expect("save layered");
    let bytes = std::fs::read(&path).expect("read back");
    export_fixture("typed16", &bytes);
    assert!(
        (bytes.len() as u64) <= report["worst_case_bytes"].as_u64().unwrap(),
        "the preflight's worst case must bound the file it wrote"
    );

    // 1. The bytes themselves, through the independent reader.
    let file = fixture::read(&bytes);
    assert_eq!((file.width, file.height, file.depth), (4, 3, 16));
    assert_eq!((file.channels, file.color), (4, 3));
    assert_eq!(file.icc, IccProfile::display_p3().bytes().to_vec());
    assert_eq!(
        file.resolution,
        fixture::resolution_info(300.0, 240.0),
        "300 x 240 PPI"
    );
    let order: Vec<&str> = file.records.iter().map(|r| r.name.as_str()).collect();
    assert_eq!(
        order,
        ["Base", "</Layer group>", source.name.as_str(), "Group"],
        "closing divider before the contents, folder after them"
    );
    assert_eq!(file.records[1].divider, Some(3));
    assert_eq!(file.records[3].divider, Some(1), "typed folders are open");
    assert_eq!(file.records[3].divider_blend, Some(*b"pass"));
    assert_eq!(file.records[0].rgba(), source.base);
    let inner = &file.records[2];
    assert_eq!(
        inner.rgba(),
        source.hidden,
        "hidden RGB is written straight"
    );
    assert_eq!(inner.bounds, [1, -1, 3, 1]);
    assert_eq!(inner.opacity, 128);
    assert_eq!(inner.blend, *b"mul ");
    assert_eq!(inner.flags & 2, 2, "the hidden layer stays hidden");
    assert_eq!(inner.clipping, 0);
    assert_eq!(inner.pascal.len(), 255, "the Pascal name is truncated");
    assert_eq!(inner.plane(-2), &source.mask[..]);
    let mask = inner.mask.as_ref().expect("mask body");
    assert_eq!(mask.bounds, [1, -1, 3, 1]);
    assert_eq!(mask.outside, 255);
    assert_eq!(mask.flags, 2, "the mask is disabled");

    // 2. The stored preview against the independent matte, and its recovery.
    let rendered = pixels_of(&projection);
    for (stored, straight) in file.composite.iter().zip(&rendered) {
        let alpha = straight[3];
        assert_eq!(stored[3], alpha, "alpha is stored straight");
        for component in 0..3 {
            assert_eq!(
                stored[component],
                fixture::matte(straight[component], alpha),
                "the preview is matted on white"
            );
            let recovered = fixture::unmatte(stored[component], alpha);
            match alpha {
                0 => assert_eq!(recovered, 0, "alpha 0 has no recoverable colour"),
                65535 => assert_eq!(recovered, straight[component]),
                _ => {
                    let tolerance = (32767.5 / f64::from(alpha)).ceil() as i64;
                    assert!(
                        (i64::from(recovered) - i64::from(straight[component])).abs() <= tolerance,
                        "recovered {recovered} vs {} at alpha {alpha}",
                        straight[component]
                    );
                }
            }
        }
    }

    // 3. The file reopened through the typed importer.
    let back = open_typed(&path, LAYERS);
    let info = info(&back);
    let layers = info["layers"].as_array().unwrap();
    let names: Vec<&str> = layers.iter().map(|l| l["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["Base", source.name.as_str(), "Group"]);
    assert_eq!(layers[1]["offset"], json!([-1, 1]));
    assert_eq!(layers[1]["opacity"], Value::from(128.0f32 / 255.0));
    assert_eq!(layers[1]["visible"], Value::Bool(false));
    assert_eq!(
        layers[1]["blend_mode"],
        Value::from(BlendMode::Multiply.to_c())
    );
    assert_eq!(layers[2]["kind"], Value::from("pass_through_group"));
    assert_eq!(layer_pixels(&back, 0), source.base);
    assert_eq!(layer_pixels(&back, 1), source.hidden);
    let reopened = back.layer(1).and_then(|layer| layer.mask()).expect("mask");
    assert_eq!(reopened.plane().samples(), &source.mask[..]);
    assert_eq!(reopened.properties().offset, (-1, 1));
    assert_eq!(reopened.properties().outside_coverage, 255);
    assert!(!reopened.enabled());
}

#[test]
fn the_flattened_mode_writes_exactly_the_supplied_projection() {
    let directory = TempDir::new().expect("tempdir");
    let source = built();
    let projection = source.document.render(LIMIT, 0, None).expect("projection");
    let path = directory.path().join("flat.psd");
    let report = export_report(&source.document, FLATTENED);
    assert_eq!(report["layer_count"], Value::from(1));
    assert_eq!(report["group_count"], Value::from(0));
    save16(&source.document, &projection, &path, FLATTENED).expect("save flattened");
    let file = fixture::read(&std::fs::read(&path).expect("read back"));
    assert_eq!(file.records.len(), 1);
    assert_eq!(file.records[0].name, "Composite");
    assert_eq!(file.records[0].divider, None);
    assert_eq!(file.records[0].rgba(), pixels_of(&projection));
}

#[test]
fn every_sixteen_bit_refusal_leaves_an_existing_destination_untouched() {
    let directory = TempDir::new().expect("tempdir");
    let path = directory.path().join("kept.psd");
    let sentinel = b"an existing file".to_vec();
    let profile = IccProfile::display_p3();
    let image = |w: u32, h: u32| {
        PrecisionImage::from_rgba16(
            w,
            h,
            vec![[1, 2, 3, 65535]; (w * h) as usize],
            Arc::clone(&profile),
            None,
        )
        .unwrap()
    };

    // An adjustment layer is no longer among the refusals: one without a
    // Photoshop record is baked (`psd_adjustment_bake.rs`).
    let mut properties = MaskProperties::at((0, 0));
    properties.density = 0.5;
    let feathered = PrecisionDocument::from_parts(
        2,
        2,
        Arc::clone(&profile),
        Metadata::default(),
        Resolution::default(),
        vec![Layer::raster("Base", image(2, 2), (0, 0), 1.0, true)
            .unwrap()
            .with_mask(Some(
                PrecisionMask::new(
                    PrecisionPlane::from_u16(2, 2, vec![0, 1, 2, 3], None).unwrap(),
                    properties,
                    true,
                )
                .unwrap(),
            ))],
    )
    .unwrap();

    let wide =
        PrecisionDocument::from_image(image(30_001, 1), Metadata::default(), Resolution::default())
            .unwrap();

    for (document, expected) in [
        (&feathered, "has density or feather"),
        (&wide, "30000 pixels per side"),
    ] {
        std::fs::write(&path, &sentinel).unwrap();
        let projection = document.render(LIMIT, 0, None).expect("projection");
        let message = save16(document, &projection, &path, LAYERS).expect_err("refusal");
        assert!(message.contains(expected), "{message}");
        assert!(
            export_report(document, LAYERS)["refusal"]
                .as_str()
                .unwrap()
                .contains(expected),
            "the report states the same refusal"
        );
        assert_eq!(std::fs::read(&path).unwrap(), sentinel);
    }

    // A reduced file-size cap refuses; a reduced reopen envelope is a LOSS.
    let source = built();
    let tiny: Value = serde_json::from_str(&source.document.psd_export_report_with_limits(
        LAYERS,
        512,
        u64::MAX,
    ))
    .unwrap();
    assert_eq!(tiny["depth"], Value::from(16));
    assert_eq!(tiny["available"], Value::Bool(false));
    assert!(tiny["refusal"]
        .as_str()
        .unwrap()
        .contains("past the format's"));
    let envelope: Value = serde_json::from_str(&source.document.psd_export_report_with_limits(
        LAYERS,
        u64::MAX,
        1024,
    ))
    .unwrap();
    assert_eq!(envelope["available"], Value::Bool(true));
    assert!(strings(&envelope, "losses")
        .iter()
        .any(|l| l.contains("layered import envelope")));
}

#[test]
fn the_sixteen_bit_exports_tolerate_null_inputs() {
    let directory = TempDir::new().expect("tempdir");
    let path = cpath(&directory.path().join("never.psd"));
    let source = built();
    let projection = source.document.render(LIMIT, 0, None).expect("projection");

    let mut error: *mut c_char = ptr::null_mut();
    assert!(unsafe {
        dr_precision_doc_open_psd(
            ptr::null(),
            LAYERS,
            ptr::null(),
            LIMIT,
            0,
            ptr::null(),
            &mut error,
        )
    }
    .is_null());
    assert!(!take_err_string(error).is_empty());
    assert!(unsafe {
        dr_precision_doc_open_psd(
            ptr::null(),
            LAYERS,
            ptr::null(),
            LIMIT,
            0,
            ptr::null(),
            ptr::null_mut(),
        )
    }
    .is_null());

    let mut error: *mut c_char = ptr::null_mut();
    assert!(
        unsafe { dr_precision_doc_psd_export_report(ptr::null(), 0, LAYERS, &mut error) }.is_null()
    );
    assert!(!take_err_string(error).is_empty());
    let mut error: *mut c_char = ptr::null_mut();
    assert!(
        unsafe { dr_precision_doc_psd_export_report(&source.document, 0, -1, &mut error) }
            .is_null()
    );
    assert!(take_err_string(error).contains("unknown PSD export mode"));

    let mut error: *mut c_char = ptr::null_mut();
    assert!(!unsafe {
        dr_precision_doc_save_psd(
            ptr::null(),
            &projection,
            path.as_ptr(),
            0,
            LAYERS,
            LIMIT,
            0,
            std::ptr::null(),
            &mut error,
        )
    });
    assert!(!take_err_string(error).is_empty());
    let mut error: *mut c_char = ptr::null_mut();
    assert!(!unsafe {
        dr_precision_doc_save_psd(
            &source.document,
            ptr::null(),
            path.as_ptr(),
            0,
            LAYERS,
            LIMIT,
            0,
            std::ptr::null(),
            &mut error,
        )
    });
    assert!(!take_err_string(error).is_empty());
    assert!(!unsafe {
        dr_precision_doc_save_psd(
            ptr::null(),
            ptr::null(),
            ptr::null(),
            0,
            LAYERS,
            LIMIT,
            0,
            std::ptr::null(),
            ptr::null_mut(),
        )
    });
    assert!(!directory.path().join("never.psd").exists());
}
