//! Image ▸ Adjustments, Auto and Invert on one plane of a 32-bit float
//! document, black-box through `scalar_edit` (`dr_precision_doc_edit` and its
//! plan) and `dr_precision_doc_scalar_auto_levels`.
//!
//! A layer mask and a saved alpha channel are u16 coverage at every depth, so
//! a float document edits them code for code as a 16-bit document does. The
//! oracle is a 16-bit TWIN document holding the same mask and channel codes:
//! every request's result must equal the twin's exactly, and the float
//! layer's samples must keep their bits.
//!
//! - AUTO. The host's two requests, the derive and then `levels_channels`
//!   with the derived values, on the mask and on the channel, for each of
//!   Auto Tone, Auto Contrast and Auto Color. The codes span about
//!   8000…52000, so the derive is not the identity.
//! - PINS (green before Auto's write was admitted, labelled so): the Invert,
//!   Levels and Threshold adjustments and the morphology invert.
//!
//! One Red, Green, Blue or Alpha component of a float layer takes
//! `float_plane_adjust`'s rule. The layer holds values below 0, above 1 and
//! alphas 0, 0.5 and 1. Each bound is declared here before the first run and
//! never widened:
//!
//! - COLOUR TWIN. Levels, Exposure, Threshold (a display form) and a
//!   Hue/Saturation Colorize on Red, Green and Blue equal the layer's own
//!   apply (`dr_precision_doc_selected_edit`) on a twin float layer of
//!   `[v, v, v, 1]`, its result `r` when `r == g == b` bit for bit, else the
//!   Rec. 709 luma in f64 rounded once: the plane's bits EXACTLY; every other
//!   component bit for bit; Exposure keeps a value above white above white.
//! - ALPHA INVERT. Every alpha becomes `1.0f32 − a` bit for bit; RGB is kept.
//! - ALPHA TONE. Levels and Shadows/Highlights on the Alpha of an alpha ramp
//!   against the same request on a 16-bit mask of `round(65535·a)`: within
//!   T_ALPHA_CODES codes of 65535.
//! - GATE. Weight 0 keeps the bits, 65535 takes the full result's, between is
//!   `v0·(1−t) + v1·t` in f64 rounded once (exact).
//! - REFUSALS. Invert, Shadows/Highlights and a legacy White Balance on Red
//!   give the layer's sentence; each Auto, and `levels_channels`, on Red and
//!   on Alpha gives its named sentence; in the plan and in the edit (and the
//!   derive for Auto).
//! - PLAN. A Red adjustment's scratch is at most the whole layer's apply
//!   plan for the same description.
use std::ffi::{CStr, CString};
use std::ptr;

use doctored_core::ffi_precision_edit::dr_precision_doc_scalar_auto_levels;
use doctored_core::ffi_selected_adjustment::{
    dr_precision_doc_selected_edit, dr_precision_doc_selected_edit_plan,
};
use doctored_core::precision_document::PrecisionDocument;
use serde_json::{json, Value};

use crate::float_plane_filters::{bits, float_document, layer32, try_edit, try_plan, with_planes};
use crate::native_scalar as native;
use native::LIMIT;

const W: u32 = 20;
const H: u32 = 14;
/// ALPHA TONE's bound, in codes of 65535.
const T_ALPHA_CODES: i64 = 3;
/// `blend.rs`'s f32 luma constants.
const LUMA: [f32; 3] = [0.2126, 0.7152, 0.0722];
const COLORS: [&str; 3] = ["red", "green", "blue"];

// ------------------------------------------------------------- fixtures --

/// Codes spread over about 8000…52000, so Auto stretches them.
fn mid_codes(seed: u32) -> Vec<u16> {
    (0..W * H)
        .map(|i| (8000 + (i * 2741 + seed * 977) % 44001) as u16)
        .collect()
}

/// Straight float samples below 0 and above 1, opaque.
fn layer_samples() -> Vec<[f32; 4]> {
    (0..W * H)
        .map(|i| {
            let (x, y) = (i % W, i / W);
            [
                -0.3 + 0.17 * x as f32,
                2.0 * (y as f32) / (H as f32) - 0.2,
                0.05 * ((x + y) % 7) as f32,
                1.0,
            ]
        })
        .collect()
}

