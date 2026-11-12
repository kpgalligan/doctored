//! Independent RAW native framing, exact cached camera development and portability.
//! The encoded fixture is deliberately opaque and its decoder unavailable: native
//! open must use the saved genuine u16 master/fallback without interpreting RAW.
use doctored_core::doc::DrDocument;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use doctored_core::retained_source::{SourceEdit, SourceInput};
use image::{Rgba, RgbaImage};
use serde_json::{json, Value};
use std::ffi::{CStr, CString};
use std::ptr;
use std::sync::Arc;
use tempfile::TempDir;

const ORIGINAL: &[u8] = b"camera-container-original\0\x00\x01\x7f\x80\xff\nnot-a-preview";
const HASH: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
fn descriptor(temperature: f64) -> Value {
    json!({"schema_version":1,"original_filename":"camera-original.NEF","content_sha256":HASH,
        "decoder":{"identifier":"unavailable.camera.decoder","version":"2001.unknown","implementation":"saved test environment"},
        "recipe":{"schema_version":1,"temperature":temperature,"tint":3.25,"exposure":0.75,
            "contrast":0.0,"saturation":1.0,"baseline_exposure":0.125,"highlight_recovery":false,
            "output_depth":16,"output_profile":"captured ICC","defaults_resolved":true}})
}
fn typed(seed: u16) -> PrecisionDocument {
    let image = PrecisionImage::from_rgba16(
        4,
        2,
        (0..8)
            .map(|i| {
                [
                    seed + i,
                    12003,
                    49997 - i,
                    [0, 1, 32769, 65535][i as usize % 4],
                ]
            })
            .collect(),
        IccProfile::display_p3(),
        None,
    )
    .unwrap();
    PrecisionDocument::from_image(image, Metadata::default(), Resolution::default()).unwrap()
}
fn byte() -> DrDocument {
    DrDocument::from_pixels(RgbaImage::from_pixel(4, 2, Rgba([29, 71, 199, 255])))
}
fn place(revision: &str) -> SourceEdit {
    SourceEdit::parse(
        &json!({"op":"place","source_id":"camera-source","revision_id":revision,
        "name":"Developed camera","x":0,"y":0,"depth_conversion":"convert_fallback"})
        .to_string(),
    )
    .unwrap()
}
fn edit8(d: &DrDocument, input: Option<SourceInput<'_>>, request: SourceEdit) -> DrDocument {
    d.source_edit_input(input, &request, REFERENCE_WORKING_BYTES, 0, None)
        .unwrap()
        .unwrap()
}
fn edit16(
    d: &PrecisionDocument,
    input: Option<SourceInput<'_>>,
    request: SourceEdit,
) -> PrecisionDocument {
    d.source_edit_input(input, &request, REFERENCE_WORKING_BYTES, 0, None)
        .unwrap()
        .unwrap()
}
fn ffi_error(error: *mut std::ffi::c_char) -> String {
    if error.is_null() {
        return "missing FFI error".into();
    }
    unsafe {
        let text = CStr::from_ptr(error).to_string_lossy().into_owned();
        doctored_core::ffi::dr_string_free(error);
        text
    }
}
fn attach8(
    d: &DrDocument,
    layer: usize,
    original: Option<(&DrDocument, usize)>,
    settings: Value,
) -> DrDocument {
    let descriptor = CString::new(settings.to_string()).unwrap();
    let mut error = ptr::null_mut();
    let out = unsafe {
        if let Some((original, original_layer)) = original {
            doctored_core::ffi_raw_source::dr_doc_attach_raw_source_from(
                d,
                layer,
                original,
                original_layer,
                descriptor.as_ptr(),
                REFERENCE_WORKING_BYTES,
                0,
                ptr::null(),
                &mut error,
            )
        } else {
            doctored_core::ffi_raw_source::dr_doc_attach_raw_source(
                d,
                layer,
                ORIGINAL.as_ptr(),
                ORIGINAL.len(),
                descriptor.as_ptr(),
                REFERENCE_WORKING_BYTES,
                0,
                ptr::null(),
                &mut error,
            )
        }
    };
    assert!(!out.is_null(), "{}", ffi_error(error));
    assert!(error.is_null());
    unsafe { *Box::from_raw(out) }
}
fn attach16(
    d: &PrecisionDocument,
    layer: usize,
    original: Option<(&PrecisionDocument, usize)>,
    settings: Value,
) -> PrecisionDocument {
    let descriptor = CString::new(settings.to_string()).unwrap();
    let mut error = ptr::null_mut();
    let out = unsafe {
        if let Some((original, original_layer)) = original {
            doctored_core::ffi_raw_source::dr_precision_doc_attach_raw_source_from(
                d,
                layer,
                original,
                original_layer,
                descriptor.as_ptr(),
                REFERENCE_WORKING_BYTES,
                0,
                ptr::null(),
                &mut error,
            )
        } else {
            doctored_core::ffi_raw_source::dr_precision_doc_attach_raw_source(
                d,
                layer,
                ORIGINAL.as_ptr(),
                ORIGINAL.len(),
                descriptor.as_ptr(),
                REFERENCE_WORKING_BYTES,
                0,
                ptr::null(),
                &mut error,
            )
        }
    };
    assert!(!out.is_null(), "{}", ffi_error(error));
    assert!(error.is_null());
    unsafe { *Box::from_raw(out) }
}
fn save8(d: &DrDocument, dir: &TempDir) -> Vec<u8> {
    let path = dir.path().join("byte.rz");
    let cpath = CString::new(path.to_str().unwrap()).unwrap();
    let mut error = ptr::null_mut();
    assert!(
        unsafe {
            doctored_core::ffi_doc::dr_doc_save_native(
                d,
                cpath.as_ptr(),
                std::ptr::null(),
                &mut error,
            )
        },
        "{}",
        ffi_error(error)
    );
    assert!(error.is_null());
    std::fs::read(path).unwrap()
}
fn save16(d: &PrecisionDocument, dir: &TempDir) -> Vec<u8> {
    let path = dir.path().join("typed.rz");
    let cpath = CString::new(path.to_str().unwrap()).unwrap();
    let mut error = ptr::null_mut();
    assert!(
        unsafe {
            doctored_core::ffi_precision::dr_precision_doc_save_native(
                d,
                cpath.as_ptr(),
                REFERENCE_WORKING_BYTES,
                0,
                ptr::null(),
                &mut error,
            )
        },
        "{}",
        ffi_error(error)
    );
    assert!(error.is_null());
    std::fs::read(path).unwrap()
}
fn open8(bytes: &[u8], dir: &TempDir) -> Result<DrDocument, String> {
    let path = dir.path().join("moved-byte.rz");
    std::fs::write(&path, bytes).unwrap();
    DrDocument::open(path.to_str().unwrap())
}
fn open16(bytes: &[u8]) -> Result<PrecisionDocument, String> {
    PrecisionDocument::parse_native(bytes, REFERENCE_WORKING_BYTES, 0, None)
}
fn u32_at(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap())
}
fn u64_at(bytes: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(bytes[at..at + 8].try_into().unwrap())
}
fn slot<'a>(bytes: &'a [u8], at: &mut usize) -> &'a [u8] {
    let len = u32_at(bytes, *at) as usize;
    *at += 4;
    let value = &bytes[*at..*at + len];
    *at += len;
    value
}
struct Asset {
    depth: usize,
    profile: Vec<u8>,
    png: std::ops::Range<usize>,
    flag: usize,
    raw_ref: Option<usize>,
    descriptor: Option<usize>,
}
struct Frame {
    assets: Vec<Asset>,
    raw_table: usize,
    payloads: Vec<std::ops::Range<usize>>,
}
fn frame(bytes: &[u8]) -> Frame {
    // Offset 8: 12E-1's identity wrapper owns the outermost word.
    assert!(matches!(u32_at(bytes, 8), 18 | 19));
    let start = bytes.windows(4).position(|v| v == b"RSRC").unwrap();
    let count = u32_at(bytes, start + 4) as usize;
    let mut at = start + 8;
    let mut assets = Vec::new();
    for _ in 0..count {
        slot(bytes, &mut at); // stable identity
        slot(bytes, &mut at); // developed revision
        let depth = at;
        at += 4;
        let profile = slot(bytes, &mut at).to_vec();
        at += 8; // dimensions
        let len = u64_at(bytes, at) as usize;
        at += 8;
        let png = at..at + len;
        at += len;
        let flag = at;
        at += 1;
        let (raw_ref, descriptor) = if bytes[flag] == 1 {
            let raw_ref = at;
            at += 4;
            let descriptor = at;
            slot(bytes, &mut at);
            (Some(raw_ref), Some(descriptor))
        } else {
            (None, None)
        };
        assets.push(Asset {
            depth,
            profile,
            png,
            flag,
            raw_ref,
            descriptor,
        });
    }
    let count = u32_at(bytes, at);
    at += 4;
    for _ in 0..count {
        at += 60;
        let linked = bytes[at];
        at += 1;
        if linked == 1 {
            slot(bytes, &mut at);
            at += 64;
        }
    }
    let raw_table = at;
    assert_eq!(&bytes[at..at + 4], b"RRAW");
    let count = u32_at(bytes, at + 4);
    at += 8;
    let mut payloads = Vec::new();
    for _ in 0..count {
        let len = u64_at(bytes, at) as usize;
        at += 8;
        payloads.push(at..at + len);
        at += len;
    }
    // The RAW frame is no longer the file's last block: the always-written
    // `LIDS` table closes every file since 12E-1, and this walk checks it is
    // exactly what follows.
    assert_eq!(&bytes[at..at + 4], b"LIDS");
    assert_eq!(u32_at(bytes, at + 4), 1, "identity schema");
    let records = u32_at(bytes, at + 8) as usize;
    assert_eq!(bytes.len(), at + 12 + 8 * records);
    Frame {
        assets,
        raw_table,
        payloads,
    }
}
fn assert_cache(bytes: &[u8], f: &Frame, asset: usize, input: &PrecisionDocument, settings: Value) {
    let a = &f.assets[asset];
    assert_eq!(bytes[a.depth], 16);
    assert_eq!(a.profile, *input.profile().bytes().as_ref());
    let decoded = image::load_from_memory(&bytes[a.png.clone()]).unwrap();
    assert_eq!(decoded.color(), image::ColorType::Rgba16);
    let expected: Vec<u16> = input
        .layer_source(0)
        .unwrap()
        .to_rgba16()
        .ok()
        .unwrap()
        .iter()
        .flatten()
        .copied()
        .collect();
    assert_eq!(*decoded.to_rgba16().as_raw(), expected);
    let mut at = a.descriptor.unwrap();
    assert_eq!(
        serde_json::from_slice::<Value>(slot(bytes, &mut at)).unwrap(),
        settings
    );
    let index = u32_at(bytes, a.raw_ref.unwrap()) as usize;
    assert_eq!(&bytes[f.payloads[index].clone()], ORIGINAL);
}

