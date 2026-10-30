//! Photoshop's layer effects (`lfx2`), fill opacity (`iOpa`) and blending
//! ranges, black-box through the C API on both depths: an authored record
//! carries them into one Doctored layer style, the bodies Doctored cannot
//! render are declined with their reason, and a styled document writes the
//! three constructs back as the exact bytes hand-encoded here.
//!
//! Every PSD read here is an independent byte construction
//! (`common/psd_mask_fixture.rs` at 8 bits, `common/psd16_fixture.rs` at 16)
//! and every expected descriptor body comes from this suite's own builder
//! (`common/psd_descriptor_fixture.rs`), never from the crate's writer.

use std::ffi::{c_char, CString};
use std::path::Path;
use std::ptr;
use std::sync::Arc;

use doctored_core::doc::DrDocument;
use doctored_core::ffi_psd::{
    dr_doc_open_psd, dr_doc_psd_export_report, dr_doc_save_psd, dr_precision_doc_open_psd,
    dr_precision_doc_psd_export_report, dr_precision_doc_save_psd, dr_psd_import_report,
};
use doctored_core::icc::IccProfile;
use doctored_core::job_control::DrJobControl;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer as PLayer};
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use doctored_core::style::LayerStyle;
use image::{GrayImage, Rgba, RgbaImage};
use serde_json::{json, Value};
use tempfile::TempDir;

use crate::common;
use crate::psd16_fixture as fixture16;
use crate::psd_descriptor_fixture as builder;
use crate::psd_mask_fixture as fixture8;
use builder::{boolean, desc, doub, enumeration, item, long, objc, rgbc, untf};
use common::{cpath, take_err_string};

const LAYERS: i32 = 0;
const LIMIT: u64 = REFERENCE_WORKING_BYTES;
const CANVAS: [u32; 2] = [2, 1];
const RGBA: [u8; 8] = [11, 29, 47, 255, 101, 151, 199, 255];

// ------------------------------------------------------------- C plumbing --

pub(crate) fn import_report(path: &Path) -> Value {
    let mut error = ptr::null_mut();
    let raw = unsafe { dr_psd_import_report(cpath(path).as_ptr(), &mut error) };
    assert!(!raw.is_null(), "{}", take_err_string(error));
    serde_json::from_str(&take_err_string(raw)).expect("import report JSON")
}

pub(crate) fn open_psd(path: &Path, report: &Value) -> Box<DrDocument> {
    let token = CString::new(report["source_fingerprint"].as_str().unwrap()).unwrap();
    let mut error = ptr::null_mut();
    let raw = unsafe { dr_doc_open_psd(cpath(path).as_ptr(), LAYERS, token.as_ptr(), &mut error) };
    assert!(!raw.is_null(), "open_psd: {}", take_err_string(error));
    unsafe { Box::from_raw(raw) }
}

pub(crate) struct Owned(*mut PrecisionDocument);
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

pub(crate) fn open_typed(path: &Path, report: &Value) -> Owned {
    let token = CString::new(report["source_fingerprint"].as_str().unwrap()).unwrap();
    let mut error = ptr::null_mut();
    let raw = unsafe {
        dr_precision_doc_open_psd(
            cpath(path).as_ptr(),
            LAYERS,
            token.as_ptr(),
            LIMIT,
            0,
            ptr::null::<DrJobControl>(),
            &mut error,
        )
    };
    assert!(!raw.is_null(), "typed open: {}", take_err_string(error));
    Owned(raw)
}

fn export_report(doc: &DrDocument) -> Value {
    let mut error: *mut c_char = ptr::null_mut();
    let json = unsafe { dr_doc_psd_export_report(doc, 0, LAYERS, &mut error) };
    assert!(!json.is_null(), "export report: {}", take_err_string(error));
    serde_json::from_str(&take_err_string(json)).expect("export report JSON")
}

fn export_report16(doc: &PrecisionDocument) -> Value {
    let mut error: *mut c_char = ptr::null_mut();
    let json = unsafe { dr_precision_doc_psd_export_report(doc, 0, LAYERS, &mut error) };
    assert!(!json.is_null(), "export report: {}", take_err_string(error));
    serde_json::from_str(&take_err_string(json)).expect("export report JSON")
}

pub(crate) fn save(doc: &DrDocument, path: &Path) -> Result<(), String> {
    let c = cpath(path);
    let mut error: *mut c_char = ptr::null_mut();
    if unsafe { dr_doc_save_psd(doc, ptr::null(), c.as_ptr(), 0, LAYERS, &mut error) } {
        Ok(())
    } else {
        Err(take_err_string(error))
    }
}

pub(crate) fn save16(
    doc: &PrecisionDocument,
    projection: &PrecisionImage,
    path: &Path,
) -> Result<(), String> {
    let mut error: *mut c_char = ptr::null_mut();
    if unsafe {
        dr_precision_doc_save_psd(
            doc,
            projection,
            cpath(path).as_ptr(),
            0,
            LAYERS,
            LIMIT,
            0,
            &mut error,
        )
    } {
        Ok(())
    } else {
        Err(take_err_string(error))
    }
}

fn strings(value: &Value, key: &str) -> Vec<String> {
    value[key]
        .as_array()
        .unwrap_or_else(|| panic!("{key} array"))
        .iter()
        .map(|v| v.as_str().expect("string").to_string())
        .collect()
}

fn write(dir: &TempDir, name: &str, bytes: &[u8]) -> std::path::PathBuf {
    let path = dir.path().join(name);
    std::fs::write(&path, bytes).unwrap();
    if let Ok(directory) = std::env::var("DR_PSD_EFFECT_FIXTURES") {
        let directory = Path::new(&directory);
        std::fs::create_dir_all(directory).unwrap();
        std::fs::write(directory.join(name), bytes).unwrap();
    }
    path
}

// ------------------------------------------------------- fixture plumbing --

/// One image resource: signature, id, an empty Pascal name, an even body.
pub(crate) fn resource(id: u16, body: &[u8]) -> Vec<u8> {
    let mut out = b"8BIM".to_vec();
    out.extend(id.to_be_bytes());
    out.extend([0, 0]);
    out.extend((body.len() as u32).to_be_bytes());
    out.extend(body);
    if !body.len().is_multiple_of(2) {
        out.push(0);
    }
    out
}

/// `extra` appended to the file's image-resource section, whose length field
/// sits at offset 30, immediately after the empty colour-mode data.
pub(crate) fn with_resources(mut bytes: Vec<u8>, extra: &[u8]) -> Vec<u8> {
    assert_eq!(
        &bytes[26..30],
        &[0; 4],
        "the fixture writes no colour-mode data"
    );
    let len = u32::from_be_bytes(bytes[30..34].try_into().unwrap()) as usize;
    bytes.splice(34 + len..34 + len, extra.iter().copied());
    bytes[30..34].copy_from_slice(&((len + extra.len()) as u32).to_be_bytes());
    bytes
}

/// An `lfx2` block body: the leading zero word, then a version-16 descriptor
/// of class `null`.
pub(crate) fn lfx2(items: &[Vec<u8>]) -> Vec<u8> {
    let mut out = 0u32.to_be_bytes().to_vec();
    out.extend(16u32.to_be_bytes());
    out.extend(desc(b"null", items));
    if !out.len().is_multiple_of(2) {
        out.push(0);
    }
    out
}

/// One VlLs element: the OSType and the value, with no key.
fn element(os_type: &[u8; 4], value: &[u8]) -> Vec<u8> {
    let mut out = os_type.to_vec();
    out.extend(value);
    out
}

