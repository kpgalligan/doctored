//! #1 phase 3c: 32-bit Grayscale files — a 32-bit gray PSD or PSB opens as
//! a 32-bit Gray document, a 32-bit Gray document writes one, and its
//! kept-gray TIFF at 32 bits is a one- or two-sample float TIFF that reopens
//! as a 32-bit Gray document, through the C API.
//!
//! Oracles, stated before running:
//! * Photoshop's OWN `layers32.psd` (`gray-files-547/take2`, the read fixture
//!   `tests/fixtures/psd_photoshop/gray/layers32.psd`): its stored composite
//!   plane (Raw, the file's last `32·24·4` bytes, read here) is the
//!   reference for our layered render within `1e-3·|ps| + 1e-6` (the
//!   request's relative 1e-3) and for the mode-1 open bit for bit;
//! * Adobe's published framing, read here: the header's channel count, mode
//!   and depth, the `Mt32` / `Lr32` blocks, resource 1039 and an ICC `desc`
//!   and `kTRC` per ICC.1;
//! * the composite's matte `c·a + (1 − a)` formed in f64, the 32-bit RGB
//!   writer's stated rule;
//! * every round trip compares f32 samples and u16 mask codes BIT for bit
//!   against the source document.

use std::ffi::{c_char, CStr, CString};
use std::path::{Path, PathBuf};
use std::ptr;

use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_export::{
    dr_export_result_bytes, dr_export_result_free, dr_export_result_preview,
    dr_precision_doc_export_encode,
};
use doctored_core::ffi_gray::{dr_precision_doc_convert_to_gray, dr_precision_doc_new_gray};
use doctored_core::ffi_precision::{dr_precision_doc_descriptor, dr_precision_doc_free};
use doctored_core::ffi_precision_conversion::dr_precision_doc_promote_to_float;
use doctored_core::ffi_precision_inspect::dr_precision_doc_profile;
use doctored_core::ffi_psd::{
    dr_precision_doc_open_gray_psd, dr_precision_doc_save_psd, dr_psd_import_report,
};
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer};
use doctored_core::precision_image::PrecisionImage;
use doctored_core::tiff_float::{self, FloatSampleType};
use serde_json::{json, Value};
use tempfile::TempDir;

use crate::common::{cpath, take_err_string};
use crate::gray_fixture::gray_icc;
use crate::gray_ops::{edit, LIMIT};

/// `layers32.psd`'s canvas.
const PS_W: usize = 32;
const PS_H: usize = 24;
const W: u32 = 8;
const H: u32 = 6;
const TIFF_REFUSAL: &str = "A TIFF with layers is not written for a 32-bit Grayscale document; export a PSD or PSB, or a flat 32-bit TIFF (File ▸ Export…).";
const GENERAL: &str = "This file opens as a Grayscale document, keeping its own gray values, declared depth and embedded Gray profile, with either choice.";
const RELABELLED: &str = "This file opens as a Grayscale document, keeping its own gray values and declared depth, with either choice.";

// ------------------------------------------------------------ the C API --

fn fixture() -> Option<PathBuf> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/psd_photoshop/gray/layers32.psd");
    // A committed fixture with no ImageMagick dependency: its absence is a failure, never a skip.
    assert!(
        path.is_file(),
        "the committed Photoshop fixture {} is missing",
        path.display()
    );
    Some(path)
}

fn json_of(text: *mut c_char, err: *mut c_char) -> Value {
    assert!(!text.is_null(), "{}", take_err_string(err));
    let value = serde_json::from_str(unsafe { CStr::from_ptr(text) }.to_str().unwrap()).unwrap();
    unsafe { dr_string_free(text) };
    value
}

fn report(path: &Path) -> Value {
    let mut err = ptr::null_mut();
    json_of(
        unsafe { dr_psd_import_report(cpath(path).as_ptr(), &mut err) },
        err,
    )
}

fn descriptor(doc: *const PrecisionDocument) -> Value {
    let mut err = ptr::null_mut();
    json_of(unsafe { dr_precision_doc_descriptor(doc, &mut err) }, err)
}

