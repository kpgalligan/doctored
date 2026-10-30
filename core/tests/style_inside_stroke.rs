//! An Inside Stroke, and a Center Stroke's inner half, REPLACES the
//! layer under its band — it composites over the stack beneath the layer, not
//! over the layer's own pixels — at 8 bits (`dr_doc_set_layer_style` +
//! `dr_doc_flattened`), 16 bits and 32 bits (`dr_precision_doc_edit`
//! `layer_style`).
//!
//! The oracle is written here from Photoshop 26.10.0's measured statement
//! (`scripts/photoshop-probe/float_styles.py` take5,
//! `hard_normal_inside_beneath_{16,32}`; the members and their fit are
//! `scripts/photoshop-probe/style_v1x_take.py`'s), never read off the
//! implementation: with `B0` the composite beneath the layer (after its Below
//! effects; under a knockout, the backdrop the layer punches to), `X` the
//! pixel the layer and the interiors ranked before the band make, `a` the
//! layer's shape and `b` the band's coverage (the closed form below, `b <=
//! a`), the pixel is the premultiplied `lerp(X, over(B0, stroke, a · opacity),
//! b / a)`. Every pixel here is opaque (the backdrop is), so the mix is the
//! plain one. The band is `a − erode(a, size)`: `255 − round(255 ·
//! clamp(d − size, 0, 1))` for a rect, `d` the axis distance to its
//! complement. On a shape with a partial pixel (the alpha-128 rim) it is
//! measured from the anti-aliased edge (NA1, `inside_corner_band`, #2, the
//! Stroke confirm take), which along this straight rim is the larger of that
//! and `round(255 · clamp(size − (|p − q| − u_q), 0, 1))` over the partial
//! pixels q, `u_q = 1 − a_q`, bounded by `a`.

use std::ffi::{c_char, CStr, CString};
use std::ptr;

use doctored_core::doc::{BlendMode, DrDocument};
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_precision_edit::dr_precision_doc_edit;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer as Layer};
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use image::RgbaImage;
use serde_json::{json, Value};

use crate::common;
use crate::float_fixture::float_doc;
use common::{flat_pixels, styled};

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
const CANVAS: (u32, u32) = (40, 32);
const RECT: (i32, i32, u32, u32) = (10, 8, 14, 12);
const LEFT: [u8; 3] = [40, 60, 200];
const RIGHT: [u8; 3] = [230, 200, 40];
const SOURCE: [u8; 3] = [200, 40, 90];
const STROKE: [u8; 3] = [30, 220, 160];

// ------------------------------------------------------------ the oracle --

fn unit(c: [u8; 3]) -> [f64; 3] {
    c.map(|v| f64::from(v) / 255.0)
}

/// The W3C source-over of `cs` at `sa` onto an OPAQUE `cb` (Normal or
/// Multiply).
fn over(cb: [f64; 3], cs: [f64; 3], sa: f64, multiply: bool) -> [f64; 3] {
    std::array::from_fn(|c| {
        let b = if multiply { cb[c] * cs[c] } else { cs[c] };
        cb[c] + sa * (b - cb[c])
    })
}

fn lerp(x: [f64; 3], y: [f64; 3], s: f64) -> [f64; 3] {
    std::array::from_fn(|c| x[c] + (y[c] - x[c]) * s)
}

fn byte(v: [f64; 3]) -> [u8; 4] {
    let q = |x: f64| (x * 255.0).round().clamp(0.0, 255.0) as u8;
    [q(v[0]), q(v[1]), q(v[2]), 255]
}

/// The inside band of `rect` at `(x, y)` for `size` px, as a coverage in
/// [0, 1]: `1 − clamp(d − size, 0, 1)` with `d` the axis distance to the
/// rect's complement; 0 outside the rect.
fn inside_band(rect: (i32, i32, u32, u32), size: f64, x: i64, y: i64) -> f64 {
    let (x0, y0) = (i64::from(rect.0), i64::from(rect.1));
    let (x1, y1) = (x0 + i64::from(rect.2), y0 + i64::from(rect.3));
    if x < x0 || y < y0 || x >= x1 || y >= y1 {
        return 0.0;
    }
    let d = (x - x0 + 1).min(x1 - x).min(y - y0 + 1).min(y1 - y) as f64;
    let shrunk = (((d - size).clamp(0.0, 1.0)) * 255.0).round() / 255.0;
    1.0 - shrunk
}

/// [`inside_band`] of a rect whose LEFT column is at `rim` / 255, measured
/// from the anti-aliased contour (module doc): the nearest partial pixel of
/// `(x, y)` is the rim pixel of its row, `x − x0` away.
fn aa_rim_band(rect: (i32, i32, u32, u32), size: f64, rim: f64, x: i64, y: i64) -> f64 {
    if !in_rect(rect, x, y) {
        return 0.0;
    }
    let full = inside_band(rect, size, x, y);
    let u = 1.0 - rim / 255.0;
    let d = (x - i64::from(rect.0)) as f64 - u;
    let part = ((size - d).clamp(0.0, 1.0) * 255.0).round() / 255.0;
    full.max(part)
}

fn in_rect(rect: (i32, i32, u32, u32), x: i64, y: i64) -> bool {
    inside_band(rect, 1e9, x, y) > 0.0
}

