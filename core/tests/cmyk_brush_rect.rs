//! 13B-9 on a CMYK document's inks: the retouch brushes run their two `L`
//! passes over the stroke's rect, not the layer (`cmyk_brush_rect`: Dodge
//! and Burn, the Blur, Sharpen and Sponge brushes and the History Brush over
//! the stroke's frame, Smudge over its footprint). The product path
//! (`dr_precision_doc_edit`, `dr_precision_doc_smudge`,
//! `dr_precision_doc_history_brush`) is held to the whole-layer seam
//! (`PrecisionDocument::cmyk_brush_whole_layer`), which keeps the
//! layer-sized pass documents: every case answers the same document to the
//! bit (`CmykImage::same_samples` on every layer, offsets and layer count
//! equal), or both answer nothing, or both refuse in the same sentence.
//!
//! The cases, on a canvas-sized target and on a layer smaller than the
//! canvas at an offset (partly off it), under a visible layer above: Dodge
//! and Burn with and without Protect Tones; Blur, Sharpen, Saturate,
//! Desaturate and Saturate with Vibrance; Blur and Sharpen with Sample All
//! Layers; Smudge with fractional dabs crossing the layer's edge, with
//! Sample All Layers; the History Brush from a state whose source layer is
//! smaller and at another offset. Each runs with a stroke, with a selection
//! plane, on a transparency-locked target and with a dab within the halo of
//! the layer's edge (each must change the document), and with a stroke that
//! misses the layer (nothing; with an invalid strength or opacity, the same
//! refusal).

use std::ffi::CString;
use std::ptr;

use doctored_core::doc_lock::LOCK_TRANSPARENCY;
use doctored_core::ffi_retouch_brush::{dr_precision_doc_history_brush, dr_precision_doc_smudge};
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::CmykBrushStroke;
use serde_json::{json, Value};

use crate::cmyk_structure_fixture::{cmyk_layers, free, try_edit, RgbLayer, LIMIT};
use crate::common::take_err_string;

const W: u32 = 120;
const H: u32 = 90;
const N: usize = W as usize * H as usize;
/// The offset layer: partly off the canvas's right edge.
const OFFSET: (i32, i32, u32, u32) = (70, 20, 64, 50);
/// The dab: full coverage with a two-pixel feathered ring, inside both
/// targets.
const DAB: (u32, u32, u32, u32) = (80, 34, 20, 16);
const HARD: &str =
    r#"{"version":1,"hardness":1.0,"roundness":1.0,"angle_degrees":0.0,"pixel":false}"#;

/// Samples varying in every channel, a few at partial alpha.
fn pattern(w: u32, h: u32, seed: u32) -> Vec<[u16; 4]> {
    (0..w * h)
        .map(|i| {
            let (x, y) = (i % w, i / w);
            [
                ((x * 2711 + y * 911 + seed * 1234) % 65536) as u16,
                ((x * 523 + y * 3301 + seed * 77) % 65536) as u16,
                (((x ^ y) * 1999 + seed) % 65536) as u16,
                if (x + y * 3 + seed).is_multiple_of(17) {
                    20000
                } else {
                    65535
                },
            ]
        })
        .collect()
}

/// A canvas layer, the offset layer (`offset`, `size`) and a visible layer
/// above, seeded by `seed`.
fn document(seed: u32, (x, y): (i32, i32), (w, h): (u32, u32)) -> *mut PrecisionDocument {
    cmyk_layers(
        W,
        H,
        vec![
            RgbLayer {
                name: "Canvas",
                w: W,
                h: H,
                offset: (0, 0),
                pixels: pattern(W, H, seed),
            },
            RgbLayer {
                name: "Offset",
                w,
                h,
                offset: (x, y),
                pixels: pattern(w, h, seed + 3),
            },
            RgbLayer {
                name: "Above",
                w: 60,
                h: 50,
                offset: (30, 15),
                pixels: pattern(60, 50, seed + 5),
            },
        ],
    )
}

