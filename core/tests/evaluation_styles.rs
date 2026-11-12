//! P7-24 phase 3: layer styles and Blend If under linear-light evaluation, at
//! both depths, full frame and regional, against `linear_oracle`.
//!
//! The rule under test: only what enters the accumulator is decoded — the
//! layer's stored pixels and each effect's document-space ink — and every
//! alpha, coverage, Fill, package and blend statement runs unchanged on
//! linear components. Blend If reads ENCODED values on both sides (the
//! stored pixel as it is, the Underlying composite re-encoded), so a range
//! selects the pixels it selects in the encoded document over the same
//! picture. A gradient effect keeps its encoded ramp, so where it is opaque
//! it shows the encoded document's pixels.
//!
//! Every byte case is also rendered by `dr_doc_render_region` and every typed
//! one by `dr_precision_doc_render_region`, which must equal the full frame —
//! except a styled adjustment, which the regional compositor refuses by name
//! under either policy.

use std::ffi::{c_char, CString};
use std::path::Path;
use std::ptr;
use std::sync::Arc;

use doctored_core::doc::{BlendMode, DrDocument};
use doctored_core::ffi_precision::{dr_precision_doc_render, dr_precision_image_free};
use doctored_core::ffi_psd::{
    dr_doc_open_psd, dr_doc_save_psd, dr_precision_doc_open_psd, dr_precision_doc_save_psd,
    dr_psd_import_report,
};
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{
    PrecisionDocument, PrecisionDocumentLayer as PrecisionLayer,
};
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use doctored_core::style::LayerStyle;
use image::{Rgba, RgbaImage};
use tempfile::TempDir;

use crate::linear_oracle::{
    assert_within, encode_opaque, flatten, lerp3, lin, linear, pixel, region, typed_code,
    typed_edit, typed_encode_opaque, typed_linear, typed_region, typed_render, typed_within,
    with_meta,
};

const W: u32 = 24;
const H: u32 = 24;
const BG: [u8; 3] = [180, 140, 90];
const LAYER: [u8; 3] = [40, 120, 200];
const OVERLAY: [u8; 3] = [0x33, 0x66, 0xcc];
const RED: [u8; 3] = [255, 0, 0];
/// Mid-tone effect colours, so an undecoded ink is far from a decoded one.
const SHADOW: [u8; 3] = [0x40, 0x20, 0x10];
const STROKE: [u8; 3] = [0xc0, 0x80, 0x30];
/// The layer's rect, `x0..x1` × `y0..y1`.
const RECT: (u32, u32, u32, u32) = (6, 6, 14, 14);
/// 0.6 is exactly the opacity byte 153, so no quantization separates the
/// oracle's weight from the compositor's.
const PART: f64 = 0.6;

fn inside(x: u32, y: u32) -> bool {
    x >= RECT.0 && x < RECT.2 && y >= RECT.1 && y < RECT.3
}

fn index(x: u32, y: u32) -> usize {
    (y * W + x) as usize
}

fn filled(f: impl Fn(u32, u32) -> Option<[u8; 3]>) -> RgbaImage {
    RgbaImage::from_fn(W, H, |x, y| match f(x, y) {
        Some([r, g, b]) => Rgba([r, g, b, 255]),
        None => Rgba([0, 0, 0, 0]),
    })
}

fn rect_of(color: [u8; 3]) -> RgbaImage {
    filled(|x, y| inside(x, y).then_some(color))
}

fn byte_doc(backdrop: &RgbaImage, top: &RgbaImage) -> DrDocument {
    DrDocument::from_pixels(backdrop.clone())
        .adding_image_layer(0, top.clone(), "Top")
        .expect("the top layer")
}

fn with_style(doc: &DrDocument, idx: usize, json: &str) -> DrDocument {
    let style = LayerStyle::from_json(json).unwrap_or_else(|e| panic!("{json}: {e}"));
    doc.set_layer_style(idx, Some(Arc::new(style)))
        .expect("a non-identity style lands")
}

/// The linear projection, checked against the regional render first.
fn byte_linear(doc: &DrDocument, what: &str) -> Vec<u8> {
    let switched = linear(doc);
    let full = flatten(&switched).unwrap_or_else(|e| panic!("{what}: {e}"));
    assert!(
        region(&switched).unwrap_or_else(|e| panic!("{what} region: {e}")) == full,
        "{what}: the regional render is the full one"
    );
    full
}

