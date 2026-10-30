//! Phase 13A-3b-i: the 16-bit regional render over TYPED layer styles — every
//! effect on a raster, the package model, fill + typed Blend If beside
//! effects, masks, a styled clip base (planes' shape and the pad-0 shape) and
//! a styled member, styled rasters inside groups, and an effect-less styled
//! GROUP lone, as a clip base and with a member — against the full-frame
//! typed projection, plus the plan's `Global` rows and formulas, the reach
//! test, the refusals and the float policy. The oracle is INDEPENDENT of the
//! regional code: `dr_precision_doc_render`'s projection read back through
//! `dr_precision_image_pixels_rgba16`, cropped and top-left-cell sampled
//! here, compared as u16 codes.

use std::ffi::{c_char, CString};
use std::ptr;
use std::sync::Arc;

use doctored_core::doc::BlendMode;
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

fn doc_sized(w: u32, h: u32, layers: Vec<Layer>) -> PrecisionDocument {
    PrecisionDocument::from_parts(
        w,
        h,
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

// ------------------------------------------------------------- lone --

#[test]
fn every_effect_on_a_lone_u16_raster_matches_the_projection() {
    for (label, json) in every_effect() {
        for (mode, opacity) in VARIANTS {
            let top = styled(shaped("Top", 17, 12, (20, 12), mode, opacity), &json);
            let d = doc(vec![base(), top]);
            let what = format!("{label} {mode:?} {opacity}");
            assert_regions_match_with(&d, &what, &around((20, 12), 17, 12, 4));
        }
    }
    // Partly off the canvas.
    let json = fx(&[
        "{\"type\":\"drop_shadow\",\"distance\":9,\"size\":2}",
        "{\"type\":\"stroke\",\"size\":2}",
        "{\"type\":\"inner_glow\",\"size\":3}",
    ]);
    for (mode, opacity) in VARIANTS {
        let top = styled(shaped("Top", 17, 12, (-6, -4), mode, opacity), &json);
        assert_regions_match(&doc(vec![base(), top]), "off canvas");
    }
}

#[test]
fn fill_and_typed_blend_if_beside_effects_match_lone_and_as_a_base() {
    let json = format!(
        "{{\"fill_opacity\":0.5,{BLEND_IF_GRAY},\"effects\":[\
         {{\"type\":\"drop_shadow\",\"distance\":4,\"size\":2}},\
         {{\"type\":\"color_overlay\",\"opacity\":0.5}},\
         {{\"type\":\"inner_glow\",\"size\":3}}]}}"
    );
    for (mode, opacity) in VARIANTS {
        let top = styled(shaped("Top", 17, 12, (8, 6), mode, opacity), &json);
        assert!(top.style().is_some());
        let d = doc(vec![base(), top.clone()]);
        let what = format!("lone fill + blend if + effects {mode:?} {opacity}");
        assert_regions_match_with(&d, &what, &around((8, 6), 17, 12, 3));
        // As a clip base with a visible member.
        let member =
            translucent("Member", 15, 15, (16, 9), BlendMode::Multiply, 0.6).with_clipped(true);
        let d = doc(vec![base(), top, member]);
        assert_regions_match(&d, &format!("base {what}"));
    }
}

#[test]
fn a_masked_styled_u16_raster_matches_with_feather_0_and_6() {
    let json = fx(&[
        "{\"type\":\"drop_shadow\",\"distance\":3,\"size\":2}",
        "{\"type\":\"stroke\",\"size\":2,\"position\":\"center\"}",
        "{\"type\":\"inner_shadow\",\"distance\":2,\"size\":2}",
    ]);
    for feather in [0.0, 6.0] {
        for (mode, opacity) in [(BlendMode::Normal, 1.0), (BlendMode::Multiply, 0.6)] {
            let top = styled(shaped("Top", 17, 12, (8, 6), mode, opacity), &json)
                .with_mask(Some(mask(14, 10, (10, 7), 0.8, feather)));
            let d = doc(vec![base(), top]);
            let what = format!("masked feather {feather} {mode:?} {opacity}");
            assert_regions_match_with(&d, &what, &around((10, 7), 14, 10, 0));
        }
    }
}

// ------------------------------------------------------------ clip units --

const BASE_STYLE: &str = "{\"effects\":[{\"type\":\"drop_shadow\",\"distance\":5,\"size\":2},\
                          {\"type\":\"stroke\",\"size\":3},\
                          {\"type\":\"color_overlay\",\"blend\":\"multiply\",\"color\":\"#80c0ff\"}]}";

/// A base at (6, 5), a visible Multiply 0.6 member exceeding it and a
/// visible zero-opacity member (the reference still clamps after it).
fn clip_layers(mode: BlendMode, opacity: f32, base_style: &str) -> Vec<Layer> {
    vec![
        base(),
        styled(
            shaped("ClipBase", 19, 13, (6, 5), mode, opacity),
            base_style,
        ),
        translucent("Member", 15, 15, (14, 8), BlendMode::Multiply, 0.6).with_clipped(true),
        translucent("Zero", 9, 9, (2, 2), BlendMode::Normal, 0.0).with_clipped(true),
    ]
}

#[test]
fn a_styled_u16_clip_base_clamps_to_its_shape() {
    for (mode, opacity) in VARIANTS {
        let layers = clip_layers(mode, opacity, BASE_STYLE);
        let what = format!("styled base {mode:?} {opacity}");
        assert_regions_match_with(&doc(layers.clone()), &what, &around((6, 5), 19, 13, 4));
        // The zero-opacity member alone.
        let mut only = layers.clone();
        only.remove(2);
        assert_regions_match(&doc(only), &format!("{what}, zero-opacity member only"));
    }
    // A fill-only base: the pad-0 shape clamps (0 outside the layer rect).
    let fill = format!("{{\"fill_opacity\":0.3,{BLEND_IF_GRAY}}}");
    let d = doc(clip_layers(BlendMode::Multiply, 0.6, &fill));
    assert_regions_match_with(&d, "fill-only base", &around((6, 5), 19, 13, 1));
    let d = doc(clip_layers(BlendMode::Normal, 1.0, "{\"fill_opacity\":0}"));
    assert_regions_match(&d, "fill 0 base");
}

#[test]
fn a_styled_u16_member_matches_under_an_unstyled_and_a_styled_base() {
    let member = fx(&[
        "{\"type\":\"stroke\",\"size\":2,\"color\":\"#ffffff\"}",
        "{\"type\":\"drop_shadow\",\"distance\":3,\"size\":1}",
    ]);
    for opacity in [1.0, 0.6] {
        let mut layers = clip_layers(BlendMode::Normal, 1.0, BASE_STYLE);
        layers[1] = shaped("ClipBase", 19, 13, (6, 5), BlendMode::Normal, 1.0);
        layers[2] = styled(
            translucent("Member", 15, 15, (14, 8), BlendMode::Multiply, opacity),
            &member,
        )
        .with_clipped(true);
        let what = format!("styled member {opacity}");
        assert_regions_match_with(&doc(layers.clone()), &what, &around((14, 8), 15, 15, 3));
        layers[1] = styled(layers[1].clone(), BASE_STYLE);
        assert_regions_match(&doc(layers), &format!("{what}, styled base"));
    }
}

// ---------------------------------------------------------------- groups --

#[test]
fn a_styled_u16_raster_inside_isolated_and_pass_through_groups_matches() {
    let json = fx(&[
        "{\"type\":\"drop_shadow\",\"distance\":4,\"size\":2}",
        "{\"type\":\"outer_glow\",\"size\":3}",
        "{\"type\":\"satin\",\"distance\":3,\"size\":3}",
    ]);
    let children = || {
        vec![
            translucent("A", 15, 10, (3, 3), BlendMode::Normal, 1.0),
            styled(shaped("B", 17, 12, (12, 9), BlendMode::Screen, 0.7), &json),
        ]
    };
    for opacity in [1.0, 0.7] {
        let pass = Layer::pass_through("Pass", children(), opacity, true).unwrap();
        assert_regions_match(&doc(vec![base(), pass]), &format!("pass-through {opacity}"));
    }
    let iso = Layer::isolated_normal("Iso", children(), 0.8, true)
        .unwrap()
        .with_blend_mode(BlendMode::Multiply)
        .unwrap();
    assert_regions_match(&doc(vec![base(), iso.clone()]), "isolated multiply");
    let outer = Layer::isolated_normal("Outer", vec![iso], 0.9, true).unwrap();
    assert_regions_match(&doc(vec![base(), outer]), "nested");
}

#[test]
fn an_effect_less_styled_group_matches_lone_as_a_base_and_with_a_member() {
    let children = || {
        vec![
            translucent("A", 15, 10, (3, 3), BlendMode::Normal, 1.0),
            shaped("B", 17, 12, (12, 9), BlendMode::Screen, 0.7),
        ]
    };
    let fill_blend_if = format!("{{\"fill_opacity\":0.5,{BLEND_IF_GRAY}}}");
    for json in [
        "{\"fill_opacity\":0.5}",
        "{\"fill_opacity\":0}",
        fill_blend_if.as_str(),
    ] {
        for (mode, opacity) in [(BlendMode::Normal, 1.0), (BlendMode::Multiply, 0.7)] {
            let group = styled(
                Layer::isolated_normal("Group", children(), opacity, true)
                    .unwrap()
                    .with_blend_mode(mode)
                    .unwrap(),
                json,
            );
            let what = format!("{json} {mode:?} {opacity}");
            let d = doc(vec![base(), group.clone()]);
            assert_regions_match(&d, &format!("lone group {what}"));
            // Masked.
            let masked = group
                .clone()
                .with_mask(Some(mask(20, 16, (8, 5), 0.8, 3.0)));
            assert_regions_match(&doc(vec![base(), masked]), &format!("masked group {what}"));
            // As a clip base with a visible member.
            let member =
                translucent("Member", 15, 15, (14, 8), BlendMode::Multiply, 0.6).with_clipped(true);
            let d = doc(vec![base(), group.clone(), member.clone()]);
            assert_regions_match(&d, &format!("group base {what}"));
            // A styled group as a MEMBER.
            let d = doc(vec![
                base(),
                shaped("Under", 19, 13, (6, 5), BlendMode::Normal, 1.0),
                group.with_clipped(true),
            ]);
            assert_regions_match(&d, &format!("group member {what}"));
        }
    }
}

// ------------------------------------------------------------------ plan --

#[test]
fn a_typed_effects_row_is_global_and_charged_by_style_scratch() {
    // Outside stroke size 3 (pad 5) and a colour overlay: no blur (3 taps,
    // factor 1), two enabled effects. Restating `style_scratch`:
    // P = (w + 10)(h + 10); scratch P·(2 + 2·2) + P·32 + 3·4 + 4096 +
    // 16·min(hp, 256)·wp; work P·(2 + 8·2).
    let json = fx(&[
        "{\"type\":\"stroke\",\"size\":3}",
        "{\"type\":\"color_overlay\"}",
    ]);
    let plain = shaped("Top", 17, 12, (8, 6), BlendMode::Normal, 0.6);
    let window = (4, 3, 9, 7);
    let q = plan(&doc(vec![base(), plain.clone()]), window, 1).unwrap();
    let d = doc(vec![base(), styled(plain, &json)]);
    let p = plan(&d, window, 1).unwrap();
    let (wp, hp) = (17u64 + 10, 12u64 + 10);
    let planes = wp * hp;
    let scratch = planes * 6 + planes * 32 + 12 + 4096 + 16 * hp.min(256) * wp;
    assert_eq!(
        p["scratch_heap_bytes"].as_u64().unwrap(),
        q["scratch_heap_bytes"].as_u64().unwrap() + scratch
    );
    assert_eq!(
        p["work_units"].as_u64().unwrap(),
        q["work_units"].as_u64().unwrap() + planes * 18
    );
    let row = &p["layers"][1];
    assert_eq!(row["style"], "effects");
    assert_eq!(
        row["neighborhood"],
        json!({"kind":"global","halo_pixels":null})
    );
    assert_eq!(row["global_analysis"], "layer_style_planes");
    assert_eq!(p["global_analysis"], json!(["layer_style_planes"]));
    assert_eq!(
        p["required_input_region"],
        json!({"x":0,"y":0,"width":W,"height":H})
    );
    assert_eq!(q["layers"][1]["style"], Value::Null);
    assert_eq!(q["global_analysis"], false);
    // A fill-only row and a fill-only GROUP row: "fill", nothing charged.
    let fill = doc(vec![
        base(),
        styled(
            shaped("Top", 17, 12, (8, 6), BlendMode::Normal, 0.6),
            "{\"fill_opacity\":0.5}",
        ),
    ]);
    let f = plan(&fill, window, 1).unwrap();
    assert_eq!(f["layers"][1]["style"], "fill");
    assert_eq!(f["layers"][1]["global_analysis"], false);
    assert_eq!(f["global_analysis"], false);
    assert_eq!(f["scratch_heap_bytes"], q["scratch_heap_bytes"]);
    assert_eq!(f["work_units"], q["work_units"]);
    assert_eq!(
        f["required_input_region"],
        json!({"x":4,"y":3,"width":9,"height":7})
    );
    let group = styled(
        Layer::isolated_normal(
            "Group",
            vec![shaped("A", 17, 12, (8, 6), BlendMode::Normal, 1.0)],
            1.0,
            true,
        )
        .unwrap(),
        "{\"fill_opacity\":0.5}",
    );
    let g = plan(&doc(vec![base(), group]), window, 1).unwrap();
    assert_eq!(g["layers"][2]["kind"], "group");
    assert_eq!(g["layers"][2]["style"], "fill");
    assert_eq!(g["layers"][1]["style"], Value::Null);
    assert_eq!(g["global_analysis"], false);
    // A feathered mask on the effects row adds no halo; a feathered
    // unstyled layer beside it sets the top-level neighbourhood.
    let masked = styled(shaped("Top", 17, 12, (8, 6), BlendMode::Normal, 1.0), &json)
        .with_mask(Some(mask(14, 10, (10, 7), 1.0, 6.0)));
    let beside = translucent("Beside", 12, 9, (25, 15), BlendMode::Normal, 1.0)
        .with_mask(Some(mask(12, 9, (25, 15), 1.0, 3.0)));
    let p = plan(&doc(vec![base(), masked, beside]), window, 1).unwrap();
    assert_eq!(p["layers"][1]["neighborhood"]["kind"], "global");
    assert_eq!(p["layers"][2]["neighborhood"]["kind"], "spatial");
    assert_eq!(p["neighborhood"], p["layers"][2]["neighborhood"]);
    assert_eq!(
        p["required_input_region"],
        json!({"x":0,"y":0,"width":W,"height":H})
    );
}

// ----------------------------------------------------------------- reach --

fn reach_doc(corner: u16, json: &str) -> PrecisionDocument {
    let background =
        Layer::raster("Base", image16(120, 90, 1, |_, _| 65535), (0, 0), 1.0, true).unwrap();
    let image = image16(100, 70, 3, |x, y| match (x, y) {
        (0, 0) => corner,
        (0, _) | (_, 0) => 0,
        _ => 65535,
    });
    let top = Layer::raster("Top", image, (10, 10), 1.0, true)
        .unwrap()
        .with_layer_style(Some(&style(json)), (120, 90))
        .unwrap();
    doc_sized(120, 90, vec![background, top])
}

#[test]
fn a_far_pixel_outside_the_window_reaches_it_at_u16() {
    let aligned = fx(
        &["{\"type\":\"gradient_overlay\",\"gradient\":{\"angle\":30,\
                        \"align_with_layer\":true}}"],
    );
    let inner = fx(&["{\"type\":\"inner_shadow\",\"distance\":40,\"size\":0,\"opacity\":1}"]);
    for (label, json, window) in [
        ("aligned gradient", aligned.as_str(), (60, 45, 8, 6)),
        ("inner shadow 40", inner.as_str(), (27, 42, 7, 7)),
    ] {
        let on = reach_doc(65535, json);
        let off = reach_doc(0, json);
        assert_ne!(
            expected(&full(&on), window, 1),
            expected(&full(&off), window, 1),
            "{label}: the far pixel changes the window"
        );
        for d in [&on, &off] {
            let p = plan(d, window, 1).unwrap();
            assert_eq!(
                p["required_input_region"],
                json!({"x":0,"y":0,"width":120,"height":90}),
                "{label}"
            );
            let reference = full(d);
            for div in [1, 2, 3] {
                assert_eq!(
                    region(d, window, div).unwrap(),
                    expected(&reference, window, div),
                    "{label} {window:?}/{div}"
                );
            }
        }
    }
}

// ------------------------------------------------------------ refusals --

#[test]
fn knockout_and_grouped_interiors_are_admitted_beside_group_effects_at_u16() {
    // Since 13A-3c a knockout and a grouped interior take the unit pass
    // regionally and equal the reference (they refused before).
    let top = || shaped("Top", 17, 12, (5, 3), BlendMode::Normal, 0.9);
    assert_regions_match(
        &doc(vec![base(), styled(top(), "{\"knockout\":\"shallow\"}")]),
        "a Shallow knockout",
    );
    let grouped = "{\"blend_interior_as_group\":true,\"effects\":[{\"type\":\"color_overlay\"}]}";
    assert_regions_match(
        &doc(vec![base(), styled(top(), grouped)]),
        "a grouped interior",
    );
    let both = "{\"knockout\":\"deep\",\"blend_interior_as_group\":true,\
                \"effects\":[{\"type\":\"color_overlay\"}]}";
    assert_regions_match(
        &doc(vec![base(), styled(top(), both)]),
        "Deep beside a grouped interior",
    );
    // A group with an enabled effect is admitted since 13A-3b-ii (the
    // materialized group), and so is a knockout beside the effect.
    let group = |json: &str| {
        styled(
            Layer::isolated_normal("Group", vec![top()], 1.0, true).unwrap(),
            json,
        )
    };
    let shadow = fx(&["{\"type\":\"drop_shadow\",\"distance\":3,\"size\":2}"]);
    assert_regions_match(&doc(vec![base(), group(&shadow)]), "a group with effects");
    assert_regions_match(
        &doc(vec![
            base(),
            group("{\"knockout\":\"shallow\",\"effects\":[{\"type\":\"drop_shadow\"}]}"),
        ]),
        "a knockout group with effects",
    );
    // A knockout base under a grouped-interior member.
    let member = styled(
        translucent("Member", 9, 9, (2, 2), BlendMode::Normal, 1.0),
        grouped,
    )
    .with_clipped(true);
    let layers = vec![base(), styled(top(), "{\"knockout\":\"shallow\"}"), member];
    assert_regions_match(
        &doc(layers.clone()),
        "a knockout base with a grouped member",
    );
    // A guided Shadows/Highlights member of the same unit: under the
    // knockout (routed) base it is staged since 13A-4b-ii-b-2 and equals the
    // reference.
    let op = json!({"type":"adjust","op":"shadows_highlights",
                    "params":{"shadows":{"amount":0.6,"tone":0.4},"radius":12},
                    "evaluation":"precision_v1"});
    let guided = Layer::adjustment("Guided", &op.to_string(), 1.0, true, &profile())
        .unwrap()
        .with_clipped(true);
    let mut ordered = layers;
    ordered.push(guided);
    assert_regions_match(&doc(ordered), "a guided member under a knockout base");
    // Hidden, a styled group is skipped unchecked.
    let hidden = styled(
        Layer::isolated_normal("Group", vec![top()], 1.0, false).unwrap(),
        &shadow,
    );
    assert_regions_match(&doc(vec![base(), hidden]), "hidden styled group");
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
fn a_styled_float_document_renders_regionally_as_in_full() {
    // A float document holds a style, so the float regional
    // render meets one — fill, Below and Interior effects, Blend If and the
    // package model, over extended-range values — and equals the full render.
    let pair = crate::float_fixture::shaped_pair(W, H);
    for (style, opacity) in [
        (
            json!({"fill_opacity":0.5,"effects":[
                {"type":"drop_shadow","blend":"multiply","color":"#ff8000","opacity":0.75,
                 "distance":3,"size":2,"use_global_light":false,"angle":120},
                {"type":"color_overlay","blend":"linear_dodge","color":"#0040ff","opacity":0.5},
                {"type":"outer_glow","blend":"linear_dodge","color":"#ffffff","size":3}],
                "blend_if":{"channel":"red","this_layer":[0,0,60,140],
                            "underlying":[0,0,255,255]}}),
            1.0,
        ),
        (
            json!({"effects":[
                {"type":"stroke","position":"center","size":3,"color":"#ff8000"},
                {"type":"inner_glow","blend":"darken","color":"#ffffff","size":2},
                {"type":"gradient_overlay","opacity":0.6},
                {"type":"bevel_emboss","highlight_blend":"linear_dodge","size":3},
                {"type":"satin","size":2}]}),
            0.6,
        ),
    ] {
        let mut d = edit(
            &pair,
            json!({"op":"layer_style","version":1,"layer":1,"style":style}),
        )
        .expect("a float document holds the style");
        if opacity < 1.0 {
            let mut err: *mut c_char = ptr::null_mut();
            let out = unsafe {
                doctored_core::ffi_precision::dr_precision_doc_set_layer_opacity(
                    &d, 1, opacity, &mut err,
                )
            };
            assert!(!out.is_null(), "{}", take(err));
            d = *unsafe { Box::from_raw(out) };
        }
        assert!(d.is_float());
        assert_regions_match(&d, "styled float document");
    }
    // A part the float mode cannot hold refuses by name at the edit.
    let refused = edit(
        &pair,
        json!({"op":"layer_style","version":1,"layer":1,"style":{
               "effects":[{"type":"outer_glow","blend":"screen"}]}}),
    )
    .err()
    .expect("refused");
    assert_eq!(
        refused,
        "the screen blend mode on the outer_glow layer effect is unavailable in a 32-bit float document"
    );
    if !std::path::Path::new(FLOAT_FIXTURE).exists() {
        return;
    }
    let fixture = PrecisionDocument::open_preserving(FLOAT_FIXTURE, LIMIT, 0, None).unwrap();
    // The float policy is the plan's FIRST check, so a record it refuses
    // refuses the plan, the regional render and the full render in one
    // sentence.
    // (Point-form Curves is admitted; Vibrance stays
    // refused at 32 bits.)
    let vibrance = fixture
        .add_adjustment(
            "Invert",
            "{\"type\":\"adjust\",\"evaluation\":\"precision_v1\",\"op\":\"invert\",\
             \"params\":{}}",
            1.0,
            LIMIT,
            0,
            None,
        )
        .unwrap()
        .unwrap();
    // Invert is refused at float by decision; Vibrance runs since then.
    let reason = vibrance.float_refusal().expect("invert is refused");
    let window = (0, 0, 4, 4);
    assert_eq!(plan(&vibrance, window, 1).unwrap_err(), reason);
    assert_eq!(region(&vibrance, window, 1).unwrap_err(), reason);
    assert_eq!(vibrance.render(LIMIT, 0, None).err().unwrap(), reason);
}

// -------------------------------------------------------- cancellation --

#[test]
fn a_pre_cancelled_styled_u16_render_returns_no_output() {
    let d = doc(clip_layers(BlendMode::Normal, 0.6, BASE_STYLE));
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
