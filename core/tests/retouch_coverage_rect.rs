//! 13B-9c: a typed Clone, Heal, Spot Heal or Content-Aware Fill request may
//! name the canvas rect its coverage covers (`"coverage_rect":[x,y,w,h]`),
//! and then sends `w·h` samples (`·2` with `selection`): the coverage plane
//! cropped to the rect, then the selection plane cropped to the same rect.
//! Coverage outside the rect is 0.
//!
//! The oracle is the canvas-form request on the same document: for every
//! coverage that is zero outside the rect, the rect form answers the same
//! document to the bit (`same_samples` on every layer, offsets and layer
//! count equal), or the same refusal, through `dr_precision_doc_edit` and
//! `dr_precision_doc_retouch_from`, at 16 bits, 32-bit float and on CMYK
//! ink, with and without a selection, in place and onto a new layer, on a
//! canvas layer and on a layer standing partly off the canvas. The plan
//! reports the rect form's input shape; each refusal names itself.
//!
//! The confined-overreach pair holds the order of the shared prelude: a
//! Heal's raw coverage may reach past its `x/y/width/height` where the
//! selection is zero (the selection confines it, so it is admitted and
//! equals the same request with that overreach zeroed), and refuses where
//! the selection is not.
//!
//! The coverage brushes take the same key (13B-9's residue): a `dodge_burn`
//! (Dodge, Burn, Protect Tones) or `retouch_brush` request (Blur, Sharpen,
//! Saturate, Desaturate, the Vibrance sponge, Blur and Sharpen with Sample
//! All Layers), and the History Brush export's `coverage_rect` argument,
//! answer what their canvas form answers to the bit at each depth, with and
//! without a selection, on the canvas layer, the off-canvas layer and a
//! transparency-locked one; a rect that misses the layer answers nothing in
//! both forms. Their plans report the rect form's input, and each refusal
//! names itself.

use std::ffi::{CStr, CString};
use std::ptr;

use doctored_core::doc_lock::LOCK_TRANSPARENCY;
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_cmyk::dr_precision_doc_convert_to_cmyk;
use doctored_core::ffi_precision_edit::{dr_precision_doc_edit, dr_precision_doc_edit_plan};
use doctored_core::ffi_retouch::dr_precision_doc_retouch_from;
use doctored_core::ffi_retouch_brush::{
    dr_precision_doc_history_brush, dr_precision_doc_history_brush_plan,
};
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer as Layer};
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use serde_json::{json, Value};

use crate::cmyk_fixture::press_profile;

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
const W: u32 = 96;
const H: u32 = 72;
const N: usize = W as usize * H as usize;
/// The layer standing partly off the canvas (off its left and bottom
/// edges): `(x, y, w, h)`.
const OFF: (i32, i32, u32, u32) = (-12, 44, 60, 40);
/// The dab: full coverage over this rect, feathered by two pixels.
const DAB: (u32, u32, u32, u32) = (20, 46, 18, 14);
/// The coverage rect the rect-form requests name: the dab's feathered
/// extent grown by 3 px on every side.
const RECT: (u32, u32, u32, u32) = (15, 41, 28, 24);

// ------------------------------------------------------------- fixtures --

fn pattern16(w: u32, h: u32, seed: u32) -> Vec<[u16; 4]> {
    (0..w * h)
        .map(|i| {
            let (x, y) = (i % w, i / w);
            [
                ((x * 2711 + y * 911 + seed * 1234) % 65536) as u16,
                ((x * 523 + y * 3301 + seed * 77) % 65536) as u16,
                (((x ^ y) * 1999 + seed) % 65536) as u16,
                if x % 31 == 5 && y % 29 == 7 {
                    30000
                } else {
                    65535
                },
            ]
        })
        .collect()
}

fn image16(w: u32, h: u32, seed: u32) -> PrecisionImage {
    PrecisionImage::from_rgba16(w, h, pattern16(w, h, seed), IccProfile::srgb(), None).unwrap()
}