fn backdrop(x: u32) -> [u8; 3] {
    if x < CANVAS.0 / 2 {
        LEFT
    } else {
        RIGHT
    }
}

fn striped(w: u32, h: u32) -> RgbaImage {
    RgbaImage::from_fn(w, h, |x, _| {
        let c = backdrop(x);
        image::Rgba([c[0], c[1], c[2], 255])
    })
}

fn rect_over_stripes(color: [u8; 3]) -> DrDocument {
    let doc = DrDocument::from_pixels(striped(CANVAS.0, CANVAS.1));
    let rect = RgbaImage::from_pixel(
        RECT.2,
        RECT.3,
        image::Rgba([color[0], color[1], color[2], 255]),
    );
    doc.adding_image_layer(0, rect, "Rect")
        .and_then(|d| d.with_layer_offset(1, RECT.0, RECT.1))
        .expect("the rect layer")
}

fn flat(doc: &DrDocument) -> Vec<u8> {
    let handle = Box::into_raw(Box::new(doc.clone()));
    let out = flat_pixels(handle);
    unsafe { doctored_core::ffi_doc::dr_doc_free(handle) };
    out
}

fn at(img: &[u8], x: u32, y: u32) -> [u8; 4] {
    common::pixel(img, CANVAS.0, x, y)
}

fn stroke_json(position: &str, size: u32, opacity: f64, blend: &str, extra: &str) -> String {
    format!(
        "{{{extra}\"effects\":[{{\"type\":\"stroke\",\"position\":\"{position}\",\
         \"size\":{size},\"color\":\"#1edca0\",\"opacity\":{opacity},\"blend\":\"{blend}\"}}]}}"
    )
}

/// Every pixel of a flattened `doc` against `want(x, y)`, ±1 per channel.
fn assert_every_pixel(img: &[u8], want: impl Fn(u32, u32) -> [u8; 4], what: &str) {
    let mut band_pixels = 0;
    for y in 0..CANVAS.1 {
        for x in 0..CANVAS.0 {
            let (got, want) = (at(img, x, y), want(x, y));
            if inside_band(RECT, 2.0, i64::from(x), i64::from(y)) > 0.0 {
                band_pixels += 1;
            }
            for c in 0..4 {
                assert!(
                    (i32::from(got[c]) - i32::from(want[c])).abs() <= 1,
                    "{what} ({x}, {y}): got {got:?}, want {want:?}"
                );
            }
        }
    }
    assert!(band_pixels > 0, "{what}: the band covers no pixel");
}

// ---------------------------------------------------------- 8 bits, FFI --

#[test]
fn a_hard_inside_band_shows_the_stack_beneath_the_layer() {
    for (blend, multiply) in [("normal", false), ("multiply", true)] {
        let doc = styled(
            &rect_over_stripes(SOURCE),
            1,
            &stroke_json("inside", 2, 0.5, blend, ""),
        );
        let img = flat(&doc);
        assert_every_pixel(
            &img,
            |x, y| {
                let b0 = unit(backdrop(x));
                let (xi, yi) = (i64::from(x), i64::from(y));
                if !in_rect(RECT, xi, yi) {
                    return byte(b0);
                }
                let px = unit(SOURCE);
                let s = inside_band(RECT, 2.0, xi, yi);
                byte(lerp(px, over(b0, unit(STROKE), 0.5, multiply), s))
            },
            blend,
        );
        // The band does not show the layer's colour: a stroke at 50 % over
        // the layer would.
        let over_layer = byte(over(unit(SOURCE), unit(STROKE), 0.5, multiply));
        assert_ne!(at(&img, 10, 13), over_layer, "{blend}: not over the layer");
    }
}

#[test]
fn a_center_strokes_inner_half_replaces_and_its_outer_half_stays_below() {
    let doc = styled(
        &rect_over_stripes(SOURCE),
        1,
        &stroke_json("center", 4, 0.5, "normal", ""),
    );
    let img = flat(&doc);
    // Inside the rect: the inner half (2 px) replaces the layer.
    let (x, y) = (10, 13);
    let b0 = unit(backdrop(x));
    assert_eq!(
        at(&img, x, y),
        byte(over(b0, unit(STROKE), 0.5, false)),
        "inner half"
    );
    // Just outside: the outer half composites below as before (the layer
    // does not cover it).
    assert_eq!(
        at(&img, 9, 13),
        byte(over(b0, unit(STROKE), 0.5, false)),
        "outer half"
    );
    assert_eq!(
        at(&img, 16, 13),
        byte(unit(SOURCE)),
        "the interior is the layer"
    );
}

#[test]
fn an_anti_aliased_rim_under_a_full_band_is_the_disjoint_mix() {
    // The layer's left column is at alpha 128: the band holds all of that
    // pixel's shape (`s = 1`), so the pixel is the band over B0 at 128/255 —
    // the layer is gone there, never the band over a half-covered pixel.
    let doc = DrDocument::from_pixels(striped(CANVAS.0, CANVAS.1));
    let rect = RgbaImage::from_fn(RECT.2, RECT.3, |x, _| {
        image::Rgba([
            SOURCE[0],
            SOURCE[1],
            SOURCE[2],
            if x == 0 { 128 } else { 255 },
        ])
    });
    let doc = doc
        .adding_image_layer(0, rect, "Rect")
        .and_then(|d| d.with_layer_offset(1, RECT.0, RECT.1))
        .expect("the rect layer");
    let img = flat(&styled(
        &doc,
        1,
        &stroke_json("inside", 2, 1.0, "normal", ""),
    ));
    let b0 = unit(backdrop(10));
    let want = byte(over(b0, unit(STROKE), 128.0 / 255.0, false));
    let got = at(&img, 10, 13);
    for c in 0..3 {
        assert!(
            (i32::from(got[c]) - i32::from(want[c])).abs() <= 1,
            "rim: got {got:?}, want {want:?}"
        );
    }
    let half = over(b0, unit(SOURCE), 128.0 / 255.0, false);
    assert_ne!(
        got,
        byte(over(half, unit(STROKE), 128.0 / 255.0, false)),
        "not the band over the half-covered pixel"
    );
}

