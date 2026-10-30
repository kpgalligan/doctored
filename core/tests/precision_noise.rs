//! Layer style NOISE at 16 bits, at 32 bits and in
//! CMYK ink, and the typed `.rz` word 97. Every depth renders its effects
//! through the one u8 plane renderer, so a noisy effect at 16 or 32 bits is
//! the byte document's within one 8-bit code (coverage `a = (65535 − R) /
//! 257` at 16 bits, `255 (1 − R)` at 32, the float composite being linear),
//! and a CMYK effect printing an explicit ink is that ink at full coverage
//! times `N / 255`, `N` this file's own restatement of the law at the
//! coverage `a` of the Noise 0 RGB16 twin (`cmyk_style_fixture::twin`):
//! SplitMix64 at (the seed, the layer coordinate, the effect kind's
//! render-order index), `round(clamp(a + 255 n (2u − 1)))`, 0 at `a` 0, held
//! on a Below effect to `round(min(255, k a))` (k 8.25 on the drop shadow).
//!
//! The typed file claims word 97 right after 46 (ahead of 96), a CMYK file
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
use crate::common::{rect_layer_doc, styled as styled8, take_err_string};

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
const CANVAS: (u32, u32) = (96, 64);
const RECT: (i32, i32, u32, u32) = (24, 16, 48, 32);

// ----------------------------------------------------------- the oracle --

const SEED: u64 = 0x525A_204E_4F49_5345;

fn unit(stream: u64, lx: i64, ly: i64) -> f64 {
    let state = SEED
        ^ (lx as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)
        ^ (ly as u64).wrapping_mul(0xBF58_476D_1CE4_E5B9)
        ^ stream.wrapping_mul(0x94D0_49BB_1331_11EB);
    let mut z = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^= z >> 31;
    (z >> 40) as f64 / (1u64 << 24) as f64
}

/// The law at coverage `a`, held to `cap a` when `cap` is given.
fn noise(a: u8, n: f64, u: f64, cap: Option<f64>) -> f64 {
    if a == 0 {
        return 0.0;
    }
    let v = (f64::from(a) + 255.0 * n * (2.0 * u - 1.0))
        .clamp(0.0, 255.0)
        .round();
    cap.map_or(v, |k| v.min((k * f64::from(a)).min(255.0).round()))
}

/// The four noisy effects: (what, effect, stream, cap).
fn cases() -> Vec<(&'static str, Value, u64, Option<f64>)> {
    vec![
        (
            "drop shadow",
            json!({"type":"drop_shadow","blend":"normal","color":"#000000","opacity":1,
                "distance":0,"spread":0,"size":8,"layer_knocks_out":false,
                "use_global_light":false,"noise":0.5}),
            0,
            Some(8.25),
        ),
        (
            "outer glow",
            json!({"type":"outer_glow","blend":"normal","color":"#000000","opacity":1,
                "spread":0,"size":8,"noise":0.25}),
            1,
            Some(8.25 / 2.0),
        ),
        (
            "inner glow",
            json!({"type":"inner_glow","blend":"normal","color":"#000000","opacity":1,
                "choke":0,"size":8,"noise":1}),
            7,
            None,
        ),
        (
            "inner shadow",
            json!({"type":"inner_shadow","blend":"normal","color":"#000000","opacity":1,
                "choke":0,"distance":0,"size":8,"use_global_light":false,"noise":0.5}),
            8,
            None,
        ),
    ]
}

fn style(effect: Value) -> Value {
    json!({"fill_opacity":0,"effects":[effect]})
}

fn byte_coverage(effect: &Value) -> Vec<f64> {
    let byte_doc = rect_layer_doc(CANVAS, RECT, [0, 0, 0, 255]);
    styled8(&byte_doc, 1, &style(effect.clone()).to_string())
        .flattened()
        .unwrap()
        .pixels()
        .map(|p| 255.0 - f64::from(p.0[0]))
        .collect()
}

fn assert_within(got: &[f64], want: &[f64], what: &str) {
    let mut differ = 0;
    for (i, (g, w)) in got.iter().zip(want).enumerate() {
        assert!((g - w).abs() <= 1.0, "{what}: pixel {i}: {g} against {w}");
        differ += usize::from(*w > 0.0);
    }
    assert!(differ > 200, "{what}: only {differ} pixels drawn");
}

// ------------------------------------------------------------- 16 bits --

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

/// The scene at 16 bits: white canvas, a black rect at RECT.
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

fn coverage16(doc: &PrecisionDocument) -> Vec<f64> {
    let px = doc.render(LIMIT, 0, None).unwrap().to_rgba16().unwrap();
    px.iter()
        .map(|p| (65535.0 - f64::from(p[0])) / 257.0)
        .collect()
}

#[test]
fn sixteen_bit_noise_is_the_byte_render() {
    for (what, effect, _, _) in cases() {
        let got = coverage16(&set_style(&typed_doc(), &style(effect.clone())));
        assert_within(&got, &byte_coverage(&effect), what);
    }
}

