//! The Stroke's Fill Type Gradient options only the Stroke has at 8 bits,
//! black-box through `dr_doc_set_layer_style` and the
//! projection: Shape Burst at Outside, Inside and Center against the
//! phase-3 law (`scripts/photoshop-probe/gradient_identify_673_fit.py`,
//! PHASE-3's `burst.d`: `D` the 8-neighbour chamfer across the contour,
//! Outside `1 − D / (size + 1)`, Inside `D / (size + 1)`, Center one ramp
//! over `size + 2`, Scale ignored) under the Classic ramp
//! (`common::classic_ramp`), its two declared ends (a shape with no pixel at
//! coverage ≥ 128 draws its band in the first stop), every Gradient Style
//! over the stroke's box GROWN by the band's outward reach (BOX = BAND),
//! stacked strokes each drawing their own fill,
//! Dither (deterministic, never more than one code), the JSON (written only
//! when set, `shape_burst` refused on a Gradient Overlay by name), a cache
//! patch after a paint dab against a fresh render, and the byte `.rz` word
//! 94 both ways. Every oracle is a closed form written here; never a golden.

use std::f64::consts::TAU;
use std::sync::Arc;

use doctored_core::doc::DrDocument;
use doctored_core::ffi_doc::*;
use doctored_core::style::{GradientStyle, LayerStyle};
use serde_json::Value;

use crate::common;
use common::*;

/// A 48 x 40 white canvas under a 22 x 18 red rect made at (12, 10): a 6 px
/// outside band stays 6 px from every canvas edge, a 6 px inside band
/// leaves a 10 x 6 interior.
const CANVAS: (u32, u32) = (48, 40);
const RECT: (i32, i32, u32, u32) = (12, 10, 22, 18);
const SIZE: f64 = 6.0;

fn rect_doc() -> DrDocument {
    rect_layer_doc(CANVAS, RECT, RED)
}

/// One stroke of `size` at `position`, a black-to-white gradient with the
/// extra gradient keys `gradient` spliced in.
fn stroke(position: &str, size: f64, gradient: &str) -> String {
    let gradient = if gradient.is_empty() {
        String::new()
    } else {
        format!(",{gradient}")
    };
    format!(
        "{{\"type\":\"stroke\",\"size\":{size},\"position\":\"{position}\",\
         \"fill_type\":\"gradient\",\"gradient\":{{\"stops\":[\
         {{\"position\":0,\"color\":\"#000000\",\"opacity\":1}},\
         {{\"position\":1,\"color\":\"#ffffff\",\"opacity\":1}}]{gradient}}}}}"
    )
}

fn style_of(effects: &[String]) -> String {
    format!("{{\"effects\":[{}]}}", effects.join(","))
}

fn flat(doc: &DrDocument) -> Vec<u8> {
    doc.flattened().unwrap().into_raw()
}

fn at(flat: &[u8], x: u32, y: u32) -> [u8; 4] {
    pixel(flat, CANVAS.0, x, y)
}

/// The opaque grey of `t` on the 8-bit grid, the projection's quantization.
fn grey(t: f64) -> [u8; 4] {
    let v = (t.clamp(0.0, 1.0) * 255.0).round() as u8;
    [v, v, v, 255]
}

/// The 5 x 5 chamfer distance (steps 1, sqrt 2, sqrt 5) of `(dx, dy)` in
/// closed form (`style_stroke.rs`'s, restated): with `a >= b` the absolute
/// components, `b` knight moves and `a − 2b` axial steps when `a >= 2b`,
/// else `a − b` knights and `2b − a` diagonals.
fn chamfer(dx: i64, dy: i64) -> f64 {
    let (a, b) = (dx.abs().max(dy.abs()), dx.abs().min(dy.abs()));
    let (knight, diagonal) = (5f64.sqrt(), 2f64.sqrt());
    if a >= 2 * b {
        b as f64 * knight + (a - 2 * b) as f64
    } else {
        (a - b) as f64 * knight + (2 * b - a) as f64 * diagonal
    }
}