#[test]
fn na1_leaves_the_binary_part_of_a_partial_plane_unchanged_at_integer_sizes() {
    // One partial pixel far from the hard rect moves the whole plane to NA1
    // (`style_fx_stroke::inner_band`'s `has_partial` gate). Along the rect's
    // axis-aligned edges NA1 at an integer size is the thresholded band, so
    // every pixel more than `n + 1` (Chebyshev) from the partial pixel
    // renders exactly as it does on the same layer without that pixel.
    const PARTIAL: (u32, u32) = (36, 2);
    let layer = |with_partial: bool| {
        let pixels = RgbaImage::from_fn(CANVAS.0, CANVAS.1, |x, y| {
            let alpha = if in_rect(RECT, i64::from(x), i64::from(y)) {
                255
            } else if with_partial && (x, y) == PARTIAL {
                128
            } else {
                0
            };
            image::Rgba([SOURCE[0], SOURCE[1], SOURCE[2], alpha])
        });
        DrDocument::from_pixels(striped(CANVAS.0, CANVAS.1))
            .adding_image_layer(0, pixels, "Rect")
            .expect("the canvas-sized layer")
    };
    let (partial, binary) = (layer(true), layer(false));
    for n in 1..=6u32 {
        let style = stroke_json("inside", n, 1.0, "normal", "");
        let (got, want) = (
            flat(&styled(&partial, 1, &style)),
            flat(&styled(&binary, 1, &style)),
        );
        let mut compared = 0;
        for y in 0..CANVAS.1 {
            for x in 0..CANVAS.0 {
                let far = x.abs_diff(PARTIAL.0).max(y.abs_diff(PARTIAL.1));
                if far <= n + 1 {
                    continue;
                }
                compared += 1;
                assert_eq!(
                    at(&got, x, y),
                    at(&want, x, y),
                    "Size {n} at ({x}, {y}): the binary part moved"
                );
            }
        }
        assert!(
            compared > (RECT.2 * RECT.3) as usize,
            "Size {n}: the rect is compared"
        );
    }
}

#[test]
fn a_styled_clip_base_and_its_members_are_replaced_under_the_band() {
    let green = [20u8, 180, 60];
    let member = RgbaImage::from_pixel(
        CANVAS.0,
        CANVAS.1,
        image::Rgba([green[0], green[1], green[2], 255]),
    );
    let doc = rect_over_stripes(SOURCE)
        .adding_image_layer(1, member, "Member")
        .and_then(|d| d.with_layer_clipped(2, true))
        .expect("the clipped member");
    let img = flat(&styled(
        &doc,
        1,
        &stroke_json("inside", 2, 0.5, "normal", ""),
    ));
    assert_every_pixel(
        &img,
        |x, y| {
            let b0 = unit(backdrop(x));
            let (xi, yi) = (i64::from(x), i64::from(y));
            if !in_rect(RECT, xi, yi) {
                return byte(b0);
            }
            let s = inside_band(RECT, 2.0, xi, yi);
            byte(lerp(unit(green), over(b0, unit(STROKE), 0.5, false), s))
        },
        "clip base",
    );
}

#[test]
fn a_grouped_colour_overlay_on_a_multiply_layer_is_replaced_under_the_band() {
    // D6: the overlay recolours the unit, which meets B0 once with the layer's
    // Multiply; the band then replaces that result under it, Normal.
    let overlay = [250u8, 140, 20];
    let doc = rect_over_stripes(SOURCE)
        .with_layer_blend_mode(1, BlendMode::Multiply)
        .expect("multiply");
    let style = "{\"blend_interior_as_group\":true,\"effects\":[\
                 {\"type\":\"color_overlay\",\"color\":\"#fa8c14\",\"opacity\":1},\
                 {\"type\":\"stroke\",\"position\":\"inside\",\"size\":2,\
                 \"color\":\"#1edca0\",\"opacity\":0.5}]}";
    let img = flat(&styled(&doc, 1, style));
    assert_every_pixel(
        &img,
        |x, y| {
            let b0 = unit(backdrop(x));
            let (xi, yi) = (i64::from(x), i64::from(y));
            if !in_rect(RECT, xi, yi) {
                return byte(b0);
            }
            let unit_px = over(b0, unit(overlay), 1.0, true);
            let s = inside_band(RECT, 2.0, xi, yi);
            byte(lerp(unit_px, over(b0, unit(STROKE), 0.5, false), s))
        },
        "D6",
    );
}

