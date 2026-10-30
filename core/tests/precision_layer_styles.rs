//! Phase10D typed layer styles: the nine effects and fill opacity on the
//! 16-bit model under the shape-quantized byte-plane policy
//! (designs/phase-10d-layer-styles-status.md) — promoted-document parity
//! with the byte compositor per effect, an independent f64 oracle on the
//! unblurred effects, styled clip bases and isolated groups, refusals, the
//! `layer_style` edit, conditional native36/`LSTY` persistence, the 16-bit
//! PSD bake/disclosure rule and the scratch plan.
use crate::group_roots;
use doctored_core::doc::{BlendMode, DrDocument};
use doctored_core::doc_mask::MaskProperties;
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_precision::dr_precision_doc_info;
use doctored_core::ffi_precision_edit::{dr_precision_doc_edit, dr_precision_doc_edit_plan};
use doctored_core::ffi_psd::{
    dr_precision_doc_open_psd, dr_precision_doc_psd_export_report, dr_precision_doc_save_psd,
    dr_psd_import_report,
};
use doctored_core::icc::IccProfile;
use doctored_core::job_control::DrJobControl;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer as Layer};
use doctored_core::precision_geometry::GeometryEdit;
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use doctored_core::precision_mask::{PrecisionMask, PrecisionPlane};
use doctored_core::style::{Effect, EffectKind, LayerStyle};
use group_roots::GroupRoots;
use image::{GrayImage, Luma, Rgba, RgbaImage};
use serde_json::{json, Value};
use std::ffi::{c_char, CStr, CString};
use std::path::Path;
use std::ptr;
use std::sync::Arc;
use tempfile::TempDir;

use crate::common;
use crate::identity;
use crate::psd16_fixture as fixture;
use common::{cpath, styled, take_err_string};
use fixture::Record;

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
const GIB: u64 = 1024 * 1024 * 1024;

// ------------------------------------------------------------- plumbing --

