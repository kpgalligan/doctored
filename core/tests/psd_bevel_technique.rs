//! Bevel & Emboss Technique (#2) across the PSD `lfx2` codec and `.asl`:
//! `bvlT` ↔ `technique` (`SfBL` smooth, `PrBL` chisel_hard, `Slmt`
//! chisel_soft). The expected `bvlT` items are built with the hand-encoded
//! descriptor builder (`psd_descriptor_fixture`), never with the crate's own
//! writer. A Smooth bevel still writes `SfBL` and reads back with no key.
//! The two Photoshop-authored PSDs (bevel-chisel-578 take1's
//! `bevel_08_h_{hard,soft}_8`, `fixtures/psd_photoshop/bevel_chisel_578`)
//! import as Chisel Hard and Chisel Soft, no longer as Smooth.

use std::ffi::{c_char, CString};
use std::path::Path;
use std::ptr;

use doctored_core::doc::DrDocument;
use doctored_core::ffi_precision_edit::dr_precision_doc_edit;
use doctored_core::ffi_psd::{
    dr_doc_save_psd, dr_precision_doc_open_psd, dr_precision_doc_save_psd,
};
use doctored_core::ffi_style::{dr_asl_decode, dr_asl_encode};
use doctored_core::icc::IccProfile;
use doctored_core::job_control::DrJobControl;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer as PLayer};
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use serde_json::{json, Value};
use tempfile::TempDir;

use crate::common::psd_photoshop::{fixture, import_report, open_psd, strings, LAYERS};
use crate::common::{cpath, rect_layer_doc, styled, take_err_string};
use crate::psd_descriptor_fixture::enumeration;

const LIMIT: u64 = REFERENCE_WORKING_BYTES;

/// The three techniques: style JSON name and Photoshop's `bvlT` id.
const TECHNIQUES: [(&str, &[u8; 4]); 3] = [
    ("smooth", b"SfBL"),
    ("chisel_hard", b"PrBL"),
    ("chisel_soft", b"Slmt"),
];

fn bevel_style(technique: &str) -> Value {
    json!({"effects":[{"type":"bevel_emboss","style":"emboss","technique":technique,
        "size":6,"soften":1,"angle":45,"use_global_light":false,"altitude":40}]})
}

/// The `bvlT` item as Photoshop writes it: `bvlT` enum `bvlT` `<id>`.
fn bvlt(id: &[u8; 4]) -> Vec<u8> {
    enumeration(b"bvlT", b"bvlT", id)
}

