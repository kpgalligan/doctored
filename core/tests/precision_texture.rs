//! The Bevel & Emboss TEXTURE at 16 bits, at 32 bits and in
//! CMYK ink, and the typed `.rz` word 98. Every depth renders the bevel
//! through the one u8 plane renderer, so a textured bevel at 16 or 32 bits
//! is the byte document's within one 8-bit code (the highlight white over
//! black, so the red IS the code: `R / 257` at 16 bits, `255 R` at 32, the
//! float composite being linear), and a CMYK bevel printing an explicit
//! highlight ink is that ink times `H / 255`, `H` the RGB16 twin's
//! coverage (`cmyk_style_fixture::twin`) — the texture is height, never
//! inked.
//!
//! The typed file claims word 98 right after 46 (ahead of 97), a CMYK file
//! right after its rung, and both refuse the other way round.

use std::ffi::{c_char, CString};
use std::ptr;

use doctored_core::ffi_precision_edit::dr_precision_doc_edit;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer as Layer};
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use serde_json::{json, Value};
use tempfile::TempDir;

use crate::cmyk_structure_fixture::free;
use crate::cmyk_style_fixture::{backdrop, flat, hard_rect, layer, styled, twin, twin_effect};
use crate::common::{solid, styled as styled8, take_err_string};

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
const CANVAS: (u32, u32) = (96, 64);
const RECT: (i32, i32, u32, u32) = (16, 12, 64, 40);

/// The textured bevel: highlight white Normal 100 %, shadow off, angle 0,
/// altitude 30, the stripes preset at depth −2, phase (2, 1), linked.
fn bevel() -> Value {
    json!({"type":"bevel_emboss","style":"inner_bevel","depth":1,"size":10,"angle":0,
        "use_global_light":false,"altitude":30,"highlight_blend":"normal",
        "highlight_color":"#ffffff","highlight_opacity":1,"shadow_opacity":0,
        "texture_enabled":true,"texture":{"pattern":{"preset":"stripes"},"depth":-2,
            "phase":[2,1]}})
}

fn style(effect: Value) -> Value {
    json!({"effects":[effect]})
}

/// The byte render's red: a black canvas under a black rect layer made at
/// RECT.
fn byte_red(effect: &Value) -> Vec<f64> {
    let doc =
        doctored_core::doc::DrDocument::from_pixels(solid(CANVAS.0, CANVAS.1, [0, 0, 0, 255]))
            .adding_image_layer(0, solid(RECT.2, RECT.3, [0, 0, 0, 255]), "Rect")
            .unwrap();
    let mut doc = doc.with_layer_offset(1, RECT.0, RECT.1).unwrap();
    doc.layers[1].reference = (0, 0);
    styled8(&doc, 1, &style(effect.clone()).to_string())
        .flattened()
        .unwrap()
        .pixels()
        .map(|p| f64::from(p.0[0]))
        .collect()
}

