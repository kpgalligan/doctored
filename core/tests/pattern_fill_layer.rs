//! Pattern fill layers: the plane-less `pattern` artwork
//! component through `dr_artwork_render` / `dr_artwork_render_float` at 8, 16
//! and 32 bits, its placement (origin O = phase + the layer's reference when
//! linked, the host writing `anchor` = −reference; phase when unlinked), its
//! refusals, its parity with a Pattern Overlay, and the reference getter
//! (`dr_doc_layer_reference_x` / `_y`, the typed info row's `"reference"`);
//! and `dr_turn_pattern`, the turned tile a canvas quarter
//! turn or flip gives a linked fill, against closed-form turns and a linked
//! Pattern Overlay's own turn.
//!
//! Oracles are closed forms written here, never read off the implementation:
//! at scale 1 and angle 0 with an integer origin, pixel (x, y) is tile[(x −
//! Ox) mod w, (y − Oy) mod h]; at scale 0.5 on an even tile it is the
//! periodic 2 × 2 box mean of the tile (`pattern_tile`'s PYRAMID level one,
//! residual scale 1). Tile codes are document values: code / 255 at 8 and 16
//! bits, and linear light code / 255 at 32 bits.
use std::ffi::{c_char, CStr, CString};
use std::ptr;

use doctored_core::doc::DrDocument;
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_artwork::{dr_artwork_render, dr_artwork_render_float, dr_doc_artwork};
use doctored_core::ffi_doc::{dr_doc_flip_horizontal, dr_doc_rotate90};
use doctored_core::ffi_group::dr_doc_move_layers;
use doctored_core::ffi_layer_reference::{
    dr_doc_layer_reference_x, dr_doc_layer_reference_y, dr_turn_pattern,
};
use doctored_core::ffi_precision::dr_precision_doc_info;
use doctored_core::ffi_precision_edit::dr_precision_doc_edit;
use doctored_core::ffi_style::dr_doc_layer_style;
use doctored_core::icc::IccProfile;
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::{PrecisionImage, PrecisionStorage, REFERENCE_WORKING_BYTES};
use image::RgbaImage;
use serde_json::{json, Value};

use crate::common::{layer_pixels, solid, styled};

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
const W: u32 = 13;
const H: u32 = 7;

/// A tile: `(w, h, straight RGBA8)`.
type Tile = (u32, u32, Vec<u8>);

fn tile_of(w: u32, h: u32, f: impl Fn(u32, u32) -> [u8; 4]) -> Tile {
    let mut rgba = Vec::new();
    for y in 0..h {
        for x in 0..w {
            rgba.extend_from_slice(&f(x, y));
        }
    }
    (w, h, rgba)
}

/// An asymmetric opaque 4 × 3 tile: every texel distinct.
fn tile_4x3() -> Tile {
    tile_of(4, 3, |x, y| {
        [
            (40 * x + 20) as u8,
            (60 * y + 30) as u8,
            ((3 * x + 5 * y) * 10 + 5) as u8,
            255,
        ]
    })
}

/// An opaque 4 × 2 tile for the half-scale box.
fn tile_4x2() -> Tile {
    tile_of(4, 2, |x, y| {
        [
            (50 * x + 10) as u8,
            (100 * y + 40) as u8,
            (200 - 30 * x - 70 * y) as u8,
            255,
        ]
    })
}

/// Standard base64 (RFC 4648, padded), written here so the test does not
/// borrow the core's encoder.
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

fn custom(tile: &Tile) -> Value {
    json!({"name": "T", "width": tile.0, "height": tile.1, "rgba": base64(&tile.2)})
}

/// The pattern fill keys: linked layers carry `anchor` = −reference.
fn fill(tile: &Tile, scale: f64, phase: (i32, i32), link: Option<(i32, i32)>) -> Value {
    let mut f = json!({"pattern": custom(tile), "angle": 0, "scale": scale,
        "link_with_layer": link.is_some(), "phase": [phase.0, phase.1]});
    if let Some(r) = link {
        f["anchor"] = json!([-r.0, -r.1]);
    }
    f
}

