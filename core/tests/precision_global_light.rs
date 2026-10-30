//! The TYPED document's global light — the byte
//! `DrDocument::set_global_light` twin.
//!
//! The edit is the `{op:"global_light"}` request through
//! `dr_precision_doc_edit` and the read is `dr_precision_doc_info`'s
//! `global_light` object. The render oracle is the byte rule itself: an
//! effect with `use_global_light` under a 45°/60° light renders exactly as
//! the same effect authored with its OWN 45°/60° and `use_global_light` off.
//! The persisted form is checked against a table this file assembles itself
//! — `LGLT`, u32 schema 1, f32 angle, f32 altitude — and the default-light
//! file against the bytes of a document that was never touched, so the
//! conditional word 58 is proven absent rather than assumed.

use std::ffi::{c_char, CStr, CString};
use std::path::Path;
use std::ptr;
use std::sync::Arc;

use doctored_core::doc::DrDocument;
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_precision::dr_precision_doc_info;
use doctored_core::ffi_precision_conversion::dr_doc_promote_structure;
use doctored_core::ffi_precision_edit::dr_precision_doc_edit;
use doctored_core::ffi_psd::{
    dr_precision_doc_open_psd, dr_precision_doc_save_psd, dr_psd_import_report,
};
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer as Layer};
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use doctored_core::precision_quantize::PrecisionDither;
use doctored_core::style::{GlobalLight, LayerStyle};
use image::{Rgba, RgbaImage};
use serde_json::{json, Value};
use tempfile::TempDir;

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
const WORD: u32 = 58;

// ------------------------------------------------------------- plumbing --

fn take_error(error: *mut c_char) -> Option<String> {
    if error.is_null() {
        return None;
    }
    unsafe {
        let text = CStr::from_ptr(error).to_string_lossy().into_owned();
        dr_string_free(error);
        Some(text)
    }
}

fn edit(
    document: &PrecisionDocument,
    request: Value,
) -> Result<Option<Box<PrecisionDocument>>, String> {
    let request = CString::new(request.to_string()).unwrap();
    let mut error = ptr::null_mut();
    let result = unsafe {
        dr_precision_doc_edit(
            document,
            request.as_ptr(),
            ptr::null(),
            0,
            LIMIT,
            0,
            ptr::null(),
            &mut error,
        )
    };
    if let Some(error) = take_error(error) {
        return Err(error);
    }
    Ok((!result.is_null()).then(|| unsafe { Box::from_raw(result) }))
}

fn must(document: &PrecisionDocument, request: Value) -> Box<PrecisionDocument> {
    match edit(document, request.clone()) {
        Ok(Some(out)) => out,
        Ok(None) => panic!("{request} reported no change"),
        Err(message) => panic!("{request}: {message}"),
    }
}

fn refusal(document: &PrecisionDocument, request: Value) -> String {
    match edit(document, request.clone()) {
        Err(message) => message,
        Ok(Some(_)) => panic!("{request} must be refused"),
        Ok(None) => panic!("{request} must be refused, not answered as a no-op"),
    }
}

fn light(document: &PrecisionDocument, angle: f32, altitude: f32) -> Box<PrecisionDocument> {
    must(
        document,
        json!({"op": "global_light", "angle": angle, "altitude": altitude}),
    )
}

fn info(document: &PrecisionDocument) -> Value {
    let mut error = ptr::null_mut();
    let raw = unsafe { dr_precision_doc_info(document, &mut error) };
    let text = take_error(raw).unwrap_or_else(|| panic!("info: {:?}", take_error(error)));
    serde_json::from_str(&text).expect("info JSON")
}

fn light_of(document: &PrecisionDocument) -> (f64, f64) {
    let value = &info(document)["global_light"];
    (
        value["angle"].as_f64().expect("an angle"),
        value["altitude"].as_f64().expect("an altitude"),
    )
}

fn render(document: &PrecisionDocument) -> Vec<[u16; 4]> {
    document
        .render(LIMIT, 0, None)
        .expect("render")
        .to_rgba16()
        .expect("u16 projection")
}

fn word(bytes: &[u8], index: usize) -> u32 {
    let at = 4 + index * 4;
    u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap())
}

