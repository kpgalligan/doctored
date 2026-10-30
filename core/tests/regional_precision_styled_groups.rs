//! Phase 13A-3b-ii: the 16-bit regional render over styled GROUPS WITH
//! ENABLED EFFECTS — materialized as `render_styled` materializes them (the
//! children rendered by `render_layers` itself into a canvas-sized buffer,
//! the planes built over it) — every effect, the package model, fill + typed
//! Blend If beside the group style, the group's mask, mixed children, the
//! styled group as a clip base and a clip member, inside other groups and
//! nested; the plan's `Global {"group_materialization"}` rows and the
//! `collect` mirror they charge; the refusals and the float policy. The
//! oracle is INDEPENDENT of the regional code: `dr_precision_doc_render`'s
//! projection read back through `dr_precision_image_pixels_rgba16`, cropped
//! and top-left-cell sampled here, compared as u16 codes.

use std::ffi::{c_char, CString};
use std::ptr;
use std::sync::Arc;

use doctored_core::doc::BlendMode;
use doctored_core::doc_evaluation::Evaluation;
use doctored_core::doc_mask::MaskProperties;
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_jobs::*;
use doctored_core::ffi_precision::*;
use doctored_core::ffi_precision_edit::dr_precision_doc_edit;
use doctored_core::ffi_regional::*;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer as Layer};
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use doctored_core::precision_mask::{PrecisionMask, PrecisionPlane};
use doctored_core::style::LayerStyle;
use serde_json::{json, Value};

use crate::region_plan;

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
const W: u32 = 41;
const H: u32 = 29;
const FLOAT_FIXTURE: &str = "tests/fixtures/float_11f/scene.tiff";

const BLEND_IF_GRAY: &str =
    "\"blend_if\":{\"channel\":\"gray\",\"this_layer\":[10,60,180,240],\"underlying\":[0,40,200,255]}";

// ------------------------------------------------------------- plumbing --

fn take(p: *mut c_char) -> String {
    if p.is_null() {
        return "<no error message>".into();
    }
    let text = unsafe { std::ffi::CStr::from_ptr(p) }
        .to_string_lossy()
        .into_owned();
    unsafe { dr_string_free(p) };
    text
}

fn samples(image: *mut PrecisionImage) -> (u32, u32, Vec<u32>) {
    let (w, h) = unsafe {
        (
            dr_precision_image_width(image),
            dr_precision_image_height(image),
        )
    };
    let n = (w * h * 4) as usize;
    let p16 = unsafe { dr_precision_image_pixels_rgba16(image) };
    let out = if !p16.is_null() {
        unsafe { std::slice::from_raw_parts(p16, n) }
            .iter()
            .map(|&v| u32::from(v))
            .collect()
    } else {
        unsafe { &*image }
            .to_rgba32()
            .expect("an image with samples")
            .as_flattened()
            .iter()
            .map(|v| v.to_bits())
            .collect()
    };
    unsafe { dr_precision_image_free(image) };
    (w, h, out)
}

fn full(doc: &PrecisionDocument) -> (u32, u32, Vec<u32>) {
    let mut err: *mut c_char = ptr::null_mut();
    let image = unsafe { dr_precision_doc_render(doc, LIMIT, 0, ptr::null(), &mut err) };
    assert!(!image.is_null(), "render: {}", take(err));
    samples(image)
}

fn region(
    doc: &PrecisionDocument,
    (x, y, w, h): (u32, u32, u32, u32),
    d: u32,
) -> Result<(u32, u32, Vec<u32>), String> {
    let mut err: *mut c_char = ptr::null_mut();
    let image = unsafe {
        dr_precision_doc_render_region(doc, x, y, w, h, d, LIMIT, 0, ptr::null(), &mut err)
    };
    if image.is_null() {
        return Err(take(err));
    }
    assert!(err.is_null(), "err_out set on success");
    Ok(samples(image))
}

fn plan(
    doc: &PrecisionDocument,
    (x, y, w, h): (u32, u32, u32, u32),
    d: u32,
) -> Result<Value, String> {
    region_plan::typed(doc, (x, y, w, h), d)
}