/// A canvas layer under a layer standing partly off the canvas.
fn document16(seed: u32) -> PrecisionDocument {
    let (ox, oy, ow, oh) = OFF;
    PrecisionDocument::from_parts(
        W,
        H,
        IccProfile::srgb(),
        Metadata::default(),
        Resolution::default(),
        vec![
            Layer::raster("L", image16(W, H, seed), (0, 0), 1.0, true).unwrap(),
            Layer::raster("O", image16(ow, oh, seed + 5), (ox, oy), 1.0, true).unwrap(),
        ],
    )
    .unwrap()
}

fn take(error: *mut std::ffi::c_char) -> String {
    let text = unsafe { CStr::from_ptr(error) }
        .to_string_lossy()
        .into_owned();
    unsafe { dr_string_free(error) };
    text
}

fn document32(seed: u32) -> PrecisionDocument {
    let doc = document16(seed).promote_to_float(LIMIT, 0, None).unwrap();
    assert!(doc.is_float());
    doc
}

fn document_cmyk(seed: u32) -> PrecisionDocument {
    let rgb = document16(seed);
    let profile = press_profile("Press Test");
    let mut error = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_convert_to_cmyk(
            &rgb,
            profile.as_ptr(),
            profile.len(),
            c"{}".as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut error,
        )
    };
    assert!(!out.is_null(), "to CMYK: {}", take(error));
    let doc = *unsafe { Box::from_raw(out) };
    assert!(doc.is_cmyk());
    doc
}

/// Each depth's documents, every layer keeping its offset and extent.
fn documents(seed: u32) -> Vec<(&'static str, PrecisionDocument)> {
    let out = vec![
        ("16-bit", document16(seed)),
        ("float", document32(seed)),
        ("cmyk", document_cmyk(seed)),
    ];
    for (depth, doc) in &out {
        let layer = doc.layer(1).unwrap();
        let size = layer
            .source()
            .map(PrecisionImage::dimensions)
            .or_else(|| layer.cmyk_source().map(|i| i.dimensions()));
        assert_eq!(
            (layer.offset(), size),
            ((OFF.0, OFF.1), Some((OFF.2, OFF.3))),
            "{depth}: the off-canvas layer keeps its geometry"
        );
    }
    out
}

fn inside((px, py): (i64, i64), (x, y, w, h): (u32, u32, u32, u32)) -> bool {
    px >= i64::from(x) && px < i64::from(x + w) && py >= i64::from(y) && py < i64::from(y + h)
}

