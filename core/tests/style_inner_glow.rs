//! Inner glow (work item E3): an edge glow at full choke is the complement
//! of the shrink-rect closed form (a hard band inside the contour) whose
//! full pixels glow full at every Range and whose partial pixels keep today's
//! Divide (#2's Choke 100 % member), a centre glow at full choke below Size
//! 24 is one minus that (the member's mirror) and from Size 24 keeps today's
//! Range rule, soft and partly-choked glows are the measured law's box pair
//! (#2) over the hand-built grown inverse with its Range, a
//! centre glow of size 0 is the whole shape, colours
//! follow the W3C reference blend with the effect's OWN blend mode (also on
//! a Multiply layer), and the common cases: disabled / opacity 0 / edge
//! size 0 render nothing, a masked shape and an offset rect map, fill
//! opacity 0 keeps the glow, a transparent layer renders nothing, purity.
//! Black-box through the FFI setter (`common::styled`) and the public
//! projection; the oracles are analytic — never a golden image.

use std::ffi::c_int;

use doctored_core::doc::{BlendMode, DrDocument, MaskKind};
use image::{Rgba, RgbaImage};

use crate::common;
use common::*;

// --------------------------------------------------------------- fixture --

/// A 40x32 white canvas under a 16x12 rect at (10, 8): the rect is at least
/// 8 px from every canvas edge, so a size-4 glow's kernel (radius 5, 11
/// taps) never reads the border, and a full-choke band of 4 px still leaves
/// an 8x4 interior to prove the band stops.
const CANVAS: (u32, u32) = (40, 32);
const RECT: (i32, i32, u32, u32) = (10, 8, 16, 12);
/// A mid-tone rect colour: Screen with the default pale-yellow glow changes
/// every channel, so a wrong blend or opacity shows.
const NAVY: [u8; 4] = [40, 80, 160, 255];
/// The schema's default glow colour, `#ffffbe`.
const GLOW: [u8; 4] = [255, 255, 190, 255];
const DEFAULT_OPACITY: f32 = 0.75;

fn rect_doc() -> DrDocument {
    rect_layer_doc(CANVAS, RECT, NAVY)
}

/// An inner-glow style with `extra` spliced in as further keys (`,"k":v`).
fn glow_json(source: &str, choke: f32, size: f32, extra: &str) -> String {
    format!(
        "{{\"effects\":[{{\"type\":\"inner_glow\",\"source\":\"{source}\",\
         \"choke\":{choke},\"size\":{size}{extra}}}]}}"
    )
}

fn px(flat: &RgbaImage, x: u32, y: u32) -> [u8; 4] {
    flat.get_pixel(x, y).0
}

fn quantized(v: [f32; 4]) -> [u8; 4] {
    [
        (v[0] * 255.0).round() as u8,
        (v[1] * 255.0).round() as u8,
        (v[2] * 255.0).round() as u8,
        (v[3] * 255.0).round() as u8,
    ]
}

fn assert_px_close(got: [u8; 4], want: [u8; 4], what: &str) {
    assert_close(&got, &want, what);
}

fn in_rect(rect: (i32, i32, u32, u32), x: i64, y: i64) -> bool {
    let (x0, y0) = (i64::from(rect.0), i64::from(rect.1));
    x >= x0 && y >= y0 && x < x0 + i64::from(rect.2) && y < y0 + i64::from(rect.3)
}

/// For a pixel INSIDE `rect`, the distance to the nearest pixel outside it
/// — axis-aligned, because the complement of a rect is nearest along an
/// axis (tests.md §5.3): `min(x - x0 + 1, x1 - x, y - y0 + 1, y1 - y)`.
fn d_out(rect: (i32, i32, u32, u32), x: i64, y: i64) -> i64 {
    let (x0, y0) = (i64::from(rect.0), i64::from(rect.1));
    let (x1, y1) = (x0 + i64::from(rect.2), y0 + i64::from(rect.3));
    (x - x0 + 1).min(x1 - x).min(y - y0 + 1).min(y1 - y)
}

/// The rect shrunk by `r` at an inside pixel, from the header's contract
/// `coverage' = clamp(s - r + 0.5, 0, 1) * 255` with `s = d_out - 0.5`.
fn shrunk_rect(rect: (i32, i32, u32, u32), r: f32, x: i64, y: i64) -> u8 {
    ((d_out(rect, x, y) as f32 - r).clamp(0.0, 1.0) * 255.0).round() as u8
}