fn promote(document: &DrDocument) -> Box<PrecisionDocument> {
    let mut error = ptr::null_mut();
    let out = unsafe { dr_doc_promote_structure(document, LIMIT, 0, ptr::null(), &mut error) };
    assert!(!out.is_null(), "promote: {:?}", take_error(error));
    unsafe { Box::from_raw(out) }
}

// ------------------------------------------------------------- fixtures --

fn image(seed: u16) -> PrecisionImage {
    let pixels = (0..4)
        .map(|i| {
            [
                seed.wrapping_mul(97).wrapping_add(i * 911),
                40_000,
                9_000 + i * 3,
                50_000,
            ]
        })
        .collect();
    PrecisionImage::from_rgba16(2, 2, pixels, IccProfile::srgb(), None).expect("an image")
}

fn raster(name: &str, seed: u16) -> Layer {
    Layer::raster(name, image(seed), (0, 0), 0.8, true).expect("a raster")
}

/// `Base`, `A`, `Top`, postorder 0, 1, 2.
fn flat() -> PrecisionDocument {
    PrecisionDocument::from_parts(
        2,
        2,
        IccProfile::srgb(),
        Metadata::default(),
        Resolution::default(),
        vec![raster("Base", 1), raster("A", 2), raster("Top", 4)],
    )
    .expect("a document")
}

/// `Base`, then group `G` holding `A`. Postorder: Base 0, A 1, G 2.
fn grouped() -> PrecisionDocument {
    let group = Layer::isolated_normal("G", vec![raster("A", 2)], 0.9, true).expect("a group");
    PrecisionDocument::from_parts(
        2,
        2,
        IccProfile::srgb(),
        Metadata::default(),
        Resolution::default(),
        vec![raster("Base", 1), group],
    )
    .expect("a document")
}

fn float_document() -> PrecisionDocument {
    let document =
        PrecisionDocument::open_preserving("tests/fixtures/float_11f/scene.tiff", LIMIT, 0, None)
            .expect("the float fixture opens");
    assert!(document.is_float());
    document
}

/// A drop shadow and a bevel that read the global light.
const GLOBAL: &str = r#"{"effects":[
    {"type":"drop_shadow","angle":120,"use_global_light":true,"distance":4,"size":2},
    {"type":"bevel_emboss","size":3,"angle":120,"altitude":30,"use_global_light":true}]}"#;

/// The same two effects authored with their OWN 45°/60°.
const OWN: &str = r#"{"effects":[
    {"type":"drop_shadow","angle":45,"use_global_light":false,"distance":4,"size":2},
    {"type":"bevel_emboss","size":3,"angle":45,"altitude":60,"use_global_light":false}]}"#;

/// A 24×20 byte document: a transparent canvas with an opaque 8×6 block in
/// the middle, styled with `style`, under `light`.
fn byte_styled(style: &str, light: GlobalLight) -> DrDocument {
    let mut pixels = RgbaImage::new(24, 20);
    for y in 7..13 {
        for x in 8..16 {
            pixels.put_pixel(x, y, Rgba([200, 90, (x * 9) as u8, 255]));
        }
    }
    let mut document = DrDocument::from_pixels(pixels);
    document.layers[0].style = Some(Arc::new(
        LayerStyle::from_json(style).expect("the test style"),
    ));
    document.global_light = light;
    document
}

const LIT: GlobalLight = GlobalLight {
    angle: 45.0,
    altitude: 60.0,
};

// --------------------------------------------------------------- the op --