/// Full coverage over `rect` with a two-pixel feathered ring.
fn coverage(rect: (u32, u32, u32, u32)) -> Vec<u16> {
    let (x, y, w, h) = rect;
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

/// A selection ramping across the canvas, zero left of x = 24.
fn selection() -> Vec<u16> {
    (0..N)
        .map(|i| {
            let x = i % W as usize;
            if x < 24 {
                0
            } else {
                ((x - 23) * 65535 / (W as usize - 23)) as u16
            }
        })
        .collect()
}

/// `plane` (canvas-sized) cropped to `rect`, row-major.
fn crop(plane: &[u16], (x, y, w, h): (u32, u32, u32, u32)) -> Vec<u16> {
    let mut out = Vec::with_capacity((w * h) as usize);
    for row in y..y + h {
        let at = (row * W + x) as usize;
        out.extend_from_slice(&plane[at..at + w as usize]);
    }
    out
}

/// The canvas-form samples and the rect-form samples of one stroke.
fn inputs(stroke: &[u16], selection: Option<&[u16]>) -> (Vec<u16>, Vec<u16>) {
    inputs_over(stroke, selection, RECT)
}

/// [`inputs`] over `over`.
fn inputs_over(
    stroke: &[u16],
    selection: Option<&[u16]>,
    over: (u32, u32, u32, u32),
) -> (Vec<u16>, Vec<u16>) {
    let mut canvas = stroke.to_vec();
    let mut rect = crop(stroke, over);
    if let Some(selection) = selection {
        canvas.extend_from_slice(selection);
        rect.extend(crop(selection, over));
    }
    (canvas, rect)
}

/// `base` with `extra`'s fields set.
fn with(base: &Value, extra: Value) -> Value {
    let mut out = base.clone();
    for (k, v) in extra.as_object().unwrap() {
        out[k] = v.clone();
    }
    out
}

/// `request` with `selection: true` when `selected` (Content-Aware Fill
/// takes no selection plane: `None`), and `output` unless it is absent.
fn variant(request: &Value, selected: bool, output: Option<&str>) -> Option<Value> {
    let mut out = request.clone();
    if selected {
        if out["op"] == "content_aware_fill" {
            return None;
        }
        out["selection"] = json!(true);
    }
    if let Some(output) = output {
        out["output"] = json!(output);
        // Content-Aware Fill new-layer output samples a rendered plate.
        if output == "new_layer" && out["op"] == "content_aware_fill" {
            out["sample"] = json!("all");
        }
    }
    Some(out)
}

/// `request` in rect form.
fn rect_form(request: &Value, rect: (u32, u32, u32, u32)) -> Value {
    with(
        request,
        json!({"coverage_rect": [rect.0, rect.1, rect.2, rect.3]}),
    )
}

// ----------------------------------------------------------------- paths --

type Answer = Result<Option<Box<PrecisionDocument>>, String>;

fn edit(doc: &PrecisionDocument, request: &Value, samples: &[u16]) -> Answer {
    let text = CString::new(request.to_string()).unwrap();
    let mut error = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_edit(
            doc,
            text.as_ptr(),
            samples.as_ptr(),
            samples.len(),
            LIMIT,
            0,
            ptr::null(),
            &mut error,
        )
    };
    if !error.is_null() {
        return Err(take(error));
    }
    Ok((!out.is_null()).then(|| unsafe { Box::from_raw(out) }))
}

fn edit_from(
    doc: &PrecisionDocument,
    plates: &PrecisionDocument,
    request: &Value,
    samples: &[u16],
) -> Answer {
    let text = CString::new(request.to_string()).unwrap();
    let mut error = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_retouch_from(
            doc,
            plates,
            text.as_ptr(),
            samples.as_ptr(),
            samples.len(),
            LIMIT,
            0,
            ptr::null(),
            &mut error,
        )
    };
    if !error.is_null() {
        return Err(take(error));
    }
    Ok((!out.is_null()).then(|| unsafe { Box::from_raw(out) }))
}

/// The refusal of an answer that must refuse.
fn refused(answer: Answer) -> String {
    match answer {
        Err(e) => e,
        Ok(out) => panic!("not refused: changed {}", out.is_some()),
    }
}

fn plan(doc: &PrecisionDocument, request: &Value) -> Result<Value, String> {
    let text = CString::new(request.to_string()).unwrap();
    let mut error = ptr::null_mut();
    let out = unsafe { dr_precision_doc_edit_plan(doc, text.as_ptr(), LIMIT, 0, &mut error) };
    if out.is_null() {
        return Err(take(error));
    }
    let value = serde_json::from_str(unsafe { CStr::from_ptr(out) }.to_str().unwrap()).unwrap();
    unsafe { dr_string_free(out) };
    Ok(value)
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
            return Some(format!(
                "layer {i} offset {:?} against {:?}",
                la.offset(),
                lb.offset()
            ));
        }
        let same = match (la.source(), lb.source(), la.cmyk_source(), lb.cmyk_source()) {
            (Some(x), Some(y), None, None) => x.same_samples(y),
            (None, None, Some(x), Some(y)) => x.same_samples(y),
            (None, None, None, None) => true,
            _ => false,
        };
        if !same {
            return Some(format!("layer {i}'s samples differ"));
        }
    }
    None
}