#[test]
fn byte_native_preserves_original_raw_genuine_master_and_cached_fallback_without_decoder() {
    let dir = TempDir::new().unwrap();
    let input = typed(30001);
    let plain = edit8(
        &byte(),
        Some(SourceInput::Typed(&input)),
        place("development-a"),
    );
    let raw = attach8(&plain, 1, None, descriptor(5500.0));
    let bytes = save8(&raw, &dir);
    assert_eq!(u32_at(&bytes, 8), 18);
    let f = frame(&bytes);
    assert_eq!(f.assets.len(), 1);
    assert_cache(&bytes, &f, 0, &input, descriptor(5500.0));
    let opened = open8(&bytes, &dir).unwrap();
    let source = opened.layers[1].effective_source(&opened.profile).unwrap();
    assert_eq!(
        source.asset.raw.as_ref().unwrap().encoded.as_slice(),
        ORIGINAL
    );
    assert_eq!(
        source.asset.raw.as_ref().unwrap().descriptor.json(),
        descriptor(5500.0)
    );
    assert_eq!(
        source.asset.rgba16().unwrap().to_rgba16().ok(),
        input.layer_source(0).unwrap().to_rgba16().ok()
    );
    assert_eq!(
        *opened.layers[0].pixels.contiguous_image(),
        *raw.layers[0].pixels.contiguous_image()
    );
    assert_eq!(
        *opened.layers[1].pixels.contiguous_image(),
        *raw.layers[1].pixels.contiguous_image()
    );
    assert!(open16(&bytes).is_err());
}