fn open(path: &Path, mode: i32) -> *mut PrecisionDocument {
    let fingerprint = CString::new(report(path)["source_fingerprint"].as_str().unwrap()).unwrap();
    let working = CString::new("dot_gain_20").unwrap();
    let mut err = ptr::null_mut();
    let doc = unsafe {
        dr_precision_doc_open_gray_psd(
            cpath(path).as_ptr(),
            mode,
            fingerprint.as_ptr(),
            working.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(
        !doc.is_null(),
        "{}: {}",
        path.display(),
        take_err_string(err)
    );
    doc
}

fn free(doc: *mut PrecisionDocument) {
    unsafe { dr_precision_doc_free(doc) };
}

/// The document's own profile handle's bytes (a Gray document's twin).
fn twin_bytes(doc: *const PrecisionDocument) -> Vec<u8> {
    let mut length = 0;
    let bytes = unsafe { dr_precision_doc_profile(doc, &mut length) };
    assert!(!bytes.is_null());
    unsafe { std::slice::from_raw_parts(bytes, length) }.to_vec()
}

fn projection(doc: *const PrecisionDocument) -> PrecisionImage {
    unsafe { &*doc }.render(LIMIT, 0, None).unwrap()
}

fn save(
    doc: *const PrecisionDocument,
    projection: &PrecisionImage,
    path: &Path,
    container: i32,
    mode: i32,
) -> Result<(), String> {
    let mut err = ptr::null_mut();
    let ok = unsafe {
        dr_precision_doc_save_psd(
            doc,
            projection,
            cpath(path).as_ptr(),
            container,
            mode,
            LIMIT,
            0,
            &mut err,
        )
    };
    if ok {
        Ok(())
    } else {
        Err(take_err_string(err))
    }
}

fn f32_bits(samples: &[[f32; 4]]) -> Vec<[u32; 4]> {
    samples.iter().map(|p| p.map(f32::to_bits)).collect()
}

/// Every record's (name, f32 sample bits, mask codes), postorder.
type Layers = Vec<(String, Option<Vec<[u32; 4]>>, Option<Vec<u16>>)>;

fn layers(doc: *const PrecisionDocument) -> Layers {
    let doc = unsafe { &*doc };
    (0..doc.layer_count())
        .map(|i| {
            let layer: &PrecisionDocumentLayer = doc.layer(i).unwrap();
            (
                layer.name().to_string(),
                layer.source().map(|s| f32_bits(&s.to_rgba32().unwrap())),
                layer.mask().map(|m| m.plane().samples().to_vec()),
            )
        })
        .collect()
}

// ----------------------------------------------------- reading the files --

fn be16(bytes: &[u8], at: usize) -> u16 {
    u16::from_be_bytes([bytes[at], bytes[at + 1]])
}

fn be32(bytes: &[u8], at: usize) -> usize {
    u32::from_be_bytes(bytes[at..at + 4].try_into().unwrap()) as usize
}

/// A PSD's or PSB's resource 1039 (Adobe's image-resource framing).
fn icc_resource(bytes: &[u8]) -> Option<Vec<u8>> {
    let mut at = 26 + 4 + be32(bytes, 26);
    let end = at + 4 + be32(bytes, at);
    at += 4;
    while at < end {
        let id = be16(bytes, at + 4);
        let name = usize::from(bytes[at + 6]);
        at += 6 + ((name + 2) & !1);
        let len = be32(bytes, at);
        at += 4;
        if id == 1039 {
            return Some(bytes[at..at + len].to_vec());
        }
        at += len + (len & 1);
    }
    None
}

/// An ICC tag's data per ICC.1 (the tag table at byte 128).
fn icc_tag<'a>(icc: &'a [u8], sig: &[u8; 4]) -> &'a [u8] {
    let entry = (0..be32(icc, 128))
        .map(|i| 132 + 12 * i)
        .find(|&at| &icc[at..at + 4] == sig)
        .unwrap_or_else(|| panic!("no {} tag", String::from_utf8_lossy(sig)));
    let (offset, len) = (be32(icc, entry + 4), be32(icc, entry + 8));
    &icc[offset..offset + len]
}

/// A v2 `desc` tag's ASCII name.
fn icc_name(icc: &[u8]) -> String {
    let desc = icc_tag(icc, b"desc");
    assert_eq!(&desc[..4], b"desc");
    let count = be32(desc, 8);
    String::from_utf8(desc[12..12 + count - 1].to_vec()).unwrap()
}

