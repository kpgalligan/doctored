//! The Bevel & Emboss TEXTURE in PSD: our 8- and 16-bit
//! exports write `useTexture` true, the texture's `textureDepth` (`UntF
//! #Prc`, percent) and `InvT`, and its tile in the pattern table; the import
//! reads the same texture back; a bevel without one writes `useTexture`
//! false and reads back no texture key; a hand-encoded `ebbl` whose texture
//! names a pattern the file does not carry declines when `useTexture` is on
//! and drops the texture when it is off. The expected items are built with
//! `psd_descriptor_fixture`, never the crate's writer.

use std::ffi::{c_char, CString};
use std::path::Path;
use std::ptr;

use doctored_core::doc::DrDocument;
use doctored_core::ffi_precision_edit::dr_precision_doc_edit;
use doctored_core::ffi_psd::{
    dr_doc_save_psd, dr_precision_doc_open_psd, dr_precision_doc_save_psd,
};
use doctored_core::icc::IccProfile;
use doctored_core::job_control::DrJobControl;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer as PLayer};
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use serde_json::{json, Value};
use tempfile::TempDir;

use crate::common::psd_photoshop::{import_report, open_psd, strings, LAYERS};
use crate::common::{cpath, rect_layer_doc, styled, take_err_string};
use crate::psd_descriptor_fixture::{boolean, desc, objc, text, untf};
use crate::psd_mask_fixture as fixture8;

const LIMIT: u64 = REFERENCE_WORKING_BYTES;

fn textured() -> Value {
    json!({"effects":[{"type":"bevel_emboss","size":6,"texture_enabled":true,
        "texture":{"pattern":{"preset":"dots"},"scale":0.75,"depth":-2.5,"invert":true,
            "link_with_layer":false,"phase":[3,5]}}]})
}

fn contains(bytes: &[u8], needle: &[u8]) -> bool {
    bytes.windows(needle.len()).any(|w| w == needle)
}

fn assert_written(bytes: &[u8], what: &str) {
    assert!(
        contains(bytes, &boolean(b"useTexture", 1)),
        "{what}: useTexture"
    );
    assert!(
        contains(bytes, &untf(b"textureDepth", b"#Prc", -250.0)),
        "{what}: textureDepth"
    );
    assert!(contains(bytes, &boolean(b"InvT", 1)), "{what}: InvT");
    assert!(contains(bytes, &boolean(b"Algn", 0)), "{what}: Algn");
    assert!(
        contains(bytes, b"Patt") || contains(bytes, b"Pat2"),
        "{what}: the pattern table"
    );
}

fn save8(doc: &DrDocument, path: &Path) {
    let mut error: *mut c_char = ptr::null_mut();
    let ok = unsafe {
        dr_doc_save_psd(
            doc,
            ptr::null(),
            cpath(path).as_ptr(),
            0,
            LAYERS,
            &mut error,
        )
    };
    assert!(ok, "{}", take_err_string(error));
}

fn style_value(doc: &DrDocument, index: usize) -> Value {
    serde_json::from_str(&doc.layers[index].style.as_deref().unwrap().to_json()).unwrap()
}

#[test]
fn a_texture_crosses_our_8_bit_psd_both_ways() {
    let dir = TempDir::new().unwrap();
    let doc = styled(
        &rect_layer_doc((64, 48), (16, 12, 32, 24), [0x30, 0xa0, 0x50, 255]),
        1,
        &textured().to_string(),
    );
    let path = dir.path().join("texture8.psd");
    save8(&doc, &path);
    assert_written(&std::fs::read(&path).unwrap(), "8-bit");
    let report = import_report(&path);
    assert_eq!(
        strings(&report, "verified_losses"),
        Vec::<String>::new(),
        "{report}"
    );
    let back = open_psd(&path, &report, LAYERS);
    let top = back.layers.len() - 1;
    let got = style_value(&back, top);
    assert_eq!(got, style_value(&doc, 1));
    assert_eq!(got["effects"][0]["texture"]["depth"], json!(-2.5));
    assert_eq!(back.flattened().unwrap(), doc.flattened().unwrap());
}

