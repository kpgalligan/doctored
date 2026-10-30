//! 13B-3c-1's compositor parity record: the typed compositor's reads moved
//! from `pixel(index)` onto the tiles' own runs (`row_segments`) and samples
//! (`sample_at`), so the full projection (`dr_precision_doc_render`, the
//! row path) and the regional render (`dr_precision_doc_render_region` at
//! divisor 1, the per-sample path) must stay equal sample for sample over
//! 600×520 canvases whose rasters sit partially off each edge, entirely off
//! each side, as a 1×1 at (257, 3), across tile seams at non-tile-aligned
//! offsets, masked, Multiply at 0.7 and (u16) under Blend If, styled with a
//! drop shadow and a stroke, a knockout-shallow styled base with a clipped
//! member and a styled isolated group; and a 4400×40 canvas whose 4400-wide
//! raster at (−50, 0) crosses a 4096-sample chunk boundary mid-run.
//!
//! The oracle is INDEPENDENT of the regional code: the full projection read
//! back through `dr_precision_image_pixels_rgba16` / `to_rgba32` and cropped in
//! plain index arithmetic, compared as u16 codes / f32 bit patterns. Each
//! fixture prints `parity <label> <depth> <sha256>` of the projection's
//! little-endian samples: the same lines from the HEAD archive and from the
//! changed tree are the byte-identity record. This file uses only APIs that
//! exist at 740294b so it runs on both.
//!
//! The u16 fixtures are built with `PrecisionDocument::from_parts`; the f32
//! ones from a float TIFF (`tiff_float::encode_to_vec`) opened with
//! `open_preserving`, the rasters copied out of it and pasted in place, the
//! offsets set through the admitted float `canvas_size` / `crop`. Styles and
//! Blend If are refused at float, so those fixtures are u16 only.

use std::ffi::{c_char, CStr, CString};
use std::ptr;
use std::sync::Arc;

use doctored_core::doc::BlendMode;
use doctored_core::doc_mask::MaskProperties;
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_precision::{
    dr_precision_doc_render, dr_precision_image_free, dr_precision_image_height,
    dr_precision_image_pixels_rgba16, dr_precision_image_width,
};
use doctored_core::ffi_precision_edit::dr_precision_doc_edit;
use doctored_core::ffi_regional::dr_precision_doc_render_region;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer as Layer};
use doctored_core::precision_geometry::GeometryEdit;
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use doctored_core::precision_mask::{PrecisionMask, PrecisionPlane};
use doctored_core::style::{BlendIf, LayerStyle};
use doctored_core::tiff_float::FloatTiff;
use serde_json::{json, Value};

use crate::common::sha256;

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
const W: u32 = 600;
const H: u32 = 520;

// ------------------------------------------------------------- plumbing --

fn take(p: *mut c_char) -> String {
    if p.is_null() {
        return "<no error message>".into();
    }
    let text = unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned();
    unsafe { dr_string_free(p) };
    text
}

/// A rendered image's dimensions and samples as `u32` (u16 codes widened,
/// f32 through `to_bits`), plus its little-endian sample bytes.
struct Samples {
    width: u32,
    height: u32,
    values: Vec<u32>,
    bytes: Vec<u8>,
}

fn samples(image: *mut PrecisionImage) -> Samples {
    let (width, height) = unsafe {
        (
            dr_precision_image_width(image),
            dr_precision_image_height(image),
        )
    };
    let n = (width * height * 4) as usize;
    let p16 = unsafe { dr_precision_image_pixels_rgba16(image) };
    let (values, bytes) = if !p16.is_null() {
        let s = unsafe { std::slice::from_raw_parts(p16, n) };
        (
            s.iter().map(|&v| u32::from(v)).collect(),
            s.iter().flat_map(|v| v.to_le_bytes()).collect(),
        )
    } else {
        let owned = unsafe { &*image }
            .to_rgba32()
            .expect("an image with samples");
        let s = owned.as_flattened();
        (
            s.iter().map(|v| v.to_bits()).collect(),
            s.iter().flat_map(|v| v.to_le_bytes()).collect(),
        )
    };
    unsafe { dr_precision_image_free(image) };
    Samples {
        width,
        height,
        values,
        bytes,
    }
}