/// The rect form against the canvas form: both change the document alike,
/// or both refuse alike. `changes` asks that the canvas form changes it.
fn compare(failures: &mut Vec<String>, label: &str, canvas: Answer, rect: Answer, changes: bool) {
    match (canvas, rect) {
        (Ok(Some(a)), Ok(Some(b))) => {
            if let Some(why) = difference(&a, &b) {
                failures.push(format!("{label}: {why}"));
            }
        }
        (Ok(None), Ok(None)) if !changes => {}
        (Err(a), Err(b)) if a == b && !changes => {}
        (canvas, rect) => failures.push(format!(
            "{label}: canvas form {:?}, rect form {:?}",
            canvas.map(|d| d.is_some()),
            rect.map(|d| d.is_some())
        )),
    }
}

/// The four ops' canvas-form requests on `layer`.
fn requests(layer: usize) -> [(&'static str, Value); 4] {
    let (x, y, w, h) = (DAB.0 - 2, DAB.1 - 2, DAB.2 + 4, DAB.3 + 4);
    let bounds = json!({"layer": layer, "x": x, "y": y, "width": w, "height": h});
    let shift = [1.0, 0.0, 0.0, 1.0, 21.5, -6.25];
    [
        (
            "clone",
            json!({"op": "retouch_clone", "layer": layer, "matrix": shift, "opacity": 0.8}),
        ),
        (
            "heal",
            json!({"op": "retouch_heal", "layer": layer, "matrix": shift}),
        ),
        (
            "spot_heal",
            with(&bounds, json!({"op": "spot_heal", "seed": 7})),
        ),
        (
            "content_aware_fill",
            with(&bounds, json!({"op": "content_aware_fill", "seed": 7})),
        ),
    ]
}

// ----------------------------------------------------------------- tests --

#[test]
fn the_rect_form_answers_the_canvas_form() {
    let stroke = coverage(DAB);
    assert!(
        stroke
            .iter()
            .enumerate()
            .all(|(i, c)| *c == 0
                || inside(((i % W as usize) as i64, (i / W as usize) as i64), RECT)),
        "the rect holds the whole stroke"
    );
    let gate = selection();
    let mut failures = Vec::new();
    for (depth, doc) in documents(1) {
        for layer in [0usize, 1] {
            for (op, request) in requests(layer) {
                for selected in [false, true] {
                    for output in ["layer", "new_layer"] {
                        let Some(request) = variant(&request, selected, Some(output)) else {
                            continue;
                        };
                        let (canvas, rect) = inputs(&stroke, selected.then_some(&gate[..]));
                        let label = format!(
                            "{depth} {op} layer {layer} selection {selected} output {output}"
                        );
                        compare(
                            &mut failures,
                            &label,
                            edit(&doc, &request, &canvas),
                            edit(&doc, &rect_form(&request, RECT), &rect),
                            true,
                        );
                    }
                }
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn retouch_from_takes_the_rect_form() {
    let stroke = coverage(DAB);
    let gate = selection();
    let plates = documents(9);
    let mut failures = Vec::new();
    for ((depth, doc), (_, plates)) in documents(1).into_iter().zip(&plates) {
        for (op, request) in requests(0).into_iter().take(2) {
            for selected in [false, true] {
                let Some(request) = variant(&request, selected, None) else {
                    continue;
                };
                let (canvas, rect) = inputs(&stroke, selected.then_some(&gate[..]));
                compare(
                    &mut failures,
                    &format!("{depth} {op} from plates, selection {selected}"),
                    edit_from(&doc, plates, &request, &canvas),
                    edit_from(&doc, plates, &rect_form(&request, RECT), &rect),
                    true,
                );
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn the_plan_reports_the_rect_forms_input() {
    for (depth, doc) in documents(1) {
        for (op, request) in requests(0) {
            for selected in [false, true] {
                let planes = if selected { 2 } else { 1 };
                let Some(request) = variant(&request, selected, None) else {
                    continue;
                };
                let canvas = plan(&doc, &request).unwrap();
                assert_eq!(
                    (
                        &canvas["input_width"],
                        &canvas["input_height"],
                        &canvas["input_planes"],
                        &canvas["input_samples"]
                    ),
                    (&json!(W), &json!(H), &json!(planes), &json!(N * planes)),
                    "{depth} {op} canvas form, selection {selected}"
                );
                let rect = plan(&doc, &rect_form(&request, RECT)).unwrap();
                assert_eq!(
                    (
                        &rect["input_width"],
                        &rect["input_height"],
                        &rect["input_planes"],
                        &rect["input_samples"]
                    ),
                    (
                        &json!(RECT.2),
                        &json!(RECT.3),
                        &json!(planes),
                        &json!((RECT.2 * RECT.3) as usize * planes)
                    ),
                    "{depth} {op} rect form, selection {selected}"
                );
            }
        }
    }
}

#[test]
fn each_refusal_names_itself() {
    const OUTSIDE: &str = "retouch coverage_rect must be a nonempty rectangle inside the canvas";
    let stroke = coverage(DAB);
    for (depth, doc) in documents(1) {
        for (op, request) in requests(0) {
            for rect in [
                (90, 10, 10, 10),
                (10, 70, 10, 3),
                (10, 10, 0, 5),
                (10, 10, 5, 0),
            ] {
                assert_eq!(
                    plan(&doc, &rect_form(&request, rect)).unwrap_err(),
                    OUTSIDE,
                    "{depth} {op} {rect:?}"
                );
            }
            // A sample count that is not the plan's.
            assert_eq!(
                refused(edit(&doc, &rect_form(&request, RECT), &stroke)),
                "high-depth input sample length does not match the core plan",
                "{depth} {op}"
            );
        }
        // Coverage outside the request's bounds, in both forms.
        let narrow = json!({"op": "spot_heal", "layer": 0, "seed": 7,
            "x": DAB.0, "y": DAB.1, "width": DAB.2, "height": DAB.3});
        for (form, request, samples) in [
            ("canvas", narrow.clone(), stroke.clone()),
            ("rect", rect_form(&narrow, RECT), crop(&stroke, RECT)),
        ] {
            assert_eq!(
                refused(edit(&doc, &request, &samples)),
                "retouch coverage exceeds its admitted bounds",
                "{depth} {form} form"
            );
        }
    }
    let doc = document16(1);
    let red_eye = json!({"op": "red_eye", "layer": 0, "x": 2, "y": 2, "width": 5, "height": 5,
        "coverage_rect": [0, 0, 10, 10]});
    assert_eq!(
        plan(&doc, &red_eye).unwrap_err(),
        "coverage_rect is not an argument of this retouch operation"
    );
}

/// The Heal of the confined-overreach pair: bounds = the dab, the raw
/// coverage reaching 3 px past their left edge into columns 17..20.
#[test]
fn a_selection_confines_coverage_past_the_bounds_in_both_forms() {
    let (x, y, w, h) = DAB;
    let reach = (x - 3, y, w + 3, h);
    let bounds = json!({"op": "retouch_heal", "layer": 0, "selection": true,
        "matrix": [1.0, 0.0, 0.0, 1.0, 21.5, -6.25], "x": x, "y": y, "width": w, "height": h});
    let full = |rect: (u32, u32, u32, u32)| -> Vec<u16> {
        (0..N)
            .map(|i| {
                let p = ((i % W as usize) as i64, (i / W as usize) as i64);
                if inside(p, rect) {
                    65535
                } else {
                    0
                }
            })
            .collect()
    };
    let overreach = full(reach);
    let within = full(DAB);
    // Zero left of x = 20 (the bounds' left edge), 65535 from it on.
    let zero_left = |edge: usize| -> Vec<u16> {
        (0..N)
            .map(|i| if i % W as usize >= edge { 65535 } else { 0 })
            .collect()
    };
    let rect = (x - 3, y, w + 3, h);
    let with_selection = |stroke: &[u16], gate: &[u16]| {
        let mut canvas = stroke.to_vec();
        canvas.extend_from_slice(gate);
        let mut cropped = crop(stroke, rect);
        cropped.extend(crop(gate, rect));
        (canvas, cropped)
    };
    let mut failures = Vec::new();
    for (depth, doc) in documents(1) {
        let confined = zero_left(x as usize);
        let (over_canvas, over_rect) = with_selection(&overreach, &confined);
        let (within_canvas, _) = with_selection(&within, &confined);
        // The canvas form: the regression guard (admitted on the base).
        compare(
            &mut failures,
            &format!("{depth} confined overreach, canvas form"),
            edit(&doc, &bounds, &over_canvas),
            edit(&doc, &bounds, &within_canvas),
            true,
        );
        compare(
            &mut failures,
            &format!("{depth} confined overreach, rect form"),
            edit(&doc, &rect_form(&bounds, rect), &over_rect),
            edit(&doc, &bounds, &within_canvas),
            true,
        );
        // The same overreach under a nonzero selection refuses.
        let open = zero_left(0);
        let (open_canvas, open_rect) = with_selection(&overreach, &open);
        for (form, answer) in [
            ("canvas", edit(&doc, &bounds, &open_canvas)),
            ("rect", edit(&doc, &rect_form(&bounds, rect), &open_rect)),
        ] {
            match answer {
                Err(e) if e == "retouch coverage exceeds its admitted bounds" => {}
                other => failures.push(format!(
                    "{depth} open overreach, {form} form: {:?}",
                    other.map(|d| d.is_some())
                )),
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

// ------------------------------------------------- the coverage brushes --

/// The coverage brushes' refusal of a rect outside the canvas.
const BRUSH_OUTSIDE: &str = "brush coverage_rect must be a nonempty rectangle inside the canvas";
/// A rect that misses the off-canvas layer (`OFF`: canvas x < 48, y ≥ 44),
/// and the dab inside it.
const MISS_RECT: (u32, u32, u32, u32) = (58, 4, 26, 22);
const MISS_DAB: (u32, u32, u32, u32) = (62, 8, 18, 14);

/// The coverage brushes' canvas-form `dr_precision_doc_edit` requests on
/// `layer`.
fn brushes(layer: usize) -> Vec<(&'static str, Value)> {
    let dodge = |extra: Value| {
        let base = json!({"op":"dodge_burn","layer":layer,"exposure":0.6,"range":1,"burn":false});
        with(&base, extra)
    };
    let brush = |kind: &str, extra: Value| {
        let base = json!({"op":"retouch_brush","layer":layer,"kind":kind,"strength":0.8});
        with(&base, extra)
    };
    vec![
        ("dodge", dodge(json!({}))),
        ("burn", dodge(json!({"burn": true, "range": 0}))),
        (
            "protect tones",
            dodge(json!({"protect_tones": true, "range": 2})),
        ),
        ("blur", brush("blur", json!({}))),
        ("sharpen", brush("sharpen", json!({}))),
        ("saturate", brush("saturate", json!({}))),
        ("desaturate", brush("desaturate", json!({}))),
        ("vibrance", brush("saturate", json!({"vibrance": true}))),
        (
            "blur sal",
            brush("blur", json!({"sample_all_layers": true})),
        ),
        (
            "sharpen sal",
            brush("sharpen", json!({"sample_all_layers": true})),
        ),
    ]
}

/// `rect` as the History Brush export's `coverage_rect` words.
fn words(rect: Option<(u32, u32, u32, u32)>) -> Option<[u32; 4]> {
    rect.map(|(x, y, w, h)| [x, y, w, h])
}

/// The History Brush painting layer `layer` of `source` back over `doc`'s
/// at opacity 0.7, its input over `rect` (the canvas when `None`).
fn history(
    doc: &PrecisionDocument,
    source: &PrecisionDocument,
    layer: usize,
    samples: &[u16],
    rect: Option<(u32, u32, u32, u32)>,
) -> Answer {
    let rect = words(rect);
    let mut error = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_history_brush(
            doc,
            layer,
            source,
            layer,
            0.7,
            samples.as_ptr(),
            samples.len(),
            rect.as_ref().map_or(ptr::null(), |r| r.as_ptr()),
            LIMIT,
            0,
            ptr::null(),
            &mut error,
        )
    };
    if !error.is_null() {
        return Err(take(error));
    }
    Ok((!out.is_null()).then(|| unsafe { Box::from_raw(out) }))
}

fn history_plan(
    doc: &PrecisionDocument,
    source: &PrecisionDocument,
    selected: bool,
    rect: Option<(u32, u32, u32, u32)>,
) -> Result<Value, String> {
    let rect = words(rect);
    let mut error = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_history_brush_plan(
            doc,
            0,
            source,
            0,
            selected,
            rect.as_ref().map_or(ptr::null(), |r| r.as_ptr()),
            LIMIT,
            0,
            &mut error,
        )
    };
    if out.is_null() {
        return Err(take(error));
    }
    let value = serde_json::from_str(unsafe { CStr::from_ptr(out) }.to_str().unwrap()).unwrap();
    unsafe { dr_string_free(out) };
    Ok(value)
}

/// Both forms answer no change.
fn nothing(failures: &mut Vec<String>, label: &str, canvas: Answer, rect: Answer) {
    match (canvas, rect) {
        (Ok(None), Ok(None)) => {}
        (canvas, rect) => failures.push(format!(
            "{label}: canvas form {:?}, rect form {:?} (both should change nothing)",
            canvas.map(|d| d.is_some()),
            rect.map(|d| d.is_some())
        )),
    }
}

#[test]
fn the_brush_rect_form_answers_the_canvas_form() {
    let stroke = coverage(DAB);
    let missing = coverage(MISS_DAB);
    let gate = selection();
    let sources = documents(9);
    let mut failures = Vec::new();
    for ((depth, doc), (_, source)) in documents(1).into_iter().zip(&sources) {
        for layer in [0usize, 1] {
            let locked = doc
                .with_layer_locks(layer, LOCK_TRANSPARENCY)
                .unwrap()
                .unwrap();
            for selected in [false, true] {
                let plane = selected.then_some(&gate[..]);
                let (canvas, rect) = inputs(&stroke, plane);
                let (miss_canvas, miss_rect) = inputs_over(&missing, plane, MISS_RECT);
                for (name, request) in brushes(layer) {
                    let mut request = request;
                    if selected {
                        request["selection"] = json!(true);
                    }
                    let label = format!("{depth} {name} layer {layer} selection {selected}");
                    for (target, on) in [("", &doc), (" locked", &locked)] {
                        compare(
                            &mut failures,
                            &format!("{label}{target}"),
                            edit(on, &request, &canvas),
                            edit(on, &rect_form(&request, RECT), &rect),
                            true,
                        );
                    }
                    if layer == 1 {
                        nothing(
                            &mut failures,
                            &format!("{label} missing rect"),
                            edit(&doc, &request, &miss_canvas),
                            edit(&doc, &rect_form(&request, MISS_RECT), &miss_rect),
                        );
                    }
                }
                let label = format!("{depth} history layer {layer} selection {selected}");
                for (target, on) in [("", &doc), (" locked", &locked)] {
                    compare(
                        &mut failures,
                        &format!("{label}{target}"),
                        history(on, source, layer, &canvas, None),
                        history(on, source, layer, &rect, Some(RECT)),
                        true,
                    );
                }
                if layer == 1 {
                    nothing(
                        &mut failures,
                        &format!("{label} missing rect"),
                        history(&doc, source, layer, &miss_canvas, None),
                        history(&doc, source, layer, &miss_rect, Some(MISS_RECT)),
                    );
                }
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn the_brush_plan_reports_the_rect_forms_input() {
    let sources = documents(9);
    for ((depth, doc), (_, source)) in documents(1).into_iter().zip(&sources) {
        for selected in [false, true] {
            let planes = if selected { 2 } else { 1 };
            let shape = |plan: &Value| {
                (
                    plan["input_width"].clone(),
                    plan["input_height"].clone(),
                    plan["input_planes"].clone(),
                    plan["input_samples"].clone(),
                )
            };
            let canvas_shape = (json!(W), json!(H), json!(planes), json!(N * planes));
            let rect_shape = (
                json!(RECT.2),
                json!(RECT.3),
                json!(planes),
                json!((RECT.2 * RECT.3) as usize * planes),
            );
            for (name, request) in brushes(0) {
                let mut request = request;
                request["selection"] = json!(selected);
                let canvas = plan(&doc, &request).unwrap();
                assert_eq!(
                    shape(&canvas),
                    canvas_shape,
                    "{depth} {name} canvas form, selection {selected}"
                );
                let rect = plan(&doc, &rect_form(&request, RECT)).unwrap();
                assert_eq!(
                    shape(&rect),
                    rect_shape,
                    "{depth} {name} rect form, selection {selected}"
                );
            }
            let canvas = history_plan(&doc, source, selected, None).unwrap();
            assert_eq!(
                shape(&canvas),
                canvas_shape,
                "{depth} history canvas form, selection {selected}"
            );
            let rect = history_plan(&doc, source, selected, Some(RECT)).unwrap();
            assert_eq!(
                shape(&rect),
                rect_shape,
                "{depth} history rect form, selection {selected}"
            );
        }
    }
}

#[test]
fn each_brush_refusal_names_itself() {
    const COUNT: &str = "high-depth input sample length does not match the core plan";
    let stroke = coverage(DAB);
    let sources = documents(9);
    let outside = [
        (90, 10, 10, 10),
        (10, 70, 10, 3),
        (10, 10, 0, 5),
        (10, 10, 5, 0),
    ];
    for ((depth, doc), (_, source)) in documents(1).into_iter().zip(&sources) {
        for (name, request) in brushes(0) {
            for rect in outside {
                assert_eq!(
                    plan(&doc, &rect_form(&request, rect)).unwrap_err(),
                    BRUSH_OUTSIDE,
                    "{depth} {name} {rect:?}"
                );
            }
            // A sample count that is not the plan's.
            assert_eq!(
                refused(edit(&doc, &rect_form(&request, RECT), &stroke)),
                COUNT,
                "{depth} {name}"
            );
        }
        for rect in outside {
            assert_eq!(
                history_plan(&doc, source, false, Some(rect)).unwrap_err(),
                BRUSH_OUTSIDE,
                "{depth} history plan {rect:?}"
            );
            assert_eq!(
                refused(history(&doc, source, 0, &crop(&stroke, RECT), Some(rect))),
                BRUSH_OUTSIDE,
                "{depth} history {rect:?}"
            );
        }
        assert_eq!(
            refused(history(&doc, source, 0, &stroke, Some(RECT))),
            COUNT,
            "{depth} history"
        );
        // The key is the coverage brushes' alone.
        let paint = if depth == "cmyk" {
            json!({"op":"paint_blend","layer":0,"ink":[0.1,0.2,0.3,0.4,1.0],"opacity":1.0,
                "coverage_rect":[RECT.0, RECT.1, RECT.2, RECT.3]})
        } else {
            json!({"op":"paint_blend","layer":0,"color":[0.1,0.2,0.3,1.0],
                "color_space":"document","opacity":1.0,"blend_mode":0,
                "coverage_rect":[RECT.0, RECT.1, RECT.2, RECT.3]})
        };
        let refusal = plan(&doc, &paint).unwrap_err();
        assert!(
            refusal.contains("unknown field `coverage_rect`"),
            "{depth} paint_blend: {refusal}"
        );
    }
}