fn assert_within(got: &[f64], want: &[f64], what: &str) {
    let mut lit = 0;
    for (i, (g, w)) in got.iter().zip(want).enumerate() {
        assert!((g - w).abs() <= 1.0, "{what}: pixel {i}: {g} against {w}");
        lit += usize::from(*w > 0.0);
    }
    assert!(lit > 200, "{what}: only {lit} pixels lit");
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

/// The scene at 16 bits: a black canvas, a black rect at RECT.
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
                image16(CANVAS.0, CANVAS.1, [0, 0, 0, 255]),
                (0, 0),
                1.0,
                true,
            )
            .unwrap(),
            Layer::raster(
                "Rect",
                image16(RECT.2, RECT.3, [0, 0, 0, 255]),
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

fn red16(doc: &PrecisionDocument) -> Vec<f64> {
    let px = doc.render(LIMIT, 0, None).unwrap().to_rgba16().unwrap();
    px.iter().map(|p| f64::from(p[0]) / 257.0).collect()
}

#[test]
fn sixteen_bit_texture_is_the_byte_render() {
    let got = red16(&set_style(&typed_doc(), &style(bevel())));
    assert_within(&got, &byte_red(&bevel()), "16-bit");
}

#[test]
fn thirty_two_bit_texture_is_the_byte_render() {
    let float = typed_doc().promote_to_float(LIMIT, 0, None).unwrap();
    assert!(float.is_float());
    let px = set_style(&float, &style(bevel()))
        .render(LIMIT, 0, None)
        .unwrap()
        .to_rgba32()
        .unwrap();
    let got: Vec<f64> = px.iter().map(|p| 255.0 * f64::from(p[0])).collect();
    assert_within(&got, &byte_red(&bevel()), "32-bit");
}

// ----------------------------------------------------------------- CMYK --

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

#[test]
fn a_cmyk_texture_mixes_its_full_ink_by_the_byte_coverage() {
    let (x0, y0) = (RECT.0 as u32, RECT.1 as u32);
    let alpha = hard_rect(CANVAS.0, CANVAS.1, (x0, y0, x0 + RECT.2, y0 + RECT.3));
    let ink = [0.6, 0.2, 0.1, 0.5];
    let full = ink.map(|f: f64| 65535.0 * f);
    let coverage = twin(
        CANVAS.0,
        CANVAS.1,
        &alpha,
        &twin_effect(&bevel(), Some("highlight")),
        (120.0, 30.0),
    );
    for depth in [8, 16] {
        let mut inked = bevel();
        inked["highlight_ink"] = json!(ink);
        let mut s = style(inked);
        s["fill_opacity"] = json!(0);
        let doc = styled(cmyk_base(depth), 1, &s);
        let got = flat(doc);
        free(doc);
        let mut lit = 0;
        for (i, (&t, g)) in coverage.iter().zip(&got).enumerate() {
            let level = (255.0 * t).round();
            for c in 0..4 {
                let want = full[c] * level / 255.0;
                let e = (f64::from(g[c]) - want).abs() / 257.0;
                assert!(
                    e <= 1.0,
                    "{depth}-bit: pixel {i} plate {c}: {g:?} against {want} (H {level})"
                );
            }
            lit += usize::from(level > 0.0);
        }
        assert!(lit > 200, "{depth}-bit: only {lit} pixels lit");
    }
}

// --------------------------------------------------------------- .rz 98 --

#[test]
fn the_typed_word_98_round_trips_and_refuses_both_ways() {
    let dir = TempDir::new().unwrap();
    let plain = dir.path().join("plain.rz");
    set_style(&typed_doc(), &style(json!({"type":"bevel_emboss"})))
        .save_native(plain.to_str().unwrap(), LIMIT, 0, None)
        .unwrap();
    let plain = std::fs::read(&plain).unwrap();
    assert_ne!(
        &plain[8..12],
        &98u32.to_le_bytes(),
        "no 98 without a texture"
    );
    let doc = set_style(&typed_doc(), &style(bevel()));
    let path = dir.path().join("texture.rz");
    let path = path.to_str().unwrap();
    doc.save_native(path, LIMIT, 0, None).unwrap();
    let bytes = std::fs::read(path).unwrap();
    assert_eq!(&bytes[4..8], &46u32.to_le_bytes());
    assert_eq!(&bytes[8..12], &98u32.to_le_bytes(), "98 after 46");
    assert_eq!(&bytes[12..16], &plain[8..12], "then the plain word");
    let reopened = PrecisionDocument::open_native(path, LIMIT, 0, None).unwrap();
    assert_eq!(
        reopened.layer(1).unwrap().reported_style(),
        doc.layer(1).unwrap().reported_style()
    );
    assert_eq!(red16(&reopened), red16(&doc));
    let mut stripped = bytes.clone();
    stripped.drain(8..12);
    let cut = dir.path().join("cut.rz");
    std::fs::write(&cut, &stripped).unwrap();
    let Err(message) = PrecisionDocument::open_native(cut.to_str().unwrap(), LIMIT, 0, None) else {
        panic!("a texture without the word must refuse");
    };
    assert_eq!(message, "native bevel texture requires DRDC 98");
    let mut forged = plain.clone();
    forged.splice(8..8, 98u32.to_le_bytes());
    let path = dir.path().join("forged.rz");
    std::fs::write(&path, &forged).unwrap();
    let Err(message) = PrecisionDocument::open_native(path.to_str().unwrap(), LIMIT, 0, None)
    else {
        panic!("the word without a texture must refuse");
    };
    assert_eq!(message, "invalid native texture word");
    // With noise too, 98 comes before 97.
    let both = set_style(
        &typed_doc(),
        &json!({"effects":[bevel(),{"type":"inner_glow","noise":0.2}]}),
    );
    let path = dir.path().join("both.rz");
    both.save_native(path.to_str().unwrap(), LIMIT, 0, None)
        .unwrap();
    let bytes = std::fs::read(&path).unwrap();
    assert_eq!(&bytes[8..12], &98u32.to_le_bytes());
    assert_eq!(&bytes[12..16], &97u32.to_le_bytes(), "98 ahead of 97");
}

#[test]
fn a_cmyk_file_claims_word_98_after_its_rung() {
    let dir = TempDir::new().unwrap();
    let doc = styled(cmyk_base(16), 1, &style(bevel()));
    let path = dir.path().join("cmyk.rz");
    unsafe { &*doc }
        .save_native(path.to_str().unwrap(), LIMIT, 0, None)
        .unwrap();
    let bytes = std::fs::read(&path).unwrap();
    assert_eq!(&bytes[4..8], &46u32.to_le_bytes());
    let rung = u32::from_le_bytes(bytes[8..12].try_into().unwrap());
    assert!((82..=88).contains(&rung), "the CMYK rung, got {rung}");
    assert_eq!(&bytes[12..16], &98u32.to_le_bytes(), "98 after the rung");
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
        panic!("a CMYK texture without the word must refuse");
    };
    assert_eq!(message, "native bevel texture requires DRDC 98");
    free(reopened);
    free(doc);
}