/// The independent oracle: sample (x + i·d, y + j·d) of the full render.
fn expected(
    (cw, _, reference): &(u32, u32, Vec<u32>),
    (x, y, w, h): (u32, u32, u32, u32),
    d: u32,
) -> (u32, u32, Vec<u32>) {
    let ow = w.div_ceil(d);
    let oh = h.div_ceil(d);
    let mut out = Vec::new();
    for j in 0..oh {
        for i in 0..ow {
            let cx = u64::from(x) + u64::from(i) * u64::from(d);
            let cy = u64::from(y) + u64::from(j) * u64::from(d);
            let at = ((cy * u64::from(*cw) + cx) * 4) as usize;
            out.extend_from_slice(&reference[at..at + 4]);
        }
    }
    (ow, oh, out)
}

fn assert_regions_match_with(doc: &PrecisionDocument, what: &str, extra: &[(u32, u32, u32, u32)]) {
    let reference = full(doc);
    let (cw, ch) = doc.dimensions();
    let mut windows = vec![
        (0, 0, cw, ch),
        (3, 2, 11, 7),
        (0, 0, 5, 4),
        (cw - 6, 0, 6, 5),
        (0, ch - 3, 7, 3),
        (cw - 4, ch - 5, 4, 5),
        (0, 4, cw, 3),
        (7, 0, 2, ch),
        (cw - 1, ch - 1, 1, 1),
        (0, 0, 1, 1),
        (19, 13, 1, 1),
    ];
    windows.extend_from_slice(extra);
    for window in windows {
        for d in [1, 2, 3, 7, u32::MAX] {
            let got =
                region(doc, window, d).unwrap_or_else(|e| panic!("{what}: {window:?}/{d}: {e}"));
            assert_eq!(
                got,
                expected(&reference, window, d),
                "{what}: window {window:?} divisor {d}"
            );
        }
    }
}

fn assert_regions_match(doc: &PrecisionDocument, what: &str) {
    assert_regions_match_with(doc, what, &[]);
}

// ------------------------------------------------------------- fixtures --

fn profile() -> Arc<IccProfile> {
    IccProfile::srgb()
}

fn image16(w: u32, h: u32, seed: u32, alpha: impl Fn(u32, u32) -> u16) -> PrecisionImage {
    let pixels = (0..w * h)
        .map(|i| {
            let (x, y) = (i % w, i / w);
            [
                ((x * 2711 + y * 911 + seed * 1234) % 65536) as u16,
                ((x * 523 + y * 3301 + seed * 77) % 65536) as u16,
                ((x ^ y) * 1999 % 65536 + seed) as u16,
                alpha(x, y),
            ]
        })
        .collect();
    PrecisionImage::from_rgba16(w, h, pixels, profile(), None).unwrap()
}

/// A shaped u16 layer: transparent outside an ellipse and in a hole, graded
/// alpha near the rim, opaque inside.
fn shape16(w: u32, h: u32, seed: u32) -> PrecisionImage {
    image16(w, h, seed, |x, y| {
        let (cx, cy) = (w as f32 / 2.0, h as f32 / 2.0);
        let dx = (x as f32 + 0.5 - cx) / cx;
        let dy = (y as f32 + 0.5 - cy) / cy;
        let r = dx * dx + dy * dy;
        let hole = x == w / 2 + 1 && (y == h / 2 || y == h / 2 + 1);
        if r > 1.0 || hole {
            0
        } else if r > 0.7 {
            (9000 + (x * 3001 + y * 1777 + seed * 97) % 50000) as u16
        } else {
            65535
        }
    })
}

fn base() -> Layer {
    Layer::raster("Base", image16(W, H, 1, |_, _| 65535), (0, 0), 1.0, true).unwrap()
}

fn shaped(name: &str, w: u32, h: u32, offset: (i32, i32), mode: BlendMode, opacity: f32) -> Layer {
    Layer::raster(name, shape16(w, h, 5), offset, opacity, true)
        .unwrap()
        .with_blend_mode(mode)
        .unwrap()
}