#[test]
fn a_knockout_layers_band_punches_to_the_backdrop_its_pixels_punch_to() {
    // A canvas-sized opaque bottom entry is the Background (D5); a Deep
    // knockout over a middle layer punches the rect to it, and its inside
    // band composites over that same B0.
    let middle = [90u8, 90, 90];
    let doc = DrDocument::from_pixels(striped(CANVAS.0, CANVAS.1))
        .adding_image_layer(
            0,
            RgbaImage::from_pixel(
                CANVAS.0,
                CANVAS.1,
                image::Rgba([middle[0], middle[1], middle[2], 255]),
            ),
            "Middle",
        )
        .expect("middle");
    let rect = RgbaImage::from_pixel(
        RECT.2,
        RECT.3,
        image::Rgba([SOURCE[0], SOURCE[1], SOURCE[2], 255]),
    );
    let doc = doc
        .adding_image_layer(1, rect, "Rect")
        .and_then(|d| d.with_layer_offset(2, RECT.0, RECT.1))
        .expect("rect");
    let style = stroke_json(
        "inside",
        2,
        0.5,
        "normal",
        "\"knockout\":\"deep\",\"fill_opacity\":0.5,",
    );
    let img = flat(&styled(&doc, 2, &style));
    assert_every_pixel(
        &img,
        |x, y| {
            let (xi, yi) = (i64::from(x), i64::from(y));
            if !in_rect(RECT, xi, yi) {
                return byte(unit(middle));
            }
            let b0 = unit(backdrop(x));
            // The knocked-out pixels at fill 1/2 over the Background.
            let px = over(b0, unit(SOURCE), 0.5, false);
            let s = inside_band(RECT, 2.0, xi, yi);
            byte(lerp(px, over(b0, unit(STROKE), 0.5, false), s))
        },
        "knockout",
    );
}

#[test]
fn the_package_mixes_the_replaced_band_back_at_the_layer_opacity() {
    let doc = rect_over_stripes(SOURCE)
        .with_layer_opacity(1, 0.5)
        .expect("opacity");
    let img = flat(&styled(
        &doc,
        1,
        &stroke_json("inside", 2, 0.5, "normal", ""),
    ));
    assert_every_pixel(
        &img,
        |x, y| {
            let b0 = unit(backdrop(x));
            let (xi, yi) = (i64::from(x), i64::from(y));
            if !in_rect(RECT, xi, yi) {
                return byte(b0);
            }
            let s = inside_band(RECT, 2.0, xi, yi);
            let packaged = lerp(unit(SOURCE), over(b0, unit(STROKE), 0.5, false), s);
            byte(lerp(b0, packaged, 0.5))
        },
        "package",
    );
}

#[test]
fn an_inside_stroke_at_zero_opacity_still_shows_the_stack_beneath_its_band() {
    // Photoshop's inside stroke at 0 % still replaces the layer under its
    // band (take1's `zero` rows read B0 to 0.00 codes): the band shows the
    // stack beneath, with no stroke colour. An outside stroke at 0 % draws
    // nothing at all.
    let doc = rect_over_stripes(SOURCE);
    let img = flat(&styled(
        &doc,
        1,
        &stroke_json("inside", 2, 0.0, "normal", ""),
    ));
    assert_every_pixel(
        &img,
        |x, y| {
            let b0 = unit(backdrop(x));
            let (xi, yi) = (i64::from(x), i64::from(y));
            if !in_rect(RECT, xi, yi) {
                return byte(b0);
            }
            byte(lerp(unit(SOURCE), b0, inside_band(RECT, 2.0, xi, yi)))
        },
        "inside at 0 %",
    );
    let outside = flat(&styled(
        &doc,
        1,
        &stroke_json("outside", 2, 0.0, "normal", ""),
    ));
    assert_eq!(outside, flat(&doc), "outside at 0 %");
}

// ------------------------------------------------------- 16 and 32 bits --

fn take(err: *mut c_char) -> String {
    let text = unsafe { CStr::from_ptr(err) }
        .to_string_lossy()
        .into_owned();
    unsafe { dr_string_free(err) };
    text
}

fn typed_styled(doc: &PrecisionDocument, layer: usize, style: Value) -> PrecisionDocument {
    let request = json!({"op":"layer_style","version":1,"layer":layer,"style":style});
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
    assert!(err.is_null(), "{request}: {}", take(err));
    assert!(!out.is_null(), "{request} changed nothing");
    *unsafe { Box::from_raw(out) }
}

fn image16(w: u32, h: u32, f: impl Fn(u32, u32) -> [u16; 4]) -> PrecisionImage {
    let pixels = (0..w * h).map(|i| f(i % w, i / w)).collect();
    PrecisionImage::from_rgba16(w, h, pixels, IccProfile::srgb(), None).unwrap()
}

/// A 16-bit sRGB document: the striped backdrop under the rect (`alpha` per
/// rect pixel), both at the 16-bit promotion of their 8-bit codes.
fn typed_doc(alpha: impl Fn(u32, u32) -> u16) -> PrecisionDocument {
    let p = |c: [u8; 3], a: u16| {
        [
            257 * u16::from(c[0]),
            257 * u16::from(c[1]),
            257 * u16::from(c[2]),
            a,
        ]
    };
    let back = image16(CANVAS.0, CANVAS.1, |x, _| p(backdrop(x), 65535));
    let rect = image16(RECT.2, RECT.3, |x, y| p(SOURCE, alpha(x, y)));
    PrecisionDocument::from_parts(
        CANVAS.0,
        CANVAS.1,
        IccProfile::srgb(),
        Metadata::default(),
        Resolution::default(),
        vec![
            Layer::raster("Back", back, (0, 0), 1.0, true).unwrap(),
            Layer::raster("Rect", rect, (RECT.0, RECT.1), 1.0, true).unwrap(),
        ],
    )
    .unwrap()
}