/// The rect's INVERSE grown by `r` at an inside pixel of the rect (the
/// pixel is outside the inverse, whose nearest inside pixel is the rect's
/// nearest outside one): `clamp(s + r + 0.5, 0, 1) * 255` with
/// `s = 0.5 - d_out`.
fn grown_inverse(rect: (i32, i32, u32, u32), r: f32, x: i64, y: i64) -> u8 {
    ((r + 1.0 - d_out(rect, x, y) as f32).clamp(0.0, 1.0) * 255.0).round() as u8
}

/// The measured law's `(n, w)` (#2, the runner's `law_split` in
/// `scripts/photoshop-probe/inner_glow_578_law_fit.py`): `S = round(size)`,
/// `n = floor(S · choke + 0.5)`, `w = S − n`, and a width of 1 acts as 2.
fn law_split(size: f32, choke: f32) -> (i64, i64) {
    let s = (f64::from(size) + 0.5).floor().max(1.0) as i64;
    let n = ((s as f64 * f64::from(choke) * 1e6).round() / 1e6 + 0.5).floor() as i64;
    let w = s - n;
    (n, if w == 1 { 2 } else { w })
}

/// The law's analytic oracle for the glow's coverage plane over the canvas
/// (#2), for a row with Size·(1 − Choke) > 0: (1) the rect's inverse grown
/// by `n` in closed form, for both sources; (2) the separable triangle
/// `(w − |j|) / w²`, edge-clamped (none at `w = 0`); (3) `min(1, B / r)` for
/// an edge glow, one minus that for a centre glow; (4) `floor(255 g + 0.5)`,
/// then clipped to the rect. The core renders on a layer-local padded
/// plane; the two agree wherever the triangle reaches no border, which the
/// fixture guarantees, and because the inverse is a constant 255 out to both
/// borders.
fn oracle_plane(
    rect: (i32, i32, u32, u32),
    source: &str,
    choke: f32,
    size: f32,
    range: f64,
) -> Vec<u8> {
    let (n, w) = law_split(size, choke);
    let (cw, ch) = (CANVAS.0 as i64, CANVAS.1 as i64);
    let band: Vec<f64> = (0..ch * cw)
        .map(|i| {
            let (x, y) = (i % cw, i / cw);
            if in_rect(rect, x, y) {
                f64::from(grown_inverse(rect, n as f32, x, y)) / 255.0
            } else {
                1.0
            }
        })
        .collect();
    let tap = |j: i64| (w - j.abs()) as f64 / (w * w) as f64;
    let blur = |plane: &[f64], horizontal: bool| -> Vec<f64> {
        if w == 0 {
            return plane.to_vec();
        }
        (0..ch * cw)
            .map(|i| {
                let (x, y) = (i % cw, i / cw);
                (-(w - 1)..w)
                    .map(|j| {
                        let (sx, sy) = if horizontal {
                            ((x + j).clamp(0, cw - 1), y)
                        } else {
                            (x, (y + j).clamp(0, ch - 1))
                        };
                        tap(j) * plane[(sy * cw + sx) as usize]
                    })
                    .sum()
            })
            .collect()
    };
    let b = blur(&blur(&band, true), false);
    (0..ch * cw)
        .map(|i| {
            let (x, y) = (i % cw, i / cw);
            if !in_rect(rect, x, y) {
                return 0;
            }
            let e = (b[i as usize] / range).min(1.0);
            let g = if source == "center" { 1.0 - e } else { e };
            (255.0 * g + 0.5).floor() as u8
        })
        .collect()
}

/// The glow at coverage `cov` over the (opaque) pixel colour `bg`, through
/// the W3C reference composite with the effect's own blend mode and
/// opacity, quantized once like the projection.
fn glow_over(bg: [u8; 4], glow: [u8; 4], cov: u8, opacity: f32, mode: c_int) -> [u8; 4] {
    quantized(ref_composite(
        to_unit(bg),
        to_unit(glow),
        opacity * f32::from(cov) / 255.0,
        mode,
    ))
}

// -------------------------------------------------------- closed forms --