#[test]
fn thirty_two_bit_noise_is_the_byte_render() {
    let float = typed_doc().promote_to_float(LIMIT, 0, None).unwrap();
    assert!(float.is_float());
    for (what, effect, _, _) in cases() {
        let px = set_style(&float, &style(effect.clone()))
            .render(LIMIT, 0, None)
            .unwrap()
            .to_rgba32()
            .unwrap();
        let got: Vec<f64> = px.iter().map(|p| 255.0 * (1.0 - f64::from(p[0]))).collect();
        assert_within(&got, &byte_coverage(&effect), what);
    }
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
fn a_cmyk_noise_mixes_its_full_ink_by_the_law() {
    let (x0, y0) = (RECT.0 as u32, RECT.1 as u32);
    let alpha = hard_rect(CANVAS.0, CANVAS.1, (x0, y0, x0 + RECT.2, y0 + RECT.3));
    let ink = [0.6, 0.2, 0.1, 0.5];
    let full = ink.map(|f: f64| 65535.0 * f);
    for depth in [8, 16] {
        for (what, effect, stream, cap) in cases() {
            let mut plain = effect.clone();
            plain.as_object_mut().unwrap().remove("noise");
            let n = effect["noise"].as_f64().unwrap();
            let coverage = twin(
                CANVAS.0,
                CANVAS.1,
                &alpha,
                &twin_effect(&plain, None),
                (120.0, 30.0),
            );
            let mut inked = effect.clone();
            inked["ink"] = json!(ink);
            let doc = styled(cmyk_base(depth), 1, &style(inked));
            let got = flat(doc);
            free(doc);
            let mut drawn = 0;
            for (i, (&t, g)) in coverage.iter().zip(&got).enumerate() {
                let a = (255.0 * t).round().clamp(0.0, 255.0) as u8;
                let (x, y) = ((i as u32 % CANVAS.0) as i64, (i as u32 / CANVAS.0) as i64);
                let level = noise(a, n, unit(stream, x, y), cap);
                for c in 0..4 {
                    let want = full[c] * level / 255.0;
                    let e = (f64::from(g[c]) - want).abs() / 257.0;
                    assert!(
                        e <= 1.0,
                        "{depth}-bit {what}: pixel {i} plate {c}: {g:?} against {want} (a {a})"
                    );
                }
                drawn += usize::from(level > 0.0);
            }
            assert!(drawn > 200, "{depth}-bit {what}: only {drawn} pixels drawn");
        }
    }
}

// --------------------------------------------------------------- .rz 97 --

fn noisy() -> Value {
    style(json!({"type":"inner_shadow","noise":0.35}))
}

#[test]
fn the_typed_word_97_round_trips_and_refuses_both_ways() {
    let dir = TempDir::new().unwrap();
    let plain = dir.path().join("plain.rz");
    set_style(&typed_doc(), &style(json!({"type":"inner_shadow"})))
        .save_native(plain.to_str().unwrap(), LIMIT, 0, None)
        .unwrap();
    let plain = std::fs::read(&plain).unwrap();
    assert_ne!(&plain[8..12], &97u32.to_le_bytes(), "no 97 without noise");
    let doc = set_style(&typed_doc(), &noisy());
    let path = dir.path().join("noise.rz");
    let path = path.to_str().unwrap();
    doc.save_native(path, LIMIT, 0, None).unwrap();
    let bytes = std::fs::read(path).unwrap();
    assert_eq!(&bytes[4..8], &46u32.to_le_bytes());
    assert_eq!(&bytes[8..12], &97u32.to_le_bytes(), "97 after 46");
    assert_eq!(&bytes[12..16], &plain[8..12], "then the plain word");
    let reopened = PrecisionDocument::open_native(path, LIMIT, 0, None).unwrap();
    assert_eq!(
        reopened.layer(1).unwrap().reported_style(),
        doc.layer(1).unwrap().reported_style()
    );
    assert_eq!(coverage16(&reopened), coverage16(&doc));
    let mut stripped = bytes.clone();
    stripped.drain(8..12);
    let cut = dir.path().join("cut.rz");
    std::fs::write(&cut, &stripped).unwrap();
    let Err(message) = PrecisionDocument::open_native(cut.to_str().unwrap(), LIMIT, 0, None) else {
        panic!("noise without the word must refuse");
    };
    assert_eq!(message, "native layer style noise requires DRDC 97");
    let mut forged = plain.clone();
    forged.splice(8..8, 97u32.to_le_bytes());
    let path = dir.path().join("forged.rz");
    std::fs::write(&path, &forged).unwrap();
    let Err(message) = PrecisionDocument::open_native(path.to_str().unwrap(), LIMIT, 0, None)
    else {
        panic!("the word without noise must refuse");
    };
    assert_eq!(message, "invalid native noise word");
    // With a contour too, 97 comes before 96.
    let both = set_style(
        &typed_doc(),
        &style(json!({"type":"outer_glow","noise":0.2,"contour":{"preset":"cone"}})),
    );
    let path = dir.path().join("both.rz");
    both.save_native(path.to_str().unwrap(), LIMIT, 0, None)
        .unwrap();
    let bytes = std::fs::read(&path).unwrap();
    assert_eq!(&bytes[8..12], &97u32.to_le_bytes());
    assert_eq!(&bytes[12..16], &96u32.to_le_bytes(), "97 ahead of 96");
}

#[test]
fn a_cmyk_file_claims_word_97_after_its_rung() {
    let dir = TempDir::new().unwrap();
    let doc = styled(cmyk_base(16), 1, &noisy());
    let path = dir.path().join("cmyk.rz");
    unsafe { &*doc }
        .save_native(path.to_str().unwrap(), LIMIT, 0, None)
        .unwrap();
    let bytes = std::fs::read(&path).unwrap();
    assert_eq!(&bytes[4..8], &46u32.to_le_bytes());
    let rung = u32::from_le_bytes(bytes[8..12].try_into().unwrap());
    assert!((82..=88).contains(&rung), "the CMYK rung, got {rung}");
    assert_eq!(&bytes[12..16], &97u32.to_le_bytes(), "97 after the rung");
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
        panic!("CMYK noise without the word must refuse");
    };
    assert_eq!(message, "native layer style noise requires DRDC 97");
    free(reopened);
    free(doc);
}
