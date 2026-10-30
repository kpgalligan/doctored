//! CROP PAST THE CANVAS with the Background (Default) fill,
//! through `dr_doc_geometry_edit` (8 bits) and `dr_precision_doc_edit`
//! (16 bits, float, CMYK), each case against an oracle written here:
//!
//! * the new canvas is the window; every old sample lands bit-exact at its
//!   new offset `(−x, −y)`; a saved channel is 0 in the new area;
//! * an opaque Background (D5: visible, Normal, 100 %, unmasked, unstyled,
//!   full canvas at (0, 0)) with Delete Cropped Pixels ON is padded with the
//!   background colour; with it OFF, or on any other record (a normal layer,
//!   a hidden or offset bottom layer, one with a transparent pixel), the new
//!   area is transparent (measured against Photoshop: take1 of
//!   `crop-extend-665`, R1, R2, R7);
//! * a turned extended crop's corners are the colour exactly (Lanczos3) and
//!   its edge is the shared f64 reference over a padded plane (bicubic, ±1
//!   code), its saved channel the reference resampled once into the window;
//! * the comps follow the pad's moved origin; the plan charges the pad;
//! * every refusal by its sentence.
#![allow(clippy::needless_range_loop)]

use std::ffi::{c_char, CString};
use std::ptr;

use doctored_core::doc::DrDocument;
use doctored_core::ffi_cmyk::dr_precision_doc_convert_to_cmyk;
use doctored_core::ffi_comps::dr_doc_edit_layer_comps;
use doctored_core::ffi_delete_cropped::dr_doc_delete_cropped_pixels;
use doctored_core::ffi_geometry::{dr_doc_geometry_edit, dr_doc_geometry_edit_plan};
use doctored_core::ffi_precision_edit::dr_precision_doc_edit;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer as Layer};
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use doctored_core::precision_mask::{PrecisionChannel, PrecisionPlane};
use image::{GrayImage, Luma, Rgba, RgbaImage};
use serde_json::{json, Value};

use crate::cmyk_fixture::press_profile;
use crate::common::take_err_string;
use crate::float_fixture::float_doc;
use crate::inverse_mapping::{
    assert_plane_within_one, assert_rgba_within_one, invert, reference_plane, reference_rgba,
};

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
const W: u32 = 16;
const H: u32 = 12;
/// Take1's swatch, #C83228.
const RED: [u8; 3] = [200, 50, 40];
const IDENTITY: [f64; 6] = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];
/// CMYK ink exactly representable in f32, so its codes are exact.
const INK: [f64; 4] = [0.25, 0.75, 0.5, 0.125];

type Window = (i32, i32, u32, u32);
/// A reference plane: its samples, offset and size.
type Plane = (Vec<[u32; 4]>, (i32, i32), (u32, u32));

/// Each side alone, then all four.
const WINDOWS: [Window; 5] = [
    (-3, 0, W + 3, H),
    (0, -2, W, H + 2),
    (0, 0, W + 4, H),
    (0, 0, W, H + 5),
    (-3, -2, W + 7, H + 5),
];

fn red_unit() -> [f64; 3] {
    RED.map(|v| f64::from(v) / 255.0)
}

fn color() -> Value {
    json!({"color": red_unit(), "color_space": "document"})
}

fn straighten(
    matrix: [f64; 6],
    window: Window,
    filter: &str,
    delete: bool,
    background: Option<Value>,
) -> Value {
    let mut request = json!({"op":"straighten","matrix":matrix,"filter":filter,
        "x":window.0,"y":window.1,"width":window.2,"height":window.3,
        "delete_cropped":delete});
    if let Some(background) = background {
        request["background"] = background;
    }
    request
}

fn crop(window: Window, delete: bool, background: Option<Value>) -> Value {
    straighten(IDENTITY, window, "nearest", delete, background)
}

/// The Crop tool's matrix: rotate by `−angle` about the window's centre.
fn rotation(angle: f64, window: Window) -> [f64; 6] {
    let cx = f64::from(window.0) + f64::from(window.2) / 2.0;
    let cy = f64::from(window.1) + f64::from(window.3) / 2.0;
    let radians = -angle * std::f64::consts::PI / 180.0;
    let (s, c) = (radians.sin(), radians.cos());
    [c, s, -s, c, cx - c * cx + s * cy, cy - s * cx - c * cy]
}

// ------------------------------------------------------------- plumbing --