fn render16(doc: &PrecisionDocument) -> Vec<[u16; 4]> {
    doc.render(LIMIT, 0, None).unwrap().to_rgba16().unwrap()
}

fn style_value(position: &str, size: u32, opacity: f64) -> Value {
    json!({"effects":[{"type":"stroke","position":position,"size":size,"color":"#1edca0",
                       "opacity":opacity}]})
}

#[test]
fn the_16_bit_band_replaces_the_layer_and_its_rim() {
    let doc = typed_styled(
        &typed_doc(|x, _| if x == 0 { 128 * 257 } else { 65535 }),
        1,
        style_value("inside", 2, 0.5),
    );
    let out = render16(&doc);
    for y in 0..CANVAS.1 {
        for x in 0..CANVAS.0 {
            let (xi, yi) = (i64::from(x), i64::from(y));
            let b0 = unit(backdrop(x));
            let want = if !in_rect(RECT, xi, yi) {
                b0
            } else {
                let a = if xi == i64::from(RECT.0) {
                    128.0 / 255.0
                } else {
                    1.0
                };
                // The rim's shape is 128/255; its band holds all of it, and
                // the band, measured from the anti-aliased contour, reaches
                // `1 − 128/255` into the third column.
                let band = aa_rim_band(RECT, 2.0, 128.0, xi, yi).min(a);
                let s = band / a;
                let px = over(b0, unit(SOURCE), a, false);
                lerp(px, over(b0, unit(STROKE), a * 0.5, false), s)
            };
            let got = out[(y * CANVAS.0 + x) as usize];
            for c in 0..3 {
                let g = f64::from(got[c]) / 65535.0;
                assert!(
                    (g - want[c]).abs() * 255.0 <= 0.02,
                    "({x}, {y}) channel {c}: got {g}, want {}",
                    want[c]
                );
            }
        }
    }
}

/// IEC 61966-2.1 decode, written here.
fn decode(v: f64) -> f64 {
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}

#[test]
fn the_32_bit_band_replaces_the_layer_over_extended_range_values() {
    let (w, h) = CANVAS;
    let profile = IccProfile::srgb();
    let back: Vec<[f32; 4]> = (0..w * h)
        .map(|i| {
            if i % w < w / 2 {
                [4.0, 2.0, -0.25, 1.0]
            } else {
                [0.18, 0.5, 1.0, 1.0]
            }
        })
        .collect();
    let src: Vec<[f32; 4]> = (0..w * h)
        .map(|i| {
            let (x, y) = (i64::from(i % w), i64::from(i / w));
            if in_rect(RECT, x, y) {
                [2.5, 0.25, 0.75, 1.0]
            } else {
                [0.0; 4]
            }
        })
        .collect();
    let doc = typed_styled(
        &float_doc(w, h, &profile, &[back.clone(), src.clone()]),
        1,
        style_value("inside", 2, 0.5),
    );
    assert_eq!(doc.float_refusal(), None);
    let out = doc.render(LIMIT, 0, None).unwrap().to_rgba32().unwrap();
    let ink = unit(STROKE).map(decode);
    for (i, got) in out.iter().enumerate() {
        let (x, y) = (i as i64 % i64::from(w), i as i64 / i64::from(w));
        let b0 = back[i].map(f64::from);
        let b0 = [b0[0], b0[1], b0[2]];
        let want = if in_rect(RECT, x, y) {
            let s = inside_band(RECT, 2.0, x, y);
            let px = [
                f64::from(src[i][0]),
                f64::from(src[i][1]),
                f64::from(src[i][2]),
            ];
            lerp(px, over(b0, ink, 0.5, false), s)
        } else {
            b0
        };
        for c in 0..3 {
            assert!(
                (f64::from(got[c]) - want[c]).abs() <= 2e-5 * want[c].abs().max(1.0),
                "({x}, {y}) channel {c}: got {}, want {}",
                got[c],
                want[c]
            );
        }
    }
}

// --------------------------------------------- Photoshop's take5 rows --

/// One take5 row: the backdrop, the source and Photoshop's pixel two rows
/// inside the top edge of a 96 x 32 rect under an inside stroke of size 10,
/// (255, 128, 0), Normal, 50 % — inside the band, `b = a = 1`. Receipt
/// `doctored-fixtures/photoshop/float-styles-473/take5/job/stroke/
/// stroke_hard_normal_inside_16.tif`, sha256
/// `175ba22a89273eb285caaa77b72755d8c8968306c188e969167cf376ef30c37c`.
const TAKE5_16: [([u16; 3], [u16; 3], [u16; 3]); 6] = [
    ([0, 0, 0], [0, 48192, 65535], [32895, 16512, 0]),
    (
        [30235, 30235, 30235],
        [65535, 48192, 35199],
        [47953, 31570, 15058],
    ),
    (
        [65535, 65535, 65535],
        [65535, 65535, 65535],
        [65535, 49151, 32640],
    ),
    (
        [65535, 65535, 65535],
        [65535, 65535, 65535],
        [65535, 49151, 32640],
    ),
    (
        [65535, 48192, 35199],
        [30235, 30235, 30235],
        [65535, 40513, 17530],
    ),
    ([0, 48192, 65535], [0, 0, 0], [32895, 40513, 32640]),
];

