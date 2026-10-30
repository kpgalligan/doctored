//! Native artwork/source composition and lifecycle through public operations.
//! ADSC/source framing is inspected independently. Deliberately broken PNG data
//! acts as a canary: malformed nonpixel tables must refuse before pixel decode.
use crate::group_roots;
use doctored_core::doc::DrDocument;
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_artwork::*;
use doctored_core::ffi_doc::{dr_doc_layer_meta, dr_doc_save_native};
use doctored_core::ffi_precision::dr_precision_doc_save_native;
use doctored_core::ffi_precision_edit::dr_precision_doc_edit;
use doctored_core::ffi_raw_source::{dr_doc_attach_raw_source, dr_precision_doc_attach_raw_source};
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use doctored_core::retained_source::{SourceEdit, SourceInput};
use group_roots::GroupRoots;
use image::{Rgba, RgbaImage};
use serde_json::{json, Value};
use std::ffi::{c_char, CStr, CString};
use std::ops::Range;
use std::ptr;
use std::sync::Arc;
use tempfile::TempDir;

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
const CAMERA: &[u8] = b"8E original camera bytes\0not a decodable camera or preview\xff";
const LINK: &str = "/nonexistent/rasterize-phase8e/原本.png";
const HASH: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const DESCRIPTION: &str = r#"{"type":"shape","version":1,"probe":"native artwork","continuous_color":[0.123456789,0.321987654,0.73456789]}"#;