#[test]
fn typed_native_preserves_unknown_decoder_and_subbyte_samples_through_portable_reopen() {
    let dir = TempDir::new().unwrap();
    let input = typed(30001);
    let plain = edit16(
        &typed(11),
        Some(SourceInput::Typed(&input)),
        place("development-a"),
    );
    let raw = attach16(&plain, 1, None, descriptor(5500.0));
    let bytes = save16(&raw, &dir);
    assert_eq!(u32_at(&bytes, 8), 19);
    let f = frame(&bytes);
    assert_cache(&bytes, &f, 0, &input, descriptor(5500.0));
    let opened = open16(&bytes).unwrap();
    let source = opened.layer(1).unwrap().effective_source().unwrap();
    assert_eq!(
        source.asset.raw.as_ref().unwrap().encoded.as_slice(),
        ORIGINAL
    );
    assert_eq!(
        source.asset.raw.as_ref().unwrap().descriptor.json(),
        descriptor(5500.0)
    );
    assert_eq!(
        opened.layer_source(1).unwrap().to_rgba16().ok(),
        raw.layer_source(1).unwrap().to_rgba16().ok()
    );
    assert!(open8(&bytes, &dir).is_err());
}

fn replacement() -> SourceEdit {
    SourceEdit::parse(&json!({"op":"replace","layer":1,"revision_id":"development-b","depth_conversion":"convert_fallback"}).to_string()).unwrap()
}
#[test]
fn byte_development_revisions_share_one_original_payload_and_keep_each_cached_master() {
    let dir = TempDir::new().unwrap();
    let a = typed(30001);
    let b = typed(41001);
    let plain = edit8(
        &byte(),
        Some(SourceInput::Typed(&a)),
        place("development-a"),
    );
    let original = attach8(&plain, 1, None, descriptor(5500.0));
    let duplicate = original.duplicate_layers(&[1]).unwrap();
    let replaced = edit8(&duplicate, Some(SourceInput::Typed(&b)), replacement());
    let changed = attach8(&replaced, 1, Some((&original, 1)), descriptor(7100.0));
    let bytes = save8(&changed, &dir);
    let f = frame(&bytes);
    assert_eq!(f.assets.len(), 2);
    assert_eq!(f.payloads.len(), 1);
    assert_cache(&bytes, &f, 0, &b, descriptor(7100.0));
    assert_cache(&bytes, &f, 1, &a, descriptor(5500.0));
    let opened = open8(&bytes, &dir).unwrap();
    let a = opened.layers[1]
        .effective_source(&opened.profile)
        .unwrap()
        .asset;
    let b = opened.layers[2]
        .effective_source(&opened.profile)
        .unwrap()
        .asset;
    assert!(!Arc::ptr_eq(&a, &b));
    assert!(Arc::ptr_eq(
        &a.raw.as_ref().unwrap().encoded,
        &b.raw.as_ref().unwrap().encoded
    ));
    assert_eq!(a.id, b.id);
    assert_eq!(a.revision_id, "development-b");
    assert_eq!(b.revision_id, "development-a");
}

