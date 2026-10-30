//! Layer style NOISE in PSD: each of the four effects
//! that has one writes its `noise` as `Nose` (`UntF #Prc`, percent) in our
//! 8- and 16-bit exports, the import reads it back as the same `noise`, a
//! style without noise still writes `Nose` 0 (every earlier writer's item)
//! and reads back with no `noise` key, and a hand-encoded `lfx2` drop shadow
//! with `Nose` 0 adds no key while `Nose` 37 reads as 0.37. The expected
//! items are built with `psd_descriptor_fixture`, never the crate's writer.

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
use crate::psd_descriptor_fixture::{boolean, desc, objc, untf};
use crate::psd_mask_fixture as fixture8;

const LIMIT: u64 = REFERENCE_WORKING_BYTES;

/// One noise per class, each its own percent so its item is its own bytes.
const NOISES: [(&str, f64); 4] = [
    ("drop_shadow", 0.25),
    ("inner_shadow", 0.5),
    ("outer_glow", 0.75),
    ("inner_glow", 1.0),
];

fn noisy() -> Value {
    let effects: Vec<Value> = NOISES
        .iter()
        .map(|(kind, noise)| json!({"type": kind, "noise": noise}))
        .collect();
    json!({ "effects": effects })
}

fn contains(bytes: &[u8], needle: &[u8]) -> bool {
    bytes.windows(needle.len()).any(|w| w == needle)
}

fn assert_written(bytes: &[u8], what: &str) {
    for (kind, noise) in NOISES {
        assert!(
            contains(bytes, &untf(b"Nose", b"#Prc", noise * 100.0)),
            "{what}: {kind}'s Nose {}",
            noise * 100.0
        );
    }
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

fn noises(style: &Value) -> Vec<Option<f64>> {
    style["effects"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["noise"].as_f64())
        .collect()
}

#[test]
fn every_noise_crosses_our_8_bit_psd_both_ways() {
    let dir = TempDir::new().unwrap();
    let doc = styled(
        &rect_layer_doc((64, 48), (16, 12, 32, 24), [0x30, 0xa0, 0x50, 255]),
        1,
        &noisy().to_string(),
    );
    let path = dir.path().join("noise8.psd");
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
    assert_eq!(style_value(&back, top), style_value(&doc, 1));
    assert_eq!(
        noises(&style_value(&back, top)),
        vec![Some(0.25), Some(0.75), Some(1.0), Some(0.5)],
        "render order: drop shadow, outer glow, inner glow, inner shadow"
    );
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
fn every_noise_crosses_our_16_bit_psd_both_ways() {
    let dir = TempDir::new().unwrap();
    let typed = typed_doc(&noisy());
    let path = dir.path().join("noise16.psd");
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
fn a_style_without_noise_writes_nose_zero_and_reads_no_key() {
    let dir = TempDir::new().unwrap();
    let doc = styled(
        &rect_layer_doc((32, 24), (8, 6, 16, 12), [0, 0, 0, 255]),
        1,
        r#"{"effects":[{"type":"drop_shadow"},{"type":"inner_shadow"},{"type":"outer_glow"},
            {"type":"inner_glow"}]}"#,
    );
    let path = dir.path().join("plain.psd");
    save8(&doc, &path);
    let bytes = std::fs::read(&path).unwrap();
    assert!(contains(&bytes, &untf(b"Nose", b"#Prc", 0.0)), "Nose 0");
    let back = open_psd(&path, &import_report(&path), LAYERS);
    let text = back.layers[back.layers.len() - 1]
        .style
        .as_deref()
        .unwrap()
        .to_json();
    assert!(!text.contains("noise"), "noise read from Nose 0: {text}");
}

/// A one-layer 8-bit file whose drop shadow carries `Nose` `percent`.
fn shadow_file(percent: f64) -> Vec<u8> {
    let items = vec![
        boolean(b"masterFXSwitch", 1),
        objc(
            b"DrSh",
            b"DrSh",
            &[boolean(b"enab", 1), untf(b"Nose", b"#Prc", percent)],
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
fn a_photoshop_nose_reads_as_noise_and_zero_adds_no_key() {
    let dir = TempDir::new().unwrap();
    for (percent, want) in [(0.0, None), (37.0, Some(0.37))] {
        let path = dir.path().join(format!("nose{percent}.psd"));
        std::fs::write(&path, shadow_file(percent)).unwrap();
        let report = import_report(&path);
        let doc = open_psd(&path, &report, LAYERS);
        let style = style_value(&doc, 0);
        assert_eq!(noises(&style), vec![want], "Nose {percent}: {style}");
    }
}