fn contains(bytes: &[u8], needle: &[u8]) -> bool {
    bytes.windows(needle.len()).any(|w| w == needle)
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

fn style_json(doc: &DrDocument, index: usize) -> String {
    doc.layers[index]
        .style
        .as_deref()
        .expect("a style")
        .to_json()
}

#[test]
fn each_technique_crosses_our_8_bit_psd_both_ways() {
    let dir = TempDir::new().unwrap();
    for (name, id) in TECHNIQUES {
        let doc = styled(
            &rect_layer_doc((64, 48), (16, 12, 32, 24), [0x30, 0xa0, 0x50, 255]),
            1,
            &bevel_style(name).to_string(),
        );
        let path = dir.path().join(format!("{name}.psd"));
        save8(&doc, &path);
        let bytes = std::fs::read(&path).unwrap();
        assert!(contains(&bytes, &bvlt(id)), "{name} writes bvlT {id:?}");
        for (other, other_id) in TECHNIQUES {
            if other != name {
                assert!(!contains(&bytes, &bvlt(other_id)), "{name}: no {other}");
            }
        }
        let report = import_report(&path);
        assert_eq!(
            strings(&report, "verified_losses"),
            Vec::<String>::new(),
            "{report}"
        );
        let back = open_psd(&path, &report, LAYERS);
        let index = back.layers.len() - 1;
        let text = style_json(&back, index);
        assert_eq!(text, style_json(&doc, 1), "{name}");
        assert_eq!(
            text.contains("technique"),
            name != "smooth",
            "{name}: only a Chisel reads a key: {text}"
        );
        assert_eq!(
            back.flattened().unwrap(),
            doc.flattened().unwrap(),
            "{name}: the reopened file renders the same"
        );
    }
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
fn each_technique_crosses_our_16_bit_psd_both_ways() {
    let dir = TempDir::new().unwrap();
    for (name, id) in TECHNIQUES {
        let typed = typed_doc(&bevel_style(name));
        let path = dir.path().join(format!("{name}16.psd"));
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
        let bytes = std::fs::read(&path).unwrap();
        assert!(contains(&bytes, &bvlt(id)), "{name} writes bvlT {id:?}");
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
        let reported = back.layer(top).unwrap().reported_style().unwrap();
        assert_eq!(
            Some(reported.clone()),
            typed.layer(1).unwrap().reported_style(),
            "{name}"
        );
        assert_eq!(
            reported.contains("technique"),
            name != "smooth",
            "{reported}"
        );
    }
}

// ------------------------------------------------------------------ .asl --

/// Standard base64 (RFC 4648, padded) decoded, written here so the test
/// does not lean on the crate's codec.
fn base64(text: &str) -> Vec<u8> {
    let value = |c: u8| -> u32 {
        match c {
            b'A'..=b'Z' => u32::from(c - b'A'),
            b'a'..=b'z' => u32::from(c - b'a') + 26,
            b'0'..=b'9' => u32::from(c - b'0') + 52,
            b'+' => 62,
            b'/' => 63,
            other => panic!("not base64: {other}"),
        }
    };
    let mut out = Vec::new();
    for chunk in text.as_bytes().chunks(4) {
        let pad = chunk.iter().filter(|&&c| c == b'=').count();
        let mut n = 0u32;
        for (i, &c) in chunk.iter().enumerate() {
            n |= if c == b'=' { 0 } else { value(c) } << (18 - 6 * i);
        }
        out.extend_from_slice(&n.to_be_bytes()[1..4 - pad]);
    }
    out
}

#[test]
fn a_style_library_keeps_each_technique() {
    let presets: Vec<Value> = TECHNIQUES
        .iter()
        .map(|(name, _)| json!({"name": name, "blending": null, "style": bevel_style(name)}))
        .collect();
    let c = CString::new(Value::from(presets).to_string()).unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    let raw = unsafe { dr_asl_encode(c.as_ptr(), &mut err) };
    assert!(!raw.is_null(), "{}", take_err_string(err));
    let out: Value = serde_json::from_str(&take_err_string(raw)).unwrap();
    assert_eq!(out["skipped"], json!([]));
    let bytes = base64(out["asl"].as_str().unwrap());
    for (_, id) in TECHNIQUES {
        assert!(contains(&bytes, &bvlt(id)), "the library writes {id:?}");
    }
    let mut err: *mut c_char = ptr::null_mut();
    let raw = unsafe { dr_asl_decode(bytes.as_ptr(), bytes.len(), &mut err) };
    assert!(!raw.is_null(), "{}", take_err_string(err));
    let report: Value = serde_json::from_str(&take_err_string(raw)).unwrap();
    let styles = report["styles"].as_array().unwrap();
    assert_eq!(styles.len(), 3);
    for ((name, _), got) in TECHNIQUES.iter().zip(styles) {
        assert_eq!(got["dropped"], json!([]), "{name}");
        let effect = &got["style"]["effects"][0];
        let want = if *name == "smooth" {
            Value::Null
        } else {
            json!(name)
        };
        assert_eq!(effect["technique"], want, "{name}: {effect}");
    }
}

// ------------------------------------------------- Photoshop's own files --

#[test]
fn photoshops_chisel_psds_import_their_technique() {
    for (file, name) in [
        ("chisel_hard_rgb8.psd", "chisel_hard"),
        ("chisel_soft_rgb8.psd", "chisel_soft"),
    ] {
        let path = fixture("bevel_chisel_578", file);
        let report = import_report(&path);
        let doc = open_psd(&path, &report, LAYERS);
        let styled: Vec<Value> = doc
            .layers
            .iter()
            .filter_map(|l| l.style.as_deref())
            .map(|s| serde_json::from_str(&s.to_json()).unwrap())
            .collect();
        assert_eq!(styled.len(), 1, "{file}: one styled layer");
        let bevel = styled[0]["effects"]
            .as_array()
            .unwrap()
            .iter()
            .find(|e| e["type"] == "bevel_emboss")
            .unwrap_or_else(|| panic!("{file}: a bevel: {}", styled[0]))
            .clone();
        // The case: A3's default bevel, the highlight white Normal 100 %.
        assert_eq!(bevel["technique"], json!(name), "{file}: {bevel}");
        assert_eq!(bevel["style"], json!("inner_bevel"), "{file}");
        assert_eq!(bevel["size"].as_f64(), Some(5.0), "{file}");
        assert_eq!(bevel["highlight_opacity"].as_f64(), Some(1.0), "{file}");
        assert_eq!(bevel["shadow_opacity"].as_f64(), Some(0.0), "{file}");
    }
}