/// One `Grdn` gradient: two stops, the second at a nondefault opacity.
fn grdn(stops: usize, transparency: usize) -> Vec<u8> {
    let colour = |index: usize| {
        let level = if index == 0 { 0.0 } else { 255.0 };
        element(
            b"Objc",
            &desc(
                b"Clrt",
                &[
                    rgbc(b"Clr ", level, level, level),
                    enumeration(b"Type", b"Clry", b"UsrS"),
                    long(b"Lctn", if index == 0 { 0 } else { 4096 }),
                    long(b"Mdpn", 50),
                ],
            ),
        )
    };
    let opacity = |index: usize| {
        element(
            b"Objc",
            &desc(
                b"TrnS",
                &[
                    untf(b"Opct", b"#Prc", if index == 0 { 100.0 } else { 50.0 }),
                    long(b"Lctn", if index == 0 { 0 } else { 4096 }),
                    long(b"Mdpn", 50),
                ],
            ),
        )
    };
    let list = |count: usize, make: &dyn Fn(usize) -> Vec<u8>| {
        let mut bytes = (count as u32).to_be_bytes().to_vec();
        for index in 0..count {
            bytes.extend(make(index));
        }
        bytes
    };
    objc(
        b"Grad",
        b"Grdn",
        &[
            item(b"Nm  ", b"TEXT", &builder::uni("")),
            enumeration(b"GrdF", b"GrdF", b"CstS"),
            doub(b"Intr", 4096.0),
            item(b"Clrs", b"VlLs", &list(stops, &colour)),
            item(b"Trns", b"VlLs", &list(transparency, &opacity)),
        ],
    )
}

fn raster(blocks: Vec<u8>, ranges: Vec<u8>) -> fixture8::Record {
    let mut record = fixture8::Record::raster([0, 0, 1, 2], &RGBA, false);
    record.name = "Styled";
    record.blocks = blocks;
    record.ranges = ranges;
    record
}

pub(crate) fn file8(blocks: Vec<u8>, ranges: Vec<u8>) -> Vec<u8> {
    fixture8::fixture(&[raster(blocks, ranges)], CANVAS, false)
}

/// The colour `rgb` carried from `from`'s space into `to`'s, through the same
/// document conversion a host would run — the independent oracle for the
/// effect-colour conversion.
fn converted(rgb: [u8; 3], from: &Arc<IccProfile>, to: &Arc<IccProfile>) -> [u8; 3] {
    let mut pixels = RgbaImage::new(1, 1);
    pixels.put_pixel(0, 0, Rgba([rgb[0], rgb[1], rgb[2], 255]));
    let mut document = DrDocument::from_pixels(pixels);
    document.profile = Arc::clone(from);
    let out = document
        .convert_to_profile(to)
        .expect("the conversion succeeds")
        .expect("a conversion between two different profiles");
    let pixel_image = out.layers[0].pixels.contiguous_image();
    let pixel = pixel_image.get_pixel(0, 0);
    [pixel[0], pixel[1], pixel[2]]
}

// ------------------------------------------------------------- the import --

/// Every one of the nine effect classes, with nondefault values, one disabled
/// instance, a gradient stroke and a radial gradient overlay. `Scl ` 50 halves
/// every pixel quantity on the way in.
fn authored_items() -> Vec<Vec<u8>> {
    vec![
        untf(b"Scl ", b"#Prc", 50.0),
        boolean(b"masterFXSwitch", 1),
        objc(
            b"DrSh",
            b"DrSh",
            &[
                boolean(b"enab", 1),
                enumeration(b"Md  ", b"BlnM", b"Mltp"),
                rgbc(b"Clr ", 10.0, 20.0, 30.0),
                untf(b"Opct", b"#Prc", 60.0),
                boolean(b"uglg", 1),
                untf(b"lagl", b"#Ang", 45.0),
                untf(b"Dstn", b"#Pxl", 8.0),
                untf(b"Ckmt", b"#Prc", 20.0),
                untf(b"blur", b"#Pxl", 12.0),
                boolean(b"layerConceals", 0),
            ],
        ),
        objc(
            b"IrSh",
            b"IrSh",
            &[
                boolean(b"enab", 1),
                enumeration(b"Md  ", b"BlnM", b"Scrn"),
                rgbc(b"Clr ", 40.0, 50.0, 60.0),
                untf(b"Opct", b"#Prc", 70.0),
                boolean(b"uglg", 0),
                untf(b"lagl", b"#Ang", 30.0),
                untf(b"Dstn", b"#Pxl", 6.0),
                untf(b"Ckmt", b"#Prc", 10.0),
                untf(b"blur", b"#Pxl", 8.0),
            ],
        ),
        objc(
            b"OrGl",
            b"OrGl",
            &[
                boolean(b"enab", 1),
                enumeration(b"Md  ", b"BlnM", b"Scrn"),
                rgbc(b"Clr ", 255.0, 255.0, 190.0),
                untf(b"Opct", b"#Prc", 55.0),
                untf(b"Ckmt", b"#Prc", 30.0),
                untf(b"blur", b"#Pxl", 20.0),
            ],
        ),
        objc(
            b"IrGl",
            b"IrGl",
            &[
                boolean(b"enab", 0),
                enumeration(b"Md  ", b"BlnM", b"Mltp"),
                rgbc(b"Clr ", 1.0, 2.0, 3.0),
                untf(b"Opct", b"#Prc", 45.0),
                untf(b"Ckmt", b"#Prc", 5.0),
                untf(b"blur", b"#Pxl", 14.0),
                enumeration(b"glwS", b"IGSr", b"SrcC"),
            ],
        ),
        objc(
            b"ebbl",
            b"ebbl",
            &[
                boolean(b"enab", 1),
                enumeration(b"hglM", b"BlnM", b"Scrn"),
                rgbc(b"hglC", 200.0, 201.0, 202.0),
                untf(b"hglO", b"#Prc", 80.0),
                enumeration(b"sdwM", b"BlnM", b"Mltp"),
                rgbc(b"sdwC", 9.0, 8.0, 7.0),
                untf(b"sdwO", b"#Prc", 65.0),
                enumeration(b"bvlS", b"BESl", b"Embs"),
                enumeration(b"bvlD", b"BESs", b"Out "),
                boolean(b"uglg", 0),
                untf(b"lagl", b"#Ang", 100.0),
                untf(b"Lald", b"#Ang", 40.0),
                untf(b"srgR", b"#Prc", 150.0),
                untf(b"blur", b"#Pxl", 10.0),
                untf(b"Sftn", b"#Pxl", 4.0),
            ],
        ),
        objc(
            b"ChFX",
            b"ChFX",
            &[
                boolean(b"enab", 1),
                enumeration(b"Md  ", b"BlnM", b"Mltp"),
                rgbc(b"Clr ", 70.0, 80.0, 90.0),
                untf(b"Opct", b"#Prc", 35.0),
                untf(b"lagl", b"#Ang", 19.0),
                untf(b"Dstn", b"#Pxl", 22.0),
                untf(b"blur", b"#Pxl", 16.0),
                boolean(b"Invr", 0),
            ],
        ),
        objc(
            b"SoFi",
            b"SoFi",
            &[
                boolean(b"enab", 1),
                enumeration(b"Md  ", b"BlnM", b"Scrn"),
                rgbc(b"Clr ", 255.0, 0.0, 0.0),
                untf(b"Opct", b"#Prc", 90.0),
            ],
        ),
        objc(
            b"GrFl",
            b"GrFl",
            &[
                boolean(b"enab", 1),
                enumeration(b"Md  ", b"BlnM", b"Mltp"),
                untf(b"Opct", b"#Prc", 85.0),
                grdn(2, 2),
                untf(b"Angl", b"#Ang", 45.0),
                enumeration(b"Type", b"GrdT", b"Rdl "),
                boolean(b"Rvrs", 1),
                boolean(b"Algn", 0),
                untf(b"Scl ", b"#Prc", 120.0),
            ],
        ),
        objc(
            b"FrFX",
            b"FrFX",
            &[
                boolean(b"enab", 1),
                enumeration(b"Styl", b"FStl", b"InsF"),
                enumeration(b"PntT", b"FrFl", b"GrFl"),
                enumeration(b"Md  ", b"BlnM", b"Scrn"),
                untf(b"Opct", b"#Prc", 75.0),
                untf(b"Sz  ", b"#Pxl", 6.0),
                grdn(2, 2),
                untf(b"Angl", b"#Ang", 90.0),
                enumeration(b"Type", b"GrdT", b"Lnr "),
                boolean(b"Rvrs", 0),
                boolean(b"Algn", 1),
                untf(b"Scl ", b"#Prc", 100.0),
            ],
        ),
    ]
}