/// Where canvas pixel `(x, y)` sits: `Some((true, D))` outside the rect at
/// chamfer distance D from it, `Some((false, d_out))` inside it at axis
/// distance d_out from its complement (the edge pixel 1).
fn distance(x: u32, y: u32) -> (bool, f64) {
    let (x, y) = (i64::from(x), i64::from(y));
    let (x0, y0) = (i64::from(RECT.0), i64::from(RECT.1));
    let (x1, y1) = (x0 + i64::from(RECT.2), y0 + i64::from(RECT.3));
    if x >= x0 && x < x1 && y >= y0 && y < y1 {
        let d = (x - x0 + 1).min(x1 - x).min(y - y0 + 1).min(y1 - y);
        return (false, d as f64);
    }
    let (nx, ny) = (x.clamp(x0, x1 - 1), y.clamp(y0, y1 - 1));
    (true, chamfer(x - nx, y - ny))
}

/// Whether the band of a stroke at `position` covers `(x, y)` fully: an
/// outside pixel within the outward reach (the band's straight-edge and
/// corner rule, `style_stroke.rs`'s `outside_cov` at 255), an inside pixel
/// within the inward reach.
fn full(position: &str, size: f64, x: u32, y: u32) -> bool {
    let (outward, inward) = match position {
        "outside" => (size, 0.0),
        "inside" => (0.0, size),
        _ => (size / 2.0, size / 2.0),
    };
    match distance(x, y) {
        (true, d) => d <= outward,
        (false, d) => d <= inward,
    }
}

/// The 8-neighbour chamfer distance (steps 1, sqrt 2) across the rect's
/// contour at canvas `(x, y)`: `(true, D)` outside it, `D = (a − b) + b
/// sqrt 2` from the nearest rect pixel (`a >= b` the absolute components),
/// `(false, D)` inside it, the axis distance to its complement (the edge
/// pixel 1).
fn contour(x: u32, y: u32) -> (bool, f64) {
    let (x, y) = (i64::from(x), i64::from(y));
    let (x0, y0) = (i64::from(RECT.0), i64::from(RECT.1));
    let (x1, y1) = (x0 + i64::from(RECT.2), y0 + i64::from(RECT.3));
    if x >= x0 && x < x1 && y >= y0 && y < y1 {
        let d = (x - x0 + 1).min(x1 - x).min(y - y0 + 1).min(y1 - y);
        return (false, d as f64);
    }
    let (dx, dy) = (x - x.clamp(x0, x1 - 1), y - y.clamp(y0, y1 - 1));
    let (a, b) = (dx.abs().max(dy.abs()), dx.abs().min(dy.abs()));
    (true, (a - b) as f64 + b as f64 * 2f64.sqrt())
}

/// THE SHAPE BURST law (module doc): `t` from the outer edge (0) to the
/// inner edge (1), `D` clamped to `[0, size + 1]`.
fn burst_t(position: &str, size: f64, x: u32, y: u32) -> f64 {
    let (outside, raw) = contour(x, y);
    let d = raw.clamp(0.0, size + 1.0);
    let t = match (position, outside) {
        ("outside", true) => 1.0 - d / (size + 1.0),
        ("inside", false) => d / (size + 1.0),
        ("center", true) => (size / 2.0 + 1.0 - d) / (size + 2.0),
        ("center", false) => (d + size / 2.0) / (size + 2.0),
        _ => unreachable!("only band pixels"),
    };
    t.clamp(0.0, 1.0)
}

/// The grey a black-to-white Shape Burst draws at `(x, y)`: the Classic
/// ramp of [`burst_t`] (`common::classic_ramp`).
fn burst(position: &str, size: f64, x: u32, y: u32) -> f64 {
    classic_ramp(&[(0.0, 0.0), (1.0, 1.0)], burst_t(position, size, x, y))
}