#[test]
fn the_op_sets_sanitizes_and_refuses_as_the_byte_setter_does() {
    let base = flat();
    assert_eq!(light_of(&base), (120.0, 30.0), "the default light");
    let before = render(&base);

    let lit = light(&base, 45.0, 60.0);
    assert_eq!(light_of(&lit), (45.0, 60.0));
    assert_eq!(render(&lit), before, "no style reads it, so no pixel moves");
    assert!(
        edit(
            &lit,
            json!({"op": "global_light", "angle": 45, "altitude": 60})
        )
        .expect("not an error")
        .is_none(),
        "the same light is the exact no-op"
    );

    let sanitized = light(&base, 540.0, 120.0);
    assert_eq!(light_of(&sanitized), (-180.0, 90.0));

    // Both numbers are required and an unknown field refuses.
    assert!(refusal(&base, json!({"op": "global_light", "angle": 45}))
        .contains("invalid native global light request"));
    assert!(refusal(
        &base,
        json!({"op": "global_light", "angle": 45, "altitude": 60, "extra": 1})
    )
    .contains("invalid native global light request"));

    // A float document holds the light too (measured: the take
    // set and read back a 32-bit document's light in Photoshop 26.10.0),
    // starting at the default.
    let float = float_document();
    assert_eq!(light_of(&float), (120.0, 30.0));
    let lit = light(&float, 45.0, 60.0);
    assert_eq!(light_of(&lit), (45.0, 60.0));
    assert!(lit.is_float());
    assert!(doctored_core::precision_capability::require("global_light", "document", 16).is_ok());
    assert!(doctored_core::precision_capability::require("global_light", "document", 32).is_ok());
}

// ------------------------------------------------------- render oracle --

#[test]
fn the_typed_render_reads_the_document_light() {
    let global = promote(&byte_styled(GLOBAL, LIT));
    let own = promote(&byte_styled(OWN, GlobalLight::default()));
    assert_eq!(light_of(&global), (45.0, 60.0), "8→16 carries the light");
    assert_eq!(
        render(&global),
        render(&own),
        "a global-light effect under 45/60 is the same effect authored at 45/60"
    );
    assert_eq!(global.style_extent16(0), own.style_extent16(0));

    // Negative control: under the default light it differs.
    let default = promote(&byte_styled(GLOBAL, GlobalLight::default()));
    // (The grown extent pads every direction by the shift, so only the
    // pixels can tell the two lights apart.)
    assert_ne!(render(&default), render(&own));

    // …and moving the light on the typed document moves it to the same place.
    let moved = light(&default, 45.0, 60.0);
    assert_eq!(render(&moved), render(&own));
    assert_eq!(moved.style_extent16(0), own.style_extent16(0));
}

// --------------------------------------------------------------- carry --

#[test]
fn the_light_survives_structure_geometry_profile_and_conversion() {
    let lit = light(&flat(), 45.0, 60.0);
    let expect = |document: &PrecisionDocument, what: &str| {
        assert_eq!(light_of(document), (45.0, 60.0), "{what} keeps the light");
    };
    expect(
        &must(
            &lit,
            json!({"op": "group_layers", "layers": [1], "name": "G"}),
        ),
        "group_layers",
    );
    expect(&must(&lit, json!({"op": "rotate90"})), "rotate90");
    expect(
        &lit.assign_profile(&IccProfile::display_p3(), LIMIT, 0, None)
            .expect("assign")
            .expect("a change"),
        "assign_profile",
    );
    expect(
        &must(&lit, json!({"op": "merge_down", "layer": 2})),
        "merge_down",
    );
    expect(
        &must(
            &lit,
            json!({"op": "stamp_visible", "name": "Stamp", "anchor": 2}),
        ),
        "stamp_visible",
    );
    expect(&must(&lit, json!({"op": "flatten"})), "flatten");

    // 16→8, structure-preserving and flattened.
    let narrowed = lit
        .narrow_structure_to_u8(PrecisionDither::None, LIMIT, 0, None)
        .expect("narrow");
    assert_eq!(narrowed.global_light, LIT);
    let flattened = lit
        .downconvert(PrecisionDither::None, LIMIT, 0, None)
        .expect("downconvert");
    assert_eq!(flattened.global_light, LIT);
    // …and 8→16 flattened.
    let promoted = byte_styled(GLOBAL, LIT)
        .promote_flattened(LIMIT, 0, None)
        .expect("promote flattened");
    assert_eq!(light_of(&promoted), (45.0, 60.0));

    // Save and reopen.
    let bytes = lit.encode_native(LIMIT, 0, None).expect("bytes");
    let reopened = PrecisionDocument::parse_native(&bytes, LIMIT, 0, None).expect("reopen");
    expect(&reopened, "save/reopen");
}