fn translucent(
    name: &str,
    w: u32,
    h: u32,
    offset: (i32, i32),
    mode: BlendMode,
    opacity: f32,
) -> Layer {
    let image = image16(w, h, 7, |x, y| {
        (9000 + (x * 3001 + y * 1777) % 56000) as u16
    });
    Layer::raster(name, image, offset, opacity, true)
        .unwrap()
        .with_blend_mode(mode)
        .unwrap()
}

fn style(json: &str) -> LayerStyle {
    LayerStyle::from_json(json).unwrap_or_else(|e| panic!("{json}: {e}"))
}

fn styled(layer: Layer, json: &str) -> Layer {
    layer.with_layer_style(Some(&style(json)), (W, H)).unwrap()
}

fn adjustment(name: &str, params: Value) -> Layer {
    let op = json!({"type":"adjust","op":name,"params":params,"evaluation":"precision_v1"});
    Layer::adjustment(name, &op.to_string(), 1.0, true, &profile()).unwrap()
}

fn doc(layers: Vec<Layer>) -> PrecisionDocument {
    PrecisionDocument::from_parts(
        W,
        H,
        profile(),
        Metadata::default(),
        Resolution::default(),
        layers,
    )
    .unwrap()
}

fn gradient(w: u32, h: u32) -> Vec<u16> {
    (0..w * h)
        .map(|i| {
            let (x, y) = (i % w, i / w);
            if x == w / 2 && y == h / 2 {
                0
            } else {
                ((x * 65535 / (w - 1).max(1)) ^ (y * 2311)) as u16
            }
        })
        .collect()
}

fn mask(w: u32, h: u32, offset: (i32, i32), density: f32, feather: f32) -> PrecisionMask {
    let mut properties = MaskProperties::at(offset);
    properties.density = density;
    properties.feather = feather;
    PrecisionMask::new(
        PrecisionPlane::from_u16(w, h, gradient(w, h), None).unwrap(),
        properties,
        true,
    )
    .unwrap()
}

fn around(offset: (i32, i32), w: u32, h: u32, grow: i64) -> Vec<(u32, u32, u32, u32)> {
    let clamp = |v: i64, hi: u32| v.clamp(0, i64::from(hi) - 1) as u32;
    let (x0, y0) = (i64::from(offset.0) - grow, i64::from(offset.1) - grow);
    let (x1, y1) = (
        i64::from(offset.0) + i64::from(w) + grow,
        i64::from(offset.1) + i64::from(h) + grow,
    );
    let mut out = Vec::new();
    for (cx, cy) in [(x0, y0), (x1, y0), (x0, y1), (x1, y1)] {
        let x = clamp(cx - 3, W);
        let y = clamp(cy - 2, H);
        out.push((x, y, (W - x).min(6), (H - y).min(5)));
    }
    out
}

fn fx(effects: &[&str]) -> String {
    format!("{{\"effects\":[{}]}}", effects.join(","))
}

