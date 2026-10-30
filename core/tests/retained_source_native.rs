//! Independent asset framing, corruption/preflight and original-sample oracles.
//! Source operations run through their C ABI; native files must stand alone.
use crate::group_roots;
use group_roots::GroupRoots;
use std::ffi::{c_char, CStr, CString};
use std::ptr;
use std::sync::Arc;

use doctored_core::doc::DrDocument;
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_doc::{dr_doc_free, dr_doc_open, dr_doc_save_native};
use doctored_core::ffi_precision::{
    dr_precision_doc_free, dr_precision_doc_open, dr_precision_doc_save_native,
    dr_precision_resources_report,
};
use doctored_core::ffi_retained_source::*;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use image::{Rgba, RgbaImage};
use serde_json::{json, Value};
use tempfile::TempDir;

use crate::resource_report;

fn error(p: *mut c_char) -> String {
    if p.is_null() {
        return String::new();
    }
    let text = unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned();
    unsafe { dr_string_free(p) };
    text
}
fn value(p: *mut c_char, err: *mut c_char) -> Value {
    assert!(err.is_null(), "{}", error(err));
    assert!(!p.is_null());
    let out = serde_json::from_slice(unsafe { CStr::from_ptr(p) }.to_bytes()).unwrap();
    unsafe { dr_string_free(p) };
    out
}
fn byte_edit(doc: &DrDocument, input: Option<&DrDocument>, request: Value) -> DrDocument {
    let request = CString::new(request.to_string()).unwrap();
    let mut err = ptr::null_mut();
    let p = unsafe {
        dr_doc_mixed_source_edit(
            doc,
            input.map_or(ptr::null(), |v| v),
            ptr::null(),
            request.as_ptr(),
            REFERENCE_WORKING_BYTES,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!p.is_null(), "{}", error(err));
    let out = unsafe { &*p }.clone();
    unsafe { dr_doc_free(p) };
    out
}
fn typed_edit(
    doc: &PrecisionDocument,
    input: Option<&PrecisionDocument>,
    request: Value,
) -> PrecisionDocument {
    let request = CString::new(request.to_string()).unwrap();
    let mut err = ptr::null_mut();
    let p = unsafe {
        dr_precision_doc_source_edit(
            doc,
            input.map_or(ptr::null(), |v| v),
            request.as_ptr(),
            REFERENCE_WORKING_BYTES,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!p.is_null(), "{}", error(err));
    let out = unsafe { &*p }.clone();
    unsafe { dr_precision_doc_free(p) };
    out
}
fn scale(layer: usize, factor: f64) -> Value {
    json!({"op":"transform","layer":layer,"matrix":[factor,0.0,0.0,factor,0.0,0.0],"filter":"nearest"})
}
fn place() -> Value {
    json!({"op":"place","source_id":"source-original-42","name":"Original","x":0,"y":0})
}
fn byte_source(profile: Arc<IccProfile>) -> DrDocument {
    let mut doc = DrDocument::from_pixels(RgbaImage::from_fn(4, 2, |x, y| {
        Rgba([
            30 + x as u8,
            121,
            199 - y as u8,
            [0, 1, 128, 255][x as usize],
        ])
    }));
    doc.profile = profile;
    doc
}
fn typed_source(profile: Arc<IccProfile>) -> PrecisionDocument {
    let image = PrecisionImage::from_rgba16(
        4,
        2,
        (0..8)
            .map(|i| {
                [
                    30001 + i,
                    12003,
                    49997 - i,
                    [0, 1, 32769, 65535][i as usize % 4],
                ]
            })
            .collect(),
        profile,
        None,
    )
    .unwrap();
    PrecisionDocument::from_image(image, Metadata::default(), Resolution::default()).unwrap()
}
fn byte_native(doc: &DrDocument, dir: &TempDir) -> Vec<u8> {
    let path = dir.path().join("portable-byte.rz");
    let cpath = CString::new(path.to_str().unwrap()).unwrap();
    let mut err = ptr::null_mut();
    assert!(
        unsafe { dr_doc_save_native(doc, cpath.as_ptr(), &mut err) },
        "{}",
        error(err)
    );
    std::fs::read(path).unwrap()
}
fn typed_native(doc: &PrecisionDocument, dir: &TempDir) -> Vec<u8> {
    let path = dir.path().join("portable-typed.rz");
    let cpath = CString::new(path.to_str().unwrap()).unwrap();
    let mut err = ptr::null_mut();
    assert!(
        unsafe {
            dr_precision_doc_save_native(
                doc,
                cpath.as_ptr(),
                REFERENCE_WORKING_BYTES,
                0,
                ptr::null(),
                &mut err,
            )
        },
        "{}",
        error(err)
    );
    std::fs::read(path).unwrap()
}
fn byte_open(bytes: &[u8], dir: &TempDir) -> Result<DrDocument, String> {
    let path = dir.path().join("recovered-byte.rz");
    std::fs::write(&path, bytes).unwrap();
    let path = CString::new(path.to_str().unwrap()).unwrap();
    let mut err = ptr::null_mut();
    let p = unsafe { dr_doc_open(path.as_ptr(), &mut err) };
    if p.is_null() {
        return Err(error(err));
    }
    let doc = unsafe { &*p }.clone();
    unsafe { dr_doc_free(p) };
    Ok(doc)
}
fn typed_open(bytes: &[u8], dir: &TempDir) -> Result<PrecisionDocument, String> {
    let path = dir.path().join("recovered-typed.rz");
    std::fs::write(&path, bytes).unwrap();
    let path = CString::new(path.to_str().unwrap()).unwrap();
    let mut err = ptr::null_mut();
    let p = unsafe {
        dr_precision_doc_open(
            path.as_ptr(),
            REFERENCE_WORKING_BYTES,
            0,
            ptr::null(),
            &mut err,
        )
    };
    if p.is_null() {
        return Err(error(err));
    }
    let doc = unsafe { &*p }.clone();
    unsafe { dr_precision_doc_free(p) };
    Ok(doc)
}
fn byte_report(docs: &[&DrDocument]) -> Value {
    let handles: Vec<*const DrDocument> = docs.iter().map(|v| *v as *const _).collect();
    resource_report::report(&handles, &[], &[], &[], 0, 0).expect("a resource report")
}
fn typed_report(docs: &[&PrecisionDocument]) -> Value {
    let handles: Vec<*const PrecisionDocument> = docs.iter().map(|v| *v as *const _).collect();
    let mut err = ptr::null_mut();
    value(
        unsafe {
            dr_precision_resources_report(handles.as_ptr(), handles.len(), ptr::null(), 0, &mut err)
        },
        err,
    )
}
fn u32_at(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap())
}
fn u64_at(bytes: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(bytes[at..at + 8].try_into().unwrap())
}
struct Tail {
    start: usize,
    id: usize,
    profile: usize,
    dimensions: usize,
    png: usize,
    bindings: usize,
}
fn tail(bytes: &[u8]) -> Tail {
    // Our small deterministic PNG fixtures contain no RSRC payload bytes.
    let start = bytes.windows(4).position(|v| v == b"RSRC").unwrap();
    assert_eq!(u32_at(bytes, start + 4), 1);
    let id = start + 8;
    let profile = id + 4 + u32_at(bytes, id) as usize;
    let dimensions = profile + 4 + u32_at(bytes, profile) as usize;
    let png = dimensions + 16;
    let bindings = png + u64_at(bytes, dimensions + 8) as usize;
    Tail {
        start,
        id,
        profile,
        dimensions,
        png,
        bindings,
    }
}

#[test]
fn byte_sources_roundtrip_original_profile_shared_identity_and_exact_restore() {
    let dir = TempDir::new().unwrap();
    let input = byte_source(IccProfile::display_p3());
    let base = byte_source(IccProfile::srgb());
    let placed = byte_edit(&base, Some(&input), place());
    let smaller = byte_edit(&placed, None, scale(1, 0.5));
    let duplicate = smaller.duplicating_layer(1).unwrap();
    let encoded = byte_native(&duplicate, &dir);
    // Offset 8: 12E-1's identity wrapper owns the outermost word.
    assert_eq!(u32_at(&encoded, 8), 14);
    let record = tail(&encoded);
    assert_eq!(u32_at(&encoded, record.bindings), 2);
    assert_eq!(
        encoded
            .windows(b"source-original-42".len())
            .filter(|v| *v == b"source-original-42")
            .count(),
        1
    );
    std::fs::remove_file(dir.path().join("portable-byte.rz")).unwrap();
    let reopened = byte_open(&encoded, &dir).unwrap();
    let a = reopened.layers[1]
        .effective_source(&reopened.profile)
        .unwrap();
    let b = reopened.layers[2]
        .effective_source(&reopened.profile)
        .unwrap();
    assert!(Arc::ptr_eq(&a.asset, &b.asset));
    assert!(a.asset.rgba8().unwrap().same_bytes(&input.layers[0].pixels));
    assert_eq!(a.asset.profile().bytes(), input.profile.bytes());
    assert_eq!(reopened.profile.bytes(), base.profile.bytes());
    assert_eq!(
        *reopened.layers[1].pixels.contiguous_image(),
        *duplicate.layers[1].pixels.contiguous_image()
    );
    let restored = byte_edit(&reopened, None, scale(1, 2.0));
    assert_eq!(
        *restored.layers[1].pixels.contiguous_image(),
        *placed.layers[1].pixels.contiguous_image()
    );
    assert_eq!(
        restored.source_info(1).unwrap()["source_id"],
        "source-original-42"
    );
    let report = byte_report(&[&reopened]);
    assert_eq!(
        report["categories"]["embedded_source_pixels"]["unique_bytes"],
        32
    );
    assert_eq!(
        report["categories"]["embedded_source_pixels"]["allocation_count"],
        1
    );
    assert_eq!(
        byte_report(&[&reopened, &reopened])["unique_retained_bytes"],
        report["unique_retained_bytes"]
    );
}

#[test]
fn typed_sources_roundtrip_sub_byte_rgb_alpha_hidden_rgb_and_shared_native_asset() {
    let dir = TempDir::new().unwrap();
    let source = typed_source(IccProfile::display_p3());
    let base = typed_source(IccProfile::srgb());
    let placed = typed_edit(&base, Some(&source), place());
    let smaller = typed_edit(&placed, None, scale(1, 0.5));
    let duplicate = smaller.duplicate_layer(1).unwrap().unwrap();
    let bytes = typed_native(&duplicate, &dir);
    assert_eq!(u32_at(&bytes, 8), 15);
    let record = tail(&bytes);
    assert_eq!(u32_at(&bytes, record.bindings), 2);
    // Independently decode the embedded original PNG, not the layer fallback.
    let png_len = u64_at(&bytes, record.dimensions + 8) as usize;
    let decoded = image::load_from_memory(&bytes[record.png..record.png + png_len]).unwrap();
    assert_eq!(decoded.color(), image::ColorType::Rgba16);
    assert_eq!(
        decoded.to_rgba16().as_raw(),
        &source
            .layer_source(0)
            .unwrap()
            .to_rgba16()
            .ok()
            .unwrap()
            .iter()
            .flat_map(|p| p.iter().copied())
            .collect::<Vec<_>>()
    );
    std::fs::remove_file(dir.path().join("portable-typed.rz")).unwrap();
    let reopened = typed_open(&bytes, &dir).unwrap();
    let a = reopened.layer(1).unwrap().effective_source().unwrap();
    let b = reopened.layer(2).unwrap().effective_source().unwrap();
    assert!(Arc::ptr_eq(&a.asset, &b.asset));
    assert_eq!(
        a.asset.rgba16().unwrap().to_rgba16().ok(),
        source.layer_source(0).unwrap().to_rgba16().ok()
    );
    assert_eq!(a.asset.profile().bytes(), source.profile().bytes());
    assert_eq!(reopened.profile().bytes(), base.profile().bytes());
    assert_eq!(
        reopened.layer_source(1).unwrap().to_rgba16().ok(),
        duplicate.layer_source(1).unwrap().to_rgba16().ok()
    );
    let restored = typed_edit(&reopened, None, scale(1, 2.0));
    assert_eq!(
        restored.layer_source(1).unwrap().to_rgba16().ok(),
        placed.layer_source(1).unwrap().to_rgba16().ok()
    );
    assert_eq!(
        typed_report(&[&reopened, &reopened])["unique_retained_bytes"],
        typed_report(&[&reopened])["unique_retained_bytes"]
    );
    assert!(
        typed_report(&[&reopened])["shared_bytes_saved"]
            .as_u64()
            .unwrap()
            > 64
    );
}

#[test]
fn old_source_free_versions_stay_readable_and_source_storage_never_crosses_readers() {
    let dir = TempDir::new().unwrap();
    let byte = byte_source(IccProfile::srgb());
    let typed = typed_source(IccProfile::srgb());
    let old_byte = byte_native(&byte, &dir);
    let old_typed = typed_native(&typed, &dir);
    assert_eq!(u32_at(&old_byte, 8), 10);
    assert_eq!(u32_at(&old_typed, 8), 13);
    assert!(byte_open(&old_byte, &dir).is_ok());
    assert!(typed_open(&old_typed, &dir).is_ok());
    let new_byte = byte_native(&byte_edit(&byte, Some(&byte), place()), &dir);
    let new_typed = typed_native(&typed_edit(&typed, Some(&typed), place()), &dir);
    assert!(typed_open(&new_byte, &dir)
        .err()
        .unwrap()
        .contains("not a supported"));
    assert!(byte_open(&new_typed, &dir)
        .err()
        .unwrap()
        .contains("no RGBA8 fallback"));
}

fn corruptions(bytes: &[u8]) -> Vec<Vec<u8>> {
    let t = tail(bytes);
    let mut out = Vec::new();
    for (at, data) in [
        (t.start + 4, 1025_u32.to_le_bytes().to_vec()),
        (t.id, 129_u32.to_le_bytes().to_vec()),
        (t.id + 4, b"!".to_vec()),
        (t.profile, (16_u32 * 1024 * 1024 + 1).to_le_bytes().to_vec()),
        (t.profile + 4, [0_u8; 16].to_vec()),
        (t.dimensions, 50000_u32.to_le_bytes().to_vec()),
        (t.dimensions + 8, u64::MAX.to_le_bytes().to_vec()),
        (t.bindings, 0_u32.to_le_bytes().to_vec()),
        (t.bindings + 4, 1000_u32.to_le_bytes().to_vec()),
        (t.bindings + 8, 1_u32.to_le_bytes().to_vec()),
        (t.bindings + 12, vec![4]),
        (t.bindings + 13, vec![1]),
        (t.bindings + 16, f64::NAN.to_le_bytes().to_vec()),
        (t.bindings + 16, 2.0_f64.to_le_bytes().to_vec()),
    ] {
        let mut corrupted = bytes.to_vec();
        corrupted[at..at + data.len()].copy_from_slice(&data);
        out.push(corrupted);
    }
    // Independently duplicate the asset record: identities must be unique,
    // and even a new identity cannot introduce an unreferenced hidden asset.
    let mut two_assets = bytes[..t.start + 4].to_vec();
    two_assets.extend(2_u32.to_le_bytes());
    two_assets.extend_from_slice(&bytes[t.id..t.bindings]);
    let duplicate_at = two_assets.len();
    two_assets.extend_from_slice(&bytes[t.id..t.bindings]);
    two_assets.extend_from_slice(&bytes[t.bindings..]);
    out.push(two_assets.clone());
    two_assets[duplicate_at + 4] = b'X';
    out.push(two_assets);
    let mut two_bindings = bytes[..t.bindings].to_vec();
    two_bindings.extend(2_u32.to_le_bytes());
    two_bindings.extend_from_slice(&bytes[t.bindings + 4..]);
    two_bindings.extend_from_slice(&bytes[t.bindings + 4..]);
    out.push(two_bindings);
    let mut trailing = bytes.to_vec();
    trailing.push(0);
    out.push(trailing);
    for at in [
        t.start,
        t.id + 1,
        t.profile + 1,
        t.png + 4,
        t.bindings + 4,
        bytes.len() - 1,
    ] {
        out.push(bytes[..at].to_vec());
    }
    out
}

#[test]
fn byte_source_table_refuses_bad_lengths_profiles_references_flags_transforms_and_truncation() {
    let dir = TempDir::new().unwrap();
    let source = byte_source(IccProfile::srgb());
    let doc = byte_edit(&source, Some(&source), place());
    let bytes = byte_native(&doc, &dir);
    for (index, malformed) in corruptions(&bytes).iter().enumerate() {
        assert!(
            byte_open(malformed, &dir).is_err(),
            "corruption {index} was accepted"
        );
    }
    let mut malformed = bytes.clone();
    let t = tail(&bytes);
    malformed[t.bindings + 8..t.bindings + 12].copy_from_slice(&1_u32.to_le_bytes());
    // A corrupt compressed earlier fallback must not mask the invalid tail reference.
    let idat = malformed.windows(4).position(|v| v == b"IDAT").unwrap();
    malformed[idat + 4] ^= 0xff;
    assert!(byte_open(&malformed, &dir)
        .err()
        .unwrap()
        .contains("source reference"));
}

#[test]
fn typed_source_table_refuses_corruption_before_decoding_any_earlier_fallback() {
    let dir = TempDir::new().unwrap();
    let source = typed_source(IccProfile::srgb());
    let doc = typed_edit(&source, Some(&source), place());
    let bytes = typed_native(&doc, &dir);
    for (index, malformed) in corruptions(&bytes).iter().enumerate() {
        assert!(
            typed_open(malformed, &dir).is_err(),
            "corruption {index} was accepted"
        );
    }
    let mut malformed = bytes.clone();
    let t = tail(&bytes);
    malformed[t.bindings + 8..t.bindings + 12].copy_from_slice(&1_u32.to_le_bytes());
    let idat = malformed.windows(4).position(|v| v == b"IDAT").unwrap();
    malformed[idat + 4] ^= 0xff;
    assert!(typed_open(&malformed, &dir)
        .err()
        .unwrap()
        .contains("source reference"));
}

#[test]
fn typed_native_admission_counts_source_once_and_every_serialized_fallback() {
    let dir = TempDir::new().unwrap();
    let source = typed_source(IccProfile::srgb());
    let placed = typed_edit(&source, Some(&source), place());
    let small = typed_edit(&placed, None, scale(1, 0.5));
    let duplicated = small.duplicate_layer(1).unwrap().unwrap();
    // Base8 + two fallback2 + one source8 pixels, all RGBA16 => 160 bytes.
    let plan = duplicated.native_plan().unwrap();
    assert_eq!(plan.serialized_pixel_bytes, (8 + 2 + 2 + 8) * 8);
    assert_eq!(
        plan.serialized_pixel_bytes - small.native_plan().unwrap().serialized_pixel_bytes,
        16
    );
    let encoded = typed_native(&duplicated, &dir);
    assert!(encoded.len() as u64 <= plan.encoded_bound);
    let reopened = typed_open(&encoded, &dir).unwrap();
    assert!(reopened.retained_bytes() <= plan.reopen_retained_bytes);
    let total = duplicated.retained_bytes() + plan.write_scratch_bytes;
    assert!(duplicated.encode_native(total - 1, 0, None).is_err());
    assert!(duplicated.encode_native(total, 0, None).is_ok());
    assert!(PrecisionDocument::parse_native(&encoded, 1, 0, None).is_err());
}

#[test]
fn stale_bindings_keep_live_allocations_but_never_resurrect_in_native_files() {
    let dir = TempDir::new().unwrap();
    let input = byte_source(IccProfile::display_p3());
    let base = byte_source(IccProfile::srgb());
    let placed = byte_edit(&base, Some(&input), place());
    let changed = placed
        .with_layer_pixels(1, RgbaImage::from_pixel(4, 2, Rgba([99, 98, 97, 255])))
        .unwrap();
    assert!(!changed.source_info(1).unwrap()["valid"].as_bool().unwrap());
    assert_eq!(
        byte_report(&[&changed])["categories"]["embedded_source_pixels"]["unique_bytes"],
        32
    );
    let bytes = byte_native(&changed, &dir);
    assert_eq!(u32_at(&bytes, 8), 10);
    let reopened = byte_open(&bytes, &dir).unwrap();
    assert!(reopened.layers[1].retained_source.is_none());
    assert_eq!(
        *reopened.layers[1].pixels.contiguous_image(),
        *changed.layers[1].pixels.contiguous_image()
    );
    let input = typed_source(IccProfile::display_p3());
    let base = typed_source(IccProfile::srgb());
    let placed = typed_edit(&base, Some(&input), place());
    let paint = doctored_core::precision_paint::PaintEdit::parse(
        r#"{"op":"paint","layer":1,"color":[0.8,0.2,0.3,1.0],"color_space":"document","opacity":1.0}"#,
    ).unwrap();
    let changed = placed
        .paint(&paint, &[65535; 8], REFERENCE_WORKING_BYTES, 0, None)
        .unwrap()
        .unwrap();
    assert!(!changed.source_info(1).unwrap()["valid"].as_bool().unwrap());
    // Typed reconstruction releases invalid bindings immediately. A still-live
    // Undo snapshot remains fully counted, with the source allocation once.
    assert!(typed_report(&[&changed])["categories"]["embedded_source_identity"].is_null());
    assert_eq!(
        typed_report(&[&changed, &placed])["categories"]["embedded_source_identity"]
            ["allocation_count"],
        2
    );
    let bytes = typed_native(&changed, &dir);
    assert_eq!(u32_at(&bytes, 8), 13);
    let reopened = typed_open(&bytes, &dir).unwrap();
    assert!(reopened.layer(1).unwrap().effective_source().is_none());
}

#[test]
fn byte_writer_counts_source_assets_at_capacity_and_preserves_previous_good_file() {
    let dir = TempDir::new().unwrap();
    let source = DrDocument::from_pixels(RgbaImage::from_pixel(625, 625, Rgba([1, 2, 3, 255])));
    let mut placed = byte_edit(&source, Some(&source), place());
    placed.layers = vec![placed.layers[1].clone(); 1024];
    // Fallbacks fit the established 400M pixel cap exactly; the uniquely stored
    // original adds another390625 pixels and must be refused before PNG encode.
    let path = dir.path().join("prior-good.rz");
    source.save_native(path.to_str().unwrap()).unwrap();
    let prior = std::fs::read(&path).unwrap();
    let failure = placed.save_native(path.to_str().unwrap()).unwrap_err();
    assert!(failure.contains("embedded sources"), "{failure}");
    assert_eq!(std::fs::read(path).unwrap(), prior);
    placed.layers.truncate(1023);
    assert_eq!(placed.layers.len() * 625 * 625 + 625 * 625, 400_000_000);
    let mut invalid = placed.layers[0].retained_source.as_ref().unwrap().clone();
    if let doctored_core::source_transform::SourceTransform::Affine(ref mut matrix) =
        invalid.transform
    {
        matrix[0] = 2.0;
    }
    placed.layers[0].retained_source = Some(invalid);
    assert!(placed
        .save_native(dir.path().join("invalid.rz").to_str().unwrap())
        .unwrap_err()
        .contains("fallback extent"));
}

fn random_word(state: &mut u64) -> u64 {
    // Fixture-only xorshift stream: independent, bounded and incompressible.
    *state ^= *state << 13;
    *state ^= *state >> 7;
    *state ^= *state << 17;
    *state
}

#[test]
fn byte_source_png_larger_than_layer_metadata_cap_is_portable_without_base64() {
    let dir = TempDir::new().unwrap();
    let mut random = 0x9377_29b9_1138_ba45;
    let pixels = RgbaImage::from_fn(2048, 2048, |_, _| {
        Rgba((random_word(&mut random) as u32).to_le_bytes())
    });
    let input = DrDocument::from_pixels(pixels);
    let base = DrDocument::from_pixels(RgbaImage::from_pixel(1, 1, Rgba([0, 0, 0, 0])));
    let placed = byte_edit(&base, Some(&input), place());
    assert!(placed.layers[1].meta.is_none());
    let bytes = byte_native(&placed, &dir);
    let record = tail(&bytes);
    assert!(u64_at(&bytes, record.dimensions + 8) > 16 * 1024 * 1024);
    let reopened = byte_open(&bytes, &dir).unwrap();
    let source = reopened.layers[1]
        .effective_source(&reopened.profile)
        .unwrap();
    assert!(source
        .asset
        .rgba8()
        .unwrap()
        .same_bytes(&input.layers[0].pixels));
    assert!(reopened.layers[1].meta.is_none());
}

#[test]
fn typed_source_png_larger_than_layer_metadata_cap_keeps_genuine_codes() {
    let dir = TempDir::new().unwrap();
    let mut random = 0x9357_29b9_1108_ba45;
    let values = (0..2048 * 1024)
        .map(|_| {
            let bytes = random_word(&mut random).to_le_bytes();
            std::array::from_fn(|i| u16::from_le_bytes([bytes[2 * i], bytes[2 * i + 1]]))
        })
        .collect();
    let image = PrecisionImage::from_rgba16(2048, 1024, values, IccProfile::srgb(), None).unwrap();
    let input =
        PrecisionDocument::from_image(image, Metadata::default(), Resolution::default()).unwrap();
    let base = typed_source(IccProfile::srgb());
    let placed = typed_edit(&base, Some(&input), place());
    let bytes = typed_native(&placed, &dir);
    let record = tail(&bytes);
    assert!(u64_at(&bytes, record.dimensions + 8) > 16 * 1024 * 1024);
    let reopened = typed_open(&bytes, &dir).unwrap();
    let source = reopened.layer(1).unwrap().effective_source().unwrap();
    assert_eq!(
        source.asset.rgba16().unwrap().to_rgba16().ok(),
        input.layer_source(0).unwrap().to_rgba16().ok()
    );
}

#[test]
fn aggregate_source_profile_cap_matches_writer_and_both_readers_before_decoding() {
    let dir = TempDir::new().unwrap();
    let mut profile_bytes = IccProfile::srgb().bytes().to_vec();
    profile_bytes.resize(16 * 1024 * 1024, 0);
    profile_bytes[..4].copy_from_slice(&(16_u32 * 1024 * 1024).to_be_bytes());
    let large_profile = Arc::new(IccProfile::parse(&profile_bytes).unwrap());
    let source = byte_source(IccProfile::srgb());
    let mut placed = byte_edit(&source, Some(&source), place());
    let original = placed.layers[1].clone();
    placed.layers = (0..5)
        .map(|i| {
            let mut layer = original.clone();
            let mut binding = layer.retained_source.as_ref().unwrap().clone();
            let mut asset = (*binding.asset).clone();
            asset.id = format!("source-{i}");
            if let doctored_core::retained_source::SourcePixels::Rgba8 { profile, .. } =
                &mut asset.pixels
            {
                *profile = large_profile.clone();
            }
            binding.asset = Arc::new(asset);
            layer.retained_source = Some(binding);
            layer
        })
        .collect();
    let failure = placed
        .save_native(dir.path().join("too-many-profiles.rz").to_str().unwrap())
        .unwrap_err();
    assert!(failure.contains("profile aggregate"), "{failure}");
    // Build one independent 80 MiB table. Each original-profile slot individually
    // fits, but their aggregate must refuse before PNG or profile decoding.
    let valid_byte = byte_native(&byte_edit(&source, Some(&source), place()), &dir);
    let byte_tail = tail(&valid_byte);
    let png_len = u64_at(&valid_byte, byte_tail.dimensions + 8) as usize;
    let source_png = &valid_byte[byte_tail.png..byte_tail.png + png_len];
    let mut table = b"RSRC".to_vec();
    table.extend(5_u32.to_le_bytes());
    for i in 0..5 {
        let id = format!("source-{i}");
        table.extend((id.len() as u32).to_le_bytes());
        table.extend(id.as_bytes());
        table.extend((profile_bytes.len() as u32).to_le_bytes());
        table.extend(&profile_bytes);
        // Fifth profile alone proves the refusal; no fifth dimensions or pixels
        // are needed. This also checks that aggregate validation is early.
        if i < 4 {
            table.extend(4_u32.to_le_bytes());
            table.extend(2_u32.to_le_bytes());
            table.extend((source_png.len() as u64).to_le_bytes());
            table.extend(source_png);
        }
    }
    let mut bytes = valid_byte[..byte_tail.start].to_vec();
    bytes.extend(&table);
    assert!(byte_open(&bytes, &dir)
        .err()
        .unwrap()
        .contains("profile aggregate"));
    let input = typed_source(IccProfile::srgb());
    let valid_typed = typed_native(&typed_edit(&input, Some(&input), place()), &dir);
    let typed_tail = tail(&valid_typed);
    let mut bytes = valid_typed[..typed_tail.start].to_vec();
    bytes.extend(table);
    // Both storage readers share table preflight: the byte PNGs in this crafted
    // typed file must not be touched before the aggregate profiles refuse.
    let failure = PrecisionDocument::parse_native(&bytes, REFERENCE_WORKING_BYTES, 0, None)
        .err()
        .unwrap();
    assert!(failure.contains("profile aggregate"), "{failure}");
}

#[test]
fn source_document_profile_refuses_corruption_while_legacy_repair_stays_compatible() {
    let dir = TempDir::new().unwrap();
    let source = byte_source(IccProfile::display_p3());
    for retained in [false, true] {
        let doc = if retained {
            byte_edit(&source, Some(&source), place())
        } else {
            source.clone()
        };
        let mut bytes = byte_native(&doc, &dir);
        let profile = source.profile.bytes();
        let at = bytes
            .windows(profile.len())
            .position(|v| v == profile.as_ref())
            .unwrap();
        // The document tail precedes the source table, so this is the fallback
        // profile. Break its ICC signature while preserving every slot length.
        bytes[at + 36] ^= 0xff;
        let result = byte_open(&bytes, &dir);
        if retained {
            assert!(result.err().unwrap().contains("document RGB profile"));
        } else {
            assert!(result.unwrap().profile.is_builtin_srgb());
        }
    }
}

#[test]
fn typed_source_bindings_follow_public_postorder_through_nested_native_groups() {
    let dir = TempDir::new().unwrap();
    let input = typed_source(IccProfile::display_p3());
    let base = typed_source(IccProfile::srgb());
    let placed = typed_edit(&base, Some(&input), place());
    let duplicate = placed.duplicate_layer(1).unwrap().unwrap();
    let inner = duplicate.group_all_pass_through(0.75).unwrap().unwrap();
    let nested = inner.group_all(0.5).unwrap().unwrap();
    let bytes = typed_native(&nested, &dir);
    let reopened = typed_open(&bytes, &dir).unwrap();
    assert_eq!(reopened.layer_count(), 5);
    assert!(reopened.layer(0).unwrap().effective_source().is_none());
    let a = reopened.layer(1).unwrap().effective_source().unwrap();
    let b = reopened.layer(2).unwrap().effective_source().unwrap();
    assert!(Arc::ptr_eq(&a.asset, &b.asset));
    assert_eq!(
        a.asset.rgba16().unwrap().to_rgba16().ok(),
        input.layer_source(0).unwrap().to_rgba16().ok()
    );
    assert_eq!(a.asset.profile().bytes(), input.profile().bytes());
    for index in [3, 4] {
        assert!(reopened.layer(index).unwrap().children().is_some());
        assert!(reopened.layer(index).unwrap().effective_source().is_none());
    }
    assert_eq!(
        reopened.native_plan().unwrap().serialized_pixel_bytes,
        nested.native_plan().unwrap().serialized_pixel_bytes
    );
}

#[test]
fn byte_source_binding_cannot_target_an_adjustment_or_described_raster() {
    let dir = TempDir::new().unwrap();
    let source = byte_source(IccProfile::srgb());
    let mut placed = byte_edit(&source, Some(&source), place());
    for metadata in [
        r#"{"type":"adjustment","op":"invert","params":{}}"#,
        r#"{"type":"text","text":"Editable"}"#,
        r#"{"opaque_future_description":true}"#,
    ] {
        placed.layers[0].meta = Some(metadata.into());
        let mut bytes = byte_native(&placed, &dir);
        let table = tail(&bytes);
        // Both rasters have the same bounds, so geometry alone would allow
        // redirecting the source binding onto the metadata-bearing record.
        bytes[table.bindings + 4..table.bindings + 8].copy_from_slice(&0_u32.to_le_bytes());
        let idat = bytes.windows(4).position(|v| v == b"IDAT").unwrap();
        bytes[idat + 4] ^= 0xff;
        let failure = byte_open(&bytes, &dir).err().unwrap();
        assert!(failure.contains("without layer metadata"), "{failure}");
    }
}

// -------------------------------- G5 (12F-1): Canvas Size across a file --

/// The dyadic sub-pixel translation the rerender uses (`retained_sources.rs`'s
/// RERENDER): every f64 sum in it is exact, so the comparison below is an
/// equality rather than a tolerance.
fn rerender(layer: usize) -> Value {
    json!({"op":"transform","layer":layer,"matrix":[1.0,0.0,0.0,1.0,0.25,0.5],"filter":"bilinear"})
}

#[test]
fn a_canvas_size_that_moved_an_embedded_source_survives_a_native_round_trip_at_both_depths() {
    let dir = TempDir::new().unwrap();

    // BYTE.
    let source = byte_source(IccProfile::srgb());
    let placed = byte_edit(&source, Some(&source), place());
    let moved = placed
        .canvas_resize(12, 9, (3, 2))
        .expect("the byte canvas resize");
    let transform = moved.source_info(1).unwrap()["transform"].clone();
    assert_eq!(moved.source_info(1).unwrap()["valid"], true);

    let bytes = byte_native(&moved, &dir);
    let reopened = byte_open(&bytes, &dir).expect("the byte file reopens");
    assert_eq!(
        reopened.source_info(1).unwrap()["valid"],
        true,
        "the source is still valid after the file"
    );
    assert_eq!(
        reopened.source_info(1).unwrap()["transform"],
        transform,
        "with the IDENTICAL folded transform"
    );
    assert_eq!(
        byte_native(&reopened, &dir),
        bytes,
        "reopen then resave is byte-identical"
    );
    assert_eq!(
        byte_edit(&reopened, None, rerender(1)).layers[1]
            .pixels
            .contiguous_image()
            .as_raw(),
        byte_edit(&moved, None, rerender(1)).layers[1]
            .pixels
            .contiguous_image()
            .as_raw(),
        "and the same rerender gives the same pixels on both sides of the file"
    );

    // TYPED.
    let source = typed_source(IccProfile::srgb());
    let placed = typed_edit(&source, Some(&source), place());
    let edit = doctored_core::precision_geometry::GeometryEdit::parse(
        &json!({"op":"canvas_size","width":12,"height":9,"x":3,"y":2}).to_string(),
    )
    .unwrap();
    let moved = placed
        .geometry(&edit, REFERENCE_WORKING_BYTES, 0, None)
        .expect("Canvas Size is LIFTED at 16 bits")
        .expect("the canvas changed");
    let info = |doc: &PrecisionDocument| -> Value {
        let mut err = ptr::null_mut();
        value(
            unsafe { dr_precision_doc_source_info(doc, 1, &mut err) },
            err,
        )
    };
    let transform = info(&moved)["transform"].clone();
    assert_eq!(info(&moved)["valid"], true);

    let bytes = typed_native(&moved, &dir);
    let reopened = typed_open(&bytes, &dir).expect("the typed file reopens");
    assert_eq!(info(&reopened)["valid"], true);
    assert_eq!(info(&reopened)["transform"], transform);
    assert_eq!(
        typed_native(&reopened, &dir),
        bytes,
        "reopen then resave is byte-identical at 16 bits too"
    );
    assert_eq!(
        typed_edit(&reopened, None, rerender(1))
            .layer_source(1)
            .unwrap()
            .to_rgba16()
            .ok(),
        typed_edit(&moved, None, rerender(1))
            .layer_source(1)
            .unwrap()
            .to_rgba16()
            .ok(),
    );
}
