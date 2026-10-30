//! At 16 and 32 bits: a layer style or Blend If on a Pass
//! Through group. Take1's P1 and P3 measured Photoshop compositing such a
//! group ISOLATED (`PrecisionDocumentLayer::isolates`): its children no
//! longer reach the backdrop, so an adjustment inside it adjusts only the
//! group's own pixels.
//!
//! The scene is an opaque backdrop under a Pass Through group holding an
//! opaque square and an adjustment (Invert at 16 bits, Exposure +1 EV at 32).
//! Two independent oracles:
//! - TWIN: the same scene with the group made Normal (isolated) and the same
//!   style renders sample for sample the same;
//! - PLAIN: a backdrop pixel outside the children is the adjusted backdrop
//!   under the unstyled Pass Through group (Invert's `65535 − c`, Exposure's
//!   `2v`) and the backdrop itself under the styled one.
//!
//! Also: a styled Normal group switches to Pass Through and keeps its style;
//! the regional render agrees with the full render; `.rz` and the 8→16 and
//! 16→8 structure conversions carry the group as a styled Pass Through group.

use std::ffi::{c_char, CStr, CString};
use std::ptr;

use doctored_core::doc::{BlendMode, DrDocument, Layer as ByteLayer, LayerKind};
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_precision::{dr_precision_image_free, dr_precision_image_pixels_rgba16};
use doctored_core::ffi_precision_edit::dr_precision_doc_edit;
use doctored_core::ffi_regional::dr_precision_doc_render_region;
use doctored_core::icc::IccProfile;
use doctored_core::layer_insert::InsertPosition;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer as Layer};
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use doctored_core::precision_quantize::PrecisionDither;
use image::{Rgba, RgbaImage};
use serde_json::{json, Value};

use crate::common::styled;
use crate::float_fixture::float_doc;

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
const W: u32 = 24;
const H: u32 = 16;
/// The square inside the group: (x, y, w, h).
const SQUARE: (u32, u32, u32, u32) = (14, 4, 6, 6);
/// A backdrop pixel clear of the square and of a 2 px stroke around it.
const PLAIN: (u32, u32) = (2, 2);
/// The group's record: backdrop 0, square 1, adjustment 2, group 3.
const GROUP: usize = 3;

const STROKE_AND_BLEND_IF: &str = r##"{"blend_if":{"channel":"gray","this_layer":[0,0,128,128],"underlying":[0,0,255,255]},"effects":[{"type":"stroke","size":2,"color":"#ff0000"}]}"##;
const BLEND_IF_ONLY: &str =
    r#"{"blend_if":{"channel":"gray","this_layer":[0,0,128,128],"underlying":[0,0,255,255]}}"#;
const STROKE_ONLY: &str = r##"{"effects":[{"type":"stroke","size":2,"color":"#ff0000"}]}"##;
const FILL_ONLY: &str = r#"{"fill_opacity":0.5}"#;
const STYLES: [&str; 4] = [STROKE_AND_BLEND_IF, BLEND_IF_ONLY, STROKE_ONLY, FILL_ONLY];

// ------------------------------------------------------------- plumbing --

fn inside(x: u32, y: u32, (rx, ry, rw, rh): (u32, u32, u32, u32)) -> bool {
    x >= rx && x < rx + rw && y >= ry && y < ry + rh
}

fn backdrop8(x: u32, y: u32) -> [u8; 4] {
    let v = (12 + x * 9 + y) as u8;
    [v, 255 - v, v / 2 + 60, 255]
}

fn backdrop16(x: u32, y: u32) -> [u16; 4] {
    backdrop8(x, y).map(|v| u16::from(v) * 257)
}

fn square8(x: u32, y: u32) -> [u8; 4] {
    if inside(x, y, SQUARE) {
        [200, 200, 200, 255]
    } else {
        [0, 0, 0, 0]
    }
}

fn invert() -> String {
    json!({"type":"adjust","op":"invert","params":{},"evaluation":"precision_v1"}).to_string()
}

fn image16(f: impl Fn(u32, u32) -> [u8; 4]) -> PrecisionImage {
    let pixels = (0..W * H)
        .map(|i| f(i % W, i / W).map(|v| u16::from(v) * 257))
        .collect();
    PrecisionImage::from_rgba16(W, H, pixels, IccProfile::srgb(), None).unwrap()
}