// -------------------------------------------------------------- native --

fn table(angle: f32, altitude: f32) -> Vec<u8> {
    let mut table = b"LGLT".to_vec();
    table.extend(1u32.to_le_bytes());
    table.extend(angle.to_le_bytes());
    table.extend(altitude.to_le_bytes());
    table
}

fn lids_at(bytes: &[u8], records: usize) -> usize {
    let tail = bytes.len() - (12 + 8 * records);
    assert_eq!(&bytes[tail..tail + 4], b"LIDS", "LIDS stays LAST");
    tail
}

#[test]
fn the_light_round_trips_the_native_file_and_the_default_is_the_57_writer() {
    let base = flat();
    let plain = base.encode_native(LIMIT, 0, None).expect("plain bytes");
    assert!(!plain.windows(4).any(|w| w == b"LGLT"));
    assert_ne!(word(&plain, 1), WORD);

    let lit = light(&base, 45.0, 60.0);
    let back = light(&lit, 120.0, 30.0);
    assert_eq!(
        back.encode_native(LIMIT, 0, None).expect("back"),
        plain,
        "default → 45/60 → default writes the untouched document's exact bytes"
    );

    let bytes = lit.encode_native(LIMIT, 0, None).expect("lit bytes");
    assert_eq!(word(&bytes, 0), 46, "the identity word stays outermost");
    assert_eq!(word(&bytes, 1), WORD, "58 is the first word inside 46");
    let tail = lids_at(&bytes, lit.layer_count());
    let expected = table(45.0, 60.0);
    assert_eq!(
        &bytes[tail - expected.len()..tail],
        &expected[..],
        "LGLT, as this file assembles it, sits immediately before LIDS"
    );
    let reopened = PrecisionDocument::parse_native(&bytes, LIMIT, 0, None).expect("reopen");
    assert_eq!(light_of(&reopened), (45.0, 60.0));
    assert_eq!(
        reopened.encode_native(LIMIT, 0, None).expect("again"),
        bytes,
        "reopen then resave is byte-identical"
    );
}

#[test]
fn the_light_word_combines_with_a_closed_group() {
    let closed = must(
        &grouped(),
        json!({"op": "layer_open", "layer": 2, "open": false}),
    );
    let lit = light(&closed, 45.0, 60.0);
    let bytes = lit.encode_native(LIMIT, 0, None).expect("bytes");
    assert_eq!(
        (word(&bytes, 0), word(&bytes, 1), word(&bytes, 2)),
        (46, WORD, 55)
    );
    let tail = lids_at(&bytes, lit.layer_count());
    let expected = table(45.0, 60.0);
    assert_eq!(&bytes[tail - expected.len()..tail], &expected[..]);
    let reopened = PrecisionDocument::parse_native(&bytes, LIMIT, 0, None).expect("reopen");
    assert_eq!(light_of(&reopened), (45.0, 60.0));
    assert_eq!(info(&reopened)["layers"][2]["open"], json!(false));
    assert_eq!(
        reopened.encode_native(LIMIT, 0, None).expect("again"),
        bytes
    );
}

