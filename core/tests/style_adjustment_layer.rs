//! A layer style and Blend If on an 8-bit ADJUSTMENT layer
//! (`style_adjustment`). The rule is Photoshop's as the take measured
//! it (`scripts/photoshop-probe/styles_scope_558_take.py`; S-MASK and POST
//! fitted from it): the effects hang off the layer's MASK over the canvas
//! (S-MASK), the adjustment reads the accumulator after its own Below
//! effects (POST) and is applied in place with alpha kept (a 50 % backdrop
//! stays at 50 %: ALPHA-KEPT), Fill scales `k`, This Layer reads the adjusted
//! colour, and a styled base's clipped members are frozen to that shape
//! (C-SHAPE).
//!
//! The oracles are written here from the rules: the interior in f64 as the
//! unstyled adjustment's lerp with Invert's `1 − c` and the Blend If ramp
//! table restated; POST as the unstyled adjustment over the backdrop with
//! the effects alone drawn on it (the same style at Fill 0 on an opaque
//! raster under the same mask, whose shape is the S-MASK one); the package
//! as the premultiplied mix of the opacity-1 projection. Tolerance
//! (declared before measuring): ±1 code per channel against an f64 oracle
//! or a two-step one, exact where both sides run the same statements.

use std::ffi::c_char;
use std::ptr;
use std::sync::Arc;

use doctored_core::doc::{BlendMode, DrDocument};
use doctored_core::ffi::dr_image_free;
use doctored_core::ffi_doc::{dr_doc_free, dr_doc_open, dr_doc_save_native};
use doctored_core::ffi_regional::dr_doc_render_region;
use image::{GrayImage, Luma, Rgba, RgbaImage};
use tempfile::TempDir;

use crate::common::{adjust_meta, cpath, ffi_style, styled, take_err_string, try_styled};

const W: u32 = 24;
const H: u32 = 16;
/// `(x, y, w, h)`: a transparent hole, a 50 % band beside it, the mask rect.
const HOLE: (u32, u32, u32, u32) = (2, 5, 4, 4);
const BAND: (u32, u32, u32, u32) = (6, 5, 4, 4);
const MASK: (u32, u32, u32, u32) = (4, 2, 16, 12);
/// The mask's one partial column (128) inside `MASK`.
const PARTIAL_COLUMN: u32 = 12;

const STROKE: &str = r##"{"effects":[{"type":"stroke","size":2,"color":"#ff0000"}]}"##;
const SHADOW: &str = r#"{"effects":[{"type":"drop_shadow","opacity":1,"distance":3,"size":0,"angle":180,"use_global_light":false,"layer_knocks_out":false}]}"#;
const OVERLAY: &str = r##"{"effects":[{"type":"color_overlay","color":"#0000ff"}]}"##;

fn inside(x: u32, y: u32, (rx, ry, rw, rh): (u32, u32, u32, u32)) -> bool {
    x >= rx && x < rx + rw && y >= ry && y < ry + rh
}