fn full(doc: &PrecisionDocument) -> Samples {
    let mut err: *mut c_char = ptr::null_mut();
    let image = unsafe { dr_precision_doc_render(doc, LIMIT, 0, ptr::null(), &mut err) };
    assert!(!image.is_null(), "render: {}", take(err));
    samples(image)
}

fn region(doc: &PrecisionDocument, (x, y, w, h): (u32, u32, u32, u32)) -> Result<Samples, String> {
    let mut err: *mut c_char = ptr::null_mut();
    let image = unsafe {
        dr_precision_doc_render_region(doc, x, y, w, h, 1, LIMIT, 0, ptr::null(), &mut err)
    };
    if image.is_null() {
        return Err(take(err));
    }
    Ok(samples(image))
}

/// The independent oracle: the window cut out of the full projection.
fn crop(reference: &Samples, (x, y, w, h): (u32, u32, u32, u32)) -> Vec<u32> {
    let mut out = Vec::with_capacity((w * h * 4) as usize);
    for row in y..y + h {
        let at = ((row * reference.width + x) * 4) as usize;
        out.extend_from_slice(&reference.values[at..at + (w * 4) as usize]);
    }
    out
}

/// Full projection == whole-canvas region bitwise == crops of windows
/// touching each edge, a partial-overlap window and a 1×1 window; a 0×0
/// region refuses; prints the projection's `sha256`.
fn assert_parity(doc: &PrecisionDocument, label: &str, depth: &str) {
    let reference = full(doc);
    let (cw, ch) = doc.dimensions();
    assert_eq!((reference.width, reference.height), (cw, ch));
    let whole = region(doc, (0, 0, cw, ch)).unwrap_or_else(|e| panic!("{label}: whole: {e}"));
    assert!(
        whole.values == reference.values,
        "{label} {depth}: the whole-canvas region equals the projection"
    );
    let (ww, wh) = (cw.min(70), ch.min(40));
    let windows = [
        (0, (ch / 3).min(ch - wh), ww, wh),
        (cw / 3, 0, ww, wh),
        (cw - ww, (ch / 2).min(ch - wh), ww, wh),
        (cw / 2, ch - wh, ww, wh),
        (0, 0, ww, wh),
        (cw - ww, ch - wh, ww, wh),
        (cw / 4, ch / 5, (cw / 2).max(1), (ch / 2).max(1)),
        (257.min(cw - 1), 3.min(ch - 1), 1, 1),
        (cw - 1, ch - 1, 1, 1),
    ];
    for window in windows {
        let got = region(doc, window).unwrap_or_else(|e| panic!("{label}: {window:?}: {e}"));
        assert_eq!((got.width, got.height), (window.2, window.3));
        assert!(
            got.values == crop(&reference, window),
            "{label} {depth}: window {window:?}"
        );
    }
    assert_eq!(
        region(doc, (0, 0, 0, 0)).err().as_deref(),
        Some("render region must have nonzero dimensions"),
        "{label}: a 0×0 region refuses"
    );
    println!(
        "parity {label} {depth} {}",
        sha256::sha256_hex(&reference.bytes)
    );
}

fn profile() -> Arc<IccProfile> {
    IccProfile::srgb()
}

fn pattern16(w: u32, h: u32, seed: u32, opaque: bool) -> Vec<[u16; 4]> {
    (0..w * h)
        .map(|i| {
            let (x, y) = (i % w, i / w);
            [
                ((x * 2711 + y * 911 + seed * 1234) % 65536) as u16,
                ((x * 523 + y * 3301 + seed * 77) % 65536) as u16,
                (((x ^ y) * 1999 + seed) % 65536) as u16,
                if opaque {
                    65535
                } else {
                    (9000 + (x * 3001 + y * 1777 + seed * 97) % 56000) as u16
                },
            ]
        })
        .collect()
}