/// The float document under test and its 16-bit twin, each with the same
/// mask and channel codes.
fn pair() -> (PrecisionDocument, PrecisionDocument) {
    let (mask, channel) = (mid_codes(1), mid_codes(5));
    let float = with_planes(&float_document(W, H, &layer_samples()), &mask, &channel);
    let n = (W * H) as usize;
    let twin = with_planes(
        &native::doc(W, H, vec![[1000, 2000, 3000, 65535]; n]),
        &mask,
        &channel,
    );
    (float, twin)
}

fn filter(target: &str, op: &str, params: Value) -> Value {
    json!({"op":"scalar_edit","target":target,"index":0,"selection":false,
        "operation":{"kind":"filter","edit":{"layer":0,"op":op,"params":params}}})
}

fn adjustment(target: &str, op: &str, params: Value) -> Value {
    let description =
        json!({"type":"adjust","evaluation":"precision_v1","op":op,"params":params}).to_string();
    json!({"op":"scalar_edit","target":target,"index":0,"selection":false,
        "operation":{"kind":"filter","edit":{"layer":0,"op":"adjustment","params":{},
            "description":description}}})
}

/// The derive the host sends first (`PrecisionScalarAuto`).
fn derive(d: &PrecisionDocument, v: &Value) -> Result<Value, String> {
    let t = native::text(v.clone());
    let mut e = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_scalar_auto_levels(
            d,
            t.as_ptr(),
            ptr::null(),
            0,
            LIMIT,
            0,
            ptr::null(),
            &mut e,
        )
    };
    if out.is_null() {
        return Err(unsafe { native::error(e) });
    }
    let s = unsafe { CStr::from_ptr(out) }
        .to_string_lossy()
        .into_owned();
    unsafe { doctored_core::ffi::dr_string_free(out) };
    Ok(serde_json::from_str(&s).unwrap())
}

/// Auto as the host runs it: the derive, then `levels_channels` with the
/// derived values.
fn auto(
    d: &PrecisionDocument,
    target: &str,
    op: &str,
) -> Result<Option<PrecisionDocument>, String> {
    let derived = derive(d, &filter(target, op, json!({"clip": 0.001})))?;
    assert_eq!(derived["identity"], false, "{target} {op}: {derived}");
    let params =
        json!({"black":derived["black"],"white":derived["white"],"gamma":derived["gamma"]});
    try_edit(d, &filter(target, "levels_channels", params), &[])
}

fn target_codes(d: &PrecisionDocument, target: &str) -> Vec<u16> {
    native::plane(d, target, false)
}

// ------------------------------------------------------------------ tests --

#[test]
fn auto_on_a_float_mask_or_channel_is_the_16_bit_twin() {
    let (float, twin) = pair();
    let layer = bits(&layer32(&float));
    for target in ["mask", "channel"] {
        for op in ["auto_tone", "auto_contrast", "auto_color"] {
            let want = auto(&twin, target, op)
                .unwrap_or_else(|e| panic!("twin {target} {op}: {e}"))
                .expect("the twin stretches");
            let got = auto(&float, target, op)
                .unwrap_or_else(|e| panic!("float {target} {op}: {e}"))
                .expect("the float plane stretches");
            let codes = target_codes(&got, target);
            assert_eq!(codes, target_codes(&want, target), "{target} {op}");
            assert!(
                codes.contains(&0) && codes.contains(&65535),
                "{target} {op}"
            );
            assert_eq!(
                bits(&layer32(&got)),
                layer,
                "{target} {op}: the layer keeps its bits"
            );
        }
    }
}

#[test]
fn pin_adjustments_and_invert_on_a_float_mask_or_channel_are_the_16_bit_twin() {
    let (float, twin) = pair();
    let layer = bits(&layer32(&float));
    let requests = |target: &str| {
        vec![
            adjustment(target, "invert", json!({})),
            adjustment(
                target,
                "levels",
                json!({"black": 0.2, "white": 0.7, "gamma": 1.3}),
            ),
            adjustment(target, "threshold", json!({"level": 0.45})),
            json!({"op":"scalar_edit","target":target,"index":0,"selection":false,
                "operation":{"kind":"morphology","operation":{"kind":"invert"}}}),
        ]
    };
    for target in ["mask", "channel"] {
        for v in requests(target) {
            let want = try_edit(&twin, &v, &[])
                .unwrap_or_else(|e| panic!("twin {v}: {e}"))
                .expect("the twin changes");
            let got = try_edit(&float, &v, &[])
                .unwrap_or_else(|e| panic!("float {v}: {e}"))
                .expect("the float plane changes");
            assert_eq!(
                target_codes(&got, target),
                target_codes(&want, target),
                "{v}"
            );
            assert_eq!(bits(&layer32(&got)), layer, "{v}: the layer keeps its bits");
        }
    }
    let inverted = try_edit(&float, &adjustment("mask", "invert", json!({})), &[])
        .unwrap()
        .unwrap();
    let before = target_codes(&float, "mask");
    let after = target_codes(&inverted, "mask");
    assert!(before.iter().zip(&after).all(|(b, a)| *a == 65535 - *b));
}

