//! Bevel & Emboss — black-box through the style FFI and the projection.
//! The light is `L = (cos alt cos ang, −cos alt sin ang, sin alt)`; highlight
//! `clamp(d)`, shadow `clamp(−d)`, each restricted to its band and
//! composited below or inside the pixels with its own blend and its byte
//! opacity (`common::byte_unit`), over the W3C reference blend. One oracle,
//! the measured law `B` of every Smooth bevel (#2, `style_bevel_smooth`),
//! restated independently: the height `H` is the separable triangle `(W −
//! |j|) / W²` on `a = sel / 255` (reads clamped), `W` the size rounded half
//! up (1 acts as 2) for an Inner and an Outer Bevel and the same of half the
//! size for Emboss and Pillow Emboss; a Pillow shades `H·a + (1 − H)(1 −
//! a)`; `hx = depth · W · (h[x+1] − h[x−1]) / 2` and the same in y; the
//! NORMALIZED law `d = (nl − lz) / (1 − lz)` where `nl = n·L >= lz`, `(nl −
//! lz) / lz` below; Soften the same triangle on `d`.
//!
//! Nothing here reads a core plane; the tolerances are ±2 per colour channel,
//! alpha exact: the kernel's f32 and the oracle's f64 round `d`
//! independently.

use std::ffi::c_int;

use doctored_core::doc::{BlendMode, DrDocument, MaskKind};
use image::{Rgba, RgbaImage};

use crate::common;
use common::*;

// --------------------------------------------------------------- fixture --

/// A mid-gray backdrop: an outer highlight is screen-white, invisible over
/// the shared white fixture, so every oracle here starts from gray.
const GRAY: [u8; 4] = [128, 128, 128, 255];
const RECT_COLOR: [u8; 4] = [120, 160, 200, 255];
const CANVAS: (u32, u32) = (60, 50);
/// 24×20 at (18, 15): at least 15 px of margin on every side, more than
/// the 12 px a size-4 + soften-4 bevel reaches, so the oracle's
/// clamp-to-edge feather equals the core's zero-padded plane.
const RECT: (i32, i32, u32, u32) = (18, 15, 24, 20);
/// A row or column is "pure" — its height depends on one axis only — 6 px
/// in from an edge: the law at size 4 slopes over `W = 4` px (`k < W`) for
/// an Inner and an Outer Bevel and over `W = 2` each side of the edge for
/// Emboss and Pillow Emboss, inside the same 6.
const BAND: i64 = 6;

/// `rect_layer_doc` over a chosen backdrop instead of white.
fn rect_over(
    bg: [u8; 4],
    canvas: (u32, u32),
    rect: (i32, i32, u32, u32),
    color: [u8; 4],
) -> DrDocument {
    let doc = DrDocument::from_pixels(solid(canvas.0, canvas.1, bg));
    let doc = doc
        .adding_image_layer(0, solid(rect.2, rect.3, color), "Rect")
        .expect("add rect layer");
    doc.with_layer_offset(1, rect.0, rect.1).expect("offset")
}

fn gray_doc() -> DrDocument {
    rect_over(GRAY, CANVAS, RECT, RECT_COLOR)
}

fn in_rect(rect: (i32, i32, u32, u32), x: i64, y: i64) -> bool {
    x >= i64::from(rect.0)
        && y >= i64::from(rect.1)
        && x < i64::from(rect.0) + i64::from(rect.2)
        && y < i64::from(rect.1) + i64::from(rect.3)
}