#[test]
fn edge_glow_at_full_choke_is_the_shrink_rect_band() {
    // choke 1 at size 4: the inverse grown by 4 and no blur, i.e. exactly
    // the 4 px the rect loses when shrunk by 4 — full coverage where
    // d_out <= 4, nothing from d_out = 5 in.
    let flat = styled(&rect_doc(), 1, &glow_json("edge", 1.0, 4.0, ""))
        .flattened()
        .unwrap();
    let band_colour = glow_over(NAVY, GLOW, 255, DEFAULT_OPACITY, BLEND_SCREEN);
    assert_ne!(band_colour, NAVY, "the fixture's glow is visible");
    let (mut band, mut interior) = (0, 0);
    for y in 0..CANVAS.1 {
        for x in 0..CANVAS.0 {
            let got = px(&flat, x, y);
            let (xi, yi) = (i64::from(x), i64::from(y));
            if !in_rect(RECT, xi, yi) {
                assert_eq!(got, WHITE, "({x},{y}) outside the shape");
                continue;
            }
            let cov = 255 - shrunk_rect(RECT, 4.0, xi, yi);
            if cov == 0 {
                assert!(d_out(RECT, xi, yi) >= 5);
                assert_eq!(got, NAVY, "({x},{y}) interior is untouched");
                interior += 1;
            } else {
                assert_eq!(cov, 255, "a full choke has no soft ramp");
                assert!(d_out(RECT, xi, yi) <= 4);
                assert_px_close(got, band_colour, &format!("({x},{y}) band"));
                band += 1;
            }
        }
    }
    assert_eq!(interior, 8 * 4, "the 16x12 rect keeps an 8x4 interior");
    assert_eq!(band, 16 * 12 - 8 * 4);
}

#[test]
fn center_glow_at_size_zero_is_the_whole_shape() {
    let want = glow_over(NAVY, GLOW, 255, DEFAULT_OPACITY, BLEND_SCREEN);
    for choke in [0.0, 1.0] {
        let flat = styled(&rect_doc(), 1, &glow_json("center", choke, 0.0, ""))
            .flattened()
            .unwrap();
        for y in 0..CANVAS.1 {
            for x in 0..CANVAS.0 {
                let got = px(&flat, x, y);
                if in_rect(RECT, i64::from(x), i64::from(y)) {
                    assert_px_close(got, want, &format!("choke {choke} ({x},{y})"));
                } else {
                    assert_eq!(got, WHITE, "choke {choke} ({x},{y}) outside");
                }
            }
        }
    }
}

#[test]
fn center_glow_at_full_choke_is_the_shrunk_rect() {
    // choke 1 at size 4: the shape eroded by 4 and no blur — the glow is the
    // 8x4 interior at full coverage and the 4 px band is untouched.
    let flat = styled(&rect_doc(), 1, &glow_json("center", 1.0, 4.0, ""))
        .flattened()
        .unwrap();
    let want = glow_over(NAVY, GLOW, 255, DEFAULT_OPACITY, BLEND_SCREEN);
    let mut glowing = 0;
    for y in 0..CANVAS.1 {
        for x in 0..CANVAS.0 {
            let got = px(&flat, x, y);
            let (xi, yi) = (i64::from(x), i64::from(y));
            if !in_rect(RECT, xi, yi) {
                assert_eq!(got, WHITE);
            } else if d_out(RECT, xi, yi) >= 5 {
                assert_px_close(got, want, &format!("({x},{y}) plateau"));
                glowing += 1;
            } else {
                assert_eq!(got, NAVY, "({x},{y}) the eroded band is untouched");
            }
        }
    }
    assert_eq!(glowing, 8 * 4);
}

// ----------------------------------------------------------- law oracle --

