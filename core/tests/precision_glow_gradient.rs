//! Gradient glows with Jitter at 16 bits, at 32 bits and in CMYK
//! ink, the typed `.rz` word 99 and a CMYK PSD's stop inks. Every depth
//! ramps the glow through the one u8 plane renderer and its `t` field, so a
//! gradient glow at 16 bits is the byte document's within one 8-bit code; a
//! 32-bit document interpolates its stops in linear light, so it is held to
//! the byte render with every stop black (its coverage alone: `255 R`
//! over white is `255 − α` there, the float composite being linear); and a
//! CMYK glow prints its stops' inks interpolated at the smoothed `t` times
//! the coverage, both restated here from the colour twin's plane `v`
//! (`cmyk_style_fixture::twin`): `t = 1 − v / 255`, the segment fraction
//! through `(f + 3f² − 2f³) / 2`, `t'` stored as `round(65535 t') / 65535`
//! (the inks read there), the opacity `a` linear at the unsmoothed `t`,
//! coverage `round(min(255 a, min(255, round(4.125 v))))` on the Outer Glow
//! and `round(255 a)` on the Inner Glow.
//!
//! The typed file claims word 99 right after 46 (ahead of 98), a CMYK file
//! right after its rung, and both refuse the other way round.

use std::ffi::{c_char, CString};
use std::ptr;

use doctored_core::ffi_precision_edit::dr_precision_doc_edit;
use doctored_core::ffi_psd::dr_precision_doc_save_psd;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer as Layer};
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use serde_json::{json, Value};
use tempfile::TempDir;

use crate::cmyk_psd_import::open_psd;
use crate::cmyk_structure_fixture::free;
use crate::cmyk_style_fixture::{backdrop, flat, hard_rect, layer, styled, twin, twin_effect};
use crate::common::{cpath, rect_layer_doc, styled as styled8, take_err_string};

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
const CANVAS: (u32, u32) = (96, 64);
const RECT: (i32, i32, u32, u32) = (24, 16, 48, 32);
const K: f64 = 4.125;

fn three() -> Value {
    json!({"stops":[{"position":0,"color":"#000000","opacity":1},
        {"position":0.5,"color":"#ff0000","opacity":0.6},
        {"position":1,"color":"#ffffff","opacity":1}]})
}

fn black_fade() -> Value {
    json!({"stops":[{"position":0,"color":"#000000","opacity":1},
        {"position":0.4,"color":"#000000","opacity":0.4},
        {"position":1,"color":"#000000","opacity":0}]})
}

fn glow(kind: &str, gradient: Value, jitter: f64) -> Value {
    json!({"type":kind,"blend":"normal","opacity":1,"size":12,"fill_type":"gradient",
        "gradient":gradient,"jitter":jitter})
}

fn style(effect: &Value) -> Value {
    json!({"fill_opacity":0,"effects":[effect]})
}

/// The byte render's RGB: a white canvas under a white rect layer made at
/// RECT, the glow at Fill 0.
fn byte_rgb(effect: &Value) -> Vec<[f64; 3]> {
    let doc = rect_layer_doc(CANVAS, RECT, [255; 4]);
    styled8(&doc, 1, &style(effect).to_string())
        .flattened()
        .unwrap()
        .pixels()
        .map(|p| [0, 1, 2].map(|c| f64::from(p.0[c])))
        .collect()
}

fn assert_within(got: &[[f64; 3]], want: &[[f64; 3]], what: &str) {
    let mut lit = 0;
    for (i, (g, w)) in got.iter().zip(want).enumerate() {
        for c in 0..3 {
            assert!(
                (g[c] - w[c]).abs() <= 1.0,
                "{what}: pixel {i} channel {c}: {g:?} against {w:?}"
            );
        }
        lit += usize::from(w[0] < 255.0 || w[1] < 255.0);
    }
    assert!(lit > 300, "{what}: only {lit} pixels lit");
}

fn image16(w: u32, h: u32, px: [u8; 4]) -> PrecisionImage {
    PrecisionImage::from_rgba16(
        w,
        h,
        vec![px.map(|v| u16::from(v) * 257); (w * h) as usize],
        IccProfile::srgb(),
        None,
    )
    .unwrap()
}

