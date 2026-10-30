//! Wide-gamut style colours: a colour slot's `linear` — the colour as
//! linear-light sRGB-primaries components, extended range — through the style
//! JSON (exact round trip, the hex-equivalent drop, the `color` it replaces,
//! malformed input), the composite at 8, 16 and 32 bits, and the `.rz` word
//! 100 at both depths.
//!
//! The composite oracle is written here, not read from the crate: the
//! standard linear sRGB → Display P3 matrix (both D65, the published
//! rounding) and the sRGB transfer curve Display P3 shares. The test colour
//! (1.2249, −0.0421, −0.0196) is Display P3's red primary in linear sRGB
//! components, so a Color Overlay of it must draw (1, 0, 0) in a P3
//! document — a red no 8-bit sRGB hex reaches.

use std::ffi::{c_char, CString};
use std::ptr;
use std::sync::Arc;

use doctored_core::doc::DrDocument;
use doctored_core::ffi_precision_edit::dr_precision_doc_edit;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer as Layer};
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use doctored_core::style::LayerStyle;
use serde_json::{json, Value};
use tempfile::TempDir;

use crate::common::{rect_layer_doc, styled, take_err_string};

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
const CANVAS: (u32, u32) = (16, 12);
const RECT: (i32, i32, u32, u32) = (4, 3, 8, 6);
/// Display P3's red primary in linear sRGB components (module doc).
const P3_RED: [f64; 3] = [1.2249, -0.0421, -0.0196];
/// Linear sRGB → linear Display P3 (rows).
const SRGB_TO_P3: [[f64; 3]; 3] = [
    [0.822_462_1, 0.177_538_0, 0.0],
    [0.033_194_1, 0.966_805_8, 0.0],
    [0.017_082_7, 0.072_397_4, 0.910_519_9],
];

fn encode(v: f64) -> f64 {
    let v = v.clamp(0.0, 1.0);
    if v <= 0.003_130_8 {
        12.92 * v
    } else {
        1.055 * v.powf(1.0 / 2.4) - 0.055
    }
}

fn decode(v: f64) -> f64 {
    if v <= 0.040_45 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}

fn p3_linear(srgb_linear: [f64; 3]) -> [f64; 3] {
    SRGB_TO_P3.map(|row| (0..3).map(|c| row[c] * srgb_linear[c]).sum())
}

fn overlay(linear: [f64; 3]) -> Value {
    json!({"effects":[{"type":"color_overlay","color":"#000000","linear":linear}]})
}

/// The canonical JSON of `style` as a value (the setter's own canonical form).
fn canonical(style: &Value) -> Value {
    let parsed = LayerStyle::from_json(&style.to_string()).unwrap();
    serde_json::from_str(&parsed.to_json()).unwrap()
}

// ------------------------------------------------------------------ JSON --

#[test]
fn linear_round_trips_exactly_and_replaces_its_colour() {
    let linear = [0.123_456_79, -0.05, 1.7];
    let v = canonical(&overlay(linear));
    let e = &v["effects"][0];
    let stored: Vec<f32> = e["linear"]
        .as_array()
        .unwrap()
        .iter()
        .map(|n| n.as_f64().unwrap() as f32)
        .collect();
    assert_eq!(stored, linear.map(|c| c as f32).to_vec());
    // The writer prints the f32 widened, unquantized; re-parsing is exact.
    let again = canonical(&v);
    assert_eq!(again, v, "the canonical form is a fixed point");
    // `color` is the nearest byte of the clamped sRGB numbers.
    let byte = linear.map(|c| (255.0 * encode(c)).round() as u8);
    assert_eq!(
        e["color"],
        json!(format!("#{:02x}{:02x}{:02x}", byte[0], byte[1], byte[2]))
    );
}

#[test]
fn a_mismatched_colour_is_replaced_by_the_wide_colours_byte() {
    let linear = [0.5, 0.2, 0.1];
    let v = canonical(
        &json!({"effects":[{"type":"satin","color":"#00ff00","linear":linear},
        {"type":"bevel_emboss","highlight_color":"#000000","highlight_linear":linear,
         "shadow_color":"#ffffff","shadow_linear":[0.01, 0.02, 0.03]}]}),
    );
    let byte = |l: [f64; 3]| {
        let b = l.map(|c| (255.0 * encode(c)).round() as u8);
        json!(format!("#{:02x}{:02x}{:02x}", b[0], b[1], b[2]))
    };
    let satin = v["effects"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["type"] == "satin")
        .unwrap();
    assert_eq!(satin["color"], byte(linear));
    assert!(satin["linear"].is_array());
    let bevel = v["effects"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["type"] == "bevel_emboss")
        .unwrap();
    assert_eq!(bevel["highlight_color"], byte(linear));
    assert_eq!(bevel["shadow_color"], byte([0.01, 0.02, 0.03]));
    assert!(bevel["highlight_linear"].is_array() && bevel["shadow_linear"].is_array());
}