#[test]
fn soft_and_partly_choked_glows_match_the_box_law_oracle() {
    // Size 3: choke 0 -> (n, w) = (0, 3), choke 0.5 -> (2, 2) (a width of 1
    // acts as 2); size 4: (0, 4) and (2, 2). Both sources, Range 50 % and
    // 100 %. The opacity is its byte (`byte_unit`).
    let opacity = byte_unit(f64::from(DEFAULT_OPACITY)) as f32;
    for source in ["edge", "center"] {
        for size in [3.0f32, 4.0] {
            for range in [0.5f64, 1.0] {
                let mut contours = Vec::new();
                for choke in [0.0f32, 0.5] {
                    let extra = format!(",\"range\":{range}");
                    let flat = styled(&rect_doc(), 1, &glow_json(source, choke, size, &extra))
                        .flattened()
                        .unwrap();
                    let plane = oracle_plane(RECT, source, choke, size, range);
                    let mut soft = 0;
                    for y in 0..CANVAS.1 {
                        for x in 0..CANVAS.0 {
                            let got = px(&flat, x, y);
                            let what = format!(
                                "{source} size {size} range {range} choke {choke} ({x},{y})"
                            );
                            if !in_rect(RECT, i64::from(x), i64::from(y)) {
                                assert_eq!(got, WHITE, "{what} outside");
                                continue;
                            }
                            let cov = plane[(y * CANVAS.0 + x) as usize];
                            let want = glow_over(NAVY, GLOW, cov, opacity, BLEND_SCREEN);
                            assert_px_close(got, want, &format!("{what} coverage {cov}"));
                            assert_eq!(got[3], 255, "{what} alpha");
                            if cov != 0 && cov != 255 {
                                soft += 1;
                            }
                        }
                    }
                    assert!(
                        soft > 0,
                        "{source} size {size} range {range} choke {choke}: a ramp"
                    );
                    contours.push(plane[(RECT.1 as u32 * CANVAS.0 + RECT.0 as u32 + 8) as usize]);
                }
                // Choke hardens the glow at the contour: for an edge glow the
                // band it adds raises the contour pixel; for a centre glow,
                // one minus the edge, it drops.
                let (unchoked, choked) = (contours[0], contours[1]);
                let what = format!("{source} size {size} range {range}");
                match source {
                    "edge" => assert!(choked > unchoked, "{what}: {unchoked} -> {choked}"),
                    _ => assert!(choked < unchoked, "{what}: {unchoked} -> {choked}"),
                }
            }
        }
    }
}

#[test]
fn full_choke_edge_band_glows_full_at_every_range() {
    // At Choke 100 % Size·(1 − Choke) is 0. An edge glow runs the Choke 100 %
    // member (#2, identify take2): a pixel fully inside the band glows full
    // at every Range (today's Divide halved it to 128 at Range 100 %). This
    // rect's band has no partial pixel. A centre glow below Size 24 runs the
    // member's mirror, one minus the edge: the plateau inside the band is
    // full at every Range and the band is 0.
    let opacity = byte_unit(f64::from(DEFAULT_OPACITY)) as f32;
    for range in [0.25f64, 0.5, 0.75, 1.0] {
        let extra = format!(",\"range\":{range}");
        let flat = styled(&rect_doc(), 1, &glow_json("edge", 1.0, 4.0, &extra))
            .flattened()
            .unwrap();
        let band_colour = glow_over(NAVY, GLOW, 255, opacity, BLEND_SCREEN);
        let mut band = 0;
        for y in 0..CANVAS.1 {
            for x in 0..CANVAS.0 {
                let got = px(&flat, x, y);
                let (xi, yi) = (i64::from(x), i64::from(y));
                if !in_rect(RECT, xi, yi) {
                    assert_eq!(got, WHITE, "range {range} ({x},{y}) outside");
                } else if d_out(RECT, xi, yi) <= 4 {
                    assert_px_close(got, band_colour, &format!("range {range} ({x},{y}) band"));
                    band += 1;
                } else {
                    assert_eq!(got, NAVY, "range {range} ({x},{y}) interior");
                }
            }
        }
        assert_eq!(band, 16 * 12 - 8 * 4, "range {range}");
    }
    let plateau = glow_over(NAVY, GLOW, 255, opacity, BLEND_SCREEN);
    for range in [0.25f64, 0.5, 0.75, 1.0] {
        let extra = format!(",\"range\":{range}");
        let flat = styled(&rect_doc(), 1, &glow_json("center", 1.0, 4.0, &extra))
            .flattened()
            .unwrap();
        let mut full = 0;
        for y in 0..CANVAS.1 {
            for x in 0..CANVAS.0 {
                let got = px(&flat, x, y);
                let (xi, yi) = (i64::from(x), i64::from(y));
                if !in_rect(RECT, xi, yi) {
                    assert_eq!(got, WHITE, "centre range {range} ({x},{y}) outside");
                } else if d_out(RECT, xi, yi) >= 5 {
                    assert_px_close(
                        got,
                        plateau,
                        &format!("centre range {range} ({x},{y}) plateau full"),
                    );
                    full += 1;
                } else {
                    assert_eq!(got, NAVY, "centre range {range} ({x},{y}) band is 0");
                }
            }
        }
        assert_eq!(full, 8 * 4, "centre range {range}");
    }
}