fn children() -> Vec<Layer> {
    vec![
        Layer::raster("Square", image16(square8), (0, 0), 1.0, true).unwrap(),
        Layer::adjustment("Invert", &invert(), 1.0, true, &IccProfile::srgb()).unwrap(),
    ]
}

/// The scene with its group Pass Through (`pass`) or Normal.
fn scene(pass: bool) -> PrecisionDocument {
    let group = if pass {
        Layer::pass_through("P", children(), 1.0, true)
    } else {
        Layer::isolated_normal("P", children(), 1.0, true)
    }
    .unwrap();
    PrecisionDocument::from_parts(
        W,
        H,
        IccProfile::srgb(),
        Metadata::default(),
        Resolution::default(),
        vec![
            Layer::raster("Backdrop", image16(backdrop8), (0, 0), 1.0, true).unwrap(),
            group,
        ],
    )
    .unwrap()
}

fn take(p: *mut c_char) -> String {
    let text = unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned();
    unsafe { dr_string_free(p) };
    text
}

/// `dr_precision_doc_edit`; `Ok(None)` for an exact no-op.
fn edit(doc: &PrecisionDocument, request: Value) -> Result<Option<PrecisionDocument>, String> {
    let text = CString::new(request.to_string()).unwrap();
    let mut err = ptr::null_mut();
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
    if !err.is_null() {
        assert!(out.is_null());
        return Err(take(err));
    }
    Ok((!out.is_null()).then(|| *unsafe { Box::from_raw(out) }))
}

fn set_style(doc: &PrecisionDocument, layer: usize, style: &str) -> PrecisionDocument {
    let style: Value = serde_json::from_str(style).unwrap();
    edit(
        doc,
        json!({"op":"layer_style","version":1,"layer":layer,"style":style}),
    )
    .unwrap_or_else(|e| panic!("{style}: {e}"))
    .unwrap_or_else(|| panic!("{style} changed nothing"))
}

fn render16(doc: &PrecisionDocument) -> Vec<[u16; 4]> {
    doc.render(LIMIT, 0, None).unwrap().to_rgba16().unwrap()
}

fn at<T: Copy>(pixels: &[T], (x, y): (u32, u32)) -> T {
    pixels[(y * W + x) as usize]
}

// ------------------------------------------------------------- 16 bits --

#[test]
fn a_styled_pass_through_group_composites_isolated_at_16_bits() {
    let plain = render16(&scene(true));
    let b = backdrop16(PLAIN.0, PLAIN.1);
    // The unstyled group passes its adjustment through to the backdrop.
    assert_eq!(
        at(&plain, PLAIN),
        [65535 - b[0], 65535 - b[1], 65535 - b[2], 65535]
    );
    for style in STYLES {
        let pass = set_style(&scene(true), GROUP, style);
        let group = pass.layer(GROUP).unwrap();
        assert!(group.is_pass_through(), "{style}");
        assert_eq!(group.blend_mode(), BlendMode::PassThrough, "{style}");
        assert!(group.reported_style().is_some(), "{style}");
        let got = render16(&pass);
        assert_eq!(at(&got, PLAIN), b, "{style}: the backdrop is not adjusted");
        let twin = render16(&set_style(&scene(false), GROUP, style));
        assert_eq!(got, twin, "{style}: the isolated twin");
    }
}

#[test]
fn a_styled_normal_group_switches_to_pass_through_and_keeps_its_style() {
    for style in STYLES {
        let twin = set_style(&scene(false), GROUP, style);
        let pass = edit(
            &twin,
            json!({"op":"layer_blend","version":1,"layer":GROUP,
                   "blend_mode":BlendMode::PassThrough.to_c()}),
        )
        .unwrap()
        .expect("a change");
        let group = pass.layer(GROUP).unwrap();
        assert!(group.is_pass_through(), "{style}");
        assert_eq!(
            group.reported_style(),
            twin.layer(GROUP).unwrap().reported_style(),
            "{style}"
        );
        assert_eq!(render16(&pass), render16(&twin), "{style}");
        // And back: Normal keeps it too.
        let normal = edit(
            &pass,
            json!({"op":"layer_blend","version":1,"layer":GROUP,
                   "blend_mode":BlendMode::Normal.to_c()}),
        )
        .unwrap()
        .expect("a change");
        assert!(!normal.layer(GROUP).unwrap().is_pass_through());
        assert!(normal.layer(GROUP).unwrap().reported_style().is_some());
    }
}