fn image16(w: u32, h: u32, seed: u32, opaque: bool) -> PrecisionImage {
    PrecisionImage::from_rgba16(w, h, pattern16(w, h, seed, opaque), profile(), None).unwrap()
}

fn raster(name: &str, (w, h): (u32, u32), offset: (i32, i32), seed: u32) -> Layer {
    Layer::raster(name, image16(w, h, seed, false), offset, 1.0, true).unwrap()
}

fn base16() -> Layer {
    Layer::raster("Base", image16(W, H, 1, true), (0, 0), 1.0, true).unwrap()
}

fn doc16(width: u32, height: u32, layers: Vec<Layer>) -> PrecisionDocument {
    PrecisionDocument::from_parts(
        width,
        height,
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
            ((x * 65535 / (w - 1).max(1)) ^ (y * 2311)) as u16
        })
        .collect()
}

fn mask(w: u32, h: u32, offset: (i32, i32)) -> PrecisionMask {
    PrecisionMask::new(
        PrecisionPlane::from_u16(w, h, gradient(w, h), None).unwrap(),
        MaskProperties::at(offset),
        true,
    )
    .unwrap()
}

fn style(json: &str) -> LayerStyle {
    LayerStyle::from_json(json).unwrap_or_else(|e| panic!("{json}: {e}"))
}

fn styled(layer: Layer, json: &str) -> Layer {
    layer.with_layer_style(Some(&style(json)), (W, H)).unwrap()
}

fn edit(doc: &PrecisionDocument, request: Value) -> PrecisionDocument {
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
    assert!(!out.is_null(), "{request}: {}", take(err));
    *unsafe { Box::from_raw(out) }
}

fn geometry(doc: &PrecisionDocument, request: Value) -> PrecisionDocument {
    let parsed = GeometryEdit::parse(&request.to_string()).expect("a geometry request");
    doc.geometry(&parsed, LIMIT, 0, None)
        .expect("the edit runs")
        .expect("the edit changes something")
}

// ------------------------------------------------------ the rasters --