#[test]
fn full_choke_centre_keeps_todays_kernel_from_size_24() {
    // From Size 24 a centre glow at Choke 100 % keeps today's kernel (#2's
    // CHOKE 100 CENTRE SPLIT): the shape eroded by 24 px, no blur, through
    // today's Divide, which halves the plateau at Range 100 % (255 / 2 ->
    // 128). The rect's sides are at least 51 px so an 8x4 plateau survives
    // the 24 px erosion on every side.
    let canvas = (76u32, 68u32);
    let rect = (10, 8, 56u32, 52u32);
    let doc = rect_layer_doc(canvas, rect, NAVY);
    let opacity = byte_unit(f64::from(DEFAULT_OPACITY)) as f32;
    let flat = styled(&doc, 1, &glow_json("center", 1.0, 24.0, ",\"range\":1"))
        .flattened()
        .unwrap();
    let plateau = glow_over(NAVY, GLOW, 128, opacity, BLEND_SCREEN);
    let mut glowing = 0;
    for y in 0..canvas.1 {
        for x in 0..canvas.0 {
            let got = px(&flat, x, y);
            let (xi, yi) = (i64::from(x), i64::from(y));
            if !in_rect(rect, xi, yi) {
                assert_eq!(got, WHITE, "({x},{y}) outside");
            } else if d_out(rect, xi, yi) >= 25 {
                assert_px_close(got, plateau, &format!("({x},{y}) plateau at 128"));
                glowing += 1;
            } else {
                assert_eq!(got, NAVY, "({x},{y}) the eroded band is untouched");
            }
        }
    }
    assert_eq!(glowing, 8 * 4, "the 56x52 rect keeps an 8x4 plateau");
}

#[test]
fn full_choke_partial_pixels_keep_todays_divide() {
    // A binary disc's band (Size 3) has partial pixels where the Euclidean
    // distance to the nearest outside pixel centre is not an integer. The
    // oracle restates the member (#2): the band byte `round(255 clamp(S + 1
    // − d))`, then `e` = 1 where it is full and today's Divide `min(1, b /
    // 2r)` elsewhere, unrounded; the edge glow is `e` and the centre glow `1 −
    // e`, each `floor(255 g + 0.5)` once.
    let (cx, cy, r2) = (20.0f64, 16.0f64, 81.0f64);
    let inside = |x: i64, y: i64| {
        let (dx, dy) = (x as f64 + 0.5 - cx, y as f64 + 0.5 - cy);
        dx * dx + dy * dy <= r2
    };
    let (cw, ch) = (i64::from(CANVAS.0), i64::from(CANVAS.1));
    let layer = RgbaImage::from_fn(CANVAS.0, CANVAS.1, |x, y| {
        if inside(i64::from(x), i64::from(y)) {
            Rgba(NAVY)
        } else {
            Rgba([0, 0, 0, 0])
        }
    });
    let doc = DrDocument::from_pixels(solid(CANVAS.0, CANVAS.1, WHITE))
        .adding_image_layer(0, layer, "Disc")
        .expect("disc layer");
    let opacity = byte_unit(f64::from(DEFAULT_OPACITY)) as f32;
    let size = 3.0f64;
    for (source, range) in [
        ("edge", 0.75f64),
        ("edge", 1.0),
        ("center", 0.75),
        ("center", 1.0),
    ] {
        let extra = format!(",\"range\":{range}");
        let flat = styled(&doc, 1, &glow_json(source, 1.0, size as f32, &extra))
            .flattened()
            .unwrap();
        let mut partial = 0;
        for y in 0..ch {
            for x in 0..cw {
                let got = px(&flat, x as u32, y as u32);
                if !inside(x, y) {
                    assert_eq!(got, WHITE, "{source} range {range} ({x},{y}) outside");
                    continue;
                }
                let mut d2 = f64::INFINITY;
                for oy in 0..ch {
                    for ox in 0..cw {
                        if !inside(ox, oy) {
                            d2 = d2.min(((ox - x) * (ox - x) + (oy - y) * (oy - y)) as f64);
                        }
                    }
                }
                let b = ((size + 1.0 - d2.sqrt()).clamp(0.0, 1.0) * 255.0).round();
                let e = if b == 255.0 {
                    1.0
                } else {
                    (b / 255.0 / (2.0 * range)).min(1.0)
                };
                let g = if source == "center" { 1.0 - e } else { e };
                let cov = (255.0 * g + 0.5).floor();
                if b > 0.0 && b < 255.0 {
                    partial += 1;
                }
                let want = glow_over(NAVY, GLOW, cov as u8, opacity, BLEND_SCREEN);
                assert_px_close(
                    got,
                    want,
                    &format!("{source} range {range} ({x},{y}) band {b}"),
                );
            }
        }
        assert!(
            partial > 0,
            "{source} range {range}: the disc's band has partial pixels"
        );
    }
}