#[test]
fn a_crafted_light_table_refuses_or_repairs() {
    let lit = light(&flat(), 45.0, 60.0);
    let good = lit.encode_native(LIMIT, 0, None).expect("bytes");
    let at = good
        .windows(4)
        .position(|w| w == b"LGLT")
        .expect("an LGLT table");
    let patched = |offset: usize, value: &[u8]| {
        let mut bytes = good.clone();
        bytes[at + offset..at + offset + value.len()].copy_from_slice(value);
        bytes
    };
    let refuse = |bytes: &[u8], what: &str| -> String {
        match PrecisionDocument::parse_native(bytes, LIMIT, 0, None) {
            Err(message) => message,
            Ok(_) => panic!("{what} must refuse the open"),
        }
    };
    assert_eq!(
        refuse(&patched(0, b"XXXX"), "a bad tag"),
        "unsupported native global light schema"
    );
    assert_eq!(
        refuse(&patched(4, &2u32.to_le_bytes()), "a bad schema"),
        "unsupported native global light schema"
    );

    // NaN repairs to the default through `GlobalLight::sane`, and the resave
    // drops the table.
    let mut nan = patched(8, &f32::NAN.to_le_bytes());
    nan[at + 12..at + 16].copy_from_slice(&f32::NAN.to_le_bytes());
    let repaired = PrecisionDocument::parse_native(&nan, LIMIT, 0, None).expect("repaired");
    assert_eq!(light_of(&repaired), (120.0, 30.0));
    let resaved = repaired.encode_native(LIMIT, 0, None).expect("resave");
    assert!(!resaved.windows(4).any(|w| w == b"LGLT"));
    assert_ne!(word(&resaved, 1), WORD);

    // A float document's light persists in the same table inside 40,
    // and the file re-encodes byte for byte.
    let lit = light(&float_document(), 45.0, 60.0);
    let float = lit.encode_native(LIMIT, 0, None).expect("float bytes");
    assert_eq!(word(&float, 0), 46);
    assert!(float.windows(4).any(|w| w == b"LGLT"));
    let reopened = PrecisionDocument::parse_native(&float, LIMIT, 0, None).expect("reopens");
    assert!(reopened.is_float());
    assert_eq!(light_of(&reopened), (45.0, 60.0));
    assert_eq!(reopened.encode_native(LIMIT, 0, None).unwrap(), float);
}

// ----------------------------------------------------------------- PSD --

fn cpath(path: &Path) -> CString {
    CString::new(path.to_str().expect("utf-8 path")).expect("no NUL")
}

/// The body of image resource `id`, found by its `8BIM` header with an empty
/// Pascal name.
fn resource(bytes: &[u8], id: u16) -> Option<Vec<u8>> {
    let mut header = b"8BIM".to_vec();
    header.extend(id.to_be_bytes());
    header.extend([0, 0]);
    let at = bytes.windows(8).position(|w| w == &header[..])? + 8;
    let size = u32::from_be_bytes(bytes[at..at + 4].try_into().unwrap()) as usize;
    Some(bytes[at + 4..at + 4 + size].to_vec())
}

#[test]
fn a_sixteen_bit_psd_writes_and_reads_the_document_light() {
    let directory = TempDir::new().expect("tempdir");
    let path = directory.path().join("lit16.psd");
    let lit = promote(&byte_styled(GLOBAL, LIT));
    assert_eq!(light_of(&lit), (45.0, 60.0));
    let projection = lit.render(LIMIT, 0, None).expect("projection");
    let mut error = ptr::null_mut();
    let saved = unsafe {
        dr_precision_doc_save_psd(
            &*lit,
            &projection,
            cpath(&path).as_ptr(),
            0,
            0,
            LIMIT,
            0,
            &mut error,
        )
    };
    assert!(saved, "{:?}", take_error(error));
    let bytes = std::fs::read(&path).expect("read back");
    assert_eq!(resource(&bytes, 1037), Some(45i32.to_be_bytes().to_vec()));
    assert_eq!(resource(&bytes, 1049), Some(60i32.to_be_bytes().to_vec()));

    let mut error = ptr::null_mut();
    let report = unsafe { dr_psd_import_report(cpath(&path).as_ptr(), &mut error) };
    let report: Value = serde_json::from_str(&take_error(report).expect("a report"))
        .unwrap_or_else(|_| panic!("{:?}", take_error(error)));
    let fingerprint = CString::new(report["source_fingerprint"].as_str().unwrap()).unwrap();
    let mut error = ptr::null_mut();
    let opened = unsafe {
        dr_precision_doc_open_psd(
            cpath(&path).as_ptr(),
            0,
            fingerprint.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut error,
        )
    };
    assert!(!opened.is_null(), "{:?}", take_error(error));
    let opened = unsafe { Box::from_raw(opened) };
    assert_eq!(light_of(&opened), (45.0, 60.0));
    let style: Value = serde_json::from_str(
        info(&opened)["layers"][0]["style"]
            .as_str()
            .expect("the style JSON"),
    )
    .expect("style JSON");
    assert!(
        style["effects"]
            .as_array()
            .expect("effects")
            .iter()
            .all(|effect| effect["use_global_light"] == json!(true)),
        "{style}"
    );
}