fn byte_try(doc: &DrDocument, request: &Value) -> Result<Option<DrDocument>, String> {
    let c = CString::new(request.to_string()).unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    let out = unsafe { dr_doc_geometry_edit(doc, c.as_ptr(), LIMIT, 0, ptr::null(), &mut err) };
    if out.is_null() {
        if err.is_null() {
            Ok(None)
        } else {
            Err(take_err_string(err))
        }
    } else {
        Ok(Some(*unsafe { Box::from_raw(out) }))
    }
}

fn byte_must(doc: &DrDocument, request: &Value) -> DrDocument {
    byte_try(doc, request)
        .unwrap_or_else(|e| panic!("{request}: {e}"))
        .unwrap_or_else(|| panic!("{request} changed nothing"))
}

fn byte_plan_scratch(doc: &DrDocument, request: &Value) -> u64 {
    let c = CString::new(request.to_string()).unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    let out = unsafe { dr_doc_geometry_edit_plan(doc, c.as_ptr(), &mut err) };
    assert!(!out.is_null(), "{request}: {}", take_err_string(err));
    let plan: Value = serde_json::from_str(&take_err_string(out)).unwrap();
    plan["scratch_bytes"].as_u64().expect("scratch_bytes")
}

fn typed_try(
    doc: &PrecisionDocument,
    request: &Value,
) -> Result<Option<PrecisionDocument>, String> {
    let c = CString::new(request.to_string()).unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_edit(
            doc,
            c.as_ptr(),
            ptr::null(),
            0,
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    if out.is_null() {
        if err.is_null() {
            Ok(None)
        } else {
            Err(take_err_string(err))
        }
    } else {
        Ok(Some(*unsafe { Box::from_raw(out) }))
    }
}

fn typed_must(doc: &PrecisionDocument, request: &Value) -> PrecisionDocument {
    typed_try(doc, request)
        .unwrap_or_else(|e| panic!("{request}: {e}"))
        .unwrap_or_else(|| panic!("{request} changed nothing"))
}

/// Delete Cropped Pixels' host trim at 8 bits (the byte arm leaves it to the
/// host, `dr_doc_delete_cropped_pixels`); the input when nothing reaches out.
fn trimmed(doc: DrDocument) -> DrDocument {
    let out = unsafe { dr_doc_delete_cropped_pixels(&doc) };
    if out.is_null() {
        doc
    } else {
        *unsafe { Box::from_raw(out) }
    }
}

// --------------------------------------------------------------- 8 bits --

fn pattern(w: u32, h: u32, seed: u8) -> RgbaImage {
    RgbaImage::from_fn(w, h, |x, y| {
        Rgba([
            seed.wrapping_add((x * 37 + y * 11) as u8),
            seed.wrapping_add((x * 5 + y * 53) as u8),
            seed ^ (x as u8),
            255,
        ])
    })
}

fn byte_channel_plane() -> GrayImage {
    GrayImage::from_fn(W, H, |x, y| Luma([(1 + (x * 13 + y * 7) % 250) as u8]))
}

fn byte_doc() -> DrDocument {
    let doc = DrDocument::from_pixels(pattern(W, H, 9));
    doc.add_channel(
        "Saved",
        byte_channel_plane().as_raw(),
        W,
        H,
        [255, 0, 0],
        0.5,
    )
    .unwrap()
}

/// A byte layer's sample at CANVAS point `(x, y)`; transparent off its storage.
fn byte_at(doc: &DrDocument, layer: usize, x: i32, y: i32) -> [u8; 4] {
    let l = &doc.layers[layer];
    let (lx, ly) = (x - l.offset.0, y - l.offset.1);
    if lx < 0 || ly < 0 {
        return [0; 4];
    }
    l.pixels.sample_at(lx as u32, ly as u32).unwrap_or([0; 4])
}

fn inside_old(window: Window, i: u32, j: u32, canvas: (u32, u32)) -> Option<(u32, u32)> {
    let (ox, oy) = (window.0 + i as i32, window.1 + j as i32);
    (ox >= 0 && oy >= 0 && (ox as u32) < canvas.0 && (oy as u32) < canvas.1)
        .then_some((ox as u32, oy as u32))
}

#[test]
fn byte_each_side_pads_the_background_and_keeps_old_samples_exact() {
    let doc = byte_doc();
    let source = pattern(W, H, 9);
    let channel = byte_channel_plane();
    for window in WINDOWS {
        for delete in [true, false] {
            let request = crop(window, delete, Some(color()));
            let out = byte_must(&doc, &request);
            let out = if delete { trimmed(out) } else { out };
            assert_eq!((out.width, out.height), (window.2, window.3), "{request}");
            for j in 0..window.3 {
                for i in 0..window.2 {
                    let got = byte_at(&out, 0, i as i32, j as i32);
                    match inside_old(window, i, j, (W, H)) {
                        Some((ox, oy)) => {
                            assert_eq!(got, source.get_pixel(ox, oy).0, "{request} old ({i},{j})")
                        }
                        None if delete => assert_eq!(
                            got,
                            [RED[0], RED[1], RED[2], 255],
                            "{request} new area ({i},{j})"
                        ),
                        None => assert_eq!(got[3], 0, "{request}: Delete Cropped off pads nothing"),
                    }
                    let want = inside_old(window, i, j, (W, H))
                        .map_or(0, |(ox, oy)| channel.get_pixel(ox, oy).0[0]);
                    assert_eq!(
                        out.channels[0].data.get_pixel(i, j).0[0],
                        want,
                        "{request} channel ({i},{j})"
                    );
                }
            }
        }
    }
}

#[test]
fn byte_only_the_opaque_background_is_padded() {
    let window = (-3, -2, W + 7, H + 5);
    let request = crop(window, true, Some(color()));
    let new_area = (0, 0); // canvas point (0, 0) is the old (−3, −2)
                           // Background plus a normal full-canvas layer: only record 0 is padded.
    let two = byte_doc()
        .adding_image_layer(0, pattern(W, H, 77), "Over")
        .unwrap();
    let out = trimmed(byte_must(&two, &request));
    assert_eq!(
        byte_at(&out, 0, new_area.0, new_area.1),
        [RED[0], RED[1], RED[2], 255]
    );
    assert_eq!(
        byte_at(&out, 1, new_area.0, new_area.1)[3],
        0,
        "a normal layer stays transparent"
    );
    assert_eq!(out.layers[1].pixels.dimensions(), (W, H));
    // Each disqualified bottom layer: nothing is padded.
    let mut hidden = byte_doc();
    hidden.layers[0].visible = false;
    let mut offset = byte_doc();
    offset.layers[0].offset = (1, 0);
    let mut holed = byte_doc();
    let mut pixels = pattern(W, H, 9);
    pixels.put_pixel(5, 5, Rgba([9, 9, 9, 0]));
    holed.layers[0].pixels = doctored_core::layer_raster::LayerRaster::unseeded(pixels);
    for (what, doc) in [
        ("hidden", hidden),
        ("offset", offset),
        ("one transparent pixel", holed),
    ] {
        let out = trimmed(byte_must(&doc, &request));
        assert_eq!(
            byte_at(&out, 0, new_area.0, new_area.1)[3],
            0,
            "{what}: not a Background, no pad"
        );
    }
}

#[test]
fn byte_comps_follow_the_padded_origin_and_the_plan_charges_the_pad() {
    let mut doc = byte_doc();
    doc.layers[0].offset = (2, 1);
    let c =
        CString::new(json!({"op":"layer_comp","action":"save","name":"Shot"}).to_string()).unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    let saved = unsafe { dr_doc_edit_layer_comps(&doc, c.as_ptr(), &mut err) };
    assert!(!saved.is_null(), "{}", take_err_string(err));
    let mut doc = *unsafe { Box::from_raw(saved) };
    doc.layers[0].offset = (0, 0);
    let window = (-3, -2, W + 7, H + 5);
    let out = byte_must(&doc, &crop(window, true, Some(color())));
    let id = out.layers[0].id;
    let entry = out.layer_comps()[0]
        .entries
        .iter()
        .find(|e| e.layer_id == id)
        .and_then(|e| e.offset)
        .expect("a recorded offset");
    let at = out.layers[0].offset;
    assert_eq!(
        (entry.0 - at.0, entry.1 - at.1),
        (2, 1),
        "the comp keeps its placement relative to the padded layer"
    );
    let with = byte_plan_scratch(&doc, &crop(window, true, Some(color())));
    let without = byte_plan_scratch(&doc, &crop(window, true, None));
    assert!(
        with >= without + u64::from(window.2 * window.3) * 4,
        "the pad is charged: {with} against {without}"
    );
}

// ---------------------------------------------------- 16 bits, float, CMYK --

fn pixels16(w: u32, h: u32, seed: u16) -> Vec<[u16; 4]> {
    (0..w * h)
        .map(|i| {
            let v = seed.wrapping_add((i * 997) as u16);
            [v, v.wrapping_mul(3), 65535 - v, 65535]
        })
        .collect()
}

fn channel16() -> Vec<u16> {
    (0..W * H).map(|i| (1 + i * 523 % 65000) as u16).collect()
}

fn typed16(pixels: Vec<[u16; 4]>, extra: Vec<Layer>) -> PrecisionDocument {
    let image = PrecisionImage::from_rgba16(W, H, pixels, IccProfile::srgb(), None).unwrap();
    let mut layers = vec![Layer::raster("Background", image, (0, 0), 1.0, true).unwrap()];
    layers.extend(extra);
    let channel = PrecisionChannel::new(
        "Saved",
        PrecisionPlane::from_u16(W, H, channel16(), None).unwrap(),
    )
    .unwrap();
    PrecisionDocument::from_parts_and_channels(
        W,
        H,
        IccProfile::srgb(),
        Metadata::default(),
        Resolution::default(),
        layers,
        vec![channel],
    )
    .unwrap()
}

fn cmyk_doc() -> PrecisionDocument {
    let rgb = typed16(pixels16(W, H, 300), Vec::new());
    let profile = press_profile("Press Test");
    let mut err: *mut c_char = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_convert_to_cmyk(
            &rgb,
            profile.as_ptr(),
            profile.len(),
            c"{}".as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!out.is_null(), "to CMYK: {}", take_err_string(err));
    *unsafe { Box::from_raw(out) }
}

fn float_pixels() -> Vec<[f32; 4]> {
    (0..W * H)
        .map(|i| [i as f32 / 200.0, 0.25, 1.0 - i as f32 / 400.0, 1.0])
        .collect()
}

/// The sRGB decode, the float document's linear ink of a `document` colour.
fn srgb_linear(v: f64) -> f64 {
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}

/// Record 0 of a typed result as `(samples, offset, size)` in u32 codes or
/// f32 bits, whichever the storage holds, CMYK as its five codes.
enum Samples {
    U16(Vec<[u16; 4]>),
    F32(Vec<[f32; 4]>),
    Cmyk(Vec<[u16; 5]>),
}

fn record0(doc: &PrecisionDocument) -> (Samples, (i32, i32), (u32, u32)) {
    let layer = doc.layer(0).expect("record 0");
    if let Some(image) = layer.cmyk_source() {
        return (
            Samples::Cmyk(doc.cmyk_layer_samples(0).unwrap()),
            layer.offset(),
            image.dimensions(),
        );
    }
    let image = layer.source().expect("a raster");
    let samples = match image.to_rgba16() {
        Ok(s) => Samples::U16(s),
        Err(_) => Samples::F32(image.to_rgba32().unwrap()),
    };
    (samples, layer.offset(), image.dimensions())
}

#[test]
fn typed_each_side_pads_the_background_at_16_bits_float_and_cmyk() {
    let sixteen = typed16(pixels16(W, H, 100), Vec::new());
    let float = float_doc(W, H, &IccProfile::srgb(), &[float_pixels()]);
    let cmyk = cmyk_doc();
    let red = red_unit();
    let red16 = [
        u16::from(RED[0]) * 257,
        u16::from(RED[1]) * 257,
        u16::from(RED[2]) * 257,
        65535,
    ];
    let ink = INK.map(|v| (v * 65535.0).round() as u16);
    let (old16, _, _) = record0(&sixteen);
    let (old_float, _, _) = record0(&float);
    let (old_cmyk, _, _) = record0(&cmyk);
    for window in WINDOWS {
        for delete in [true, false] {
            for (what, doc, background) in [
                ("16", &sixteen, color()),
                ("float", &float, color()),
                ("cmyk", &cmyk, json!({"ink": INK})),
            ] {
                let request = crop(window, delete, Some(background));
                let out = typed_must(doc, &request);
                assert_eq!(out.dimensions(), (window.2, window.3), "{what} {request}");
                let (got, offset, size) = record0(&out);
                if delete {
                    assert_eq!((offset, size), ((0, 0), (window.2, window.3)), "{what}");
                } else {
                    assert_eq!((offset, size), ((-window.0, -window.1), (W, H)), "{what}");
                }
                for j in 0..window.3 {
                    for i in 0..window.2 {
                        let old = inside_old(window, i, j, (W, H));
                        let (lx, ly) = (i as i32 - offset.0, j as i32 - offset.1);
                        let held =
                            lx >= 0 && ly >= 0 && (lx as u32) < size.0 && (ly as u32) < size.1;
                        if !held {
                            assert!(!delete && old.is_none(), "{what} {request} ({i},{j})");
                            continue;
                        }
                        let k = ly as usize * size.0 as usize + lx as usize;
                        let o = old.map(|(ox, oy)| (oy * W + ox) as usize);
                        match (&got, &old16, &old_float, &old_cmyk) {
                            (Samples::U16(g), Samples::U16(src), _, _) => match o {
                                Some(o) => assert_eq!(g[k], src[o], "{what} old ({i},{j})"),
                                None => assert_eq!(g[k], red16, "{what} pad ({i},{j})"),
                            },
                            (Samples::F32(g), _, Samples::F32(src), _) => match o {
                                Some(o) => assert_eq!(
                                    g[k].map(f32::to_bits),
                                    src[o].map(f32::to_bits),
                                    "{what} old ({i},{j})"
                                ),
                                None => {
                                    for c in 0..3 {
                                        let want = srgb_linear(red[c]);
                                        assert!(
                                            (f64::from(g[k][c]) - want).abs() < 1e-3,
                                            "{what} pad channel {c}: {} against {want}",
                                            g[k][c]
                                        );
                                    }
                                    assert_eq!(g[k][3], 1.0);
                                }
                            },
                            (Samples::Cmyk(g), _, _, Samples::Cmyk(src)) => match o {
                                Some(o) => assert_eq!(g[k], src[o], "{what} old ({i},{j})"),
                                None => assert_eq!(
                                    g[k],
                                    [ink[0], ink[1], ink[2], ink[3], 65535],
                                    "{what} pad ({i},{j})"
                                ),
                            },
                            _ => panic!("{what}: storage changed"),
                        }
                    }
                }
            }
            // The 16-bit saved channel: exact inside, 0 in the new area.
            let out = typed_must(&sixteen, &crop(window, delete, Some(color())));
            let samples = out.channels()[0].plane().samples().to_vec();
            let source = channel16();
            for j in 0..window.3 {
                for i in 0..window.2 {
                    let want = inside_old(window, i, j, (W, H))
                        .map_or(0, |(ox, oy)| source[(oy * W + ox) as usize]);
                    assert_eq!(
                        samples[(j * window.2 + i) as usize],
                        want,
                        "channel ({i},{j})"
                    );
                }
            }
        }
    }
}

#[test]
fn typed_only_the_opaque_background_is_padded() {
    let window = (-3, -2, W + 7, H + 5);
    let request = crop(window, true, Some(color()));
    // A normal layer above: it is moved, never padded.
    let over =
        PrecisionImage::from_rgba16(W, H, pixels16(W, H, 5), IccProfile::srgb(), None).unwrap();
    let two = typed16(
        pixels16(W, H, 100),
        vec![Layer::raster("Over", over, (0, 0), 1.0, true).unwrap()],
    );
    let out = typed_must(&two, &request);
    let upper = out.layer(1).unwrap();
    assert_eq!(
        (upper.offset(), upper.source().unwrap().dimensions()),
        ((3, 2), (W, H))
    );
    assert_eq!(record0(&out).1, (0, 0));
    // One transparent pixel: no pad, so record 0 keeps only its old extent.
    let mut holed = pixels16(W, H, 100);
    holed[5 * W as usize + 5][3] = 0;
    let out = typed_must(&typed16(holed, Vec::new()), &request);
    assert_eq!((record0(&out).1, record0(&out).2), ((3, 2), (W, H)));
    // Hidden and offset bottom records: no pad.
    let hidden_image =
        PrecisionImage::from_rgba16(W, H, pixels16(W, H, 100), IccProfile::srgb(), None).unwrap();
    let hidden = PrecisionDocument::from_parts(
        W,
        H,
        IccProfile::srgb(),
        Metadata::default(),
        Resolution::default(),
        vec![Layer::raster("Hidden", hidden_image, (0, 0), 1.0, false).unwrap()],
    )
    .unwrap();
    let out = typed_must(&hidden, &request);
    assert_eq!(
        record0(&out).2,
        (W, H),
        "a hidden bottom record is not padded"
    );
    let moved_image =
        PrecisionImage::from_rgba16(W, H, pixels16(W, H, 100), IccProfile::srgb(), None).unwrap();
    let moved = PrecisionDocument::from_parts(
        W,
        H,
        IccProfile::srgb(),
        Metadata::default(),
        Resolution::default(),
        vec![Layer::raster("Moved", moved_image, (1, 0), 1.0, true).unwrap()],
    )
    .unwrap();
    let out = typed_must(&moved, &request);
    assert_eq!(
        (record0(&out).1, record0(&out).2),
        ((4, 2), (W, H)),
        "an offset bottom record is only moved"
    );
}

// ------------------------------------------------------------ turned crop --

/// A reference plane far larger than any pad: the colour everywhere the old
/// canvas is not, so the reference reads exactly what a padded plane holds.
fn reference_padded(source: &[[u32; 4]], fill: [u32; 4]) -> Plane {
    let margin = 40;
    let (pw, ph) = (W + 2 * margin, H + 2 * margin);
    let mut out = vec![fill; (pw * ph) as usize];
    for y in 0..H {
        for x in 0..W {
            out[((y + margin) * pw + x + margin) as usize] = source[(y * W + x) as usize];
        }
    }
    (out, (-(margin as i32), -(margin as i32)), (pw, ph))
}

#[test]
fn a_turned_extended_crop_pads_its_corners_and_resamples_its_edge_once() {
    let window = (-4, -3, W + 8, H + 6);
    let matrix = rotation(8.0, window);
    let corners = [
        (0, 0),
        (window.2 - 1, 0),
        (0, window.3 - 1),
        (window.2 - 1, window.3 - 1),
    ];
    // 8 bits.
    let doc = byte_doc();
    let corner_out = trimmed(byte_must(
        &doc,
        &straighten(matrix, window, "lanczos3", true, Some(color())),
    ));
    for (i, j) in corners {
        assert_eq!(
            byte_at(&corner_out, 0, i as i32, j as i32),
            [RED[0], RED[1], RED[2], 255],
            "8-bit corner ({i},{j})"
        );
    }
    let out = trimmed(byte_must(
        &doc,
        &straighten(matrix, window, "bicubic", true, Some(color())),
    ));
    let source: Vec<[u32; 4]> = pattern(W, H, 9)
        .pixels()
        .map(|p| p.0.map(u32::from))
        .collect();
    let fill = [u32::from(RED[0]), u32::from(RED[1]), u32::from(RED[2]), 255];
    let (padded, at, dims) = reference_padded(&source, fill);
    let dest = (window.0, window.1, window.2, window.3);
    let want = reference_rgba(&padded, dims, at, dest, invert(matrix), "bicubic", 255);
    let got: Vec<[u32; 4]> = (0..window.3)
        .flat_map(|j| (0..window.2).map(move |i| (i, j)))
        .map(|(i, j)| byte_at(&out, 0, i as i32, j as i32).map(u32::from))
        .collect();
    assert_rgba_within_one(&got, &want, "8-bit turned edge");
    let channel: Vec<u32> = byte_channel_plane()
        .pixels()
        .map(|p| u32::from(p.0[0]))
        .collect();
    let want = reference_plane(
        &channel,
        (W, H),
        (0, 0),
        dest,
        invert(matrix),
        "bicubic",
        0,
        255,
    );
    let got: Vec<u32> = out.channels[0]
        .data
        .pixels()
        .map(|p| u32::from(p.0[0]))
        .collect();
    assert_plane_within_one(&got, &want, "8-bit channel resampled once");

    // 16 bits.
    let doc = typed16(pixels16(W, H, 100), Vec::new());
    let corner_out = typed_must(
        &doc,
        &straighten(matrix, window, "lanczos3", true, Some(color())),
    );
    let red16 = [51400, 12850, 10280, 65535];
    let (Samples::U16(got), offset, size) = record0(&corner_out) else {
        panic!("16-bit storage")
    };
    assert_eq!((offset, size), ((0, 0), (window.2, window.3)));
    for (i, j) in corners {
        assert_eq!(
            got[(j * size.0 + i) as usize],
            red16,
            "16-bit corner ({i},{j})"
        );
    }
    let out = typed_must(
        &doc,
        &straighten(matrix, window, "bicubic", true, Some(color())),
    );
    let source: Vec<[u32; 4]> = pixels16(W, H, 100)
        .iter()
        .map(|p| p.map(u32::from))
        .collect();
    let (padded, at, dims) = reference_padded(&source, red16.map(u32::from));
    let want = reference_rgba(&padded, dims, at, dest, invert(matrix), "bicubic", 65535);
    let (Samples::U16(got), _, _) = record0(&out) else {
        panic!("16-bit storage")
    };
    let got: Vec<[u32; 4]> = got.iter().map(|p| p.map(u32::from)).collect();
    assert_rgba_within_one(&got, &want, "16-bit turned edge");
    let channel: Vec<u32> = channel16().iter().map(|&v| u32::from(v)).collect();
    let want = reference_plane(
        &channel,
        (W, H),
        (0, 0),
        dest,
        invert(matrix),
        "bicubic",
        0,
        65535,
    );
    let got: Vec<u32> = out.channels()[0]
        .plane()
        .samples()
        .iter()
        .map(|&v| u32::from(v))
        .collect();
    assert_plane_within_one(&got, &want, "16-bit channel resampled once");
}

// --------------------------------------------------------------- refusals --

#[test]
fn every_refusal_states_its_own_sentence() {
    let big = DrDocument::from_pixels(pattern(64, 48, 3));
    let pad = "the crop reaches too far past the canvas for the Background to grow: choose a window nearer the canvas";
    let far = crop((-20000, -20000, 100, 100), true, Some(color()));
    assert_eq!(byte_try(&big, &far).err().as_deref(), Some(pad));
    let image =
        PrecisionImage::from_rgba16(64, 48, pixels16(64, 48, 1), IccProfile::srgb(), None).unwrap();
    let typed = PrecisionDocument::from_parts(
        64,
        48,
        IccProfile::srgb(),
        Metadata::default(),
        Resolution::default(),
        vec![Layer::raster("Background", image, (0, 0), 1.0, true).unwrap()],
    )
    .unwrap();
    assert_eq!(typed_try(&typed, &far).err().as_deref(), Some(pad));

    let range = "the crop window's edges must lie within the signed 32-bit range";
    let low = crop((i32::MIN, 0, 10, 10), true, None);
    assert_eq!(byte_try(&big, &low).err().as_deref(), Some(range));
    assert_eq!(typed_try(&typed, &low).err().as_deref(), Some(range));
    let high = crop((i32::MAX - 5, 0, 10, 10), true, Some(color()));
    assert_eq!(byte_try(&big, &high).err().as_deref(), Some(range));

    let limit = "precision prototype dimensions exceed the supported pixel limit";
    let huge = crop((-10, -10, 20_000, 6_000), true, None);
    assert_eq!(byte_try(&big, &huge).err().as_deref(), Some(limit));
    assert_eq!(typed_try(&typed, &huge).err().as_deref(), Some(limit));

    let channels = "the saved alpha channels do not fit the extended canvas: delete some first";
    let mut many = big.clone();
    let plane = GrayImage::from_pixel(64, 48, Luma([7]));
    for i in 0..10 {
        many = many
            .add_channel(&format!("C{i}"), plane.as_raw(), 64, 48, [255, 0, 0], 0.5)
            .unwrap();
    }
    let wide = crop((-5000, -5000, 10_000, 10_000), false, None);
    assert_eq!(byte_try(&many, &wide).err().as_deref(), Some(channels));

    let shape = "crop background takes color on an RGB document and ink on a CMYK one";
    let inked = crop((-2, 0, 66, 48), true, Some(json!({"ink": INK})));
    assert_eq!(byte_try(&big, &inked).err().as_deref(), Some(shape));
    assert_eq!(typed_try(&typed, &inked).err().as_deref(), Some(shape));
    let coloured = crop((-2, 0, W + 2, H), true, Some(color()));
    assert_eq!(
        typed_try(&cmyk_doc(), &coloured).err().as_deref(),
        Some(shape)
    );

    let srgb = crop(
        (-2, 0, 66, 48),
        true,
        Some(json!({"color": red_unit(), "color_space": "srgb"})),
    );
    assert_eq!(
        byte_try(&big, &srgb).err().as_deref(),
        Some("an 8-bit crop background takes color_space document")
    );
    let linear = crop(
        (-2, 0, 66, 48),
        true,
        Some(json!({"color": red_unit(), "color_space": "linear"})),
    );
    assert_eq!(
        typed_try(&typed, &linear).err().as_deref(),
        Some("linear ink is a 32-bit float document's paint and gradient colour")
    );
}