unsafe fn text(pointer: *mut c_char) -> String {
    if pointer.is_null() {
        return String::new();
    }
    let value = unsafe { CStr::from_ptr(pointer) }
        .to_str()
        .unwrap()
        .to_owned();
    unsafe { dr_string_free(pointer) };
    value
}
fn typed(profile: Arc<IccProfile>, seed: u16) -> PrecisionDocument {
    let image = PrecisionImage::from_rgba16(
        4,
        2,
        (0..8)
            .map(|i| [seed + i, 12003 + i, 49007 - i, 65535])
            .collect(),
        profile,
        None,
    )
    .unwrap();
    PrecisionDocument::from_image(
        image,
        Metadata::default(),
        Resolution { x: 240.0, y: 300.0 },
    )
    .unwrap()
}
fn byte() -> DrDocument {
    DrDocument::from_pixels(RgbaImage::from_fn(4, 2, |x, y| {
        Rgba([11 + x as u8, 53 + y as u8, 173, 255])
    }))
}
fn ink(profile: Arc<IccProfile>) -> PrecisionImage {
    PrecisionImage::from_rgba16(
        2,
        2,
        vec![
            [10001, 20002, 30003, 0],
            [10003, 20005, 30007, 1],
            [10111, 20222, 30333, 32769],
            [12345, 23456, 34567, 65535],
        ],
        profile,
        None,
    )
    .unwrap()
}
fn description(doc: &PrecisionDocument, layer: usize) -> Option<String> {
    let pointer = unsafe { dr_precision_doc_artwork_description(doc, layer) };
    if pointer.is_null() {
        None
    } else {
        Some(unsafe { text(pointer) })
    }
}
fn describe(
    doc: &PrecisionDocument,
    layer: usize,
    value: Option<&str>,
) -> Result<Option<PrecisionDocument>, String> {
    let value = value.map(|value| CString::new(value).unwrap());
    let mut error = ptr::null_mut();
    let result = unsafe {
        dr_precision_doc_artwork_metadata(
            doc,
            layer,
            value.as_ref().map_or(ptr::null(), |value| value.as_ptr()),
            LIMIT,
            0,
            &mut error,
        )
    };
    let message = unsafe { text(error) };
    if !message.is_empty() {
        return Err(message);
    }
    Ok(if result.is_null() {
        None
    } else {
        Some(unsafe { *Box::from_raw(result) })
    })
}
fn append_art16(doc: &PrecisionDocument, value: &str) -> PrecisionDocument {
    let image = ink(doc.profile().clone());
    let name = CString::new("Continuous ink").unwrap();
    let value = CString::new(value).unwrap();
    let mut error = ptr::null_mut();
    let result = unsafe {
        dr_precision_doc_artwork(
            doc,
            0,
            false,
            &image,
            1,
            -1,
            name.as_ptr(),
            value.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut error,
        )
    };
    assert!(error.is_null(), "{}", unsafe { text(error) });
    assert!(!result.is_null());
    unsafe { *Box::from_raw(result) }
}
fn append_art8(doc: &DrDocument, layer: usize, value: &str) -> DrDocument {
    let image = ink(doc.profile.clone());
    let name = CString::new("Continuous ink").unwrap();
    let value = CString::new(value).unwrap();
    let mut error = ptr::null_mut();
    let result = unsafe {
        dr_doc_artwork(
            doc,
            layer,
            false,
            &image,
            1,
            -1,
            name.as_ptr(),
            value.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut error,
        )
    };
    assert!(error.is_null(), "{}", unsafe { text(error) });
    assert!(!result.is_null());
    unsafe { *Box::from_raw(result) }
}
fn edit(doc: &PrecisionDocument, request: Value, samples: &[u16]) -> PrecisionDocument {
    let request = CString::new(request.to_string()).unwrap();
    let mut error = ptr::null_mut();
    let result = unsafe {
        dr_precision_doc_edit(
            doc,
            request.as_ptr(),
            samples.as_ptr(),
            samples.len(),
            LIMIT,
            0,
            ptr::null(),
            &mut error,
        )
    };
    assert!(error.is_null(), "{}", unsafe { text(error) });
    assert!(!result.is_null());
    unsafe { *Box::from_raw(result) }
}
fn place(id: &str, linked: bool) -> SourceEdit {
    let mut request = json!({"op":"place","source_id":id,"revision_id":"revision-a","name":id,"x":0,"y":0,"depth_conversion":"convert_fallback"});
    if linked {
        request["link"] = json!({"path":LINK,"sha256":HASH});
    }
    SourceEdit::parse(&request.to_string()).unwrap()
}
fn raw_descriptor() -> CString {
    CString::new(json!({"schema_version":1,"original_filename":"camera.NEF","content_sha256":HASH,
        "decoder":{"identifier":"unavailable.test.decoder","version":"future","implementation":"preserve saved bytes"},
        "recipe":{"version":99,"tint":11.999950408935547_f64,"unknown_effective":{"value":0.125}}}).to_string()).unwrap()
}
fn attach16(doc: &PrecisionDocument) -> PrecisionDocument {
    let descriptor = raw_descriptor();
    let mut error = ptr::null_mut();
    let result = unsafe {
        dr_precision_doc_attach_raw_source(
            doc,
            1,
            CAMERA.as_ptr(),
            CAMERA.len(),
            descriptor.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut error,
        )
    };
    assert!(error.is_null(), "{}", unsafe { text(error) });
    assert!(!result.is_null());
    unsafe { *Box::from_raw(result) }
}
fn attach8(doc: &DrDocument) -> DrDocument {
    let descriptor = raw_descriptor();
    let mut error = ptr::null_mut();
    let result = unsafe {
        dr_doc_attach_raw_source(
            doc,
            1,
            CAMERA.as_ptr(),
            CAMERA.len(),
            descriptor.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut error,
        )
    };
    assert!(error.is_null(), "{}", unsafe { text(error) });
    assert!(!result.is_null());
    unsafe { *Box::from_raw(result) }
}
fn mixed16() -> PrecisionDocument {
    let base = typed(IccProfile::display_p3(), 17);
    let camera = typed(IccProfile::display_p3(), 30001);
    let doc = base
        .source_edit_input(
            Some(SourceInput::Typed(&camera)),
            &place("camera", false),
            LIMIT,
            0,
            None,
        )
        .unwrap()
        .unwrap();
    let doc = attach16(&doc);
    let linked = byte();
    let doc = doc
        .source_edit_input(
            Some(SourceInput::Byte(&linked)),
            &place("linked", true),
            LIMIT,
            0,
            None,
        )
        .unwrap()
        .unwrap();
    let doc = append_art16(&doc, DESCRIPTION);
    let doc = edit(
        &doc,
        json!({"op":"set_mask","layer":3,"width":2,"height":2,"x":-1,"y":1,"linked":false,"density":0.875,"feather":0.75}),
        &[1, 257, 513, 65535],
    );
    let doc = edit(
        &doc,
        json!({"op":"set_channel","index":null,"name":"Native coverage"}),
        &[0, 1, 2, 255, 256, 257, 32769, 65535],
    );
    let grouped = doc.group_all(0.75).unwrap().unwrap();
    grouped.duplicate_layer(4).unwrap().unwrap()
}
fn save16(doc: &PrecisionDocument, directory: &TempDir) -> Vec<u8> {
    let path = directory.path().join("typed.rz");
    let path = CString::new(path.to_str().unwrap()).unwrap();
    let mut error = ptr::null_mut();
    assert!(
        unsafe {
            dr_precision_doc_save_native(doc, path.as_ptr(), LIMIT, 0, ptr::null(), &mut error)
        },
        "{}",
        unsafe { text(error) }
    );
    assert!(error.is_null());
    std::fs::read(path.to_str().unwrap()).unwrap()
}
fn open16(bytes: &[u8]) -> Result<PrecisionDocument, String> {
    PrecisionDocument::parse_native(bytes, LIMIT, 0, None)
}
fn word(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap())
}
fn put_word(bytes: &mut [u8], at: usize, value: u32) {
    bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
}
fn unique_tag(bytes: &[u8], tag: &[u8]) -> usize {
    let positions: Vec<_> = bytes
        .windows(tag.len())
        .enumerate()
        .filter_map(|(i, value)| (value == tag).then_some(i))
        .collect();
    assert_eq!(
        positions.len(),
        1,
        "ambiguous independently inspected fixture tag"
    );
    positions[0]
}
struct DescriptionTable {
    start: usize,
    references: Vec<usize>,
    texts: Vec<Range<usize>>,
}
fn table(bytes: &[u8]) -> DescriptionTable {
    // Offset 8: 12E-1's identity wrapper owns the outermost word at both
    // depths, and its `LIDS` table closes the file.
    assert_eq!(word(bytes, 4), 46);
    assert_eq!(word(bytes, 8), 20);
    let start = unique_tag(bytes, b"ADSC");
    let count = word(bytes, start + 4) as usize;
    let mut at = start + 8;
    let mut references = Vec::new();
    let mut texts = Vec::new();
    for _ in 0..count {
        references.push(at);
        let size = word(bytes, at + 4) as usize;
        at += 8;
        texts.push(at..at + size);
        at += size;
    }
    // The `ADSC` table is no longer last: the always-written `LIDS` table
    // closes every file since 12E-1, and this walk checks it is exactly what
    // follows.
    assert_eq!(&bytes[at..at + 4], b"LIDS");
    assert_eq!(word(bytes, at + 4), 1, "identity schema");
    let records = word(bytes, at + 8) as usize;
    assert_eq!(bytes.len(), at + 12 + 8 * records);
    DescriptionTable {
        start,
        references,
        texts,
    }
}
fn poison_first_png(bytes: &mut [u8]) {
    let signature = b"\x89PNG\r\n\x1a\n";
    let mut at = bytes
        .windows(signature.len())
        .position(|part| part == signature)
        .unwrap()
        + signature.len();
    loop {
        let size = u32::from_be_bytes(bytes[at..at + 4].try_into().unwrap()) as usize;
        if &bytes[at + 4..at + 8] == b"IDAT" {
            assert!(size > 2);
            bytes[at + 8] ^= 0xff;
            return;
        }
        at += 12 + size;
    }
}
fn refusal(bytes: &[u8], expected: &str) {
    let error = match open16(bytes) {
        Ok(_) => panic!("malformed native unexpectedly opened"),
        Err(error) => error,
    };
    assert!(
        error.to_lowercase().contains(&expected.to_lowercase()),
        "expected {expected}, received {error}"
    );
}