/// Full coverage over `(x, y, w, h)` with a two-pixel feathered ring.
fn dab((x, y, w, h): (u32, u32, u32, u32)) -> Vec<u16> {
    (0..N)
        .map(|i| {
            let (px, py) = ((i % W as usize) as i64, (i / W as usize) as i64);
            let dx = (i64::from(x) - px).max(px - i64::from(x + w) + 1).max(0);
            let dy = (i64::from(y) - py).max(py - i64::from(y + h) + 1).max(0);
            match dx.max(dy) {
                0 => 65535,
                1 => 40000,
                2 => 16000,
                _ => 0,
            }
        })
        .collect()
}

/// Two dabs within the Gaussian's halo of an edge: the offset layer's
/// top-left corner and the canvas's bottom-left.
fn edge_stroke() -> Vec<u16> {
    dab((71, 21, 6, 5))
        .into_iter()
        .zip(dab((1, 84, 6, 4)))
        .map(|(a, b)| a.max(b))
        .collect()
}

/// A stroke that misses target `layer`: left of the offset layer, and
/// nothing at all on the canvas-sized one.
fn missing_stroke(layer: usize) -> Vec<u16> {
    if layer == 1 {
        dab((5, 70, 8, 8))
    } else {
        vec![0; N]
    }
}

/// A selection ramping across the canvas, zero left of x = 84.
fn selection() -> Vec<u16> {
    (0..N)
        .map(|i| {
            let x = i % W as usize;
            if x < 84 {
                0
            } else {
                ((x - 83) * 65535 / (W as usize - 83)) as u16
            }
        })
        .collect()
}

/// Fractional dabs from `(x0, y0)` to `(x1, y1)`, diameter 7.5, about 2 px
/// apart (`dr_brush_u16`'s tuples).
fn line(x0: f64, y0: f64, x1: f64, y1: f64) -> Vec<f64> {
    (0..=48)
        .flat_map(|i| {
            let t = f64::from(i) / 48.0;
            [x0 + (x1 - x0) * t, y0 + (y1 - y0) * t, 7.5, 1.0]
        })
        .collect()
}

/// Where `a` and `b` differ, or `None` when their layers match to the bit.
fn difference(a: &PrecisionDocument, b: &PrecisionDocument) -> Option<String> {
    if a.layer_count() != b.layer_count() {
        return Some(format!(
            "{} layers against {}",
            a.layer_count(),
            b.layer_count()
        ));
    }
    for i in 0..a.layer_count() {
        let (la, lb) = (a.layer(i).unwrap(), b.layer(i).unwrap());
        if la.offset() != lb.offset() {
            return Some(format!("layer {i} offset"));
        }
        match (la.cmyk_source(), lb.cmyk_source()) {
            (Some(x), Some(y)) if x.same_samples(y) => {}
            (None, None) => {}
            _ => return Some(format!("layer {i}'s inks differ")),
        }
    }
    None
}

/// What a case must answer.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Expect {
    Change,
    Nothing,
    Refused,
}

/// The product's answer against the whole-layer seam's.
fn compare(
    failures: &mut Vec<String>,
    label: &str,
    product: Result<*mut PrecisionDocument, String>,
    oracle: Result<Option<PrecisionDocument>, String>,
    expect: Expect,
) {
    match (&product, &oracle, expect) {
        (Ok(p), Ok(Some(o)), Expect::Change) if !p.is_null() => {
            if let Some(why) = difference(unsafe { &**p }, o) {
                failures.push(format!("{label}: {why}"));
            }
        }
        (Ok(p), Ok(None), Expect::Nothing) if p.is_null() => {}
        (Err(a), Err(b), Expect::Refused) if a == b => {}
        _ => failures.push(format!(
            "{label} (expect {expect:?}): product {:?}, whole layer {:?}",
            product.as_ref().map(|p| !p.is_null()),
            oracle.as_ref().map(Option::is_some)
        )),
    }
    if let Ok(p) = product {
        if !p.is_null() {
            free(p);
        }
    }
}

