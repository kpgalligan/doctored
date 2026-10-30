//! #1 phase 2d, layer effects and Blend If on a Grayscale document
//! (`gray_style`), through the FFI. The oracle is the RGB TWIN (the Gray
//! document converted to RGB through its own profile bytes) given the style
//! the Gray layer STORES (`reported_style`): each of the nine effects —
//! Stroke as a colour, a gradient and a coloured custom pattern, Pattern
//! Overlay with a coloured custom tile, every colour coloured and not the
//! default — composites R = G = B and equals the twin's composite exactly at
//! 8 and 16 bits and bit for bit at 32, and re-setting the stored style is
//! the exact no-op. Color Overlay #ff0000 stores the gray of sRGB's
//! published Y row (`round(255·srgb_encode(Y))`, ±1) and composites to
//! `257·round(255·srgb_decode(v/255)^(1/2.2))` ± 257 on Gray Gamma 2.2. A
//! wide colour greys to `[Y, Y, Y]` of the same row, and one whose `Y` is an
//! sRGB byte stores that hex with no `linear`. Effect modes follow the
//! Grayscale list (Hue refuses with the layer-mode sentence; at 32 bits a
//! float-refused mode with the float sentence first). Blend If on Gray is
//! twin-equal and hides by the layer's own code; any other channel refuses,
//! an identity one included, on both setters. The `.rz` round trip keeps the
//! render and the style; RGB → Gray carries an effect, a Gray Blend If and a
//! custom tile greyed, and names a Hue effect and a Red Blend If. Bounds
//! declared before measuring.

use std::ffi::CString;
use std::ptr;

use doctored_core::ffi_gray::dr_precision_doc_convert_to_gray;
use doctored_core::ffi_precision::{dr_precision_doc_open, dr_precision_doc_save_native};
use doctored_core::ffi_precision_conversion::dr_precision_doc_promote_to_float;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer as Layer};
use doctored_core::precision_image::PrecisionImage;
use serde_json::{json, Value};
use tempfile::TempDir;

use crate::common::{cpath, take_err_string};
use crate::gray_fixture::{srgb_decode, srgb_encode, SRGB_Y_ROW};
use crate::gray_ops::{assert_neutral, edit, free, twin, uniform_gray, LIMIT};

const W: u32 = 24;
const H: u32 = 16;
/// The shape layer: 12 × 8 at (6, 4), transparent around it.
const SW: u32 = 12;
const SH: u32 = 8;
const OFFSET: (i32, i32) = (6, 4);

const HUE: &str = "The Hue blend mode is unavailable in a Grayscale document.";

fn red_refusal() -> String {
    "Blend If on the Red channel is unavailable in a Grayscale document.".into()
}