/// Asserts every fully covered band pixel of `doc` against `want(x, y)`
/// (the grey in 0..1: `t` itself on a linear ramp, [`value`] of it on a
/// Linear or Angle one), ±1, and returns how many it checked.
fn assert_band(
    doc: &DrDocument,
    position: &str,
    size: f64,
    want: impl Fn(u32, u32) -> Option<f64>,
    what: &str,
) -> usize {
    let img = flat(doc);
    let mut n = 0;
    for y in 0..CANVAS.1 {
        for x in 0..CANVAS.0 {
            if !full(position, size, x, y) {
                continue;
            }
            if let Some(t) = want(x, y) {
                assert_close(
                    &at(&img, x, y),
                    &grey(t),
                    &format!("{what} ({x},{y}) t {t}"),
                );
                n += 1;
            }
        }
    }
    n
}

// ------------------------------------------------------------ Shape Burst --

#[test]
fn shape_burst_ramps_across_the_band_at_every_position() {
    for position in ["outside", "inside", "center"] {
        let json = style_of(&[stroke(position, SIZE, "\"style\":\"shape_burst\"")]);
        let doc = styled(&rect_doc(), 1, &json);
        let n = assert_band(
            &doc,
            position,
            SIZE,
            |x, y| Some(burst(position, SIZE, x, y)),
            position,
        );
        assert!(n > 200, "{position}: {n} band pixels checked");
    }
}

#[test]
fn shape_burst_ignores_scale_angle_and_the_box_and_reverse_flips_it() {
    // Scale (the take's IGNORED), the angle and the box do not move it;
    // Reverse flips it.
    let base = flat(&styled(
        &rect_doc(),
        1,
        &style_of(&[stroke("center", SIZE, "\"style\":\"shape_burst\"")]),
    ));
    for extra in [
        "\"style\":\"shape_burst\",\"scale\":0.5",
        "\"style\":\"shape_burst\",\"angle\":37",
        "\"style\":\"shape_burst\",\"align_with_layer\":false",
    ] {
        let got = flat(&styled(
            &rect_doc(),
            1,
            &style_of(&[stroke("center", SIZE, extra)]),
        ));
        assert_eq!(got, base, "{extra}: identical");
    }
    let reversed = styled(
        &rect_doc(),
        1,
        &style_of(&[stroke(
            "outside",
            SIZE,
            "\"style\":\"shape_burst\",\"reverse\":true",
        )]),
    );
    assert_band(
        &reversed,
        "outside",
        SIZE,
        |x, y| {
            let t = 1.0 - burst_t("outside", SIZE, x, y);
            Some(classic_ramp(&[(0.0, 0.0), (1.0, 1.0)], t))
        },
        "reversed",
    );
}

#[test]
fn a_shape_with_no_pixel_at_half_coverage_draws_its_band_in_the_first_stop() {
    // `burst_t`'s declared ends (style_stroke_fill's module doc): a rect at
    // alpha 102 has no pixel at coverage >= 128, so every pixel is outside
    // the contour and its other side is empty (`D` +inf, clamped to size +
    // 1). Outside reads t = 0 and Center's outer half t = 0: the band, still
    // cut by the seeded 5 x 5 chamfer from the 102 pixels, draws black.
    let faint = rect_layer_doc(CANVAS, RECT, [255, 0, 0, 102]);
    let y = (RECT.1 + 9) as u32;
    for (position, reach) in [("outside", 6), ("center", 2)] {
        let json = style_of(&[stroke(position, 8.0, "\"style\":\"shape_burst\"")]);
        let img = flat(&styled(&faint, 1, &json));
        for k in 1..=reach {
            let x = (RECT.0 - k) as u32;
            assert_eq!(
                at(&img, x, y),
                [0, 0, 0, 255],
                "{position}: ({x}, {y}), {k} px outside, reads the first stop"
            );
        }
    }
}