/// A `dodge_burn` or `retouch_brush` request on `layer`, against the seam.
fn edit_case(
    f: &mut Vec<String>,
    label: &str,
    doc: *mut PrecisionDocument,
    request: &Value,
    input: &[u16],
    expect: Expect,
) {
    let text = request.to_string();
    let product = try_edit(doc, &text, input);
    let oracle = unsafe { &*doc }.cmyk_brush_whole_layer(
        CmykBrushStroke::Edit {
            request: &text,
            input,
        },
        LIMIT,
    );
    compare(f, &format!("{label} {text}"), product, oracle, expect);
}

/// One Smudge stroke at `strength`, against the seam.
#[allow(clippy::too_many_arguments)]
fn smudge_case(
    f: &mut Vec<String>,
    label: &str,
    doc: *mut PrecisionDocument,
    layer: usize,
    dabs: &[f64],
    (strength, sample_all_layers): (f64, bool),
    selection: &[u16],
    expect: Expect,
) {
    let brush = CString::new(HARD).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_smudge(
            doc,
            layer,
            brush.as_ptr(),
            dabs.as_ptr(),
            dabs.len() / 4,
            strength,
            if selection.is_empty() {
                ptr::null()
            } else {
                selection.as_ptr()
            },
            selection.len(),
            sample_all_layers,
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    let product = if out.is_null() && !err.is_null() {
        Err(take_err_string(err))
    } else {
        Ok(out)
    };
    let oracle = unsafe { &*doc }.cmyk_brush_whole_layer(
        CmykBrushStroke::Smudge {
            layer,
            brush_request: HARD,
            dabs,
            strength,
            selection,
            sample_all_layers,
        },
        LIMIT,
    );
    let label = format!("{label} smudge layer {layer} strength {strength} sal {sample_all_layers}");
    compare(f, &label, product, oracle, expect);
}

/// One History Brush stroke from `state`'s layer 1, against the seam.
#[allow(clippy::too_many_arguments)]
fn history_case(
    f: &mut Vec<String>,
    label: &str,
    doc: *mut PrecisionDocument,
    layer: usize,
    state: *mut PrecisionDocument,
    opacity: f64,
    input: &[u16],
    expect: Expect,
) {
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_history_brush(
            doc,
            layer,
            state,
            1,
            opacity,
            input.as_ptr(),
            input.len(),
            ptr::null(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    let product = if out.is_null() && !err.is_null() {
        Err(take_err_string(err))
    } else {
        Ok(out)
    };
    let oracle = unsafe { &*doc }.cmyk_brush_whole_layer(
        CmykBrushStroke::History {
            layer,
            source: unsafe { &*state },
            source_layer: 1,
            opacity,
            input,
            rect: None,
        },
        LIMIT,
    );
    let label = format!("{label} history layer {layer} opacity {opacity}");
    compare(f, &label, product, oracle, expect);
}

/// `request` on `layer` with `selection` set.
fn on(request: &Value, layer: usize, selected: bool) -> Value {
    let mut out = request.clone();
    out["layer"] = json!(layer);
    if selected {
        out["selection"] = json!(true);
    }
    out
}

#[test]
fn the_brush_rect_passes_equal_the_whole_layer_passes() {
    let (x, y, w, h) = OFFSET;
    let doc = document(1, (x, y), (w, h));
    let locked = Box::into_raw(Box::new(
        unsafe { &*doc }
            .with_layer_locks(0, LOCK_TRANSPARENCY)
            .unwrap()
            .unwrap()
            .with_layer_locks(1, LOCK_TRANSPARENCY)
            .unwrap()
            .unwrap(),
    ));
    // The state: its layer 1 smaller than the target's and at another offset.
    let state = document(9, (60, 30), (40, 30));
    let stroke = dab(DAB);
    let gate = selection();
    let gated: Vec<u16> = stroke.iter().chain(&gate).copied().collect();
    let edge = edge_stroke();
    let mut failures = Vec::new();
    let f = &mut failures;
    let brushes = [
        json!({"op":"dodge_burn","layer":0,"exposure":0.6,"range":1,"burn":false}),
        json!({"op":"dodge_burn","layer":0,"exposure":0.6,"range":0,"burn":true}),
        json!({"op":"dodge_burn","layer":0,"exposure":0.6,"range":2,"burn":false,"protect_tones":true}),
        json!({"op":"dodge_burn","layer":0,"exposure":0.6,"range":1,"burn":true,"protect_tones":true}),
        json!({"op":"retouch_brush","layer":0,"kind":"blur","strength":0.8}),
        json!({"op":"retouch_brush","layer":0,"kind":"sharpen","strength":0.8}),
        json!({"op":"retouch_brush","layer":0,"kind":"saturate","strength":0.8}),
        json!({"op":"retouch_brush","layer":0,"kind":"desaturate","strength":0.8}),
        json!({"op":"retouch_brush","layer":0,"kind":"saturate","strength":0.8,"vibrance":true}),
        json!({"op":"retouch_brush","layer":0,"kind":"blur","strength":0.8,"sample_all_layers":true}),
        json!({"op":"retouch_brush","layer":0,"kind":"sharpen","strength":0.8,"sample_all_layers":true}),
    ];
    for layer in [0usize, 1] {
        let missing = missing_stroke(layer);
        for request in &brushes {
            let plain = on(request, layer, false);
            edit_case(f, "stroke", doc, &plain, &stroke, Expect::Change);
            let selected = on(request, layer, true);
            edit_case(f, "selection", doc, &selected, &gated, Expect::Change);
            edit_case(f, "locked", locked, &plain, &stroke, Expect::Change);
            edit_case(f, "edge", doc, &plain, &edge, Expect::Change);
            edit_case(f, "missing", doc, &plain, &missing, Expect::Nothing);
            let mut invalid = plain.clone();
            if invalid["op"] == "dodge_burn" {
                invalid["exposure"] = json!(1.5);
            } else {
                invalid["strength"] = json!(1.5);
            }
            edit_case(f, "invalid", doc, &invalid, &missing, Expect::Refused);
        }
        // Smudge: fractional dabs crossing the canvas's left edge (the
        // canvas layer's) and the offset layer's left edge.
        let across = line(-3.3, 45.6, 95.1, 47.4);
        let away = line(-40.2, 45.0, -20.7, 46.0);
        for sal in [false, true] {
            smudge_case(
                f,
                "stroke",
                doc,
                layer,
                &across,
                (0.6, sal),
                &[],
                Expect::Change,
            );
            smudge_case(
                f,
                "selection",
                doc,
                layer,
                &across,
                (0.6, sal),
                &gate,
                Expect::Change,
            );
            smudge_case(
                f,
                "locked",
                locked,
                layer,
                &across,
                (0.6, sal),
                &[],
                Expect::Change,
            );
            smudge_case(
                f,
                "missing",
                doc,
                layer,
                &away,
                (0.6, sal),
                &[],
                Expect::Nothing,
            );
            smudge_case(
                f,
                "invalid",
                doc,
                layer,
                &away,
                (1.5, sal),
                &[],
                Expect::Refused,
            );
        }
        // The History Brush: the state's layer 1 under the dab, and none of
        // it under the offset layer's top-left edge dab.
        history_case(f, "stroke", doc, layer, state, 0.7, &stroke, Expect::Change);
        history_case(
            f,
            "selection",
            doc,
            layer,
            state,
            0.7,
            &gated,
            Expect::Change,
        );
        history_case(
            f,
            "locked",
            locked,
            layer,
            state,
            0.7,
            &stroke,
            Expect::Change,
        );
        history_case(f, "edge", doc, layer, state, 0.7, &edge, Expect::Change);
        history_case(
            f,
            "missing",
            doc,
            layer,
            state,
            0.7,
            &missing,
            Expect::Nothing,
        );
        history_case(
            f,
            "invalid",
            doc,
            layer,
            state,
            1.5,
            &missing,
            Expect::Refused,
        );
    }
    free(state);
    free(locked);
    free(doc);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