#[test]
fn a_hex_equivalent_linear_is_dropped_for_its_byte() {
    let code = [0x11u8, 0x80, 0xfe];
    let linear = code.map(|c| decode(f64::from(c) / 255.0));
    let v = canonical(&overlay(linear));
    let e = &v["effects"][0];
    assert!(e.get("linear").is_none(), "{e}");
    assert_eq!(e["color"], json!("#1180fe"));
    // No key at all is byte-identical to the style without one.
    let plain = canonical(&json!({"effects":[{"type":"color_overlay","color":"#1180fe"}]}));
    assert_eq!(v, plain);
}

#[test]
fn components_clamp_and_malformed_linear_is_refused_by_name() {
    let v = canonical(&overlay([10.0, -10.0, 0.5]));
    assert_eq!(v["effects"][0]["linear"], json!([4.0, -4.0, 0.5]));
    for (effect, key) in [
        (json!({"type":"color_overlay","linear":[1, 2]}), "linear"),
        (json!({"type":"drop_shadow","linear":"red"}), "linear"),
        (
            json!({"type":"bevel_emboss","highlight_linear":[1, 2, "a"]}),
            "highlight_linear",
        ),
        (
            json!({"type":"bevel_emboss","shadow_linear":[1, 2, 3, 4]}),
            "shadow_linear",
        ),
    ] {
        let style = json!({ "effects": [effect] }).to_string();
        let err = LayerStyle::from_json(&style).unwrap_err();
        assert_eq!(
            err,
            format!("effects[0].{key} must be an array of 3 numbers")
        );
        let err = LayerStyle::from_json_lenient(&style).unwrap_err();
        assert_eq!(
            err,
            format!("effects[0].{key} must be an array of 3 numbers")
        );
    }
}

// ------------------------------------------------------------- composite --

fn byte_doc(profile: Arc<IccProfile>) -> DrDocument {
    let mut doc = rect_layer_doc(CANVAS, RECT, [255, 255, 255, 255]);
    doc.profile = profile;
    doc
}

fn image16(w: u32, h: u32, profile: &Arc<IccProfile>) -> PrecisionImage {
    PrecisionImage::from_rgba16(
        w,
        h,
        vec![[65535; 4]; (w * h) as usize],
        Arc::clone(profile),
        None,
    )
    .unwrap()
}

fn typed_doc(profile: Arc<IccProfile>) -> PrecisionDocument {
    PrecisionDocument::from_parts(
        CANVAS.0,
        CANVAS.1,
        Arc::clone(&profile),
        Metadata::default(),
        Resolution::default(),
        vec![
            Layer::raster(
                "Base",
                image16(CANVAS.0, CANVAS.1, &profile),
                (0, 0),
                1.0,
                true,
            )
            .unwrap(),
            Layer::raster(
                "Rect",
                image16(RECT.2, RECT.3, &profile),
                (RECT.0, RECT.1),
                1.0,
                true,
            )
            .unwrap(),
        ],
    )
    .unwrap()
}