// ------------------------------------------------------------- the box --

/// The black-to-white grey of a `style` gradient at `t`: the Classic
/// ramp for Linear, Radial, Angle and Reflected
/// (`common::classic_ramp`), `t` itself for Diamond.
fn value(style: &str, t: f64) -> f64 {
    if matches!(style, "linear" | "radial" | "angle" | "reflected") {
        classic_ramp(&[(0.0, 0.0), (1.0, 1.0)], t)
    } else {
        t
    }
}

/// `GradientSampler::t` restated (style_gradient's doc) in f64 over the
/// stroke's box: the layer's bounds (layer coordinates, the layer made at
/// RECT) grown by `reach`. Linear, Angle and Reflected read the pixel's
/// top-left corner: Linear's extent `E` is the line through the centre
/// inside the box, `min(bw / |cos a|, bh / |sin a|) · scale`, Reflected is
/// `|2u / E|` with the same `E`, and Angle sweeps clockwise. Radial reads the
/// corner from the INTEGER centre `(bx + ⌊bw/2⌋, by + ⌊bh/2⌋)` over the
/// length of the whole-pixel vector `(⌊(E/2) |cos a|⌋, ⌊(E/2) |sin a|⌋)`.
fn style_t(style: &str, angle: f64, scale: f64, reach: f64, x: u32, y: u32) -> f64 {
    let (bx, by) = (-reach, -reach);
    let (bw, bh) = (
        f64::from(RECT.2) + 2.0 * reach,
        f64::from(RECT.3) + 2.0 * reach,
    );
    let (cx, cy) = (bx + bw / 2.0, by + bh / 2.0);
    let a = angle.to_radians();
    let (sin, cos) = a.sin_cos();
    let dx = f64::from(x) - f64::from(RECT.0) + 0.5 - cx;
    let dy = f64::from(y) - f64::from(RECT.1) + 0.5 - cy;
    let u = dx * cos - dy * sin;
    let v = dx * sin + dy * cos;
    let major = bw.max(bh) * scale;
    let (kx, ky) = (dx - 0.5, dy - 0.5);
    let line = (bw / cos.abs()).min(bh / sin.abs()) * scale;
    let ku = kx * cos - ky * sin;
    let t = match style {
        "linear" => 0.5 + ku / line,
        "reflected" => (2.0 * ku / line).abs(),
        "radial" => {
            let ix = f64::from(x) - f64::from(RECT.0) - (bx + (bw / 2.0).floor());
            let iy = f64::from(y) - f64::from(RECT.1) - (by + (bh / 2.0).floor());
            let (rx, ry) = (
                (line / 2.0 * cos.abs()).floor(),
                (line / 2.0 * sin.abs()).floor(),
            );
            (ix * ix + iy * iy).sqrt() / (rx * rx + ry * ry).sqrt()
        }
        "diamond" => 2.0 * (u.abs() + v.abs()) / major,
        _ => (a - (-ky).atan2(kx)).rem_euclid(TAU) / TAU,
    };
    t.clamp(0.0, 1.0)
}

#[test]
fn every_style_spans_the_box_grown_by_the_bands_reach() {
    for (position, reach) in [("outside", SIZE), ("center", SIZE / 2.0), ("inside", 0.0)] {
        for (style, angle, scale) in [
            ("linear", 30.0, 1.0),
            ("linear", 0.0, 0.8),
            ("radial", 90.0, 1.0),
            ("angle", 30.0, 1.0),
            ("reflected", -60.0, 1.2),
            ("diamond", 15.0, 1.0),
        ] {
            let json = style_of(&[stroke(
                position,
                SIZE,
                &format!("\"style\":\"{style}\",\"angle\":{angle},\"scale\":{scale}"),
            )]);
            let doc = styled(&rect_doc(), 1, &json);
            let n = assert_band(
                &doc,
                position,
                SIZE,
                |x, y| {
                    let t = style_t(style, angle, scale, reach, x, y);
                    // The angle style's seam: f32 against f64 may land on
                    // either side of it.
                    (style != "angle" || (0.01..0.99).contains(&t)).then_some(value(style, t))
                },
                &format!("{position} {style} {angle} {scale}"),
            );
            assert!(n > 100, "{position} {style}: {n} checked");
        }
    }
}