#[test]
fn typed_development_revisions_deduplicate_original_bytes_independently_of_masters() {
    let dir = TempDir::new().unwrap();
    let a = typed(30001);
    let b = typed(41001);
    let plain = edit16(
        &typed(11),
        Some(SourceInput::Typed(&a)),
        place("development-a"),
    );
    let original = attach16(&plain, 1, None, descriptor(5500.0));
    let duplicate = original.duplicate_layer(1).unwrap().unwrap();
    let replaced = edit16(&duplicate, Some(SourceInput::Typed(&b)), replacement());
    let changed = attach16(&replaced, 1, Some((&original, 1)), descriptor(7100.0));
    let bytes = save16(&changed, &dir);
    let f = frame(&bytes);
    assert_eq!(f.assets.len(), 2);
    assert_eq!(f.payloads.len(), 1);
    assert_cache(&bytes, &f, 0, &b, descriptor(7100.0));
    assert_cache(&bytes, &f, 1, &a, descriptor(5500.0));
    let opened = open16(&bytes).unwrap();
    let a = opened.layer(1).unwrap().effective_source().unwrap().asset;
    let b = opened.layer(2).unwrap().effective_source().unwrap().asset;
    assert!(!Arc::ptr_eq(&a, &b));
    assert!(Arc::ptr_eq(
        &a.raw.as_ref().unwrap().encoded,
        &b.raw.as_ref().unwrap().encoded
    ));
    assert_eq!(
        opened.layer_source(0).unwrap().to_rgba16().ok(),
        changed.layer_source(0).unwrap().to_rgba16().ok()
    );
}