// ------------------------------------------------- one float colour plane --

/// Straight float samples below 0, above 1 and alpha 0, 0.5 and 1.
fn extended() -> Vec<[f32; 4]> {
    (0..W * H)
        .map(|i| {
            let (x, y) = (i % W, i / W);
            let r = -0.4 + 0.13 * x as f32 + 0.3 * ((x * 7 + y * 3) % 5) as f32;
            let g = 2.5 * ((x * 13 + y * 29) % 11) as f32 / 11.0 - 0.3;
            let b = 0.01 * (y * y) as f32 - 0.2 + 0.2 * ((x + 2 * y) % 3) as f32;
            [r, g, b, [0.0, 0.5, 1.0][((x + 2 * y) % 3) as usize]]
        })
        .collect()
}

fn component(pixels: &[[f32; 4]], c: usize) -> Vec<f32> {
    pixels.iter().map(|p| p[c]).collect()
}

/// The plane rule's Result, restated: `r` when the three agree bit for bit,
/// else the Rec. 709 luma in f64 rounded once.
fn rule(p: [f32; 4]) -> f32 {
    if p[0].to_bits() == p[1].to_bits() && p[1].to_bits() == p[2].to_bits() {
        return p[0];
    }
    (f64::from(LUMA[0]) * f64::from(p[0])
        + f64::from(LUMA[1]) * f64::from(p[1])
        + f64::from(LUMA[2]) * f64::from(p[2])) as f32
}

fn description(op: &str, params: &Value) -> String {
    json!({"type":"adjust","evaluation":"precision_v1","op":op,"params":params}).to_string()
}

fn layer_request(op: &str, params: &Value) -> CString {
    native::text(json!({"layer":0,"op":"adjustment","description":description(op, params)}))
}

/// The layer's own destructive apply on layer 0, unselected; `None` is the
/// no-op answer.
fn layer_apply(d: &PrecisionDocument, op: &str, params: &Value) -> Option<PrecisionDocument> {
    let mut e = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_selected_edit(
            d,
            layer_request(op, params).as_ptr(),
            ptr::null(),
            0,
            LIMIT,
            0,
            ptr::null(),
            &mut e,
        )
    };
    let error = unsafe { native::error(e) };
    assert!(error.is_empty(), "layer {op}: {error}");
    (!out.is_null()).then(|| *unsafe { Box::from_raw(out) })
}

fn layer_plan(d: &PrecisionDocument, op: &str, params: &Value) -> u64 {
    let mut e = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_selected_edit_plan(d, layer_request(op, params).as_ptr(), LIMIT, 0, &mut e)
    };
    assert!(!out.is_null(), "{}", unsafe { native::error(e) });
    let s = unsafe { CStr::from_ptr(out) }
        .to_string_lossy()
        .into_owned();
    unsafe { doctored_core::ffi::dr_string_free(out) };
    serde_json::from_str::<Value>(&s).unwrap()["scratch_bytes"]
        .as_u64()
        .unwrap()
}

fn plane_adjustment(target: &str, op: &str, params: &Value, selection: bool) -> Value {
    let mut v = adjustment(target, op, params.clone());
    v["selection"] = json!(selection);
    v
}

fn sentence(operator: &str) -> String {
    format!("{operator} is unavailable in a 32-bit float document")
}