#[test]
fn stacked_strokes_each_draw_their_own_fill() {
    // A Shape Burst Inside under a Linear Outside (array order is render
    // order): each band shows its own instance's fill.
    let json = style_of(&[
        stroke("inside", 4.0, "\"style\":\"shape_burst\""),
        stroke("outside", 4.0, "\"style\":\"linear\",\"angle\":0"),
    ]);
    let doc = styled(&rect_doc(), 1, &json);
    let inside = assert_band(
        &doc,
        "inside",
        4.0,
        |x, y| Some(burst("inside", 4.0, x, y)),
        "the inside Shape Burst",
    );
    let outside = assert_band(
        &doc,
        "outside",
        4.0,
        |x, y| Some(value("linear", style_t("linear", 0.0, 1.0, 4.0, x, y))),
        "the outside Linear",
    );
    assert!(inside > 100 && outside > 100);
}

// --------------------------------------------------------------- Dither --

#[test]
fn dither_is_deterministic_and_moves_no_sample_more_than_a_code() {
    for effect in [
        stroke("outside", SIZE, "\"style\":\"shape_burst\""),
        stroke("center", SIZE, "\"style\":\"linear\",\"angle\":20"),
        "{\"type\":\"gradient_overlay\",\"gradient\":{\"style\":\"radial\"}}".to_string(),
    ] {
        let plain = flat(&styled(
            &rect_doc(),
            1,
            &style_of(std::slice::from_ref(&effect)),
        ));
        let dithered_json =
            style_of(&[effect.replacen("\"gradient\":{", "\"gradient\":{\"dither\":true,", 1)]);
        let dithered = flat(&styled(&rect_doc(), 1, &dithered_json));
        let again = flat(&styled(&rect_doc(), 1, &dithered_json));
        assert_eq!(dithered, again, "{effect}: deterministic");
        let mut moved = 0;
        for (a, b) in plain.iter().zip(&dithered) {
            let d = (i32::from(*a) - i32::from(*b)).abs();
            assert!(d <= 1, "{effect}: a sample moved {d} codes");
            moved += usize::from(d == 1);
        }
        assert!(moved > 0, "{effect}: Dither moved no sample");
    }
}

// ----------------------------------------------------------------- JSON --

fn effect0(doc: &DrDocument) -> Value {
    let text = ffi_style(doc as *const DrDocument, 1).expect("a style");
    serde_json::from_str::<Value>(&text).unwrap()["effects"][0].clone()
}

#[test]
fn shape_burst_and_dither_are_written_only_when_set() {
    let plain = styled(&rect_doc(), 1, &style_of(&[stroke("outside", SIZE, "")]));
    let g = &effect0(&plain)["gradient"];
    assert_eq!(g["style"], "linear");
    assert!(g.get("dither").is_none(), "no dither key at its default");
    let off = styled(
        &rect_doc(),
        1,
        &style_of(&[stroke("outside", SIZE, "\"dither\":false")]),
    );
    assert!(effect0(&off)["gradient"].get("dither").is_none());
    let set = styled(
        &rect_doc(),
        1,
        &style_of(&[stroke(
            "outside",
            SIZE,
            "\"style\":\"shape_burst\",\"dither\":true",
        )]),
    );
    let g = &effect0(&set)["gradient"];
    assert_eq!(g["style"], "shape_burst");
    assert_eq!(g["dither"], true);
    let text = ffi_style(&set as *const DrDocument, 1).unwrap();
    assert_eq!(parse(&text).to_json(), text, "the canonical echo");
    let overlay = styled(
        &rect_doc(),
        1,
        "{\"effects\":[{\"type\":\"gradient_overlay\",\"gradient\":{\"dither\":true}}]}",
    );
    assert_eq!(
        effect0(&overlay)["gradient"]["dither"],
        true,
        "the overlay too"
    );
}