#[test]
fn v20_nested_artwork_raw_and_linked_sources_roundtrip_every_code_mask_and_description() {
    let original = mixed16();
    let directory = TempDir::new().unwrap();
    let bytes = save16(&original, &directory);
    assert_eq!(word(&bytes, 4), 46, "the identity wrapper is outermost");
    assert_eq!(
        (word(&bytes, 8), word(&bytes, 28), word(&bytes, 32)),
        (20, 2, 10)
    );
    let sources = unique_tag(&bytes, b"RSRC");
    assert_eq!(
        word(&bytes, sources - 4),
        19,
        "RAW and linked resources use their existing source schema"
    );
    assert_eq!(
        word(&bytes, sources + 4),
        2,
        "duplicated groups share source assets"
    );
    let raw = unique_tag(&bytes, b"RRAW");
    assert_eq!(word(&bytes, raw + 4), 1);
    assert_eq!(
        bytes
            .windows(CAMERA.len())
            .filter(|value| *value == CAMERA)
            .count(),
        1
    );
    let descriptions = table(&bytes);
    assert_eq!(
        descriptions
            .references
            .iter()
            .map(|at| word(&bytes, *at))
            .collect::<Vec<_>>(),
        [3, 8]
    );
    for range in &descriptions.texts {
        assert_eq!(&bytes[range.clone()], DESCRIPTION.as_bytes());
    }
    let reopened = open16(&bytes).unwrap();
    let old_info = original.descriptor().unwrap();
    let new_info = reopened.descriptor().unwrap();
    for key in [
        "width",
        "height",
        "storage",
        "evaluation",
        "resolution",
        "layer_count",
        "channel_count",
        "profile_bytes",
        "metadata_bytes",
    ] {
        assert_eq!(new_info[key], old_info[key], "native semantic field {key}");
    }
    assert_eq!(reopened.channels()[0].name(), "Native coverage");
    assert_eq!(
        reopened.channels()[0].plane().samples(),
        &[0, 1, 2, 255, 256, 257, 32769, 65535]
    );
    assert_eq!(reopened.profile().bytes(), original.profile().bytes());
    for index in 0..original.layer_count() {
        assert_eq!(description(&reopened, index), description(&original, index));
        let left = original.layer(index).unwrap();
        let right = reopened.layer(index).unwrap();
        if let Some(source) = left.source() {
            assert_eq!(
                right.source().unwrap().to_rgba16().ok(),
                source.to_rgba16().ok()
            );
            assert_eq!(right.offset(), left.offset());
        }
        if let Some(mask) = left.mask() {
            let restored = right.mask().unwrap();
            assert_eq!(restored.plane().samples(), mask.plane().samples());
            assert_eq!(restored.plane().dimensions(), mask.plane().dimensions());
            assert_eq!(restored.properties().offset, mask.properties().offset);
            assert_eq!(restored.properties().density, mask.properties().density);
            assert_eq!(restored.properties().feather, mask.properties().feather);
            assert_eq!(restored.properties().linked, mask.properties().linked);
        }
    }
    for index in [1, 6] {
        let source = reopened.layer(index).unwrap().effective_source().unwrap();
        let raw = source.asset.raw.as_ref().unwrap();
        assert_eq!(raw.encoded.as_slice(), CAMERA);
        assert_eq!(
            raw.descriptor.recipe["tint"].as_f64().unwrap().to_bits(),
            11.999950408935547_f64.to_bits()
        );
        assert_eq!(raw.descriptor.recipe["version"], 99);
        assert_eq!(
            source.asset.rgba16().unwrap().to_rgba16().ok(),
            typed(IccProfile::display_p3(), 30001)
                .layer_source(0)
                .unwrap()
                .to_rgba16()
                .ok()
        );
    }
    for index in [2, 7] {
        let source = reopened.layer(index).unwrap().effective_source().unwrap();
        assert_eq!(source.link.as_ref().unwrap().path, LINK);
        assert_eq!(source.asset.depth(), 8);
    }
    assert!(
        reopened.encode_native(LIMIT, 0, None).unwrap().len()
            <= reopened.native_plan().unwrap().encoded_bound as usize
    );
}