/// A 16-bit document: white canvas, a green rect, `style` on the rect.
fn typed_doc(style: &Value) -> PrecisionDocument {
    let image = |w: u32, h: u32, px: [u8; 4]| {
        PrecisionImage::from_rgba16(
            w,
            h,
            vec![px.map(|v| u16::from(v) * 257); (w * h) as usize],
            IccProfile::srgb(),
            None,
        )
        .unwrap()
    };
    let doc = PrecisionDocument::from_parts(
        64,
        48,
        IccProfile::srgb(),
        Metadata::default(),
        Resolution::default(),
        vec![
            PLayer::raster("Base", image(64, 48, [255; 4]), (0, 0), 1.0, true).unwrap(),
            PLayer::raster(
                "Rect",
                image(32, 24, [0x30, 0xa0, 0x50, 255]),
                (16, 12),
                1.0,
                true,
            )
            .unwrap(),
        ],
    )
    .unwrap();
    let request = json!({"op":"layer_style","version":1,"layer":1,"style":style});
    let c = CString::new(request.to_string()).unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_edit(
            &doc,
            c.as_ptr(),
            ptr::null(),
            0,
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!out.is_null(), "{}", take_err_string(err));
    *unsafe { Box::from_raw(out) }
}

#[test]
fn a_texture_crosses_our_16_bit_psd_both_ways() {
    let dir = TempDir::new().unwrap();
    let typed = typed_doc(&textured());
    let path = dir.path().join("texture16.psd");
    let projection = typed.render(LIMIT, 0, None).unwrap();
    let mut error: *mut c_char = ptr::null_mut();
    let ok = unsafe {
        dr_precision_doc_save_psd(
            &typed,
            &projection,
            cpath(&path).as_ptr(),
            0,
            LAYERS,
            LIMIT,
            0,
            &mut error,
        )
    };
    assert!(ok, "{}", take_err_string(error));
    assert_written(&std::fs::read(&path).unwrap(), "16-bit");
    let report = import_report(&path);
    let token = CString::new(report["source_fingerprint"].as_str().unwrap()).unwrap();
    let mut error = ptr::null_mut();
    let raw = unsafe {
        dr_precision_doc_open_psd(
            cpath(&path).as_ptr(),
            LAYERS,
            token.as_ptr(),
            LIMIT,
            0,
            ptr::null::<DrJobControl>(),
            &mut error,
        )
    };
    assert!(!raw.is_null(), "{}", take_err_string(error));
    let back = unsafe { Box::from_raw(raw) };
    let top = back.layer_count() - 1;
    assert_eq!(
        back.layer(top).unwrap().reported_style(),
        typed.layer(1).unwrap().reported_style()
    );
}

#[test]
fn a_bevel_without_texture_writes_use_texture_false_and_reads_no_key() {
    let dir = TempDir::new().unwrap();
    let doc = styled(
        &rect_layer_doc((32, 24), (8, 6, 16, 12), [0, 0, 0, 255]),
        1,
        r#"{"effects":[{"type":"bevel_emboss","texture":{"depth":3}}]}"#,
    );
    let path = dir.path().join("plain.psd");
    save8(&doc, &path);
    let bytes = std::fs::read(&path).unwrap();
    assert!(
        contains(&bytes, &boolean(b"useTexture", 0)),
        "useTexture false"
    );
    assert!(!contains(&bytes, &untf(b"textureDepth", b"#Prc", 300.0)));
    let back = open_psd(&path, &import_report(&path), LAYERS);
    let text = back.layers[back.layers.len() - 1]
        .style
        .as_deref()
        .unwrap()
        .to_json();
    assert!(!text.contains("texture"), "texture read from false: {text}");
}