fn rect_selection(w: u32, h: u32, rect: (i32, i32, u32, u32)) -> Vec<u8> {
    selection(w, h, |x, y| {
        if in_rect(rect, i64::from(x), i64::from(y)) {
            255
        } else {
            0
        }
    })
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

/// Per-channel comparison with a stated tolerance, alpha exact.
fn assert_px(got: [u8; 4], want: [u8; 4], tol: i32, what: &str) {
    for c in 0..4 {
        let t = if c == 3 { 0 } else { tol };
        assert!(
            (i32::from(got[c]) - i32::from(want[c])).abs() <= t,
            "{what}: channel {c} is {} expected {} (±{t}); got {got:?} want {want:?}",
            got[c],
            want[c]
        );
    }
}

fn assert_image(got: &RgbaImage, want: &RgbaImage, tol: i32, what: &str) {
    assert_eq!(got.dimensions(), want.dimensions(), "{what}: dimensions");
    for y in 0..got.height() {
        for x in 0..got.width() {
            assert_px(
                px(got, x, y),
                px(want, x, y),
                tol,
                &format!("{what} ({x},{y})"),
            );
        }
    }
}

/// "Only lighter": every channel ≥ the reference (a screen-white
/// highlight never darkens) — and strictly lighter when `strict`.
fn assert_lighter(got: [u8; 4], base: [u8; 4], strict: bool, what: &str) {
    assert_eq!(got[3], base[3], "{what}: alpha");
    for c in 0..3 {
        assert!(
            got[c] >= base[c],
            "{what}: channel {c} darkened: {got:?} vs {base:?}"
        );
    }
    if strict {
        assert!(got[..3] != base[..3], "{what}: not lit: {got:?}");
    }
}

/// "Only darker": every channel ≤ the reference (a multiply-black shadow
/// never lightens) — and strictly darker when `strict`.
fn assert_darker(got: [u8; 4], base: [u8; 4], strict: bool, what: &str) {
    assert_eq!(got[3], base[3], "{what}: alpha");
    for c in 0..3 {
        assert!(
            got[c] <= base[c],
            "{what}: channel {c} lightened: {got:?} vs {base:?}"
        );
    }
    if strict {
        assert!(got[..3] != base[..3], "{what}: not shaded: {got:?}");
    }
}

// ---------------------------------------------------------------- style --

/// One bevel effect's knobs: the JSON the FFI takes and the numbers the
/// oracle uses come from the same value.
#[derive(Clone)]
struct Bevel {
    style: &'static str,
    depth: f32,
    down: bool,
    size: f32,
    soften: f32,
    angle: f32,
    altitude: f32,
    use_global_light: bool,
    highlight: (c_int, &'static str, [u8; 3], f32),
    shadow: (c_int, &'static str, [u8; 3], f32),
    fill_opacity: f32,
    enabled: bool,
}

impl Default for Bevel {
    /// The core's defaults with the light made explicit (angle 0 = from the
    /// right) and its own, so the document light plays no part unless a
    /// test asks for it.
    fn default() -> Self {
        Bevel {
            style: "inner_bevel",
            depth: 1.0,
            down: false,
            size: 4.0,
            soften: 0.0,
            angle: 0.0,
            altitude: 30.0,
            use_global_light: false,
            highlight: (BLEND_SCREEN, "screen", [255, 255, 255], 0.75),
            shadow: (BLEND_MULTIPLY, "multiply", [0, 0, 0], 0.75),
            fill_opacity: 1.0,
            enabled: true,
        }
    }
}

impl Bevel {
    fn json(&self) -> String {
        let hex = |c: [u8; 3]| format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2]);
        format!(
            "{{\"fill_opacity\":{},\"effects\":[{{\"type\":\"bevel_emboss\",\"enabled\":{},\
             \"style\":\"{}\",\"depth\":{},\"direction\":\"{}\",\"size\":{},\"soften\":{},\
             \"angle\":{},\"use_global_light\":{},\"altitude\":{},\
             \"highlight_blend\":\"{}\",\"highlight_color\":\"{}\",\"highlight_opacity\":{},\
             \"shadow_blend\":\"{}\",\"shadow_color\":\"{}\",\"shadow_opacity\":{}}}]}}",
            self.fill_opacity,
            self.enabled,
            self.style,
            self.depth,
            if self.down { "down" } else { "up" },
            self.size,
            self.soften,
            self.angle,
            self.use_global_light,
            self.altitude,
            self.highlight.1,
            hex(self.highlight.2),
            self.highlight.3,
            self.shadow.1,
            hex(self.shadow.2),
            self.shadow.3,
        )
    }
}

// --------------------------------------------------------------- oracle --

/// `round(a · b / 255)` per pixel.
fn mul(a: &[u8], b: &[u8]) -> Vec<u8> {
    a.iter()
        .zip(b)
        .map(|(&x, &y)| ((u32::from(x) * u32::from(y) + 127) / 255) as u8)
        .collect()
}

fn inv(a: &[u8]) -> Vec<u8> {
    a.iter().map(|&v| 255 - v).collect()
}

fn coverage(d: f32) -> u8 {
    (255.0 * d.clamp(0.0, 1.0)).round() as u8
}

/// The law's box-pair width for a size or soften of `v` px (module doc):
/// 0 for none, else `v` rounded half up, at least 1, and 2 where that is 1.
fn law_width(v: f32) -> usize {
    if !v.is_finite() || v <= 0.0 {
        return 0;
    }
    match (f64::from(v) + 0.5).floor().max(1.0) as usize {
        1 => 2,
        s => s,
    }
}

/// The separable triangle `(width − |j|) / width²` over `plane`, x then y,
/// reads clamped to the plane; the plane itself at width 0.
fn triangle(plane: &[f64], w: u32, h: u32, width: usize) -> Vec<f64> {
    if width == 0 {
        return plane.to_vec();
    }
    let (wi, hi, n) = (i64::from(w), i64::from(h), width as i64);
    let taps: Vec<(i64, f64)> = (1 - n..n)
        .map(|j| (j, (n - j.abs()) as f64 / (n * n) as f64))
        .collect();
    let mut across = vec![0.0; plane.len()];
    for y in 0..hi {
        for x in 0..wi {
            across[(y * wi + x) as usize] = taps
                .iter()
                .map(|&(j, k)| k * plane[(y * wi + (x + j).clamp(0, wi - 1)) as usize])
                .sum();
        }
    }
    let mut out = vec![0.0; plane.len()];
    for y in 0..hi {
        for x in 0..wi {
            out[(y * wi + x) as usize] = taps
                .iter()
                .map(|&(j, k)| k * across[((y + j).clamp(0, hi - 1) * wi + x) as usize])
                .sum();
        }
    }
    out
}

/// The NORMALIZED law for slopes (`hx`, `hy`) under `l` (module doc).
fn normalized(hx: f64, hy: f64, l: (f64, f64, f64)) -> f64 {
    let nl = (-hx * l.0 - hy * l.1 + l.2) / (1.0 + hx * hx + hy * hy).sqrt();
    if nl >= l.2 {
        if l.2 < 1.0 {
            (nl - l.2) / (1.0 - l.2)
        } else {
            0.0
        }
    } else if l.2 > 0.0 {
        (nl - l.2) / l.2
    } else {
        0.0
    }
}

/// `L` for (`angle`, `altitude`) in degrees, y-down.
fn light_vector(light: (f32, f32)) -> (f64, f64, f64) {
    let (ang, alt) = (
        f64::from(light.0).to_radians(),
        f64::from(light.1).to_radians(),
    );
    (alt.cos() * ang.cos(), -alt.cos() * ang.sin(), alt.sin())
}

/// The Smooth law `B` of the bevel's style (module doc): the shading `d` per
/// pixel after the soften.
fn law_shading(sel: &[u8], w: u32, h: u32, b: &Bevel, light: (f32, f32)) -> Vec<f32> {
    let width = match b.style {
        "emboss" | "pillow_emboss" => law_width(b.size / 2.0),
        _ => law_width(b.size),
    };
    let unit: Vec<f64> = sel.iter().map(|&v| f64::from(v) / 255.0).collect();
    let big = triangle(&unit, w, h, width);
    let height: Vec<f64> = if b.style == "pillow_emboss" {
        big.iter()
            .zip(&unit)
            .map(|(&hh, &a)| hh * a + (1.0 - hh) * (1.0 - a))
            .collect()
    } else {
        big
    };
    let (wi, hi) = (i64::from(w), i64::from(h));
    let at = |x: i64, y: i64| height[(y.clamp(0, hi - 1) * wi + x.clamp(0, wi - 1)) as usize];
    let gain = f64::from(b.depth) * width as f64 / 2.0;
    let sign = if b.down { -1.0 } else { 1.0 };
    let l = light_vector(light);
    let mut d = Vec::with_capacity(sel.len());
    for y in 0..hi {
        for x in 0..wi {
            let hx = (at(x + 1, y) - at(x - 1, y)) * gain * sign;
            let hy = (at(x, y + 1) - at(x, y - 1)) * gain * sign;
            d.push(normalized(hx, hy, l));
        }
    }
    triangle(&d, w, h, law_width(b.soften))
        .into_iter()
        .map(|v| v as f32)
        .collect()
}

/// The four coverage planes an effect emits — (outer highlight, outer
/// shadow, inner highlight, inner shadow), each already restricted to its
/// band; a style without that half leaves its planes zero.
struct Planes {
    outer_hl: Vec<u8>,
    outer_sh: Vec<u8>,
    inner_hl: Vec<u8>,
    inner_sh: Vec<u8>,
}

fn planes(sel: &[u8], w: u32, h: u32, b: &Bevel, light: (f32, f32)) -> Planes {
    let d = match b.style {
        "inner_bevel" | "outer_bevel" | "emboss" | "pillow_emboss" => {
            law_shading(sel, w, h, b, light)
        }
        other => panic!("unknown style {other}"),
    };
    let hl: Vec<u8> = d.iter().map(|&v| coverage(v)).collect();
    let sh: Vec<u8> = d.iter().map(|&v| coverage(-v)).collect();
    let zero = vec![0u8; sel.len()];
    let (has_inner, has_outer) = match b.style {
        "inner_bevel" => (true, false),
        "outer_bevel" => (false, true),
        _ => (true, true),
    };
    let pick =
        |on: bool, plane: &[u8], band: &[u8]| if on { mul(plane, band) } else { zero.clone() };
    let outside = inv(sel);
    Planes {
        outer_hl: pick(has_outer, &hl, &outside),
        outer_sh: pick(has_outer, &sh, &outside),
        inner_hl: pick(has_inner, &hl, sel),
        inner_sh: pick(has_inner, &sh, sel),
    }
}

/// The expected projection over a solid `bg`: below halves, then the layer
/// pixel (if any) at `fill_opacity` with `layer_mode`, then the inner
/// halves — every step the W3C reference composite, quantized once.
fn expected(
    bg: [u8; 4],
    w: u32,
    h: u32,
    layer_px: impl Fn(u32, u32) -> Option<[u8; 4]>,
    p: &Planes,
    b: &Bevel,
    layer_mode: c_int,
) -> RgbaImage {
    let (hl_mode, _, hl_rgb, hl_op) = b.highlight;
    let (sh_mode, _, sh_rgb, sh_op) = b.shadow;
    let (hl_op, sh_op) = (
        byte_unit(f64::from(hl_op)) as f32,
        byte_unit(f64::from(sh_op)) as f32,
    );
    let unit = |c: [u8; 3]| {
        [
            f32::from(c[0]) / 255.0,
            f32::from(c[1]) / 255.0,
            f32::from(c[2]) / 255.0,
            1.0,
        ]
    };
    RgbaImage::from_fn(w, h, |x, y| {
        let i = (y * w + x) as usize;
        let mut acc = to_unit(bg);
        acc = ref_composite(
            acc,
            unit(hl_rgb),
            hl_op * f32::from(p.outer_hl[i]) / 255.0,
            hl_mode,
        );
        acc = ref_composite(
            acc,
            unit(sh_rgb),
            sh_op * f32::from(p.outer_sh[i]) / 255.0,
            sh_mode,
        );
        if let Some(l) = layer_px(x, y) {
            acc = ref_composite(acc, to_unit(l), b.fill_opacity, layer_mode);
        }
        acc = ref_composite(
            acc,
            unit(hl_rgb),
            hl_op * f32::from(p.inner_hl[i]) / 255.0,
            hl_mode,
        );
        acc = ref_composite(
            acc,
            unit(sh_rgb),
            sh_op * f32::from(p.inner_sh[i]) / 255.0,
            sh_mode,
        );
        Rgba(quantized(acc))
    })
}

/// The oracle projection of `RECT` on `CANVAS` over `bg` with its own light.
fn expected_rect(bg: [u8; 4], b: &Bevel, layer_mode: c_int) -> RgbaImage {
    let sel = rect_selection(CANVAS.0, CANVAS.1, RECT);
    let p = planes(&sel, CANVAS.0, CANVAS.1, b, (b.angle, b.altitude));
    expected(
        bg,
        CANVAS.0,
        CANVAS.1,
        |x, y| in_rect(RECT, i64::from(x), i64::from(y)).then_some(RECT_COLOR),
        &p,
        b,
        layer_mode,
    )
}

// ---------------------------------------------------------------- tests --

/// Rows whose height depends on x only (BAND px clear of the top and
/// bottom edges, so the vertical kernel and the central difference never
/// see them) and the same for columns.
fn pure_rows() -> std::ops::Range<u32> {
    (RECT.1 as u32 + BAND as u32)..(RECT.1 as u32 + RECT.3 - BAND as u32)
}

fn pure_cols() -> std::ops::Range<u32> {
    (RECT.0 as u32 + BAND as u32)..(RECT.0 as u32 + RECT.2 - BAND as u32)
}

/// Distance of column `x` inward from the rect's right edge (0 = the edge
/// pixel), and from the left edge.
fn from_right(x: u32) -> i64 {
    i64::from(RECT.0) + i64::from(RECT.2) - 1 - i64::from(x)
}

fn from_left(x: u32) -> i64 {
    i64::from(x) - i64::from(RECT.0)
}

#[test]
fn inner_bevel_lights_the_edge_facing_the_light_and_shades_the_far_one() {
    // Light from the right (angle 0): on the right band h falls toward the
    // edge, so n.x > 0 and d > 0 — highlight only; on the left band the
    // slope faces away — shadow only; on the top and bottom bands the
    // height varies in y only, so the slope is across the light and the
    // NORMALIZED law tips it below `lz`: shadow only.
    let flat = styled(&gray_doc(), 1, &Bevel::default().json())
        .flattened()
        .unwrap();
    for y in pure_rows() {
        for x in RECT.0 as u32..RECT.0 as u32 + RECT.2 {
            let got = px(&flat, x, y);
            let (r, l) = (from_right(x), from_left(x));
            let what = format!("({x},{y})");
            if r < BAND {
                assert_lighter(got, RECT_COLOR, r == 0, &what);
            } else if l < BAND {
                assert_darker(got, RECT_COLOR, l == 0, &what);
            } else {
                assert_eq!(got, RECT_COLOR, "{what}: the plateau is untouched");
            }
        }
    }
    let mut shaded = 0;
    for x in pure_cols() {
        for y in RECT.1 as u32..RECT.1 as u32 + RECT.3 {
            let got = px(&flat, x, y);
            assert_darker(
                got,
                RECT_COLOR,
                false,
                &format!("top/bottom band ({x},{y})"),
            );
            if got != RECT_COLOR {
                shaded += 1;
            }
        }
    }
    assert!(shaded > 0, "the top and bottom bands are shaded");
    // The inner bevel is Interior only: nothing lands outside the shape.
    for y in 0..CANVAS.1 {
        for x in 0..CANVAS.0 {
            if !in_rect(RECT, i64::from(x), i64::from(y)) {
                assert_eq!(px(&flat, x, y), GRAY, "outside ({x},{y})");
            }
        }
    }
}

#[test]
fn angle_180_and_direction_down_both_swap_the_bands() {
    let from_left_light = Bevel {
        angle: 180.0,
        ..Bevel::default()
    };
    let down = Bevel {
        down: true,
        ..Bevel::default()
    };
    let a = styled(&gray_doc(), 1, &from_left_light.json())
        .flattened()
        .unwrap();
    let b = styled(&gray_doc(), 1, &down.json()).flattened().unwrap();
    for y in pure_rows() {
        for x in RECT.0 as u32..RECT.0 as u32 + RECT.2 {
            let (r, l) = (from_right(x), from_left(x));
            for (flat, name) in [(&a, "angle 180"), (&b, "direction down")] {
                let got = px(flat, x, y);
                let what = format!("{name} ({x},{y})");
                if l < BAND {
                    assert_lighter(got, RECT_COLOR, l == 0, &what);
                } else if r < BAND {
                    assert_darker(got, RECT_COLOR, r == 0, &what);
                } else {
                    assert_eq!(got, RECT_COLOR, "{what}");
                }
            }
        }
    }
    // Negating d and reversing the light are the same operation on the
    // pure bands; at the corners `sin 180°` is a few ulps from 0, hence ±1.
    assert_image(&a, &b, 1, "angle 180 vs direction down");
}

#[test]
fn nothing_at_size_0_zero_opacity_or_disabled() {
    let plain = gray_doc().flattened().unwrap();
    let cases = [
        (
            "size 0",
            Bevel {
                size: 0.0,
                ..Bevel::default()
            },
        ),
        (
            "both opacities 0",
            Bevel {
                highlight: (BLEND_SCREEN, "screen", [255, 255, 255], 0.0),
                shadow: (BLEND_MULTIPLY, "multiply", [0, 0, 0], 0.0),
                ..Bevel::default()
            },
        ),
    ];
    for (name, bevel) in cases {
        let flat = styled(&gray_doc(), 1, &bevel.json()).flattened().unwrap();
        assert_eq!(
            flat, plain,
            "{name} must render exactly the unstyled projection"
        );
    }
    // A disabled bevel alone is an identity style, which the core refuses
    // to store (the setter's one rule); beside a fill opacity it is stored
    // and renders exactly as the fill opacity alone.
    let disabled = Bevel {
        enabled: false,
        ..Bevel::default()
    };
    assert!(
        try_styled(&gray_doc(), 1, Some(&disabled.json()))
            .expect("valid")
            .is_none(),
        "a disabled effect alone is an identity style"
    );
    let disabled_with_fill = Bevel {
        fill_opacity: 0.5,
        ..disabled
    };
    let fill_only = styled(&gray_doc(), 1, "{\"fill_opacity\":0.5}")
        .flattened()
        .unwrap();
    assert_eq!(
        styled(&gray_doc(), 1, &disabled_with_fill.json())
            .flattened()
            .unwrap(),
        fill_only,
        "a disabled bevel renders nothing"
    );
    // One opacity at 0 drops only that half.
    let no_shadow = Bevel {
        shadow: (BLEND_MULTIPLY, "multiply", [0, 0, 0], 0.0),
        ..Bevel::default()
    };
    let flat = styled(&gray_doc(), 1, &no_shadow.json())
        .flattened()
        .unwrap();
    let y = pure_rows().start;
    assert_lighter(
        px(&flat, RECT.0 as u32 + RECT.2 - 1, y),
        RECT_COLOR,
        true,
        "highlight kept",
    );
    assert_eq!(px(&flat, RECT.0 as u32, y), RECT_COLOR, "shadow dropped");
}

#[test]
fn an_overhead_inner_bevel_shades_its_band_and_lights_nothing() {
    // At altitude 90 the NORMALIZED law is `1 / sqrt(1 + s²) − 1 <= 0`: every
    // slope is shadow, flat ground 0, and no highlight anywhere.
    let overhead = Bevel {
        altitude: 90.0,
        ..Bevel::default()
    };
    let plain = gray_doc().flattened().unwrap();
    let highlight_only = Bevel {
        shadow: (BLEND_MULTIPLY, "multiply", [0, 0, 0], 0.0),
        ..overhead.clone()
    };
    assert_eq!(
        styled(&gray_doc(), 1, &highlight_only.json())
            .flattened()
            .unwrap(),
        plain,
        "an overhead highlight lights nothing"
    );
    let flat = styled(&gray_doc(), 1, &overhead.json())
        .flattened()
        .unwrap();
    assert_image(
        &flat,
        &expected_rect(GRAY, &overhead, BLEND_NORMAL),
        2,
        "inner bevel at altitude 90",
    );
    for y in pure_rows() {
        for x in RECT.0 as u32..RECT.0 as u32 + RECT.2 {
            let got = px(&flat, x, y);
            let (r, l) = (from_right(x), from_left(x));
            let what = format!("({x},{y})");
            if r < BAND || l < BAND {
                assert_darker(got, RECT_COLOR, r == 0 || l == 0, &what);
            } else {
                assert_eq!(got, RECT_COLOR, "{what}: the plateau is untouched");
            }
        }
    }
}

#[test]
fn an_overhead_emboss_shades_both_sides_of_its_edge_and_lights_nothing() {
    // An Emboss runs the law too (#2), so at altitude 90 every slope is
    // shadow under the NORMALIZED law and nothing is highlight: the edge's
    // slope straddles the contour, so both the inside and the outside are
    // shaded.
    let overhead = Bevel {
        style: "emboss",
        altitude: 90.0,
        ..Bevel::default()
    };
    let plain = gray_doc().flattened().unwrap();
    let highlight_only = Bevel {
        shadow: (BLEND_MULTIPLY, "multiply", [0, 0, 0], 0.0),
        ..overhead.clone()
    };
    assert_eq!(
        styled(&gray_doc(), 1, &highlight_only.json())
            .flattened()
            .unwrap(),
        plain,
        "an overhead emboss's highlight lights nothing"
    );
    let flat = styled(&gray_doc(), 1, &overhead.json())
        .flattened()
        .unwrap();
    assert_image(
        &flat,
        &expected_rect(GRAY, &overhead, BLEND_NORMAL),
        2,
        "emboss at altitude 90",
    );
    let y = pure_rows().start;
    let (right_edge, left_edge) = (RECT.0 as u32 + RECT.2 - 1, RECT.0 as u32);
    for (x, base, what) in [
        (right_edge, RECT_COLOR, "inside right"),
        (right_edge + 1, GRAY, "outside right"),
        (left_edge, RECT_COLOR, "inside left"),
        (left_edge - 1, GRAY, "outside left"),
    ] {
        assert_darker(
            px(&flat, x, y),
            base,
            true,
            &format!("overhead emboss {what}"),
        );
    }
    let mid = RECT.0 as u32 + RECT.2 / 2;
    assert_eq!(px(&flat, mid, y), RECT_COLOR, "the plateau is untouched");
}

#[test]
fn a_full_layer_bevels_only_along_the_canvas_border() {
    // A canvas-sized opaque layer is constant height everywhere but at
    // the layer's boundary, which the zero-padded plane makes an edge —
    // a frame bevel on a full layer. Flat ground shades to 0:
    // every pixel further than the band from the border is untouched.
    let doc = DrDocument::from_pixels(solid(CANVAS.0, CANVAS.1, GRAY))
        .adding_image_layer(0, solid(CANVAS.0, CANVAS.1, RECT_COLOR), "Full")
        .expect("add layer");
    let flat = styled(&doc, 1, &Bevel::default().json())
        .flattened()
        .unwrap();
    let band = BAND as u32;
    for y in 0..CANVAS.1 {
        for x in 0..CANVAS.0 {
            let got = px(&flat, x, y);
            let inside = x >= band && y >= band && x < CANVAS.0 - band && y < CANVAS.1 - band;
            if inside {
                assert_eq!(got, RECT_COLOR, "flat ground ({x},{y})");
            }
        }
    }
    let y = CANVAS.1 / 2;
    assert_lighter(
        px(&flat, CANVAS.0 - 1, y),
        RECT_COLOR,
        true,
        "right border lit",
    );
    assert_darker(px(&flat, 0, y), RECT_COLOR, true, "left border shaded");
}

#[test]
fn inner_bevel_matches_the_box_pair_law_oracle() {
    let bevel = Bevel::default();
    let flat = styled(&gray_doc(), 1, &bevel.json()).flattened().unwrap();
    assert_image(
        &flat,
        &expected_rect(GRAY, &bevel, BLEND_NORMAL),
        2,
        "inner bevel, size 4",
    );

    // The closed form, spelled out: along one pure row, k px in from the
    // right edge, the box pair's slope is |hx| = depth·(W − 0.5 − k)/W for
    // k < W (W = 4), falling toward the edge (hx < 0) and 0 beyond; the
    // NORMALIZED law at (0°, 30°) is d = (nl − lz)/(1 − lz) with nl = (−hx
    // cos 30° + sin 30°)/sqrt(1 + hx²), and the band's pixels are
    // screen-white at the byte 0.75 · d over the rect colour.
    let (w, depth) = (4.0f64, 1.0f64);
    let y = pure_rows().start;
    let (lx, lz) = (30f64.to_radians().cos(), 30f64.to_radians().sin());
    let mut lit_columns = 0;
    for x in RECT.0 as u32 + RECT.2 - BAND as u32..RECT.0 as u32 + RECT.2 {
        let k = from_right(x) as f64;
        let hx = if k < w {
            -depth * (w - 0.5 - k) / w
        } else {
            0.0
        };
        let nl = (-hx * lx + lz) / (1.0 + hx * hx).sqrt();
        let d = (nl - lz) / (1.0 - lz);
        let cov = coverage(d as f32);
        let want = quantized(ref_composite(
            to_unit(RECT_COLOR),
            [1.0, 1.0, 1.0, 1.0],
            byte_unit(0.75) as f32 * f32::from(cov) / 255.0,
            BLEND_SCREEN,
        ));
        assert_px(
            px(&flat, x, y),
            want,
            2,
            &format!("column {x} (k = {k}, d = {d:.3})"),
        );
        if cov > 0 {
            lit_columns += 1;
        }
    }
    assert_eq!(lit_columns, 4, "the band is lit across its W = 4 columns");
}

#[test]
fn outer_bevel_emboss_and_pillow_place_their_halves_below_and_inside() {
    for style in ["outer_bevel", "emboss", "pillow_emboss"] {
        let bevel = Bevel {
            style,
            ..Bevel::default()
        };
        let flat = styled(&gray_doc(), 1, &bevel.json()).flattened().unwrap();
        assert_image(&flat, &expected_rect(GRAY, &bevel, BLEND_NORMAL), 2, style);
    }
    // The shape of each style, directly: light from the right.
    let y = pure_rows().start;
    let (right_edge, left_edge) = (RECT.0 as u32 + RECT.2 - 1, RECT.0 as u32);
    let (right_out, left_out) = (right_edge + 1, left_edge - 1);
    let outer = styled(
        &gray_doc(),
        1,
        &Bevel {
            style: "outer_bevel",
            ..Bevel::default()
        }
        .json(),
    )
    .flattened()
    .unwrap();
    assert_lighter(
        px(&outer, right_out, y),
        GRAY,
        true,
        "outer bevel: outside right lit",
    );
    assert_darker(
        px(&outer, left_out, y),
        GRAY,
        true,
        "outer bevel: outside left shaded",
    );
    assert_eq!(
        px(&outer, right_edge, y),
        RECT_COLOR,
        "outer bevel: inside untouched"
    );
    let emboss = styled(
        &gray_doc(),
        1,
        &Bevel {
            style: "emboss",
            ..Bevel::default()
        }
        .json(),
    )
    .flattened()
    .unwrap();
    assert_lighter(
        px(&emboss, right_out, y),
        GRAY,
        true,
        "emboss: outside right lit",
    );
    assert_lighter(
        px(&emboss, right_edge, y),
        RECT_COLOR,
        true,
        "emboss: inside right lit",
    );
    assert_darker(
        px(&emboss, left_out, y),
        GRAY,
        true,
        "emboss: outside left shaded",
    );
    assert_darker(
        px(&emboss, left_edge, y),
        RECT_COLOR,
        true,
        "emboss: inside left shaded",
    );
    let pillow = styled(
        &gray_doc(),
        1,
        &Bevel {
            style: "pillow_emboss",
            ..Bevel::default()
        }
        .json(),
    )
    .flattened()
    .unwrap();
    // Photoshop's measured pillow (#2, identify take2): the inside falls
    // toward the edge and the outside rises away from it.
    assert_lighter(
        px(&pillow, right_edge, y),
        RECT_COLOR,
        true,
        "pillow: inside right lit",
    );
    assert_darker(
        px(&pillow, right_out, y),
        GRAY,
        true,
        "pillow: outside right shaded",
    );
    assert_darker(
        px(&pillow, left_edge, y),
        RECT_COLOR,
        true,
        "pillow: inside left shaded",
    );
    assert_lighter(
        px(&pillow, left_out, y),
        GRAY,
        true,
        "pillow: outside left lit",
    );
}

#[test]
fn highlight_and_shadow_keep_their_own_blend_and_opacity_on_a_multiply_layer() {
    // A Multiply-blend LAYER with Normal-blend halves in their own colours:
    // the halves follow ref_composite(..., NORMAL) over the multiplied
    // pixels, never the layer's Multiply.
    let bevel = Bevel {
        style: "emboss",
        highlight: (BLEND_NORMAL, "normal", [255, 200, 40], 0.4),
        shadow: (BLEND_NORMAL, "normal", [30, 60, 220], 0.6),
        ..Bevel::default()
    };
    let doc = gray_doc()
        .with_layer_blend_mode(1, BlendMode::Multiply)
        .expect("multiply");
    let flat = styled(&doc, 1, &bevel.json()).flattened().unwrap();
    assert_image(
        &flat,
        &expected_rect(GRAY, &bevel, BLEND_MULTIPLY),
        2,
        "emboss on a multiply layer",
    );
    // Sanity, without the oracle: the multiplied rect is dark, and the
    // Normal orange highlight on its right edge is brighter than that in
    // red — Multiply could only have darkened it.
    let multiplied = quantized(ref_composite(
        to_unit(GRAY),
        to_unit(RECT_COLOR),
        1.0,
        BLEND_MULTIPLY,
    ));
    let y = pure_rows().start;
    let lit = px(&flat, RECT.0 as u32 + RECT.2 - 1, y);
    assert!(
        lit[0] > multiplied[0],
        "highlight {lit:?} must lighten red over {multiplied:?}"
    );
    let shaded = px(&flat, RECT.0 as u32, y);
    assert!(
        shaded[2] > multiplied[2],
        "a blue Normal shadow {shaded:?} adds blue over {multiplied:?}"
    );
}

#[test]
fn fill_opacity_zero_keeps_the_bevel() {
    let bevel = Bevel {
        style: "emboss",
        fill_opacity: 0.0,
        ..Bevel::default()
    };
    let flat = styled(&gray_doc(), 1, &bevel.json()).flattened().unwrap();
    assert_image(
        &flat,
        &expected_rect(GRAY, &bevel, BLEND_NORMAL),
        2,
        "emboss at fill 0",
    );
    let y = pure_rows().start;
    let mid = RECT.0 as u32 + RECT.2 / 2;
    assert_eq!(px(&flat, mid, y), GRAY, "the pixels are gone");
    assert_lighter(
        px(&flat, RECT.0 as u32 + RECT.2 - 1, y),
        GRAY,
        true,
        "the inner highlight stays",
    );
}

#[test]
fn soften_and_depth_follow_the_oracle() {
    let soft = Bevel {
        soften: 4.0,
        ..Bevel::default()
    };
    let flat = styled(&gray_doc(), 1, &soft.json()).flattened().unwrap();
    assert_image(
        &flat,
        &expected_rect(GRAY, &soft, BLEND_NORMAL),
        2,
        "soften 4",
    );
    // Softening the step at the edge lowers the rim's slope: the edge
    // pixel is less lit than without soften — and still nothing outside.
    let hard = styled(&gray_doc(), 1, &Bevel::default().json())
        .flattened()
        .unwrap();
    let y = pure_rows().start;
    let edge = RECT.0 as u32 + RECT.2 - 1;
    assert!(
        px(&flat, edge, y)[0] < px(&hard, edge, y)[0],
        "soften lowers the rim"
    );
    assert_eq!(
        px(&flat, edge + 1, y),
        GRAY,
        "an inner bevel stays inside even softened"
    );

    let deep = Bevel {
        depth: 5.0,
        ..Bevel::default()
    };
    let flat = styled(&gray_doc(), 1, &deep.json()).flattened().unwrap();
    assert_image(
        &flat,
        &expected_rect(GRAY, &deep, BLEND_NORMAL),
        2,
        "depth 5",
    );
    assert!(
        px(&flat, edge, y)[0] > px(&hard, edge, y)[0],
        "depth raises the contrast"
    );
}

#[test]
fn use_global_light_reads_the_document_light_and_altitude() {
    let global = Bevel {
        use_global_light: true,
        angle: 120.0, // ignored while the global light is on
        altitude: 60.0,
        ..Bevel::default()
    };
    let own = Bevel::default(); // angle 0, altitude 30, its own light
    let lit_from_right = styled(&with_light(&gray_doc(), 0.0, 30.0), 1, &global.json())
        .flattened()
        .unwrap();
    assert_eq!(
        lit_from_right,
        styled(&gray_doc(), 1, &own.json()).flattened().unwrap(),
        "global (0°, 30°) resolves to exactly the effect's own (0°, 30°)"
    );
    let own_overhead = Bevel {
        altitude: 90.0,
        ..Bevel::default()
    };
    let overhead = styled(&with_light(&gray_doc(), 0.0, 90.0), 1, &global.json())
        .flattened()
        .unwrap();
    assert_eq!(
        overhead,
        styled(&gray_doc(), 1, &own_overhead.json())
            .flattened()
            .unwrap(),
        "global (0°, 90°) resolves to exactly the effect's own (0°, 90°)"
    );
    // And an effect with its own light ignores the document's.
    let own_left = Bevel {
        angle: 180.0,
        ..Bevel::default()
    };
    let flat = styled(&with_light(&gray_doc(), 0.0, 30.0), 1, &own_left.json())
        .flattened()
        .unwrap();
    let y = pure_rows().start;
    assert_lighter(
        px(&flat, RECT.0 as u32, y),
        RECT_COLOR,
        true,
        "own angle 180 lights the left",
    );
}

#[test]
fn offset_layers_and_masks_follow_the_shape() {
    // The rect hangs off the top-left corner: (-8, -6). The oracle runs on
    // a virtual plane extended by 20 px on those sides, where the rect
    // sits fully inside, and is cropped back to the canvas.
    let bevel = Bevel {
        style: "emboss",
        ..Bevel::default()
    };
    let rect = (-8, -6, RECT.2, RECT.3);
    let doc = rect_over(GRAY, CANVAS, rect, RECT_COLOR);
    let flat = styled(&doc, 1, &bevel.json()).flattened().unwrap();
    let (ext, vw, vh) = (20u32, CANVAS.0 + 20, CANVAS.1 + 20);
    let vrect = (rect.0 + ext as i32, rect.1 + ext as i32, rect.2, rect.3);
    let sel = rect_selection(vw, vh, vrect);
    let p = planes(&sel, vw, vh, &bevel, (bevel.angle, bevel.altitude));
    let virtual_expected = expected(
        GRAY,
        vw,
        vh,
        |x, y| in_rect(vrect, i64::from(x), i64::from(y)).then_some(RECT_COLOR),
        &p,
        &bevel,
        BLEND_NORMAL,
    );
    let want = RgbaImage::from_fn(CANVAS.0, CANVAS.1, |x, y| {
        *virtual_expected.get_pixel(x + ext, y + ext)
    });
    assert_image(&flat, &want, 2, "emboss partly off-canvas");
    // The rect's right edge is at canvas x = 15 and row 4 is 10 px below
    // its (off-canvas) top: a pure row.
    let right_edge = (rect.0 + rect.2 as i32 - 1) as u32;
    assert_lighter(
        px(&flat, right_edge, 4),
        RECT_COLOR,
        true,
        "the visible right edge is lit",
    );

    // A mask hiding the right half moves the shape's right edge — and the
    // bevel with it: the effect is a function of alpha × mask.
    let half = i64::from(RECT.0) + i64::from(RECT.2) / 2;
    let mask = selection(CANVAS.0, CANVAS.1, |x, _| {
        if i64::from(x) < half {
            255
        } else {
            0
        }
    });
    let masked = gray_doc()
        .add_mask(1, MaskKind::FromSelection(&mask))
        .expect("mask");
    let flat = styled(&masked, 1, &Bevel::default().json())
        .flattened()
        .unwrap();
    let sel: Vec<u8> = rect_selection(CANVAS.0, CANVAS.1, RECT)
        .iter()
        .zip(&mask)
        .map(|(&s, &m)| ((u32::from(s) * u32::from(m) + 127) / 255) as u8)
        .collect();
    let p = planes(&sel, CANVAS.0, CANVAS.1, &Bevel::default(), (0.0, 30.0));
    let want = expected(
        GRAY,
        CANVAS.0,
        CANVAS.1,
        |x, y| {
            (in_rect(RECT, i64::from(x), i64::from(y)) && i64::from(x) < half).then_some(RECT_COLOR)
        },
        &p,
        &Bevel::default(),
        BLEND_NORMAL,
    );
    assert_image(&flat, &want, 2, "inner bevel on a masked rect");
    let y = pure_rows().start;
    assert_lighter(
        px(&flat, half as u32 - 1, y),
        RECT_COLOR,
        true,
        "the mask's edge is the lit edge",
    );
    assert_eq!(
        px(&flat, RECT.0 as u32 + RECT.2 - 1, y),
        GRAY,
        "the masked half shows the backdrop"
    );
}

#[test]
fn transparent_layers_render_nothing_and_the_source_is_unchanged() {
    let base = DrDocument::from_pixels(solid(CANVAS.0, CANVAS.1, GRAY))
        .adding_image_layer(0, solid(RECT.2, RECT.3, [0, 0, 0, 0]), "Clear")
        .expect("add layer");
    let plain = base.flattened().unwrap();
    let flat = styled(&base, 1, &Bevel::default().json())
        .flattened()
        .unwrap();
    assert_eq!(flat, plain, "a fully transparent shape has no edges");

    let source = gray_doc();
    let before = source.flattened().unwrap();
    let styled_doc = styled(&source, 1, &Bevel::default().json());
    assert_eq!(
        source.flattened().unwrap(),
        before,
        "the input document is untouched"
    );
    assert!(
        source.layers[1].style.is_none(),
        "the input layer carries no style"
    );
    assert!(styled_doc.layers[1].style.is_some());
    assert_ne!(
        styled_doc.flattened().unwrap(),
        before,
        "the styled copy renders the bevel"
    );
}

#[test]
fn the_canonical_style_sets_again_as_unchanged() {
    let doc = styled(&gray_doc(), 1, &Bevel::default().json());
    let canonical = doc.layers[1]
        .style
        .as_ref()
        .expect("style stored")
        .to_json();
    assert!(
        canonical.contains("\"type\":\"bevel_emboss\""),
        "{canonical}"
    );
    assert!(
        canonical.contains("\"style\":\"inner_bevel\""),
        "{canonical}"
    );
    assert!(
        try_styled(&doc, 1, Some(&canonical))
            .expect("valid")
            .is_none(),
        "the echo is refused as unchanged"
    );
    assert!(
        try_styled(&doc, 1, Some(&Bevel::default().json()))
            .expect("valid")
            .is_none(),
        "and so is the original text"
    );
}