fn error(p: *mut c_char) -> Option<String> {
    if p.is_null() {
        return None;
    }
    let text = unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned();
    unsafe { dr_string_free(p) };
    Some(text)
}
fn edit_samples(
    doc: &PrecisionDocument,
    request: Value,
    samples: &[u16],
) -> Result<Option<PrecisionDocument>, String> {
    let request = CString::new(request.to_string()).unwrap();
    let mut err = ptr::null_mut();
    let result = unsafe {
        dr_precision_doc_edit(
            doc,
            request.as_ptr(),
            if samples.is_empty() {
                ptr::null()
            } else {
                samples.as_ptr()
            },
            samples.len(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    if let Some(message) = error(err) {
        assert!(result.is_null());
        return Err(message);
    }
    Ok((!result.is_null()).then(|| *unsafe { Box::from_raw(result) }))
}
fn edit(doc: &PrecisionDocument, request: Value) -> Result<Option<PrecisionDocument>, String> {
    edit_samples(doc, request, &[])
}
fn changed(doc: &PrecisionDocument, request: Value) -> PrecisionDocument {
    edit(doc, request).unwrap().unwrap()
}
fn plan(doc: &PrecisionDocument, request: Value) -> Result<Value, String> {
    let request = CString::new(request.to_string()).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe { dr_precision_doc_edit_plan(doc, request.as_ptr(), LIMIT, 0, &mut err) };
    if let Some(message) = error(err) {
        assert!(out.is_null());
        return Err(message);
    }
    Ok(serde_json::from_str(&error(out).unwrap()).unwrap())
}
fn info(doc: &PrecisionDocument) -> Value {
    let mut err = ptr::null_mut();
    let out = unsafe { dr_precision_doc_info(doc, &mut err) };
    assert!(!out.is_null(), "{:?}", error(err));
    serde_json::from_str(&error(out).unwrap()).unwrap()
}

/// The layer rows a REOPENED document must match, with the one field that is
/// deliberately not persisted taken out.
///
/// `id` is session-scoped stable layer identity (`doc_identity`), the same
/// rule `Guide::id` and `Channel::id` follow: the `.rz` carries no ids and
/// every reader MINTS, so a reopened document's rows differ there by design.
/// Everything else in the row is a persistence claim and is compared.
fn persisted_layers(doc: &PrecisionDocument) -> Value {
    Value::Array(
        info(doc)["layers"]
            .as_array()
            .unwrap()
            .iter()
            .map(|row| {
                let mut row = row.as_object().unwrap().clone();
                assert!(
                    row.remove("id")
                        .and_then(|v| v.as_u64())
                        .is_some_and(|id| id != 0),
                    "every layer row carries a non-zero id"
                );
                Value::Object(row)
            })
            .collect(),
    )
}
fn request(layer: usize, style: Value) -> Value {
    json!({"op":"layer_style","version":1,"layer":layer,"style":style})
}
fn set(doc: &PrecisionDocument, layer: usize, style: Value) -> PrecisionDocument {
    changed(doc, request(layer, style))
}
fn parse(json: &str) -> LayerStyle {
    LayerStyle::from_json(json).unwrap_or_else(|e| panic!("{json}: {e}"))
}
fn image(w: u32, h: u32, pixels: Vec<[u16; 4]>) -> PrecisionImage {
    assert_eq!(pixels.len(), (w * h) as usize);
    PrecisionImage::from_rgba16(w, h, pixels, IccProfile::srgb(), None).unwrap()
}
fn layer(name: &str, w: u32, h: u32, pixels: Vec<[u16; 4]>, at: (i32, i32), opacity: f32) -> Layer {
    Layer::raster(name, image(w, h, pixels), at, opacity, true).unwrap()
}
fn document(w: u32, h: u32, layers: Vec<Layer>) -> PrecisionDocument {
    PrecisionDocument::from_parts(
        w,
        h,
        IccProfile::srgb(),
        Metadata::default(),
        Resolution::default(),
        layers,
    )
    .unwrap()
}
fn render(doc: &PrecisionDocument) -> Vec<[u16; 4]> {
    doc.render(LIMIT, 0, None).unwrap().to_rgba16().unwrap()
}
fn mask(w: u32, h: u32, samples: Vec<u16>, at: (i32, i32)) -> PrecisionMask {
    PrecisionMask::new(
        PrecisionPlane::from_u16(w, h, samples, None).unwrap(),
        MaskProperties::at(at),
        true,
    )
    .unwrap()
}
fn invert() -> String {
    json!({"type":"adjust","op":"invert","params":{},"evaluation":"precision_v1"}).to_string()
}
fn adjustment(name: &str) -> Layer {
    Layer::adjustment(name, &invert(), 1.0, true, &IccProfile::srgb()).unwrap()
}
fn promoted(p: [u8; 4]) -> [u16; 4] {
    p.map(|v| u16::from(v) * 257)
}
fn with_style(layer: Layer, json: &str, canvas: (u32, u32)) -> Layer {
    layer
        .with_layer_style(Some(&parse(json)), canvas)
        .unwrap_or_else(|e| panic!("{json}: {e}"))
}

// ------------------------------------------------------- byte fixtures --

/// The anti-aliased 48×40 test layer: a soft disc whose colour varies per
/// pixel, so every effect sees a fractional edge.
const LAYER: (u32, u32) = (48, 40);
const AT: (i32, i32) = (7, 5);
const CANVAS: (u32, u32) = (64, 56);
const BG: [u8; 4] = [200, 150, 100, 255];

fn disc_px(x: u32, y: u32) -> [u8; 4] {
    let dx = x as f32 - 23.5;
    let dy = y as f32 - 19.5;
    let r = (dx * dx + dy * dy).sqrt();
    let a = if r <= 14.0 {
        255.0
    } else if r >= 19.0 {
        0.0
    } else {
        255.0 * (19.0 - r) / 5.0
    };
    [
        (x * 5 % 256) as u8,
        (y * 6 % 256) as u8,
        (128 + (x + y) % 64) as u8,
        a.round() as u8,
    ]
}
fn mask_px(x: u32, y: u32) -> u8 {
    ((x * 5 + y * 3) % 256) as u8
}
fn byte_layer() -> RgbaImage {
    RgbaImage::from_fn(LAYER.0, LAYER.1, |x, y| Rgba(disc_px(x, y)))
}
fn typed_layer_px() -> Vec<[u16; 4]> {
    (0..LAYER.1)
        .flat_map(|y| (0..LAYER.0).map(move |x| promoted(disc_px(x, y))))
        .collect()
}
fn byte_doc(masked: bool, opacity: f32, blend: BlendMode) -> DrDocument {
    let mut doc = DrDocument::from_pixels(RgbaImage::from_pixel(CANVAS.0, CANVAS.1, Rgba(BG)))
        .adding_image_layer(0, byte_layer(), "Top")
        .unwrap()
        .with_layer_offset(1, AT.0, AT.1)
        .unwrap();
    if opacity != 1.0 {
        doc = doc.with_layer_opacity(1, opacity).unwrap();
    }
    if blend != BlendMode::Normal {
        doc = doc.with_layer_blend_mode(1, blend).unwrap();
    }
    if masked {
        doc.layers[1].mask = Some(Arc::new(GrayImage::from_fn(LAYER.0, LAYER.1, |x, y| {
            Luma([mask_px(x, y)])
        })));
        doc.layers[1].mask_enabled = true;
        doc.layers[1].mask_properties = MaskProperties::at(AT);
    }
    doc
}
fn typed_doc(masked: bool, opacity: f32, blend: BlendMode, style: &str) -> PrecisionDocument {
    let mut top = layer("Top", LAYER.0, LAYER.1, typed_layer_px(), AT, opacity)
        .with_blend_mode(blend)
        .unwrap();
    if masked {
        let samples: Vec<u16> = (0..LAYER.1)
            .flat_map(|y| (0..LAYER.0).map(move |x| u16::from(mask_px(x, y)) * 257))
            .collect();
        top = top.with_mask(Some(mask(LAYER.0, LAYER.1, samples, AT)));
    }
    document(
        CANVAS.0,
        CANVAS.1,
        vec![
            layer(
                "B",
                CANVAS.0,
                CANVAS.1,
                vec![promoted(BG); (CANVAS.0 * CANVAS.1) as usize],
                (0, 0),
                1.0,
            ),
            with_style(top, style, CANVAS),
        ],
    )
}
/// The worst `|typed − 257·byte|` over every component and how many pixels
/// pass 129 codes.
fn parity(typed: &[[u16; 4]], byte: &RgbaImage, what: &str) -> usize {
    let (w, _) = byte.dimensions();
    let mut worst = 0u32;
    let mut past = 0usize;
    for (i, t) in typed.iter().enumerate() {
        let b = byte.get_pixel(i as u32 % w, i as u32 / w).0;
        let gap = (0..4)
            .map(|c| (i32::from(t[c]) - i32::from(b[c]) * 257).unsigned_abs())
            .max()
            .unwrap();
        worst = worst.max(gap);
        past += usize::from(gap > 129);
    }
    eprintln!("{what}: worst |typed - 257 * byte| = {worst} codes, {past} pixels past 129");
    past
}

// ------------------------------------------ (a) parity per effect --

const STYLES: [(&str, &str); 21] = [
    ("drop_shadow", r#"{"effects":[{"type":"drop_shadow"}]}"#),
    (
        "drop_shadow own light + spread",
        r#"{"effects":[{"type":"drop_shadow","use_global_light":false,"angle":45,"distance":9,"spread":0.5}]}"#,
    ),
    ("inner_shadow", r#"{"effects":[{"type":"inner_shadow"}]}"#),
    (
        "inner_shadow own light size 0",
        r#"{"effects":[{"type":"inner_shadow","use_global_light":false,"angle":30,"distance":3,"size":0}]}"#,
    ),
    ("outer_glow", r#"{"effects":[{"type":"outer_glow"}]}"#),
    (
        "outer_glow spread",
        r#"{"effects":[{"type":"outer_glow","spread":0.5,"size":8}]}"#,
    ),
    ("inner_glow", r#"{"effects":[{"type":"inner_glow"}]}"#),
    (
        "inner_glow center size 0",
        r#"{"effects":[{"type":"inner_glow","source":"center","size":0}]}"#,
    ),
    (
        "inner_glow center choke 1",
        r#"{"effects":[{"type":"inner_glow","source":"center","choke":1,"size":4,"range":1}]}"#,
    ),
    ("stroke", r#"{"effects":[{"type":"stroke"}]}"#),
    (
        "stroke gradient center",
        r#"{"effects":[{"type":"stroke","fill_type":"gradient","position":"center","size":4}]}"#,
    ),
    ("color_overlay", r#"{"effects":[{"type":"color_overlay"}]}"#),
    (
        "color_overlay multiply",
        r##"{"effects":[{"type":"color_overlay","color":"#00ff80","opacity":0.4,"blend":"multiply"}]}"##,
    ),
    (
        "gradient_overlay",
        r#"{"effects":[{"type":"gradient_overlay"}]}"#,
    ),
    (
        "gradient_overlay radial canvas",
        r#"{"effects":[{"type":"gradient_overlay","gradient":{"style":"radial","align_with_layer":false}}]}"#,
    ),
    ("bevel_emboss", r#"{"effects":[{"type":"bevel_emboss"}]}"#),
    (
        "bevel_emboss emboss",
        r#"{"effects":[{"type":"bevel_emboss","style":"emboss","size":5}]}"#,
    ),
    (
        "bevel_emboss pillow",
        r#"{"effects":[{"type":"bevel_emboss","style":"pillow_emboss","size":5}]}"#,
    ),
    (
        "bevel_emboss outer",
        r#"{"effects":[{"type":"bevel_emboss","style":"outer_bevel","size":3}]}"#,
    ),
    ("satin", r#"{"effects":[{"type":"satin"}]}"#),
    (
        "satin invert false",
        r#"{"effects":[{"type":"satin","invert":false}]}"#,
    ),
];

#[test]
fn every_effect_matches_the_byte_compositor_on_a_promoted_document() {
    let mut cases = 0;
    for (name, style) in STYLES {
        for masked in [false, true] {
            for opacity in [1.0f32, 0.625] {
                for blend in [BlendMode::Normal, BlendMode::Multiply] {
                    let flat = styled(&byte_doc(masked, opacity, blend), 1, style)
                        .flattened()
                        .unwrap();
                    let typed = render(&typed_doc(masked, opacity, blend, style));
                    let what = format!("{name} masked={masked} opacity={opacity} blend={blend:?}");
                    assert_eq!(parity(&typed, &flat, &what), 0, "{what}");
                    cases += 1;
                }
            }
        }
    }
    eprintln!("{cases} parity cases");
}

// ------------------------------------------------ (b) f64 oracle --

fn blend(mode: &str, cb: f64, cs: f64) -> f64 {
    match mode {
        "normal" => cs,
        "multiply" => cb * cs,
        "screen" => cb + cs - cb * cs,
        other => panic!("mode {other}"),
    }
}
/// The W3C general source-over with a separable blend, in f64.
fn over(dst: [f64; 4], cs: [f64; 3], sa: f64, mode: &str) -> [f64; 4] {
    if sa <= 0. {
        return dst;
    }
    let ab = dst[3];
    let ao = sa + ab * (1. - sa);
    let mut out = [0.; 4];
    for c in 0..3 {
        out[c] = (sa * (1. - ab) * cs[c]
            + sa * ab * blend(mode, dst[c], cs[c])
            + (1. - sa) * ab * dst[c])
            / ao;
    }
    out[3] = ao;
    out
}
fn mix(bg: [f64; 4], pk: [f64; 4], t: f64) -> [f64; 4] {
    let a = bg[3] + (pk[3] - bg[3]) * t;
    if a <= 0. {
        return [0.; 4];
    }
    let mut out = [0.; 4];
    for c in 0..3 {
        let b = bg[c] * bg[3];
        let p = pk[c] * pk[3];
        out[c] = ((b + (p - b) * t) / a).clamp(0., 1.);
    }
    out[3] = a.min(1.);
    out
}
fn deviation(actual: [u16; 4], expected: [f64; 4]) -> f64 {
    (0..4)
        .map(|c| (actual[c] as f64 - expected[c] * 65535.).abs())
        .fold(0., f64::max)
}
fn hex(rgb: [u8; 3]) -> [f64; 3] {
    rgb.map(|v| v as f64 / 255.)
}

const OW: u32 = 24;
const OH: u32 = 20;
const OL: (u32, u32) = (12, 10);
const OAT: (i32, i32) = (4, 3);
const OBG: [u16; 4] = [30001, 20002, 10003, 65535];

fn oracle_px(x: u32, y: u32) -> [u16; 4] {
    [
        ((x * 4000 + y * 1000) % 65536) as u16,
        (40000 - x * 1500) as u16,
        (12345 + y * 900) as u16,
        ((3000 + x * 4500 + y * 1300) % 65536) as u16,
    ]
}
/// The quantized typed shape of `oracle_px`: `round(a16 / 257)`.
fn shape_at(lx: i64, ly: i64) -> u32 {
    if lx < 0 || ly < 0 || lx >= OL.0 as i64 || ly >= OL.1 as i64 {
        return 0;
    }
    (oracle_px(lx as u32, ly as u32)[3] as f64 / 257.).round() as u32
}

#[derive(Clone, Copy)]
enum Fx {
    Shadow,
    Color,
    Gradient,
}

fn oracle_style(fx: &[Fx], fill: f64, mode: &str) -> String {
    let mut effects = Vec::new();
    for f in fx {
        effects.push(match f {
            Fx::Shadow => json!({"type":"drop_shadow","use_global_light":false,"angle":90,"distance":2,"size":0,"spread":0,"opacity":0.75,"color":"#000000","blend":mode,"layer_knocks_out":true}),
            Fx::Color => json!({"type":"color_overlay","color":"#3080c0","opacity":0.7,"blend":mode}),
            // Two equal stops: the overlay's coverage is the shape and its
            // colour a constant, so the oracle needs no gradient sampler.
            Fx::Gradient => json!({"type":"gradient_overlay","opacity":0.6,"blend":mode,"gradient":{"stops":[{"position":0,"color":"#20a060","opacity":1},{"position":1,"color":"#20a060","opacity":1}]}}),
        });
    }
    json!({"fill_opacity":fill,"effects":effects}).to_string()
}

/// One canvas pixel through the byte order in f64: Below (the hard shadow,
/// shifted (0, 2), knocked out by the shape at its landing and landing under
/// the layer there), the pixels at
/// `alpha × fill`, then Interior in render rank (gradient, colour).
fn oracle_pixel(x: u32, y: u32, fx: &[Fx], fill: f64, mode: &str, opacity: f64) -> [f64; 4] {
    let bg = OBG.map(|v| v as f64 / 65535.);
    let lx = i64::from(x) - i64::from(OAT.0);
    let ly = i64::from(y) - i64::from(OAT.1);
    let mut acc = bg;
    if fx.iter().any(|f| matches!(f, Fx::Shadow)) {
        let mut cov = shape_at(lx, ly - 2);
        let under = shape_at(lx, ly);
        if cov > 0 {
            cov = (cov * (255 - under) + 127) / 255;
        }
        // The knocked-out shadow lands under the layer (#2 part 4, DK1):
        // at `sa / (1 − a·fill)` where the layer covers its landing at `a`.
        let mut sa = cov as f64 / 255. * 0.75;
        let la = under as f64 / 255. * fill;
        if la > 0. && la < 1. {
            sa = (sa / (1. - la)).min(1.);
        }
        acc = over(acc, [0., 0., 0.], sa, mode);
    }
    if lx >= 0 && ly >= 0 && lx < OL.0 as i64 && ly < OL.1 as i64 {
        let p = oracle_px(lx as u32, ly as u32).map(|v| v as f64 / 65535.);
        acc = over(acc, [p[0], p[1], p[2]], p[3] * fill, "normal");
    }
    let s = shape_at(lx, ly) as f64 / 255.;
    if fx.iter().any(|f| matches!(f, Fx::Gradient)) {
        acc = over(acc, hex([0x20, 0xa0, 0x60]), s * 0.6, mode);
    }
    if fx.iter().any(|f| matches!(f, Fx::Color)) {
        acc = over(acc, hex([0x30, 0x80, 0xc0]), s * 0.7, mode);
    }
    if opacity < 1. {
        mix(bg, acc, opacity)
    } else {
        acc
    }
}

#[test]
fn unblurred_effects_and_fill_opacity_meet_the_f64_oracle() {
    let pixels: Vec<[u16; 4]> = (0..OL.1)
        .flat_map(|y| (0..OL.0).map(move |x| oracle_px(x, y)))
        .collect();
    let bg = layer("B", OW, OH, vec![OBG; (OW * OH) as usize], (0, 0), 1.0);
    let mut worst = 0f64;
    let cases: [&[Fx]; 4] = [
        &[Fx::Color],
        &[Fx::Shadow],
        &[Fx::Gradient],
        &[Fx::Shadow, Fx::Gradient, Fx::Color],
    ];
    for fx in cases {
        for fill in [0.0f64, 0.5, 1.0] {
            for mode in ["normal", "multiply", "screen"] {
                for opacity in [1.0f32, 0.625] {
                    let style = oracle_style(fx, fill, mode);
                    let doc = document(
                        OW,
                        OH,
                        vec![
                            bg.clone(),
                            with_style(
                                layer("T", OL.0, OL.1, pixels.clone(), OAT, opacity),
                                &style,
                                (OW, OH),
                            ),
                        ],
                    );
                    let rendered = render(&doc);
                    for y in 0..OH {
                        for x in 0..OW {
                            // Every case has effects, so a partial opacity
                            // packages the layer: the two bytes never meet in
                            // one product (#2).
                            let expected = oracle_pixel(
                                x,
                                y,
                                fx,
                                common::byte_unit(fill),
                                mode,
                                common::byte_unit(f64::from(opacity)),
                            );
                            let d = deviation(rendered[(y * OW + x) as usize], expected);
                            worst = worst.max(d);
                            assert!(
                                d <= 0.65,
                                "{style} opacity {opacity} at ({x},{y}): {:?} vs {expected:?} ({d})",
                                rendered[(y * OW + x) as usize]
                            );
                        }
                    }
                }
            }
        }
    }
    eprintln!("f64 oracle: worst deviation {worst:.3} codes");
    // Fill 0 leaves the effects intact: the overlay shows where the pixels
    // would have been.
    let doc = document(
        OW,
        OH,
        vec![
            bg.clone(),
            with_style(
                layer("T", OL.0, OL.1, pixels.clone(), OAT, 1.0),
                &oracle_style(&[Fx::Color], 0.0, "normal"),
                (OW, OH),
            ),
        ],
    );
    let rendered = render(&doc);
    let (x, y) = (OAT.0 as u32 + 5, OAT.1 as u32 + 5);
    assert_ne!(
        rendered[(y * OW + x) as usize],
        OBG,
        "the overlay still renders at fill 0"
    );
}

// ---------------------------------------------- (c) styled clip base --

const CB_LAYER: (u32, u32) = (24, 20);
const CB_AT: (i32, i32) = (10, 8);
const CB_STYLE: &str = r#"{"fill_opacity":0.5,"blend_if":{"channel":"red","this_layer":[0,0,100,100],"underlying":[0,0,255,255]},"effects":[{"type":"drop_shadow"}]}"#;
const M1: [u8; 4] = [30, 220, 90, 255];
const M2: [u8; 4] = [200, 50, 50, 160];

fn base_px(x: u32, y: u32) -> [u8; 4] {
    let dx = x as f32 - 11.5;
    let dy = y as f32 - 9.5;
    let r = (dx * dx + dy * dy).sqrt();
    let a = if r <= 6.0 {
        255.0
    } else if r >= 9.5 {
        0.0
    } else {
        255.0 * (9.5 - r) / 3.5
    };
    [(x * 9 % 256) as u8, 40, 60, a.round() as u8]
}

#[test]
fn a_styled_clip_base_matches_the_byte_compositor_and_confines_its_members() {
    for opaque in [true, false] {
        for members in [1usize, 2] {
            for opacity in [1.0f32, 0.625] {
                let bg = if opaque { BG } else { [0, 0, 0, 0] };
                let mut byte =
                    DrDocument::from_pixels(RgbaImage::from_pixel(CANVAS.0, CANVAS.1, Rgba(bg)))
                        .adding_image_layer(
                            0,
                            RgbaImage::from_fn(CB_LAYER.0, CB_LAYER.1, |x, y| Rgba(base_px(x, y))),
                            "Base",
                        )
                        .unwrap()
                        .with_layer_offset(1, CB_AT.0, CB_AT.1)
                        .unwrap()
                        .adding_image_layer(
                            1,
                            RgbaImage::from_pixel(CANVAS.0, CANVAS.1, Rgba(M1)),
                            "M1",
                        )
                        .unwrap()
                        .with_layer_clipped(2, true)
                        .unwrap();
                if members == 2 {
                    byte = byte
                        .adding_image_layer(
                            2,
                            RgbaImage::from_pixel(CANVAS.0, CANVAS.1, Rgba(M2)),
                            "M2",
                        )
                        .unwrap()
                        .with_layer_clipped(3, true)
                        .unwrap()
                        .with_layer_blend_mode(3, BlendMode::Multiply)
                        .unwrap();
                }
                if opacity != 1.0 {
                    byte = byte.with_layer_opacity(1, opacity).unwrap();
                }
                let flat = styled(&byte, 1, CB_STYLE).flattened().unwrap();

                let canvas = (CANVAS.0 * CANVAS.1) as usize;
                let mut layers = vec![
                    layer(
                        "B",
                        CANVAS.0,
                        CANVAS.1,
                        vec![promoted(bg); canvas],
                        (0, 0),
                        1.0,
                    ),
                    with_style(
                        layer(
                            "Base",
                            CB_LAYER.0,
                            CB_LAYER.1,
                            (0..CB_LAYER.1)
                                .flat_map(|y| (0..CB_LAYER.0).map(move |x| promoted(base_px(x, y))))
                                .collect(),
                            CB_AT,
                            opacity,
                        ),
                        CB_STYLE,
                        CANVAS,
                    ),
                    layer(
                        "M1",
                        CANVAS.0,
                        CANVAS.1,
                        vec![promoted(M1); canvas],
                        (0, 0),
                        1.0,
                    )
                    .with_clipped(true),
                ];
                if members == 2 {
                    layers.push(
                        layer(
                            "M2",
                            CANVAS.0,
                            CANVAS.1,
                            vec![promoted(M2); canvas],
                            (0, 0),
                            1.0,
                        )
                        .with_blend_mode(BlendMode::Multiply)
                        .unwrap()
                        .with_clipped(true),
                    );
                }
                let doc = document(CANVAS.0, CANVAS.1, layers);
                assert!(doc.layer(1).unwrap().blend_if().is_some());
                assert!(doc.layer(1).unwrap().style().is_some());
                let typed = render(&doc);
                let what = format!("clip base opaque={opaque} members={members} opacity={opacity}");
                assert_eq!(parity(&typed, &flat, &what), 0, "{what}");
                if !opaque {
                    // Every member is confined to the base's shape: nothing
                    // lands outside the style's grown extent, though both
                    // members cover the whole canvas.
                    let (x0, y0, x1, y1) = doc.style_extent16(1).unwrap();
                    let mut outside = 0;
                    for y in 0..CANVAS.1 {
                        for x in 0..CANVAS.0 {
                            let inside = i64::from(x) >= x0
                                && i64::from(x) < x1
                                && i64::from(y) >= y0
                                && i64::from(y) < y1;
                            if !inside {
                                assert_eq!(
                                    typed[(y * CANVAS.0 + x) as usize],
                                    [0; 4],
                                    "{what} at ({x},{y})"
                                );
                                outside += 1;
                            }
                        }
                    }
                    assert!(outside > 0);
                }
            }
        }
    }
}

// ------------------------------------- (d) styled groups and refusals --

const GROUP_STYLE: &str =
    r#"{"effects":[{"type":"drop_shadow"},{"type":"color_overlay","opacity":0.5}]}"#;

#[test]
fn a_styled_isolated_group_matches_the_byte_compositor_and_refusals_hold() {
    for opacity in [1.0f32, 0.625] {
        let byte = DrDocument::from_pixels(RgbaImage::from_pixel(CANVAS.0, CANVAS.1, Rgba(BG)))
            .adding_image_layer(
                0,
                RgbaImage::from_fn(CB_LAYER.0, CB_LAYER.1, |x, y| Rgba(base_px(x, y))),
                "R",
            )
            .unwrap()
            .with_layer_offset(1, CB_AT.0, CB_AT.1)
            .unwrap();
        let (byte, group, _, _) = byte.group_layers(&[1], "G").unwrap();
        let byte = if opacity != 1.0 {
            byte.with_layer_opacity(group, opacity).unwrap()
        } else {
            byte
        };
        let flat = styled(&byte, group, GROUP_STYLE).flattened().unwrap();
        let canvas = (CANVAS.0 * CANVAS.1) as usize;
        let inner = layer(
            "R",
            CB_LAYER.0,
            CB_LAYER.1,
            (0..CB_LAYER.1)
                .flat_map(|y| (0..CB_LAYER.0).map(move |x| promoted(base_px(x, y))))
                .collect(),
            CB_AT,
            1.0,
        );
        let doc = document(
            CANVAS.0,
            CANVAS.1,
            vec![
                layer(
                    "B",
                    CANVAS.0,
                    CANVAS.1,
                    vec![promoted(BG); canvas],
                    (0, 0),
                    1.0,
                ),
                with_style(
                    Layer::isolated_normal("G", vec![inner], opacity, true).unwrap(),
                    GROUP_STYLE,
                    CANVAS,
                ),
            ],
        );
        let what = format!("styled group opacity {opacity}");
        assert_eq!(parity(&render(&doc), &flat, &what), 0, "{what}");
    }

    // Refusals at the constructor and the edit. An adjustment carries one
    // (`precision_style_adjustment`).
    let style = parse(GROUP_STYLE);
    let px = vec![promoted(BG); 16];
    assert!(adjustment("A")
        .with_layer_style(Some(&style), (4, 4))
        .unwrap()
        .style()
        .is_some());
    // A Pass Through group carries one.
    let pass = Layer::pass_through(
        "P",
        vec![layer("R", 4, 4, px.clone(), (0, 0), 1.0)],
        1.,
        true,
    )
    .unwrap()
    .with_layer_style(Some(&style), (4, 4))
    .unwrap();
    assert!(pass.is_pass_through() && pass.style().is_some());
    let group = Layer::isolated_normal(
        "G",
        vec![layer("R", 4, 4, px.clone(), (0, 0), 1.0)],
        1.,
        true,
    )
    .unwrap()
    .with_layer_style(Some(&style), (4, 4))
    .unwrap();
    let passed = group
        .clone()
        .with_blend_mode(BlendMode::PassThrough)
        .unwrap();
    assert!(passed.is_pass_through() && passed.style() == group.style());
    assert!(group.with_blend_mode(BlendMode::Multiply).is_ok());
    // An oversize plane: a styled isolated group on a 9000×9000 canvas with
    // the pad cap (an inner shadow at distance 30000) refuses up front.
    let wide = r#"{"effects":[{"type":"inner_shadow","distance":30000}]}"#;
    let big = document(
        9000,
        9000,
        vec![Layer::isolated_normal(
            "G",
            vec![layer("R", 4, 4, px.clone(), (0, 0), 1.0)],
            1.,
            true,
        )
        .unwrap()],
    );
    let refusal = big
        .layer(1)
        .unwrap()
        .clone()
        .with_layer_style(Some(&parse(wide)), (9000, 9000))
        .err()
        .unwrap();
    assert!(refusal.contains("pixel limit"), "{refusal}");
    let via_edit = edit(&big, request(1, serde_json::from_str(wide).unwrap()))
        .err()
        .unwrap();
    assert!(via_edit.contains("pixel limit"), "{via_edit}");
    // The raster itself (4×4 + 2·1024) is fine.
    assert!(big
        .layer(0)
        .unwrap()
        .clone()
        .with_layer_style(Some(&parse(wide)), (9000, 9000))
        .is_ok());

    // Geometry: growing a styled 100×100 raster to 8000×8000 puts its padded
    // plane past the limit; the plan refuses, naming the layer, before any
    // resample (the job reports no progress).
    let styled_doc = document(
        100,
        100,
        vec![with_style(
            layer("Styled", 100, 100, vec![promoted(BG); 10_000], (0, 0), 1.0),
            wide,
            (100, 100),
        )],
    );
    let resize = json!({"op":"resize","width":8000,"height":8000,"filter":"nearest"});
    let refusal = plan(&styled_doc, resize.clone()).err().unwrap();
    assert!(
        refusal.contains("“Styled”") && refusal.contains("pixel limit"),
        "{refusal}"
    );
    let control = DrJobControl::default();
    let edit_ = GeometryEdit::parse(&resize.to_string()).unwrap();
    let refusal = styled_doc
        .geometry(&edit_, LIMIT, 0, Some(&control))
        .err()
        .unwrap();
    assert!(refusal.contains("“Styled”"), "{refusal}");
    assert_eq!(control.progress(), 0.0, "refused before any resample");
    // A modest resize scales the effects instead.
    let grown = changed(
        &styled_doc,
        json!({"op":"resize","width":200,"height":200,"filter":"nearest"}),
    );
    assert!(grown.layer(0).unwrap().style().is_some());
}

// ------------------------------------------------------- (e) the edit --

fn effect_size(doc: &PrecisionDocument, index: usize) -> (f32, f32) {
    match doc
        .layer(index)
        .unwrap()
        .style()
        .unwrap()
        .effect(EffectKind::DropShadow)
    {
        Some(Effect::DropShadow(d)) => (d.size, d.distance),
        other => panic!("no drop shadow: {other:?}"),
    }
}

#[test]
fn layer_style_plans_edits_echoes_and_preserves_through_edits() {
    let px = [1000u16, 2000, 3000, 65535];
    let doc = document(
        4,
        4,
        vec![
            layer("B", 4, 4, vec![px; 16], (0, 0), 1.),
            layer("T", 4, 4, vec![px; 16], (0, 0), 0.5).with_mask(Some(mask(
                4,
                4,
                vec![1; 16],
                (0, 0),
            ))),
            Layer::isolated_normal("G", vec![adjustment("A")], 1., true).unwrap(),
            Layer::pass_through(
                "P",
                vec![layer("R", 4, 4, vec![px; 16], (0, 0), 1.)],
                1.,
                true,
            )
            .unwrap(),
        ],
    );
    let full = json!({"fill_opacity":0.5,"blend_if":{"channel":"red","this_layer":[10,20,240,255],"underlying":[0,0,200,220]},"effects":[{"type":"drop_shadow"},{"type":"color_overlay","enabled":false}]});
    let req = request(1, full.clone());
    let planned = plan(&doc, req.clone()).unwrap();
    assert_eq!(planned["input_samples"], json!(0));
    assert!(planned["scratch_bytes"].as_u64().unwrap() > 0);
    let set_doc = edit(&doc, req.clone()).unwrap().unwrap();
    let canonical = parse(&full.to_string()).to_json();
    assert_eq!(
        info(&set_doc)["layers"][1]["style"],
        Value::String(canonical.clone()),
        "the style row is the canonical merged JSON"
    );
    assert_eq!(
        set_doc.layer(1).unwrap().reported_style().unwrap(),
        canonical
    );
    assert!(set_doc.layer(1).unwrap().blend_if().is_some());
    let stored = set_doc.layer(1).unwrap().style().unwrap();
    assert!(stored.blend_if.is_none() && stored.fill_opacity == 0.5);
    assert_eq!(stored.effects.len(), 2, "a disabled effect is kept");
    assert!(doc.layer(1).unwrap().style().is_none(), "pure");
    assert!(
        edit(&set_doc, req).unwrap().is_none(),
        "exact no-op is NULL"
    );
    assert!(PrecisionImage::ptr_eq(
        set_doc.layer_source(1).unwrap(),
        doc.layer_source(1).unwrap()
    ));
    // Echo: the reported style set back is the exact no-op.
    let echo: Value = serde_json::from_str(&canonical).unwrap();
    assert!(edit(&set_doc, request(1, echo)).unwrap().is_none());
    // Clearing the Blend If alone keeps the effects.
    let no_ranges = changed(
        &set_doc,
        json!({"op":"layer_blend_if","version":1,"layer":1,"blend_if":null}),
    );
    assert!(no_ranges.layer(1).unwrap().blend_if().is_none());
    assert!(no_ranges.layer(1).unwrap().style().is_some());
    assert_eq!(
        no_ranges.layer(1).unwrap().reported_style().unwrap(),
        no_ranges.layer(1).unwrap().style().unwrap().to_json()
    );
    // A Blend If-only style sets no effects field.
    let only = set(
        &doc,
        1,
        json!({"blend_if":{"channel":"gray","this_layer":[0,0,128,160]}}),
    );
    assert!(
        only.layer(1).unwrap().style().is_none() && only.layer(1).unwrap().blend_if().is_some()
    );
    // Identity and null clear both fields; on a plain layer they are no-ops.
    let identity = json!({"fill_opacity":1,"effects":[{"type":"drop_shadow","enabled":false}]});
    let cleared = set(&set_doc, 1, identity.clone());
    assert!(
        cleared.layer(1).unwrap().style().is_none()
            && cleared.layer(1).unwrap().blend_if().is_none()
    );
    assert_eq!(info(&cleared)["layers"][1]["style"], Value::Null);
    let cleared = set(&set_doc, 1, Value::Null);
    assert!(cleared.layer(1).unwrap().reported_style().is_none());
    assert!(edit(&doc, request(1, identity)).unwrap().is_none());
    assert!(edit(&doc, request(1, Value::Null)).unwrap().is_none());
    let on_group = set(&doc, 3, full.clone());
    assert!(on_group.layer(3).unwrap().style().is_some());
    for (bad, needle) in [
        (
            json!({"op":"layer_style","version":1,"layer":1,"style":full.clone(),"extra":1}),
            "unknown field",
        ),
        (
            json!({"op":"layer_style","version":2,"layer":1,"style":full.clone()}),
            "version",
        ),
        (
            request(1, json!({"effects":[{"type":"drop_shadow","size":"big"}]})),
            "size",
        ),
        (
            request(1, json!({"effects":[{"type":"drop_shadox"}]})),
            "type",
        ),
        (
            request(1, json!({"fill_opacity":"half","effects":[]})),
            "fill_opacity",
        ),
        (request(1, json!([1, 2])), "object"),
        (
            request(
                1,
                json!({"effects":[{"type":"drop_shadow","blend_mode":"normal"}]}),
            ),
            "unknown layer style field effects[0].blend_mode",
        ),
        (request(6, full.clone()), "out of range"),
    ] {
        let message = edit(&doc, bad.clone()).err().expect(needle);
        assert!(message.contains(needle), "{message}");
        assert!(plan(&doc, bad).is_err());
    }
    // A Pass Through group takes one.
    let on_pass_through = set(&doc, 5, full.clone());
    assert!(on_pass_through.layer(5).unwrap().is_pass_through());
    assert!(on_pass_through.layer(5).unwrap().style().is_some());
    // An adjustment record takes one.
    assert!(set(&doc, 2, full.clone())
        .layer(2)
        .unwrap()
        .style()
        .is_some());
    let supported = info(&set_doc)["supported_operations"].clone();
    assert_eq!(
        supported
            .as_array()
            .unwrap()
            .iter()
            .filter(|v| v == &&json!("style"))
            .count(),
        1
    );
    assert!(!info(&set_doc)["unsupported_operations"]
        .as_array()
        .unwrap()
        .contains(&json!("style")));
    for (target, ok) in [("raster", true), ("group", true), ("adjustment", true)] {
        assert_eq!(
            doctored_core::precision_capability::require("layer_style", target, 16).is_ok(),
            ok
        );
    }
    assert!(doctored_core::precision_capability::require("layer_style", "raster", 8).is_err());

    // Preservation through every metadata, geometry and retouch edit.
    let doc = set(
        &document(
            4,
            4,
            vec![
                layer("B", 4, 4, vec![px; 16], (0, 0), 1.),
                Layer::pass_through(
                    "G",
                    vec![
                        layer("T", 4, 4, vec![px; 16], (0, 0), 1.),
                        layer("M", 4, 4, vec![px; 16], (0, 0), 0.5),
                    ],
                    1.,
                    true,
                )
                .unwrap(),
            ],
        ),
        1,
        full.clone(),
    );
    let flags = |d: &PrecisionDocument| {
        (0..d.layer_count())
            .map(|i| d.layer(i).unwrap().style().is_some())
            .collect::<Vec<_>>()
    };
    assert_eq!(flags(&doc), [false, true, false, false]);
    let cases: Vec<(&str, PrecisionDocument, Vec<bool>)> = vec![
        (
            "duplicate",
            doc.duplicate_layer(1).unwrap().unwrap(),
            vec![false, true, true, false, false],
        ),
        (
            "rename",
            changed(
                &doc,
                json!({"op":"rename_layer","layer":1,"name":"Renamed"}),
            ),
            flags(&doc),
        ),
        (
            "opacity",
            doc.set_layer_opacity(1, 0.25).unwrap().unwrap(),
            flags(&doc),
        ),
        (
            "group_all",
            doc.group_all(1.0).unwrap().unwrap(),
            vec![false, true, false, false, false],
        ),
        (
            "blend",
            changed(
                &doc,
                json!({"op":"layer_blend","version":1,"layer":1,"blend_mode":1}),
            ),
            flags(&doc),
        ),
        (
            "crop",
            changed(&doc, json!({"op":"crop","x":1,"y":0,"width":2,"height":4})),
            flags(&doc),
        ),
        (
            "resize",
            changed(
                &doc,
                json!({"op":"resize","width":8,"height":8,"filter":"nearest"}),
            ),
            flags(&doc),
        ),
        (
            "clipped",
            changed(&doc, json!({"op":"layer_clipped","layer":2,"clipped":true})),
            flags(&doc),
        ),
    ];
    for (name, out, expected) in &cases {
        assert_eq!(&flags(out), expected, "{name}");
        let (size, distance) = effect_size(out, 1);
        if *name == "resize" {
            assert_eq!((size, distance), (10.0, 10.0), "Scale Effects at 2×");
        } else {
            assert_eq!((size, distance), (5.0, 5.0), "{name}");
        }
        assert!(out.encode_native(LIMIT, 0, None).is_ok(), "{name}");
    }
    let repaired = doc
        .retouch(
            &doctored_core::precision_retouch::RetouchEdit::parse(
                &json!({"op":"retouch_clone","layer":1,"output":"new_layer"}).to_string(),
            )
            .unwrap(),
            &[65535; 16],
            LIMIT,
            0,
            None,
        )
        .unwrap()
        .unwrap();
    assert_eq!(flags(&repaired), [false, true, false, false, false]);
}

// ------------------------------------------------------------ (f) native --

fn words(values: &[u32]) -> Vec<u8> {
    values.iter().flat_map(|v| v.to_le_bytes()).collect()
}
/// The encoded file with 12E-1's always-written identity wrapper peeled off.
/// This file's subject is an EARLIER wrapper, so the outer word is asserted
/// and the `LIDS` tail removed, leaving byte-for-byte what the writer these
/// tests were written against produced.
fn bytes(doc: &PrecisionDocument) -> Vec<u8> {
    identity::typed_prior_body(&doc.encode_native(LIMIT, 0, None).unwrap())
}
fn word(data: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(data[at..at + 4].try_into().unwrap())
}
fn tag_at(data: &[u8], tag: &[u8]) -> usize {
    data.windows(4).rposition(|v| v == tag).unwrap()
}
/// An in-test reader of the `LSTY` tail.
fn read_lsty(data: &[u8]) -> Vec<(u32, String)> {
    let at = tag_at(data, b"LSTY");
    assert_eq!(word(data, at + 4), 1, "schema");
    let count = word(data, at + 8) as usize;
    let mut cursor = at + 12;
    let mut out = Vec::new();
    for _ in 0..count {
        let index = word(data, cursor);
        let len = word(data, cursor + 4) as usize;
        out.push((
            index,
            String::from_utf8(data[cursor + 8..cursor + 8 + len].to_vec()).unwrap(),
        ));
        cursor += 8 + len;
    }
    assert_eq!(cursor, data.len(), "the tail ends the file");
    out
}
fn lsty(entries: &[(u32, &str)]) -> Vec<u8> {
    let mut out = b"LSTY".to_vec();
    out.extend(words(&[1, entries.len() as u32]));
    for (index, body) in entries {
        out.extend(words(&[*index, body.len() as u32]));
        out.extend(body.as_bytes());
    }
    out
}
fn outside(doc: &PrecisionDocument, index: usize) -> PrecisionDocument {
    let mask = doc.layer(index).unwrap().mask().unwrap();
    let mut p = mask.properties().clone();
    p.outside_coverage = 255;
    doc.set_layer_mask(
        index,
        Some(mask.with_properties(p, mask.enabled()).unwrap()),
        LIMIT,
        0,
        None,
    )
    .unwrap()
    .unwrap()
}
/// B(0), pass-through G{Inner masked (1), Member (2)}(3), Adj(4), Top(5).
fn native_fixture() -> PrecisionDocument {
    let px: Vec<_> = (0..4)
        .map(|i| {
            [
                1001 + i,
                2003 + i,
                3007 + i,
                [0, 3, 32768, 65535][i as usize],
            ]
        })
        .collect();
    document(
        4,
        1,
        vec![
            layer("B", 4, 1, px.clone(), (0, 0), 1.),
            Layer::pass_through(
                "G",
                vec![
                    layer("Inner", 4, 1, px.clone(), (1, 0), 0.625).with_mask(Some(mask(
                        3,
                        1,
                        vec![0, 123, 32769],
                        (1, 0),
                    ))),
                    layer("Member", 4, 1, px.clone(), (-1, 0), 1.),
                ],
                0.875,
                true,
            )
            .unwrap(),
            adjustment("Adj"),
            layer("Top", 4, 1, px, (0, 0), 0.5),
        ],
    )
}
const TOP_STYLE: &str = r#"{"fill_opacity":0.5,"blend_if":{"channel":"gray","this_layer":[10,20,240,255]},"effects":[{"type":"drop_shadow"},{"type":"stroke","size":2}]}"#;
const INNER_STYLE: &str = r##"{"effects":[{"type":"color_overlay","color":"#123456"}]}"##;
fn styled_fixture(plain: &PrecisionDocument) -> PrecisionDocument {
    set(
        &set(plain, 5, serde_json::from_str(TOP_STYLE).unwrap()),
        1,
        serde_json::from_str(INNER_STYLE).unwrap(),
    )
}
fn style_rows(doc: &PrecisionDocument) -> Vec<Option<String>> {
    (0..doc.layer_count())
        .map(|i| doc.layer(i).unwrap().reported_style())
        .collect()
}

#[test]
fn native36_wraps_the_exact_prior_writer_and_composes_with_35_32_29_34() {
    let plain = native_fixture();
    let with = styled_fixture(&plain);
    let data = bytes(&with);
    assert_eq!(&data[4..16], words(&[36, 35, 13]));
    let top = with.layer(5).unwrap().style().unwrap().to_json();
    let inner = with.layer(1).unwrap().style().unwrap().to_json();
    assert_eq!(read_lsty(&data), [(1, inner.clone()), (5, top.clone())]);
    assert!(tag_at(&data, b"LBIF") < tag_at(&data, b"LSTY"));
    // The Blend If-only document is the v35 fixture the 36 word wraps.
    let ranges_only = changed(
        &changed(&with, request(1, Value::Null)),
        json!({"op":"layer_style","version":1,"layer":5,"style":{"blend_if":{"channel":"gray","this_layer":[10,20,240,255]}}}),
    );
    let prior = bytes(&ranges_only);
    assert_eq!(word(&prior, 4), 35);
    assert_eq!(&data[8..tag_at(&data, b"LSTY")], &prior[4..]);
    assert_eq!(
        &data[tag_at(&data, b"LSTY")..],
        lsty(&[(1, &inner), (5, &top)])
    );
    // Releasing the styles restores the exact prior writers.
    assert_eq!(
        bytes(&changed(
            &changed(&with, request(1, Value::Null)),
            json!({"op":"layer_style","version":1,"layer":5,"style":{"blend_if":{"channel":"gray","this_layer":[10,20,240,255]}}}),
        )),
        prior
    );
    let released = changed(
        &changed(&with, request(1, Value::Null)),
        request(5, Value::Null),
    );
    assert_eq!(bytes(&released), bytes(&plain));
    assert_eq!(word(&bytes(&plain), 4), 13);
    let reopened = PrecisionDocument::parse_native(&data, LIMIT, 0, None).unwrap();
    assert_eq!(style_rows(&reopened), style_rows(&with));
    assert_eq!(persisted_layers(&reopened), persisted_layers(&with));
    assert_eq!(render(&reopened), render(&with));
    assert_eq!(bytes(&reopened), data);

    // 36 → 35 → 32 → 29 → 34 → body → LBLD6 → MOUT → LCLP → LBIF → LSTY.
    let composed = changed(
        &outside(
            &changed(
                &plain,
                json!({"op":"layer_blend","version":1,"layer":0,"blend_mode":BlendMode::Dissolve.to_c()}),
            ),
            1,
        ),
        json!({"op":"layer_clipped","layer":2,"clipped":true}),
    );
    let composed_styled = set(&composed, 5, serde_json::from_str(TOP_STYLE).unwrap());
    let data = bytes(&composed_styled);
    assert_eq!(&data[4..28], words(&[36, 35, 32, 29, 34, 13]));
    let at = |tag: &[u8]| tag_at(&data, tag);
    assert!(
        at(b"LBLD") < at(b"MOUT")
            && at(b"MOUT") < at(b"LCLP")
            && at(b"LCLP") < at(b"LBIF")
            && at(b"LBIF") < at(b"LSTY")
    );
    assert_eq!(read_lsty(&data).len(), 1);
    let prior = bytes(&composed);
    assert_eq!(&prior[4..20], words(&[32, 29, 34, 13]));
    assert_eq!(bytes(&set(&composed_styled, 5, Value::Null)), prior);
    let reopened = PrecisionDocument::parse_native(&data, LIMIT, 0, None).unwrap();
    assert_eq!(style_rows(&reopened), style_rows(&composed_styled));
    assert!(reopened.layer(2).unwrap().clipped());
    assert_eq!(reopened.layer(0).unwrap().blend_mode(), BlendMode::Dissolve);
    assert_eq!(bytes(&reopened), data);
    // Without a Blend If anywhere, 36 wraps the body directly.
    let effects_only = set(&plain, 1, serde_json::from_str(INNER_STYLE).unwrap());
    let data = bytes(&effects_only);
    assert_eq!(&data[4..12], words(&[36, 13]));
    assert_eq!(&data[8..tag_at(&data, b"LSTY")], &bytes(&plain)[4..]);
}

#[test]
fn malformed_layer_style_framing_refuses_before_any_png_decode() {
    let plain = native_fixture();
    let good = bytes(&set(&plain, 5, serde_json::from_str(INNER_STYLE).unwrap()));
    let tail = tag_at(&good, b"LSTY");
    let body = read_lsty(&good)[0].1.clone();
    let head = good[..tail].to_vec();
    // Every corruption also corrupts a PNG so the framing error must win.
    let corrupt = |data: &mut Vec<u8>| {
        let idat = data.windows(4).position(|b| b == b"IDAT").unwrap();
        data[idat + 4] ^= 0xff;
    };
    let refuse = |data: Vec<u8>, needle: &str| {
        let control = DrJobControl::default();
        let error = PrecisionDocument::parse_native(&data, LIMIT, 0, Some(&control))
            .err()
            .expect("refusal");
        assert!(error.contains(needle), "{error}");
        assert_eq!(control.progress(), 0.0, "refused before any decode");
    };
    let with_tail = |tail: Vec<u8>| {
        let mut data = head.clone();
        data.extend(tail);
        data
    };
    for (position, value, needle) in [
        (tail + 4, 2, "layer style schema"),
        (tail + 8, 0, "layer style count"),
        (tail + 8, 9, "layer style count"),
        (tail + 12, 7, "layer style reference"),
        (tail + 16, 0, "layer style body length"),
        // Past `MAX_STYLE_JSON_LEN` (2 MiB, for custom pattern tiles).
        (tail + 16, 2 * 1024 * 1024 + 1, "layer style body length"),
    ] {
        let mut bad = good.clone();
        bad[position..position + 4].copy_from_slice(&u32::to_le_bytes(value));
        corrupt(&mut bad);
        refuse(bad, needle);
    }
    // Record 4 is the adjustment, which carries one.
    let mut on_adjustment = good.clone();
    on_adjustment[tail + 12..tail + 16].copy_from_slice(&4u32.to_le_bytes());
    let reopened = PrecisionDocument::parse_native(&on_adjustment, LIMIT, 0, None).unwrap();
    assert!(reopened.layer(4).unwrap().style().is_some());
    // Record 3 is the Pass Through group, which carries one.
    let mut on_pass_through = good.clone();
    on_pass_through[tail + 12..tail + 16].copy_from_slice(&3u32.to_le_bytes());
    let reopened = PrecisionDocument::parse_native(&on_pass_through, LIMIT, 0, None).unwrap();
    assert!(reopened.layer(3).unwrap().is_pass_through());
    assert!(reopened.layer(3).unwrap().style().is_some());
    let noncanonical = body.replacen("\"version\":1", "\"version\":2", 1);
    let unknown_key = body.replacen("\"opacity\"", "\"opacitx\"", 1);
    let unknown_type = body.replacen("color_overlay", "color_overlax", 1);
    let with_ranges = body.replacen(
        "\"blend_if\":null",
        "\"blend_if\":{\"channel\":\"red\",\"this_layer\":[0,0,100,100],\"underlying\":[0,0,255,255]}",
        1,
    );
    let identity = r##"{"blend_if":null,"effects":[{"blend":"normal","color":"#123456","enabled":false,"opacity":1.0,"type":"color_overlay"}],"fill_opacity":1.0,"version":1}"##;
    for (bad_body, needle) in [
        (noncanonical.as_str(), "not canonical"),
        (unknown_key.as_str(), "not canonical"),
        (unknown_type.as_str(), "type"),
        (with_ranges.as_str(), "Blend If"),
        (identity, "identity"),
        ("{\"effects\":[}", "invalid native layer style body"),
        ("\u{1}", "not valid JSON"),
    ] {
        let mut bad = with_tail(lsty(&[(5, bad_body)]));
        corrupt(&mut bad);
        refuse(bad, needle);
    }
    for entries in [
        vec![(5u32, body.as_str()), (1, body.as_str())],
        vec![(1, body.as_str()), (1, body.as_str())],
    ] {
        let mut bad = with_tail(lsty(&entries));
        corrupt(&mut bad);
        refuse(bad, "layer style reference");
    }
    let mut trailing = good.clone();
    trailing.push(0);
    corrupt(&mut trailing);
    refuse(trailing, "trailing");
    let mut truncated = good.clone();
    truncated.truncate(good.len() - 1);
    corrupt(&mut truncated);
    refuse(truncated, "");
    // LSTY under a 35 header.
    let ranged = set(
        &plain,
        5,
        json!({"blend_if":{"channel":"gray","this_layer":[10,20,240,255]}}),
    );
    let mut under35 = bytes(&ranged);
    assert_eq!(word(&under35, 4), 35);
    under35.extend_from_slice(&good[tail..]);
    corrupt(&mut under35);
    refuse(under35, "trailing");
    // 36 nested inside 35.
    let both = set(&ranged, 1, serde_json::from_str(INNER_STYLE).unwrap());
    let data = bytes(&both);
    assert_eq!(&data[4..12], words(&[36, 35]));
    let mut reversed = data.clone();
    reversed[4..12].copy_from_slice(&words(&[35, 36]));
    corrupt(&mut reversed);
    refuse(reversed, "blend-if typed body version");
    // A record whose declared plane plus the pad passes the limit: the
    // style's entry pointed at an isolated group on a 9000×9000 canvas.
    let wide = r#"{"effects":[{"type":"inner_shadow","distance":30000}]}"#;
    let px = vec![promoted(BG); 16];
    let big = set(
        &document(
            9000,
            9000,
            vec![
                Layer::isolated_normal(
                    "G",
                    vec![layer("R", 4, 4, px.clone(), (0, 0), 1.)],
                    1.,
                    true,
                )
                .unwrap(),
                layer("T", 4, 4, px, (0, 0), 1.),
            ],
        ),
        2,
        serde_json::from_str(wide).unwrap(),
    );
    let data = bytes(&big);
    assert!(PrecisionDocument::parse_native(&data, LIMIT, 0, None).is_ok());
    let at = tag_at(&data, b"LSTY");
    assert_eq!(word(&data, at + 12), 2);
    let mut oversize = data.clone();
    oversize[at + 12..at + 16].copy_from_slice(&1u32.to_le_bytes());
    corrupt(&mut oversize);
    refuse(oversize, "plane exceeds the pixel limit");
    // The uncorrupted saves still open.
    assert!(PrecisionDocument::parse_native(&good, LIMIT, 0, None).is_ok());
    assert!(PrecisionDocument::parse_native(&bytes(&both), LIMIT, 0, None).is_ok());
}

// ------------------------------------------------------------- (g) PSD16 --

fn report(path: &Path) -> Value {
    let mut err = ptr::null_mut();
    let json = unsafe { dr_psd_import_report(cpath(path).as_ptr(), &mut err) };
    assert!(!json.is_null(), "{}", take_err_string(err));
    serde_json::from_str(&take_err_string(json)).unwrap()
}
fn open16(path: &Path, notice: &Value) -> PrecisionDocument {
    let token = CString::new(notice["source_fingerprint"].as_str().unwrap()).unwrap();
    let mut err = ptr::null_mut();
    let raw = unsafe {
        dr_precision_doc_open_psd(
            cpath(path).as_ptr(),
            0,
            token.as_ptr(),
            LIMIT,
            0,
            ptr::null::<DrJobControl>(),
            &mut err,
        )
    };
    assert!(!raw.is_null(), "{}", take_err_string(err));
    *unsafe { Box::from_raw(raw) }
}
fn export_report(doc: &PrecisionDocument) -> Value {
    let mut err = ptr::null_mut();
    let json = unsafe { dr_precision_doc_psd_export_report(doc, 0, 0, &mut err) };
    assert!(!json.is_null(), "{}", take_err_string(err));
    serde_json::from_str(&take_err_string(json)).unwrap()
}
fn planes(rgba: &[[u16; 4]]) -> Vec<(i16, Vec<u8>)> {
    [(-1i16, 3usize), (0, 0), (1, 1), (2, 2)]
        .into_iter()
        .map(|(id, component)| {
            let samples: Vec<u16> = rgba.iter().map(|p| p[component]).collect();
            (id, fixture::raw_plane(&samples))
        })
        .collect()
}
fn strings(value: &Value) -> Vec<String> {
    value
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect()
}

#[test]
fn a_styled_raster_is_baked_and_disclosed_through_sixteen_bit_psd() {
    let directory = TempDir::new().unwrap();
    let pixels: Vec<[u16; 4]> = (0..OL.1)
        .flat_map(|y| (0..OL.0).map(move |x| oracle_px(x, y)))
        .collect();
    let mask_samples: Vec<u16> = (0..OL.0 * OL.1).map(|i| (i * 500 % 65536) as u16).collect();
    // Fill opacity crosses in Photoshop's 1/255 `iOpa` steps; 0.6 is exactly
    // 153/255, so the whole style round-trips unchanged.
    let top_style = r#"{"fill_opacity":0.6,"blend_if":{"channel":"red","this_layer":[0,0,100,100]},"effects":[{"type":"drop_shadow"},{"type":"stroke","position":"outside"},{"type":"color_overlay","opacity":0.5}]}"#;
    let source = document(
        OW,
        OH,
        vec![
            layer("B", OW, OH, vec![OBG; (OW * OH) as usize], (0, 0), 1.),
            with_style(
                layer("Top", OL.0, OL.1, pixels.clone(), OAT, 0.6).with_mask(Some(mask(
                    OL.0,
                    OL.1,
                    mask_samples,
                    OAT,
                ))),
                top_style,
                (OW, OH),
            ),
            layer(
                "Plain",
                6,
                6,
                vec![[200 * 257, 5000, 6000, 65535]; 36],
                (2, 2),
                1.,
            )
            .with_blend_if(Some(doctored_core::style::BlendIf {
                channel: doctored_core::style::BlendIfChannel::Red,
                this_layer: [0, 0, 100, 100],
                underlying: [0, 0, 255, 255],
                ..doctored_core::style::BlendIf::default()
            }))
            .unwrap(),
            Layer::raster(
                "Multi",
                image(6, 6, vec![[10000, 20000, 30000, 65535]; 36]),
                (1, 1),
                1.,
                false,
            )
            .unwrap()
            .with_blend_mode(BlendMode::Multiply)
            .unwrap()
            .with_layer_style(
                Some(&parse(r#"{"effects":[{"type":"color_overlay"}]}"#)),
                (OW, OH),
            )
            .unwrap(),
        ],
    );
    let preflight = export_report(&source);
    assert_eq!(preflight["available"], Value::Bool(true), "{preflight}");
    let losses = strings(&preflight["losses"]);
    assert!(
        !losses.iter().any(|l| l.contains("rasterized")),
        "nothing is baked: {losses:?}"
    );
    let observations = strings(&preflight["observations"]);
    let top_sentences: Vec<&String> = observations
        .iter()
        .filter(|o| o.contains("“Top”"))
        .collect();
    assert_eq!(top_sentences.len(), 1, "{observations:?}");
    let sentence = top_sentences[0];
    assert!(
        sentence.starts_with("Layer “Top” has a layer style (drop_shadow, stroke, color_overlay, fill opacity 60 %, Blend If on red);"),
        "{sentence}"
    );
    assert!(
        sentence.contains(
            "written as Photoshop layer effects and blending options (lfx2, iOpa, blending ranges)"
        ),
        "{sentence}"
    );
    assert!(
        observations
            .iter()
            .any(|o| o.starts_with("Layer “Plain” has a layer style (Blend If on red);")),
        "{observations:?}"
    );
    let projection = source.render(LIMIT, 0, None).unwrap();
    let path = directory.path().join("styled16.psd");
    let mut err = ptr::null_mut();
    assert!(
        unsafe {
            dr_precision_doc_save_psd(
                &source,
                &projection,
                cpath(&path).as_ptr(),
                0,
                0,
                LIMIT,
                0,
                &mut err,
            )
        },
        "{}",
        take_err_string(err)
    );
    let file = fixture::read(&std::fs::read(&path).unwrap());
    let by_name = fixture::by_name(&file);
    let top = by_name["Top"];
    assert_eq!(
        top.bounds,
        [OAT.1, OAT.0, OAT.1 + OL.1 as i32, OAT.0 + OL.0 as i32],
        "a preserved style keeps the layer's own rectangle"
    );
    assert!(
        top.channels.iter().any(|(id, _)| *id == -2),
        "and its own mask channel"
    );
    assert!(top.mask.is_some());
    assert_eq!(top.opacity, 153);
    assert_eq!(&top.blend, b"norm");
    assert_eq!(top.ranges.len(), 40, "its Blend If travels as ranges");
    let keys: Vec<[u8; 4]> = top.blocks.iter().map(|(key, _)| *key).collect();
    assert!(keys.contains(b"lfx2") && keys.contains(b"iOpa"), "{keys:?}");
    assert_eq!(&by_name["Multi"].blend, b"mul ");
    assert_eq!(by_name["Multi"].opacity, 255);
    assert_eq!(by_name["Plain"].ranges.len(), 40);
    assert!(
        by_name["Plain"]
            .blocks
            .iter()
            .all(|(key, _)| key != b"lfx2"),
        "a Blend If without effects writes no effects descriptor"
    );
    let notice = report(&path);
    assert_eq!(notice["import_precision"], json!("rgba16"));
    let reopened = open16(&path, &notice);
    let rows = info(&reopened)["layers"].as_array().unwrap().clone();
    assert_eq!(
        rows[1]["style"],
        json!(source.layer(1).unwrap().reported_style().unwrap()),
        "the carried style is the one that was written"
    );
    assert_eq!(
        rows[2]["style"],
        json!(source.layer(2).unwrap().reported_style().unwrap())
    );
    assert!(
        !strings(&notice["verified_losses"])
            .iter()
            .any(|l| l.contains("layer-style metadata")),
        "{}",
        notice["verified_losses"]
    );
    // The reopened file renders what the source rendered: the effects, the
    // fill opacity and both Blend If ramps all crossed.
    let expected = render(&source);
    let actual = render(&reopened);
    let worst = expected
        .iter()
        .zip(&actual)
        .map(|(e, a)| {
            (0..4)
                .map(|c| (i32::from(e[c]) - i32::from(a[c])).abs())
                .max()
                .unwrap()
        })
        .max()
        .unwrap();
    eprintln!("reopened style vs source render: worst {worst} codes");
    assert!(worst <= 1, "{worst}");

    // A styled group is written as its folder record's own effects.
    let grouped = document(
        OW,
        OH,
        vec![with_style(
            Layer::isolated_normal(
                "G",
                vec![layer("R", OL.0, OL.1, pixels.clone(), OAT, 1.)],
                1.,
                true,
            )
            .unwrap(),
            r#"{"effects":[{"type":"drop_shadow"}]}"#,
            (OW, OH),
        )],
    );
    let grouped_report = export_report(&grouped);
    assert_eq!(
        grouped_report["available"],
        Value::Bool(true),
        "{grouped_report}"
    );
    assert!(
        strings(&grouped_report["observations"])
            .iter()
            .any(|o| o.starts_with("Layer “G” has a layer style (drop_shadow);")),
        "{}",
        grouped_report["observations"]
    );

    // An authored lfx2 record imports with the marker loss and no style.
    let px = vec![[4097u16, 51234, 60000, 65535], [4098, 51233, 60000, 32768]];
    let mut record = Record::raster("R", [0, 0, 1, 2], planes(&px));
    record.blocks.extend(fixture::block(b"lfx2", &[255; 4]));
    let bytes = fixture::fixture(
        (2, 1),
        &[record],
        IccProfile::srgb().bytes(),
        (72.0, 72.0),
        &px,
        true,
        false,
    );
    let authored = directory.path().join("lfx2.psd");
    std::fs::write(&authored, bytes).unwrap();
    let notice = report(&authored);
    assert!(
        strings(&notice["verified_losses"])
            .iter()
            .any(|l| l.contains("layer-style metadata")),
        "{}",
        notice["verified_losses"]
    );
    let opened = open16(&authored, &notice);
    assert!(opened.layer(0).unwrap().style().is_none());
    assert_eq!(info(&opened)["layers"][0]["style"], Value::Null);
}

// ------------------------------------------------------------ (h) scratch --

#[test]
fn a_photo_sized_styled_layer_plans_within_a_gigabyte_and_renders() {
    let (w, h) = (5000u32, 4000u32);
    let px = vec![[20000u16, 30000, 40000, 65535]; (w * h) as usize];
    let doc = document(
        w,
        h,
        vec![with_style(
            layer("Photo", w, h, px, (0, 0), 1.),
            r#"{"effects":[{"type":"drop_shadow"}]}"#,
            (w, h),
        )],
    );
    let plan = doc.render_plan().unwrap();
    eprintln!(
        "5000×4000 drop shadow plan: {} MiB scratch",
        plan.scratch_bytes >> 20
    );
    assert!(plan.scratch_bytes <= GIB, "{}", plan.scratch_bytes);
    let rendered = doc.render(2 * GIB, 0, None).unwrap();
    assert_eq!(rendered.dimensions(), (w, h));
    let nine = r#"{"effects":[{"type":"drop_shadow"},{"type":"inner_shadow"},{"type":"outer_glow"},{"type":"inner_glow"},{"type":"stroke"},{"type":"color_overlay"},{"type":"gradient_overlay"},{"type":"bevel_emboss"},{"type":"satin"}]}"#;
    let nine = set(&doc, 0, serde_json::from_str(nine).unwrap());
    let plan = nine.render_plan().unwrap();
    eprintln!(
        "5000×4000 nine effects plan: {} MiB scratch",
        plan.scratch_bytes >> 20
    );
    assert!(
        plan.scratch_bytes <= GIB + GIB / 2,
        "{}",
        plan.scratch_bytes
    );
}

// --------------------------------------------------- the request's scale --

#[test]
fn scale_multiplies_the_style_lengths_before_the_checks() {
    let px = [1000u16, 2000, 3000, 65535];
    let doc = document(16, 16, vec![layer("T", 16, 16, vec![px; 256], (0, 0), 1.)]);
    let style = json!({"effects":[{"type":"drop_shadow","size":10,"distance":6}]});
    let scaled = |scale: Value| {
        let mut req = request(0, style.clone());
        req["scale"] = scale;
        req
    };
    // The lengths are the authored ones times the factor, written out here.
    let half = changed(&doc, scaled(json!(0.5)));
    assert_eq!(effect_size(&half, 0), (5.0, 3.0));
    let double = changed(&doc, scaled(json!(2.0)));
    assert_eq!(effect_size(&double, 0), (20.0, 12.0));
    // A factor of 1 and an absent key are the same edit.
    let plain = set(&doc, 0, style.clone());
    let one = changed(&doc, scaled(json!(1.0)));
    assert_eq!(effect_size(&plain, 0), (10.0, 6.0));
    assert_eq!(
        one.layer(0).unwrap().reported_style(),
        plain.layer(0).unwrap().reported_style()
    );
    // The equality check runs on the SCALED style: a layer already holding
    // the half-size style answers the exact no-op.
    assert!(edit(&half, scaled(json!(0.5))).unwrap().is_none());
    assert!(edit(&half, scaled(json!(1.0))).unwrap().is_some());
    // The plan refuses what the edit refuses.
    for bad in [json!(0.0), json!(-2.0)] {
        let message = "native layer style scale must be finite and greater than 0";
        assert_eq!(edit(&doc, scaled(bad.clone())).err().unwrap(), message);
        assert_eq!(plan(&doc, scaled(bad)).err().unwrap(), message);
    }
    assert!(edit(&doc, scaled(json!("half")))
        .err()
        .unwrap()
        .starts_with("invalid native layer style request"));
    // A null style clears whatever the scale is.
    let mut clear = request(0, Value::Null);
    clear["scale"] = json!(0.25);
    let cleared = changed(&half, clear.clone());
    assert!(cleared.layer(0).unwrap().style().is_none());
    assert!(edit(&doc, clear).unwrap().is_none());
}