#[test]
fn a_colour_plane_adjustment_is_the_layer_form_on_its_gray_twin() {
    let d = float_document(W, H, &extended());
    let before = layer32(&d);
    let mut checked = 0;
    for (op, params) in [
        (
            "levels",
            json!({"black": 0.1, "white": 0.8, "gamma": 1.3, "output_white": 0.95}),
        ),
        ("exposure", json!({"exposure": 1.0})),
        ("threshold", json!({"level": 0.45})),
        (
            "hue_saturation",
            json!({"colorize": true, "colorize_hue": 310, "colorize_saturation": 0.8}),
        ),
    ] {
        for (c, target) in COLORS.iter().enumerate() {
            let values = component(&before, c);
            let gray: Vec<[f32; 4]> = values.iter().map(|&v| [v, v, v, 1.0]).collect();
            let want: Vec<f32> = layer32(
                &layer_apply(&float_document(W, H, &gray), op, &params)
                    .unwrap_or_else(|| panic!("{target} {op}: the twin changes")),
            )
            .into_iter()
            .map(rule)
            .collect();
            let v = adjustment(target, op, params.clone());
            assert_eq!(
                try_plan(&d, &v).unwrap()["input_samples"],
                0,
                "{target} {op}"
            );
            let got = layer32(
                &try_edit(&d, &v, &[])
                    .unwrap_or_else(|e| panic!("{target} {op}: {e}"))
                    .expect("the plane changes"),
            );
            for i in 0..before.len() {
                assert_eq!(
                    got[i][c].to_bits(),
                    want[i].to_bits(),
                    "{target} {op} pixel {i}: {} vs {}",
                    got[i][c],
                    want[i]
                );
                for k in (0..4).filter(|k| *k != c) {
                    assert_eq!(got[i][k].to_bits(), before[i][k].to_bits(), "{target} {op}");
                }
            }
            if op == "exposure" {
                assert!(
                    (0..before.len()).any(|i| before[i][c] > 1.0 && got[i][c] > 1.0),
                    "{target}: a value above white stays above white"
                );
            }
            checked += 1;
        }
    }
    assert_eq!(checked, 12);
}

#[test]
fn invert_on_the_float_alpha_is_one_minus_a() {
    let d = float_document(W, H, &extended());
    let before = layer32(&d);
    let got = layer32(
        &try_edit(&d, &adjustment("alpha", "invert", json!({})), &[])
            .unwrap()
            .expect("the alpha changes"),
    );
    for i in 0..before.len() {
        assert_eq!(
            got[i][3].to_bits(),
            (1.0f32 - before[i][3]).to_bits(),
            "{i}"
        );
        for k in 0..3 {
            assert_eq!(got[i][k].to_bits(), before[i][k].to_bits(), "{i} {k}");
        }
    }
}

#[test]
fn a_tone_on_the_float_alpha_is_the_16_bit_mask_within_its_bound() {
    let n = (W * H) as usize;
    let ramp: Vec<[f32; 4]> = (0..n)
        .map(|i| {
            let a = ((i * 7919) % n) as f32 / (n - 1) as f32;
            [0.5 + i as f32 * 0.01, -0.2, 3.0, a]
        })
        .collect();
    let d = float_document(W, H, &ramp);
    let alphas = component(&layer32(&d), 3);
    let codes: Vec<u16> = alphas
        .iter()
        .map(|&a| (f64::from(a) * 65535.0).round() as u16)
        .collect();
    let twin = with_planes(
        &native::doc(W, H, vec![[1000, 2000, 3000, 65535]; n]),
        &codes,
        &codes,
    );
    for (op, params) in [
        ("levels", json!({"black": 0.2, "white": 0.7, "gamma": 1.3})),
        (
            "shadows_highlights",
            json!({"shadows": {"amount": 0.35}, "highlights": {"amount": 0.2}}),
        ),
    ] {
        let want = target_codes(
            &try_edit(&twin, &adjustment("mask", op, params.clone()), &[])
                .unwrap()
                .expect("the mask changes"),
            "mask",
        );
        let got = layer32(
            &try_edit(&d, &adjustment("alpha", op, params.clone()), &[])
                .unwrap_or_else(|e| panic!("alpha {op}: {e}"))
                .expect("the alpha changes"),
        );
        let mut worst = 0;
        for i in 0..n {
            let a = got[i][3];
            assert!((0.0..=1.0).contains(&a), "{op} {i}: {a}");
            let code = (f64::from(a) * 65535.0).round() as i64;
            worst = worst.max((code - i64::from(want[i])).abs());
            assert_eq!(got[i][0].to_bits(), ramp[i][0].to_bits(), "{op}: RGB kept");
        }
        assert!(worst <= T_ALPHA_CODES, "{op}: {worst} codes");
    }
}