/// The byte file's effect list, one depth up.
fn every_effect() -> Vec<(&'static str, String)> {
    vec![
        (
            "drop shadow 3/2",
            fx(&["{\"type\":\"drop_shadow\",\"distance\":3,\"size\":2}"]),
        ),
        (
            "drop shadow 9",
            fx(&["{\"type\":\"drop_shadow\",\"distance\":9,\"size\":3,\"spread\":0.3}"]),
        ),
        (
            "outer glow",
            fx(&["{\"type\":\"outer_glow\",\"size\":4,\"color\":\"#40ff80\"}"]),
        ),
        (
            "stroke outside",
            fx(&["{\"type\":\"stroke\",\"size\":3,\"position\":\"outside\",\"color\":\"#ff2000\"}"]),
        ),
        (
            "stroke inside",
            fx(&["{\"type\":\"stroke\",\"size\":2,\"position\":\"inside\",\"color\":\"#20ff00\",\"opacity\":0.7}"]),
        ),
        (
            "stroke center",
            fx(&["{\"type\":\"stroke\",\"size\":3,\"position\":\"center\",\"blend\":\"screen\"}"]),
        ),
        (
            "colour overlay multiply",
            fx(&["{\"type\":\"color_overlay\",\"blend\":\"multiply\",\"color\":\"#3080c0\",\"opacity\":0.8}"]),
        ),
        (
            "gradient overlay aligned",
            fx(&["{\"type\":\"gradient_overlay\",\"gradient\":{\"angle\":30,\"align_with_layer\":true,\
                  \"stops\":[{\"position\":0,\"color\":\"#ff0000\",\"opacity\":1},\
                  {\"position\":1,\"color\":\"#0000ff\",\"opacity\":0.6}]}}"]),
        ),
        (
            "gradient overlay canvas",
            fx(&["{\"type\":\"gradient_overlay\",\"blend\":\"overlay\",\"gradient\":{\"style\":\"radial\",\
                  \"align_with_layer\":false}}"]),
        ),
        (
            "inner shadow 0",
            fx(&["{\"type\":\"inner_shadow\",\"distance\":0,\"size\":4}"]),
        ),
        (
            "inner shadow 40",
            fx(&["{\"type\":\"inner_shadow\",\"distance\":40,\"size\":0,\"opacity\":0.9}"]),
        ),
        (
            "inner glow",
            fx(&["{\"type\":\"inner_glow\",\"size\":3,\"source\":\"center\"}"]),
        ),
        (
            "bevel",
            fx(&["{\"type\":\"bevel_emboss\",\"size\":4,\"depth\":2,\"highlight_opacity\":0.9,\
                  \"shadow_opacity\":0.8}"]),
        ),
        (
            "satin",
            fx(&["{\"type\":\"satin\",\"distance\":5,\"size\":4,\"color\":\"#602000\"}"]),
        ),
    ]
}

const VARIANTS: [(BlendMode, f32); 4] = [
    (BlendMode::Normal, 1.0),
    (BlendMode::Normal, 0.6),
    (BlendMode::Multiply, 0.6),
    (BlendMode::Dissolve, 0.6),
];

const GROUP_STYLE: &str = "{\"effects\":[{\"type\":\"drop_shadow\",\"distance\":5,\"size\":2},\
                           {\"type\":\"stroke\",\"size\":3},\
                           {\"type\":\"color_overlay\",\"blend\":\"multiply\",\"color\":\"#80c0ff\"}]}";

/// A shaped raster A at (6, 5) and a translucent Screen 0.8 raster B partly
/// off the canvas at (-4, 15).
fn pair() -> Vec<Layer> {
    vec![
        shaped("A", 17, 12, (6, 5), BlendMode::Normal, 1.0),
        translucent("B", 13, 10, (-4, 15), BlendMode::Screen, 0.8),
    ]
}

/// An isolated group over `children` in `mode`/`opacity` carrying `json`.
fn group_of(children: Vec<Layer>, json: &str, mode: BlendMode, opacity: f32) -> Layer {
    styled(
        Layer::isolated_normal("Group", children, opacity, true)
            .unwrap()
            .with_blend_mode(mode)
            .unwrap(),
        json,
    )
}

// ------------------------------------------------------------- lone --

#[test]
fn every_effect_on_a_lone_u16_styled_group_matches_the_projection() {
    for (label, json) in every_effect() {
        for (mode, opacity) in VARIANTS {
            let d = doc(vec![base(), group_of(pair(), &json, mode, opacity)]);
            let what = format!("{label} {mode:?} {opacity}");
            assert_regions_match_with(&d, &what, &around((-4, 5), 27, 20, 4));
        }
    }
}

#[test]
fn fill_and_typed_blend_if_beside_the_group_style_match() {
    let json = format!(
        "{{\"fill_opacity\":0.5,{BLEND_IF_GRAY},\"effects\":[\
         {{\"type\":\"drop_shadow\",\"distance\":4,\"size\":2}},\
         {{\"type\":\"color_overlay\",\"opacity\":0.5}},\
         {{\"type\":\"inner_glow\",\"size\":3}}]}}"
    );
    for (mode, opacity) in VARIANTS {
        let group = group_of(pair(), &json, mode, opacity);
        assert!(group.style().is_some());
        let d = doc(vec![base(), group]);
        let what = format!("fill + blend if + effects {mode:?} {opacity}");
        assert_regions_match_with(&d, &what, &around((-4, 5), 27, 20, 3));
    }
}