#[test]
fn shape_burst_is_the_strokes_alone() {
    let json = "{\"effects\":[{\"type\":\"gradient_overlay\",\
                \"gradient\":{\"style\":\"shape_burst\"}}]}";
    let doc = rect_doc();
    assert_eq!(
        style_error(&doc as *const DrDocument, 1, json).as_deref(),
        Some(
            "style: effects[0].gradient.style must be one of linear, radial, angle, \
             reflected, diamond"
        )
    );
    let lenient = LayerStyle::from_json_lenient(json).unwrap();
    let doctored_core::style::Effect::GradientOverlay(g) = &lenient.effects[0] else {
        panic!("a gradient overlay");
    };
    assert_eq!(g.gradient.style, GradientStyle::Linear, "lenient: linear");
    let bad = style_of(&[stroke("outside", SIZE, "\"dither\":1")]);
    assert_eq!(
        style_error(&doc as *const DrDocument, 1, &bad).as_deref(),
        Some("style: effects[0].gradient.dither must be a boolean")
    );
}

// ---------------------------------------------------------------- cache --

/// Paints a canvas-frame premultiplied overlay onto layer `idx` through
/// `dr_doc_painting_layer`.
fn painted(doc: &DrDocument, idx: usize, overlay: &[u8], mode: i32) -> DrDocument {
    let handle = Box::into_raw(Box::new(doc.clone()));
    let out = unsafe {
        dr_doc_painting_layer(
            handle,
            idx,
            overlay.as_ptr(),
            doc.width,
            doc.height,
            mode,
            1.0,
        )
    };
    unsafe { dr_doc_free(handle) };
    assert!(!out.is_null(), "painting_layer refused");
    *unsafe { Box::from_raw(out) }
}

#[test]
fn a_patch_after_a_paint_dab_equals_a_fresh_render() {
    // A canvas-sized layer holding the rect: a dab beside it and a hole in
    // it change the shape locally; the Shape Burst field spans the whole
    // plane, so the cache re-renders it whole and must equal a fresh style.
    let mut content = image::RgbaImage::new(CANVAS.0, CANVAS.1);
    for y in 10..28 {
        for x in 12..34 {
            content.put_pixel(x, y, image::Rgba(RED));
        }
    }
    let base = DrDocument::from_pixels(solid(CANVAS.0, CANVAS.1, WHITE))
        .adding_image_layer(0, content, "Content")
        .unwrap();
    let overlay = |rect: (u32, u32, u32, u32), px: [u8; 4]| {
        let mut out = vec![0u8; (CANVAS.0 * CANVAS.1 * 4) as usize];
        for y in rect.1..rect.1 + rect.3 {
            for x in rect.0..rect.0 + rect.2 {
                let i = ((y * CANVAS.0 + x) * 4) as usize;
                out[i..i + 4].copy_from_slice(&px);
            }
        }
        out
    };
    for json in [
        style_of(&[stroke("center", SIZE, "\"style\":\"shape_burst\"")]),
        style_of(&[stroke(
            "outside",
            4.0,
            "\"style\":\"shape_burst\",\"dither\":true",
        )]),
    ] {
        let cached = styled(&base, 1, &json);
        let _ = cached.flattened().unwrap();
        for (name, paint, mode) in [
            (
                "dab",
                overlay((38, 30, 4, 4), [0, 0, 255, 255]),
                COMPOSITE_OVER,
            ),
            (
                "hole",
                overlay((20, 15, 5, 4), [0, 0, 0, 255]),
                COMPOSITE_ERASE,
            ),
        ] {
            let edited = painted(&cached, 1, &paint, mode);
            let mut fresh = edited.clone();
            fresh.layers[1].style = Some(Arc::new(parse(&json)));
            assert_eq!(
                edited.flattened().unwrap().into_raw(),
                fresh.flattened().unwrap().into_raw(),
                "{json}: {name}"
            );
        }
    }
}