#[test]
fn the_selection_gates_a_plane_adjustment_at_its_ends_and_lerps_between() {
    let d = float_document(W, H, &extended());
    let before = layer32(&d);
    let n = (W * H) as usize;
    let weights: Vec<u16> = (0..n).map(|i| [0, 65535, 20000, 47000][i % 4]).collect();
    let params = json!({"exposure": 0.7});
    for (c, target) in [(0, "red"), (3, "alpha")] {
        let op = if c == 3 { "levels" } else { "exposure" };
        let params = if c == 3 {
            json!({"black": 0.1, "gamma": 0.8})
        } else {
            params.clone()
        };
        let full = layer32(
            &try_edit(&d, &plane_adjustment(target, op, &params, false), &[])
                .unwrap()
                .unwrap(),
        );
        let v = plane_adjustment(target, op, &params, true);
        assert_eq!(try_plan(&d, &v).unwrap()["input_samples"], n);
        let got = layer32(&try_edit(&d, &v, &weights).unwrap().unwrap());
        for i in 0..n {
            let want = match weights[i] {
                0 => before[i][c],
                65535 => full[i][c],
                w => {
                    let t = f64::from(w) / 65535.0;
                    (f64::from(before[i][c]) * (1.0 - t) + f64::from(full[i][c]) * t) as f32
                }
            };
            assert_eq!(got[i][c].to_bits(), want.to_bits(), "{target} {i}");
        }
    }
}

#[test]
fn plane_adjustments_refuse_by_name() {
    let d = float_document(W, H, &extended());
    let layer = [
        ("invert", json!({}), sentence("the invert adjustment")),
        (
            "shadows_highlights",
            json!({"shadows": {"amount": 0.35}}),
            sentence("the shadows_highlights adjustment"),
        ),
        (
            "white_balance",
            json!({"temperature": 5000, "evaluation": "legacy_srgb"}),
            sentence("the white_balance adjustment with legacy evaluation"),
        ),
    ];
    for (op, params, want) in &layer {
        let v = adjustment("red", op, params.clone());
        assert_eq!(try_plan(&d, &v).err().as_ref(), Some(want), "red {op} plan");
        assert_eq!(
            try_edit(&d, &v, &[]).err().as_ref(),
            Some(want),
            "red {op} edit"
        );
    }
    let colour_auto = sentence(
        "automatic tone correction (Auto Tone, Auto Contrast, Auto Color and the Levels \
         dialog's Auto buttons)",
    );
    let alpha_auto = "Auto Tone, Auto Contrast and Auto Color are unavailable on a 32-bit \
                      float layer's Alpha: Auto reads a 16-bit histogram, which a float alpha \
                      has no form of without quantizing; run it on a layer mask or an alpha \
                      channel"
        .to_string();
    for (target, want) in [
        ("red", &colour_auto),
        ("blue", &colour_auto),
        ("alpha", &alpha_auto),
    ] {
        for op in ["auto_tone", "auto_contrast", "auto_color"] {
            let v = filter(target, op, json!({"clip": 0.001}));
            assert_eq!(
                try_plan(&d, &v).err().as_ref(),
                Some(want),
                "{target} {op} plan"
            );
            assert_eq!(
                try_edit(&d, &v, &[]).err().as_ref(),
                Some(want),
                "{target} {op} edit"
            );
            assert_eq!(
                derive(&d, &v).err().as_ref(),
                Some(want),
                "{target} {op} derive"
            );
        }
        let v = filter(
            target,
            "levels_channels",
            json!({"black": [0.1, 0.1, 0.1], "white": [0.9, 0.9, 0.9], "gamma": [1, 1, 1]}),
        );
        assert_eq!(
            try_plan(&d, &v).err().as_ref(),
            Some(want),
            "{target} levels_channels"
        );
        assert_eq!(
            try_edit(&d, &v, &[]).err().as_ref(),
            Some(want),
            "{target} levels_channels"
        );
    }
}

#[test]
fn a_red_plane_adjustment_plans_no_more_than_the_whole_layer() {
    let (w, h) = (1600u32, 1200u32);
    let pixels: Vec<[f32; 4]> = (0..w * h)
        .map(|i| [(i % 977) as f32 / 300.0 - 0.5, 0.25, 1.5, 1.0])
        .collect();
    let d = crate::float_filters::solid_doc(&crate::float_filters::Picture { w, h, pixels });
    for (op, params) in [
        ("levels", json!({"black": 0.1, "gamma": 1.2})),
        ("exposure", json!({"exposure": 1.0})),
    ] {
        let plane = try_plan(&d, &adjustment("red", op, params.clone())).unwrap()["scratch_bytes"]
            .as_u64()
            .unwrap();
        let layer = layer_plan(&d, op, &params);
        assert!(plane <= layer, "{op}: plane {plane} > layer {layer}");
    }
}