#[test]
fn byte_artwork_keeps_legacy_description_with_raw_and_missing_linked_resources() {
    let directory = TempDir::new().unwrap();
    let camera = typed(IccProfile::display_p3(), 30001);
    let doc = byte()
        .source_edit_input(
            Some(SourceInput::Typed(&camera)),
            &place("camera", false),
            LIMIT,
            0,
            None,
        )
        .unwrap()
        .unwrap();
    let doc = attach8(&doc);
    let linked = byte();
    let doc = doc
        .source_edit_input(
            Some(SourceInput::Byte(&linked)),
            &place("linked", true),
            LIMIT,
            0,
            None,
        )
        .unwrap()
        .unwrap();
    let doc = append_art8(&doc, 2, DESCRIPTION);
    let path = directory.path().join("byte.rz");
    let cpath = CString::new(path.to_str().unwrap()).unwrap();
    let mut error = ptr::null_mut();
    assert!(
        unsafe { dr_doc_save_native(&doc, cpath.as_ptr(), &mut error) },
        "{}",
        unsafe { text(error) }
    );
    assert!(error.is_null());
    let bytes = std::fs::read(&path).unwrap();
    assert_eq!(word(&bytes, 4), 45, "the identity wrapper is outermost");
    assert_eq!(
        word(&bytes, 8),
        18,
        "byte format already carries descriptions"
    );
    let moved = directory.path().join("moved-byte.rz");
    std::fs::rename(&path, &moved).unwrap();
    let reopened = DrDocument::open(moved.to_str().unwrap()).unwrap();
    assert_eq!(
        unsafe { text(dr_doc_layer_meta(&reopened, 3)) },
        DESCRIPTION
    );
    for index in 0..4 {
        assert_eq!(
            *reopened.layers[index].pixels.contiguous_image(),
            *doc.layers[index].pixels.contiguous_image()
        );
    }
    let raw = reopened.layers[1]
        .effective_source(&reopened.profile)
        .unwrap();
    assert_eq!(raw.asset.raw.as_ref().unwrap().encoded.as_slice(), CAMERA);
    assert_eq!(
        raw.asset.rgba16().unwrap().to_rgba16().ok(),
        camera.layer_source(0).unwrap().to_rgba16().ok()
    );
    assert_eq!(
        reopened.layers[2]
            .effective_source(&reopened.profile)
            .unwrap()
            .link
            .as_ref()
            .unwrap()
            .path,
        LINK
    );
    assert!(open16(&bytes).is_err());
}

