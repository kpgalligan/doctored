//! Layer style CONTOURS in PSD: every contour-family key
//! crosses our own 8- and 16-bit exports both ways (`TrnS`/`MpgS` `ShpC`,
//! `AntA`, `Inpr`, `useShape`, `antialiasGloss`); a style without contours
//! writes the Linear items every earlier writer wrote; Photoshop's own name
//! forms, as its readback of our export recorded them in
//! `fixtures/psd_photoshop/contour/photoshop.json` (`contour_take.py`'s
//! inspect job), read back as their presets from a hand-encoded `lfx2`; a
//! custom curve keeps its corners; a name-only `ShpC` that is no preset
//! declines by name; and Photoshop's disabled outer-glow slots in
//! `fixtures/psd_photoshop/blend_if/source.psd` carry their `Inpr` 1 % as
//! written, which a `.rz` save claims as word 96 without changing a pixel.

use std::ffi::{c_char, CString};
use std::path::{Path, PathBuf};
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
use doctored_core::style::LayerStyle;
use serde_json::{json, Value};
use tempfile::TempDir;

use crate::common::psd_photoshop::{fixture, import_report, open_psd, strings, LAYERS};
use crate::common::{cpath, rect_layer_doc, styled, take_err_string};
use crate::psd_descriptor_fixture::{boolean, desc, doub, item, objc, text};
use crate::psd_mask_fixture as fixture8;

const LIMIT: u64 = REFERENCE_WORKING_BYTES;

/// Every contour-family key on every effect that has one (`CU2` holds a
/// corner).
fn contoured() -> Value {
    let cu2 = json!([{"x":0,"y":40},{"x":96,"y":230,"corner":true},{"x":200,"y":90},
        {"x":255,"y":200}]);
    json!({"effects":[
        {"type":"drop_shadow","contour":{"preset":"cone"},"anti_aliased":true},
        {"type":"inner_shadow","contour":{"preset":"sawtooth_1"}},
        {"type":"outer_glow","contour":{"preset":"ring_double"},"range":0.25},
        {"type":"inner_glow","contour":{"points":cu2},"range":0.75,"anti_aliased":true},
        {"type":"satin","contour":{"preset":"gaussian"}},
        {"type":"bevel_emboss","gloss_contour":{"preset":"cone_inverted"},
            "gloss_anti_aliased":true,"contour_enabled":true,
            "contour":{"preset":"half_round"},"contour_anti_aliased":true,"contour_range":0.3}
    ]})
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
            std::ptr::null(),
            &mut error,
        )
    };
    assert!(ok, "{}", take_err_string(error));
}

fn utf16(text: &str) -> Vec<u8> {
    text.encode_utf16().flat_map(u16::to_be_bytes).collect()
}

fn contains(bytes: &[u8], needle: &[u8]) -> bool {
    bytes.windows(needle.len()).any(|w| w == needle)
}

fn style_json(doc: &DrDocument, index: usize) -> String {
    doc.layers[index]
        .style
        .as_deref()
        .expect("a style")
        .to_json()
}

// ---------------------------------------------------------- our export --

#[test]
fn every_contour_key_crosses_our_8_bit_psd_both_ways() {
    let dir = TempDir::new().unwrap();
    let doc = styled(
        &rect_layer_doc((64, 48), (16, 12, 32, 24), [0x30, 0xa0, 0x50, 255]),
        1,
        &contoured().to_string(),
    );
    let path = dir.path().join("contour8.psd");
    save8(&doc, &path);
    let bytes = std::fs::read(&path).unwrap();
    for name in [
        "$$$/Contours/Defaults/Cone=Cone",
        "$$$/Contours/Defaults/DoubleRing=Ring - Double",
        "$$$/Contours/Defaults/HalfRound=Half Round",
        "Custom",
    ] {
        assert!(contains(&bytes, &utf16(name)), "{name} is written");
    }
    let report = import_report(&path);
    assert_eq!(
        strings(&report, "verified_losses"),
        Vec::<String>::new(),
        "{report}"
    );
    let back = open_psd(&path, &report, LAYERS);
    let index = back.layers.len() - 1;
    assert_eq!(style_json(&back, index), style_json(&doc, 1));
    assert_eq!(
        back.flattened().unwrap(),
        doc.flattened().unwrap(),
        "the reopened file renders the same"
    );
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
fn every_contour_key_crosses_our_16_bit_psd_both_ways() {
    let dir = TempDir::new().unwrap();
    let typed = typed_doc(&contoured());
    let path = dir.path().join("contour16.psd");
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
            std::ptr::null(),
            &mut error,
        )
    };
    assert!(ok, "{}", take_err_string(error));
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
fn a_style_without_contours_writes_the_linear_items() {
    let dir = TempDir::new().unwrap();
    let doc = styled(
        &rect_layer_doc((32, 24), (8, 6, 16, 12), [0, 0, 0, 255]),
        1,
        r#"{"effects":[{"type":"drop_shadow"},{"type":"outer_glow"},{"type":"bevel_emboss"}]}"#,
    );
    let path = dir.path().join("linear.psd");
    save8(&doc, &path);
    let bytes = std::fs::read(&path).unwrap();
    assert!(contains(&bytes, &utf16("Linear")));
    assert!(!contains(&bytes, &utf16("$$$/Contours")), "no preset name");
    assert!(!contains(&bytes, &utf16("Custom")), "no custom curve");
    let back = open_psd(&path, &import_report(&path), LAYERS);
    let text = style_json(&back, back.layers.len() - 1);
    for key in ["contour", "anti_aliased", "\"range\"", "contour_enabled"] {
        assert!(!text.contains(key), "{key} read from a Linear file: {text}");
    }
}

