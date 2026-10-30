//! Gradient glows with Jitter in PSD: our 8- and 16-bit exports write
//! a Gradient glow's `Grad` (its stops) beside its `Clr `, and every glow's
//! `ShdN` as its `jitter` percent; the import reads the same keys back — a
//! Gradient glow keeps its colour, a Color glow its Jitter; a Color glow's
//! stored stops are not carried (no `Grad` without Gradient mode); and a
//! hand-encoded glow in Photoshop's own shape (`Grad`, no `Clr `, `ShdN` 40)
//! imports as a Gradient glow with Jitter 0.4. The expected items are built
//! with `psd_descriptor_fixture`, never the crate's writer.

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
use crate::psd_descriptor_fixture::{
    boolean, desc, doub, enumeration, item, long, objc, rgbc, uni, untf,
};
use crate::psd_mask_fixture as fixture8;

const LIMIT: u64 = REFERENCE_WORKING_BYTES;

fn glows() -> Value {
    json!({"effects":[
        {"type":"outer_glow","color":"#ff0000","size":6,"fill_type":"gradient","jitter":0.35,
            "gradient":{"stops":[{"position":0,"color":"#000000","opacity":1},
                {"position":0.5,"color":"#ff8000","opacity":0.5},
                {"position":1,"color":"#ffffff","opacity":1}]}},
        {"type":"inner_glow","color":"#00ff00","size":4,"jitter":0.6}]})
}

fn contains(bytes: &[u8], needle: &[u8]) -> bool {
    bytes.windows(needle.len()).any(|w| w == needle)
}

fn element(os_type: &[u8; 4], value: &[u8]) -> Vec<u8> {
    let mut out = os_type.to_vec();
    out.extend(value);
    out
}

/// One stop: its RGB, opacity % and location.
type Stop = ([f64; 3], f64, i32);