#[test]
fn malformed_v20_tables_refuse_before_png_pixel_decoding() {
    let directory = TempDir::new().unwrap();
    let mut base = save16(&mixed16(), &directory);
    let locations = table(&base);
    let source_schema = unique_tag(&base, b"RSRC") - 4;
    poison_first_png(&mut base);
    refusal(&base, "png");
    let mut bad = base.clone();
    put_word(&mut bad, source_schema, 18);
    refusal(&bad, "embedded-source schema");
    let mut bad = base.clone();
    bad[locations.start..locations.start + 4].copy_from_slice(b"BAD!");
    refusal(&bad, "artwork description table");
    let mut bad = base.clone();
    put_word(&mut bad, locations.start + 4, 11);
    refusal(&bad, "description count");
    let mut bad = base.clone();
    put_word(&mut bad, locations.references[0], 10);
    refusal(&bad, "reference");
    let mut bad = base.clone();
    put_word(&mut bad, locations.references[1], 3);
    refusal(&bad, "reference");
    let mut bad = base.clone();
    put_word(&mut bad, locations.references[0], 4);
    refusal(&bad, "ordinary raster");
    let mut bad = base.clone();
    put_word(&mut bad, locations.references[0], 2);
    refusal(&bad, "ordinary raster");
    let mut bad = base.clone();
    put_word(&mut bad, locations.references[0] + 4, 65537);
    refusal(&bad, "size");
    let mut bad = base.clone();
    bad[locations.texts[0].start] = 0xff;
    refusal(&bad, "utf-8");
    let mut bad = base.clone();
    let needle = b"\"version\":1";
    let position = bad[locations.texts[0].clone()]
        .windows(needle.len())
        .position(|value| value == needle)
        .unwrap()
        + locations.texts[0].start
        + needle.len()
        - 1;
    bad[position] = b'0';
    refusal(&bad, "positive version");
    let mut bad = base.clone();
    bad.push(0);
    refusal(&bad, "trailing");
}