/// The style those nine effects describe, written out independently: every
/// pixel quantity already halved by `Scl ` 50, `iOpa` 128 as the fill opacity
/// and the red blending-range pair as Blend If.
const AUTHORED_JSON: &str = r##"{
  "fill_opacity": 0.502,
  "blend_if": {"channel":"red","this_layer":[10,20,200,255],"underlying":[0,5,250,255]},
  "effects": [
    {"type":"drop_shadow","blend":"multiply","color":"#0a141e","opacity":0.6,"angle":45,
     "use_global_light":true,"distance":4,"spread":0.2,"size":6,"layer_knocks_out":false},
    {"type":"inner_shadow","blend":"screen","color":"#28323c","opacity":0.7,"angle":30,
     "use_global_light":false,"distance":3,"choke":0.1,"size":4},
    {"type":"outer_glow","blend":"screen","color":"#ffffbe","opacity":0.55,"spread":0.3,"size":10},
    {"type":"inner_glow","enabled":false,"blend":"multiply","color":"#010203","opacity":0.45,
     "choke":0.05,"size":7,"source":"center"},
    {"type":"bevel_emboss","style":"emboss","depth":1.5,"direction":"down","size":5,"soften":2,
     "angle":100,"use_global_light":false,"altitude":40,"highlight_blend":"screen",
     "highlight_color":"#c8c9ca","highlight_opacity":0.8,"shadow_blend":"multiply",
     "shadow_color":"#090807","shadow_opacity":0.65},
    {"type":"satin","blend":"multiply","color":"#46505a","opacity":0.35,"angle":19,
     "distance":11,"size":8,"invert":false},
    {"type":"color_overlay","blend":"screen","color":"#ff0000","opacity":0.9},
    {"type":"gradient_overlay","blend":"multiply","opacity":0.85,
     "gradient":{"stops":[{"position":0,"color":"#000000","opacity":1},
                          {"position":1,"color":"#ffffff","opacity":0.5}],
                 "style":"radial","angle":45,"scale":1.2,"reverse":true,"align_with_layer":false}},
    {"type":"stroke","blend":"screen","opacity":0.75,"size":3,"position":"inside",
     "fill_type":"gradient","color":"#000000",
     "gradient":{"stops":[{"position":0,"color":"#000000","opacity":1},
                          {"position":1,"color":"#ffffff","opacity":0.5}],
                 "style":"linear","angle":90,"scale":1,"reverse":false,"align_with_layer":true}}
  ]
}"##;

/// The 16-byte ranges section: the unrestricted gray pair, then a restricted
/// red one.
fn red_ranges() -> Vec<u8> {
    [
        0, 0, 255, 255, 0, 0, 255, 255, 10, 20, 200, 255, 0, 5, 250, 255,
    ]
    .to_vec()
}

fn authored_file() -> Vec<u8> {
    let mut blocks = fixture8::block(b"lfx2", &lfx2(&authored_items()));
    blocks.extend(fixture8::block(b"iOpa", &[128, 0, 0, 0]));
    with_resources(
        file8(blocks, red_ranges()),
        &resource(1037, &45i32.to_be_bytes()),
    )
}

#[test]
fn an_authored_record_carries_its_effects_fill_opacity_and_ranges() {
    let dir = TempDir::new().unwrap();
    let path = write(&dir, "authored.psd", &authored_file());
    let report = import_report(&path);
    assert_eq!(report["layered_available"], Value::Bool(true), "{report}");

    let losses = strings(&report, "verified_losses");
    assert!(losses.is_empty(), "nothing is discarded: {losses:?}");
    let observation = strings(&report, "observations")
        .into_iter()
        .find(|o| o.contains("has Photoshop layer effects"))
        .expect("the carry observation");
    assert!(
        observation.contains("(drop_shadow, outer_glow, stroke, gradient_overlay, color_overlay, satin, inner_glow, inner_shadow, bevel_emboss, fill opacity 50 % and Blend If ranges on red)"),
        "{observation}"
    );
    assert!(
        observation.contains("settings it does not model (the glows' technique) are ignored"),
        "{observation}"
    );

    let document = open_psd(&path, &report);
    let style = document.layers[0]
        .style
        .as_deref()
        .expect("the carried style");
    assert_eq!(
        style.to_json(),
        LayerStyle::from_json(AUTHORED_JSON)
            .expect("the expected style")
            .to_json()
    );
    assert_eq!(document.global_light.angle, 45.0);
    assert_eq!(document.global_light.altitude, 30.0);
}

#[test]
fn bodies_rasterize_cannot_render_are_declined_with_their_reason() {
    let dir = TempDir::new().unwrap();
    let stroke = |fill: &[u8]| {
        vec![
            boolean(b"masterFXSwitch", 1),
            objc(
                b"FrFX",
                b"FrFX",
                &[
                    boolean(b"enab", 1),
                    enumeration(b"PntT", b"FrFl", fill),
                    untf(b"Sz  ", b"#Pxl", 4.0),
                ],
            ),
        ]
    };
    let overlay = |stops: usize, transparency: usize| {
        vec![
            boolean(b"masterFXSwitch", 1),
            objc(
                b"GrFl",
                b"GrFl",
                &[boolean(b"enab", 1), grdn(stops, transparency)],
            ),
        ]
    };
    // name, the `lfx2` items, the reason, any extra block beside it.
    type Case = (&'static str, Vec<Vec<u8>>, &'static str, Vec<u8>);
    let cases: Vec<Case> = vec![
        (
            "pattern-stroke",
            stroke(b"Ptrn"),
            "stroke names a pattern the file does not carry",
            Vec::new(),
        ),
        (
            "stroke-emboss",
            vec![
                boolean(b"masterFXSwitch", 1),
                objc(
                    b"ebbl",
                    b"ebbl",
                    &[
                        boolean(b"enab", 1),
                        enumeration(b"bvlS", b"BESl", b"strokeEmboss"),
                    ],
                ),
            ],
            "bevel uses Stroke Emboss",
            Vec::new(),
        ),
        (
            "unmatched-transparency",
            overlay(2, 1),
            "gradient transparency stops do not match its colour stops",
            Vec::new(),
        ),
        (
            "one-stop",
            overlay(1, 1),
            "gradient does not have between 2 and 32 colour stops",
            Vec::new(),
        ),
        (
            "switched-off",
            vec![
                boolean(b"masterFXSwitch", 0),
                objc(b"SoFi", b"SoFi", &[boolean(b"enab", 1)]),
            ],
            "layer effects are switched off",
            Vec::new(),
        ),
        (
            "wrong-type",
            vec![
                boolean(b"masterFXSwitch", 1),
                objc(b"SoFi", b"SoFi", &[boolean(b"enab", 1), long(b"Md  ", 0)]),
            ],
            "descriptor key Md has an unexpected type",
            Vec::new(),
        ),
    ];
    for (name, items, reason, extra) in cases {
        let mut blocks = fixture8::block(b"lfx2", &lfx2(&items));
        blocks.extend(extra);
        let path = write(&dir, &format!("{name}.psd"), &file8(blocks, Vec::new()));
        let report = import_report(&path);
        assert_eq!(report["layered_available"], Value::Bool(true), "{name}");
        let losses = strings(&report, "verified_losses");
        assert!(
            losses.iter().any(|l| l.contains(reason)
                && l.contains("layered import discards them without applying them")),
            "{name}: {losses:?}"
        );
        assert!(
            losses
                .iter()
                .any(|l| l.contains("has layer-style metadata")),
            "{name} keeps its marker sentence: {losses:?}"
        );
        let document = open_psd(&path, &report);
        assert!(document.layers[0].style.is_none(), "{name}");
    }
}