/// Each fixture raster: name, size, offset on the 600×520 canvas.
type Placed = (&'static str, (u32, u32), (i32, i32));

/// Partially off each edge, then entirely off each side.
const OFF_CANVAS: [Placed; 8] = [
    ("OffLeft", (300, 200), (-100, 60)),
    ("OffRight", (300, 150), (450, 250)),
    ("OffTop", (250, 200), (200, -80)),
    ("OffBottom", (280, 200), (100, 400)),
    ("GoneLeft", (200, 100), (-300, 100)),
    ("GoneRight", (200, 100), (650, 100)),
    ("GoneTop", (200, 100), (100, -250)),
    ("GoneBottom", (200, 100), (100, 560)),
];

/// A 1×1 at (257, 3) and seam-crossing, non-tile-aligned rasters.
const SEAMS: [Placed; 3] = [
    ("Dot", (1, 1), (257, 3)),
    ("Seam", (390, 260), (130, 140)),
    ("Cross", (301, 300), (211, 213)),
];

const MASKED: Placed = ("Masked", (350, 300), (201, 77));
const MULTIPLY: Placed = ("Multiply", (500, 200), (37, 255));
const BLEND_IF: Placed = ("BlendIf", (400, 300), (-20, 100));

fn u16_rasters(placed: &[Placed], seed: u32) -> Vec<Layer> {
    placed
        .iter()
        .enumerate()
        .map(|(i, (name, size, offset))| raster(name, *size, *offset, seed + i as u32))
        .collect()
}

// ------------------------------------------------------ float assembly --

/// The float source plane: a 1800×1600 extended-range pattern, so every
/// fixture raster is cut from its interior at `(600 + ox, 520 + oy)`.
const PAD: (u32, u32) = (600, 520);

fn float_samples(w: u32, h: u32, seed: u32) -> Vec<f32> {
    (0..w * h)
        .flat_map(|i| {
            let (x, y) = (i % w, i / w);
            let v = ((x * 37 + y * 101 + seed * 13) % 1000) as f32 / 1000.0;
            [
                v * 2.5 - 0.5,
                1.0 - v,
                ((x ^ y) % 97) as f32 / 64.0,
                0.2 + 0.8 * (((x + 3 * y + seed) % 64) as f32 / 63.0),
            ]
        })
        .collect()
}

fn open_float(w: u32, h: u32, seed: u32) -> PrecisionDocument {
    let tiff = FloatTiff {
        width: w,
        height: h,
        channels: 4,
        alpha: Some(false),
        samples: float_samples(w, h, seed),
        icc: None,
        resolution: None,
        metadata: Metadata::default(),
        non_finite: 0,
    };
    let bytes = doctored_core::tiff_float::encode_to_vec(&tiff, false).expect("float TIFF");
    let directory = tempfile::TempDir::new().unwrap();
    let path = directory.path().join("parity.tiff");
    std::fs::write(&path, &bytes).unwrap();
    let doc = PrecisionDocument::open_preserving(path.to_str().unwrap(), LIMIT, 0, None)
        .expect("the float document opens");
    assert!(doc.is_float());
    doc
}

/// A float document on the 600×520 canvas: the padded source plane (which
/// ends up partially off every edge) plus each `placed` raster, copied out
/// of the source at its padded offset and pasted in place, then the canvas
/// cropped to the 600×520 window at `PAD`. Returns the document and the
/// postorder index of each placed raster.
fn float_fixture(placed: &[Placed]) -> (PrecisionDocument, Vec<usize>) {
    let (sw, sh) = (W + 2 * PAD.0, H + 2 * PAD.1);
    let mut doc = open_float(sw, sh, 3);
    let mut indices = Vec::new();
    for (name, (w, h), (ox, oy)) in placed {
        let (x, y) = ((ox + PAD.0 as i32) as u32, (oy + PAD.1 as i32) as u32);
        let payload = doc
            .clipboard_copy(
                &serde_json::from_value(
                    json!({"op":"copy","layer":0,"x":x,"y":y,"width":w,"height":h}),
                )
                .unwrap(),
                None,
                LIMIT,
                0,
                None,
            )
            .expect("a float copy");
        let anchor = doc.layer_count() - 1;
        doc = doc
            .clipboard_paste(
                &payload,
                &serde_json::from_value(
                    json!({"name":name,"anchor":anchor,"position":"top","placement":"in_place"}),
                )
                .unwrap(),
                LIMIT,
                0,
                None,
            )
            .expect("a float paste");
        indices.push(doc.layer_count() - 1);
    }
    let doc = geometry(
        &doc,
        json!({"op":"crop","x":PAD.0,"y":PAD.1,"width":W,"height":H}),
    );
    assert_eq!(doc.dimensions(), (W, H));
    for ((_, _, offset), index) in placed.iter().zip(&indices) {
        assert_eq!(doc.layer(*index).unwrap().offset(), *offset);
    }
    (doc, indices)
}

// ---------------------------------------------------------------- tests --

#[test]
fn off_canvas_rasters_match_at_u16_and_float() {
    let mut layers = vec![base16()];
    layers.extend(u16_rasters(&OFF_CANVAS, 10));
    assert_parity(&doc16(W, H, layers), "off_canvas", "u16");
    let (doc, _) = float_fixture(&OFF_CANVAS);
    assert_parity(&doc, "off_canvas", "f32");
}

#[test]
fn a_dot_and_seam_crossing_rasters_match_at_u16_and_float() {
    let mut layers = vec![base16()];
    layers.extend(u16_rasters(&SEAMS, 20));
    assert_parity(&doc16(W, H, layers), "seams", "u16");
    let (doc, _) = float_fixture(&SEAMS);
    assert_parity(&doc, "seams", "f32");
}

#[test]
fn a_masked_raster_matches_at_u16_and_float() {
    let (name, size, offset) = MASKED;
    let masked = raster(name, size, offset, 30).with_mask(Some(mask(320, 330, (180, 60))));
    assert_parity(&doc16(W, H, vec![base16(), masked]), "masked", "u16");
    let (doc, indices) = float_fixture(&[MASKED]);
    let doc = doc
        .set_layer_mask(indices[0], Some(mask(320, 330, (180, 60))), LIMIT, 0, None)
        .expect("a float mask")
        .expect("attaching a mask is a change");
    assert_parity(&doc, "masked", "f32");
}

#[test]
fn a_multiply_raster_at_seventy_percent_matches_at_u16_and_float() {
    let (name, size, offset) = MULTIPLY;
    let multiply = Layer::raster(name, image16(size.0, size.1, 40, false), offset, 0.7, true)
        .unwrap()
        .with_blend_mode(BlendMode::Multiply)
        .unwrap();
    assert_parity(&doc16(W, H, vec![base16(), multiply]), "multiply", "u16");
    let (doc, indices) = float_fixture(&[MULTIPLY]);
    let doc = edit(
        &doc,
        json!({"op":"layer_blend","version":1,"layer":indices[0],"blend_mode":BlendMode::Multiply as i32}),
    );
    let doc = doc
        .set_layer_opacity(indices[0], 0.7)
        .expect("a float opacity")
        .expect("an opacity change");
    assert_parity(&doc, "multiply", "f32");
}

/// At u16 only: float Blend If reads the stored value
/// (`float_layer_styles.rs`), which the parity oracle here does not model.
#[test]
fn a_blend_if_raster_matches_at_u16() {
    let (name, size, offset) = BLEND_IF;
    let weighted = raster(name, size, offset, 50)
        .with_blend_if(Some(BlendIf {
            this_layer: [10, 60, 180, 240],
            underlying: [0, 40, 200, 255],
            ..BlendIf::default()
        }))
        .unwrap();
    assert_parity(&doc16(W, H, vec![base16(), weighted]), "blend_if", "u16");
}

#[test]
fn a_styled_raster_across_seams_and_off_canvas_matches_at_u16() {
    let shadowed = styled(
        raster("Styled", (300, 250), (400, 380), 60),
        "{\"effects\":[{\"type\":\"drop_shadow\",\"distance\":9,\"size\":3},\
         {\"type\":\"stroke\",\"size\":3,\"position\":\"outside\",\"color\":\"#ff2000\"}]}",
    );
    assert_parity(&doc16(W, H, vec![base16(), shadowed]), "styled", "u16");
}

#[test]
fn a_knockout_shallow_base_with_a_clipped_member_matches_at_u16() {
    let knockout = styled(
        raster("Knockout", (350, 280), (140, 120), 70),
        "{\"fill_opacity\":0.5,\"knockout\":\"shallow\"}",
    );
    let member = raster("Member", (300, 300), (250, 200), 71).with_clipped(true);
    assert_parity(
        &doc16(W, H, vec![base16(), knockout, member]),
        "knockout",
        "u16",
    );
}

#[test]
fn a_styled_isolated_group_matches_at_u16() {
    let group = Layer::isolated_normal(
        "Group",
        vec![
            raster("A", (330, 240), (60, 90), 80),
            raster("B", (280, 260), (250, 230), 81),
        ],
        0.9,
        true,
    )
    .unwrap();
    let group = styled(
        group,
        "{\"effects\":[{\"type\":\"drop_shadow\",\"distance\":7,\"size\":4}]}",
    );
    assert_parity(&doc16(W, H, vec![base16(), group]), "styled_group", "u16");
}

/// A 4400-wide raster at (−50, 0): each row's 4400 columns cross the
/// compositor's 4096-sample chunk boundary in the middle of a tile run.
#[test]
fn a_wide_raster_across_a_chunk_boundary_matches_at_u16_and_float() {
    let (cw, ch) = (4400u32, 40u32);
    let wide = Layer::raster("Wide", image16(cw, ch, 90, false), (-50, 0), 1.0, true).unwrap();
    assert_parity(&doc16(cw, ch, vec![wide]), "wide", "u16");
    let doc = open_float(cw, ch, 91);
    let doc = geometry(
        &doc,
        json!({"op":"canvas_size","width":cw,"height":ch,"x":-50,"y":0}),
    );
    assert_eq!(doc.layer(0).unwrap().offset(), (-50, 0));
    assert_parity(&doc, "wide", "f32");
}