fn background() -> RgbaImage {
    filled(|_, _| Some(BG))
}

/// `encode(lerp(decode(a), decode(b), t))`, opaque.
fn mixed(a: [u8; 3], b: [u8; 3], t: f64) -> [u8; 4] {
    encode_opaque(lerp3(a.map(lin), b.map(lin), t))
}

fn typed_mixed(a: [u8; 3], b: [u8; 3], t: f64) -> [u16; 4] {
    typed_encode_opaque(lerp3(a.map(lin), b.map(lin), t))
}

fn opaque(c: [u8; 3]) -> [u8; 4] {
    [c[0], c[1], c[2], 255]
}

fn rgb(p: [u8; 4]) -> [u8; 3] {
    [p[0], p[1], p[2]]
}

// ------------------------------------------------------------------ byte --

#[test]
fn an_overlay_a_fill_and_the_package_lerp_in_linear_at_byte_depth() {
    let doc = byte_doc(&background(), &rect_of(LAYER));
    let overlay = with_style(
        &doc,
        1,
        r##"{"effects":[{"type":"color_overlay","color":"#3366cc","opacity":0.5}]}"##,
    );
    let fill = with_style(&doc, 1, r#"{"fill_opacity":0.6}"#);
    let package = with_style(
        &doc,
        1,
        r##"{"effects":[{"type":"color_overlay","color":"#3366cc","opacity":1}]}"##,
    )
    .with_layer_opacity(1, 0.6)
    .expect("a new opacity");
    for (what, doc, inner) in [
        ("overlay at 50 %", &overlay, mixed(LAYER, OVERLAY, 0.5)),
        ("fill at 60 %", &fill, mixed(BG, LAYER, PART)),
        (
            "a 100 % overlay at layer opacity 60 %",
            &package,
            mixed(BG, OVERLAY, PART),
        ),
    ] {
        let bytes = byte_linear(doc, what);
        for y in 0..H {
            for x in 0..W {
                let want = if inside(x, y) { inner } else { opaque(BG) };
                assert_within(
                    pixel(&bytes, index(x, y)),
                    want,
                    1,
                    &format!("{what} at ({x}, {y})"),
                );
            }
        }
    }
}

#[test]
fn a_drop_shadow_and_an_outside_stroke_composite_in_linear_at_byte_depth() {
    let doc = byte_doc(&background(), &rect_of(LAYER));
    let shadow = with_style(
        &doc,
        1,
        r##"{"effects":[{"type":"drop_shadow","color":"#402010","blend":"normal","opacity":0.6,"angle":0,"use_global_light":false,"distance":4,"spread":0,"size":0}]}"##,
    );
    let encoded = flatten(&shadow).expect("the encoded projection");
    let bytes = byte_linear(&shadow, "drop shadow");
    let mut shaded = 0;
    for y in 0..H {
        for x in 0..W {
            let i = index(x, y);
            if inside(x, y) {
                assert_eq!(pixel(&bytes, i), opaque(LAYER), "the layer at ({x}, {y})");
                continue;
            }
            if rgb(pixel(&encoded, i)) == BG {
                assert_eq!(pixel(&bytes, i), opaque(BG), "no shadow at ({x}, {y})");
                continue;
            }
            // A hard shadow at full coverage: the encoded document's own
            // pixel is the encoded-domain lerp (the fixture's guard).
            let flat = [0, 1, 2].map(|c| {
                (f64::from(BG[c]) + (f64::from(SHADOW[c]) - f64::from(BG[c])) * PART).round() as u8
            });
            assert_within(
                pixel(&encoded, i),
                opaque(flat),
                1,
                "the fixture's hard shadow",
            );
            assert_within(
                pixel(&bytes, i),
                mixed(BG, SHADOW, PART),
                1,
                &format!("shadow at ({x}, {y})"),
            );
            shaded += 1;
        }
    }
    assert!(shaded >= 8 * 4, "the shadow lands on {shaded} pixels");

    let stroke = with_style(
        &doc,
        1,
        r##"{"effects":[{"type":"stroke","size":2,"position":"outside","color":"#c08030","opacity":0.5}]}"##,
    );
    let encoded = flatten(&stroke).expect("the encoded projection");
    let bytes = byte_linear(&stroke, "outside stroke");
    // One pixel left of the rect, away from its corners: inside the band.
    for y in RECT.1 + 2..RECT.3 - 2 {
        let i = index(RECT.0 - 1, y);
        let flat =
            [0, 1, 2].map(|c| ((f64::from(BG[c]) + f64::from(STROKE[c])) / 2.0).round() as u8);
        assert_within(pixel(&encoded, i), opaque(flat), 1, "the fixture's band");
        assert_within(
            pixel(&bytes, i),
            mixed(BG, STROKE, 0.5),
            1,
            &format!("stroke at y {y}"),
        );
    }
}

const GRADIENT: &str = r##"{"effects":[{"type":"gradient_overlay","gradient":{"stops":[{"position":0,"color":"#102030","opacity":1},{"position":1,"color":"#f0e0d0","opacity":1}]}}]}"##;

#[test]
fn a_gradient_overlay_keeps_its_encoded_ramp_at_byte_depth() {
    let doc = with_style(&byte_doc(&background(), &rect_of(LAYER)), 1, GRADIENT);
    let encoded = flatten(&doc).expect("the encoded projection");
    let bytes = byte_linear(&doc, "gradient overlay");
    for i in 0..(W * H) as usize {
        assert_within(
            pixel(&bytes, i),
            pixel(&encoded, i),
            1,
            &format!("gradient at {i}"),
        );
    }
    let shown: Vec<[u8; 3]> = (RECT.1..RECT.3)
        .map(|y| rgb(pixel(&bytes, index(10, y))))
        .collect();
    assert!(
        shown.windows(2).any(|w| w[0] != w[1]),
        "the ramp varies down the rect"
    );
}

const GRAY_LOW: [u8; 3] = [100, 100, 100];
const GRAY_HIGH: [u8; 3] = [150, 150, 150];
const GRAY_BG: [u8; 3] = [200, 200, 200];
const GRAY_MID: [u8; 3] = [120, 120, 120];
const THIS_LAYER: &str =
    r#"{"blend_if":{"channel":"gray","this_layer":[0,0,120,120],"underlying":[0,0,255,255]}}"#;
const UNDERLYING: &str =
    r#"{"blend_if":{"channel":"gray","this_layer":[0,0,255,255],"underlying":[0,0,120,120]}}"#;
/// `(140 − 120) / (140 − 100)`: a code-120 pixel weighs exactly a half.
const SPLIT: &str =
    r#"{"blend_if":{"channel":"gray","this_layer":[0,0,100,140],"underlying":[0,0,255,255]}}"#;

/// A layer of code 100 on the rect's left half and 150 on its right.
fn two_tone_layer() -> RgbaImage {
    filled(|x, y| inside(x, y).then_some(if x < 10 { GRAY_LOW } else { GRAY_HIGH }))
}

/// A backdrop of code 100 on the left half of the canvas and 150 on the right.
fn two_tone_backdrop() -> RgbaImage {
    filled(|x, _| Some(if x < 10 { GRAY_LOW } else { GRAY_HIGH }))
}

fn gray_background() -> RgbaImage {
    filled(|_, _| Some(GRAY_BG))
}

/// Byte Blend If oracle per construct: what each rect pixel must be.
fn blend_if_expect(case: &str, x: u32) -> [u8; 4] {
    match case {
        "this" => opaque(if x < 10 { GRAY_LOW } else { GRAY_BG }),
        "under" => opaque(if x < 10 { LAYER } else { GRAY_HIGH }),
        "split" => mixed(GRAY_BG, GRAY_MID, 0.5),
        other => panic!("{other}"),
    }
}

fn blend_if_docs() -> Vec<(&'static str, RgbaImage, RgbaImage, &'static str)> {
    vec![
        ("this", gray_background(), two_tone_layer(), THIS_LAYER),
        ("under", two_tone_backdrop(), rect_of(LAYER), UNDERLYING),
        ("split", gray_background(), rect_of(GRAY_MID), SPLIT),
    ]
}

fn assert_blend_if(bytes: &[u8], backdrop: &RgbaImage, case: &str, what: &str) {
    for y in 0..H {
        for x in 0..W {
            let want = if inside(x, y) {
                blend_if_expect(case, x)
            } else {
                backdrop.get_pixel(x, y).0
            };
            assert_within(
                pixel(bytes, index(x, y)),
                want,
                1,
                &format!("{what} {case} at ({x}, {y})"),
            );
        }
    }
}

#[test]
fn blend_if_selects_what_the_encoded_document_selects_at_byte_depth() {
    for (case, backdrop, top, json) in blend_if_docs() {
        let doc = byte_doc(&backdrop, &top);
        // On the layer itself.
        let layer = with_style(&doc, 1, json);
        assert_blend_if(&byte_linear(&layer, case), &backdrop, case, "layer");
        // On an isolated group around it.
        let grouped = doc.group_layers(&[1], "Group").expect("a group").0;
        let grouped = grouped
            .with_layer_blend_mode(2, BlendMode::Normal)
            .expect("the group isolates");
        let group = with_style(&grouped, 2, json);
        assert_blend_if(&byte_linear(&group, case), &backdrop, case, "group");
        // On a clip base with one clipped member over the rect's top half:
        // wherever the encoded document hides the base the linear one hides
        // it too.
        let member = filled(|x, y| (inside(x, y) && y < 10).then_some(OVERLAY));
        let clip = with_style(&doc, 1, json)
            .adding_image_layer(1, member, "Member")
            .expect("the member")
            .with_layer_clipped(2, true)
            .expect("clipped");
        let encoded = flatten(&clip).expect("the encoded clip unit");
        let bytes = byte_linear(&clip, &format!("clip base {case}"));
        for i in 0..(W * H) as usize {
            let shown = |b: &[u8]| {
                rgb(pixel(b, i)) != rgb(backdrop.get_pixel(i as u32 % W, i as u32 / W).0)
            };
            assert_eq!(shown(&bytes), shown(&encoded), "clip base {case} at {i}");
        }
    }

    // A styled ADJUSTMENT: Invert, gated by the Underlying range.
    let backdrop = two_tone_backdrop();
    let doc = DrDocument::from_pixels(backdrop.clone())
        .adding_image_layer(0, RgbaImage::new(1, 1), "Invert")
        .expect("the adjustment entry");
    let doc = with_style(
        &with_meta(&doc, 1, r#"{"type":"adjust","op":"invert"}"#),
        1,
        UNDERLYING,
    );
    let switched = linear(&doc);
    let bytes = flatten(&switched).expect("a styled adjustment projects");
    for y in 0..H {
        for x in 0..W {
            let want = if x < 10 {
                [155, 155, 155, 255]
            } else {
                opaque(GRAY_HIGH)
            };
            assert_eq!(pixel(&bytes, index(x, y)), want, "adjustment at ({x}, {y})");
        }
    }
    let refusal = region(&switched).expect_err("a styled adjustment is not regional");
    assert!(
        refusal.contains("adjustment layer with a layer style"),
        "{refusal}"
    );
}

#[test]
fn a_merge_down_bakes_the_styled_layer_as_the_canvas_shows_it_at_byte_depth() {
    let doc = with_style(
        &byte_doc(&background(), &rect_of(LAYER)),
        1,
        r##"{"effects":[{"type":"color_overlay","color":"#3366cc","opacity":0.5},{"type":"drop_shadow","color":"#000000","opacity":0.6,"distance":3,"size":2}]}"##,
    );
    let switched = linear(&doc);
    let before = flatten(&switched).expect("the styled projection");
    let merged = switched.merging_down(1).expect("a merge");
    let after = flatten(&merged).expect("the merged projection");
    for i in 0..(W * H) as usize {
        assert_within(
            pixel(&after, i),
            pixel(&before, i),
            1,
            &format!("merged at {i}"),
        );
    }
}

#[test]
fn a_knockout_and_a_styled_clip_base_render_regionally_at_byte_depth() {
    let doc = byte_doc(&background(), &rect_of(LAYER));
    let knockout = with_style(
        &doc.adding_image_layer(1, rect_of(OVERLAY), "Knock")
            .expect("the knockout layer"),
        2,
        r##"{"knockout":"shallow","fill_opacity":0.4,"effects":[{"type":"stroke","size":2,"color":"#ff0000"}]}"##,
    );
    byte_linear(&knockout, "shallow knockout");
    let member = filled(|x, y| (inside(x, y) && x < 10).then_some(RED));
    let clip = with_style(
        &doc,
        1,
        r##"{"effects":[{"type":"color_overlay","color":"#3366cc","opacity":0.5}]}"##,
    )
    .adding_image_layer(1, member, "Member")
    .expect("the member")
    .with_layer_clipped(2, true)
    .expect("clipped");
    let bytes = byte_linear(&clip, "styled clip base");
    // The base's Interior overlay lands over the whole unit: over the member
    // where it covers, over the base elsewhere.
    assert_within(
        pixel(&bytes, index(8, 8)),
        mixed(RED, OVERLAY, 0.5),
        1,
        "member",
    );
    assert_within(
        pixel(&bytes, index(12, 8)),
        mixed(LAYER, OVERLAY, 0.5),
        1,
        "base",
    );
}

// ----------------------------------------------------------------- typed --

fn typed_pixels(image: &RgbaImage) -> Vec<[u16; 4]> {
    image
        .pixels()
        .map(|p| {
            [
                typed_code(p[0]),
                typed_code(p[1]),
                typed_code(p[2]),
                typed_code(p[3]),
            ]
        })
        .collect()
}

fn typed_raster(name: &str, image: &RgbaImage) -> PrecisionLayer {
    let image = PrecisionImage::from_rgba16(W, H, typed_pixels(image), IccProfile::srgb(), None)
        .expect("the fixture image");
    PrecisionLayer::raster(name, image, (0, 0), 1.0, true).expect("a raster")
}

fn typed_from(layers: Vec<PrecisionLayer>) -> PrecisionDocument {
    PrecisionDocument::from_parts(
        W,
        H,
        IccProfile::srgb(),
        Metadata::default(),
        Resolution::default(),
        layers,
    )
    .expect("the fixture document")
}

fn typed_doc(backdrop: &RgbaImage, top: &RgbaImage) -> PrecisionDocument {
    typed_from(vec![
        typed_raster("Base", backdrop),
        typed_raster("Top", top),
    ])
}

fn typed_style(doc: &PrecisionDocument, layer: usize, style: &str) -> PrecisionDocument {
    typed_edit(
        doc,
        &format!(r#"{{"op":"layer_style","version":1,"layer":{layer},"style":{style}}}"#),
    )
    .expect("the style lands")
}

/// A byte-schema Blend If style as the typed `layer_blend_if` edit.
fn typed_blend_if(doc: &PrecisionDocument, layer: usize, style: &str) -> PrecisionDocument {
    let value: serde_json::Value = serde_json::from_str(style).expect("a style");
    typed_edit(
        doc,
        &format!(
            r#"{{"op":"layer_blend_if","version":1,"layer":{layer},"blend_if":{}}}"#,
            value["blend_if"]
        ),
    )
    .expect("the Blend If lands")
}

fn typed_linear_checked(doc: &PrecisionDocument, what: &str) -> Vec<[u16; 4]> {
    let switched = typed_linear(doc);
    let full = typed_render(&switched).unwrap_or_else(|e| panic!("{what}: {e}"));
    assert!(
        typed_region(&switched).unwrap_or_else(|e| panic!("{what} region: {e}")) == full,
        "{what}: the typed regional render is the full one"
    );
    full
}

fn typed_opaque(c: [u8; 3]) -> [u16; 4] {
    [typed_code(c[0]), typed_code(c[1]), typed_code(c[2]), 65535]
}

#[test]
fn an_overlay_a_fill_and_the_package_lerp_in_linear_at_typed_depth() {
    let doc = typed_doc(&background(), &rect_of(LAYER));
    let overlay = typed_style(
        &doc,
        1,
        r##"{"effects":[{"type":"color_overlay","color":"#3366cc","opacity":0.5}]}"##,
    );
    let fill = typed_style(&doc, 1, r#"{"fill_opacity":0.6}"#);
    let package = typed_from(vec![typed_raster("Base", &background()), {
        let image = PrecisionImage::from_rgba16(
            W,
            H,
            typed_pixels(&rect_of(LAYER)),
            IccProfile::srgb(),
            None,
        )
        .expect("the fixture image");
        PrecisionLayer::raster("Top", image, (0, 0), 0.6, true).expect("a raster")
    }]);
    let package = typed_style(
        &package,
        1,
        r##"{"effects":[{"type":"color_overlay","color":"#3366cc","opacity":1}]}"##,
    );
    for (what, doc, inner) in [
        (
            "overlay at 50 %",
            &overlay,
            typed_mixed(LAYER, OVERLAY, 0.5),
        ),
        ("fill at 60 %", &fill, typed_mixed(BG, LAYER, PART)),
        (
            "a 100 % overlay at layer opacity 60 %",
            &package,
            typed_mixed(BG, OVERLAY, PART),
        ),
    ] {
        let samples = typed_linear_checked(doc, what);
        for y in 0..H {
            for x in 0..W {
                let want = if inside(x, y) {
                    inner
                } else {
                    typed_opaque(BG)
                };
                typed_within(
                    samples[index(x, y)],
                    want,
                    &format!("typed {what} at ({x}, {y})"),
                );
            }
        }
    }
}

#[test]
fn a_drop_shadow_and_an_outside_stroke_composite_in_linear_at_typed_depth() {
    let doc = typed_doc(&background(), &rect_of(LAYER));
    let shadow = typed_style(
        &doc,
        1,
        r##"{"effects":[{"type":"drop_shadow","color":"#402010","blend":"normal","opacity":0.6,"angle":0,"use_global_light":false,"distance":4,"spread":0,"size":0}]}"##,
    );
    let encoded = typed_render(&shadow).expect("the encoded typed projection");
    let samples = typed_linear_checked(&shadow, "typed drop shadow");
    let mut shaded = 0;
    for y in 0..H {
        for x in 0..W {
            let i = index(x, y);
            if inside(x, y) || encoded[i] == typed_opaque(BG) {
                continue;
            }
            typed_within(
                samples[i],
                typed_mixed(BG, SHADOW, PART),
                &format!("typed shadow at ({x}, {y})"),
            );
            shaded += 1;
        }
    }
    assert!(shaded >= 8 * 4, "the shadow lands on {shaded} pixels");

    let stroke = typed_style(
        &doc,
        1,
        r##"{"effects":[{"type":"stroke","size":2,"position":"outside","color":"#c08030","opacity":0.5}]}"##,
    );
    let samples = typed_linear_checked(&stroke, "typed outside stroke");
    for y in RECT.1 + 2..RECT.3 - 2 {
        typed_within(
            samples[index(RECT.0 - 1, y)],
            typed_mixed(BG, STROKE, 0.5),
            &format!("typed stroke at y {y}"),
        );
    }
}

#[test]
fn a_gradient_overlay_keeps_its_encoded_ramp_at_typed_depth() {
    let doc = typed_style(&typed_doc(&background(), &rect_of(LAYER)), 1, GRADIENT);
    let encoded = typed_render(&doc).expect("the encoded typed projection");
    let samples = typed_linear_checked(&doc, "typed gradient overlay");
    for (i, (got, want)) in samples.iter().zip(&encoded).enumerate() {
        typed_within(*got, *want, &format!("typed gradient at {i}"));
    }
}

/// The typed Blend If oracle: the byte one's codes, promoted.
fn typed_blend_if_expect(case: &str, x: u32) -> [u16; 4] {
    match case {
        "this" => typed_opaque(if x < 10 { GRAY_LOW } else { GRAY_BG }),
        "under" => typed_opaque(if x < 10 { LAYER } else { GRAY_HIGH }),
        "split" => typed_mixed(GRAY_BG, GRAY_MID, 0.5),
        other => panic!("{other}"),
    }
}

fn assert_typed_blend_if(samples: &[[u16; 4]], backdrop: &RgbaImage, case: &str, what: &str) {
    for y in 0..H {
        for x in 0..W {
            let want = if inside(x, y) {
                typed_blend_if_expect(case, x)
            } else {
                typed_opaque(rgb(backdrop.get_pixel(x, y).0))
            };
            typed_within(
                samples[index(x, y)],
                want,
                &format!("typed {what} {case} at ({x}, {y})"),
            );
        }
    }
}

#[test]
fn blend_if_selects_what_the_encoded_document_selects_at_typed_depth() {
    for (case, backdrop, top, json) in blend_if_docs() {
        let doc = typed_doc(&backdrop, &top);
        let layer = typed_blend_if(&doc, 1, json);
        assert_typed_blend_if(
            &typed_linear_checked(&layer, case),
            &backdrop,
            case,
            "layer",
        );
        let grouped = typed_edit(
            &doc,
            r#"{"op":"group_layers","layers":[1],"name":"Group","pass_through":false}"#,
        )
        .expect("a group");
        let group = typed_blend_if(&grouped, 2, json);
        assert_typed_blend_if(
            &typed_linear_checked(&group, case),
            &backdrop,
            case,
            "group",
        );
        let clip = typed_from(vec![
            typed_raster("Base", &backdrop),
            typed_raster("Top", &top),
            typed_raster(
                "Member",
                &filled(|x, y| (inside(x, y) && y < 10).then_some(OVERLAY)),
            ),
        ]);
        let clip = typed_edit(&clip, r#"{"op":"layer_clipped","layer":2,"clipped":true}"#)
            .expect("clipped");
        let clip = typed_blend_if(&clip, 1, json);
        let encoded = typed_render(&clip).expect("the encoded typed clip unit");
        let samples = typed_linear_checked(&clip, &format!("typed clip base {case}"));
        let ground: Vec<[u16; 4]> = typed_pixels(&backdrop);
        for i in 0..(W * H) as usize {
            let shown = |s: &[[u16; 4]]| {
                (0..3).any(|c| (i64::from(s[i][c]) - i64::from(ground[i][c])).abs() > 24)
            };
            assert_eq!(
                shown(&samples),
                shown(&encoded),
                "typed clip base {case} at {i}"
            );
        }
    }

    let backdrop = two_tone_backdrop();
    let adjustment = PrecisionLayer::adjustment(
        "Invert",
        r#"{"type":"adjust","evaluation":"precision_v1","op":"invert","params":{}}"#,
        1.0,
        true,
        &IccProfile::srgb(),
    )
    .expect("the typed adjustment");
    let doc = typed_from(vec![typed_raster("Base", &backdrop), adjustment]);
    let doc = typed_blend_if(&doc, 1, UNDERLYING);
    let switched = typed_linear(&doc);
    let samples = typed_render(&switched).expect("a styled typed adjustment renders");
    for y in 0..H {
        for x in 0..W {
            let want = typed_opaque(if x < 10 { [155, 155, 155] } else { GRAY_HIGH });
            typed_within(
                samples[index(x, y)],
                want,
                &format!("typed adjustment at ({x}, {y})"),
            );
        }
    }
    let refusal = typed_region(&switched).expect_err("a styled adjustment is not regional");
    assert!(
        refusal.contains("adjustment layer with a layer style"),
        "{refusal}"
    );
}

#[test]
fn a_merge_down_bakes_the_styled_layer_as_the_canvas_shows_it_at_typed_depth() {
    let doc = typed_style(
        &typed_doc(&background(), &rect_of(LAYER)),
        1,
        r##"{"effects":[{"type":"color_overlay","color":"#3366cc","opacity":0.5},{"type":"drop_shadow","color":"#000000","opacity":0.6,"distance":3,"size":2}]}"##,
    );
    let switched = typed_linear(&doc);
    let before = typed_render(&switched).expect("the styled typed projection");
    let merged = typed_edit(&switched, r#"{"op":"merge_down","layer":1}"#).expect("a merge");
    let after = typed_render(&merged).expect("the merged typed projection");
    for (i, (got, want)) in after.iter().zip(&before).enumerate() {
        typed_within(*got, *want, &format!("typed merged at {i}"));
    }
}

#[test]
fn a_knockout_and_a_styled_clip_base_render_regionally_at_typed_depth() {
    let knockout = typed_from(vec![
        typed_raster("Base", &background()),
        typed_raster("Top", &rect_of(LAYER)),
        typed_raster("Knock", &rect_of(OVERLAY)),
    ]);
    let knockout = typed_style(
        &knockout,
        2,
        r##"{"knockout":"shallow","fill_opacity":0.4,"effects":[{"type":"stroke","size":2,"color":"#ff0000"}]}"##,
    );
    typed_linear_checked(&knockout, "typed shallow knockout");
    let member = filled(|x, y| (inside(x, y) && x < 10).then_some(RED));
    let clip = typed_from(vec![
        typed_raster("Base", &background()),
        typed_raster("Top", &rect_of(LAYER)),
        typed_raster("Member", &member),
    ]);
    let clip =
        typed_edit(&clip, r#"{"op":"layer_clipped","layer":2,"clipped":true}"#).expect("clipped");
    let clip = typed_style(
        &clip,
        1,
        r##"{"effects":[{"type":"color_overlay","color":"#3366cc","opacity":0.5}]}"##,
    );
    let samples = typed_linear_checked(&clip, "typed styled clip base");
    typed_within(
        samples[index(8, 8)],
        typed_mixed(RED, OVERLAY, 0.5),
        "typed member",
    );
    typed_within(
        samples[index(12, 8)],
        typed_mixed(LAYER, OVERLAY, 0.5),
        "typed base",
    );
}

// ------------------------------------------------------ the PSD style bake --
//
// A sub-pixel stroke has no Photoshop record, so PSD export BAKES the style
// into the layer's pixels. The bake is the layer as the canvas shows it: under
// linear light it composites in linear light and is ENCODED back, so the
// reopened (encoded) document shows the linear document's picture where the
// baked layer is opaque.

const BAKED: &str = r##"{"effects":[{"type":"stroke","size":0.5,"position":"outside","color":"#c08030"},{"type":"color_overlay","color":"#3366cc","opacity":0.5}]}"##;

fn path_c(path: &Path) -> CString {
    CString::new(path.to_str().expect("a UTF-8 path")).expect("no interior NUL")
}

fn fingerprint(path: &Path) -> CString {
    let mut err: *mut c_char = ptr::null_mut();
    let json = unsafe { dr_psd_import_report(path_c(path).as_ptr(), &mut err) };
    assert!(!json.is_null(), "import report");
    let report: serde_json::Value =
        serde_json::from_str(&crate::linear_oracle::take_err(json)).expect("report JSON");
    CString::new(
        report["source_fingerprint"]
            .as_str()
            .expect("a fingerprint"),
    )
    .unwrap()
}

/// The rect's interior, away from the stroke's edge.
fn interior() -> impl Iterator<Item = usize> {
    (RECT.1 + 1..RECT.3 - 1).flat_map(|y| (RECT.0 + 1..RECT.2 - 1).map(move |x| index(x, y)))
}

#[test]
fn a_psd_style_bake_is_encoded_from_linear_light_at_byte_depth() {
    let directory = TempDir::new().unwrap();
    let path = directory.path().join("baked.psd");
    let doc = linear(&with_style(
        &byte_doc(&background(), &rect_of(LAYER)),
        1,
        BAKED,
    ));
    let shown = flatten(&doc).expect("the linear projection");
    let mut err: *mut c_char = ptr::null_mut();
    let saved = unsafe {
        dr_doc_save_psd(
            &doc,
            ptr::null(),
            path_c(&path).as_ptr(),
            0,
            0,
            std::ptr::null(),
            &mut err,
        )
    };
    assert!(saved, "PSD export: {}", crate::linear_oracle::take_err(err));
    let opened = unsafe {
        dr_doc_open_psd(
            path_c(&path).as_ptr(),
            0,
            fingerprint(&path).as_ptr(),
            std::ptr::null(),
            &mut err,
        )
    };
    assert!(
        !opened.is_null(),
        "PSD open: {}",
        crate::linear_oracle::take_err(err)
    );
    let opened = unsafe { Box::from_raw(opened) };
    let reopened = flatten(&opened).expect("the reopened projection");
    for i in interior() {
        assert_within(
            pixel(&reopened, i),
            pixel(&shown, i),
            1,
            &format!("baked at {i}"),
        );
    }
}

#[test]
fn a_psd_style_bake_is_encoded_from_linear_light_at_typed_depth() {
    let directory = TempDir::new().unwrap();
    let path = directory.path().join("baked16.psd");
    let doc = typed_linear(&typed_style(
        &typed_doc(&background(), &rect_of(LAYER)),
        1,
        BAKED,
    ));
    let shown = typed_render(&doc).expect("the linear typed projection");
    let mut err: *mut c_char = ptr::null_mut();
    let projection =
        unsafe { dr_precision_doc_render(&doc, REFERENCE_WORKING_BYTES, 0, ptr::null(), &mut err) };
    assert!(!projection.is_null(), "the projection");
    let saved = unsafe {
        dr_precision_doc_save_psd(
            &doc,
            projection,
            path_c(&path).as_ptr(),
            0,
            0,
            REFERENCE_WORKING_BYTES,
            0,
            std::ptr::null(),
            &mut err,
        )
    };
    unsafe { dr_precision_image_free(projection) };
    assert!(saved, "PSD export: {}", crate::linear_oracle::take_err(err));
    let opened = unsafe {
        dr_precision_doc_open_psd(
            path_c(&path).as_ptr(),
            0,
            fingerprint(&path).as_ptr(),
            REFERENCE_WORKING_BYTES,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(
        !opened.is_null(),
        "PSD open: {}",
        crate::linear_oracle::take_err(err)
    );
    let opened = unsafe { Box::from_raw(opened) };
    let reopened = typed_render(&opened).expect("the reopened typed projection");
    for i in interior() {
        typed_within(reopened[i], shown[i], &format!("typed baked at {i}"));
    }
}