#[test]
fn a_body_that_is_not_an_effects_descriptor_keeps_its_marker_sentence_alone() {
    let dir = TempDir::new().unwrap();
    // An additional layer block must have an even length, exactly as the
    // writer pads it.
    let framed = |version: u32, class: &[u8], items: &[Vec<u8>]| {
        let mut out = 0u32.to_be_bytes().to_vec();
        out.extend(version.to_be_bytes());
        out.extend(desc(class, items));
        if !out.len().is_multiple_of(2) {
            out.push(0);
        }
        out
    };
    let v15 = framed(15, b"null", &[boolean(b"masterFXSwitch", 1)]);
    let wrong_class = framed(16, b"DrSh", &[boolean(b"enab", 1)]);
    let mut leading = framed(16, b"null", &[boolean(b"masterFXSwitch", 1)]);
    leading[..4].copy_from_slice(&1u32.to_be_bytes());
    for (name, body) in [
        ("opaque", vec![255; 4]),
        ("v15", v15),
        ("top-level-class", wrong_class),
        ("leading-word", leading),
    ] {
        let path = write(
            &dir,
            &format!("marker-{name}.psd"),
            &file8(fixture8::block(b"lfx2", &body), Vec::new()),
        );
        let report = import_report(&path);
        let losses = strings(&report, "verified_losses");
        assert_eq!(losses.len(), 1, "{name}: {losses:?}");
        assert!(losses[0].contains("has layer-style metadata"), "{name}");
        assert!(open_psd(&path, &report).layers[0].style.is_none(), "{name}");
    }
}

#[test]
fn ranges_and_fill_opacity_outside_the_carried_shape_keep_their_sentences() {
    let dir = TempDir::new().unwrap();
    let default = [0u8, 0, 255, 255, 0, 0, 255, 255];
    let restricted = [10u8, 20, 200, 255, 0, 5, 250, 255];
    // Two restricted channels are CARRIED: one range
    // each, Gray the primary and Red its extra.
    let path = write(
        &dir,
        "ranges-two-channels.psd",
        &file8(Vec::new(), [restricted, restricted].concat()),
    );
    let report = import_report(&path);
    let losses = strings(&report, "verified_losses");
    assert!(
        !losses.iter().any(|l| l.contains("Blend If")),
        "two-channels: {losses:?}"
    );
    let style = open_psd(&path, &report).layers[0]
        .style
        .clone()
        .expect("two-channels carries a style");
    let style: Value = serde_json::from_str(&style.to_json()).unwrap();
    assert_eq!(
        style["blend_if"],
        serde_json::json!({"channel": "gray", "this_layer": [10, 20, 200, 255],
            "underlying": [0, 5, 250, 255],
            "extra": [{"channel": "red", "this_layer": [10, 20, 200, 255],
                "underlying": [0, 5, 250, 255]}]})
    );
    for (name, ranges) in [
        (
            "alpha",
            [default, default, default, default, restricted].concat(),
        ),
        ("short", restricted[..4].to_vec()),
    ] {
        let path = write(
            &dir,
            &format!("ranges-{name}.psd"),
            &file8(Vec::new(), ranges),
        );
        let report = import_report(&path);
        let losses = strings(&report, "verified_losses");
        assert!(
            losses
                .iter()
                .any(|l| l.contains("nondefault or unrecognized Blend If")),
            "{name}: {losses:?}"
        );
        assert!(open_psd(&path, &report).layers[0].style.is_none(), "{name}");
    }
    let path = write(
        &dir,
        "fill-opaque.psd",
        &file8(fixture8::block(b"iOpa", &[127; 4]), Vec::new()),
    );
    let report = import_report(&path);
    assert!(
        strings(&report, "observations")
            .iter()
            .any(|o| o.contains("payload is not interpreted or retained")),
        "{:?}",
        strings(&report, "observations")
    );
    assert!(open_psd(&path, &report).layers[0].style.is_none());
}

#[test]
fn a_display_p3_file_converts_its_effect_colours_back_to_srgb() {
    let dir = TempDir::new().unwrap();
    let authored = [200u8, 90, 40];
    let items = vec![
        boolean(b"masterFXSwitch", 1),
        objc(
            b"SoFi",
            b"SoFi",
            &[
                boolean(b"enab", 1),
                rgbc(
                    b"Clr ",
                    f64::from(authored[0]),
                    f64::from(authored[1]),
                    f64::from(authored[2]),
                ),
            ],
        ),
    ];
    let p3 = IccProfile::display_p3();
    let bytes = with_resources(
        file8(fixture8::block(b"lfx2", &lfx2(&items)), Vec::new()),
        &resource(1039, p3.bytes()),
    );
    let path = write(&dir, "p3-effects.psd", &bytes);
    let report = import_report(&path);
    let document = open_psd(&path, &report);
    let style = document.layers[0].style.as_deref().expect("the style");
    let expected = converted(authored, &p3, &IccProfile::srgb());
    let json: Value = serde_json::from_str(&style.to_json()).unwrap();
    assert_eq!(
        json["effects"][0]["color"],
        json!(format!(
            "#{:02x}{:02x}{:02x}",
            expected[0], expected[1], expected[2]
        )),
        "the authored P3 numbers convert back to sRGB"
    );
}

// ------------------------------------------------------------- the export --

/// The style the export cases carry: all nine effects with nondefault values,
/// one disabled, a gradient stroke, a gradient overlay, half-ish fill opacity
/// (exactly 153/255, the `iOpa` byte) and a gray Blend If.
const EXPORT_JSON: &str = r##"{
  "fill_opacity": 0.6,
  "blend_if": {"channel":"gray","this_layer":[0,10,240,255],"underlying":[5,15,235,250]},
  "effects": [
    {"type":"drop_shadow","blend":"multiply","color":"#112233","opacity":0.5,"angle":45,
     "use_global_light":false,"distance":6,"spread":0.25,"size":9,"layer_knocks_out":false},
    {"type":"outer_glow","blend":"screen","color":"#ffee00","opacity":0.4,"spread":0.1,"size":11},
    {"type":"stroke","blend":"normal","opacity":0.9,"size":4,"position":"center",
     "fill_type":"gradient","color":"#000000",
     "gradient":{"stops":[{"position":0,"color":"#ff0000","opacity":1},
                          {"position":1,"color":"#0000ff","opacity":0.25}],
                 "style":"reflected","angle":30,"scale":1.4,"reverse":true,
                 "align_with_layer":false}},
    {"type":"gradient_overlay","blend":"overlay","opacity":0.8,
     "gradient":{"stops":[{"position":0,"color":"#102030","opacity":0.75},
                          {"position":0.5,"color":"#405060","opacity":0.5}],
                 "style":"angle","angle":15,"scale":0.5,"reverse":false,
                 "align_with_layer":true}},
    {"type":"color_overlay","blend":"soft_light","color":"#00ff88","opacity":0.7},
    {"type":"satin","blend":"multiply","color":"#334455","opacity":0.45,"angle":25,
     "distance":12,"size":18,"invert":false},
    {"type":"inner_glow","blend":"screen","color":"#ccddee","opacity":0.35,"choke":0.2,
     "size":7,"source":"center"},
    {"type":"inner_shadow","enabled":false,"blend":"linear_burn","color":"#223344",
     "opacity":0.65,"angle":80,"use_global_light":true,"distance":3,"choke":0.15,"size":5},
    {"type":"bevel_emboss","style":"pillow_emboss","depth":2.5,"direction":"down","size":8,
     "soften":3,"angle":95,"use_global_light":true,"altitude":55,"highlight_blend":"lighten",
     "highlight_color":"#fafbfc","highlight_opacity":0.55,"shadow_blend":"darken",
     "shadow_color":"#030201","shadow_opacity":0.85}
  ]
}"##;

const LIGHT: (f32, f32) = (137.0, 52.0);

/// The style as it comes BACK: an effect that reads the document's global
/// light carries only the RESOLVED angle, so its own angle field returns as
/// that light's. Every other field round-trips exactly.
fn resolved(style: &str, light: (f32, f32)) -> String {
    let mut value: Value = serde_json::from_str(style).expect("style JSON");
    for effect in value["effects"].as_array_mut().expect("effects") {
        if effect["use_global_light"] != Value::Bool(true) {
            continue;
        }
        effect["angle"] = json!(light.0);
        if effect["altitude"].is_number() {
            effect["altitude"] = json!(light.1);
        }
    }
    LayerStyle::from_json(&value.to_string())
        .expect("the resolved style")
        .to_json()
}