fn corruptions(bytes: &[u8]) -> Vec<Vec<u8>> {
    let f = frame(bytes);
    let a = &f.assets[0];
    let descriptor = a.descriptor.unwrap();
    let mut out = Vec::new();
    for (at, value) in [
        (a.flag, vec![2]),
        (a.depth, vec![8]),
        (a.raw_ref.unwrap(), 999_u32.to_le_bytes().to_vec()),
        (descriptor, 65537_u32.to_le_bytes().to_vec()),
        (descriptor + 4, vec![0xff]),
        (descriptor + 4, vec![b'[']),
        (f.raw_table, b"XXXX".to_vec()),
        (f.raw_table + 4, 0_u32.to_le_bytes().to_vec()),
        (f.raw_table + 4, 1025_u32.to_le_bytes().to_vec()),
        (f.raw_table + 8, 0_u64.to_le_bytes().to_vec()),
        (
            f.raw_table + 8,
            (256_u64 * 1024 * 1024 + 1).to_le_bytes().to_vec(),
        ),
    ] {
        let mut bad = bytes.to_vec();
        bad[at..at + value.len()].copy_from_slice(&value);
        out.push(bad);
    }
    for at in [
        a.flag,
        a.raw_ref.unwrap() + 1,
        descriptor + 1,
        f.raw_table + 1,
        f.raw_table + 9,
        bytes.len() - 1,
    ] {
        out.push(bytes[..at].to_vec());
    }
    let mut bad = bytes.to_vec();
    bad.push(0);
    out.push(bad);
    out
}
#[test]
fn both_native_depths_refuse_malformed_raw_flags_descriptors_references_and_lengths() {
    let dir = TempDir::new().unwrap();
    let input = typed(30001);
    let plain8 = edit8(
        &byte(),
        Some(SourceInput::Typed(&input)),
        place("development-a"),
    );
    let plain16 = edit16(
        &typed(11),
        Some(SourceInput::Typed(&input)),
        place("development-a"),
    );
    let bytes8 = save8(&attach8(&plain8, 1, None, descriptor(5500.0)), &dir);
    let bytes16 = save16(&attach16(&plain16, 1, None, descriptor(5500.0)), &dir);
    for (i, bad) in corruptions(&bytes8).iter().enumerate() {
        assert!(open8(bad, &dir).is_err(), "byte corruption {i} accepted");
    }
    for (i, bad) in corruptions(&bytes16).iter().enumerate() {
        assert!(open16(bad).is_err(), "typed corruption {i} accepted");
    }
}

#[test]
fn later_invalid_raw_metadata_refuses_before_earlier_pixel_payload_decode() {
    let dir = TempDir::new().unwrap();
    let input = typed(30001);
    let plain8 = edit8(
        &byte(),
        Some(SourceInput::Typed(&input)),
        place("development-a"),
    );
    let plain16 = edit16(
        &typed(11),
        Some(SourceInput::Typed(&input)),
        place("development-a"),
    );
    for (mut bytes, typed) in [
        (
            save8(&attach8(&plain8, 1, None, descriptor(5500.0)), &dir),
            false,
        ),
        (
            save16(&attach16(&plain16, 1, None, descriptor(5500.0)), &dir),
            true,
        ),
    ] {
        let f = frame(&bytes);
        let at = f.assets[0].descriptor.unwrap() + 4;
        bytes[at] = b'[';
        let idat = bytes.windows(4).position(|v| v == b"IDAT").unwrap();
        bytes[idat + 4] ^= 0xff;
        let error = if typed {
            open16(&bytes).err().unwrap()
        } else {
            open8(&bytes, &dir).err().unwrap()
        };
        assert!(error.contains("RAW descriptor"), "{error}");
    }
}