#[test]
fn a_masked_u16_styled_group_matches_with_feather_0_and_6() {
    for feather in [0.0, 6.0] {
        for (mode, opacity) in [(BlendMode::Normal, 1.0), (BlendMode::Multiply, 0.6)] {
            let group = group_of(pair(), GROUP_STYLE, mode, opacity).with_mask(Some(mask(
                20,
                14,
                (4, 6),
                0.8,
                feather,
            )));
            let d = doc(vec![base(), group]);
            let what = format!("masked group feather {feather} {mode:?} {opacity}");
            assert_regions_match_with(&d, &what, &around((4, 6), 20, 14, 0));
        }
    }
}

/// MIXED children: a raster base with a clipped Multiply member, a
/// hue/saturation adjustment, a feather-6 masked raster, a gated (0.6)
/// pass-through subgroup, a Multiply isolated subgroup, a hidden raster and a
/// zero-opacity raster.
fn mixed() -> Vec<Layer> {
    let hidden = Layer::raster("Hidden", shape16(8, 8, 9), (30, 2), 1.0, false).unwrap();
    vec![
        shaped("A", 17, 12, (6, 5), BlendMode::Normal, 1.0),
        translucent("Member", 15, 15, (14, 8), BlendMode::Multiply, 0.6).with_clipped(true),
        adjustment("hue_saturation", json!({"hue":-60,"saturation":0.2})),
        translucent("Masked", 12, 9, (20, 3), BlendMode::Normal, 0.9).with_mask(Some(mask(
            10,
            8,
            (21, 4),
            1.0,
            6.0,
        ))),
        Layer::pass_through(
            "Gated",
            vec![shaped("C", 11, 8, (2, 16), BlendMode::Normal, 1.0)],
            0.6,
            true,
        )
        .unwrap(),
        Layer::isolated_normal(
            "Multiply",
            vec![translucent("D", 10, 7, (25, 17), BlendMode::Normal, 1.0)],
            1.0,
            true,
        )
        .unwrap()
        .with_blend_mode(BlendMode::Multiply)
        .unwrap(),
        hidden,
        translucent("Zero", 8, 8, (1, 1), BlendMode::Normal, 0.0),
    ]
}

#[test]
fn a_u16_styled_group_over_mixed_children_matches() {
    for (mode, opacity) in VARIANTS {
        let d = doc(vec![base(), group_of(mixed(), GROUP_STYLE, mode, opacity)]);
        assert_regions_match(&d, &format!("mixed children {mode:?} {opacity}"));
    }
}

// ------------------------------------------------------------ clip units --

#[test]
fn a_u16_styled_group_as_a_clip_base_clamps_its_members() {
    for (mode, opacity) in VARIANTS {
        let layers = vec![
            base(),
            group_of(pair(), GROUP_STYLE, mode, opacity),
            translucent("Member", 15, 15, (20, 12), BlendMode::Multiply, 0.6).with_clipped(true),
            translucent("Zero", 9, 9, (2, 2), BlendMode::Normal, 0.0).with_clipped(true),
        ];
        let what = format!("styled group base {mode:?} {opacity}");
        assert_regions_match_with(&doc(layers.clone()), &what, &around((20, 12), 15, 15, 2));
        let mut only = layers;
        only.remove(2);
        assert_regions_match(&doc(only), &format!("{what}, zero-opacity member only"));
    }
}