/// A percentage as the writer computes it: the stored f32 widened, then x100.
fn pct(value: f32) -> f64 {
    f64::from(value) * 100.0
}

fn px(value: f32) -> f64 {
    f64::from(value)
}

/// The linear `ShpC` contour every effect implies.
fn contour() -> Vec<Vec<u8>> {
    let point = |value: f64| {
        element(
            b"Objc",
            &desc(b"CrPt", &[doub(b"Hrzn", value), doub(b"Vrtc", value)]),
        )
    };
    let mut list = 2u32.to_be_bytes().to_vec();
    list.extend(point(0.0));
    list.extend(point(255.0));
    vec![
        item(b"Nm  ", b"TEXT", &builder::uni("Linear")),
        item(b"Crv ", b"VlLs", &list),
    ]
}

fn shpc(key: &[u8]) -> Vec<u8> {
    objc(key, b"ShpC", &contour())
}

fn offset_zero() -> Vec<u8> {
    objc(
        b"Ofst",
        b"Pnt ",
        &[untf(b"Hrzn", b"#Prc", 0.0), untf(b"Vrtc", b"#Prc", 0.0)],
    )
}

/// The written gradient keys: the `Grdn` object and the five placement keys.
fn written_gradient(
    stops: &[(f32, [f64; 3], f32)],
    style: &[u8],
    angle: f32,
    scale: f32,
    reverse: u8,
    align: u8,
) -> Vec<Vec<u8>> {
    let location = |position: f32| (f64::from(position) * 4096.0).round() as i32;
    let mut colours = (stops.len() as u32).to_be_bytes().to_vec();
    let mut transparency = (stops.len() as u32).to_be_bytes().to_vec();
    for (position, colour, opacity) in stops {
        colours.extend(element(
            b"Objc",
            &desc(
                b"Clrt",
                &[
                    rgbc(b"Clr ", colour[0], colour[1], colour[2]),
                    enumeration(b"Type", b"Clry", b"UsrS"),
                    long(b"Lctn", location(*position)),
                    long(b"Mdpn", 50),
                ],
            ),
        ));
        transparency.extend(element(
            b"Objc",
            &desc(
                b"TrnS",
                &[
                    untf(b"Opct", b"#Prc", pct(*opacity)),
                    long(b"Lctn", location(*position)),
                    long(b"Mdpn", 50),
                ],
            ),
        ));
    }
    vec![
        objc(
            b"Grad",
            b"Grdn",
            &[
                item(b"Nm  ", b"TEXT", &builder::uni("")),
                enumeration(b"GrdF", b"GrdF", b"CstS"),
                doub(b"Intr", 4096.0),
                item(b"Clrs", b"VlLs", &colours),
                item(b"Trns", b"VlLs", &transparency),
            ],
        ),
        untf(b"Angl", b"#Ang", px(angle)),
        enumeration(b"Type", b"GrdT", style),
        boolean(b"Rvrs", reverse),
        boolean(b"Algn", align),
        untf(b"Scl ", b"#Prc", pct(scale)),
        boolean(b"Dthr", 0),
        offset_zero(),
    ]
}

/// The exact `lfx2` body [`EXPORT_JSON`] must produce under [`LIGHT`], with
/// every key, every written-only default and every value in writing order.
fn expected_lfx2(light: (f32, f32)) -> Vec<u8> {
    let mut drop_shadow = vec![
        boolean(b"enab", 1),
        enumeration(b"Md  ", b"BlnM", b"Mltp"),
        rgbc(b"Clr ", 0x11 as f64, 0x22 as f64, 0x33 as f64),
        untf(b"Opct", b"#Prc", pct(0.5)),
        boolean(b"uglg", 0),
        untf(b"lagl", b"#Ang", px(45.0)),
        untf(b"Dstn", b"#Pxl", px(6.0)),
        untf(b"Ckmt", b"#Prc", pct(0.25)),
        untf(b"blur", b"#Pxl", px(9.0)),
        untf(b"Nose", b"#Prc", 0.0),
        boolean(b"AntA", 1),
        boolean(b"layerConceals", 0),
        shpc(b"TrnS"),
    ];
    drop_shadow.extend([boolean(b"present", 1), boolean(b"showInDialog", 1)]);
    let mut outer_glow = vec![
        boolean(b"enab", 1),
        enumeration(b"Md  ", b"BlnM", b"Scrn"),
        rgbc(b"Clr ", 0xff as f64, 0xee as f64, 0.0),
        untf(b"Opct", b"#Prc", pct(0.4)),
        untf(b"Ckmt", b"#Prc", pct(0.1)),
        untf(b"blur", b"#Pxl", px(11.0)),
        untf(b"Nose", b"#Prc", 0.0),
        untf(b"ShdN", b"#Prc", 0.0),
        boolean(b"AntA", 1),
        enumeration(b"GlwT", b"BETE", b"SfBL"),
        untf(b"Inpr", b"#Prc", 50.0),
        shpc(b"TrnS"),
    ];
    outer_glow.extend([boolean(b"present", 1), boolean(b"showInDialog", 1)]);
    let mut stroke = vec![
        boolean(b"enab", 1),
        enumeration(b"Styl", b"FStl", b"CtrF"),
        enumeration(b"PntT", b"FrFl", b"GrFl"),
        enumeration(b"Md  ", b"BlnM", b"Nrml"),
        untf(b"Opct", b"#Prc", pct(0.9)),
        untf(b"Sz  ", b"#Pxl", px(4.0)),
    ];
    stroke.extend(written_gradient(
        &[
            (0.0, [255.0, 0.0, 0.0], 1.0),
            (1.0, [0.0, 0.0, 255.0], 0.25),
        ],
        b"Rflc",
        30.0,
        1.4,
        1,
        0,
    ));
    stroke.extend([boolean(b"present", 1), boolean(b"showInDialog", 1)]);
    let mut gradient_overlay = vec![
        boolean(b"enab", 1),
        enumeration(b"Md  ", b"BlnM", b"Ovrl"),
        untf(b"Opct", b"#Prc", pct(0.8)),
    ];
    gradient_overlay.extend(written_gradient(
        &[
            (0.0, [0x10 as f64, 0x20 as f64, 0x30 as f64], 0.75),
            (0.5, [0x40 as f64, 0x50 as f64, 0x60 as f64], 0.5),
        ],
        b"Angl",
        15.0,
        0.5,
        0,
        1,
    ));
    gradient_overlay.extend([boolean(b"present", 1), boolean(b"showInDialog", 1)]);
    let colour_overlay = vec![
        boolean(b"enab", 1),
        enumeration(b"Md  ", b"BlnM", b"SftL"),
        rgbc(b"Clr ", 0.0, 255.0, 0x88 as f64),
        untf(b"Opct", b"#Prc", pct(0.7)),
        boolean(b"present", 1),
        boolean(b"showInDialog", 1),
    ];
    let satin = vec![
        boolean(b"enab", 1),
        enumeration(b"Md  ", b"BlnM", b"Mltp"),
        rgbc(b"Clr ", 0x33 as f64, 0x44 as f64, 0x55 as f64),
        untf(b"Opct", b"#Prc", pct(0.45)),
        untf(b"lagl", b"#Ang", px(25.0)),
        untf(b"Dstn", b"#Pxl", px(12.0)),
        untf(b"blur", b"#Pxl", px(18.0)),
        boolean(b"Invr", 0),
        boolean(b"AntA", 1),
        shpc(b"MpgS"),
        boolean(b"present", 1),
        boolean(b"showInDialog", 1),
    ];
    let inner_glow = vec![
        boolean(b"enab", 1),
        enumeration(b"Md  ", b"BlnM", b"Scrn"),
        rgbc(b"Clr ", 0xcc as f64, 0xdd as f64, 0xee as f64),
        untf(b"Opct", b"#Prc", pct(0.35)),
        untf(b"Ckmt", b"#Prc", pct(0.2)),
        untf(b"blur", b"#Pxl", px(7.0)),
        untf(b"Nose", b"#Prc", 0.0),
        untf(b"ShdN", b"#Prc", 0.0),
        boolean(b"AntA", 1),
        enumeration(b"GlwT", b"BETE", b"SfBL"),
        untf(b"Inpr", b"#Prc", 50.0),
        enumeration(b"glwS", b"IGSr", b"SrcC"),
        shpc(b"TrnS"),
        boolean(b"present", 1),
        boolean(b"showInDialog", 1),
    ];
    let inner_shadow = vec![
        boolean(b"enab", 0),
        enumeration(b"Md  ", b"BlnM", b"linearBurn"),
        rgbc(b"Clr ", 0x22 as f64, 0x33 as f64, 0x44 as f64),
        untf(b"Opct", b"#Prc", pct(0.65)),
        boolean(b"uglg", 1),
        // `use_global_light` resolves to the DOCUMENT's angle.
        untf(b"lagl", b"#Ang", px(light.0)),
        untf(b"Dstn", b"#Pxl", px(3.0)),
        untf(b"Ckmt", b"#Prc", pct(0.15)),
        untf(b"blur", b"#Pxl", px(5.0)),
        untf(b"Nose", b"#Prc", 0.0),
        boolean(b"AntA", 1),
        shpc(b"TrnS"),
        boolean(b"present", 1),
        boolean(b"showInDialog", 1),
    ];
    let bevel = vec![
        boolean(b"enab", 1),
        enumeration(b"hglM", b"BlnM", b"Lghn"),
        rgbc(b"hglC", 0xfa as f64, 0xfb as f64, 0xfc as f64),
        untf(b"hglO", b"#Prc", pct(0.55)),
        enumeration(b"sdwM", b"BlnM", b"Drkn"),
        rgbc(b"sdwC", 0x03 as f64, 0x02 as f64, 0x01 as f64),
        untf(b"sdwO", b"#Prc", pct(0.85)),
        enumeration(b"bvlS", b"BESl", b"PlEb"),
        enumeration(b"bvlD", b"BESs", b"Out "),
        boolean(b"uglg", 1),
        untf(b"lagl", b"#Ang", px(light.0)),
        untf(b"Lald", b"#Ang", px(light.1)),
        untf(b"srgR", b"#Prc", pct(2.5)),
        untf(b"blur", b"#Pxl", px(8.0)),
        untf(b"Sftn", b"#Pxl", px(3.0)),
        enumeration(b"bvlT", b"bvlT", b"SfBL"),
        boolean(b"antialiasGloss", 0),
        boolean(b"useShape", 0),
        boolean(b"useTexture", 0),
        shpc(b"TrnS"),
        shpc(b"MpgS"),
        boolean(b"present", 1),
        boolean(b"showInDialog", 1),
    ];
    // Render order is the order the model holds its effects in.
    let items = vec![
        untf(b"Scl ", b"#Prc", 100.0),
        boolean(b"masterFXSwitch", 1),
        objc(b"DrSh", b"DrSh", &drop_shadow),
        objc(b"OrGl", b"OrGl", &outer_glow),
        objc(b"FrFX", b"FrFX", &stroke),
        objc(b"GrFl", b"GrFl", &gradient_overlay),
        objc(b"SoFi", b"SoFi", &colour_overlay),
        objc(b"ChFX", b"ChFX", &satin),
        objc(b"IrGl", b"IrGl", &inner_glow),
        objc(b"IrSh", b"IrSh", &inner_shadow),
        objc(b"ebbl", b"ebbl", &bevel),
    ];
    lfx2(&items)
}