fn component(fill: &Value) -> Value {
    json!([{"kind": "pattern", "fill": fill}])
}

fn owned(p: *mut c_char) -> Option<String> {
    if p.is_null() {
        return None;
    }
    let text = unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned();
    unsafe { dr_string_free(p) };
    Some(text)
}

fn render(float: bool, components: &Value) -> Result<PrecisionImage, String> {
    let description = CString::new(components.to_string()).unwrap();
    let profile = IccProfile::srgb();
    let call = if float {
        dr_artwork_render_float
    } else {
        dr_artwork_render
    };
    let mut err = ptr::null_mut();
    let out = unsafe {
        call(
            W,
            H,
            description.as_ptr(),
            ptr::null(),
            0,
            ptr::null(),
            0,
            profile.bytes().as_ptr(),
            profile.bytes().len(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    match owned(err) {
        Some(message) => {
            assert!(out.is_null());
            Err(message)
        }
        None => {
            assert!(!out.is_null());
            Ok(*unsafe { Box::from_raw(out) })
        }
    }
}

/// Tile texel at canvas pixel (x, y) with the tile's origin at `o`.
fn texel(tile: &Tile, o: (i32, i32), x: u32, y: u32) -> [u8; 4] {
    let i = (x as i32 - o.0).rem_euclid(tile.0 as i32) as usize;
    let j = (y as i32 - o.1).rem_euclid(tile.1 as i32) as usize;
    let at = (j * tile.0 as usize + i) * 4;
    tile.2[at..at + 4].try_into().unwrap()
}

/// `image` published as a new byte layer above layer 0 of a white document:
/// the 8-bit layer's pixels.
fn published8(image: &PrecisionImage) -> Vec<u8> {
    let doc = DrDocument::from_pixels(solid(W, H, [255; 4]));
    let name = CString::new("Pattern Fill").unwrap();
    let text = CString::new(r#"{"type":"fill","version":1,"fill":"pattern"}"#).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_doc_artwork(
            &doc,
            0,
            false,
            image,
            0,
            0,
            name.as_ptr(),
            text.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert_eq!(owned(err), None);
    assert!(!out.is_null());
    let published = unsafe { Box::from_raw(out) };
    layer_pixels(&*published, 1)
}

/// The three placements: linked at reference (5, −3), unlinked with the same
/// reference (which it ignores), and linked at reference (0, 0).
/// `(the linked layer's reference, or None when unlinked; phase)`.
type Placement = (Option<(i32, i32)>, (i32, i32));

const PLACEMENTS: [Placement; 3] = [
    (Some((5, -3)), (1, 2)),
    (None, (1, 2)),
    (Some((0, 0)), (-2, 4)),
];

fn origin(link: Option<(i32, i32)>, phase: (i32, i32)) -> (i32, i32) {
    match link {
        Some(r) => (phase.0 + r.0, phase.1 + r.1),
        None => phase,
    }
}

#[test]
fn scale_one_tiles_from_phase_plus_reference_at_every_depth() {
    let tile = tile_4x3();
    for (link, phase) in PLACEMENTS {
        let o = origin(link, phase);
        let what = format!("link {link:?} phase {phase:?}");
        let components = component(&fill(&tile, 1.0, phase, link));
        // 16 bits: code × 257.
        let image = render(false, &components).expect("a 16-bit pattern fill");
        assert_eq!(image.storage(), PrecisionStorage::Rgba16Unorm);
        let pixels = image.to_rgba16().ok().unwrap();
        for y in 0..H {
            for x in 0..W {
                let want = texel(&tile, o, x, y).map(|c| u16::from(c) * 257);
                let got = pixels[(y * W + x) as usize];
                for c in 0..4 {
                    assert!(
                        got[c].abs_diff(want[c]) <= 1,
                        "{what} ({x},{y}): {got:?} against {want:?}"
                    );
                }
            }
        }
        // 8 bits: the published layer holds the codes themselves.
        let bytes = published8(&image);
        for y in 0..H {
            for x in 0..W {
                let at = ((y * W + x) * 4) as usize;
                assert_eq!(
                    bytes[at..at + 4],
                    texel(&tile, o, x, y),
                    "{what} 8-bit ({x},{y})"
                );
            }
        }
        // 32 bits: code / 255 as linear light, verbatim.
        let image = render(true, &components).expect("a float pattern fill");
        assert_eq!(image.storage(), PrecisionStorage::Rgba32Float);
        let pixels = image.to_rgba32().expect("float samples");
        for y in 0..H {
            for x in 0..W {
                let want = texel(&tile, o, x, y).map(|c| f64::from(c) / 255.0);
                let got = pixels[(y * W + x) as usize];
                for c in 0..4 {
                    assert!(
                        (f64::from(got[c]) - want[c]).abs() <= 1e-6,
                        "{what} float ({x},{y}): {got:?} against {want:?}"
                    );
                }
            }
        }
    }
}

/// The periodic 2 × 2 box mean of an even opaque tile at level index (i, 0):
/// the mean of texels (2i, 0), (2i + 1, 0), (2i, 1), (2i + 1, 1), unit.
fn box_mean(tile: &Tile, i: usize) -> [f64; 3] {
    let mut acc = [0.0; 3];
    for (x, y) in [(2 * i, 0), (2 * i + 1, 0), (2 * i, 1), (2 * i + 1, 1)] {
        let at = (y * tile.0 as usize + x % tile.0 as usize) * 4;
        for (c, slot) in acc.iter_mut().enumerate() {
            *slot += f64::from(tile.2[at + c]) / 255.0 / 4.0;
        }
    }
    acc
}

#[test]
fn half_scale_reads_the_periodic_box_mean() {
    let tile = tile_4x2();
    for (link, phase) in PLACEMENTS {
        let o = origin(link, phase);
        let components = component(&fill(&tile, 0.5, phase, link));
        let sixteen = render(false, &components)
            .unwrap()
            .to_rgba16()
            .ok()
            .unwrap();
        let float = render(true, &components).unwrap().to_rgba32().unwrap();
        for y in 0..H {
            for x in 0..W {
                // Level one is 2 × 1 at residual scale 1.
                let i = (x as i32 - o.0).rem_euclid(2) as usize;
                let want = box_mean(&tile, i);
                let at = (y * W + x) as usize;
                for c in 0..3 {
                    assert!(
                        (f64::from(sixteen[at][c]) - want[c] * 65535.0).abs() <= 1.0,
                        "16-bit ({x},{y}) channel {c}: {} against {}",
                        sixteen[at][c],
                        want[c] * 65535.0
                    );
                    assert!(
                        (f64::from(float[at][c]) - want[c]).abs() <= 1e-6,
                        "float ({x},{y}) channel {c}"
                    );
                }
                assert_eq!(sixteen[at][3], 65535);
                assert!((float[at][3] - 1.0).abs() <= 1e-6);
            }
        }
    }
}

#[test]
fn a_preset_pattern_fill_renders_its_frozen_tile() {
    // Checkerboard: 16 × 16, white where x/8 + y/8 is even, else #c0c0c0.
    let fill = json!({"pattern": {"preset": "checkerboard"}, "phase": [3, 0],
        "link_with_layer": false});
    let image = render(false, &component(&fill)).unwrap();
    let bytes = published8(&image);
    for y in 0..H {
        for x in 0..W {
            let u = (x as i32 - 3).rem_euclid(16) as u32;
            let want = if (u / 8 + y / 8).is_multiple_of(2) {
                0xff
            } else {
                0xc0
            };
            let at = ((y * W + x) * 4) as usize;
            assert_eq!(bytes[at..at + 4], [want, want, want, 255], "({x},{y})");
        }
    }
}

#[test]
fn malformed_pattern_components_are_refused_by_name() {
    let tile = tile_4x3();
    let good = fill(&tile, 1.0, (0, 0), Some((0, 0)));
    let cases: Vec<(Value, &str)> = vec![
        (
            json!([{"kind": "pattern", "fill": 7}]),
            "fill must be an object",
        ),
        (
            component(&json!({"pattern": {"preset": "plaid"}})),
            "fill.pattern.preset must be one of",
        ),
        (
            component(&json!({"pattern": {"width": 4, "height": 3, "rgba": "AAAA"}})),
            "fill.pattern.rgba must be base64",
        ),
        (
            component(&json!({"pattern": custom(&tile), "scale": "big"})),
            "fill.scale must be a number",
        ),
        (
            json!([{"kind": "pattern", "fill": good, "color": [0, 0, 0, 1]}]),
            "unknown field",
        ),
    ];
    for (components, needle) in cases {
        let message = render(false, &components).err().expect("a refusal");
        assert!(message.contains(needle), "{components}: {message}");
    }
    // A pattern takes no sample plane: one supplied is a layout error.
    let description = CString::new(component(&good).to_string()).unwrap();
    let profile = IccProfile::srgb();
    let samples = vec![65535u16; (W * H) as usize];
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_artwork_render(
            W,
            H,
            description.as_ptr(),
            samples.as_ptr(),
            samples.len(),
            ptr::null(),
            0,
            profile.bytes().as_ptr(),
            profile.bytes().len(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(out.is_null());
    assert!(owned(err).is_some());
}

/// A canvas-sized opaque raster carrying a Pattern Overlay holding the same
/// opaque tile and the same explicit anchor renders the fill's pixels.
#[test]
fn a_pattern_fill_matches_a_pattern_overlay_on_an_opaque_raster() {
    let tile = tile_4x3();
    for (link, phase) in PLACEMENTS {
        let f = fill(&tile, 1.0, phase, link);
        let fill_pixels = published8(&render(false, &component(&f)).unwrap());
        let mut effect = f.clone();
        effect["type"] = json!("pattern_overlay");
        let style = json!({"effects": [effect]}).to_string();
        let base = DrDocument::from_pixels(solid(W, H, [255; 4]))
            .adding_image_layer(0, solid(W, H, [0, 0, 0, 255]), "Base")
            .unwrap();
        let overlay: RgbaImage = styled(&base, 1, &style).flattened().unwrap();
        assert_eq!(
            overlay.as_raw(),
            &fill_pixels,
            "link {link:?} phase {phase:?}"
        );
    }
}

fn reference(doc: &DrDocument, idx: usize) -> (i32, i32) {
    unsafe {
        (
            dr_doc_layer_reference_x(doc, idx),
            dr_doc_layer_reference_y(doc, idx),
        )
    }
}

#[test]
fn the_reference_getter_follows_a_move_and_the_typed_row_reports_it() {
    let doc = DrDocument::from_pixels(solid(W, H, [255; 4]))
        .adding_image_layer(0, solid(4, 3, [9, 9, 9, 255]), "Moved")
        .unwrap();
    assert_eq!(reference(&doc, 1), (0, 0));
    let idx = [1usize];
    let moved = unsafe { dr_doc_move_layers(&doc, idx.as_ptr(), 1, 7, -4) };
    assert!(!moved.is_null());
    let moved = unsafe { Box::from_raw(moved) };
    assert_eq!(reference(&moved, 1), (7, -4), "moved by the delta");
    let again = unsafe { dr_doc_move_layers(&*moved, idx.as_ptr(), 1, -2, 6) };
    assert!(!again.is_null());
    let again = unsafe { Box::from_raw(again) };
    assert_eq!(reference(&again, 1), (5, 2));
    // Out of range answers 0.
    assert_eq!(reference(&again, 9), (0, 0));

    // Typed: the info row's "reference", (0, 0) when made, then the Canvas
    // Size translation (`doc_reference`'s map).
    let typed = PrecisionDocument::from_image(
        PrecisionImage::from_rgba16(
            W,
            H,
            vec![[1000, 2000, 3000, 65535]; (W * H) as usize],
            IccProfile::srgb(),
            None,
        )
        .unwrap(),
        Default::default(),
        Default::default(),
    )
    .unwrap();
    assert_eq!(typed_reference(&typed, 0), json!([0, 0]));
    let request = CString::new(
        json!({"op":"canvas_size","width":W + 6,"height":H + 4,"x":3,"y":2}).to_string(),
    )
    .unwrap();
    let mut err = ptr::null_mut();
    let grown = unsafe {
        dr_precision_doc_edit(
            &typed,
            request.as_ptr(),
            ptr::null(),
            0,
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!grown.is_null(), "{:?}", owned(err));
    let grown = unsafe { Box::from_raw(grown) };
    assert_eq!(typed_reference(&grown, 0), json!([3, 2]));
}

fn typed_reference(doc: &PrecisionDocument, idx: usize) -> Value {
    let mut err = ptr::null_mut();
    let info = owned(unsafe { dr_precision_doc_info(doc, &mut err) }).expect("info");
    let info: Value = serde_json::from_str(&info).unwrap();
    info["layers"][idx]["reference"].clone()
}

#[test]
fn the_reference_getters_tolerate_null() {
    unsafe {
        assert_eq!(dr_doc_layer_reference_x(ptr::null(), 0), 0);
        assert_eq!(dr_doc_layer_reference_y(ptr::null(), 0), 0);
    }
}

// ---- the turned tile ----

/// `dr_turn_pattern`'s answer for `pattern` under (a, b, c, d): `Ok(None)`
/// for NULL with no error.
fn turned(pattern: &Value, linear: [f64; 4]) -> Result<Option<Value>, String> {
    let text = CString::new(pattern.to_string()).unwrap();
    let mut err = ptr::null_mut();
    let [a, b, c, d] = linear;
    let out = unsafe { dr_turn_pattern(text.as_ptr(), a, b, c, d, &mut err) };
    if let Some(message) = owned(err) {
        assert!(out.is_null());
        return Err(message);
    }
    Ok(owned(out).map(|json| serde_json::from_str(&json).unwrap()))
}

/// The closed-form turned tile: texel (i, j) of the result read from the
/// source at `from(i, j)`, the result `w' × h'`.
fn turned_tile(tile: &Tile, size: (u32, u32), from: impl Fn(u32, u32) -> (u32, u32)) -> Tile {
    tile_of(size.0, size.1, |i, j| {
        let (x, y) = from(i, j);
        let at = ((y * tile.0 + x) * 4) as usize;
        tile.2[at..at + 4].try_into().unwrap()
    })
}

/// The five exact forms, each with its closed-form turn of a `w × h` tile.
#[allow(clippy::type_complexity)]
fn forms(w: u32, h: u32) -> Vec<([f64; 4], (u32, u32), Box<dyn Fn(u32, u32) -> (u32, u32)>)> {
    vec![
        // R90 (x, y) -> (−y, x): T'(i, j) = T(j, h − 1 − i).
        (
            [0.0, 1.0, -1.0, 0.0],
            (h, w),
            Box::new(move |i, j| (j, h - 1 - i)),
        ),
        // R180: T'(i, j) = T(w − 1 − i, h − 1 − j).
        (
            [-1.0, 0.0, 0.0, -1.0],
            (w, h),
            Box::new(move |i, j| (w - 1 - i, h - 1 - j)),
        ),
        // R270 (x, y) -> (y, −x): T'(i, j) = T(w − 1 − j, i).
        (
            [0.0, -1.0, 1.0, 0.0],
            (h, w),
            Box::new(move |i, j| (w - 1 - j, i)),
        ),
        // FlipH: T'(i, j) = T(w − 1 − i, j).
        (
            [-1.0, 0.0, 0.0, 1.0],
            (w, h),
            Box::new(move |i, j| (w - 1 - i, j)),
        ),
        // FlipV: T'(i, j) = T(i, h − 1 − j).
        (
            [1.0, 0.0, 0.0, -1.0],
            (w, h),
            Box::new(move |i, j| (i, h - 1 - j)),
        ),
    ]
}

#[test]
fn turn_pattern_turns_a_custom_tile_by_each_exact_form() {
    let tile = tile_4x3();
    for (linear, size, from) in forms(tile.0, tile.1) {
        let want = turned_tile(&tile, size, from);
        assert_eq!(
            turned(&custom(&tile), linear).unwrap(),
            Some(custom(&want)),
            "{linear:?}"
        );
    }
}

#[test]
fn turn_pattern_answers_a_preset_as_a_custom_tile_named_after_it() {
    // Checkerboard: 16 × 16, white where x/8 + y/8 is even, else #c0c0c0.
    let checker = tile_of(16, 16, |x, y| {
        let v = if (x / 8 + y / 8).is_multiple_of(2) {
            0xff
        } else {
            0xc0
        };
        [v, v, v, 255]
    });
    let (linear, size, from) = forms(16, 16).remove(0);
    let want = turned_tile(&checker, size, from);
    let got = turned(&json!({"preset": "checkerboard"}), linear)
        .unwrap()
        .expect("a turn");
    assert_eq!(
        got,
        json!({"name": "Checkerboard", "width": 16, "height": 16, "rgba": base64(&want.2)})
    );
}

#[test]
fn turn_pattern_keeps_the_tile_off_the_exact_forms_and_refuses_bad_input() {
    let pattern = custom(&tile_4x3());
    let (c30, s30) = (30f64.to_radians().cos(), 30f64.to_radians().sin());
    let (c80, s80) = (80f64.to_radians().cos(), 80f64.to_radians().sin());
    for linear in [
        [1.0, 0.0, 0.0, 1.0],
        [0.0, 1.0, 1.0, 0.0],
        [c30, s30, -s30, c30],
        // Near R90: rounding would make it one; the exact match does not.
        [c80, s80, -s80, c80],
        [2.0, 0.0, 0.0, 2.0],
    ] {
        assert_eq!(turned(&pattern, linear), Ok(None), "{linear:?}");
    }
    let r90 = [0.0, 1.0, -1.0, 0.0];
    for bad in [
        json!({"preset": "plaid"}),
        json!({"width": 4, "height": 3, "rgba": "AAAA"}),
        json!(7),
    ] {
        let message = turned(&bad, r90).expect_err("a refusal");
        assert!(message.contains("pattern"), "{bad}: {message}");
    }
    let text = CString::new("{not json").unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe { dr_turn_pattern(text.as_ptr(), 0.0, 1.0, -1.0, 0.0, &mut err) };
    assert!(out.is_null());
    assert!(owned(err).is_some());
    let mut err = ptr::null_mut();
    let out = unsafe { dr_turn_pattern(ptr::null(), 0.0, 1.0, -1.0, 0.0, &mut err) };
    assert!(out.is_null());
    assert!(owned(err).is_some());
}

/// The oracle is the linked Pattern Overlay's own turn
/// (RENDER-TURNED): after Rotate 90° and then Flip Horizontal of the canvas,
/// its stored tile is what `dr_turn_pattern` answers step by step.
#[test]
fn turn_pattern_matches_a_linked_pattern_overlays_turn() {
    let tile = tile_4x3();
    let style = json!({"effects": [{"type": "pattern_overlay", "pattern": custom(&tile),
        "link_with_layer": true}]})
    .to_string();
    let base = DrDocument::from_pixels(solid(W, H, [255; 4]))
        .adding_image_layer(0, solid(W, H, [0, 0, 0, 255]), "Base")
        .unwrap();
    let doc = styled(&base, 1, &style);
    let overlay_pattern = |doc: &DrDocument| -> Value {
        let json = owned(unsafe { dr_doc_layer_style(doc, 1) }).expect("a style");
        let style: Value = serde_json::from_str(&json).unwrap();
        style["effects"][0]["pattern"].clone()
    };
    let rotated = unsafe { dr_doc_rotate90(&doc) };
    assert!(!rotated.is_null());
    let rotated = unsafe { Box::from_raw(rotated) };
    let after_turn = turned(&custom(&tile), [0.0, 1.0, -1.0, 0.0])
        .unwrap()
        .expect("a turn");
    assert_eq!(overlay_pattern(&rotated), after_turn);
    let flipped = unsafe { dr_doc_flip_horizontal(&*rotated) };
    assert!(!flipped.is_null());
    let flipped = unsafe { Box::from_raw(flipped) };
    let after_flip = turned(&after_turn, [-1.0, 0.0, 0.0, 1.0])
        .unwrap()
        .expect("a flip");
    assert_eq!(overlay_pattern(&flipped), after_flip);
}
