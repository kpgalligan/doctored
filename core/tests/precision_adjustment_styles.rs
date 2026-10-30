//! At 16 and 32 bits: a layer style and Blend If on a typed
//! ADJUSTMENT layer (`precision_style_adjustment`), the byte rule one depth
//! up (`style_adjustment_layer.rs` states it): the shape is the mask over the
//! canvas (S-MASK), the adjustment reads the accumulator after its own Below
//! effects (POST) and is applied in place with alpha kept (ALPHA-KEPT), Fill
//! scales `k`, This Layer reads the adjusted colour, and a styled base's
//! members are frozen to the shape — a Blend-If-only base included, which
//! renders under an identity style exactly as the byte one does.
//!
//! The oracles are written here: the interior in f64 as the unstyled
//! adjustment's lerp, Invert's `1 − c` (16 bits) and Exposure's `v · 2` in
//! linear light (32 bits), and the Blend If ramp table on 16-bit codes (knees
//! at 257 k, Gray `round(65535 · (0.30 R + 0.59 G + 0.11 B))`); POST as the
//! unstyled adjustment over the backdrop with the effects alone drawn on it
//! (the same style at Fill 0 on an opaque raster under the same mask).
//! Tolerances (declared before measuring): 2 codes of 65535 at 16 bits
//! against an oracle (one 16-bit storage step apart for the two-step one),
//! 1e-5 at 32 bits.

use std::ffi::{c_char, CStr, CString};
use std::ptr;

use doctored_core::doc::{BlendMode, DrDocument};
use doctored_core::doc_comps::{CompCaptures, CompRequest};
use doctored_core::doc_mask::MaskProperties;
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_precision_edit::dr_precision_doc_edit;
use doctored_core::ffi_regional::dr_precision_doc_render_region;
use doctored_core::icc::IccProfile;
use doctored_core::layer_insert::InsertPosition;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer as Layer};
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use doctored_core::precision_mask::{PrecisionMask, PrecisionPlane};
use doctored_core::style::LayerStyle;
use image::{Rgba, RgbaImage};
use serde_json::{json, Value};
use tempfile::TempDir;

use crate::common::{adjust_meta, styled};
use crate::float_fixture::float_doc;

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
const W: u32 = 24;
const H: u32 = 16;
const HOLE: (u32, u32, u32, u32) = (2, 5, 4, 4);
const BAND: (u32, u32, u32, u32) = (6, 5, 4, 4);
const MASK: (u32, u32, u32, u32) = (4, 2, 16, 12);
const PARTIAL_COLUMN: u32 = 12;
const FULL: [i64; 4] = [0, 0, 255, 255];
const SHADOW: &str = r#"{"effects":[{"type":"drop_shadow","opacity":1,"distance":3,"size":0,"angle":180,"use_global_light":false,"layer_knocks_out":false}]}"#;
const STROKE: &str = r##"{"effects":[{"type":"stroke","size":2,"color":"#ff0000"}]}"##;

fn inside(x: u32, y: u32, (rx, ry, rw, rh): (u32, u32, u32, u32)) -> bool {
    x >= rx && x < rx + rw && y >= ry && y < ry + rh
}

/// The byte test's backdrop, at 8-bit codes.
fn backdrop8(x: u32, y: u32) -> [u8; 4] {
    let v = (12 + x * 10 + y) as u8;
    let alpha = if inside(x, y, HOLE) {
        0
    } else if inside(x, y, BAND) {
        128
    } else {
        255
    };
    [v, 255 - v, v / 2 + 60, alpha]
}

fn backdrop16(x: u32, y: u32) -> [u16; 4] {
    backdrop8(x, y).map(|v| u16::from(v) * 257)
}

fn mask16(x: u32, y: u32) -> u16 {
    if !inside(x, y, MASK) {
        0
    } else if x == PARTIAL_COLUMN {
        128 * 257
    } else {
        65535
    }
}

fn invert() -> String {
    json!({"type":"adjust","op":"invert","params":{},"evaluation":"precision_v1"}).to_string()
}

fn image16(f: impl Fn(u32, u32) -> [u16; 4]) -> PrecisionImage {
    let pixels = (0..W * H).map(|i| f(i % W, i / W)).collect();
    PrecisionImage::from_rgba16(W, H, pixels, IccProfile::srgb(), None).unwrap()
}