/// The scene at 16 bits: a white canvas, a white rect at RECT.
fn typed_doc() -> PrecisionDocument {
    PrecisionDocument::from_parts(
        CANVAS.0,
        CANVAS.1,
        IccProfile::srgb(),
        Metadata::default(),
        Resolution::default(),
        vec![
            Layer::raster(
                "Base",
                image16(CANVAS.0, CANVAS.1, [255; 4]),
                (0, 0),
                1.0,
                true,
            )
            .unwrap(),
            Layer::raster(
                "Rect",
                image16(RECT.2, RECT.3, [255; 4]),
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

fn rgb16(doc: &PrecisionDocument) -> Vec<[f64; 3]> {
    let px = doc.render(LIMIT, 0, None).unwrap().to_rgba16().unwrap();
    px.iter()
        .map(|p| [0, 1, 2].map(|c| f64::from(p[c]) / 257.0))
        .collect()
}

#[test]
fn sixteen_bit_gradient_glows_are_the_byte_render() {
    for kind in ["outer_glow", "inner_glow"] {
        let e = glow(kind, three(), 0.5);
        let got = rgb16(&set_style(&typed_doc(), &style(&e)));
        assert_within(&got, &byte_rgb(&e), &format!("16-bit {kind}"));
    }
}

#[test]
fn thirty_two_bit_gradient_glows_have_the_byte_coverage() {
    let float = typed_doc().promote_to_float(LIMIT, 0, None).unwrap();
    assert!(float.is_float());
    for kind in ["outer_glow", "inner_glow"] {
        let e = glow(kind, black_fade(), 0.5);
        let px = set_style(&float, &style(&e))
            .render(LIMIT, 0, None)
            .unwrap()
            .to_rgba32()
            .unwrap();
        let got: Vec<[f64; 3]> = px
            .iter()
            .map(|p| [0, 1, 2].map(|c| 255.0 * f64::from(p[c])))
            .collect();
        assert_within(&got, &byte_rgb(&e), &format!("32-bit {kind}"));
    }
}

// ----------------------------------------------------------------- CMYK --

const INKS: [[f64; 4]; 3] = [
    [0.1, 0.8, 0.2, 0.0],
    [0.7, 0.1, 0.0, 0.1],
    [0.0, 0.0, 0.9, 0.0],
];

fn inked_gradient() -> Value {
    json!({"stops":[{"position":0,"color":"#000000","ink":INKS[0],"opacity":1},
        {"position":0.5,"color":"#ff0000","ink":INKS[1],"opacity":0.5},
        {"position":1,"color":"#ffffff","ink":INKS[2],"opacity":1}]})
}

/// A CMYK document at `depth`: paper under one canvas-sized layer holding
/// an opaque ink rect at RECT.
fn cmyk_base(depth: u32) -> *mut PrecisionDocument {
    let (x0, y0) = (RECT.0 as u32, RECT.1 as u32);
    layer(
        backdrop(CANVAS.0, CANVAS.1, depth, &[[0.0; 4]]),
        "Shape",
        [10.0, 60.0, 30.0, 0.0],
        &hard_rect(CANVAS.0, CANVAS.1, (x0, y0, x0 + RECT.2, y0 + RECT.3)),
    )
}

/// The ink at the smoothed, stored `t'` and the opacity at `t` (module
/// doc), the stops at 0, ½ and 1.
fn ink_at(v: u8) -> ([f64; 4], f64) {
    let t = 1.0 - f64::from(v) / 255.0;
    let pos = [0.0, 0.5, 1.0];
    let opacity = [1.0, 0.5, 1.0];
    let a = if t >= 0.5 {
        opacity[1] + (opacity[2] - opacity[1]) * (t - 0.5) / 0.5
    } else {
        opacity[0] + (opacity[1] - opacity[0]) * t / 0.5
    };
    let (i, f) = if t >= 1.0 {
        (1, 1.0)
    } else if t >= 0.5 {
        (1, (t - 0.5) / 0.5)
    } else {
        (0, t / 0.5)
    };
    let f = if t > 0.0 && t < 1.0 {
        (f + 3.0 * f * f - 2.0 * f * f * f) / 2.0
    } else {
        f
    };
    let ts = ((pos[i] + 0.5 * f) * 65535.0).round() / 65535.0;
    let (i, f) = if ts >= 1.0 {
        (1, 1.0)
    } else if ts >= 0.5 {
        (1, (ts - 0.5) / 0.5)
    } else {
        (0, ts / 0.5)
    };
    (
        [0, 1, 2, 3].map(|c| INKS[i][c] + (INKS[i + 1][c] - INKS[i][c]) * f),
        a,
    )
}

#[test]
fn a_cmyk_gradient_glow_prints_its_stop_inks_by_the_coverage() {
    let (x0, y0) = (RECT.0 as u32, RECT.1 as u32);
    let alpha = hard_rect(CANVAS.0, CANVAS.1, (x0, y0, x0 + RECT.2, y0 + RECT.3));
    for kind in ["outer_glow", "inner_glow"] {
        let colour = json!({"type":kind,"blend":"normal","opacity":1,"size":12});
        let coverage = twin(
            CANVAS.0,
            CANVAS.1,
            &alpha,
            &twin_effect(&colour, None),
            (120.0, 30.0),
        );
        let effect = glow(kind, inked_gradient(), 0.0);
        let doc = styled(cmyk_base(16), 1, &style(&effect));
        let got = flat(doc);
        free(doc);
        let mut lit = 0;
        for (i, (&cov, g)) in coverage.iter().zip(&got).enumerate() {
            let v = (255.0 * cov).round() as u8;
            let level = if v == 0 {
                0.0
            } else {
                let (_, a) = ink_at(v);
                let cap = if kind == "outer_glow" {
                    (K * f64::from(v)).round().min(255.0)
                } else {
                    255.0
                };
                (255.0 * a).min(cap).round()
            };
            let ink = if v == 0 { [0.0; 4] } else { ink_at(v).0 };
            for c in 0..4 {
                let want = 65535.0 * ink[c] * level / 255.0;
                let e = (f64::from(g[c]) - want).abs() / 257.0;
                assert!(
                    e <= 1.0,
                    "{kind}: pixel {i} plate {c}: {g:?} against {want} (v {v}, level {level})"
                );
            }
            lit += usize::from(level > 0.0);
        }
        assert!(lit > 300, "{kind}: only {lit} pixels lit");
    }
}

#[test]
fn a_cmyk_psd_carries_the_glow_gradients_stop_inks() {
    let dir = TempDir::new().unwrap();
    let doc = cmyk_base(16);
    let effects = json!([
        {"type":"outer_glow","ink":INKS[1],"size":5,"fill_type":"gradient",
            "gradient":inked_gradient(),"jitter":0.25},
        {"type":"inner_glow","ink":INKS[0],"size":4,"jitter":0.5}]);
    let doc = styled(doc, 1, &json!({"effects":effects}));
    let path = dir.path().join("glow.psd");
    let mut err = ptr::null_mut();
    let ok = unsafe {
        dr_precision_doc_save_psd(
            doc,
            ptr::null(),
            cpath(&path).as_ptr(),
            0,
            0,
            LIMIT,
            0,
            std::ptr::null(),
            &mut err,
        )
    };
    assert!(ok, "{}", take_err_string(err));
    let back = open_psd(&path, 0).unwrap();
    let style_of = |d: *const PrecisionDocument| -> Value {
        let d = unsafe { &*d };
        let top = d.layer_count() - 1;
        serde_json::from_str(&d.layer(top).unwrap().reported_style().unwrap()).unwrap()
    };
    let (got, want) = (style_of(back), style_of(doc));
    assert_eq!(got["effects"], want["effects"]);
    assert_eq!(
        got["effects"][0]["gradient"]["stops"][1]["ink"],
        json!(INKS[1])
    );
    free(back);
    free(doc);
}

// --------------------------------------------------------------- .rz 99 --

#[test]
fn the_typed_word_99_round_trips_and_refuses_both_ways() {
    let dir = TempDir::new().unwrap();
    let plain = dir.path().join("plain.rz");
    set_style(&typed_doc(), &json!({"effects":[{"type":"outer_glow"}]}))
        .save_native(plain.to_str().unwrap(), LIMIT, 0, None)
        .unwrap();
    let plain = std::fs::read(&plain).unwrap();
    assert_ne!(
        &plain[8..12],
        &99u32.to_le_bytes(),
        "no 99 without a gradient glow"
    );
    let doc = set_style(&typed_doc(), &style(&glow("inner_glow", three(), 0.3)));
    let path = dir.path().join("glow.rz");
    let path = path.to_str().unwrap();
    doc.save_native(path, LIMIT, 0, None).unwrap();
    let bytes = std::fs::read(path).unwrap();
    assert_eq!(&bytes[4..8], &46u32.to_le_bytes());
    assert_eq!(&bytes[8..12], &99u32.to_le_bytes(), "99 after 46");
    let reopened = PrecisionDocument::open_native(path, LIMIT, 0, None).unwrap();
    assert_eq!(
        reopened.layer(1).unwrap().reported_style(),
        doc.layer(1).unwrap().reported_style()
    );
    assert_eq!(rgb16(&reopened), rgb16(&doc));
    let mut stripped = bytes.clone();
    stripped.drain(8..12);
    let cut = dir.path().join("cut.rz");
    std::fs::write(&cut, &stripped).unwrap();
    let Err(message) = PrecisionDocument::open_native(cut.to_str().unwrap(), LIMIT, 0, None) else {
        panic!("a gradient glow without the word must refuse");
    };
    assert_eq!(message, "native gradient glow requires DRDC 99");
    let mut forged = plain.clone();
    forged.splice(8..8, 99u32.to_le_bytes());
    let path = dir.path().join("forged.rz");
    std::fs::write(&path, &forged).unwrap();
    let Err(message) = PrecisionDocument::open_native(path.to_str().unwrap(), LIMIT, 0, None)
    else {
        panic!("the word without a gradient glow must refuse");
    };
    assert_eq!(message, "invalid native glow gradient word");
    // With a texture too, 99 comes before 98.
    let both = set_style(
        &typed_doc(),
        &json!({"effects":[{"type":"outer_glow","jitter":0.1},
            {"type":"bevel_emboss","texture_enabled":true}]}),
    );
    let path = dir.path().join("both.rz");
    both.save_native(path.to_str().unwrap(), LIMIT, 0, None)
        .unwrap();
    let bytes = std::fs::read(&path).unwrap();
    assert_eq!(&bytes[8..12], &99u32.to_le_bytes());
    assert_eq!(&bytes[12..16], &98u32.to_le_bytes(), "99 ahead of 98");
}

#[test]
fn a_float_file_round_trips_word_99() {
    let dir = TempDir::new().unwrap();
    let float = typed_doc().promote_to_float(LIMIT, 0, None).unwrap();
    let doc = set_style(&float, &style(&glow("outer_glow", black_fade(), 0.6)));
    let path = dir.path().join("float.rz");
    doc.save_native(path.to_str().unwrap(), LIMIT, 0, None)
        .unwrap();
    let bytes = std::fs::read(&path).unwrap();
    assert_eq!(&bytes[8..12], &99u32.to_le_bytes());
    let reopened = PrecisionDocument::open_native(path.to_str().unwrap(), LIMIT, 0, None).unwrap();
    assert_eq!(
        reopened.layer(1).unwrap().reported_style(),
        doc.layer(1).unwrap().reported_style()
    );
}

#[test]
fn a_cmyk_file_claims_word_99_after_its_rung() {
    let dir = TempDir::new().unwrap();
    let doc = styled(
        cmyk_base(16),
        1,
        &style(&glow("outer_glow", inked_gradient(), 0.2)),
    );
    let path = dir.path().join("cmyk.rz");
    unsafe { &*doc }
        .save_native(path.to_str().unwrap(), LIMIT, 0, None)
        .unwrap();
    let bytes = std::fs::read(&path).unwrap();
    assert_eq!(&bytes[4..8], &46u32.to_le_bytes());
    let rung = u32::from_le_bytes(bytes[8..12].try_into().unwrap());
    assert!((82..=88).contains(&rung), "the CMYK rung, got {rung}");
    assert_eq!(&bytes[12..16], &99u32.to_le_bytes(), "99 after the rung");
    let reopened = PrecisionDocument::open_native(path.to_str().unwrap(), LIMIT, 0, None).unwrap();
    let reopened = Box::into_raw(Box::new(reopened));
    assert_eq!(
        unsafe { &*reopened }.layer(1).unwrap().reported_style(),
        unsafe { &*doc }.layer(1).unwrap().reported_style()
    );
    assert_eq!(flat(reopened), flat(doc));
    let mut stripped = bytes.clone();
    stripped.drain(12..16);
    let cut = dir.path().join("cut.rz");
    std::fs::write(&cut, &stripped).unwrap();
    let Err(message) = PrecisionDocument::open_native(cut.to_str().unwrap(), LIMIT, 0, None) else {
        panic!("a CMYK gradient glow without the word must refuse");
    };
    assert_eq!(message, "native gradient glow requires DRDC 99");
    free(reopened);
    free(doc);
}