/// A one-layer 8-bit file whose bevel's texture names a pattern the file
/// does not carry, `useTexture` `on`.
fn missing_file(on: bool) -> Vec<u8> {
    let items = vec![
        boolean(b"masterFXSwitch", 1),
        objc(
            b"ebbl",
            b"ebbl",
            &[
                boolean(b"enab", 1),
                boolean(b"useTexture", u8::from(on)),
                objc(
                    b"Ptrn",
                    b"Ptrn",
                    &[text(b"Nm  ", "Nowhere"), text(b"Idnt", "no-such-id")],
                ),
                untf(b"Scl ", b"#Prc", 100.0),
                untf(b"textureDepth", b"#Prc", 100.0),
            ],
        ),
    ];
    let mut body = 0u32.to_be_bytes().to_vec();
    body.extend(16u32.to_be_bytes());
    body.extend(desc(b"null", &items));
    if !body.len().is_multiple_of(2) {
        body.push(0);
    }
    let mut record =
        fixture8::Record::raster([0, 0, 1, 2], &[11, 29, 47, 255, 101, 151, 199, 255], false);
    record.name = "Styled";
    record.blocks = fixture8::block(b"lfx2", &body);
    fixture8::fixture(&[record], [2, 1], false)
}

#[test]
fn a_missing_pattern_declines_only_when_the_texture_is_on() {
    let dir = TempDir::new().unwrap();
    let on = dir.path().join("on.psd");
    std::fs::write(&on, missing_file(true)).unwrap();
    let report = import_report(&on);
    let losses = strings(&report, "verified_losses");
    assert!(
        losses
            .iter()
            .any(|l| l.contains("bevel texture names a pattern the file does not carry")),
        "{losses:?}"
    );
    assert!(open_psd(&on, &report, LAYERS).layers[0].style.is_none());
    let off = dir.path().join("off.psd");
    std::fs::write(&off, missing_file(false)).unwrap();
    let report = import_report(&off);
    let doc = open_psd(&off, &report, LAYERS);
    let style = style_value(&doc, 0);
    assert_eq!(style["effects"][0]["type"], json!("bevel_emboss"));
    assert!(
        style["effects"][0].get("texture").is_none(),
        "no texture: {style}"
    );
}

/// `bytes` with its one `from` replaced by `to` (the same length).
fn patched(bytes: &[u8], from: &[u8], to: &[u8]) -> Vec<u8> {
    assert_eq!(from.len(), to.len());
    let at = bytes
        .windows(from.len())
        .position(|w| w == from)
        .expect("the item is written");
    assert!(!contains(&bytes[at + 1..], from), "one occurrence");
    let mut out = bytes.to_vec();
    out[at..at + to.len()].copy_from_slice(to);
    out
}

#[test]
fn an_unreadable_texture_that_is_off_drops_only_the_texture() {
    let dir = TempDir::new().unwrap();
    let doc = styled(
        &rect_layer_doc((64, 48), (16, 12, 32, 24), [0x30, 0xa0, 0x50, 255]),
        1,
        &textured().to_string(),
    );
    let source = dir.path().join("source.psd");
    save8(&doc, &source);
    // `useTexture` off, and a `Scl ` that is TEXT rather than a unit: the
    // pattern resolves, then the texture's own keys fail to read.
    let bytes = patched(
        &std::fs::read(&source).unwrap(),
        &boolean(b"useTexture", 1),
        &boolean(b"useTexture", 0),
    );
    let bytes = patched(&bytes, &untf(b"Scl ", b"#Prc", 75.0), &text(b"Scl ", "abc"));
    let path = dir.path().join("off-unreadable.psd");
    std::fs::write(&path, bytes).unwrap();
    let report = import_report(&path);
    assert_eq!(
        strings(&report, "verified_losses"),
        Vec::<String>::new(),
        "{report}"
    );
    let back = open_psd(&path, &report, LAYERS);
    let style = style_value(&back, back.layers.len() - 1);
    assert_eq!(style["effects"][0]["type"], json!("bevel_emboss"));
    assert!(
        style["effects"][0].get("texture").is_none(),
        "no texture: {style}"
    );
}
