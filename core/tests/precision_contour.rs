//! Layer style CONTOURS at 16 bits, at 32 bits and in
//! CMYK ink, and the typed `.rz` word 96. Every depth renders its effects
//! through the one u8 plane renderer, so each oracle restates the contour
//! as the test's own table `L` (the take's measured presets from
//! `fixtures/psd_photoshop/contour/tables.json`, the glows' Range rule and
//! the Below cap, written here) applied to the Linear twin's coverage at
//! the same depth:
//!
//! - 16 bits: coverage `a = (65535 − R) / 257` of black over white, and
//!   the render within one 8-bit code of the byte document's;
//! - 32 bits: coverage `a = 255 (1 − R)`, the float composite being linear;
//! - CMYK: each plate of the contoured effect is its ink at full coverage
//!   times `L[a] / 255`, `a` read from the Linear effect's RGB16 twin
//!   (`cmyk_style_fixture::twin`).
//!
//! The typed file claims word 96 right after 46 (ahead of 95), a CMYK
//! file right after its rung, and both refuse the other way round.

use std::collections::BTreeMap;
use std::ffi::{c_char, CString};
use std::path::PathBuf;
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
/// The Below caps (the take's REFIT, `contour_take.py`).
const CAP_DS: f64 = 8.25;
const CAP_OG: f64 = 8.25 / 2.0;

// ----------------------------------------------------------- the oracle --

fn tables() -> BTreeMap<String, Vec<f64>> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/psd_photoshop/contour/tables.json");
    let v: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    v["tables"]
        .as_object()
        .unwrap()
        .iter()
        .map(|(k, t)| {
            let values = t.as_array().unwrap().iter().map(|v| v.as_f64().unwrap());
            (k.clone(), values.collect())
        })
        .collect()
}

/// `L[c] = round(min(T(255 R(c / 255)), cap 255 R(c / 255)))`, `R` the
/// glows' rule `min(1, x / 2r)`.
fn lut(values: &[f64], range: Option<f64>, cap: Option<f64>) -> Vec<f64> {
    (0..256)
        .map(|c| {
            let x = c as f64 / 255.0;
            let x = range.map_or(x, |r| (x / (2.0 * r)).min(1.0));
            let at = (255.0 * x).clamp(0.0, 255.0);
            let i = (at.floor() as usize).min(254);
            let mut v = values[i] + (at - i as f64) * (values[i + 1] - values[i]);
            if let Some(k) = cap {
                v = v.min(k * 255.0 * x);
            }
            v.round().clamp(0.0, 255.0)
        })
        .collect()
}

/// The three contoured effects of every case, with their oracle tables and
/// the region each is judged on.
fn cases() -> Vec<(&'static str, Value, Value, Vec<f64>, bool)> {
    let t = tables();
    vec![
        (
            "drop shadow, Cone - Inverted",
            json!({"type":"drop_shadow","blend":"normal","color":"#000000","opacity":1,
                "distance":0,"spread":0,"size":8,"layer_knocks_out":false,
                "use_global_light":false}),
            json!({"contour":{"preset":"cone_inverted"}}),
            lut(&t["cone_inverted"], None, Some(CAP_DS)),
            true,
        ),
        // The outer glow's box-pair law (#578, Size·(1 − Spread) > 0)
        // applies its Range in the plane before the contour, so the twin is
        // the Linear glow at the same Range and the table has none.
        (
            "outer glow, Cone at Range 25 %",
            json!({"type":"outer_glow","blend":"normal","color":"#000000","opacity":1,
                "spread":0,"size":8,"range":0.25}),
            json!({"contour":{"preset":"cone"}}),
            lut(&t["cone"], None, Some(CAP_OG)),
            true,
        ),
        (
            "inner glow, Half Round",
            json!({"type":"inner_glow","blend":"normal","color":"#000000","opacity":1,
                "choke":0,"size":8}),
            json!({"contour":{"preset":"half_round"}}),
            lut(&t["half_round"], Some(0.5), None),
            false,
        ),
    ]
}

fn with(mut effect: Value, extra: &Value) -> Value {
    for (k, v) in extra.as_object().unwrap() {
        effect[k] = v.clone();
    }
    effect
}