/// The first block with `key`, verbatim.
pub(crate) fn block_body(bytes: &[u8], key: &[u8; 4]) -> Option<Vec<u8>> {
    let mut marker = b"8BIM".to_vec();
    marker.extend(key);
    let at = bytes
        .windows(8)
        .position(|window| window == marker.as_slice())?;
    let len = u32::from_be_bytes(bytes[at + 8..at + 12].try_into().unwrap()) as usize;
    Some(bytes[at + 12..at + 12 + len].to_vec())
}

/// Image resource `id`'s body, read out of the written file's resource block.
fn resource_body(bytes: &[u8], id: u16) -> Option<Vec<u8>> {
    let mut marker = b"8BIM".to_vec();
    marker.extend(id.to_be_bytes());
    marker.extend([0, 0]);
    let at = bytes
        .windows(8)
        .position(|window| window == marker.as_slice())?;
    let len = u32::from_be_bytes(bytes[at + 8..at + 12].try_into().unwrap()) as usize;
    Some(bytes[at + 12..at + 12 + len].to_vec())
}

fn checker(w: u32, h: u32) -> RgbaImage {
    let mut out = RgbaImage::new(w, h);
    for y in 0..h {
        for x in 0..w {
            let a = if (x + y) % 5 == 0 { 130 } else { 255 };
            out.put_pixel(
                x,
                y,
                Rgba([(x * 13 % 251) as u8, (y * 7 % 241) as u8, 90, a]),
            );
        }
    }
    out
}

/// A 24 x 20 document holding an 8 x 6 styled raster at (4, 3) with a plain
/// enabled mask, over an opaque base, under the nondefault [`LIGHT`].
fn export_document(profile: &Arc<IccProfile>) -> DrDocument {
    let doc = DrDocument::from_pixels(checker(24, 20));
    let doc = doc
        .adding_image_layer(0, checker(8, 6), "Styled")
        .expect("styled layer");
    let doc = doc
        .set_global_light(doctored_core::style::GlobalLight {
            angle: LIGHT.0,
            altitude: LIGHT.1,
        })
        .expect("the nondefault light");
    let mut doc = doc;
    doc.profile = Arc::clone(profile);
    doc.layers[0].name = "Base".into();
    let styled = &mut doc.layers[1];
    styled.offset = (4, 3);
    styled.style = Some(Arc::new(
        LayerStyle::from_json(EXPORT_JSON).expect("the export style"),
    ));
    let mut mask = GrayImage::new(8, 6);
    for (index, pixel) in mask.pixels_mut().enumerate() {
        pixel.0[0] = (index * 5 % 256) as u8;
    }
    styled.mask = Some(Arc::new(mask));
    styled.mask_properties.offset = (4, 3);
    doc
}

#[test]
fn a_styled_document_writes_the_three_constructs_and_reopens_equal() {
    let dir = TempDir::new().unwrap();
    let doc = export_document(&IccProfile::srgb());
    let report = export_report(&doc);
    assert_eq!(report["available"], Value::Bool(true), "{report}");
    assert!(
        !strings(&report, "losses")
            .iter()
            .any(|l| l.contains("rasterized")),
        "nothing is baked"
    );
    let observation = strings(&report, "observations")
        .into_iter()
        .find(|o| o.contains("has a layer style"))
        .expect("the preserve observation");
    assert!(
        observation.contains(
            "(drop_shadow, outer_glow, stroke, gradient_overlay, color_overlay, satin, inner_glow, bevel_emboss, fill opacity 60 %, Blend If on gray)"
        ),
        "{observation}"
    );

    let path = dir.path().join("styled8.psd");
    save(&doc, &path).expect("save layered");
    let bytes = std::fs::read(&path).expect("read back");
    write(&dir, "styled8.psd", &bytes);
    assert!(
        report["worst_case_bytes"].as_u64().unwrap() >= bytes.len() as u64,
        "the forecast covers the blocks it wrote"
    );

    assert_eq!(
        block_body(&bytes, b"lfx2").expect("an lfx2 block"),
        expected_lfx2(LIGHT)
    );
    assert_eq!(
        block_body(&bytes, b"iOpa").expect("an iOpa block"),
        [153, 0, 0, 0]
    );
    let mut ranges = [0u8; 40];
    for slot in ranges.chunks_exact_mut(4) {
        slot.copy_from_slice(&[0, 0, 255, 255]);
    }
    ranges[..4].copy_from_slice(&[0, 10, 240, 255]);
    ranges[4..8].copy_from_slice(&[5, 15, 235, 250]);
    let mut section = 40u32.to_be_bytes().to_vec();
    section.extend(ranges);
    assert_eq!(
        bytes
            .windows(section.len())
            .filter(|window| *window == section.as_slice())
            .count(),
        1,
        "exactly one record carries the blending ranges"
    );
    assert_eq!(
        resource_body(&bytes, 1037),
        Some(137i32.to_be_bytes().to_vec())
    );
    assert_eq!(
        resource_body(&bytes, 1049),
        Some(52i32.to_be_bytes().to_vec())
    );

    let back = import_report(&path);
    let reopened = open_psd(&path, &back);
    assert_eq!(
        reopened.layers[1]
            .style
            .as_deref()
            .expect("the reopened style")
            .to_json(),
        resolved(&doc.layers[1].style.as_deref().unwrap().to_json(), LIGHT)
    );
    assert_eq!(reopened.global_light, doc.global_light);
    assert!(reopened.layers[1].mask.is_some(), "its own mask travels");
}