fn mask() -> PrecisionMask {
    let samples = (0..W * H).map(|i| mask16(i % W, i / W)).collect();
    PrecisionMask::new(
        PrecisionPlane::from_u16(W, H, samples, None).unwrap(),
        MaskProperties::at((0, 0)),
        true,
    )
    .unwrap()
}

fn document(layers: Vec<Layer>) -> PrecisionDocument {
    PrecisionDocument::from_parts(
        W,
        H,
        IccProfile::srgb(),
        Metadata::default(),
        Resolution::default(),
        layers,
    )
    .unwrap()
}

fn backdrop_layer() -> Layer {
    Layer::raster("B", image16(backdrop16), (0, 0), 1.0, true).unwrap()
}

fn adjustment(opacity: f32, mode: BlendMode, masked: bool) -> Layer {
    let layer = Layer::adjustment("Adj", &invert(), opacity, true, &IccProfile::srgb())
        .unwrap()
        .with_blend_mode(mode)
        .unwrap();
    if masked {
        layer.with_mask(Some(mask()))
    } else {
        layer
    }
}

fn parse(json: &str) -> LayerStyle {
    LayerStyle::from_json(json).unwrap_or_else(|e| panic!("{json}: {e}"))
}

fn with_style(layer: Layer, json: &str) -> Layer {
    layer
        .with_layer_style(Some(&parse(json)), (W, H))
        .unwrap_or_else(|e| panic!("{json}: {e}"))
}

fn render16(doc: &PrecisionDocument) -> Vec<[u16; 4]> {
    doc.render(LIMIT, 0, None).unwrap().to_rgba16().unwrap()
}