#[test]
fn the_regional_render_agrees_with_the_full_render() {
    for style in STYLES {
        let doc = set_style(&scene(true), GROUP, style);
        let mut err = ptr::null_mut();
        let image = unsafe {
            dr_precision_doc_render_region(&doc, 0, 0, W, H, 1, LIMIT, 0, ptr::null(), &mut err)
        };
        assert!(!image.is_null(), "{style}: {}", take(err));
        let p16 = unsafe { dr_precision_image_pixels_rgba16(image) };
        assert!(!p16.is_null());
        let region: Vec<u16> =
            unsafe { std::slice::from_raw_parts(p16, (W * H * 4) as usize) }.to_vec();
        unsafe { dr_precision_image_free(image) };
        let full: Vec<u16> = render16(&doc).into_iter().flatten().collect();
        assert_eq!(region, full, "{style}");
    }
}

#[test]
fn native_save_keeps_the_styled_pass_through_group() {
    for style in STYLES {
        let doc = set_style(&scene(true), GROUP, style);
        let bytes = doc.encode_native(LIMIT, 0, None).unwrap();
        let back = PrecisionDocument::parse_native(&bytes, LIMIT, 0, None).unwrap();
        let group = back.layer(GROUP).unwrap();
        assert!(group.is_pass_through(), "{style}");
        assert_eq!(
            group.reported_style(),
            doc.layer(GROUP).unwrap().reported_style(),
            "{style}"
        );
        assert_eq!(render16(&back), render16(&doc), "{style}");
    }
}

// ------------------------------------------------------ 8 ↔ 16 bits --

fn byte_layer(name: &str, image: RgbaImage) -> ByteLayer {
    let mut layer = DrDocument::from_pixels(image).layers.remove(0);
    layer.name = name.into();
    layer
}