/// True for an identity `kTRC`: a `curv` of 0 entries, or 1 of gamma 1.0.
fn identity_trc(icc: &[u8]) -> bool {
    let trc = icc_tag(icc, b"kTRC");
    &trc[..4] == b"curv"
        && match be32(trc, 8) {
            0 => true,
            1 => be16(trc, 12) == 0x0100,
            _ => false,
        }
}

/// The `Lr32` block's record count, from the first `8BIMLr32` key.
fn lr32_count(bytes: &[u8], large: bool) -> i16 {
    let at = bytes
        .windows(8)
        .position(|w| w == b"8BIMLr32")
        .expect("an Lr32 block");
    let count = at + 8 + if large { 8 } else { 4 };
    i16::from_be_bytes([bytes[count], bytes[count + 1]])
}

/// The last `pixels` f32 samples of a file: a Raw single-plane composite.
fn last_plane(bytes: &[u8], pixels: usize) -> Vec<f32> {
    bytes[bytes.len() - pixels * 4..]
        .chunks_exact(4)
        .map(|s| f32::from_be_bytes([s[0], s[1], s[2], s[3]]))
        .collect()
}

// ------------------------------------------------------- the documents --

fn pattern(i: u32, seed: u32, partial: bool) -> [u16; 4] {
    let r = (i * 7919 + seed * 101) % 251;
    let g = (i * 104_729 + seed * 37) % 241;
    let b = (i * 1299 + seed * 7) % 239;
    let a = if partial && i % 3 == 1 {
        30_000 + (i as u16) * 300
    } else {
        65535
    };
    [(r * 257) as u16, (g * 265) as u16, (b * 270) as u16, a]
}

fn raster(
    name: &str,
    w: u32,
    h: u32,
    offset: (i32, i32),
    seed: u32,
    partial: bool,
) -> PrecisionDocumentLayer {
    let pixels: Vec<[u16; 4]> = (0..w * h).map(|i| pattern(i, seed, partial)).collect();
    let image = PrecisionImage::from_rgba16(w, h, pixels, IccProfile::srgb(), None).unwrap();
    PrecisionDocumentLayer::raster(name, image, offset, 1.0, true).unwrap()
}