#[test]
fn future_artwork_description_versions_are_retained_opaquely_and_remain_bounded() {
    let value = r#"{"type":"text","version":4294967295,"future_control":{"keep_exact":11.999950408935547}}"#;
    let base = typed(IccProfile::srgb(), 10001);
    let described = describe(&base, 0, Some(value)).unwrap().unwrap();
    let bytes = described.encode_native(LIMIT, 0, None).unwrap();
    let restored = open16(&bytes).unwrap();
    assert_eq!(description(&restored, 0).as_deref(), Some(value));
    assert_eq!(
        restored.layer_source(0).unwrap().to_rgba16().ok(),
        base.layer_source(0).unwrap().to_rgba16().ok()
    );
    // Bounded at the 16 MiB meta cap (`MAX_LARGE_DESCRIPTION`).
    let oversized =
        json!({"type":"shape","version":1,"payload":"x".repeat(16 * 1024 * 1024)}).to_string();
    assert!(describe(&base, 0, Some(&oversized)).is_err());
    assert!(describe(&base, 0, Some(r#"{"type":"shape","version":0}"#)).is_err());
    assert_eq!(description(&base, 0), None);
}

#[test]
fn descriptions_share_storage_through_structural_edits_and_semantic_noops() {
    let base = typed(IccProfile::srgb(), 10001);
    let value = json!({"type":"shape","version":1,"payload":"a".repeat(48 * 1024)}).to_string();
    let doc = describe(&base, 0, Some(&value)).unwrap().unwrap();
    assert!(doc.retained_bytes() >= base.retained_bytes() + value.len() as u64);
    let duplicate = doc.duplicate_layer(0).unwrap().unwrap();
    assert!(ptr::eq(
        duplicate.artwork_description(0).unwrap().as_ptr(),
        duplicate.artwork_description(1).unwrap().as_ptr()
    ));
    assert!(PrecisionImage::ptr_eq(
        duplicate.layer_source(0).unwrap(),
        duplicate.layer_source(1).unwrap()
    ));
    assert!(duplicate.retained_bytes() < doc.retained_bytes() + 16 * 1024);
    assert!(describe(&doc, 0, Some(&value)).unwrap().is_none());
    assert!(doc.set_layer_opacity(0, 1.0).unwrap().is_none());
    let opacity = doc.set_layer_opacity(0, 0.5).unwrap().unwrap();
    assert_eq!(description(&opacity, 0).as_deref(), Some(value.as_str()));
    let cleared = describe(&doc, 0, None).unwrap().unwrap();
    assert_eq!(description(&cleared, 0), None);
    assert_eq!(
        cleared.layer_source(0).unwrap().to_rgba16().ok(),
        doc.layer_source(0).unwrap().to_rgba16().ok()
    );
    assert!(cleared.retained_bytes() + value.len() as u64 <= doc.retained_bytes());
}

#[test]
fn raw_core_pixel_geometry_and_assign_invalidate_description_but_convert_preserves_it() {
    let base = typed(IccProfile::srgb(), 10001);
    let doc = describe(&base, 0, Some(DESCRIPTION)).unwrap().unwrap();
    let mask = edit(
        &doc,
        json!({"op":"set_mask","layer":0,"width":2,"height":2,"x":1,"y":0}),
        &[1, 257, 32769, 65535],
    );
    assert_eq!(description(&mask, 0).as_deref(), Some(DESCRIPTION));
    let moved_mask = edit(
        &mask,
        json!({"op":"affine","layer":0,"target":"mask","matrix":[1.0,0.0,0.0,1.0,1.0,0.0],"filter":"nearest"}),
        &[],
    );
    assert_eq!(description(&moved_mask, 0).as_deref(), Some(DESCRIPTION));
    let pixels = edit(
        &doc,
        json!({"op":"paint","layer":0,"color":[0.8,0.3,0.1,1.0],"color_space":"document","opacity":1.0}),
        &[65535, 0, 0, 0, 0, 0, 0, 0],
    );
    assert_eq!(description(&pixels, 0), None);
    let transformed = edit(
        &doc,
        json!({"op":"affine","layer":0,"target":"pixels","matrix":[1.0,0.0,0.0,1.0,1.0,0.0],"filter":"nearest"}),
        &[],
    );
    assert_eq!(description(&transformed, 0), None);
    let filtered = doc
        .apply_adjustment(
            0,
            &json!({"type":"adjust","evaluation":"precision_v1","op":"invert","params":{}})
                .to_string(),
            LIMIT,
            0,
            None,
        )
        .unwrap()
        .unwrap();
    assert_eq!(description(&filtered, 0), None);
    let assigned = doc
        .assign_profile(&IccProfile::display_p3(), LIMIT, 0, None)
        .unwrap()
        .unwrap();
    assert_eq!(description(&assigned, 0), None);
    let converted = doc
        .convert_to_profile(&IccProfile::display_p3(), LIMIT, 0, None)
        .unwrap()
        .unwrap();
    assert_eq!(description(&converted, 0).as_deref(), Some(DESCRIPTION));
    assert_eq!(description(&doc, 0).as_deref(), Some(DESCRIPTION));
}

#[test]
fn described_raster_cannot_be_reinterpreted_as_ordinary_retained_image_input() {
    let source = typed(IccProfile::srgb(), 10001);
    let described = describe(&source, 0, Some(DESCRIPTION)).unwrap().unwrap();
    let target = typed(IccProfile::srgb(), 17);
    let request = place("new-source", false);
    assert!(target
        .source_edit_input(Some(SourceInput::Typed(&source)), &request, LIMIT, 0, None)
        .unwrap()
        .is_some());
    let error = match target.source_edit_input(
        Some(SourceInput::Typed(&described)),
        &request,
        LIMIT,
        0,
        None,
    ) {
        Ok(_) => panic!("described raster was silently treated as an ordinary master"),
        Err(error) => error,
    };
    assert!(error.contains("ordinary raster"));
    assert!(byte()
        .source_edit_input(
            Some(SourceInput::Typed(&described)),
            &request,
            LIMIT,
            0,
            None
        )
        .is_err());
}

#[test]
fn native_save_and_open_admission_failures_preserve_existing_file_and_source() {
    let original = mixed16();
    let directory = TempDir::new().unwrap();
    let bytes = save16(&original, &directory);
    let path = CString::new(directory.path().join("typed.rz").to_str().unwrap()).unwrap();
    let mut error = ptr::null_mut();
    assert!(!unsafe {
        dr_precision_doc_save_native(&original, path.as_ptr(), 1, 0, ptr::null(), &mut error)
    });
    assert!(!unsafe { text(error) }.is_empty());
    assert_eq!(std::fs::read(path.to_str().unwrap()).unwrap(), bytes);
    assert!(PrecisionDocument::parse_native(&bytes, 1, 0, None).is_err());
    assert!(PrecisionDocument::parse_native(&bytes, LIMIT, LIMIT, None).is_err());
    assert_eq!(description(&original, 3).as_deref(), Some(DESCRIPTION));
}