#[test]
fn a_display_p3_document_writes_and_reads_converted_effect_colours() {
    let dir = TempDir::new().unwrap();
    let p3 = IccProfile::display_p3();
    let srgb = IccProfile::srgb();
    let doc = export_document(&p3);
    let path = dir.path().join("styled-p3.psd");
    save(&doc, &path).expect("save layered");
    let bytes = std::fs::read(&path).expect("read back");
    let body = block_body(&bytes, b"lfx2").expect("an lfx2 block");
    // Every authored colour is written as its Display P3 numbers.
    for authored in [[0x11u8, 0x22, 0x33], [0x00, 0xff, 0x88], [0x33, 0x44, 0x55]] {
        let expected = converted(authored, &srgb, &p3);
        let needle = rgbc(
            b"Clr ",
            f64::from(expected[0]),
            f64::from(expected[1]),
            f64::from(expected[2]),
        );
        assert!(
            body.windows(needle.len()).any(|window| window == needle),
            "the converted triple {expected:?} of {authored:?} is written"
        );
    }
    // And reads back as the reverse conversion, with every other field intact.
    let back = import_report(&path);
    let reopened = open_psd(&path, &back);
    let style: Value = serde_json::from_str(
        &reopened.layers[1]
            .style
            .as_deref()
            .expect("the reopened style")
            .to_json(),
    )
    .unwrap();
    let authored: Value = serde_json::from_str(&resolved(
        &doc.layers[1].style.as_deref().unwrap().to_json(),
        LIGHT,
    ))
    .unwrap();
    let round_trip = converted(converted([0x11, 0x22, 0x33], &srgb, &p3), &p3, &srgb);
    assert_eq!(
        style["effects"][0]["color"],
        json!(format!(
            "#{:02x}{:02x}{:02x}",
            round_trip[0], round_trip[1], round_trip[2]
        ))
    );
    // The Display P3 numbers read back as a wide colour: off the sRGB
    // byte grid, so the slot keeps its `linear` beside the nearest byte.
    assert!(
        style["effects"][0]["linear"].is_array(),
        "the converted colour reads back wide: {}",
        style["effects"][0]
    );
    let strip = |value: &Value| {
        let mut value = value.clone();
        for effect in value["effects"].as_array_mut().into_iter().flatten() {
            if let Some(effect) = effect.as_object_mut() {
                for key in ["linear", "highlight_linear", "shadow_linear"] {
                    effect.remove(key);
                }
            }
        }
        let mut text = value.to_string();
        for key in ["color", "highlight_color", "shadow_color"] {
            while let Some(at) = text.find(&format!("\"{key}\":\"#")) {
                text.replace_range(at..at + key.len() + 12, "");
            }
        }
        text
    };
    assert_eq!(
        strip(&style),
        strip(&authored),
        "every non-colour field survives the two conversions"
    );
}

#[test]
fn a_style_photoshop_cannot_hold_is_baked_with_its_reason() {
    let dir = TempDir::new().unwrap();
    for (name, json, density, feather, reason) in [
        (
            "satin",
            r#"{"effects":[{"type":"satin","distance":300}]}"#,
            1.0,
            0.0,
            "satin distance is outside Photoshop's 1\u{2013}250 px range",
        ),
        (
            "stroke",
            r#"{"effects":[{"type":"stroke","size":0.5}]}"#,
            1.0,
            0.0,
            "stroke size is below Photoshop's 1 px minimum",
        ),
        (
            "mask",
            r#"{"effects":[{"type":"color_overlay"}]}"#,
            1.0,
            1.5,
            "enabled mask has density or feather, which a PSD mask cannot store",
        ),
    ] {
        let mut doc = export_document(&IccProfile::srgb());
        doc.layers[1].style = Some(Arc::new(LayerStyle::from_json(json).expect(name)));
        doc.layers[1].mask_properties.density = density;
        doc.layers[1].mask_properties.feather = feather;
        let report = export_report(&doc);
        assert_eq!(report["available"], Value::Bool(true), "{name}: {report}");
        let loss = strings(&report, "losses")
            .into_iter()
            .find(|l| l.contains("its effects are rasterized into the layer's pixels"))
            .unwrap_or_else(|| panic!("{name}: no rasterize loss"));
        assert!(loss.contains(reason), "{name}: {loss}");
        assert!(
            !strings(&report, "observations")
                .iter()
                .any(|o| o.contains("has a layer style")),
            "{name}: a baked style is not a preserved one"
        );
        let path = dir.path().join(format!("baked-{name}.psd"));
        save(&doc, &path).expect("save layered");
        let bytes = std::fs::read(&path).expect("read back");
        assert!(block_body(&bytes, b"lfx2").is_none(), "{name}");
        assert!(resource_body(&bytes, 1037).is_none(), "{name}");
        let back = import_report(&path);
        assert!(
            open_psd(&path, &back).layers[1].style.is_none(),
            "{name}: the bake carries no style"
        );
    }
}

#[test]
fn an_unstyled_document_writes_no_effect_constructs_at_all() {
    let dir = TempDir::new().unwrap();
    let doc = DrDocument::from_pixels(checker(8, 6))
        .set_global_light(doctored_core::style::GlobalLight {
            angle: LIGHT.0,
            altitude: LIGHT.1,
        })
        .expect("the nondefault light");
    let report = export_report(&doc);
    assert!(
        strings(&report, "losses")
            .iter()
            .any(|l| l.contains("global light direction is not written")),
        "a file with no effects still loses the light: {:?}",
        strings(&report, "losses")
    );
    let path = dir.path().join("plain.psd");
    save(&doc, &path).expect("save layered");
    let bytes = std::fs::read(&path).expect("read back");
    for key in [b"lfx2", b"iOpa"] {
        assert!(block_body(&bytes, key).is_none());
    }
    for id in [1037u16, 1049] {
        assert!(resource_body(&bytes, id).is_none());
    }
    assert!(
        !bytes.windows(4).any(|window| window == 40u32.to_be_bytes()),
        "no record writes a blending-ranges section"
    );
}

// -------------------------------------------------------------- 16 bits --

fn planes16(rgba: &[[u16; 4]]) -> Vec<(i16, Vec<u8>)> {
    [(-1i16, 3usize), (0, 0), (1, 1), (2, 2)]
        .into_iter()
        .map(|(id, component)| {
            (
                id,
                fixture16::raw_plane(&rgba.iter().map(|p| p[component]).collect::<Vec<_>>()),
            )
        })
        .collect()
}

fn typed_info(document: &PrecisionDocument) -> Value {
    let mut error = ptr::null_mut();
    let json = unsafe { doctored_core::ffi_precision::dr_precision_doc_info(document, &mut error) };
    assert!(!json.is_null(), "{}", take_err_string(error));
    serde_json::from_str(&take_err_string(json)).expect("info JSON")
}

const PX16: [[u16; 4]; 2] = [[4097, 51234, 60000, 65535], [4098, 51233, 60000, 32768]];

pub(crate) fn file16(records: &[fixture16::Record]) -> Vec<u8> {
    fixture16::fixture(
        (CANVAS[0], CANVAS[1]),
        records,
        IccProfile::srgb().bytes(),
        (72.0, 72.0),
        &PX16,
        true,
        false,
    )
}