// --------------------------------------------- Photoshop's own name forms --

fn photoshop_contour(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/psd_photoshop/contour")
        .join(name)
}

/// One `Crv ` list of `CrPt` points, `Cnty` false on a corner.
fn curve(points: &[(f64, f64, bool)]) -> Vec<u8> {
    let mut value = (points.len() as u32).to_be_bytes().to_vec();
    for &(x, y, corner) in points {
        value.extend(b"Objc");
        let mut items = vec![doub(b"Hrzn", x), doub(b"Vrtc", y)];
        if corner {
            items.push(boolean(b"Cnty", 0));
        }
        value.extend(desc(b"CrPt", &items));
    }
    item(b"Crv ", b"VlLs", &value)
}

/// A one-layer 8-bit file whose drop shadow's `TrnS` is a `ShpC` named
/// `name` with `points` as its curve (none when empty).
fn shadow_file(name: &str, points: &[(f64, f64, bool)]) -> Vec<u8> {
    let mut shape = vec![text(b"Nm  ", name)];
    if !points.is_empty() {
        shape.push(curve(points));
    }
    let items = vec![
        boolean(b"masterFXSwitch", 1),
        objc(
            b"DrSh",
            b"DrSh",
            &[
                boolean(b"enab", 1),
                boolean(b"AntA", 1),
                objc(b"TrnS", b"ShpC", &shape),
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

fn shadow_contour(dir: &TempDir, file: &str, bytes: &[u8]) -> Result<Value, Vec<String>> {
    let path = dir.path().join(file);
    std::fs::write(&path, bytes).unwrap();
    let report = import_report(&path);
    let losses = strings(&report, "verified_losses");
    let doc = open_psd(&path, &report, LAYERS);
    match doc.layers[0].style.as_deref() {
        Some(style) => {
            let v: Value = serde_json::from_str(&style.to_json()).unwrap();
            Ok(v["effects"][0].clone())
        }
        None => Err(losses),
    }
}

#[test]
fn photoshops_readback_names_read_as_their_presets() {
    let text = std::fs::read_to_string(photoshop_contour("photoshop.json")).unwrap();
    let recorded: Value = serde_json::from_str(&text).unwrap();
    let layers = recorded["inspect"]["psd8"]["layers"].as_object().unwrap();
    // The take's inspect set (`verify_contour.INSPECT`): each layer's preset.
    let wanted = [
        ("P1", "drop_shadow", "transferSpec", "cone"),
        ("P2", "drop_shadow", "transferSpec", "cone_inverted"),
        ("P3", "outer_glow", "transferSpec", "gaussian"),
        ("P4", "inner_glow", "transferSpec", "half_round"),
        ("P5", "inner_shadow", "transferSpec", "ring"),
        ("P6", "satin", "mappingShape", "ring_double"),
        (
            "P7",
            "drop_shadow",
            "transferSpec",
            "rolling_slope_descending",
        ),
        ("P8", "outer_glow", "transferSpec", "rounded_steps"),
        ("P9", "bevel_emboss", "transferSpec", "sawtooth_1"),
        ("P9", "bevel_emboss", "mappingShape", "half_round"),
    ];
    let dir = TempDir::new().unwrap();
    for (i, (layer, kind, key, preset)) in wanted.into_iter().enumerate() {
        let name = layers[layer][kind][key]["name"]
            .as_str()
            .unwrap_or_else(|| panic!("{layer} {kind} {key}: {}", layers[layer]));
        assert!(name.starts_with("$$$/Contours/Defaults/"), "{name}");
        let effect = shadow_contour(&dir, &format!("name{i}.psd"), &shadow_file(name, &[]))
            .unwrap_or_else(|losses| panic!("{name} declined: {losses:?}"));
        assert_eq!(effect["contour"], json!({"preset": preset}), "{name}");
        // The plain title Photoshop's dialog shows reads the same.
        let title = name.split_once('=').unwrap().1;
        let effect = shadow_contour(&dir, &format!("title{i}.psd"), &shadow_file(title, &[]))
            .unwrap_or_else(|losses| panic!("{title} declined: {losses:?}"));
        assert_eq!(effect["contour"], json!({"preset": preset}), "{title}");
        // A localized title behind the same key reads as the preset too.
        let key = name.split_once('=').unwrap().0;
        let localized = format!("{key}=Kontur {i}");
        let effect = shadow_contour(
            &dir,
            &format!("localized{i}.psd"),
            &shadow_file(&localized, &[]),
        )
        .unwrap_or_else(|losses| panic!("{localized} declined: {losses:?}"));
        assert_eq!(effect["contour"], json!({"preset": preset}), "{localized}");
    }
    // P10's custom curve read back with its four points.
    let points = layers["P10"]["inner_glow"]["transferSpec"]["curve"]
        .as_array()
        .unwrap();
    assert_eq!(points.len(), 4);
}

#[test]
fn a_custom_curve_keeps_its_corner_and_a_bare_name_declines() {
    let dir = TempDir::new().unwrap();
    let effect = shadow_contour(
        &dir,
        "custom.psd",
        &shadow_file(
            "Custom",
            &[
                (0.0, 40.0, false),
                (96.0, 230.0, true),
                (200.0, 90.0, false),
                (255.0, 200.0, false),
            ],
        ),
    )
    .unwrap();
    assert_eq!(
        effect["contour"],
        json!({"points":[{"x":0.0,"y":40.0},{"x":96.0,"y":230.0,"corner":true},
            {"x":200.0,"y":90.0},{"x":255.0,"y":200.0}]})
    );
    assert_eq!(
        effect["anti_aliased"],
        json!(true),
        "AntA carried beside a curve"
    );
    let losses = shadow_contour(&dir, "bare.psd", &shadow_file("Spiral", &[])).unwrap_err();
    assert!(
        losses
            .iter()
            .any(|l| l.contains("contour Spiral has no curve")),
        "{losses:?}"
    );
    let linear = shadow_contour(
        &dir,
        "linear.psd",
        &shadow_file("Linear", &[(0.0, 0.0, false), (255.0, 255.0, false)]),
    )
    .unwrap();
    assert!(linear.get("contour").is_none() && linear.get("anti_aliased").is_none());
}

// ------------------------------------------------- Photoshop's own file --

#[test]
fn photoshops_disabled_glow_slots_carry_their_range_and_claim_word_96() {
    let path = fixture("blend_if", "source.psd");
    let report = import_report(&path);
    let doc = open_psd(&path, &report, LAYERS);
    let mut unranged = (*doc).clone();
    let mut overlays = 0;
    for (index, layer) in doc.layers.iter().enumerate() {
        let Some(style) = layer.style.as_deref() else {
            continue;
        };
        let v: Value = serde_json::from_str(&style.to_json()).unwrap();
        let mut plain = v.clone();
        for (i, effect) in v["effects"].as_array().unwrap().iter().enumerate() {
            for key in [
                "contour",
                "anti_aliased",
                "gloss_contour",
                "contour_enabled",
            ] {
                assert!(
                    effect.get(key).is_none(),
                    "{}: {key} in {effect}",
                    layer.name
                );
            }
            if effect["type"] == "outer_glow" {
                assert_eq!(effect["enabled"], json!(false), "{}", layer.name);
                assert_eq!(effect["range"], json!(0.01), "{}", layer.name);
                // The slot's `ShdN` 32 % is carried as its Jitter too (gradient
                // glows), so the file also claims word 99.
                assert_eq!(effect["jitter"], json!(0.32), "{}", layer.name);
                plain["effects"][i].as_object_mut().unwrap().remove("range");
            } else {
                assert!(effect.get("range").is_none(), "{}: {effect}", layer.name);
            }
        }
        unranged = styled(&unranged, index, &plain.to_string());
        overlays += 1;
    }
    assert_eq!(overlays, 4, "four overlay layers");
    assert_eq!(
        doc.flattened().unwrap(),
        unranged.flattened().unwrap(),
        "a disabled slot's range changes no pixel"
    );
    let dir = TempDir::new().unwrap();
    let saved = dir.path().join("blend_if.rz");
    doc.save_native(saved.to_str().unwrap()).unwrap();
    let bytes = std::fs::read(&saved).unwrap();
    assert_eq!(&bytes[4..8], &45u32.to_le_bytes());
    assert_eq!(
        &bytes[8..12],
        &113u32.to_le_bytes(),
        "word 113 (the Background mark a layered PSD import sets) first inside 45"
    );
    assert_eq!(
        &bytes[12..16],
        &99u32.to_le_bytes(),
        "then word 99 (the slots' Jitter)"
    );
    assert_eq!(&bytes[16..20], &96u32.to_le_bytes(), "then word 96");
    let reopened = DrDocument::open(saved.to_str().unwrap()).unwrap();
    for (a, b) in reopened.layers.iter().zip(&doc.layers) {
        assert_eq!(
            a.style.as_deref().map(LayerStyle::to_json),
            b.style.as_deref().map(LayerStyle::to_json)
        );
    }
    assert_eq!(reopened.flattened().unwrap(), doc.flattened().unwrap());
}
