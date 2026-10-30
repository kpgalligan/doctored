//! A Pass Through group in a 32-bit float document composites
//! as Photoshop 26.10.0 was measured to (`scripts/photoshop-probe/
//! float_pass_through.py`, receipt `float-pass-through-473/take1`: every
//! statement, full weight, group opacity, group mask, both and nested, is
//! H-lerp) — the children over a copy of the backdrop, then that result
//! gated back against the backdrop by group opacity × mask, premultiplied.
//!
//! Oracles, none read off the implementation: the W3C source-over step in
//! f64 with slice 1's measured formulas (Multiply `cb·cs`, Linear Dodge
//! `cb + cs` unclamped), Exposure +1 as a gain of exactly two in linear light
//! (on non-negative backdrops only, as the measurement judged it), and a
//! Levels child at the bottom of a group read from the same Levels layer
//! rendered directly over the backdrop, since that is all it ever sees.
//! Compared at 1e-5 · max(1, |want|).

use crate::float_fixture::drcb;

use std::ffi::{c_char, CStr, CString};
use std::ptr;

use doctored_core::doc::BlendMode;
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_precision::dr_precision_doc_set_layer_opacity;
use doctored_core::ffi_precision_edit::dr_precision_doc_edit;
use doctored_core::float_document::float_operators_json;
use doctored_core::icc::IccProfile;
use doctored_core::precision_clipboard::{ClipboardPayload, PasteRequest};
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::REFERENCE_WORKING_BYTES;
use doctored_core::RegionRequest;
use serde_json::{json, Value};

const LIMIT: u64 = REFERENCE_WORKING_BYTES;

/// Backdrop colours, one per column: zero, neutrals below and above white,
/// a colour above white and one with a negative component.
const BACKDROPS: [[f32; 3]; 7] = [
    [0.0, 0.0, 0.0],
    [0.18, 0.18, 0.18],
    [1.0, 1.0, 1.0],
    [4.0, 4.0, 4.0],
    [8.0, 8.0, 8.0],
    [2.0, 0.5, 0.25],
    [-0.25, 0.5, 1.0],
];
/// Backdrop alphas and group-mask codes; row `y = 3 · alpha + mask`.
const ALPHAS: [f32; 3] = [1.0, 0.5, 0.0];
const MASK: [u16; 3] = [65535, 39321, 0];
const W: u32 = BACKDROPS.len() as u32;
const H: u32 = (ALPHAS.len() * MASK.len()) as u32;

/// The measurement's children: C1 in range, C2 and C3 above white.
const C1: [f32; 3] = [0.5, 0.2, 0.1];
const C2: [f32; 3] = [3.0, 0.5, 0.25];
const C3: [f32; 3] = [0.4, 0.1, 2.0];

const NORMAL: i32 = 0;
const MULTIPLY: i32 = 1;
const LINEAR_DODGE: i32 = 12;
const PASS_THROUGH: i32 = 27;

const EXPOSURE: &str =
    r#"{"type":"adjust","evaluation":"precision_v1","op":"exposure","params":{"exposure":1.0}}"#;

fn levels() -> String {
    json!({"type":"adjust","evaluation":"precision_v1","op":"levels",
           "params":{"black": 0.0, "white": 192.0 / 255.0, "gamma": 1.0}})
    .to_string()
}

#[derive(Clone)]
enum Node {
    Raster {
        color: [f32; 3],
        mode: i32,
        opacity: f32,
    },
    Exposure,
    Levels,
    Group {
        opacity: f32,
        masked: bool,
        children: Vec<Node>,
    },
}

fn raster(color: [f32; 3], mode: i32, opacity: f32) -> Node {
    Node::Raster {
        color,
        mode,
        opacity,
    }
}

fn group(opacity: f32, masked: bool, children: Vec<Node>) -> Node {
    Node::Group {
        opacity,
        masked,
        children,
    }
}

// ------------------------------------------------------------- plumbing --