fn set_style(doc: &PrecisionDocument, style: &Value) -> PrecisionDocument {
    let request = json!({"op":"layer_style","version":1,"layer":1,"style":style});
    let c = CString::new(request.to_string()).unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_edit(
            doc,
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

/// The canvas index of the rect's centre pixel.
fn centre() -> usize {
    let (x, y) = (RECT.0 as usize + 4, RECT.1 as usize + 3);
    y * CANVAS.0 as usize + x
}

#[test]
fn p3_red_composites_as_p3_red_at_16_bits() {
    let oracle = p3_linear(P3_RED).map(encode);
    for c in 0..3 {
        assert!((oracle[c] - [1.0, 0.0, 0.0][c]).abs() <= 1e-3, "{oracle:?}");
    }
    let doc = set_style(&typed_doc(IccProfile::display_p3()), &overlay(P3_RED));
    let px = doc.render(LIMIT, 0, None).unwrap().to_rgba16().unwrap();
    let got = px[centre()];
    for c in 0..3 {
        let g = f64::from(got[c]) / 65535.0;
        assert!(
            (g - oracle[c]).abs() <= 1e-3,
            "channel {c}: {got:?} against {oracle:?}"
        );
    }
}

#[test]
fn p3_red_composites_as_the_byte_red_at_8_bits() {
    let doc = styled(
        &byte_doc(IccProfile::display_p3()),
        1,
        &overlay(P3_RED).to_string(),
    );
    let flat = doc.flattened().unwrap();
    let (x, y) = (RECT.0 as u32 + 4, RECT.1 as u32 + 3);
    let got = flat.get_pixel(x, y).0;
    for (c, want) in [255i32, 0, 0].into_iter().enumerate() {
        assert!(
            (i32::from(got[c]) - want).abs() <= 1,
            "channel {c}: {got:?} against (255, 0, 0)"
        );
    }
}

#[test]
fn p3_red_composites_as_linear_p3_red_at_32_bits() {
    let oracle = p3_linear(P3_RED);
    let float = typed_doc(IccProfile::display_p3())
        .promote_to_float(LIMIT, 0, None)
        .unwrap();
    let doc = set_style(&float, &overlay(P3_RED));
    let px = doc.render(LIMIT, 0, None).unwrap().to_rgba32().unwrap();
    let got = px[centre()];
    for (c, want) in [1.0, 0.0, 0.0].into_iter().enumerate() {
        assert!(
            (f64::from(got[c]) - want).abs() <= 1e-3,
            "channel {c}: {got:?} against (1, 0, 0)"
        );
        assert!((oracle[c] - want).abs() <= 1e-3);
    }
}

#[test]
fn a_16_bit_srgb_pick_off_the_byte_grid_keeps_its_numbers() {
    let numbers = [200.4 / 255.0, 100.3 / 255.0, 50.6 / 255.0];
    let linear = numbers.map(decode);
    let doc = set_style(&typed_doc(IccProfile::srgb()), &overlay(linear));
    let px = doc.render(LIMIT, 0, None).unwrap().to_rgba16().unwrap();
    let got = px[centre()];
    for c in 0..3 {
        let want = numbers[c] * 65535.0;
        assert!(
            (f64::from(got[c]) - want).abs() <= 2.0,
            "channel {c}: {got:?} against {want}"
        );
    }
}

// ------------------------------------------------------------------ .rz --

const WIDE: [f64; 3] = [0.9, -0.03, 0.123_456_7];

fn wide_style() -> Value {
    json!({"effects":[{"type":"drop_shadow","linear":WIDE},
        {"type":"bevel_emboss","shadow_linear":[0.2, 0.3, 0.4]}]})
}

fn style_value(json: &str) -> Value {
    serde_json::from_str(json).unwrap()
}

#[test]
fn the_byte_word_100_comes_first_round_trips_and_refuses_both_ways() {
    let dir = TempDir::new().unwrap();
    let doc = byte_doc(IccProfile::display_p3());
    let save = |name: &str, style: &Value| -> (Vec<u8>, DrDocument) {
        let styled_doc = styled(&doc, 1, &style.to_string());
        let path = dir.path().join(name);
        styled_doc.save_native(path.to_str().unwrap()).unwrap();
        (std::fs::read(&path).unwrap(), styled_doc)
    };
    let (plain, _) = save(
        "plain.rz",
        &json!({"effects":[{"type":"drop_shadow","color":"#123456"}]}),
    );
    assert_ne!(&plain[8..12], &100u32.to_le_bytes(), "no 100 without one");
    let (bytes, styled_doc) = save("wide.rz", &wide_style());
    assert_eq!(&bytes[4..8], &45u32.to_le_bytes());
    assert_eq!(&bytes[8..12], &100u32.to_le_bytes(), "FIRST inside 45");
    let path = dir.path().join("wide.rz");
    let reopened = DrDocument::open(path.to_str().unwrap()).unwrap();
    let json = reopened.layers[1].style.as_deref().unwrap().to_json();
    assert_eq!(
        json,
        styled_doc.layers[1].style.as_deref().unwrap().to_json()
    );
    let stored: Vec<f32> = style_value(&json)["effects"][0]["linear"]
        .as_array()
        .unwrap()
        .iter()
        .map(|n| n.as_f64().unwrap() as f32)
        .collect();
    assert_eq!(stored, WIDE.map(|c| c as f32).to_vec(), "exactly");
    assert_eq!(
        reopened.flattened().unwrap(),
        styled_doc.flattened().unwrap()
    );
    let mut stripped = bytes.clone();
    stripped.drain(8..12);
    let cut = dir.path().join("cut.rz");
    std::fs::write(&cut, &stripped).unwrap();
    let Err(message) = DrDocument::open(cut.to_str().unwrap()) else {
        panic!("a wide colour without the word must refuse");
    };
    assert!(
        message.ends_with("native wide style colour requires DRDC 100"),
        "{message}"
    );
    let mut forged = plain.clone();
    forged.splice(8..8, 100u32.to_le_bytes());
    let path = dir.path().join("forged.rz");
    std::fs::write(&path, &forged).unwrap();
    let Err(message) = DrDocument::open(path.to_str().unwrap()) else {
        panic!("the word without a wide colour must refuse");
    };
    assert!(
        message.ends_with("invalid native wide colour word"),
        "{message}"
    );
    // With a gradient glow too, 100 comes before 99.
    let mut both = wide_style();
    both["effects"]
        .as_array_mut()
        .unwrap()
        .push(json!({"type":"outer_glow","jitter":0.2}));
    let (bytes, _) = save("both.rz", &both);
    assert_eq!(&bytes[8..12], &100u32.to_le_bytes());
    assert_eq!(&bytes[12..16], &99u32.to_le_bytes(), "100 ahead of 99");
}

#[test]
fn the_typed_word_100_round_trips_and_refuses_both_ways() {
    let dir = TempDir::new().unwrap();
    let plain = dir.path().join("plain.rz");
    set_style(
        &typed_doc(IccProfile::display_p3()),
        &json!({"effects":[{"type":"satin"}]}),
    )
    .save_native(plain.to_str().unwrap(), LIMIT, 0, None)
    .unwrap();
    let plain = std::fs::read(&plain).unwrap();
    assert_ne!(&plain[8..12], &100u32.to_le_bytes(), "no 100 without one");
    let doc = set_style(&typed_doc(IccProfile::display_p3()), &wide_style());
    let path = dir.path().join("wide.rz");
    let path = path.to_str().unwrap();
    doc.save_native(path, LIMIT, 0, None).unwrap();
    let bytes = std::fs::read(path).unwrap();
    assert_eq!(&bytes[4..8], &46u32.to_le_bytes());
    assert_eq!(&bytes[8..12], &100u32.to_le_bytes(), "100 after 46");
    let reopened = PrecisionDocument::open_native(path, LIMIT, 0, None).unwrap();
    let json = reopened.layer(1).unwrap().reported_style().unwrap();
    assert_eq!(Some(json.clone()), doc.layer(1).unwrap().reported_style());
    let stored: Vec<f32> = style_value(&json)["effects"][0]["linear"]
        .as_array()
        .unwrap()
        .iter()
        .map(|n| n.as_f64().unwrap() as f32)
        .collect();
    assert_eq!(stored, WIDE.map(|c| c as f32).to_vec(), "exactly");
    let a = reopened
        .render(LIMIT, 0, None)
        .unwrap()
        .to_rgba16()
        .unwrap();
    let b = doc.render(LIMIT, 0, None).unwrap().to_rgba16().unwrap();
    assert_eq!(a, b);
    let mut stripped = bytes.clone();
    stripped.drain(8..12);
    let cut = dir.path().join("cut.rz");
    std::fs::write(&cut, &stripped).unwrap();
    let Err(message) = PrecisionDocument::open_native(cut.to_str().unwrap(), LIMIT, 0, None) else {
        panic!("a wide colour without the word must refuse");
    };
    assert_eq!(message, "native wide style colour requires DRDC 100");
    let mut forged = plain.clone();
    forged.splice(8..8, 100u32.to_le_bytes());
    let path = dir.path().join("forged.rz");
    std::fs::write(&path, &forged).unwrap();
    let Err(message) = PrecisionDocument::open_native(path.to_str().unwrap(), LIMIT, 0, None)
    else {
        panic!("the word without a wide colour must refuse");
    };
    assert_eq!(message, "invalid native wide colour word");
}