#[test]
fn a_u16_styled_group_member_matches_under_an_unstyled_and_a_styled_base() {
    for (mode, opacity) in [(BlendMode::Normal, 1.0), (BlendMode::Multiply, 0.6)] {
        let children = vec![
            shaped("A", 17, 12, (12, 9), BlendMode::Normal, 1.0),
            translucent("B", 13, 10, (2, 14), BlendMode::Screen, 0.8),
        ];
        let member = group_of(children, GROUP_STYLE, mode, opacity).with_clipped(true);
        let under = shaped("Under", 19, 13, (6, 5), BlendMode::Normal, 1.0);
        let what = format!("styled group member {mode:?} {opacity}");
        assert_regions_match(&doc(vec![base(), under.clone(), member.clone()]), &what);
        let under = styled(
            under,
            &fx(&["{\"type\":\"stroke\",\"size\":2,\"color\":\"#ffffff\"}"]),
        );
        assert_regions_match(
            &doc(vec![base(), under, member]),
            &format!("{what}, styled base"),
        );
    }
}

// ---------------------------------------------------------------- groups --

#[test]
fn a_u16_styled_group_inside_other_groups_and_nested_matches() {
    let inner = || group_of(pair(), GROUP_STYLE, BlendMode::Normal, 0.8);
    for opacity in [1.0, 0.6] {
        let pass = Layer::pass_through("Outer", vec![inner()], opacity, true).unwrap();
        assert_regions_match(
            &doc(vec![base(), pass]),
            &format!("inside pass-through {opacity}"),
        );
    }
    let iso = Layer::isolated_normal("Outer", vec![inner()], 1.0, true)
        .unwrap()
        .with_blend_mode(BlendMode::Multiply)
        .unwrap();
    assert_regions_match(&doc(vec![base(), iso]), "inside a Multiply isolated group");
    // NESTED: styled inside styled.
    let outer_style = fx(&[
        "{\"type\":\"outer_glow\",\"size\":3}",
        "{\"type\":\"inner_shadow\",\"distance\":3,\"size\":2}",
    ]);
    for opacity in [1.0, 0.7] {
        let nested = group_of(vec![inner()], &outer_style, BlendMode::Normal, opacity);
        assert_regions_match(
            &doc(vec![base(), nested]),
            &format!("styled inside styled {opacity}"),
        );
    }
}

// ------------------------------------------------------------------ plan --

#[test]
fn a_u16_materialized_row_is_global_and_charges_the_collect_mirror() {
    // Inside the styled group: a Multiply isolated subgroup holding a CLIP
    // UNIT (a1 + clipped a2) — two buffer levels deep, one live clip level —
    // and a gated (0.5) pass-through subgroup over b.
    let a1 = shaped("a1", 13, 9, (4, 4), BlendMode::Normal, 1.0);
    let a2 = translucent("a2", 9, 9, (8, 6), BlendMode::Normal, 1.0).with_clipped(true);
    let b = translucent("b", 13, 9, (20, 12), BlendMode::Normal, 1.0);
    let children = || {
        vec![
            Layer::isolated_normal("A", vec![a1.clone(), a2.clone()], 1.0, true)
                .unwrap()
                .with_blend_mode(BlendMode::Multiply)
                .unwrap(),
            Layer::pass_through("B", vec![b.clone()], 0.5, true).unwrap(),
        ]
    };
    let json = fx(&[
        "{\"type\":\"stroke\",\"size\":3}",
        "{\"type\":\"color_overlay\"}",
    ]);
    let plain = Layer::isolated_normal("Styled", children(), 1.0, true).unwrap();
    let window = (4, 3, 9, 7);
    let q = plan(&doc(vec![base(), plain]), window, 1).unwrap();
    let d = doc(vec![
        base(),
        group_of(children(), &json, BlendMode::Normal, 1.0),
    ]);
    let p = plan(&d, window, 1).unwrap();
    let rows: Vec<u64> = p["layers"]
        .as_array()
        .unwrap()
        .iter()
        .map(|l| l["index"].as_u64().unwrap())
        .collect();
    assert_eq!(rows, [0, 1, 2, 3, 4, 5, 6]);
    let row = &p["layers"][6];
    assert_eq!(row["kind"], "group");
    assert_eq!(row["style"], "effects");
    assert_eq!(
        row["neighborhood"],
        json!({"kind":"global","halo_pixels":null})
    );
    assert_eq!(row["global_analysis"], "group_materialization");
    assert_eq!(p["global_analysis"], json!(["group_materialization"]));
    assert_eq!(
        p["required_input_region"],
        json!({"x":0,"y":0,"width":W,"height":H})
    );
    assert_eq!(p["layers"][2]["clip_base"], 1);
    assert_eq!(p["layers"][2]["group"], 3);
    assert_eq!(p["layers"][3]["group"], 6);
    assert_eq!(p["layers"][5]["group"], 6);
    assert_eq!(q["global_analysis"], false);
    // The restatement: canvas·16·(1 + 2) + canvas·4·1 + the group's planes
    // over the canvas-sized buffer (`style_scratch`: pad 5, two effects,
    // 3 taps: P = 51·39; P·6 + P·32 + 3·4 + 4096 + 16·min(39, 256)·51).
    let canvas = u64::from(W) * u64::from(H);
    let (wp, hp) = (u64::from(W) + 10, u64::from(H) + 10);
    let planes = wp * hp;
    let style = planes * 6 + planes * 32 + 12 + 4096 + 16 * hp.min(256) * wp;
    assert_eq!(
        p["scratch_heap_bytes"].as_u64().unwrap(),
        q["scratch_heap_bytes"].as_u64().unwrap() + canvas * 16 * 3 + canvas * 4 + style
    );
    // Work: the buffer fill, `collect`'s per-row work (A: 2·canvas and its
    // clip unit 3·canvas + a1 + a2; B: 2·canvas + b) and the planes'
    // P·(2 + 8·2).
    let sub = canvas * 2 + canvas * 3 + 13 * 9 + 9 * 9 + canvas * 2 + 13 * 9;
    assert_eq!(
        p["work_units"].as_u64().unwrap(),
        q["work_units"].as_u64().unwrap() + canvas + sub + planes * 18
    );
    assert_regions_match(&d, "plan fixture");
}