/// The 32-bit twin: the same picture's linear values, `stroke_hard_normal_
/// inside_32.tif`, sha256
/// `3f5b965296ed079a6af29163ecc2eb9fcb79a937a40f68b6bc26a42a31cc24d9`. The
/// effect colour Photoshop composited there is (1, 128/255, 0) in linear
/// light, which an authored sRGB code holds in R and B only (G would be code
/// 187.5), so the rows are held on R and B.
const TAKE5_32: [([f32; 3], [f32; 3], [f32; 3]); 6] = [
    (
        [0.0, 0.0, 0.0],
        [-0.25, 0.5, 1.0],
        [0.501_960_8, 0.251_964_66, 0.0],
    ),
    (
        [0.18, 0.18, 0.18],
        [2.0, 0.5, 0.25],
        [0.591_607_8, 0.341_611_7, 0.089_647_05],
    ),
    (
        [1.0, 1.0, 1.0],
        [4.0, 4.0, 4.0],
        [1.0, 0.750_003_8, 0.498_039_24],
    ),
    (
        [4.0, 4.0, 4.0],
        [1.0, 1.0, 1.0],
        [2.494_117_5, 2.244_121_3, 1.992_156_7],
    ),
    (
        [2.0, 0.5, 0.25],
        [0.18, 0.18, 0.18],
        [1.498_039_2, 0.500_984_25, 0.124_509_8],
    ),
    (
        [-0.25, 0.5, 1.0],
        [0.0, 0.0, 0.0],
        [0.377_451, 0.500_984_25, 0.498_039_2],
    ),
];

/// The judged residual these rows are held to: Photoshop's opacity is the
/// 8-bit byte (128/255) where Doctored's is exact, so a band pixel may
/// sit `(128/255 − 1/2)·|stroke − B0|` from Photoshop's — at most 0.5 code —
/// and the replacement adds nothing to it.
const BYTE_OPACITY_GAP: f64 = 128.0 / 255.0 - 0.5;

#[test]
fn photoshops_take5_band_pixels_at_16_bits() {
    let stroke = [1.0, 128.0 / 255.0, 0.0];
    for (b0, src, ps) in TAKE5_16 {
        let back = image16(24, 24, |_, _| [b0[0], b0[1], b0[2], 65535]);
        let rect = image16(20, 20, |_, _| [src[0], src[1], src[2], 65535]);
        let doc = PrecisionDocument::from_parts(
            24,
            24,
            IccProfile::srgb(),
            Metadata::default(),
            Resolution::default(),
            vec![
                Layer::raster("Back", back, (0, 0), 1.0, true).unwrap(),
                Layer::raster("Rect", rect, (2, 2), 1.0, true).unwrap(),
            ],
        )
        .unwrap();
        let doc = typed_styled(
            &doc,
            1,
            json!({"effects":[{"type":"stroke","position":"inside","size":10,
                               "color":"#ff8000","opacity":0.5}]}),
        );
        let got = render16(&doc)[4 * 24 + 12];
        for c in 0..3 {
            let g = f64::from(got[c]) / 65535.0;
            let p = f64::from(ps[c]) / 65535.0;
            let gap = BYTE_OPACITY_GAP * (stroke[c] - f64::from(b0[c]) / 65535.0).abs();
            assert!(
                (g - p).abs() * 255.0 <= gap * 255.0 + 0.02,
                "B0 {b0:?} source {src:?} channel {c}: got {g}, Photoshop {p}"
            );
        }
        // Over the layer's own pixels (the old rule) the row misses by codes.
        let over_layer = over(
            [src[0], src[1], src[2]].map(|v| f64::from(v) / 65535.0),
            stroke,
            0.5,
            false,
        );
        let worst = (0..3)
            .map(|c| (over_layer[c] - f64::from(ps[c]) / 65535.0).abs() * 255.0)
            .fold(0.0, f64::max);
        assert!(
            b0 == src || worst > 10.0,
            "the row discriminates: {worst} codes over the layer"
        );
    }
}

#[test]
fn photoshops_take5_band_pixels_at_32_bits() {
    let profile = IccProfile::srgb();
    for (b0, src, ps) in TAKE5_32 {
        let (w, h) = (24u32, 24u32);
        let back = vec![[b0[0], b0[1], b0[2], 1.0]; (w * h) as usize];
        let rect: Vec<[f32; 4]> = (0..w * h)
            .map(|i| {
                let (x, y) = (i % w, i / w);
                if (2..22).contains(&x) && (2..22).contains(&y) {
                    [src[0], src[1], src[2], 1.0]
                } else {
                    [0.0; 4]
                }
            })
            .collect();
        let doc = typed_styled(
            &float_doc(w, h, &profile, &[back, rect]),
            1,
            json!({"effects":[{"type":"stroke","position":"inside","size":10,
                               "color":"#ff8000","opacity":0.5}]}),
        );
        let got = doc.render(LIMIT, 0, None).unwrap().to_rgba32().unwrap()[(4 * w + 12) as usize];
        for c in [0usize, 2] {
            let stroke = if c == 0 { 1.0 } else { 0.0 };
            let gap = BYTE_OPACITY_GAP * (stroke - f64::from(b0[c])).abs();
            assert!(
                (f64::from(got[c]) - f64::from(ps[c])).abs() <= gap + 1e-6,
                "B0 {b0:?} source {src:?} channel {c}: got {}, Photoshop {}",
                got[c],
                ps[c]
            );
        }
    }
}