// ------------------------------------------------------------------ .rz 94 --

#[test]
fn the_byte_word_94_round_trips_and_refuses_both_ways() {
    let dir = tempfile::TempDir::new().unwrap();
    let plain_path = dir.path().join("plain.rz");
    styled(&rect_doc(), 1, &style_of(&[stroke("outside", SIZE, "")]))
        .save_native(plain_path.to_str().unwrap())
        .unwrap();
    let plain = std::fs::read(&plain_path).unwrap();
    assert_ne!(
        &plain[8..12],
        &94u32.to_le_bytes(),
        "no 94 without the options"
    );
    for (name, extra) in [
        ("burst", "\"style\":\"shape_burst\""),
        ("dither", "\"style\":\"radial\",\"dither\":true"),
    ] {
        let doc = styled(&rect_doc(), 1, &style_of(&[stroke("center", SIZE, extra)]));
        let path = dir.path().join(format!("{name}.rz"));
        doc.save_native(path.to_str().unwrap()).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        assert_eq!(&bytes[4..8], &45u32.to_le_bytes());
        assert_eq!(
            &bytes[8..12],
            &94u32.to_le_bytes(),
            "{name}: FIRST inside 45"
        );
        assert_eq!(
            &bytes[12..16],
            &plain[8..12],
            "{name}: then the plain file's word"
        );
        let reopened = DrDocument::open(path.to_str().unwrap()).unwrap();
        assert_eq!(
            reopened.layers[1].style.as_deref().map(LayerStyle::to_json),
            doc.layers[1].style.as_deref().map(LayerStyle::to_json)
        );
        assert_eq!(flat(&reopened), flat(&doc), "{name}: renders equal");
        let mut stripped = bytes.clone();
        stripped.drain(8..12);
        let cut = dir.path().join(format!("{name}-cut.rz"));
        std::fs::write(&cut, &stripped).unwrap();
        let Err(message) = DrDocument::open(cut.to_str().unwrap()) else {
            panic!("{name}: the options without the word must refuse");
        };
        assert!(
            message.ends_with("native shape burst and dithered gradients require DRDC 94"),
            "{message}"
        );
    }
    let mut forged = plain.clone();
    forged.splice(8..8, 94u32.to_le_bytes());
    let path = dir.path().join("forged.rz");
    std::fs::write(&path, &forged).unwrap();
    let Err(message) = DrDocument::open(path.to_str().unwrap()) else {
        panic!("the word without the options must refuse");
    };
    assert!(
        message.ends_with("invalid native gradient options word"),
        "{message}"
    );
}

#[test]
fn a_comp_only_shape_burst_claims_the_word() {
    use doctored_core::doc_comps::{CompCaptures, CompRequest};
    let dir = tempfile::TempDir::new().unwrap();
    let doc = styled(
        &rect_doc(),
        1,
        &style_of(&[stroke("inside", SIZE, "\"style\":\"shape_burst\"")]),
    );
    let save = CompRequest::Save {
        name: "Burst".into(),
        comment: String::new(),
        captures: CompCaptures::default(),
    };
    let saved = doc.edit_layer_comps(&save).unwrap().unwrap();
    let cleared = try_styled(&saved, 1, None).unwrap().unwrap();
    assert!(cleared.layers[1].style.is_none());
    let path = dir.path().join("comp.rz");
    cleared.save_native(path.to_str().unwrap()).unwrap();
    let bytes = std::fs::read(&path).unwrap();
    assert_eq!(&bytes[8..12], &94u32.to_le_bytes());
    assert!(DrDocument::open(path.to_str().unwrap()).is_ok());
}