#[test]
fn a_u16_styled_raster_below_lists_both_analyses_and_a_feathered_group_mask_adds_no_halo() {
    let under = styled(
        shaped("Under", 17, 12, (8, 6), BlendMode::Normal, 1.0),
        &fx(&["{\"type\":\"stroke\",\"size\":2}"]),
    );
    let group = group_of(pair(), GROUP_STYLE, BlendMode::Normal, 1.0).with_mask(Some(mask(
        20,
        14,
        (4, 6),
        1.0,
        6.0,
    )));
    let d = doc(vec![base(), under, group]);
    let p = plan(&d, (12, 9, 5, 4), 1).unwrap();
    assert_eq!(p["layers"][1]["global_analysis"], "layer_style_planes");
    assert_eq!(p["layers"][4]["global_analysis"], "group_materialization");
    assert_eq!(p["layers"][4]["mask"], true);
    assert_eq!(
        p["global_analysis"],
        json!(["layer_style_planes", "group_materialization"])
    );
    assert_eq!(
        p["neighborhood"],
        json!({"kind":"pointwise","halo_pixels":0})
    );
    assert_eq!(
        p["required_input_region"],
        json!({"x":0,"y":0,"width":W,"height":H})
    );
    assert_regions_match(&d, "both analyses");
}

// ------------------------------------------------------------ refusals --