// --------------------------------------------- Photoshop's take2 rows --

/// take2's backdrop stripes, source and stroke (`style_v1x_take.py`, the
/// confirming take): a 128 x 64 canvas of three vertical stripes under a
/// (200, 40, 90) rect at (16, 16, 96, 32), the stroke (240, 120, 30) at 35 %.
const T2_BACKS: [[u8; 3]; 3] = [[12, 12, 12], [200, 90, 160], [90, 200, 230]];
const T2_STYLES: [(&str, &str); 4] = [
    (
        "center_alone",
        "{\"effects\":[{\"type\":\"stroke\",\"position\":\"center\",\"size\":7,\
         \"color\":\"#f0781e\",\"opacity\":0.35}]}",
    ),
    (
        "inside_co",
        "{\"effects\":[{\"type\":\"color_overlay\",\"color\":\"#fa8c14\",\"opacity\":1},\
         {\"type\":\"stroke\",\"position\":\"inside\",\"size\":3,\"color\":\"#f0781e\",\
         \"opacity\":0.35}]}",
    ),
    (
        "center_mul",
        "{\"effects\":[{\"type\":\"stroke\",\"position\":\"center\",\"size\":7,\
         \"color\":\"#f0781e\",\"opacity\":0.35,\"blend\":\"multiply\"}]}",
    ),
    (
        "inside_is",
        "{\"effects\":[{\"type\":\"inner_shadow\",\"color\":\"#000000\",\"opacity\":1,\
         \"blend\":\"normal\",\"angle\":90,\"use_global_light\":false,\"distance\":3,\
         \"choke\":0,\"size\":0},{\"type\":\"stroke\",\"position\":\"inside\",\"size\":3,\
         \"color\":\"#f0781e\",\"opacity\":0.35}]}",
    ),
];