/// A `Grad` of `stops`, in the shape the writer and Photoshop share.
fn grad(stops: &[Stop]) -> Vec<u8> {
    let list = |make: &dyn Fn(&Stop) -> Vec<u8>| {
        let mut bytes = (stops.len() as u32).to_be_bytes().to_vec();
        for s in stops {
            bytes.extend(make(s));
        }
        bytes
    };
    objc(
        b"Grad",
        b"Grdn",
        &[
            item(b"Nm  ", b"TEXT", &uni("")),
            enumeration(b"GrdF", b"GrdF", b"CstS"),
            doub(b"Intr", 4096.0),
            item(
                b"Clrs",
                b"VlLs",
                &list(&|(rgb, _, at)| {
                    element(
                        b"Objc",
                        &desc(
                            b"Clrt",
                            &[
                                rgbc(b"Clr ", rgb[0], rgb[1], rgb[2]),
                                enumeration(b"Type", b"Clry", b"UsrS"),
                                long(b"Lctn", *at),
                                long(b"Mdpn", 50),
                            ],
                        ),
                    )
                }),
            ),
            item(
                b"Trns",
                b"VlLs",
                &list(&|(_, opacity, at)| {
                    element(
                        b"Objc",
                        &desc(
                            b"TrnS",
                            &[
                                untf(b"Opct", b"#Prc", *opacity),
                                long(b"Lctn", *at),
                                long(b"Mdpn", 50),
                            ],
                        ),
                    )
                }),
            ),
        ],
    )
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

fn assert_written(bytes: &[u8], what: &str) {
    let stops = [
        ([0.0, 0.0, 0.0], 100.0, 0),
        ([255.0, 128.0, 0.0], 50.0, 2048),
        ([255.0, 255.0, 255.0], 100.0, 4096),
    ];
    assert!(
        contains(bytes, &grad(&stops)),
        "{what}: the outer glow's Grad"
    );
    assert!(
        contains(bytes, &rgbc(b"Clr ", 255.0, 0.0, 0.0)),
        "{what}: the Gradient glow keeps its Clr"
    );
    assert!(
        // The writer's percent: the stored f32 widened, times 100.
        contains(bytes, &untf(b"ShdN", b"#Prc", f64::from(0.35f32) * 100.0)),
        "{what}: the outer glow's ShdN"
    );
    assert!(
        contains(bytes, &untf(b"ShdN", b"#Prc", f64::from(0.6f32) * 100.0)),
        "{what}: the Color glow's ShdN"
    );
    let grads = bytes.windows(4).filter(|w| *w == b"Grad").count();
    assert_eq!(grads, 1, "{what}: only the Gradient glow writes Grad");
}

#[test]
fn gradient_glows_cross_our_8_bit_psd_both_ways() {
    let dir = TempDir::new().unwrap();
    let doc = styled(
        &rect_layer_doc((64, 48), (16, 12, 32, 24), [0x30, 0xa0, 0x50, 255]),
        1,
        &glows().to_string(),
    );
    let path = dir.path().join("glow8.psd");
    save8(&doc, &path);
    assert_written(&std::fs::read(&path).unwrap(), "8-bit");
    let report = import_report(&path);
    assert_eq!(
        strings(&report, "verified_losses"),
        Vec::<String>::new(),
        "{report}"
    );
    let back = open_psd(&path, &report, LAYERS);
    let got = style_value(&back, back.layers.len() - 1);
    assert_eq!(got, style_value(&doc, 1));
    assert_eq!(got["effects"][0]["color"], json!("#ff0000"));
    assert_eq!(got["effects"][1]["jitter"], json!(0.6));
    assert_eq!(back.flattened().unwrap(), doc.flattened().unwrap());
}

#[test]
fn a_color_glows_stored_stops_are_not_carried_and_no_jitter_writes_shdn_0() {
    let dir = TempDir::new().unwrap();
    let doc = styled(
        &rect_layer_doc((32, 24), (8, 6, 16, 12), [0, 0, 0, 255]),
        1,
        r##"{"effects":[{"type":"outer_glow","gradient":{"stops":[
            {"position":0,"color":"#123456"},{"position":1,"color":"#abcdef"}]}}]}"##,
    );
    let path = dir.path().join("plain.psd");
    save8(&doc, &path);
    let bytes = std::fs::read(&path).unwrap();
    assert!(!contains(&bytes, b"Grad"), "no Grad without Gradient mode");
    assert!(contains(&bytes, &untf(b"ShdN", b"#Prc", 0.0)));
    let back = open_psd(&path, &import_report(&path), LAYERS);
    let e = &style_value(&back, back.layers.len() - 1)["effects"][0];
    for key in ["fill_type", "gradient", "jitter"] {
        assert!(e.get(key).is_none(), "{key} read back: {e}");
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
fn gradient_glows_cross_our_16_bit_psd_both_ways() {
    let dir = TempDir::new().unwrap();
    let typed = typed_doc(&glows());
    let path = dir.path().join("glow16.psd");
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
fn photoshops_own_gradient_glow_imports_with_its_jitter() {
    let dir = TempDir::new().unwrap();
    let items = vec![
        boolean(b"masterFXSwitch", 1),
        objc(
            b"OrGl",
            b"OrGl",
            &[
                boolean(b"enab", 1),
                untf(b"blur", b"#Pxl", 9.0),
                untf(b"ShdN", b"#Prc", 40.0),
                grad(&[
                    ([0.0, 0.0, 0.0], 100.0, 0),
                    ([255.0, 255.0, 255.0], 50.0, 4096),
                ]),
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
    let path = dir.path().join("photoshop.psd");
    std::fs::write(&path, fixture8::fixture(&[record], [2, 1], false)).unwrap();
    let report = import_report(&path);
    assert_eq!(
        strings(&report, "verified_losses"),
        Vec::<String>::new(),
        "{report}"
    );
    let doc = open_psd(&path, &report, LAYERS);
    let e = style_value(&doc, 0)["effects"][0].clone();
    assert_eq!(e["type"], json!("outer_glow"));
    assert_eq!(e["fill_type"], json!("gradient"));
    assert_eq!(e["jitter"], json!(0.4));
    assert_eq!(e["color"], json!("#ffffbe"), "no Clr: the default colour");
    assert_eq!(
        e["gradient"]["stops"],
        json!([{"position":0.0,"color":"#000000","opacity":1.0},
            {"position":1.0,"color":"#ffffff","opacity":0.5}])
    );
}