fn style(effect: Value) -> Value {
    json!({"fill_opacity":0,"effects":[effect]})
}

fn inside(i: usize) -> bool {
    let (x, y) = ((i as u32 % CANVAS.0) as i32, (i as u32 / CANVAS.0) as i32);
    x >= RECT.0 && y >= RECT.1 && x < RECT.0 + RECT.2 as i32 && y < RECT.1 + RECT.3 as i32
}

/// Every judged pixel's contoured coverage against `L` of its twin's,
/// within `bound` codes (both read as f64 coverage).
fn assert_oracle(twin: &[f64], got: &[f64], table: &[f64], below: bool, bound: f64, what: &str) {
    let mut n = 0;
    for (i, (&a, &g)) in twin.iter().zip(got).enumerate() {
        if below == inside(i) {
            continue;
        }
        let want = table[a.round().clamp(0.0, 255.0) as usize];
        assert!(
            (g - want).abs() <= bound,
            "{what}: pixel {i} twin {a}: {g} against {want}"
        );
        n += 1;
    }
    assert!(n > 200, "{what}: only {n} pixels judged");
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
fn sixteen_bit_contours_are_the_byte_table_on_the_linear_twin() {
    let byte_doc = rect_layer_doc(CANVAS, RECT, [0, 0, 0, 255]);
    for (what, base, extra, table, below) in cases() {
        let twin = coverage16(&set_style(&typed_doc(), &style(base.clone())));
        let doc = set_style(&typed_doc(), &style(with(base.clone(), &extra)));
        let got = coverage16(&doc);
        assert_oracle(&twin, &got, &table, below, 1.0, what);
        let bytes: Vec<f64> = styled8(&byte_doc, 1, &style(with(base, &extra)).to_string())
            .flattened()
            .unwrap()
            .pixels()
            .map(|p| 255.0 - f64::from(p.0[0]))
            .collect();
        let worst = got
            .iter()
            .zip(&bytes)
            .map(|(a, b)| (a - b).abs())
            .fold(0.0, f64::max);
        assert!(
            worst <= 1.0,
            "{what}: 16 bits {worst} codes from the byte render"
        );
    }
}

// ------------------------------------------------------------- 32 bits --

#[test]
fn thirty_two_bit_contours_are_the_byte_table_on_the_linear_twin() {
    let float = typed_doc().promote_to_float(LIMIT, 0, None).unwrap();
    assert!(float.is_float());
    let coverage = |doc: &PrecisionDocument| -> Vec<f64> {
        let px = doc.render(LIMIT, 0, None).unwrap().to_rgba32().unwrap();
        px.iter().map(|p| 255.0 * (1.0 - f64::from(p[0]))).collect()
    };
    for (what, base, extra, table, below) in cases() {
        let twin = coverage(&set_style(&float, &style(base.clone())));
        let got = coverage(&set_style(&float, &style(with(base, &extra))));
        assert_oracle(&twin, &got, &table, below, 1.0, what);
    }
}

// ----------------------------------------------------------------- CMYK --

/// A CMYK document at `depth`: paper under one layer holding an opaque ink
/// rect at RECT.
fn cmyk_base(depth: u32) -> *mut PrecisionDocument {
    let (x0, y0) = (RECT.0 as u32, RECT.1 as u32);
    layer(
        backdrop(CANVAS.0, CANVAS.1, depth, &[[0.0; 4]]),
        "Shape",
        [10.0, 60.0, 30.0, 0.0],
        &hard_rect(CANVAS.0, CANVAS.1, (x0, y0, x0 + RECT.2, y0 + RECT.3)),
    )
}

fn cmyk_flat(depth: u32, style: &Value) -> Vec<[u16; 5]> {
    let doc = styled(cmyk_base(depth), 1, style);
    let out = flat(doc);
    free(doc);
    out
}

#[test]
fn a_cmyk_contour_mixes_the_twins_full_ink_by_the_table() {
    let (x0, y0) = (RECT.0 as u32, RECT.1 as u32);
    let alpha = hard_rect(CANVAS.0, CANVAS.1, (x0, y0, x0 + RECT.2, y0 + RECT.3));
    for depth in [8, 16] {
        // Each effect prints an explicit ink (fractions), at full coverage
        // `full` on each plate.
        let ink = [0.6, 0.2, 0.1, 0.5];
        let full = ink.map(|f: f64| 65535.0 * f);
        for (what, base, extra, table, below) in cases() {
            // The coverage `a` of each pixel: the RGB16 twin's (module doc).
            let coverage = twin(
                CANVAS.0,
                CANVAS.1,
                &alpha,
                &twin_effect(&base, None),
                (120.0, 30.0),
            );
            let base = with(base, &json!({"ink": ink}));
            let got = cmyk_flat(depth, &style(with(base, &extra)));
            let mut n = 0;
            for (i, (&t, g)) in coverage.iter().zip(&got).enumerate() {
                if below == inside(i) {
                    continue;
                }
                let a = (255.0 * t).round();
                let l = table[a.clamp(0.0, 255.0) as usize];
                for c in 0..4 {
                    let want = full[c] * l / 255.0;
                    let e = (f64::from(g[c]) - want).abs() / 257.0;
                    assert!(
                        e <= 1.0,
                        "{depth}-bit {what}: pixel {i} plate {c}: {g:?} against {want} (a {a})"
                    );
                }
                n += 1;
            }
            assert!(n > 200, "{depth}-bit {what}: only {n} pixels judged");
        }
    }
}

// --------------------------------------------------------------- .rz 96 --

fn contoured() -> Value {
    style(json!({"type":"outer_glow","contour":{"preset":"ring"},"range":0.4}))
}

#[test]
fn the_typed_word_96_round_trips_and_refuses_both_ways() {
    let dir = TempDir::new().unwrap();
    let plain = dir.path().join("plain.rz");
    set_style(&typed_doc(), &style(json!({"type":"outer_glow"})))
        .save_native(plain.to_str().unwrap(), LIMIT, 0, None)
        .unwrap();
    let plain = std::fs::read(&plain).unwrap();
    assert_ne!(
        &plain[8..12],
        &96u32.to_le_bytes(),
        "no 96 without a contour"
    );
    let doc = set_style(&typed_doc(), &contoured());
    let path = dir.path().join("contour.rz");
    let path = path.to_str().unwrap();
    doc.save_native(path, LIMIT, 0, None).unwrap();
    let bytes = std::fs::read(path).unwrap();
    assert_eq!(&bytes[4..8], &46u32.to_le_bytes());
    assert_eq!(&bytes[8..12], &96u32.to_le_bytes(), "96 after 46");
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
        panic!("a contour without the word must refuse");
    };
    assert_eq!(message, "native layer style contours require DRDC 96");
    let mut forged = plain.clone();
    forged.splice(8..8, 96u32.to_le_bytes());
    let path = dir.path().join("forged.rz");
    std::fs::write(&path, &forged).unwrap();
    let Err(message) = PrecisionDocument::open_native(path.to_str().unwrap(), LIMIT, 0, None)
    else {
        panic!("the word without a contour must refuse");
    };
    assert_eq!(message, "invalid native contour word");
}

#[test]
fn a_cmyk_file_claims_word_96_after_its_rung() {
    let dir = TempDir::new().unwrap();
    let doc = styled(cmyk_base(16), 1, &contoured());
    let path = dir.path().join("cmyk.rz");
    unsafe { &*doc }
        .save_native(path.to_str().unwrap(), LIMIT, 0, None)
        .unwrap();
    let bytes = std::fs::read(&path).unwrap();
    assert_eq!(&bytes[4..8], &46u32.to_le_bytes());
    let rung = u32::from_le_bytes(bytes[8..12].try_into().unwrap());
    assert!((82..=88).contains(&rung), "the CMYK rung, got {rung}");
    assert_eq!(&bytes[12..16], &96u32.to_le_bytes(), "96 after the rung");
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
        panic!("a CMYK contour without the word must refuse");
    };
    assert_eq!(message, "native layer style contours require DRDC 96");
    free(reopened);
    free(doc);
}