fn take(err: *mut c_char) -> Option<String> {
    if err.is_null() {
        return None;
    }
    let text = unsafe { CStr::from_ptr(err) }
        .to_string_lossy()
        .into_owned();
    unsafe { dr_string_free(err) };
    Some(text)
}

/// One request through `dr_precision_doc_edit`; `Err` is its refusal.
fn edit(
    doc: &PrecisionDocument,
    request: Value,
    input: &[u16],
) -> Result<PrecisionDocument, String> {
    let text = CString::new(request.to_string()).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_edit(
            doc,
            text.as_ptr(),
            if input.is_empty() {
                ptr::null()
            } else {
                input.as_ptr()
            },
            input.len(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    if let Some(message) = take(err) {
        assert!(out.is_null());
        return Err(message);
    }
    assert!(!out.is_null(), "{request} changed nothing");
    Ok(*unsafe { Box::from_raw(out) })
}

fn payload(pixels: &[[f32; 4]]) -> ClipboardPayload {
    ClipboardPayload::decode(&drcb(W, H, IccProfile::srgb().bytes(), pixels), LIMIT, 0)
        .expect("a payload")
}

fn backdrop_pixels() -> Vec<[f32; 4]> {
    (0..W * H)
        .map(|i| {
            let c = BACKDROPS[(i % W) as usize];
            [c[0], c[1], c[2], ALPHAS[(i / W) as usize / MASK.len()]]
        })
        .collect()
}

fn coverage(i: usize) -> f64 {
    f64::from(MASK[(i / W as usize) % MASK.len()]) / 65535.0
}

fn mask_samples() -> Vec<u16> {
    (0..(W * H) as usize)
        .map(|i| MASK[(i / W as usize) % MASK.len()])
        .collect()
}

fn backdrop_doc() -> PrecisionDocument {
    let doc = PrecisionDocument::from_clipboard(&payload(&backdrop_pixels()), LIMIT, 0, None)
        .expect("a float document");
    assert!(doc.is_float());
    doc
}

/// A canvas-sized opaque record of `color` pasted bit for bit at the top.
fn with_uniform(doc: &PrecisionDocument, color: [f32; 3]) -> PrecisionDocument {
    let pixels = vec![[color[0], color[1], color[2], 1.0]; (W * H) as usize];
    let request: PasteRequest = serde_json::from_value(json!({
        "name": "Child", "anchor": doc.layer_count() - 1, "position": "above",
        "placement": "in_place", "profile": "preserve",
    }))
    .unwrap();
    doc.clipboard_paste(&payload(&pixels), &request, LIMIT, 0, None)
        .expect("the child pastes")
}

fn with_opacity(doc: &PrecisionDocument, layer: usize, opacity: f32) -> PrecisionDocument {
    if opacity == 1.0 {
        return doc.clone();
    }
    let mut err = ptr::null_mut();
    let out = unsafe { dr_precision_doc_set_layer_opacity(doc, layer, opacity, &mut err) };
    assert_eq!(take(err), None);
    assert!(!out.is_null());
    *unsafe { Box::from_raw(out) }
}

fn with_mode(
    doc: &PrecisionDocument,
    layer: usize,
    mode: i32,
) -> Result<PrecisionDocument, String> {
    edit(
        doc,
        json!({"op":"layer_blend","version":1,"layer":layer,"blend_mode":mode}),
        &[],
    )
}

/// `nodes` built at the top of `doc`, bottom first; returns the document and
/// the index of each node's own record. A group is `group_layers` over its
/// children's records with Pass Through asked for, then its opacity and the
/// three-band mask.
fn build(mut doc: PrecisionDocument, nodes: &[Node]) -> (PrecisionDocument, Vec<usize>) {
    let mut tops = Vec::new();
    for node in nodes {
        match node {
            Node::Raster {
                color,
                mode,
                opacity,
            } => {
                doc = with_uniform(&doc, *color);
                let index = doc.layer_count() - 1;
                if *mode != NORMAL {
                    doc = with_mode(&doc, index, *mode).expect("the mode is set");
                }
                doc = with_opacity(&doc, index, *opacity);
            }
            Node::Exposure | Node::Levels => {
                let (name, description) = match node {
                    Node::Exposure => ("Exposure", EXPOSURE.to_string()),
                    _ => ("Levels", levels()),
                };
                doc = doc
                    .add_adjustment(name, &description, 1.0, LIMIT, 0, None)
                    .unwrap()
                    .unwrap();
            }
            Node::Group {
                opacity,
                masked,
                children,
            } => {
                let (built, kids) = build(doc, children);
                doc = edit(
                    &built,
                    json!({"op":"group_layers","layers":kids,"name":"Pass","pass_through":true}),
                    &[],
                )
                .expect("grouped");
                let index = doc.layer_count() - 1;
                assert!(doc.layer(index).unwrap().is_pass_through());
                doc = with_opacity(&doc, index, *opacity);
                if *masked {
                    doc = edit(
                        &doc,
                        json!({"op":"set_mask","layer":index,"width":W,"height":H}),
                        &mask_samples(),
                    )
                    .expect("the mask is set");
                }
            }
        }
        tops.push(doc.layer_count() - 1);
    }
    (doc, tops)
}

fn scene(nodes: &[Node]) -> PrecisionDocument {
    let (doc, _) = build(backdrop_doc(), nodes);
    assert_eq!(doc.float_refusal(), None, "the float renderer accepts it");
    doc
}

fn render(doc: &PrecisionDocument) -> Vec<[f32; 4]> {
    doc.render(LIMIT, 0, None)
        .expect("the float scene renders")
        .to_rgba32()
        .unwrap()
}

fn bits(pixels: &[[f32; 4]]) -> Vec<[u32; 4]> {
    pixels.iter().map(|p| p.map(f32::to_bits)).collect()
}

// --------------------------------------------------------------- oracle --

fn blend(mode: i32, cb: f64, cs: f64) -> f64 {
    match mode {
        NORMAL => cs,
        MULTIPLY => cb * cs,
        LINEAR_DODGE => cb + cs,
        _ => unreachable!("mode {mode} is not in this file's scenes"),
    }
}

/// One W3C source-over step of opaque straight `cs` at `sa` onto `bg`.
fn step(bg: [f64; 4], cs: [f32; 3], sa: f64, mode: i32) -> [f64; 4] {
    let ab = bg[3];
    let ao = sa + ab * (1.0 - sa);
    if ao <= 0.0 {
        return [0.0; 4];
    }
    let mut out = [0.0; 4];
    for c in 0..3 {
        let s = f64::from(cs[c]);
        out[c] =
            (sa * (1.0 - ab) * s + sa * ab * blend(mode, bg[c], s) + (1.0 - sa) * ab * bg[c]) / ao;
    }
    out[3] = ao;
    out
}

/// `a + t (b - a)` on premultiplied colour, back to straight.
fn gate(a: [f64; 4], b: [f64; 4], t: f64) -> [f64; 4] {
    let alpha = a[3] + t * (b[3] - a[3]);
    if alpha <= 0.0 {
        return [0.0; 4];
    }
    let mut out = [0.0; 4];
    for c in 0..3 {
        out[c] = (a[c] * a[3] + t * (b[c] * b[3] - a[c] * a[3])) / alpha;
    }
    out[3] = alpha;
    out
}

/// H-lerp, the measured form: every child over a COPY of what is below the
/// group, then the group's opacity × mask interpolates back to it.
fn oracle(nodes: &[Node], mut acc: [f64; 4], i: usize, levels: &[[f32; 4]]) -> [f64; 4] {
    for node in nodes {
        acc = match node {
            Node::Raster {
                color,
                mode,
                opacity,
            } => step(acc, *color, f64::from(*opacity), *mode),
            Node::Exposure if acc[3] > 0.0 => [2.0 * acc[0], 2.0 * acc[1], 2.0 * acc[2], acc[3]],
            Node::Levels if acc[3] > 0.0 => {
                let b = BACKDROPS[i % W as usize].map(f64::from);
                assert_eq!([acc[0], acc[1], acc[2]], b, "Levels sees only the backdrop");
                let l = levels[i].map(f64::from);
                [l[0], l[1], l[2], acc[3]]
            }
            Node::Exposure | Node::Levels => acc,
            Node::Group {
                opacity,
                masked,
                children,
            } => {
                let evaluated = oracle(children, acc, i, levels);
                let m = if *masked { coverage(i) } else { 1.0 };
                gate(acc, evaluated, f64::from(*opacity) * m)
            }
        };
    }
    acc
}

/// `Levels` rendered directly over the backdrop: L(B) per pixel.
fn levels_over_backdrop() -> Vec<[f32; 4]> {
    render(&scene(&[Node::Levels]))
}

fn assert_scene(nodes: &[Node], skip_negative: bool, what: &str) {
    let levels = levels_over_backdrop();
    let got = render(&scene(nodes));
    let back = backdrop_pixels();
    for (i, g) in got.iter().enumerate() {
        if skip_negative && back[i][..3].iter().any(|&v| v < 0.0) {
            continue;
        }
        let want = oracle(nodes, back[i].map(f64::from), i, &levels);
        let close = |c: usize| (f64::from(g[c]) - want[c]).abs() <= 1e-5 * want[c].abs().max(1.0);
        let ok = if want[3] == 0.0 {
            g[3] == 0.0
        } else {
            (0..4).all(close)
        };
        assert!(ok, "{what} pixel {i}: got {g:?}, want {want:?}");
    }
}

fn k1(opacity: f32, masked: bool) -> Vec<Node> {
    vec![group(opacity, masked, vec![raster(C2, MULTIPLY, 1.0)])]
}

fn k2(opacity: f32, masked: bool) -> Vec<Node> {
    vec![group(
        opacity,
        masked,
        vec![
            raster(C1, NORMAL, 0.8),
            raster(C2, MULTIPLY, 1.0),
            raster(C3, LINEAR_DODGE, 0.6),
        ],
    )]
}

fn k3(opacity: f32, masked: bool) -> Vec<Node> {
    vec![group(
        opacity,
        masked,
        vec![Node::Levels, raster(C1, NORMAL, 0.8), Node::Exposure],
    )]
}

fn k4() -> Vec<Node> {
    vec![group(
        0.6,
        true,
        vec![
            raster(C1, NORMAL, 0.8),
            group(0.6, false, vec![raster(C2, MULTIPLY, 1.0)]),
        ],
    )]
}

const WEIGHTS: [(f32, bool); 4] = [(1.0, false), (1.0, true), (0.6, false), (0.6, true)];

// ---------------------------------------------------------------- tests --

#[test]
fn blended_children_composite_into_the_backdrop_gated_by_opacity_and_mask() {
    for (opacity, masked) in WEIGHTS {
        let what = format!("opacity {opacity} masked {masked}");
        assert_scene(&k1(opacity, masked), false, &format!("K1 {what}"));
        assert_scene(&k2(opacity, masked), false, &format!("K2 {what}"));
    }
}

#[test]
fn a_nested_pass_through_group_gates_inside_its_parent() {
    assert_scene(&k4(), false, "K4");
}

#[test]
fn adjustment_children_reach_the_backdrop() {
    for (opacity, masked) in WEIGHTS {
        let what = format!("opacity {opacity} masked {masked}");
        assert_scene(&k3(opacity, masked), true, &format!("K3 {what}"));
        assert_scene(
            &[group(opacity, masked, vec![Node::Exposure])],
            true,
            &format!("Exposure {what}"),
        );
    }
}

#[test]
fn a_region_is_the_same_window_of_the_full_render() {
    let window = RegionRequest {
        x: 1,
        y: 1,
        width: W - 2,
        height: H - 2,
        preview_divisor: 1,
    };
    for nodes in [k2(0.6, true), k3(0.6, true), k4()] {
        let doc = scene(&nodes);
        let full = render(&doc);
        let region = doc
            .render_region(window, LIMIT, 0, None)
            .expect("the region renders")
            .to_rgba32()
            .unwrap();
        let want: Vec<[f32; 4]> = (1..H - 1)
            .flat_map(|y| (1..W - 1).map(move |x| (y * W + x) as usize))
            .map(|i| full[i])
            .collect();
        assert_eq!(bits(&region), bits(&want));
    }
}

#[test]
fn merging_visible_or_down_keeps_the_composite() {
    for nodes in [k2(0.6, true), k4()] {
        let doc = scene(&nodes);
        let full = render(&doc);
        let merged = edit(&doc, json!({"op":"merge_visible"}), &[]).expect("merged");
        assert_eq!(bits(&render(&merged)), bits(&full), "merge_visible");
        let group = doc.layer_count() - 1;
        let down = edit(&doc, json!({"op":"merge_down","layer":group}), &[]).expect("merged");
        assert_eq!(down.layer_count(), 1);
        assert_eq!(bits(&render(&down)), bits(&full), "merge_down");
    }
}

#[test]
fn save_and_reopen_keep_the_group_pass_through() {
    let doc = scene(&k4());
    let (outer, inner) = (doc.layer_count() - 1, doc.layer_count() - 2);
    let bytes = doc.encode_native(LIMIT, 0, None).unwrap();
    let reopened = PrecisionDocument::parse_native(&bytes, LIMIT, 0, None).unwrap();
    assert!(reopened.is_float());
    for index in [outer, inner] {
        assert!(reopened.layer(index).unwrap().is_pass_through(), "{index}");
    }
    assert_eq!(bits(&render(&reopened)), bits(&render(&doc)));
    assert_eq!(reopened.encode_native(LIMIT, 0, None).unwrap(), bytes);
    // Preserve Layers into 16 bits keeps the group Pass Through.
    let narrowed = doc.narrow_structure_to_u16(LIMIT, 0, None).unwrap();
    assert!(!narrowed.is_float());
    assert!(narrowed.layer(outer).unwrap().is_pass_through());
    assert!(narrowed.layer(inner).unwrap().is_pass_through());
}

#[test]
fn pass_through_is_admitted_on_float_groups_and_still_refused_on_a_raster() {
    let (doc, tops) = build(backdrop_doc(), &[raster(C1, NORMAL, 1.0)]);
    let isolated = edit(
        &doc,
        json!({"op":"group_layers","layers":tops,"name":"G","pass_through":false}),
        &[],
    )
    .unwrap();
    assert!(!isolated.layer(2).unwrap().is_pass_through());
    // Setting Pass Through on an existing float group is kept, as measured
    // in Photoshop 26.10.0 (`create`'s `set_read`).
    let set = with_mode(&isolated, 2, PASS_THROUGH).expect("Pass Through is set");
    assert!(set.layer(2).unwrap().is_pass_through());
    assert_eq!(set.layer(2).unwrap().blend_mode(), BlendMode::PassThrough);
    assert_eq!(set.float_refusal(), None);
    let refusal = with_mode(&doc, 1, PASS_THROUGH)
        .err()
        .expect("a raster refuses");
    assert!(
        refusal.ends_with("Pass Through requires a group"),
        "{refusal}"
    );

    let table: Value = serde_json::from_str(&float_operators_json()).unwrap();
    let admitted = table["admitted"].as_array().unwrap();
    assert!(admitted.contains(&json!("pass_through_group")));
    assert!(!table["refused"]
        .as_array()
        .unwrap()
        .iter()
        .any(|row| row["operator"].as_str().unwrap().contains("Pass Through")));
}