/// `layers` (sRGB 16-bit) promoted to float and converted to a 32-bit Gray
/// document under `icc`, else the built-in `space`.
fn gray32(
    layers: Vec<PrecisionDocumentLayer>,
    icc: Option<&[u8]>,
    space: &str,
) -> *mut PrecisionDocument {
    let rgb = PrecisionDocument::from_parts(
        W,
        H,
        IccProfile::srgb(),
        Metadata::default(),
        Resolution::default(),
        layers,
    )
    .unwrap();
    let rgb = Box::into_raw(Box::new(rgb));
    let mut err = ptr::null_mut();
    let float = unsafe { dr_precision_doc_promote_to_float(rgb, LIMIT, 0, ptr::null(), &mut err) };
    assert!(!float.is_null(), "{}", take_err_string(err));
    free(rgb);
    let options = CString::new(format!(r#"{{"space":"{space}"}}"#)).unwrap();
    let (bytes, length) = icc.map_or((ptr::null(), 0), |b| (b.as_ptr(), b.len()));
    let mut err = ptr::null_mut();
    let gray = unsafe {
        dr_precision_doc_convert_to_gray(
            float,
            bytes,
            length,
            options.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!gray.is_null(), "to gray: {}", take_err_string(err));
    free(float);
    gray
}

fn index(doc: *const PrecisionDocument, name: &str) -> usize {
    let doc = unsafe { &*doc };
    (0..doc.layer_count())
        .find(|&i| doc.layer(i).unwrap().name() == name)
        .unwrap_or_else(|| panic!("no record {name}"))
}

fn then(doc: *mut PrecisionDocument, request: Value, input: &[u16]) -> *mut PrecisionDocument {
    let out = edit(doc, &request, input)
        .unwrap_or_else(|e| panic!("{request}: {e}"))
        .unwrap_or_else(|| panic!("{request} changed nothing"));
    free(doc);
    out
}

/// A 32-bit Dot Gain 20 % document: an opaque Backdrop, and a Shape with
/// partial alpha at 50 % opacity, masked, inside a group.
fn structured() -> *mut PrecisionDocument {
    let mut doc = gray32(
        vec![
            raster("Backdrop", W, H, (0, 0), 1, false),
            raster("Shape", 5, 4, (2, 1), 2, true),
        ],
        None,
        "dot_gain_20",
    );
    let shape = index(doc, "Shape");
    let mask: Vec<u16> = (0..20u16).map(|i| 65535 - i * 3001).collect();
    doc = then(
        doc,
        json!({"op":"set_mask","layer":shape,"width":5,"height":4,"x":2,"y":1,"linked":true}),
        &mask,
    );
    let faded = unsafe {
        doctored_core::ffi_precision::dr_precision_doc_set_layer_opacity(
            doc,
            shape,
            0.5,
            ptr::null_mut(),
        )
    };
    assert!(!faded.is_null());
    free(doc);
    doc = faded;
    let shape = index(doc, "Shape");
    then(
        doc,
        json!({"op":"group_layers","layers":[shape],"name":"Group"}),
        &[],
    )
}

// ------------------------------------------------------------ the tests --

#[test]
fn photoshops_layers32_opens_as_a_32_bit_gray_document() {
    let Some(path) = fixture() else { return };
    let r = report(&path);
    assert_eq!(r["import_precision"], "gray", "{r}");
    assert_eq!(r["depth"], 32);
    assert_eq!(r["layered_available"], true, "{r}");
    assert_eq!(r["stored_composite_available"], true, "{r}");
    let doc = open(&path, 0);
    let d = descriptor(doc);
    assert_eq!(d["color_mode"], "gray");
    assert_eq!(d["gray"]["depth"], 32);
    assert_eq!(d["gray"]["profile"], "Linear Grayscale Profile");
    let records = layers(doc);
    let names: Vec<&str> = records.iter().map(|(n, _, _)| n.as_str()).collect();
    assert_eq!(names, ["Base", "Ramp"]);
    assert!(records[1].2.is_some(), "Ramp keeps its mask");
    for (name, samples, _) in &records {
        for p in samples.as_ref().expect("a raster") {
            assert!(p[0] == p[1] && p[1] == p[2], "{name}: R = G = B, {p:?}");
        }
    }
    free(doc);
}

#[test]
fn layers32_renders_and_opens_its_composite_as_photoshop_stored_it() {
    let Some(path) = fixture() else { return };
    let ps = last_plane(&std::fs::read(&path).unwrap(), PS_W * PS_H);
    // The layered render, matted over white as the stored plane is.
    let doc = open(&path, 0);
    let ours = projection(doc).to_rgba32().unwrap();
    assert_eq!(ours.len(), ps.len());
    for (i, (p, &want)) in ours.iter().zip(&ps).enumerate() {
        let (c, a) = (f64::from(p[0]), f64::from(p[3]));
        let got = c * a + (1.0 - a);
        let bound = 1e-3 * f64::from(want).abs() + 1e-6;
        assert!(
            (got - f64::from(want)).abs() <= bound,
            "pixel {i}: ours {got}, Photoshop {want}"
        );
    }
    free(doc);
    // Mode 1 is the stored plane itself, bit for bit.
    let doc = open(&path, 1);
    let composite = unsafe { &*doc }
        .layer_source(0)
        .unwrap()
        .to_rgba32()
        .unwrap();
    for (i, (p, want)) in composite.iter().zip(&ps).enumerate() {
        assert_eq!(p[0].to_bits(), want.to_bits(), "pixel {i}");
        assert!(p[0] == p[1] && p[1] == p[2] && p[3] == 1.0, "pixel {i}");
    }
    free(doc);
}

#[test]
fn a_32_bit_gray_document_writes_photoshops_32_bit_gray_psd_and_psb() {
    let dir = TempDir::new().unwrap();
    let doc = structured();
    let source = layers(doc);
    let projection = projection(doc);
    for (container, large, name) in [(0, false, "g.psd"), (1, true, "g.psb")] {
        let path = dir.path().join(name);
        save(doc, &projection, &path, container, 0).unwrap_or_else(|e| panic!("{name}: {e}"));
        let bytes = std::fs::read(&path).unwrap();
        assert_eq!(be16(&bytes, 4), if large { 2 } else { 1 }, "{name} version");
        assert_eq!(be16(&bytes, 12), 1, "{name}: one channel");
        assert_eq!(be16(&bytes, 22), 32, "{name}: depth 32");
        assert_eq!(be16(&bytes, 24), 1, "{name}: colour mode 1");
        assert!(
            !bytes.windows(8).any(|w| w == b"8BIMMt32"),
            "{name}: no Mt32 block"
        );
        assert!(lr32_count(&bytes, large) > 0, "{name}: positive Lr32 count");
        let icc = icc_resource(&bytes).expect("resource 1039");
        assert_eq!(icc_name(&icc), "Dot Gain 20% (Linear Grayscale Profile)");
        assert!(identity_trc(&icc), "{name}: a gamma-1.0 kTRC");
        let reopened = open(&path, 0);
        let d = descriptor(reopened);
        assert_eq!(d["color_mode"], "gray");
        assert_eq!(d["gray"]["depth"], 32);
        assert_eq!(d["gray"]["profile"], "Dot Gain 20%", "{name}: relabelled");
        assert_eq!(
            twin_bytes(reopened),
            twin_bytes(doc),
            "{name}: the built-in's twin"
        );
        assert_eq!(
            layers(reopened),
            source,
            "{name}: samples and masks bit for bit"
        );
        free(reopened);
    }
    // Flattened: one record, and the composite plane is c·a + (1 − a).
    let path = dir.path().join("flat.psd");
    save(doc, &projection, &path, 0, 1).unwrap();
    let bytes = std::fs::read(&path).unwrap();
    let plane = last_plane(&bytes, (W * H) as usize);
    let want: Vec<u32> = projection
        .to_rgba32()
        .unwrap()
        .iter()
        .map(|p| {
            let (c, a) = (f64::from(p[0]), f64::from(p[3]));
            ((c * a + (1.0 - a)) as f32).to_bits()
        })
        .collect();
    assert_eq!(plane.iter().map(|v| v.to_bits()).collect::<Vec<_>>(), want);
    assert_eq!(lr32_count(&bytes, false), 1);
    // A layered TIFF is refused: the 32-bit layered TIFF is RGB only.
    let tiff = dir.path().join("g.tif");
    assert_eq!(
        save(doc, &projection, &tiff, 2, 0).unwrap_err(),
        TIFF_REFUSAL
    );
    assert!(!tiff.exists());
    free(doc);
}

#[test]
fn the_report_discloses_the_linear_twin_relabel() {
    let dir = TempDir::new().unwrap();
    let doc = structured();
    let path = dir.path().join("g.psd");
    save(doc, &projection(doc), &path, 0, 0).unwrap();
    free(doc);
    let r = report(&path);
    assert_eq!(r["profile"], "Dot Gain 20%", "{r}");
    let observations: Vec<&str> = r["observations"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(Value::as_str)
        .collect();
    assert!(observations.contains(&RELABELLED), "{r}");
    assert!(observations.contains(
        &"The embedded profile “Dot Gain 20% (Linear Grayscale Profile)” is the linear (gamma 1.0) twin of Dot Gain 20% that a 32-bit PSD carries; the document takes Dot Gain 20%, and its float samples are unchanged."
    ), "{r}");
    assert!(!observations.contains(&GENERAL), "{r}");
    let Some(path) = fixture() else { return };
    let r = report(&path);
    assert_eq!(r["profile"], "Linear Grayscale Profile");
    let observations = r["observations"].as_array().unwrap();
    assert!(observations.contains(&json!(GENERAL)), "{r}");
    assert!(!observations.contains(&json!(RELABELLED)), "{r}");
}

#[test]
fn layers32_resaved_keeps_photoshops_profile_and_samples() {
    let Some(path) = fixture() else { return };
    let original = icc_resource(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(original.len(), 384);
    let doc = open(&path, 0);
    let dir = TempDir::new().unwrap();
    let out = dir.path().join("resaved.psd");
    save(doc, &projection(doc), &out, 0, 0).unwrap();
    assert_eq!(
        icc_resource(&std::fs::read(&out).unwrap()).unwrap(),
        original
    );
    let reopened = open(&out, 0);
    assert_eq!(
        descriptor(reopened)["gray"]["profile"],
        "Linear Grayscale Profile"
    );
    assert_eq!(layers(reopened), layers(doc));
    free(reopened);
    free(doc);
}

// ------------------------------------------------- the kept-gray TIFF --

struct Exported {
    bytes: Vec<u8>,
    previewed: bool,
}

fn export(doc: *const PrecisionDocument, request: &str) -> Exported {
    let request = CString::new(request).unwrap();
    let mut err = ptr::null_mut();
    let result = unsafe {
        dr_precision_doc_export_encode(
            doc,
            ptr::null(),
            request.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!result.is_null(), "{}", take_err_string(err));
    let mut len = 0usize;
    let bytes = unsafe {
        let p = dr_export_result_bytes(result, &mut len);
        std::slice::from_raw_parts(p, len).to_vec()
    };
    let mut err = ptr::null_mut();
    let preview = unsafe { dr_export_result_preview(result, &mut err) };
    let previewed = !preview.is_null();
    if previewed {
        unsafe { doctored_core::ffi::dr_image_free(preview) };
    } else {
        eprintln!("preview: {}", take_err_string(err));
    }
    unsafe { dr_export_result_free(result) };
    Exported { bytes, previewed }
}

#[test]
fn a_32_bit_gray_document_exports_a_one_or_two_sample_float_tiff() {
    let dir = TempDir::new().unwrap();
    let icc = gray_icc("Test Gray 2.2", 2.2);
    // Opaque: one sample.
    let request =
        CString::new(r#"{"depth":32,"fill_srgb":[0.4,0.4,0.4,1],"layer_name":"Background"}"#)
            .unwrap();
    let mut err = ptr::null_mut();
    let opaque = unsafe {
        dr_precision_doc_new_gray(
            W,
            H,
            icc.as_ptr(),
            icc.len(),
            request.as_ptr(),
            LIMIT,
            0,
            &mut err,
        )
    };
    assert!(!opaque.is_null(), "{}", take_err_string(err));
    let transparent = gray32(
        vec![raster("Layer", W, H, (0, 0), 3, true)],
        Some(&icc),
        "dot_gain_20",
    );
    for (doc, channels, what) in [(opaque, 1u8, "opaque"), (transparent, 2, "transparent")] {
        let want = projection(doc).to_rgba32().unwrap();
        let out = export(
            doc,
            r#"{"format":"tiff","bit_depth":32,"profile":"document"}"#,
        );
        assert!(out.previewed, "{what}: the preview decodes");
        let plan = tiff_float::inspect(&out.bytes, what).unwrap();
        assert_eq!(plan.channels, channels, "{what}");
        assert_eq!(plan.sample_type, FloatSampleType::F32, "{what}");
        assert_eq!(
            plan.alpha,
            (channels == 2).then_some(false),
            "{what}: ExtraSamples 2"
        );
        let decoded = tiff_float::decode(&out.bytes, what, LIMIT).unwrap();
        assert_eq!(
            decoded.icc.as_deref(),
            Some(&icc[..]),
            "{what}: the Gray ICC"
        );
        let samples: Vec<u32> = want
            .iter()
            .flat_map(|p| {
                let mut s = vec![p[0].to_bits()];
                if channels == 2 {
                    s.push(p[3].to_bits());
                }
                s
            })
            .collect();
        assert_eq!(
            decoded
                .samples
                .iter()
                .map(|v| v.to_bits())
                .collect::<Vec<_>>(),
            samples,
            "{what}: R and alpha bit for bit"
        );
        // Reopened, it is a 32-bit Gray document with the same samples.
        let path = dir.path().join(format!("{what}.tif"));
        std::fs::write(&path, &out.bytes).unwrap();
        let reopened = PrecisionDocument::open_preserving(path.to_str().unwrap(), LIMIT, 0, None)
            .unwrap_or_else(|e| panic!("{what}: {e}"));
        assert!(reopened.is_gray(), "{what}");
        let d = reopened.descriptor().unwrap();
        assert_eq!(d["gray"]["depth"], 32, "{what}");
        assert_eq!(d["gray"]["profile"], "Test Gray 2.2", "{what}");
        let back = reopened.layer_source(0).unwrap().to_rgba32().unwrap();
        assert_eq!(
            f32_bits(&back),
            f32_bits(
                &want
                    .iter()
                    .map(|p| [p[0], p[0], p[0], p[3]])
                    .collect::<Vec<_>>()
            ),
            "{what}: reopened bit for bit"
        );
        // Half samples: BitsPerSample 16.
        let half = export(
            doc,
            r#"{"format":"tiff","bit_depth":32,"profile":"document","float_samples":"f16"}"#,
        );
        assert!(half.previewed, "{what}: the half preview decodes");
        let plan = tiff_float::inspect(&half.bytes, what).unwrap();
        assert_eq!(
            (plan.channels, plan.sample_type),
            (channels, FloatSampleType::F16)
        );
        free(doc);
    }
}