fn edit(doc: &PrecisionDocument, request: Value) -> Result<Option<PrecisionDocument>, String> {
    let request = CString::new(request.to_string()).unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    let result = unsafe {
        dr_precision_doc_edit(
            doc,
            request.as_ptr(),
            ptr::null(),
            0,
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    if !err.is_null() {
        let message = unsafe { CStr::from_ptr(err) }
            .to_string_lossy()
            .into_owned();
        unsafe { dr_string_free(err) };
        return Err(message);
    }
    Ok((!result.is_null()).then(|| *unsafe { Box::from_raw(result) }))
}

fn set_style(doc: &PrecisionDocument, layer: usize, style: Value) -> PrecisionDocument {
    edit(
        doc,
        json!({"op":"layer_style","version":1,"layer":layer,"style":style}),
    )
    .unwrap()
    .expect("the style changed")
}

// ---------------------------------------------------------------- oracle --

fn blend64(mode: BlendMode, cb: f64, cs: f64) -> f64 {
    match mode {
        BlendMode::Normal => cs,
        BlendMode::Multiply => cb * cs,
        other => panic!("no oracle for {other:?}"),
    }
}

/// The ramp table over knees at `257 k` (`style_blend_if::source_weight_16`).
fn ramp16(v: f64, knees: [i64; 4]) -> f64 {
    let [lo0, lo1, hi0, hi1] = knees.map(|k| (k * 257) as f64);
    if v < lo0 {
        0.0
    } else if v < lo1 {
        (v - lo0) / (lo1 - lo0)
    } else if v <= hi0 {
        1.0
    } else if v <= hi1 {
        (hi1 - v) / (hi1 - hi0)
    } else {
        0.0
    }
}

fn gray16(rgb: [f64; 3]) -> f64 {
    (65535.0 * (0.30 * rgb[0] + 0.59 * rgb[1] + 0.11 * rgb[2])).round()
}

struct Case {
    fill: f64,
    opacity: f64,
    mode: BlendMode,
    masked: bool,
    this: [i64; 4],
    under: [i64; 4],
}

fn oracle16(case: &Case, x: u32, y: u32) -> [f64; 4] {
    let b = backdrop16(x, y).map(|v| f64::from(v) / 65535.0);
    if b[3] == 0.0 {
        return b;
    }
    let a = [1.0 - b[0], 1.0 - b[1], 1.0 - b[2]];
    let m = if case.masked {
        f64::from(mask16(x, y)) / 65535.0
    } else {
        1.0
    };
    let w = ramp16(gray16(a), case.this) * ramp16(gray16([b[0], b[1], b[2]]), case.under);
    // The Fill and the opacity as their two bytes' product, rounded to
    // a byte (#2, `opacity_byte::joint`).
    let k = m * crate::common::joint_unit(case.fill, case.opacity) * w;
    let mut out = b;
    for c in 0..3 {
        out[c] = b[c] + (blend64(case.mode, b[c], a[c]) - b[c]) * k;
    }
    out
}

fn close16(got: [u16; 4], want: [f64; 4], what: &str) {
    let want = want.map(|v| v * 65535.0);
    let channels = if want[3] < 0.5 { 3..4 } else { 0..4 };
    for c in channels {
        assert!(
            (f64::from(got[c]) - want[c]).abs() <= 2.0,
            "{what}: channel {c} got {got:?} want {want:?}"
        );
    }
}

fn blend_if(this: [i64; 4], under: [i64; 4]) -> String {
    format!(
        "{{\"blend_if\":{{\"channel\":\"gray\",\"this_layer\":{this:?},\"underlying\":{under:?}}}}}"
    )
}

// ------------------------------------------------------------- 16 bits --

#[test]
fn the_typed_interior_matches_the_f64_oracle() {
    let plain = |fill, this, under, masked| Case {
        fill,
        opacity: 1.0,
        mode: BlendMode::Normal,
        masked,
        this,
        under,
    };
    let cases = [
        (
            "fill 50 %",
            plain(0.5, FULL, FULL, false),
            "{\"fill_opacity\":0.5}".to_string(),
        ),
        (
            "masked Multiply at 60 % with fill 75 %",
            Case {
                fill: 0.75,
                opacity: 0.6,
                mode: BlendMode::Multiply,
                masked: true,
                this: FULL,
                under: FULL,
            },
            "{\"fill_opacity\":0.75}".to_string(),
        ),
        (
            "This Layer reads the adjusted colour",
            plain(1.0, [0, 0, 128, 128], FULL, true),
            blend_if([0, 0, 128, 128], FULL),
        ),
        (
            "Underlying reads the composite beneath",
            plain(1.0, FULL, [128, 128, 255, 255], false),
            blend_if(FULL, [128, 128, 255, 255]),
        ),
    ];
    for (what, case, json) in cases {
        let doc = document(vec![
            backdrop_layer(),
            with_style(
                adjustment(case.opacity as f32, case.mode, case.masked),
                &json,
            ),
        ]);
        let out = render16(&doc);
        for y in 0..H {
            for x in 0..W {
                close16(
                    out[(y * W + x) as usize],
                    oracle16(&case, x, y),
                    &format!("{what} at ({x}, {y})"),
                );
            }
        }
    }
}

/// `json` with Fill 0: the effects alone.
fn effects_only(json: &str) -> String {
    format!("{{\"fill_opacity\":0,{}", &json[1..])
}

fn within16(got: &[[u16; 4]], want: &[[u16; 4]], what: &str) {
    for (i, (g, w)) in got.iter().zip(want).enumerate() {
        let channels = if g[3] == 0 && w[3] == 0 { 3..4 } else { 0..4 };
        for c in channels {
            assert!(g[c].abs_diff(w[c]) <= 2, "{what} at {i}: {g:?} vs {w:?}");
        }
    }
}

#[test]
fn the_typed_shape_is_the_mask_and_the_adjustment_reads_its_below_effects() {
    // S-MASK: an unmasked adjustment's outside Stroke falls off the canvas,
    // so the projection is the unstyled adjustment's.
    assert_eq!(
        render16(&document(vec![
            backdrop_layer(),
            with_style(adjustment(1.0, BlendMode::Normal, false), STROKE),
        ])),
        render16(&document(vec![
            backdrop_layer(),
            adjustment(1.0, BlendMode::Normal, false),
        ]))
    );
    // A Color Overlay covers the mask over the canvas, the hole included.
    let overlaid = render16(&document(vec![
        backdrop_layer(),
        with_style(
            adjustment(1.0, BlendMode::Normal, true),
            r##"{"effects":[{"type":"color_overlay","color":"#0000ff"}]}"##,
        ),
    ]));
    for y in 0..H {
        for x in 0..W {
            if mask16(x, y) == 65535 {
                assert_eq!(overlaid[(y * W + x) as usize], [0, 0, 65535, 65535]);
            }
        }
    }
    // POST: the unstyled adjustment over the backdrop with the effects alone
    // drawn on it by an opaque raster under the same mask.
    for json in [SHADOW, STROKE] {
        for masked in [true, false] {
            let got = render16(&document(vec![
                backdrop_layer(),
                with_style(adjustment(1.0, BlendMode::Normal, masked), json),
            ]));
            let shape = Layer::raster(
                "Shape",
                image16(|_, _| [9000, 30000, 50000, 65535]),
                (0, 0),
                1.0,
                true,
            )
            .unwrap();
            let shape = if masked {
                shape.with_mask(Some(mask()))
            } else {
                shape
            };
            let below = render16(&document(vec![
                backdrop_layer(),
                with_style(shape, &effects_only(json)),
            ]));
            let under = Layer::raster(
                "B",
                image16(|x, y| below[(y * W + x) as usize]),
                (0, 0),
                1.0,
                true,
            )
            .unwrap();
            let want = render16(&document(vec![
                under,
                adjustment(1.0, BlendMode::Normal, masked),
            ]));
            within16(&got, &want, &format!("{json} masked {masked}"));
        }
    }
}

#[test]
fn a_typed_semi_transparent_backdrop_keeps_its_alpha() {
    // ALPHA-KEPT (the take's a1 band): the band stays at 50 %, inverted.
    let out = render16(&document(vec![
        backdrop_layer(),
        with_style(adjustment(1.0, BlendMode::Normal, false), STROKE),
    ]));
    for y in BAND.1..BAND.1 + BAND.3 {
        for x in BAND.0..BAND.0 + BAND.2 {
            let b = backdrop16(x, y);
            let got = out[(y * W + x) as usize];
            assert_eq!(got[3], b[3], "alpha at ({x}, {y})");
            for c in 0..3 {
                assert!(got[c].abs_diff(65535 - b[c]) <= 1, "({x}, {y}) {got:?}");
            }
        }
    }
}

#[test]
fn a_typed_styled_adjustment_base_freezes_its_member_and_follows_the_byte_rule() {
    let member = || {
        Layer::raster(
            "Member",
            image16(|_, _| [65535, 0, 0, 65535]),
            (0, 0),
            1.0,
            true,
        )
        .unwrap()
        .with_clipped(true)
    };
    for json in [SHADOW, &blend_if([0, 0, 250, 255], FULL)] {
        let doc = document(vec![
            backdrop_layer(),
            with_style(adjustment(1.0, BlendMode::Normal, true), json),
            member(),
        ]);
        let out = render16(&doc);
        for y in 0..H {
            for x in 0..W {
                if mask16(x, y) == 65535 {
                    assert_eq!(
                        out[(y * W + x) as usize],
                        [65535, 0, 0, 65535],
                        "{json} ({x}, {y})"
                    );
                }
            }
        }
        // The byte document of the same codes, the same style: within one
        // 8-bit code everywhere (the typed planes are byte-quantized).
        let mut byte =
            DrDocument::from_pixels(RgbaImage::from_fn(W, H, |x, y| Rgba(backdrop8(x, y))))
                .adding_image_layer(0, RgbaImage::new(W, H), "Adj")
                .unwrap();
        byte.layers[1].meta = Some(adjust_meta("invert", "{}"));
        byte.layers[1].mask = Some(std::sync::Arc::new(image::GrayImage::from_fn(
            W,
            H,
            |x, y| image::Luma([(mask16(x, y) / 257) as u8]),
        )));
        let byte = styled(&byte, 1, json)
            .adding_image_layer(
                1,
                RgbaImage::from_pixel(W, H, Rgba([255, 0, 0, 255])),
                "Member",
            )
            .unwrap();
        let mut byte = byte;
        byte.layers[2].clipped = true;
        let flat = byte.flattened().unwrap();
        for (i, got) in out.iter().enumerate() {
            let want = flat.as_raw()[i * 4..i * 4 + 4].to_vec();
            let channels = if want[3] == 0 { 3..4 } else { 0..4 };
            for c in channels {
                let typed = f64::from(got[c]) / 257.0;
                assert!(
                    (typed - f64::from(want[c])).abs() <= 1.0,
                    "{json} at {i}: typed {got:?} byte {want:?}"
                );
            }
        }
    }
    // An UNSTYLED adjustment base keeps today's rule: nothing of its unit.
    let unstyled = render16(&document(vec![
        backdrop_layer(),
        adjustment(1.0, BlendMode::Normal, true),
        member(),
    ]));
    assert_eq!(unstyled, render16(&document(vec![backdrop_layer()])));
}

#[test]
fn typed_styles_round_trip_convert_and_follow_comps() {
    let dir = TempDir::new().unwrap();
    let json = r##"{"fill_opacity":0.75,"blend_if":{"channel":"gray","this_layer":[0,0,200,255],"underlying":[0,0,255,255]},"effects":[{"type":"stroke","size":2,"color":"#ff0000"}]}"##;
    let doc = document(vec![
        backdrop_layer(),
        with_style(adjustment(1.0, BlendMode::Normal, true), json),
    ]);
    let projection = render16(&doc);
    // v35 LBIF + v36 LSTY: no new word.
    let path = dir.path().join("adjustment-style.rz");
    doc.save_native(path.to_str().unwrap(), LIMIT, 0, None)
        .unwrap();
    let back = PrecisionDocument::open_native(path.to_str().unwrap(), LIMIT, 0, None).unwrap();
    assert_eq!(
        back.layer(1).unwrap().reported_style(),
        doc.layer(1).unwrap().reported_style()
    );
    assert!(
        back.layer(1).unwrap().blend_if().is_some() && back.layer(1).unwrap().style().is_some()
    );
    assert_eq!(render16(&back), projection);
    // 8 → 16: the byte style travels on the adjustment record.
    let mut byte = DrDocument::from_pixels(RgbaImage::from_fn(W, H, |x, y| Rgba(backdrop8(x, y))))
        .adding_image_layer(0, RgbaImage::new(W, H), "Adj")
        .unwrap();
    byte.layers[1].meta = Some(adjust_meta("invert", "{}"));
    let byte = styled(&byte, 1, json);
    let promoted = byte.promote_structure(LIMIT, 0, None).unwrap();
    assert_eq!(
        promoted.layer(1).unwrap().reported_style(),
        Some(parse(json).to_json())
    );
    // A comp records the adjustment's style and restores it.
    let save = CompRequest::Save {
        name: "Styled".into(),
        comment: String::new(),
        captures: CompCaptures::default(),
    };
    let saved = doc.edit_layer_comps(&save).unwrap().unwrap();
    let cleared = edit(
        &saved,
        json!({"op":"layer_style","version":1,"layer":1,"style":null}),
    )
    .unwrap()
    .unwrap();
    assert!(cleared.layer(1).unwrap().reported_style().is_none());
    let applied = cleared
        .edit_layer_comps(&CompRequest::Apply { comp: 0 })
        .unwrap()
        .unwrap();
    assert_eq!(
        applied.layer(1).unwrap().reported_style(),
        doc.layer(1).unwrap().reported_style()
    );
}

#[test]
fn a_pass_through_group_carries_a_style_and_the_regional_render_names_a_styled_adjustment() {
    // A Pass Through group carries both.
    let group = Layer::pass_through("P", vec![backdrop_layer()], 1.0, true).unwrap();
    assert!(group
        .clone()
        .with_layer_style(Some(&parse(SHADOW)), (W, H))
        .unwrap()
        .style()
        .is_some());
    assert!(group
        .with_layer_style(Some(&parse(&blend_if([0, 0, 128, 128], FULL))), (W, H))
        .unwrap()
        .blend_if()
        .is_some());
    // The edit names nothing for an adjustment: it is accepted.
    let doc = document(vec![
        backdrop_layer(),
        adjustment(1.0, BlendMode::Normal, false),
    ]);
    let styled_doc = set_style(&doc, 1, serde_json::from_str(SHADOW).unwrap());
    assert!(styled_doc.layer(1).unwrap().style().is_some());
    let mut err: *mut c_char = ptr::null_mut();
    let image = unsafe {
        dr_precision_doc_render_region(&styled_doc, 0, 0, W, H, 1, LIMIT, 0, ptr::null(), &mut err)
    };
    assert!(image.is_null());
    let message = unsafe { CStr::from_ptr(err) }
        .to_string_lossy()
        .into_owned();
    unsafe { dr_string_free(err) };
    assert_eq!(
        message,
        "regional render refuses layer 1 (Adj): is an adjustment layer with a layer style or Blend If"
    );
}

// ------------------------------------------------------------- 32 bits --

fn linear(code: u8) -> f64 {
    let e = f64::from(code) / 255.0;
    if e <= 0.04045 {
        e / 12.92
    } else {
        ((e + 0.055) / 1.055).powf(2.4)
    }
}

fn backdrop32(x: u32, y: u32) -> [f32; 4] {
    let b = backdrop8(x, y);
    [
        linear(b[0]) as f32,
        linear(b[1]) as f32,
        linear(b[2]) as f32,
        f32::from(b[3]) / 255.0,
    ]
}

/// The float scene: the backdrop in linear light under Exposure +1 EV.
fn float_scene() -> PrecisionDocument {
    let pixels: Vec<_> = (0..W * H).map(|i| backdrop32(i % W, i / W)).collect();
    let doc = float_doc(W, H, &IccProfile::srgb(), &[pixels]);
    let description = json!({"type":"adjust","evaluation":"precision_v1","op":"exposure",
        "params":{"exposure":1.0,"offset":0.0,"gamma":1.0,"evaluation":"document_v2"}})
    .to_string();
    doc.add_adjustment("Adj", &description, 1.0, LIMIT, 0, None)
        .unwrap()
        .unwrap()
}

fn render32(doc: &PrecisionDocument) -> Vec<[f32; 4]> {
    doc.render(LIMIT, 0, None).unwrap().to_rgba32().unwrap()
}

/// Exposure's adjusted pixel in linear light, `v · 2`, reached at `fill`:
/// `v + (2v − v) · fill`, alpha kept.
fn oracle32(x: u32, y: u32, fill: f64) -> [f64; 4] {
    let b = backdrop32(x, y).map(f64::from);
    if b[3] == 0.0 {
        return b;
    }
    [
        b[0] * (1.0 + fill),
        b[1] * (1.0 + fill),
        b[2] * (1.0 + fill),
        b[3],
    ]
}

fn close32(got: [f32; 4], want: [f64; 4], what: &str) {
    let channels = if want[3] == 0.0 { 3..4 } else { 0..4 };
    for c in channels {
        assert!(
            (f64::from(got[c]) - want[c]).abs() <= 1e-5,
            "{what}: channel {c} got {got:?} want {want:?}"
        );
    }
}

#[test]
fn a_float_styled_adjustment_scales_by_fill_and_keeps_its_alpha() {
    let doc = float_scene();
    let filled = set_style(&doc, 1, json!({"fill_opacity": 0.5}));
    let out = render32(&filled);
    for y in 0..H {
        for x in 0..W {
            close32(
                out[(y * W + x) as usize],
                oracle32(x, y, 0.5),
                &format!("fill ({x}, {y})"),
            );
        }
    }
    // ALPHA-KEPT: the band stays at 50 %, fully adjusted.
    let stroked = set_style(&doc, 1, serde_json::from_str(STROKE).unwrap());
    let out = render32(&stroked);
    for y in BAND.1..BAND.1 + BAND.3 {
        for x in BAND.0..BAND.0 + BAND.2 {
            close32(
                out[(y * W + x) as usize],
                oracle32(x, y, 1.0),
                &format!("ALPHA-KEPT ({x}, {y})"),
            );
        }
    }
}

#[test]
fn a_float_styled_adjustment_base_freezes_its_member() {
    let doc = set_style(&float_scene(), 1, serde_json::from_str(SHADOW).unwrap());
    // A red member above it, clipped.
    let red: Vec<_> = (0..W * H).map(|_| [1.0f32, 0.0, 0.0, 1.0]).collect();
    let pixels: Vec<_> = (0..W * H).map(|i| backdrop32(i % W, i / W)).collect();
    let with_member = float_doc(W, H, &IccProfile::srgb(), &[pixels, red]);
    // The member is record 1 of `with_member`; the styled adjustment is
    // inserted between them, then the member clips to it.
    let description = json!({"type":"adjust","evaluation":"precision_v1","op":"exposure",
        "params":{"exposure":1.0,"offset":0.0,"gamma":1.0,"evaluation":"document_v2"}})
    .to_string();
    let unit = with_member
        .add_adjustment_at(
            "Adj",
            &description,
            1.0,
            0,
            InsertPosition::Above,
            LIMIT,
            0,
            None,
        )
        .unwrap()
        .unwrap();
    let unit = set_style(&unit, 1, serde_json::from_str(SHADOW).unwrap());
    let unit = edit(
        &unit,
        json!({"op":"layer_clipped","layer":2,"clipped":true}),
    )
    .unwrap()
    .unwrap();
    assert!(unit.layer(1).unwrap().style().is_some() && unit.layer(2).unwrap().clipped());
    let out = render32(&unit);
    for y in 0..H {
        for x in 0..W {
            // C-SHAPE on the S-MASK shape: an unmasked base's shape is the
            // whole canvas, so the member shows everywhere, the hole too.
            close32(
                out[(y * W + x) as usize],
                [1.0, 0.0, 0.0, 1.0],
                &format!("member ({x}, {y})"),
            );
        }
    }
    assert!(doc.layer(1).unwrap().style().is_some());
}