#[test]
fn edge_glow_of_size_zero_renders_nothing() {
    // A glow with no extent draws nothing — including along anti-aliased
    // edges, where inverse x coverage would otherwise leave a faint halo.
    let mut soft = solid(RECT.2, RECT.3, NAVY);
    for y in 0..RECT.3 {
        for x in 0..RECT.2 {
            if x == 0 || y == 0 || x == RECT.2 - 1 || y == RECT.3 - 1 {
                soft.put_pixel(x, y, Rgba([NAVY[0], NAVY[1], NAVY[2], 128]));
            }
        }
    }
    let base = rect_doc()
        .with_layer_pixels(1, soft)
        .expect("soft-edged rect");
    let plain = base.flattened().unwrap().into_raw();
    for choke in [0.0, 1.0] {
        let doc = styled(&base, 1, &glow_json("edge", choke, 0.0, ""));
        assert!(doc.layers[1].style.is_some(), "an enabled effect is stored");
        assert_eq!(doc.flattened().unwrap().into_raw(), plain, "choke {choke}");
    }
}

// ----------------------------------------------------------- blending --

#[test]
fn glow_blend_mode_and_opacity_follow_the_reference() {
    let grey = [128, 128, 128, 255];
    let mut seen = Vec::new();
    for (name, mode) in [
        ("normal", BLEND_NORMAL),
        ("multiply", BLEND_MULTIPLY),
        ("screen", BLEND_SCREEN),
    ] {
        let extra = format!(",\"blend\":\"{name}\",\"color\":\"#808080\",\"opacity\":0.6");
        let flat = styled(&rect_doc(), 1, &glow_json("center", 0.0, 0.0, &extra))
            .flattened()
            .unwrap();
        let want = glow_over(NAVY, grey, 255, 0.6, mode);
        assert_px_close(px(&flat, 17, 13), want, name);
        assert_eq!(px(&flat, 5, 5), WHITE, "{name}: outside untouched");
        seen.push(want);
    }
    assert!(
        seen[0] != seen[1] && seen[1] != seen[2] && seen[0] != seen[2],
        "the three modes are distinguishable on this fixture: {seen:?}"
    );
}

#[test]
fn layer_opacity_mixes_the_packaged_pixels_and_glow() {
    // The layer opacity applies once, to the package of pixels and glow:
    // the glow at its own opacity over the opaque
    // pixels, then that result mixed into the backdrop at the layer's 0.5
    // — never the glow thinned by 0.5 over pixels thinned by 0.5.
    let base = rect_doc().with_layer_opacity(1, 0.5).expect("opacity");
    let flat = styled(&base, 1, &glow_json("center", 0.0, 0.0, ""))
        .flattened()
        .unwrap();
    let pixels = ref_composite(to_unit(WHITE), to_unit(NAVY), 1.0, BLEND_NORMAL);
    let package = ref_composite(pixels, to_unit(GLOW), DEFAULT_OPACITY, BLEND_SCREEN);
    let white = to_unit(WHITE);
    let want = quantized([
        white[0] + (package[0] - white[0]) * 0.5,
        white[1] + (package[1] - white[1]) * 0.5,
        white[2] + (package[2] - white[2]) * 0.5,
        1.0,
    ]);
    assert_px_close(px(&flat, 17, 13), want, "half-opacity layer");
    assert_ne!(
        px(&flat, 17, 13),
        glow_over(NAVY, GLOW, 255, DEFAULT_OPACITY, BLEND_SCREEN),
        "the layer opacity reached the glow"
    );
    let sequential = quantized(ref_composite(
        ref_composite(white, to_unit(NAVY), 0.5, BLEND_NORMAL),
        to_unit(GLOW),
        DEFAULT_OPACITY * 0.5,
        BLEND_SCREEN,
    ));
    assert_ne!(
        px(&flat, 17, 13),
        sequential,
        "the per-contribution model differs on this fixture"
    );
}