/// A chromatic backdrop (every channel different, so Multiply and Gray read
/// distinct values) with the hole and the 50 % band.
fn backdrop_px(x: u32, y: u32) -> [u8; 4] {
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

fn mask_at(x: u32, y: u32) -> u8 {
    if !inside(x, y, MASK) {
        0
    } else if x == PARTIAL_COLUMN {
        128
    } else {
        255
    }
}

fn backdrop() -> RgbaImage {
    RgbaImage::from_fn(W, H, |x, y| Rgba(backdrop_px(x, y)))
}

fn mask_image() -> Arc<GrayImage> {
    Arc::new(GrayImage::from_fn(W, H, |x, y| Luma([mask_at(x, y)])))
}

/// `image` under a canvas-sized Invert adjustment layer "Adj" (index 1),
/// masked by `mask_at` when `masked`.
fn scene_over(image: RgbaImage, masked: bool) -> DrDocument {
    let mut doc = DrDocument::from_pixels(image)
        .adding_image_layer(0, RgbaImage::new(W, H), "Adj")
        .expect("add the adjustment layer");
    doc.layers[1].meta = Some(adjust_meta("invert", "{}"));
    if masked {
        doc.layers[1].mask = Some(mask_image());
        doc.layers[1].mask_enabled = true;
    }
    doc
}

fn scene(masked: bool) -> DrDocument {
    scene_over(backdrop(), masked)
}

/// Invert of the backdrop, alpha kept: the oracle's adjusted pixels.
fn adjusted_px(x: u32, y: u32) -> [u8; 4] {
    let b = backdrop_px(x, y);
    if b[3] == 0 {
        return [0; 4];
    }
    [255 - b[0], 255 - b[1], 255 - b[2], b[3]]
}

// ---------------------------------------------------------------- oracle --

fn unit(px: [u8; 4]) -> [f64; 4] {
    px.map(|v| f64::from(v) / 255.0)
}

fn blend64(mode: BlendMode, cb: f64, cs: f64) -> f64 {
    match mode {
        BlendMode::Normal => cs,
        BlendMode::Multiply => cb * cs,
        other => panic!("no oracle for {other:?}"),
    }
}

/// The Blend If ramp table (`style_blend_if`'s module doc) in f64.
fn ramp(v: i64, [lo0, lo1, hi0, hi1]: [i64; 4]) -> f64 {
    if v < lo0 {
        0.0
    } else if v < lo1 {
        (v - lo0) as f64 / (lo1 - lo0) as f64
    } else if v <= hi0 {
        1.0
    } else if v <= hi1 {
        (hi1 - v) as f64 / (hi1 - hi0) as f64
    } else {
        0.0
    }
}

/// Photoshop's Gray of 8-bit codes: `round(0.30 R + 0.59 G + 0.11 B)`.
fn gray(px: [u8; 4]) -> i64 {
    (0.30 * f64::from(px[0]) + 0.59 * f64::from(px[1]) + 0.11 * f64::from(px[2])).round() as i64
}

struct Case {
    fill: f64,
    opacity: f64,
    mode: BlendMode,
    masked: bool,
    this: [i64; 4],
    under: [i64; 4],
}

const FULL: [i64; 4] = [0, 0, 255, 255];

/// The styled adjustment's interior at one pixel (ALPHA-KEPT): the unstyled
/// adjustment's lerp toward the blended adjusted colour at `k = mask × fill
/// × Blend If × opacity`, alpha kept, a transparent pixel untouched.
fn oracle(case: &Case, x: u32, y: u32) -> [f64; 4] {
    let b = backdrop_px(x, y);
    if b[3] == 0 {
        return unit(b);
    }
    let a = adjusted_px(x, y);
    let m = if case.masked {
        f64::from(mask_at(x, y)) / 255.0
    } else {
        1.0
    };
    let w = ramp(gray(a), case.this) * ramp(gray(b), case.under);
    let k = m * case.fill * w * case.opacity;
    let (ub, ua) = (unit(b), unit(a));
    let mut out = ub;
    for c in 0..3 {
        out[c] = ub[c] + (blend64(case.mode, ub[c], ua[c]) - ub[c]) * k;
    }
    out
}

fn close(got: [u8; 4], want: [f64; 4], what: &str) {
    let want = want.map(|v| v * 255.0);
    let channels = if want[3] < 0.5 { 3..4 } else { 0..4 };
    for c in channels {
        assert!(
            (f64::from(got[c]) - want[c]).abs() <= 1.0,
            "{what}: channel {c} got {got:?} want {want:?}"
        );
    }
}

/// Two projections within 1 code per channel (alpha alone where both are
/// transparent).
fn close_images(got: &RgbaImage, want: &RgbaImage, what: &str) {
    for (x, y, g) in got.enumerate_pixels() {
        let w = want.get_pixel(x, y).0;
        let channels = if g[3] == 0 && w[3] == 0 { 3..4 } else { 0..4 };
        for c in channels {
            assert!(
                g.0[c].abs_diff(w[c]) <= 1,
                "{what} at ({x}, {y}): {:?} vs {w:?}",
                g.0
            );
        }
    }
}

fn configured(case: &Case, json: &str) -> DrDocument {
    let mut doc = scene(case.masked);
    if case.opacity != 1.0 {
        doc = doc.with_layer_opacity(1, case.opacity as f32).unwrap();
    }
    if case.mode != BlendMode::Normal {
        doc = doc.with_layer_blend_mode(1, case.mode).unwrap();
    }
    styled(&doc, 1, json)
}

fn blend_if(this: [i64; 4], under: [i64; 4]) -> String {
    format!(
        "{{\"blend_if\":{{\"channel\":\"gray\",\"this_layer\":{this:?},\"underlying\":{under:?}}}}}"
    )
}

/// `json` with Fill 0: the effects alone.
fn effects_only(json: &str) -> String {
    format!("{{\"fill_opacity\":0,{}", &json[1..])
}

/// The backdrop with `json`'s effects alone drawn on it, as an OPAQUE raster
/// under the same mask casts them: the S-MASK shape.
fn backdrop_with_effects(json: &str, masked: bool) -> RgbaImage {
    let mut doc = DrDocument::from_pixels(backdrop())
        .adding_image_layer(
            0,
            RgbaImage::from_pixel(W, H, Rgba([90, 40, 200, 255])),
            "Shape",
        )
        .unwrap();
    if masked {
        doc.layers[1].mask = Some(mask_image());
        doc.layers[1].mask_enabled = true;
    }
    styled(&doc, 1, &effects_only(json)).flattened().unwrap()
}

// ----------------------------------------------------------------- tests --

#[test]
fn the_interior_matches_the_f64_oracle() {
    let cases = [
        (
            "fill 50 %",
            Case {
                fill: 0.5,
                opacity: 1.0,
                mode: BlendMode::Normal,
                masked: false,
                this: FULL,
                under: FULL,
            },
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
            Case {
                fill: 1.0,
                opacity: 1.0,
                mode: BlendMode::Normal,
                masked: true,
                this: [0, 0, 128, 128],
                under: FULL,
            },
            blend_if([0, 0, 128, 128], FULL),
        ),
        (
            "Underlying reads the composite beneath",
            Case {
                fill: 1.0,
                opacity: 1.0,
                mode: BlendMode::Normal,
                masked: false,
                this: FULL,
                under: [128, 128, 255, 255],
            },
            blend_if(FULL, [128, 128, 255, 255]),
        ),
    ];
    for (what, case, json) in cases {
        let flat = configured(&case, &json).flattened().unwrap();
        let mut shown = 0;
        for y in 0..H {
            for x in 0..W {
                let want = oracle(&case, x, y);
                close(
                    flat.get_pixel(x, y).0,
                    want,
                    &format!("{what} at ({x}, {y})"),
                );
                shown += usize::from(want[3] > 0.0);
            }
        }
        assert!(shown > 0, "{what}");
    }
    // The two This Layer readings disagree somewhere, so the case above
    // pins T-ADJUSTED rather than passing for either.
    assert!((0..W).any(|x| {
        let (a, b) = (adjusted_px(x, 0), backdrop_px(x, 0));
        (gray(a) <= 128) != (gray(b) <= 128)
    }));
}

#[test]
fn the_shape_is_the_mask_over_the_canvas() {
    // S-MASK: an unmasked adjustment's shape is the whole canvas, so its
    // outside Stroke falls off it and nothing rings the hole — the
    // projection is the unstyled adjustment's, byte for byte.
    let stroked = styled(&scene(false), 1, STROKE).flattened().unwrap();
    assert_eq!(stroked.as_raw(), scene(false).flattened().unwrap().as_raw());
    // A Color Overlay covers the mask over the canvas, the hole and the band
    // included; outside the mask the backdrop is untouched.
    let overlaid = styled(&scene(true), 1, OVERLAY).flattened().unwrap();
    for y in 0..H {
        for x in 0..W {
            let got = overlaid.get_pixel(x, y).0;
            match mask_at(x, y) {
                255 => assert_eq!(got, [0, 0, 255, 255], "overlay at ({x}, {y})"),
                0 if backdrop_px(x, y)[3] == 0 => assert_eq!(got[3], 0, "hole at ({x}, {y})"),
                0 => assert_eq!(got, backdrop_px(x, y), "backdrop at ({x}, {y})"),
                _ => {}
            }
        }
    }
}

#[test]
fn the_adjustment_reads_its_own_below_effects() {
    // POST: the projection is the unstyled adjustment over the backdrop with
    // the effects alone drawn on it — under its own Drop Shadow the
    // adjustment inverts the shadow, and a hole the shadow lands in turns
    // opaque before it is adjusted.
    for json in [SHADOW, STROKE] {
        for masked in [true, false] {
            let got = styled(&scene(masked), 1, json).flattened().unwrap();
            let want = scene_over(backdrop_with_effects(json, masked), masked)
                .flattened()
                .unwrap();
            close_images(&got, &want, &format!("{json} masked {masked}"));
        }
    }
    // The shadow does land inside the shape there, so the rule is pinned.
    let shadowed = styled(&scene(true), 1, SHADOW).flattened().unwrap();
    let pre = scene(true).flattened().unwrap();
    assert_ne!(shadowed.get_pixel(10, 8), pre.get_pixel(10, 8));
    assert_eq!(
        shadowed.get_pixel(10, 8).0,
        [255, 255, 255, 255],
        "Invert(black)"
    );
}

#[test]
fn layer_opacity_packages_the_effects_and_the_interior() {
    // The package model: at opacity 50 % the projection is the opacity-1
    // projection mixed over the backdrop in premultiplied space.
    for json in [STROKE, SHADOW] {
        let full = styled(&scene(true), 1, json).flattened().unwrap();
        let half = styled(&scene(true).with_layer_opacity(1, 0.5).unwrap(), 1, json)
            .flattened()
            .unwrap();
        for y in 0..H {
            for x in 0..W {
                let (b, p) = (unit(backdrop_px(x, y)), unit(full.get_pixel(x, y).0));
                let a = b[3] + (p[3] - b[3]) * 0.5;
                let mut want = [0.0; 4];
                if a > 0.0 {
                    for c in 0..3 {
                        want[c] = (b[c] * b[3] + (p[c] * p[3] - b[c] * b[3]) * 0.5) / a;
                    }
                }
                want[3] = a;
                close(
                    half.get_pixel(x, y).0,
                    want,
                    &format!("{json} at ({x}, {y})"),
                );
            }
        }
    }
}

/// A KNOCKOUT on a styled adjustment is planned like a raster's: the Deep
/// knockout's Background snapshot is taken for it (`style_knockout`'s
/// `knockout_of` reads `renders_any_style`), and its unit pass composites the
/// adjusted backdrop at its own alpha, so with no effect it punches through
/// the layer between to the Background exactly as that raster does.
#[test]
fn a_knockout_on_a_styled_adjustment_is_its_raster_twins() {
    // An opaque middle raster over part of the backdrop (the hole included),
    // so a Deep knockout to the Background has something to punch through.
    let middle = RgbaImage::from_fn(W, H, |x, y| {
        if (3..15).contains(&x) && (4..12).contains(&y) {
            Rgba([30, 200, 90, 255])
        } else {
            Rgba([0, 0, 0, 0])
        }
    });
    let below = DrDocument::from_pixels(backdrop())
        .adding_image_layer(0, middle, "Middle")
        .unwrap();
    let beneath = below.flattened().unwrap();
    let mut adjustment = below
        .adding_image_layer(1, RgbaImage::new(W, H), "Adj")
        .unwrap();
    adjustment.layers[2].meta = Some(adjust_meta("invert", "{}"));
    let twin = below
        .adding_image_layer(
            1,
            RgbaImage::from_fn(W, H, |x, y| {
                let b = beneath.get_pixel(x, y).0;
                Rgba(if b[3] == 0 {
                    [0; 4]
                } else {
                    [255 - b[0], 255 - b[1], 255 - b[2], b[3]]
                })
            }),
            "Twin",
        )
        .unwrap();
    let json = "{\"fill_opacity\":0.5,\"knockout\":\"deep\"}";
    let got = styled(&adjustment, 2, json).flattened().unwrap();
    let want = styled(&twin, 2, json).flattened().unwrap();
    assert_eq!(got.as_raw(), want.as_raw(), "Deep knockout twin");
    // The knockout is not a no-op here, so the equality pins it.
    let unknocked = styled(&twin, 2, "{\"fill_opacity\":0.5}")
        .flattened()
        .unwrap();
    assert_ne!(
        want.as_raw(),
        unknocked.as_raw(),
        "the knockout changes the twin"
    );
}

#[test]
fn a_semi_transparent_backdrop_keeps_its_alpha() {
    // ALPHA-KEPT (the take's a1 band, 0 / 0): the 50 % band stays at 50 %,
    // fully adjusted; the hole stays transparent.
    let flat = styled(&scene(false), 1, STROKE).flattened().unwrap();
    for y in 0..H {
        for x in 0..W {
            let got = flat.get_pixel(x, y).0;
            if inside(x, y, BAND) {
                assert_eq!(got, adjusted_px(x, y), "band at ({x}, {y})");
            } else if inside(x, y, HOLE) {
                assert_eq!(got[3], 0, "hole at ({x}, {y})");
            }
        }
    }
}

#[test]
fn a_styled_adjustment_base_freezes_its_member_to_its_shape() {
    let member = |doc: DrDocument| {
        let mut doc = doc
            .adding_image_layer(
                1,
                RgbaImage::from_pixel(W, H, Rgba([255, 0, 0, 255])),
                "Member",
            )
            .unwrap();
        doc.layers[2].clipped = true;
        doc
    };
    let styled_base = member(styled(&scene(true), 1, SHADOW));
    let flat = styled_base.flattened().unwrap();
    for y in 0..H {
        for x in 0..W {
            let got = flat.get_pixel(x, y).0;
            if mask_at(x, y) == 255 {
                // C-SHAPE on the S-MASK shape: the member shows inside the
                // mask at full strength, over the hole and the band too.
                assert_eq!(got, [255, 0, 0, 255], "member at ({x}, {y})");
            } else if mask_at(x, y) == 0 && (x < MASK.0 || y < MASK.1 || y >= MASK.1 + MASK.3) {
                // Outside the shape and clear of its shadow: the backdrop
                // (alpha alone where it is transparent).
                let want = backdrop_px(x, y);
                if want[3] == 0 {
                    assert_eq!(got[3], 0, "hole at ({x}, {y})");
                } else {
                    assert_eq!(got, want, "backdrop at ({x}, {y})");
                }
            }
        }
    }
    // At opacity 50 % the unit is one package: where the member covers the
    // shape, the member at 50 % over the backdrop, the adjustment under it
    // hidden.
    let half = member(styled(
        &scene(true).with_layer_opacity(1, 0.5).unwrap(),
        1,
        "{\"fill_opacity\":0.99}",
    ))
    .flattened()
    .unwrap();
    for y in MASK.1..MASK.1 + MASK.3 {
        for x in MASK.0..MASK.0 + MASK.2 {
            let b = backdrop_px(x, y);
            if mask_at(x, y) == 255 && b[3] == 255 {
                let want = [
                    0.5 * 255.0 + 0.5 * f64::from(b[0]),
                    0.5 * f64::from(b[1]),
                    0.5 * f64::from(b[2]),
                ];
                let got = half.get_pixel(x, y).0;
                for c in 0..3 {
                    assert!(
                        (f64::from(got[c]) - want[c]).abs() <= 1.0,
                        "({x}, {y}) {got:?} {want:?}"
                    );
                }
            }
        }
    }
    // An UNSTYLED adjustment base keeps today's rule: its unit renders into a
    // transparent buffer, so neither it nor its members contribute anything.
    let unstyled = member(scene(true)).flattened().unwrap();
    assert_eq!(
        unstyled.as_raw(),
        DrDocument::from_pixels(backdrop())
            .flattened()
            .unwrap()
            .as_raw()
    );
}

#[test]
fn an_unstyled_adjustment_is_unchanged() {
    let plain = scene(true).flattened().unwrap();
    for y in 0..H {
        for x in 0..W {
            let b = unit(backdrop_px(x, y));
            let a = unit(adjusted_px(x, y));
            let k = f64::from(mask_at(x, y)) / 255.0;
            let want = if b[3] == 0.0 {
                b
            } else {
                [
                    b[0] + (a[0] - b[0]) * k,
                    b[1] + (a[1] - b[1]) * k,
                    b[2] + (a[2] - b[2]) * k,
                    b[3],
                ]
            };
            close(
                plain.get_pixel(x, y).0,
                want,
                &format!("unstyled at ({x}, {y})"),
            );
        }
    }
    // A style set and cleared again leaves exactly the unstyled projection.
    let set = styled(&scene(true), 1, "{\"fill_opacity\":0.5}");
    let cleared = try_styled(&set, 1, None).unwrap().unwrap();
    assert_eq!(cleared.flattened().unwrap().as_raw(), plain.as_raw());
}

#[test]
fn the_style_round_trips_flattens_and_refuses_regionally() {
    let dir = TempDir::new().unwrap();
    let json = "{\"fill_opacity\":0.75,\"effects\":[{\"type\":\"stroke\",\"size\":2,\"color\":\"#ff0000\"}]}";
    let doc = styled(&scene(true), 1, json);
    let projection = doc.flattened().unwrap();
    // `.rz`: no new word, the style survives the save and reopen.
    let path = dir.path().join("styled-adjustment.rz");
    let target = cpath(&path);
    let mut err: *mut c_char = ptr::null_mut();
    assert!(
        unsafe { dr_doc_save_native(&doc, target.as_ptr(), &mut err) },
        "{}",
        take_err_string(err)
    );
    let back = unsafe { dr_doc_open(target.as_ptr(), &mut err) };
    assert!(!back.is_null(), "{}", take_err_string(err));
    assert_eq!(ffi_style(back, 1), ffi_style(&doc, 1));
    assert!(ffi_style(back, 1).is_some());
    assert_eq!(
        unsafe { &*back }.flattened().unwrap().as_raw(),
        projection.as_raw()
    );
    unsafe { dr_doc_free(back) };
    // Flatten bakes exactly the projection.
    let flat = doc.flattening().unwrap();
    assert_eq!(flat.flattened().unwrap().as_raw(), projection.as_raw());
    // The regional compositor refuses by name.
    let handle = Box::into_raw(Box::new(doc.clone()));
    let image =
        unsafe { dr_doc_render_region(handle, 0, 0, W, H, 1, u64::MAX, ptr::null(), &mut err) };
    unsafe { dr_doc_free(handle) };
    assert!(image.is_null());
    if !image.is_null() {
        unsafe { dr_image_free(image) };
    }
    assert_eq!(
        take_err_string(err),
        "regional render refuses layer 1 (Adj): is an adjustment layer with a layer style or Blend If"
    );
}