#[test]
fn the_typed_import_carries_the_same_style_and_the_file_global_light() {
    let dir = TempDir::new().unwrap();
    let mut record = fixture16::Record::raster("Styled", [0, 0, 1, 2], planes16(&PX16));
    record
        .blocks
        .extend(fixture16::block(b"lfx2", &lfx2(&authored_items())));
    record
        .blocks
        .extend(fixture16::block(b"iOpa", &[128, 0, 0, 0]));
    record.ranges = red_ranges();
    let bytes = with_resources(file16(&[record]), &resource(1037, &45i32.to_be_bytes()));
    let path = write(&dir, "authored16.psd", &bytes);
    let report = import_report(&path);
    assert_eq!(report["import_precision"], json!("rgba16"));
    assert_eq!(report["layered_available"], Value::Bool(true), "{report}");
    // The file's light becomes the document's own, so nothing is
    // resolved and nothing is observed about it.
    assert!(
        !strings(&report, "observations")
            .iter()
            .any(|o| o.contains("global light")),
        "{:?}",
        strings(&report, "observations")
    );
    assert!(
        strings(&report, "verified_losses").is_empty(),
        "{:?}",
        strings(&report, "verified_losses")
    );

    // The byte style exactly, `use_global_light` kept.
    let expected = LayerStyle::from_json(AUTHORED_JSON)
        .expect("the expected style")
        .to_json();
    let document = open_typed(&path, &report);
    assert_eq!(
        document.layer(0).unwrap().reported_style(),
        Some(expected.clone())
    );
    let info = typed_info(&document);
    assert_eq!(info["layers"][0]["style"], json!(expected));
    assert_eq!(
        info["global_light"],
        json!({"angle": 45.0, "altitude": 30.0})
    );

    // The pixels are the old resolved import's: the same style with every
    // global-light effect turned into its own, under the default light.
    let mut old: Value = serde_json::from_str(AUTHORED_JSON).unwrap();
    old["effects"][0]["use_global_light"] = Value::Bool(false);
    let old = LayerStyle::from_json(&old.to_string()).expect("the old resolved style");
    let canvas = document.dimensions();
    let resolved_document = PrecisionDocument::from_parts(
        canvas.0,
        canvas.1,
        Arc::clone(document.profile()),
        Metadata::default(),
        document.resolution(),
        vec![document
            .layer(0)
            .unwrap()
            .clone()
            .with_layer_style(Some(&old), canvas)
            .expect("the old resolved layer")],
    )
    .unwrap();
    assert!(document
        .render(LIMIT, 0, None)
        .unwrap()
        .same_samples(&resolved_document.render(LIMIT, 0, None).unwrap()));
}

#[test]
fn the_typed_import_carries_a_styled_pass_through_folder_and_adjustment_record() {
    let dir = TempDir::new().unwrap();
    let overlay = lfx2(&[
        boolean(b"masterFXSwitch", 1),
        objc(
            b"SoFi",
            b"SoFi",
            &[boolean(b"enab", 1), rgbc(b"Clr ", 1.0, 2.0, 3.0)],
        ),
    ]);
    let base = fixture16::Record::raster("Base", [0, 0, 1, 2], planes16(&PX16));
    let closer = fixture16::Record::folder("</Layer group>", 3, *b"norm");
    let art = fixture16::Record::raster("Art", [0, 0, 1, 2], planes16(&PX16));
    let mut folder = fixture16::Record::folder("Outer", 1, *b"pass");
    folder.blocks.extend(fixture16::block(b"lfx2", &overlay));
    let mut curves = fixture16::Record::raster(
        "Curves 1",
        [0; 4],
        [-1i16, 0, 1, 2]
            .into_iter()
            .map(|id| (id, fixture16::raw_plane(&[])))
            .collect(),
    );
    // The identity curve: is_map 0, version 1, the RGB channel, two points.
    let curve = [0u8, 0, 1, 0, 0, 0, 1, 0, 2, 0, 0, 0, 0, 0, 255, 0, 255, 0];
    curves.blocks = fixture16::block(b"curv", &curve);
    curves.blocks.extend(fixture16::block(b"lfx2", &overlay));
    let path = write(
        &dir,
        "typed-refusals.psd",
        &file16(&[base, closer, art, folder, curves]),
    );
    let report = import_report(&path);
    let losses = strings(&report, "verified_losses");
    // The Pass Through folder CARRIES its effects too.
    assert!(
        !losses
            .iter()
            .any(|l| l.contains("“Outer”") && l.contains("layer effects")),
        "{losses:?}"
    );
    // The adjustment record CARRIES its effects at both depths, so it
    // names no loss of them.
    assert!(
        !losses
            .iter()
            .any(|l| l.contains("“Curves 1”") && l.contains("layer effects")),
        "{losses:?}"
    );
    let document = open_typed(&path, &report);
    let styled: Vec<_> = (0..document.layer_count())
        .filter(|&i| document.layer(i).unwrap().reported_style().is_some())
        .map(|i| document.layer(i).unwrap().name().to_string())
        .collect();
    assert_eq!(
        styled,
        ["Outer", "Curves 1"],
        "the Pass Through folder and the adjustment carry their styles"
    );
    let outer = (0..document.layer_count())
        .map(|i| document.layer(i).unwrap())
        .find(|l| l.name() == "Outer")
        .unwrap();
    assert!(outer.is_pass_through());
}

#[test]
fn the_sixteen_bit_writer_writes_the_same_three_constructs() {
    let dir = TempDir::new().unwrap();
    let profile = IccProfile::srgb();
    let pixels: Vec<[u16; 4]> = (0..48).map(|i| [i * 700, 20000, 30000, 65535]).collect();
    let image = PrecisionImage::from_rgba16(8, 6, pixels, Arc::clone(&profile), None).unwrap();
    let style = LayerStyle::from_json(EXPORT_JSON).expect("the export style");
    let styled = PLayer::raster("Styled", image, (4, 3), 1.0, true)
        .unwrap()
        .with_layer_style(Some(&style), (24, 20))
        .expect("the typed style");
    let base = PLayer::raster(
        "Base",
        PrecisionImage::from_rgba16(
            24,
            20,
            vec![[10000u16, 20000, 30000, 65535]; 480],
            Arc::clone(&profile),
            None,
        )
        .unwrap(),
        (0, 0),
        1.0,
        true,
    )
    .unwrap();
    let document = PrecisionDocument::from_parts(
        24,
        20,
        Arc::clone(&profile),
        Metadata::default(),
        Resolution::default(),
        vec![base, styled],
    )
    .unwrap();
    let report = export_report16(&document);
    assert_eq!(report["available"], Value::Bool(true), "{report}");
    assert!(strings(&report, "observations")
        .iter()
        .any(|o| o.contains("Layer “Styled” has a layer style")
            && o.contains("(lfx2, iOpa, blending ranges)")));

    let projection = document.render(LIMIT, 0, None).expect("projection");
    let path = dir.path().join("styled16.psd");
    save16(&document, &projection, &path).expect("save layered");
    let bytes = std::fs::read(&path).expect("read back");
    write(&dir, "styled16.psd", &bytes);
    // The document's light is the default, so the default resolved the angles.
    assert_eq!(
        block_body(&bytes, b"lfx2").expect("an lfx2 block"),
        expected_lfx2((120.0, 30.0))
    );
    assert_eq!(
        block_body(&bytes, b"iOpa").expect("an iOpa block"),
        [153, 0, 0, 0]
    );
    assert_eq!(
        resource_body(&bytes, 1037),
        Some(120i32.to_be_bytes().to_vec())
    );
    assert_eq!(
        resource_body(&bytes, 1049),
        Some(30i32.to_be_bytes().to_vec())
    );
    let file = fixture16::read(&bytes);
    assert_eq!(fixture16::by_name(&file)["Styled"].ranges.len(), 40);

    let back = import_report(&path);
    let reopened = open_typed(&path, &back);
    assert_eq!(
        reopened.layer(1).unwrap().reported_style(),
        Some(resolved(
            &document.layer(1).unwrap().reported_style().unwrap(),
            (120.0, 30.0)
        ))
    );
}