#[test]
fn the_glow_keeps_its_own_blend_on_a_multiply_layer() {
    // A Multiply-blend RED layer over grey composites to dark red; a white
    // Normal glow at opacity 1 above it must show white (its OWN blend, not
    // the layer's — "Blend Interior Effects as Group" is off). Through the
    // layer's Multiply it would stay dark red.
    let grey = [128, 128, 128, 255];
    let base = DrDocument::from_pixels(solid(CANVAS.0, CANVAS.1, grey))
        .adding_image_layer(0, solid(RECT.2, RECT.3, RED), "Rect")
        .expect("add layer")
        .with_layer_offset(1, RECT.0, RECT.1)
        .expect("offset")
        .with_layer_blend_mode(1, BlendMode::Multiply)
        .expect("blend");
    let pixels_only = quantized(ref_composite(
        to_unit(grey),
        to_unit(RED),
        1.0,
        BLEND_MULTIPLY,
    ));
    assert_eq!(base.flattened().unwrap().get_pixel(17, 13).0, pixels_only);
    let flat = styled(
        &base,
        1,
        &glow_json(
            "center",
            0.0,
            0.0,
            ",\"blend\":\"normal\",\"color\":\"#ffffff\",\"opacity\":1",
        ),
    )
    .flattened()
    .unwrap();
    for y in 0..CANVAS.1 {
        for x in 0..CANVAS.0 {
            let got = px(&flat, x, y);
            if in_rect(RECT, i64::from(x), i64::from(y)) {
                assert_px_close(got, WHITE, &format!("({x},{y}) white through Normal"));
            } else {
                assert_eq!(got, grey, "({x},{y}) outside");
            }
        }
    }
}

// --------------------------------------------------------- common cases --

#[test]
fn disabled_and_opacity_zero_render_nothing() {
    let base = rect_doc();
    let plain = base.flattened().unwrap().into_raw();
    let off = styled(&base, 1, &glow_json("edge", 0.0, 5.0, ",\"opacity\":0"));
    assert_eq!(off.flattened().unwrap().into_raw(), plain, "opacity 0");
    let disabled = styled(
        &base,
        1,
        "{\"fill_opacity\":0.5,\"effects\":[{\"type\":\"inner_glow\",\"enabled\":false}]}",
    );
    assert_eq!(
        disabled.flattened().unwrap().into_raw(),
        base.with_layer_opacity(1, 0.5)
            .expect("opacity")
            .flattened()
            .unwrap()
            .into_raw(),
        "a disabled effect renders nothing"
    );
}

#[test]
fn the_glow_follows_the_masked_shape() {
    // A mask hiding the rect's right 4 columns: the shape is 12x12 and the
    // band hugs the MASK's edge; the hidden columns show the canvas.
    let visible = (RECT.0, RECT.1, RECT.2 - 4, RECT.3);
    let sel = selection(CANVAS.0, CANVAS.1, |x, y| {
        u8::from(in_rect(visible, i64::from(x), i64::from(y))) * 255
    });
    let base = rect_doc()
        .add_mask(1, MaskKind::FromSelection(&sel))
        .expect("mask");
    let flat = styled(&base, 1, &glow_json("edge", 1.0, 4.0, ""))
        .flattened()
        .unwrap();
    let band_colour = glow_over(NAVY, GLOW, 255, DEFAULT_OPACITY, BLEND_SCREEN);
    let mut interior = 0;
    for y in 0..CANVAS.1 {
        for x in 0..CANVAS.0 {
            let got = px(&flat, x, y);
            let (xi, yi) = (i64::from(x), i64::from(y));
            if !in_rect(visible, xi, yi) {
                assert_eq!(got, WHITE, "({x},{y}) hidden or outside");
            } else if d_out(visible, xi, yi) <= 4 {
                assert_px_close(got, band_colour, &format!("({x},{y}) band"));
            } else {
                assert_eq!(got, NAVY, "({x},{y}) interior");
                interior += 1;
            }
        }
    }
    assert_eq!(
        interior,
        4 * 4,
        "the 12x12 visible shape keeps a 4x4 interior"
    );
}