#[test]
fn typed_native_plan_counts_original_payload_and_expanded_recipe_before_copy_or_decode() {
    let dir = TempDir::new().unwrap();
    let input = typed(30001);
    let plain = edit16(
        &typed(11),
        Some(SourceInput::Typed(&input)),
        place("development-a"),
    );
    let mut settings = descriptor(5500.0);
    settings["recipe"]["captured_controls"] = json!((0..400)
        .map(|i| (format!("control{i}"), json!(i as f64 / 7.0)))
        .collect::<serde_json::Map<_, _>>());
    let raw = attach16(&plain, 1, None, settings);
    let plan = raw.native_plan().unwrap();
    let bytes = save16(&raw, &dir);
    assert!(bytes.len() as u64 <= plan.encoded_bound);
    let opened = open16(&bytes).unwrap();
    assert!(opened.retained_bytes() <= plan.reopen_retained_bytes);
    assert!(
        plan.reopen_retained_bytes
            > plain.native_plan().unwrap().reopen_retained_bytes + ORIGINAL.len() as u64
    );
    let limit = raw.retained_bytes() + plan.write_scratch_bytes;
    assert!(raw.encode_native(limit - 1, 0, None).is_err());
    assert!(raw.encode_native(limit, 0, None).is_ok());
    assert!(
        PrecisionDocument::parse_native(&bytes, bytes.len() as u64 + 1024 * 1024, 0, None).is_err()
    );
}

#[test]
fn raw_and_ordinary_sources_coexist_and_unused_raw_payloads_are_refused() {
    let dir = TempDir::new().unwrap();
    let input = typed(30001);
    let plain = edit8(
        &byte(),
        Some(SourceInput::Typed(&input)),
        place("development-a"),
    );
    let raw = attach8(&plain, 1, None, descriptor(5500.0));
    let ordinary = byte();
    let mixed = edit8(
        &raw,
        Some(SourceInput::Byte(&ordinary)),
        SourceEdit::parse(
            r#"{"op":"place","source_id":"ordinary-source","name":"Ordinary","x":0,"y":0}"#,
        )
        .unwrap(),
    );
    let bytes = save8(&mixed, &dir);
    let f = frame(&bytes);
    assert_eq!(f.assets.len(), 2);
    assert_eq!(f.payloads.len(), 1);
    assert!(f.assets[1].raw_ref.is_none());
    let opened = open8(&bytes, &dir).unwrap();
    assert!(opened.layers[1]
        .effective_source(&opened.profile)
        .unwrap()
        .asset
        .raw
        .is_some());
    let ordinary_source = opened.layers[2]
        .effective_source(&opened.profile)
        .unwrap()
        .asset;
    assert!(ordinary_source.raw.is_none());
    assert!(ordinary_source
        .rgba8()
        .unwrap()
        .same_bytes(&ordinary.layers[0].pixels));
    // The extra payload is spliced in AHEAD of the identity tail, which has
    // to stay last: `LIDS` closes every file since 12E-1.
    let split = f.payloads.last().map(|p| p.end).expect("a RAW payload");
    let mut bad = bytes[..split].to_vec();
    bad[f.raw_table + 4..f.raw_table + 8].copy_from_slice(&2_u32.to_le_bytes());
    bad.extend_from_slice(&(ORIGINAL.len() as u64).to_le_bytes());
    bad.extend_from_slice(ORIGINAL);
    bad.extend_from_slice(&bytes[split..]);
    assert!(open8(&bad, &dir)
        .err()
        .unwrap()
        .contains("unreferenced retained RAW"));
}

#[test]
fn raw_detachment_restores_compatible_ordinary_source_writers() {
    let dir = TempDir::new().unwrap();
    let input = typed(30001);
    let plain8 = edit8(
        &byte(),
        Some(SourceInput::Typed(&input)),
        place("development-a"),
    );
    let plain16 = edit16(
        &typed(11),
        Some(SourceInput::Typed(&input)),
        place("development-a"),
    );
    assert_eq!(u32_at(&save8(&plain8, &dir), 8), 16);
    assert_eq!(u32_at(&save16(&plain16, &dir), 8), 17);
    let raw8 = attach8(&plain8, 1, None, descriptor(5500.0));
    let raw16 = attach16(&plain16, 1, None, descriptor(5500.0));
    let rasterize = || SourceEdit::parse(r#"{"op":"rasterize","layer":1}"#).unwrap();
    assert_eq!(
        u32_at(&save8(&edit8(&raw8, None, rasterize()), &dir), 8),
        10
    );
    assert_eq!(
        u32_at(&save16(&edit16(&raw16, None, rasterize()), &dir), 8),
        13
    );
}