#[test]
fn refusals_inside_a_u16_materialized_group_come_in_order() {
    let knockout = "{\"fill_opacity\":0.5,\"knockout\":\"shallow\"}";
    let children = || {
        vec![
            styled(
                shaped("A", 17, 12, (6, 5), BlendMode::Normal, 1.0),
                knockout,
            ),
            translucent("B", 13, 10, (-4, 15), BlendMode::Screen, 0.8),
        ]
    };
    // Since 13A-3c a knockout on the materialized group and on its child are
    // admitted and equal the reference (the child's hole stops at the
    // group's buffer, D4's hard stop).
    let both = "{\"knockout\":\"shallow\",\"effects\":[{\"type\":\"drop_shadow\"}]}";
    assert_regions_match(
        &doc(vec![
            base(),
            group_of(children(), both, BlendMode::Normal, 1.0),
        ]),
        "a knockout group and child",
    );
    assert_regions_match(
        &doc(vec![
            base(),
            group_of(children(), GROUP_STYLE, BlendMode::Normal, 1.0),
        ]),
        "a knockout child in a materialized group",
    );
    // A guided Shadows/Highlights child: admitted since 13A-4b-i (the
    // reference renders the materialized subtree) and equal to it.
    let guided = adjustment(
        "shadows_highlights",
        json!({"shadows":{"amount":0.6,"tone":0.4},"radius":12}),
    );
    assert_regions_match(
        &doc(vec![
            base(),
            group_of(
                vec![shaped("A", 17, 12, (6, 5), BlendMode::Normal, 1.0), guided],
                GROUP_STYLE,
                BlendMode::Normal,
                1.0,
            ),
        ]),
        "a guided child in a materialized group",
    );
    // A HIDDEN styled group is skipped unchecked.
    let hidden = styled(
        Layer::isolated_normal("Group", children(), 1.0, false).unwrap(),
        GROUP_STYLE,
    );
    assert_regions_match(&doc(vec![base(), hidden]), "hidden styled group");
}

#[test]
fn a_u16_styled_group_document_refuses_linear_light() {
    let d = doc(vec![
        base(),
        group_of(pair(), GROUP_STYLE, BlendMode::Normal, 1.0),
    ]);
    assert!(d.with_evaluation(Evaluation::LinearLight).is_err());
}

// --------------------------------------------------------------- float --

fn edit(doc: &PrecisionDocument, request: Value) -> Result<PrecisionDocument, String> {
    let text = CString::new(request.to_string()).unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_edit(
            doc,
            text.as_ptr(),
            ptr::null(),
            0,
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    if out.is_null() {
        return Err(take(err));
    }
    Ok(*unsafe { Box::from_raw(out) })
}

#[test]
fn a_styled_float_group_renders_regionally_as_in_full() {
    // A float document holds a style on an isolated group, so
    // the float regional render meets a materialized group — its planes over
    // the group buffer, Blend If on the group — and equals the full render.
    let grouped = edit(
        &crate::float_fixture::shaped_pair(W, H),
        json!({"op":"group_layers","layers":[1],"name":"G","pass_through":false}),
    )
    .expect("grouped");
    // Postorder, bottom-first: the backdrop, the source inside, the group.
    let styled = edit(
        &grouped,
        json!({"op":"layer_style","version":1,"layer":2,"style":{
               "fill_opacity":0.6,
               "blend_if":{"channel":"gray","this_layer":[0,0,60,140],
                           "underlying":[0,0,255,255]},
               "effects":[{"type":"drop_shadow","blend":"multiply","color":"#ff8000",
                           "distance":3,"size":2,"use_global_light":false,"angle":90},
                          {"type":"stroke","size":2,"color":"#0040ff"}]}}),
    )
    .expect("a float group holds the style");
    assert!(styled.is_float());
    assert_regions_match(&styled, "styled float group");
    if !std::path::Path::new(FLOAT_FIXTURE).exists() {
        return;
    }
    let fixture = PrecisionDocument::open_preserving(FLOAT_FIXTURE, LIMIT, 0, None).unwrap();
    assert_regions_match(&fixture, "the float fixture");
}

// -------------------------------------------------------- cancellation --

#[test]
fn a_pre_cancelled_u16_styled_group_render_returns_no_output() {
    let d = doc(vec![
        base(),
        group_of(pair(), GROUP_STYLE, BlendMode::Normal, 0.6),
    ]);
    let control = unsafe { dr_job_control_create() };
    unsafe { dr_job_control_cancel(control) };
    let mut err: *mut c_char = ptr::null_mut();
    let out =
        unsafe { dr_precision_doc_render_region(&d, 0, 0, W, H, 1, LIMIT, 0, control, &mut err) };
    assert!(out.is_null());
    assert_eq!(take(err), "operation cancelled");
    assert_eq!(unsafe { dr_job_control_scratch_bytes(control) }, 0);
    unsafe { dr_job_control_free(control) };
}