#[test]
fn an_offset_rect_partly_off_canvas_bands_along_its_true_edges() {
    // The band is measured from the rect's real edges, off-canvas ones
    // included: only the corner nearest the canvas corner is interior.
    for (rect, interior_px) in [
        ((-6, -4, RECT.2, RECT.3), 6 * 4),
        ((30, 24, RECT.2, RECT.3), 6 * 4),
    ] {
        let base = rect_layer_doc(CANVAS, rect, NAVY);
        let flat = styled(&base, 1, &glow_json("edge", 1.0, 4.0, ""))
            .flattened()
            .unwrap();
        let band_colour = glow_over(NAVY, GLOW, 255, DEFAULT_OPACITY, BLEND_SCREEN);
        let mut interior = 0;
        for y in 0..CANVAS.1 {
            for x in 0..CANVAS.0 {
                let got = px(&flat, x, y);
                let (xi, yi) = (i64::from(x), i64::from(y));
                let what = format!("rect at ({},{}) ({x},{y})", rect.0, rect.1);
                if !in_rect(rect, xi, yi) {
                    assert_eq!(got, WHITE, "{what} outside");
                } else if d_out(rect, xi, yi) <= 4 {
                    assert_px_close(got, band_colour, &format!("{what} band"));
                } else {
                    assert_eq!(got, NAVY, "{what} interior");
                    interior += 1;
                }
            }
        }
        assert_eq!(interior, interior_px, "rect at ({},{})", rect.0, rect.1);
    }
}

#[test]
fn fill_opacity_zero_keeps_the_glow() {
    // The pixels vanish, the effect stays: a Normal glow at opacity 1 IS the
    // glow colour on the band, and the canvas shows through the interior.
    let flat = styled(
        &rect_doc(),
        1,
        "{\"fill_opacity\":0,\"effects\":[{\"type\":\"inner_glow\",\"source\":\"edge\",\
         \"choke\":1,\"size\":4,\"blend\":\"normal\",\"opacity\":1}]}",
    )
    .flattened()
    .unwrap();
    for y in 0..CANVAS.1 {
        for x in 0..CANVAS.0 {
            let got = px(&flat, x, y);
            let (xi, yi) = (i64::from(x), i64::from(y));
            let want = if in_rect(RECT, xi, yi) && d_out(RECT, xi, yi) <= 4 {
                GLOW
            } else {
                WHITE
            };
            assert_eq!(got, want, "({x},{y})");
        }
    }
}

#[test]
fn transparent_and_one_pixel_layers_render_sanely() {
    let white = solid(CANVAS.0, CANVAS.1, WHITE).into_raw();
    let clear = DrDocument::from_pixels(solid(CANVAS.0, CANVAS.1, WHITE))
        .adding_image_layer(0, solid(RECT.2, RECT.3, [0, 0, 0, 0]), "Clear")
        .expect("add layer")
        .with_layer_offset(1, RECT.0, RECT.1)
        .expect("offset");
    for source in ["edge", "center"] {
        let doc = styled(&clear, 1, &glow_json(source, 0.5, 4.0, ""));
        assert_eq!(
            doc.flattened().unwrap().into_raw(),
            white,
            "{source}: a transparent shape"
        );
    }
    let dot = rect_layer_doc(CANVAS, (5, 5, 1, 1), NAVY);
    let hard = styled(&dot, 1, &glow_json("edge", 1.0, 4.0, ""))
        .flattened()
        .unwrap();
    assert_px_close(
        px(&hard, 5, 5),
        glow_over(NAVY, GLOW, 255, DEFAULT_OPACITY, BLEND_SCREEN),
        "a lone pixel is all band",
    );
    assert_eq!(px(&hard, 6, 5), WHITE);
    let soft = styled(&dot, 1, &glow_json("center", 0.0, 4.0, ""))
        .flattened()
        .unwrap();
    assert_eq!(px(&soft, 5, 5)[3], 255);
    assert_eq!(px(&soft, 4, 5), WHITE, "a glow never leaves the shape");
    assert_eq!(px(&soft, 5, 6), WHITE);
}

#[test]
fn styling_is_pure() {
    let base = rect_doc();
    let before = base.flattened().unwrap().into_raw();
    let doc = styled(&base, 1, &glow_json("edge", 0.5, 6.0, ""));
    assert!(doc.layers[1].style.is_some());
    assert!(
        base.layers[1].style.is_none(),
        "the input document is untouched"
    );
    assert_eq!(base.flattened().unwrap().into_raw(), before);
    assert_ne!(
        doc.flattened().unwrap().into_raw(),
        before,
        "the glow rendered"
    );
}