/// Standard base64 (RFC 4648, padded), written here.
fn base64(bytes: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(T[((n >> (18 - 6 * i)) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// Its inverse.
fn unbase64(text: &str) -> Vec<u8> {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = Vec::new();
    let digits: Vec<u32> = text
        .bytes()
        .filter(|b| *b != b'=')
        .map(|b| T.iter().position(|t| *t == b).unwrap() as u32)
        .collect();
    for group in digits.chunks(4) {
        let mut n = 0u32;
        for (i, d) in group.iter().enumerate() {
            n |= d << (18 - 6 * i);
        }
        for i in 0..group.len() - 1 {
            out.push((n >> (16 - 8 * i)) as u8);
        }
    }
    out
}

/// A 4 × 4 coloured tile: every texel a different colour, two translucent.
fn tile() -> Vec<u8> {
    (0..16u32)
        .flat_map(|i| {
            let a = if i % 5 == 2 { 128 } else { 255 };
            [
                (i * 53 % 256) as u8,
                (255 - i * 37 % 256) as u8,
                (i * 91 % 256) as u8,
                a,
            ]
        })
        .collect()
}

fn custom_pattern() -> Value {
    json!({"name":"Tile","width":4,"height":4,"rgba":base64(&tile())})
}

/// One pattern pixel: sRGB 16-bit codes that vary in every channel.
fn pattern(i: u32, seed: u32) -> [u16; 4] {
    let r = (i * 7919 + 1013 + seed) % 251;
    let g = (i * 104_729 + 37 + seed) % 241;
    let b = (i * 1299 + 501 + seed) % 239;
    [
        (r * 257 + 40) as u16,
        (g * 265) as u16,
        (b * 270) as u16,
        65535,
    ]
}

fn raster(name: &str, w: u32, h: u32, offset: (i32, i32), seed: u32, partial: bool) -> Layer {
    let pixels: Vec<[u16; 4]> = (0..w * h)
        .map(|i| {
            let mut p = pattern(i, seed);
            if partial && i % 7 == 3 {
                p[3] = 40_000;
            }
            p
        })
        .collect();
    let image = PrecisionImage::from_rgba16(w, h, pixels, IccProfile::srgb(), None).unwrap();
    Layer::raster(name, image, offset, 1.0, true).unwrap()
}

/// The sRGB 16-bit document of a pattern Backdrop and a pattern Shape
/// (layer 1, [`SW`] × [`SH`] at [`OFFSET`], some pixels translucent).
fn rgb_doc() -> *mut PrecisionDocument {
    let doc = PrecisionDocument::from_parts(
        W,
        H,
        IccProfile::srgb(),
        Metadata::default(),
        Resolution::default(),
        vec![
            raster("Backdrop", W, H, (0, 0), 0, false),
            raster("Shape", SW, SH, OFFSET, 17, true),
        ],
    )
    .unwrap();
    Box::into_raw(Box::new(doc))
}

fn promoted(doc: *const PrecisionDocument) -> *mut PrecisionDocument {
    let mut err = ptr::null_mut();
    let float = unsafe { dr_precision_doc_promote_to_float(doc, LIMIT, 0, ptr::null(), &mut err) };
    assert!(!float.is_null(), "{}", take_err_string(err));
    float
}

fn to_gray(doc: *const PrecisionDocument, depth: u32) -> Result<*mut PrecisionDocument, String> {
    to_gray_in(doc, "gray_gamma_22", depth)
}

/// [`to_gray`] into the Gray working space `space`.
fn to_gray_in(
    doc: *const PrecisionDocument,
    space: &str,
    depth: u32,
) -> Result<*mut PrecisionDocument, String> {
    let options = if depth == 32 {
        format!(r#"{{"space":"{space}"}}"#)
    } else {
        format!(r#"{{"space":"{space}","depth":{depth}}}"#)
    };
    let options = CString::new(options).unwrap();
    let mut err = ptr::null_mut();
    let gray = unsafe {
        dr_precision_doc_convert_to_gray(
            doc,
            ptr::null(),
            0,
            options.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    if gray.is_null() {
        Err(take_err_string(err))
    } else {
        Ok(gray)
    }
}

/// [`rgb_doc`] as a Gray Gamma 2.2 document of `depth`.
fn gray_doc(depth: u32) -> *mut PrecisionDocument {
    let rgb = rgb_doc();
    let source = if depth == 32 {
        let float = promoted(rgb);
        free(rgb);
        float
    } else {
        rgb
    };
    let gray = to_gray(source, depth).unwrap();
    free(source);
    gray
}

fn style_request(layer: usize, style: &Value) -> Value {
    json!({"op":"layer_style","version":1,"layer":layer,"style":style})
}

fn styled(doc: *const PrecisionDocument, style: &Value) -> *mut PrecisionDocument {
    edit(doc, &style_request(1, style), &[])
        .unwrap_or_else(|e| panic!("{style}: {e}"))
        .unwrap_or_else(|| panic!("{style} changed nothing"))
}

/// The composite: u16 codes, or the f32 bits at 32 bits.
fn render(doc: *const PrecisionDocument) -> Vec<[u32; 4]> {
    let image = unsafe { &*doc }.render(LIMIT, 0, None).unwrap();
    if unsafe { &*doc }.is_float() {
        let pixels = image.to_rgba32().unwrap();
        pixels.iter().map(|p| p.map(f32::to_bits)).collect()
    } else {
        let pixels = image.to_rgba16().unwrap();
        pixels.iter().map(|p| p.map(u32::from)).collect()
    }
}

/// Layer `index`'s stored style as JSON.
fn stored(doc: *const PrecisionDocument, index: usize) -> Value {
    let text = unsafe { &*doc }
        .layer(index)
        .unwrap()
        .reported_style()
        .unwrap();
    serde_json::from_str(&text).unwrap()
}

/// Every `#rrggbb` string in `value` is gray.
fn assert_hexes_gray(value: &Value, what: &str) {
    match value {
        Value::String(s) if s.len() == 7 && s.starts_with('#') => {
            assert!(
                s[1..3] == s[3..5] && s[3..5] == s[5..7],
                "{what}: colour {s}"
            );
        }
        Value::Array(items) => items.iter().for_each(|v| assert_hexes_gray(v, what)),
        Value::Object(map) => map.values().for_each(|v| assert_hexes_gray(v, what)),
        _ => {}
    }
}

fn stops(a: &str, b: &str) -> Value {
    json!({"stops":[{"position":0,"color":a,"opacity":1},{"position":1,"color":b,"opacity":1}]})
}

/// The nine effects, each alone, coloured, in modes the Grayscale list
/// holds at every depth.
fn effects() -> Vec<(&'static str, Value)> {
    vec![
        (
            "drop_shadow",
            json!({"type":"drop_shadow","color":"#c03080","blend":"multiply","distance":3,"size":2,"opacity":0.8}),
        ),
        (
            "inner_shadow",
            json!({"type":"inner_shadow","color":"#20a040","blend":"multiply","distance":2,"size":2}),
        ),
        (
            "outer_glow",
            json!({"type":"outer_glow","color":"#ff8000","blend":"linear_dodge","size":3,"spread":0.2}),
        ),
        (
            "inner_glow",
            json!({"type":"inner_glow","color":"#4060ff","blend":"lighten","size":3}),
        ),
        (
            "stroke color",
            json!({"type":"stroke","color":"#e02020","size":2,"position":"outside"}),
        ),
        (
            "stroke gradient",
            json!({"type":"stroke","fill_type":"gradient","size":2,"gradient":stops("#ff0000","#0000ff")}),
        ),
        (
            "stroke pattern",
            json!({"type":"stroke","fill_type":"pattern","size":2,"position":"center",
                "pattern_fill":{"pattern":custom_pattern()}}),
        ),
        (
            "color_overlay",
            json!({"type":"color_overlay","color":"#30c0a0","opacity":0.7}),
        ),
        (
            "gradient_overlay",
            json!({"type":"gradient_overlay","gradient":stops("#ff0000","#00ff00"),"opacity":0.9}),
        ),
        (
            "pattern_overlay",
            json!({"type":"pattern_overlay","pattern":custom_pattern(),"opacity":0.8}),
        ),
        (
            "bevel_emboss",
            json!({"type":"bevel_emboss","size":3,"highlight_color":"#ffe080",
                "highlight_blend":"linear_dodge","shadow_color":"#402080","shadow_blend":"multiply"}),
        ),
        (
            "satin",
            json!({"type":"satin","color":"#a02060","blend":"multiply","distance":3,"size":3}),
        ),
    ]
}

#[test]
fn every_effect_composites_gray_and_equals_the_twin_given_the_stored_style() {
    for depth in [8, 16, 32] {
        let gray = gray_doc(depth);
        let rgb = twin(gray);
        for (name, effect) in effects() {
            let what = format!("{name} at {depth}");
            let style = json!({"effects":[effect]});
            let g = styled(gray, &style);
            let composite = render(g);
            assert_neutral(&composite, &what);
            let kept = stored(g, 1);
            assert_hexes_gray(&kept, &what);
            let t = styled(rgb, &kept);
            assert_eq!(composite, render(t), "{what}: against the twin");
            assert!(
                edit(g, &style_request(1, &kept), &[]).unwrap().is_none(),
                "{what}: re-setting the stored style"
            );
            free(t);
            free(g);
        }
        free(rgb);
        free(gray);
    }
}

#[test]
fn a_custom_tile_stores_its_texels_gray() {
    let gray = gray_doc(16);
    let g = styled(
        gray,
        &json!({"effects":[{"type":"pattern_overlay","pattern":custom_pattern()}]}),
    );
    let texels = unbase64(
        stored(g, 1)["effects"][0]["pattern"]["rgba"]
            .as_str()
            .unwrap(),
    );
    // The tile's codes are the document's: sRGB's primaries under Gray
    // Gamma 2.2's curve (the twin), so Y = row · (c/255)^2.2 and the texel
    // is round(65535·Y^(1/2.2)) / 257, ±1.
    for (got, source) in texels.chunks(4).zip(tile().chunks(4)) {
        assert_eq!(got[3], source[3], "alpha kept");
        assert!(got[0] == got[1] && got[1] == got[2], "{got:?}");
        let y: f64 = (0..3)
            .map(|c| SRGB_Y_ROW[c] * (f64::from(source[c]) / 255.0).powf(2.2))
            .sum::<f64>();
        let want = (65535.0 * y.powf(1.0 / 2.2)).round() / 257.0;
        assert!(
            (f64::from(got[0]) - want).abs() <= 1.0,
            "{got:?} against {want}"
        );
    }
    free(g);
    free(gray);
}

#[test]
fn red_overlay_stores_and_composites_the_gray_of_its_luminance() {
    let gray = uniform_gray(4, 3, 16, 0.5);
    let g = edit(
        gray,
        &json!({"op":"layer_style","version":1,"layer":0,
            "style":{"effects":[{"type":"color_overlay","color":"#ff0000","blend":"normal"}]}}),
        &[],
    )
    .unwrap()
    .unwrap();
    let hex = stored(g, 0)["effects"][0]["color"]
        .as_str()
        .unwrap()
        .to_string();
    let v = u8::from_str_radix(&hex[1..3], 16).unwrap();
    assert_eq!(hex, format!("#{v:02x}{v:02x}{v:02x}"));
    let oracle = (255.0 * srgb_encode(SRGB_Y_ROW[0])).round();
    assert!((f64::from(v) - oracle).abs() <= 1.0, "{v} against {oracle}");
    let want = 257.0 * (255.0 * srgb_decode(f64::from(v) / 255.0).powf(1.0 / 2.2)).round();
    for p in render(g) {
        assert!(p[0] == p[1] && p[1] == p[2], "{p:?}");
        assert!(
            (f64::from(p[0]) - want).abs() <= 257.0,
            "{} against {want}",
            p[0]
        );
    }
    free(g);
    free(gray);
}

#[test]
fn effect_modes_follow_the_grayscale_list() {
    for depth in [8, 16, 32] {
        let gray = gray_doc(depth);
        let hue = json!({"effects":[{"type":"drop_shadow","blend":"hue"}]});
        assert_eq!(
            edit(gray, &style_request(1, &hue), &[]).unwrap_err(),
            HUE,
            "{depth}"
        );
        let bevel = json!({"effects":[{"type":"bevel_emboss","highlight_blend":"normal","shadow_blend":"luminosity"}]});
        assert_eq!(
            edit(gray, &style_request(1, &bevel), &[]).unwrap_err(),
            "The Luminosity blend mode is unavailable in a Grayscale document.",
            "{depth}"
        );
        let screen = json!({"effects":[{"type":"drop_shadow","blend":"screen"}]});
        let result = edit(gray, &style_request(1, &screen), &[]);
        if depth == 32 {
            assert_eq!(
                result.unwrap_err(),
                "the screen blend mode on the drop_shadow layer effect is unavailable in a 32-bit float document"
            );
        } else {
            free(result.unwrap().unwrap());
        }
        free(gray);
    }
}

#[test]
fn a_wide_colour_greys_to_its_luminance() {
    let gray = gray_doc(16);
    // Over white: kept wide, [Y, Y, Y] of sRGB's row.
    let linear = [1.5f32, 0.2, 0.1];
    let g = styled(
        gray,
        &json!({"effects":[{"type":"color_overlay","color":"#ff0000","linear":linear}]}),
    );
    let kept = stored(g, 1);
    let got: Vec<f64> = kept["effects"][0]["linear"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_f64().unwrap())
        .collect();
    let y: f64 = (0..3).map(|c| SRGB_Y_ROW[c] * f64::from(linear[c])).sum();
    assert_eq!(got.len(), 3);
    assert!(got[0] == got[1] && got[1] == got[2], "{got:?}");
    assert!((got[0] - y).abs() <= 1e-6, "{} against {y}", got[0]);
    assert_hexes_gray(&kept, "over white");
    assert_neutral(&render(g), "over white");
    free(g);
    // In gamut, off the byte grid, whose Y is the sRGB byte 128.
    let l = srgb_decode(128.0 / 255.0);
    // Y = row · linear stays l: the green offset balances the other two.
    let (a, c) = (0.01, 0.01);
    let b = (SRGB_Y_ROW[0] * a + SRGB_Y_ROW[2] * c) / SRGB_Y_ROW[1];
    let linear = [l + a, l - b, l + c];
    for c in linear {
        assert!((0.0..=1.0).contains(&c));
        let code = 255.0 * srgb_encode(c);
        assert!((code - code.round()).abs() > 0.01, "{c} is on the grid");
    }
    let g = styled(
        gray,
        &json!({"effects":[{"type":"color_overlay","color":"#808080","linear":linear}]}),
    );
    let kept = stored(g, 1);
    assert_eq!(kept["effects"][0]["color"], "#808080");
    assert!(kept["effects"][0].get("linear").is_none(), "{kept}");
    assert!(edit(g, &style_request(1, &kept), &[]).unwrap().is_none());
    free(g);
    free(gray);
}

fn blend_if_request(blend_if: &Value) -> Value {
    json!({"op":"layer_blend_if","version":1,"layer":1,"blend_if":blend_if})
}

#[test]
fn blend_if_on_gray_hides_by_the_layers_own_code_and_other_channels_refuse() {
    let gray_range =
        json!({"channel":"gray","this_layer":[128,128,255,255],"underlying":[0,0,255,255]});
    for depth in [8, 16, 32] {
        let gray = gray_doc(depth);
        let rgb = twin(gray);
        let g = styled(gray, &json!({"blend_if":gray_range}));
        let composite = render(g);
        assert_neutral(&composite, &format!("blend if at {depth}"));
        let t = styled(rgb, &stored(g, 1));
        assert_eq!(
            composite,
            render(t),
            "blend if at {depth}: against the twin"
        );
        // The layer setter agrees with the style setter.
        let b = edit(gray, &blend_if_request(&gray_range), &[])
            .unwrap()
            .unwrap();
        assert_eq!(render(b), composite);
        if depth != 32 {
            let full = render(gray);
            let hidden = edit(
                gray,
                &json!({"op":"layer_visible","layer":1,"visible":false}),
                &[],
            )
            .unwrap()
            .unwrap();
            let backdrop = render(hidden);
            let shape = unsafe { &*gray }
                .layer_source(1)
                .unwrap()
                .to_rgba16()
                .unwrap();
            let (mut dark, mut light) = (0, 0);
            for (j, p) in shape.iter().enumerate() {
                if p[3] != 65535 {
                    continue;
                }
                let (x, y) = (j as u32 % SW, j as u32 / SW);
                let i = ((y + OFFSET.1 as u32) * W + x + OFFSET.0 as u32) as usize;
                let code = (f64::from(p[0]) / 257.0).round();
                if code <= 120.0 {
                    dark += 1;
                    assert_eq!(composite[i], backdrop[i], "{depth}: pixel {i} hidden");
                } else if code >= 136.0 {
                    light += 1;
                    assert_eq!(composite[i], full[i], "{depth}: pixel {i} shown");
                }
            }
            assert!(dark > 5 && light > 5, "{dark} dark, {light} light");
            free(hidden);
        }
        // Any other channel refuses, an identity one included, on both setters.
        let red = json!({"channel":"red","this_layer":[0,0,255,255],"underlying":[0,0,255,255]});
        assert_eq!(
            edit(gray, &style_request(1, &json!({"blend_if":red})), &[]).unwrap_err(),
            red_refusal()
        );
        assert_eq!(
            edit(gray, &blend_if_request(&red), &[]).unwrap_err(),
            red_refusal()
        );
        let extra = json!({"channel":"gray","this_layer":[10,20,255,255],"underlying":[0,0,255,255],
            "extra":[{"channel":"blue","this_layer":[0,0,200,255],"underlying":[0,0,255,255]}]});
        assert_eq!(
            edit(gray, &blend_if_request(&extra), &[]).unwrap_err(),
            "Blend If on the Blue channel is unavailable in a Grayscale document."
        );
        for d in [b, t, g, rgb, gray] {
            free(d);
        }
    }
}

#[test]
fn global_light_is_admitted_and_reaches_the_effects() {
    let gray = gray_doc(16);
    let g = styled(
        gray,
        &json!({"effects":[{"type":"drop_shadow","color":"#c03080","distance":3,"size":2}]}),
    );
    let lit = edit(
        g,
        &json!({"op":"global_light","angle":30,"altitude":40}),
        &[],
    )
    .unwrap()
    .unwrap();
    assert_ne!(render(lit), render(g));
    assert_neutral(&render(lit), "global light");
    for d in [lit, g, gray] {
        free(d);
    }
}

#[test]
fn an_rz_round_trip_keeps_the_render_and_the_style() {
    let dir = TempDir::new().unwrap();
    for depth in [16, 32] {
        let gray = gray_doc(depth);
        let g = styled(
            gray,
            &json!({"blend_if":{"channel":"gray","this_layer":[20,60,255,255],"underlying":[0,0,255,255]},
                "effects":[
                    {"type":"drop_shadow","color":"#c03080","distance":3,"size":2},
                    {"type":"stroke","color":"#e02020","size":1},
                    {"type":"pattern_overlay","pattern":custom_pattern(),"opacity":0.5}]}),
        );
        let path = dir.path().join(format!("styled-{depth}.rz"));
        let c = cpath(&path);
        let mut err = ptr::null_mut();
        assert!(
            unsafe { dr_precision_doc_save_native(g, c.as_ptr(), LIMIT, 0, ptr::null(), &mut err) },
            "{}",
            take_err_string(err)
        );
        let back = unsafe { dr_precision_doc_open(c.as_ptr(), LIMIT, 0, ptr::null(), &mut err) };
        assert!(!back.is_null(), "{}", take_err_string(err));
        assert!(unsafe { &*back }.is_gray());
        assert_eq!(render(back), render(g), "{depth}");
        assert_eq!(stored(back, 1), stored(g, 1), "{depth}");
        for d in [back, g, gray] {
            free(d);
        }
    }
}

/// RGB → Gray maps a custom tile's NEUTRAL texels too: an sRGB gray `v`
/// is another tone in Dot Gain 20 % (`Y^e`, `e = ln 0.5 / ln 0.3`), as the
/// layer pixels are re-encoded through `Y`. Each texel is
/// `round(65535·Y^e) / 257` ± 1, `Y` its sRGB luminance (the published
/// row over the IEC decode), and the carried tile is a fixed point of the
/// edit path's greying.
#[test]
fn rgb_to_gray_maps_a_tiles_neutral_texels_through_the_destination() {
    let neutral: Vec<u8> = [0u8, 64, 128, 200, 255]
        .into_iter()
        .flat_map(|v| [v, v, v, 255])
        .chain([200, 40, 90, 255])
        .collect();
    let tile = json!({"name":"Neutral","width":3,"height":2,"rgba":base64(&neutral)});
    let rgb = rgb_doc();
    let source = styled(
        rgb,
        &json!({"effects":[{"type":"pattern_overlay","pattern":tile}]}),
    );
    let gray = to_gray_in(source, "dot_gain_20", 16).unwrap();
    let texels = unbase64(
        stored(gray, 1)["effects"][0]["pattern"]["rgba"]
            .as_str()
            .unwrap(),
    );
    let e = 0.5f64.ln() / 0.3f64.ln();
    for (got, source) in texels.chunks(4).zip(neutral.chunks(4)) {
        let y: f64 = (0..3)
            .map(|c| SRGB_Y_ROW[c] * srgb_decode(f64::from(source[c]) / 255.0))
            .sum();
        let want = (65535.0 * y.clamp(0.0, 1.0).powf(e)).round() / 257.0;
        assert!(got[0] == got[1] && got[1] == got[2], "{got:?}");
        assert!(
            (f64::from(got[0]) - want).abs() <= 1.0,
            "{source:?} → {got:?} against {want}"
        );
        assert_eq!(got[3], source[3]);
    }
    // sRGB 128 is not Dot Gain 20 %'s 128.
    assert!(texels[8] < 120, "{:?}", &texels[8..12]);
    assert!(
        edit(gray, &style_request(1, &stored(gray, 1)), &[])
            .unwrap()
            .is_none(),
        "the carried tile is a fixed point of the edit path"
    );
    for d in [gray, source, rgb] {
        free(d);
    }
}

#[test]
fn rgb_to_gray_carries_effects_a_gray_blend_if_and_a_tile_greyed() {
    let rgb = rgb_doc();
    let style = json!({"blend_if":{"channel":"gray","this_layer":[20,60,255,255],"underlying":[0,0,255,255]},
        "effects":[
            {"type":"color_overlay","color":"#ff0000","opacity":0.6},
            {"type":"pattern_overlay","pattern":custom_pattern(),"opacity":0.5}]});
    let source = styled(rgb, &style);
    let gray = to_gray(source, 16).unwrap();
    let kept = stored(gray, 1);
    assert_eq!(kept["blend_if"], stored(source, 1)["blend_if"]);
    let effects = kept["effects"].as_array().unwrap();
    let of = |kind: &str| {
        effects
            .iter()
            .find(|e| e["type"] == kind)
            .unwrap_or_else(|| panic!("{kind} in {kept}"))
    };
    let hex = of("color_overlay")["color"].as_str().unwrap();
    let v = f64::from(u8::from_str_radix(&hex[1..3], 16).unwrap());
    assert_hexes_gray(&kept, "carried");
    assert!((v - (255.0 * srgb_encode(SRGB_Y_ROW[0])).round()).abs() <= 1.0);
    // Each texel the 8-bit gray of its sRGB luminance in Gray Gamma 2.2, ±1.
    let texels = unbase64(of("pattern_overlay")["pattern"]["rgba"].as_str().unwrap());
    for (got, source) in texels.chunks(4).zip(tile().chunks(4)) {
        let y: f64 = (0..3)
            .map(|c| SRGB_Y_ROW[c] * srgb_decode(f64::from(source[c]) / 255.0))
            .sum();
        let want = (65535.0 * y.powf(1.0 / 2.2)).round() / 257.0;
        assert!(got[0] == got[1] && got[1] == got[2], "{got:?}");
        assert!(
            (f64::from(got[0]) - want).abs() <= 1.0,
            "{got:?} against {want}"
        );
        assert_eq!(got[3], source[3]);
    }
    assert_neutral(&render(gray), "carried");
    // A Hue effect and a Red Blend If block, naming the layer.
    let hue = styled(
        rgb,
        &json!({"effects":[{"type":"drop_shadow","blend":"hue"}]}),
    );
    assert_eq!(
        to_gray(hue, 16).unwrap_err(),
        "“Shape” is a layer with a layer effect in the Hue blend mode, which a Grayscale document cannot hold; flatten the image to convert it"
    );
    let red = styled(
        rgb,
        &json!({"blend_if":{"channel":"red","this_layer":[20,60,255,255],"underlying":[0,0,255,255]}}),
    );
    assert_eq!(
        to_gray(red, 16).unwrap_err(),
        "“Shape” is a layer with Blend If on the Red channel, which a Grayscale document cannot hold; flatten the image to convert it"
    );
    for d in [red, hue, gray, source, rgb] {
        free(d);
    }
}