/// The byte scene: the backdrop, then the square and an Invert record at
/// depth 1, then the Pass Through group record carrying `style`.
fn byte_scene(style: Option<&str>) -> DrDocument {
    let mut doc = DrDocument::from_pixels(RgbaImage::from_fn(W, H, |x, y| Rgba(backdrop8(x, y))));
    let mut square = byte_layer(
        "Square",
        RgbaImage::from_fn(W, H, |x, y| Rgba(square8(x, y))),
    );
    square.depth = 1;
    let mut adjustment = byte_layer("Invert", RgbaImage::new(1, 1));
    adjustment.meta = Some(r#"{"type":"adjust","op":"invert","params":{}}"#.into());
    adjustment.depth = 1;
    let mut group = byte_layer("P", RgbaImage::new(1, 1));
    group.kind = LayerKind::Group;
    group.blend = BlendMode::PassThrough;
    doc.layers.extend([square, adjustment, group]);
    match style {
        Some(style) => styled(&doc, GROUP, style),
        None => doc,
    }
}

fn byte_render(doc: &DrDocument) -> Vec<[u8; 4]> {
    crate::common::flat_pixels(doc)
        .chunks_exact(4)
        .map(|p| [p[0], p[1], p[2], p[3]])
        .collect()
}

#[test]
fn the_byte_document_already_isolates_and_the_conversions_carry_the_group() {
    let plain = byte_render(&byte_scene(None));
    let b = backdrop8(PLAIN.0, PLAIN.1);
    assert_eq!(at(&plain, PLAIN), [255 - b[0], 255 - b[1], 255 - b[2], 255]);
    for style in [STROKE_AND_BLEND_IF, BLEND_IF_ONLY, STROKE_ONLY] {
        let doc = byte_scene(Some(style));
        let eight = byte_render(&doc);
        assert_eq!(at(&eight, PLAIN), b, "{style}: 8 bits isolates");
        // 8 → 16: a styled Pass Through group stays one, and no note says
        // it is carried as an isolated group.
        let plan = doc.promote_structure_plan().unwrap().json();
        let notes: Vec<(u64, String)> = plan["rendering_differences"]
            .as_array()
            .unwrap()
            .iter()
            .map(|row| {
                (
                    row["index"].as_u64().unwrap(),
                    row["note"].as_str().unwrap().to_string(),
                )
            })
            .collect();
        assert!(
            !notes
                .iter()
                .any(|(_, n)| n.contains("carried as an isolated group")),
            "{notes:?}"
        );
        assert!(
            notes
                .iter()
                .any(|(i, n)| *i == GROUP as u64 && n.contains("isolated group composites")),
            "{style}: {notes:?}"
        );
        let sixteen = doc.promote_structure(LIMIT, 0, None).unwrap();
        let group = sixteen.layer(GROUP).unwrap();
        assert!(group.is_pass_through(), "{style}");
        assert!(group.reported_style().is_some(), "{style}");
        let rendered = render16(&sixteen);
        assert_eq!(
            at(&rendered, PLAIN),
            b.map(|v| u16::from(v) * 257),
            "{style}: 16 bits isolates"
        );
        for (i, (e, s)) in eight.iter().zip(&rendered).enumerate() {
            for c in 0..4 {
                let s8 = (f64::from(s[c]) / 257.0).round();
                assert!(
                    (s8 - f64::from(e[c])).abs() <= 1.0,
                    "{style}: pixel {i} channel {c}: 8-bit {e:?} 16-bit {s:?}"
                );
            }
        }
        // 16 → 8: Pass Through with the style, and the same isolation.
        let back = sixteen
            .narrow_structure_to_u8(PrecisionDither::None, LIMIT, 0, None)
            .unwrap();
        assert_eq!(back.layers[GROUP].blend, BlendMode::PassThrough, "{style}");
        assert!(back.layers[GROUP].style.is_some(), "{style}");
        assert_eq!(at(&byte_render(&back), PLAIN), b, "{style}");
    }
}

// ------------------------------------------------------------- 32 bits --

fn linear(code: u8) -> f32 {
    let e = f64::from(code) / 255.0;
    (if e <= 0.04045 {
        e / 12.92
    } else {
        ((e + 0.055) / 1.055).powf(2.4)
    }) as f32
}

fn float_scene(pass: bool) -> PrecisionDocument {
    let backdrop: Vec<[f32; 4]> = (0..W * H)
        .map(|i| {
            let b = backdrop8(i % W, i / W);
            [linear(b[0]), linear(b[1]), linear(b[2]), 1.0]
        })
        .collect();
    let square: Vec<[f32; 4]> = (0..W * H)
        .map(|i| {
            let s = square8(i % W, i / W);
            [
                linear(s[0]),
                linear(s[1]),
                linear(s[2]),
                f32::from(s[3]) / 255.0,
            ]
        })
        .collect();
    let doc = float_doc(W, H, &IccProfile::srgb(), &[backdrop, square]);
    let description = json!({"type":"adjust","evaluation":"precision_v1","op":"exposure",
        "params":{"exposure":1.0,"offset":0.0,"gamma":1.0,"evaluation":"document_v2"}})
    .to_string();
    let doc = doc
        .add_adjustment_at(
            "Exposure",
            &description,
            1.0,
            1,
            InsertPosition::Above,
            LIMIT,
            0,
            None,
        )
        .unwrap()
        .unwrap();
    edit(
        &doc,
        json!({"op":"group_layers","layers":[1,2],"name":"P","pass_through":pass}),
    )
    .unwrap()
    .unwrap()
}

fn render32(doc: &PrecisionDocument) -> Vec<[f32; 4]> {
    doc.render(LIMIT, 0, None).unwrap().to_rgba32().unwrap()
}

#[test]
fn a_styled_pass_through_group_composites_isolated_at_32_bits() {
    let b = backdrop8(PLAIN.0, PLAIN.1);
    let b = [linear(b[0]), linear(b[1]), linear(b[2]), 1.0];
    let unstyled = float_scene(true);
    assert!(unstyled.layer(GROUP).unwrap().is_pass_through());
    let plain = at(&render32(&unstyled), PLAIN);
    for c in 0..3 {
        assert!(
            (plain[c] - 2.0 * b[c]).abs() <= 1e-5,
            "Exposure passes through: {plain:?} vs 2 × {b:?}"
        );
    }
    for style in [STROKE_AND_BLEND_IF, BLEND_IF_ONLY, STROKE_ONLY] {
        let pass = set_style(&float_scene(true), GROUP, style);
        assert!(pass.layer(GROUP).unwrap().is_pass_through(), "{style}");
        let got = render32(&pass);
        let p = at(&got, PLAIN);
        for c in 0..4 {
            assert!(
                (p[c] - b[c]).abs() <= 1e-5,
                "{style}: the backdrop is not exposed: {p:?} vs {b:?}"
            );
        }
        let twin = render32(&set_style(&float_scene(false), GROUP, style));
        assert_eq!(got, twin, "{style}: the isolated twin");
    }
}