/// take2's worst rows per depth (on the rect: the disc's worst 8-bit
/// row is its band's own geometry, which the MCP drive measures), Photoshop's
/// pixel at `(x, y)` as file codes (8, 16) or linear (32). Receipts
/// `doctored-fixtures/photoshop/style-parity-v1x/take2/job/`:
/// `stroke_00/st_stroke_center_alone_8.tif` sha256 `67b974eb…5b529522`,
/// `stroke_00/st_stroke_inside_co_8.tif` `c912b6db…6de32f2`,
/// `stroke_01/st_stroke_center_mul_16.tif` `1a126c5b…51b00b17b`,
/// `stroke_00/st_stroke_inside_is_16.tif` `4a55e64a…154833effa`,
/// `stroke_01/st_stroke_center_mul_32.tif` `97213b77…9fa9ac8e3`. `judged` is
/// the take's residual of the shipped rule at that row, in codes.
/// `(variant, bits, x, y, Photoshop's pixel, judged residual)`.
type StrokeRow = (&'static str, u32, u32, u32, [f64; 3], f64);

const T2_ROWS: [StrokeRow; 5] = [
    ("center_alone", 8, 43, 19, [207.0, 70.0, 103.0], 0.686),
    ("inside_co", 8, 43, 16, [214.0, 100.0, 115.0], 0.471),
    ("center_mul", 16, 19, 19, [27116.0, 6382.0, 12590.0], 0.367),
    ("inside_is", 16, 109, 19, [36583.0, 44221.0, 41167.0], 0.011),
    (
        "center_mul",
        32,
        19,
        19,
        [0.289_420_34, 0.011_890_235, 0.052_130_118],
        0.260,
    ),
];

fn t2_back(x: u32) -> [u8; 3] {
    T2_BACKS[(x as usize * 3 / 128).min(2)]
}

fn encode(v: f64) -> f64 {
    let v = v.clamp(0.0, 1.0);
    if v <= 0.003_130_8 {
        12.92 * v
    } else {
        1.055 * v.powf(1.0 / 2.4) - 0.055
    }
}

#[test]
fn photoshops_take2_band_pixels_at_every_depth() {
    let (w, h) = (128u32, 64u32);
    let source = [200u8, 40, 90];
    let in_t2_rect = |x: u32, y: u32| (16..112).contains(&x) && (16..48).contains(&y);
    for (variant, bits, x, y, photoshop, judged) in T2_ROWS {
        let style = T2_STYLES.iter().find(|(n, _)| *n == variant).unwrap().1;
        // Photoshop's opacity is the byte 89/255 where Doctored's is
        // exact, and Doctored's band is a u8 plane (a fractional band pixel,
        // the centre stroke's half-pixel edge, rounds to 1/255 of the share):
        // at most this much of a code apart on top of the judged row.
        let gap = (0.35 - 89.0 / 255.0f64).abs() * 255.0 + 0.5;
        let (got, want, bound): ([f64; 3], [f64; 3], f64) = match bits {
            8 => {
                let back = RgbaImage::from_fn(w, h, |x, _| {
                    let c = t2_back(x);
                    image::Rgba([c[0], c[1], c[2], 255])
                });
                let rect = RgbaImage::from_pixel(
                    96,
                    32,
                    image::Rgba([source[0], source[1], source[2], 255]),
                );
                let doc = DrDocument::from_pixels(back)
                    .adding_image_layer(0, rect, "Rect")
                    .and_then(|d| d.with_layer_offset(1, 16, 16))
                    .unwrap();
                let handle = Box::into_raw(Box::new(styled(&doc, 1, style)));
                let img = flat_pixels(handle);
                unsafe { doctored_core::ffi_doc::dr_doc_free(handle) };
                let p = common::pixel(&img, w, x, y);
                (
                    [p[0], p[1], p[2]].map(f64::from),
                    photoshop,
                    judged + gap + 1.0,
                )
            }
            16 => {
                let p16 = |c: [u8; 3], a: u16| {
                    [
                        257 * u16::from(c[0]),
                        257 * u16::from(c[1]),
                        257 * u16::from(c[2]),
                        a,
                    ]
                };
                let back = image16(w, h, |x, _| p16(t2_back(x), 65535));
                let rect = image16(96, 32, |_, _| p16(source, 65535));
                let doc = PrecisionDocument::from_parts(
                    w,
                    h,
                    IccProfile::srgb(),
                    Metadata::default(),
                    Resolution::default(),
                    vec![
                        Layer::raster("Back", back, (0, 0), 1.0, true).unwrap(),
                        Layer::raster("Rect", rect, (16, 16), 1.0, true).unwrap(),
                    ],
                )
                .unwrap();
                let style: Value = serde_json::from_str(style).unwrap();
                let out = render16(&typed_styled(&doc, 1, style))[(y * w + x) as usize];
                let codes = |v: [f64; 3]| v.map(|c| c / 65535.0 * 255.0);
                (
                    codes([out[0], out[1], out[2]].map(f64::from)),
                    codes(photoshop),
                    judged + gap + 0.01,
                )
            }
            _ => {
                let lin = |c: [u8; 3]| c.map(|v| decode(f64::from(v) / 255.0) as f32);
                let back: Vec<[f32; 4]> = (0..w * h)
                    .map(|i| {
                        let c = lin(t2_back(i % w));
                        [c[0], c[1], c[2], 1.0]
                    })
                    .collect();
                let rect: Vec<[f32; 4]> = (0..w * h)
                    .map(|i| {
                        if in_t2_rect(i % w, i / w) {
                            let c = lin(source);
                            [c[0], c[1], c[2], 1.0]
                        } else {
                            [0.0; 4]
                        }
                    })
                    .collect();
                let doc = float_doc(w, h, &IccProfile::srgb(), &[back, rect]);
                let style: Value = serde_json::from_str(style).unwrap();
                let out = typed_styled(&doc, 1, style)
                    .render(LIMIT, 0, None)
                    .unwrap()
                    .to_rgba32()
                    .unwrap()[(y * w + x) as usize];
                let enc = |v: [f64; 3]| v.map(|c| encode(c) * 255.0);
                (
                    enc([out[0], out[1], out[2]].map(f64::from)),
                    enc(photoshop),
                    judged + gap + 0.01,
                )
            }
        };
        for c in 0..3 {
            assert!(
                (got[c] - want[c]).abs() <= bound,
                "{variant} at {bits} bits ({x}, {y}) channel {c}: got {}, Photoshop {} (bound {bound})",
                got[c],
                want[c]
            );
        }
    }
}

// ------------------------------------------------------------- the plan --

/// The B0 store (one u32 index and one f32×4 pixel per band pixel, at most the
/// layer's `w·h`) is charged by the typed render plan: an inside stroke plans
/// exactly `20·w·h` more than an outside one of the same size, whose pad,
/// effect count and blur are the same.
#[test]
fn the_typed_plan_charges_the_band_store() {
    let (w, h) = (300u32, 200u32);
    let p = [20000u16, 30000, 40000, 65535];
    let back = image16(w, h, |_, _| p);
    let top = image16(w, h, |x, y| {
        if (20..280).contains(&x) && (20..180).contains(&y) {
            p
        } else {
            [0; 4]
        }
    });
    let doc = PrecisionDocument::from_parts(
        w,
        h,
        IccProfile::srgb(),
        Metadata::default(),
        Resolution::default(),
        vec![
            Layer::raster("Back", back, (0, 0), 1.0, true).unwrap(),
            Layer::raster("Top", top, (0, 0), 1.0, true).unwrap(),
        ],
    )
    .unwrap();
    let planned = |position: &str| {
        typed_styled(&doc, 1, style_value(position, 4, 0.5))
            .render_plan()
            .unwrap()
            .scratch_bytes
    };
    let (inside, outside) = (planned("inside"), planned("outside"));
    assert_eq!(inside - outside, 20 * u64::from(w) * u64::from(h));
    assert_eq!(
        planned("center") - planned_center_outside_reach(&doc),
        20 * u64::from(w) * u64::from(h)
    );
}

/// An outside stroke with the reach a centre stroke of size 4 has (2 px).
fn planned_center_outside_reach(doc: &PrecisionDocument) -> u64 {
    typed_styled(doc, 1, style_value("outside", 2, 0.5))
        .render_plan()
        .unwrap()
        .scratch_bytes
}
