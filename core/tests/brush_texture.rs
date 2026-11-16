//! Brush dynamics' texture in the native rasterizer (`dr_brush_texture_create`,
//! `dr_brush_u16_textured`, `dr_brush_overlay_rgba8_textured`; `brush_texture`,
//! `brush_coverage` THE TEXTURED STROKE), black-box through the C ABI against an
//! oracle written here from the rule alone: each dab's coverage (the rotated
//! ellipse, the soft smoothstep at the pixel centre, the hard tip's 8×8 sample
//! mean) times its deposit times `1 − depth·(1 − t)`, where `t` is the Gray
//! luminosity (0.30, 0.59, 0.11) of the tile's PREMULTIPLIED colour at the
//! pixel's canvas coordinate with the tile anchored at the canvas origin —
//! scale 1 reads texel (x mod w, y mod h), scale 2 the corner-aligned wrapping
//! bilinear at (x / 2, y / 2), scale 0.5 on an even tile the mean of the 2×2
//! texel box at (2x, 2y) — then source-over accumulation in dab order and one
//! rounding to a u16 code. The oracle reads t in f64 where the core reads the
//! luminosity in f32, so a stroke compares within one code; the exact claims
//! (depth 0, a white tile, a transparent texel, a moved origin) compare bit
//! for bit.
use crate::native_scalar::{error, text, LIMIT};
use doctored_core::ffi_brush::*;
use doctored_core::ffi_brush_texture::*;
use serde_json::{json, Value};
use std::ptr;

const W: u32 = 40;
const H: u32 = 30;

const RANGE: &str = "native brush texture holds a tile of 1 to 512 pixels a side, a scale from 0.01 to 10 and a depth from 0 to 1";
const STAMPED: &str = "native brush texture requires a stamped tip that is not aliased";

struct Tile {
    w: u32,
    h: u32,
    rgba: Vec<u8>,
}

/// A 5 × 3 tile of greys, colours, a transparent and two part-transparent
/// texels: odd sides, so the footprint's corner is never a tile corner.
fn mixed() -> Tile {
    let texels: [[u8; 4]; 15] = [
        [0, 0, 0, 255],
        [255, 255, 255, 255],
        [128, 128, 128, 255],
        [255, 0, 0, 255],
        [0, 0, 255, 255],
        [255, 255, 255, 0],
        [255, 255, 255, 128],
        [64, 200, 30, 255],
        [255, 255, 255, 255],
        [10, 10, 10, 255],
        [200, 200, 200, 255],
        [0, 255, 0, 255],
        [90, 90, 90, 64],
        [255, 255, 255, 255],
        [30, 60, 90, 255],
    ];
    Tile {
        w: 5,
        h: 3,
        rgba: texels.concat(),
    }
}

/// A 4 × 6 tile for scale 0.5 (even sides: one halving is the 2×2 box).
fn even() -> Tile {
    let rgba = (0..24u32)
        .flat_map(|i| {
            let v = ((i * 53) % 256) as u8;
            [
                v,
                255 - v,
                (i * 11) as u8,
                if i % 7 == 3 { 100 } else { 255 },
            ]
        })
        .collect();
    Tile { w: 4, h: 6, rgba }
}

fn white() -> Tile {
    Tile {
        w: 3,
        h: 2,
        rgba: vec![255; 24],
    }
}

/// The texture handle, freed on drop.
struct Texture(*mut DrBrushTexture);

impl Drop for Texture {
    fn drop(&mut self) {
        unsafe { dr_brush_texture_free(self.0) }
    }
}

fn texture(tile: &Tile, scale: f64, depth: f64) -> Result<Texture, String> {
    let mut e = ptr::null_mut();
    let t = unsafe {
        dr_brush_texture_create(tile.rgba.as_ptr(), tile.w, tile.h, scale, depth, &mut e)
    };
    if t.is_null() {
        Err(unsafe { error(e) })
    } else {
        Ok(Texture(t))
    }
}

fn request(hardness: f64) -> Value {
    json!({"version":1,"hardness":hardness,"roundness":1.,"angle_degrees":0.,"pixel":false})
}

fn plain(request: &Value, dabs: &[f64]) -> Vec<u16> {
    let t = text(request.clone());
    let mut out = vec![777; (W * H) as usize];
    let mut e = ptr::null_mut();
    let ok = unsafe {
        dr_brush_u16(
            W,
            H,
            t.as_ptr(),
            dabs.as_ptr(),
            dabs.len() / 4,
            out.as_mut_ptr(),
            out.len(),
            LIMIT,
            0,
            ptr::null(),
            &mut e,
        )
    };
    assert!(ok, "{}", unsafe { error(e) });
    out
}

fn shaped(request: &Value, dabs: &[f64], shapes: &[f64]) -> Vec<u16> {
    let t = text(request.clone());
    let mut out = vec![777; (W * H) as usize];
    let mut e = ptr::null_mut();
    let ok = unsafe {
        dr_brush_u16_shaped(
            W,
            H,
            t.as_ptr(),
            dabs.as_ptr(),
            dabs.len() / 4,
            shapes.as_ptr(),
            out.as_mut_ptr(),
            out.len(),
            LIMIT,
            0,
            ptr::null(),
            &mut e,
        )
    };
    assert!(ok, "{}", unsafe { error(e) });
    out
}

/// `dr_brush_u16_textured` on a `w` x `h` canvas whose pixel (0, 0) is canvas
/// `origin`, or its refusal; a refusal must leave the output untouched.
fn textured_at(
    (w, h): (u32, u32),
    request: &Value,
    dabs: &[f64],
    shapes: Option<&[f64]>,
    texture: *const DrBrushTexture,
    origin: (i64, i64),
) -> Result<Vec<u16>, String> {
    let t = text(request.clone());
    let mut out = vec![777; (w * h) as usize];
    let mut e = ptr::null_mut();
    let ok = unsafe {
        dr_brush_u16_textured(
            w,
            h,
            t.as_ptr(),
            dabs.as_ptr(),
            dabs.len() / 4,
            shapes.map_or(ptr::null(), <[f64]>::as_ptr),
            texture,
            origin.0,
            origin.1,
            out.as_mut_ptr(),
            out.len(),
            LIMIT,
            0,
            ptr::null(),
            &mut e,
        )
    };
    if ok {
        Ok(out)
    } else {
        assert!(out.iter().all(|&v| v == 777), "a refusal wrote output");
        Err(unsafe { error(e) })
    }
}

fn textured(
    request: &Value,
    dabs: &[f64],
    shapes: Option<&[f64]>,
    texture: &Texture,
) -> Result<Vec<u16>, String> {
    textured_at((W, H), request, dabs, shapes, texture.0, (0, 0))
}

/// `dr_brush_overlay_rgba8_textured` over the whole canvas, or its refusal.
fn overlay(
    request: &Value,
    dabs: &[f64],
    texture: *const DrBrushTexture,
    ink: [u8; 3],
    selection: Option<&[u8]>,
) -> Result<Vec<u8>, String> {
    let t = text(request.clone());
    let mut out = vec![77; (W * H * 4) as usize];
    let mut e = ptr::null_mut();
    let ok = unsafe {
        dr_brush_overlay_rgba8_textured(
            W,
            H,
            t.as_ptr(),
            dabs.as_ptr(),
            dabs.len() / 4,
            ptr::null(),
            texture,
            0,
            0,
            ink.as_ptr(),
            selection.map_or(ptr::null(), <[u8]>::as_ptr),
            selection.map_or(0, <[u8]>::len),
            out.as_mut_ptr(),
            out.len(),
            LIMIT,
            0,
            ptr::null(),
            &mut e,
        )
    };
    if ok {
        Ok(out)
    } else {
        assert!(out.iter().all(|&v| v == 77), "a refusal wrote output");
        Err(unsafe { error(e) })
    }
}

/// The rule's `t` at canvas pixel (x, y) (module doc): the Gray luminosity of
/// the placed tile's premultiplied colour, which is the straight colour's
/// luminosity times its alpha.
fn oracle_t(tile: &Tile, scale: f64, x: i64, y: i64) -> f64 {
    let (w, h) = (i64::from(tile.w), i64::from(tile.h));
    let texel = |i: i64, j: i64| -> [f64; 4] {
        let k = ((j.rem_euclid(h) * w + i.rem_euclid(w)) * 4) as usize;
        let p = &tile.rgba[k..k + 4];
        let a = f64::from(p[3]) / 255.;
        let c = |n: usize| f64::from(p[n]) / 255. * a;
        [c(0), c(1), c(2), a]
    };
    let mix = |taps: &[([f64; 4], f64)]| -> [f64; 4] {
        let mut sum = [0.; 4];
        for (p, k) in taps {
            for c in 0..4 {
                sum[c] += p[c] * k;
            }
        }
        sum
    };
    let p = if scale == 1. {
        texel(x, y)
    } else if scale == 2. {
        let (u, v) = (x as f64 / 2., y as f64 / 2.);
        let (i, j) = (u.floor() as i64, v.floor() as i64);
        let (fu, fv) = (u - u.floor(), v - v.floor());
        mix(&[
            (texel(i, j), (1. - fu) * (1. - fv)),
            (texel(i + 1, j), fu * (1. - fv)),
            (texel(i, j + 1), (1. - fu) * fv),
            (texel(i + 1, j + 1), fu * fv),
        ])
    } else if scale == 0.5 {
        assert!(tile.w.is_multiple_of(2) && tile.h.is_multiple_of(2));
        let (i, j) = (2 * x, 2 * y);
        mix(&[
            (texel(i, j), 0.25),
            (texel(i + 1, j), 0.25),
            (texel(i, j + 1), 0.25),
            (texel(i + 1, j + 1), 0.25),
        ])
    } else {
        unreachable!("the oracle covers scales 1, 2 and 0.5")
    };
    0.30 * p[0] + 0.59 * p[1] + 0.11 * p[2]
}

fn oracle_factor(tile: &Tile, scale: f64, depth: f64, x: i64, y: i64) -> f64 {
    1. - depth * (1. - oracle_t(tile, scale, x, y))
}

/// One dab's coverage of pixel (x, y) at `angle` and `roundness`, before its
/// deposit (the ellipse's normalized distance; soft: smoothstep at the pixel
/// centre; hard: the mean of 8×8 samples inside).
fn oracle_pixel(dab: &[f64], hardness: f64, angle: f64, roundness: f64, x: u32, y: u32) -> f64 {
    let (cx, cy, r) = (dab[0], dab[1], dab[2] / 2.);
    let (s, c) = angle.to_radians().sin_cos();
    let distance = |px: f64, py: f64| {
        let (dx, dy) = (px - cx, py - cy);
        let (u, v) = (c * dx - s * dy, s * dx + c * dy);
        (u / r).hypot(v / (r * roundness))
    };
    if hardness < 1. {
        let t = distance(f64::from(x) + 0.5, f64::from(y) + 0.5);
        if t <= hardness {
            1.
        } else if t >= 1. {
            0.
        } else {
            let k = (t - hardness) / (1. - hardness);
            1. - k * k * (3. - 2. * k)
        }
    } else {
        let mut inside = 0;
        for sy in 0..8 {
            for sx in 0..8 {
                let px = f64::from(x) + f64::from(2 * sx + 1) / 16.;
                let py = f64::from(y) + f64::from(2 * sy + 1) / 16.;
                if distance(px, py) <= 1. {
                    inside += 1;
                }
            }
        }
        f64::from(inside) / 64.
    }
}

/// The oracle's stroke: each dab at its shape (the request's 0°, 1 without
/// one), times its deposit and `factor` at the pixel, source-over in order,
/// one rounding.
fn oracle(
    hardness: f64,
    dabs: &[f64],
    shapes: Option<&[f64]>,
    factor: impl Fn(i64, i64) -> f64,
) -> Vec<u16> {
    let mut field = vec![0_f64; (W * H) as usize];
    for (i, dab) in dabs.chunks_exact(4).enumerate() {
        let (angle, roundness) = shapes.map_or((0., 1.), |s| (s[2 * i], s[2 * i + 1]));
        for y in 0..H {
            for x in 0..W {
                let c = oracle_pixel(dab, hardness, angle, roundness, x, y)
                    * dab[3]
                    * factor(i64::from(x), i64::from(y));
                let old = &mut field[(y * W + x) as usize];
                *old += c * (1. - *old);
            }
        }
    }
    field.iter().map(|v| (v * 65535.).round() as u16).collect()
}

/// The pixels where `got` and `want` differ by more than one code.
fn misses(got: &[u16], want: &[u16]) -> usize {
    got.iter()
        .zip(want)
        .filter(|(a, b)| a.abs_diff(**b) > 1)
        .count()
}

const DABS: [f64; 20] = [
    10.3, 9.7, 14., 1., //
    16.8, 13.2, 11.5, 0.6, //
    22.4, 15.9, 17., 0.85, //
    27.1, 18.6, 9.25, 1., //
    31.6, 20.3, 13., 0.45,
];

#[test]
fn a_hard_dab_interior_takes_its_deposit_times_the_factor() {
    let tile = mixed();
    // The box starts at x = 8, which is not a multiple of the tile's 5.
    let dab = |deposit: f64| [20.375, 15.625, 24., deposit];
    let interior: Vec<usize> = plain(&request(1.), &dab(1.))
        .iter()
        .enumerate()
        .filter(|(_, &v)| v == 65535)
        .map(|(i, _)| i)
        .collect();
    assert!(interior.len() > 300);
    for depth in [1., 0.5] {
        let tex = texture(&tile, 1., depth).unwrap();
        for deposit in [1., 0.4] {
            let got = textured(&request(1.), &dab(deposit), None, &tex).unwrap();
            for &i in &interior {
                let (x, y) = ((i as u32 % W) as i64, (i as u32 / W) as i64);
                let want = (deposit * oracle_factor(&tile, 1., depth, x, y) * 65535.).round();
                assert!(
                    (f64::from(got[i]) - want).abs() <= 1.,
                    "depth {depth} deposit {deposit} ({x}, {y}): {} against {want}",
                    got[i]
                );
            }
            let want = oracle(1., &dab(deposit), None, |x, y| {
                oracle_factor(&tile, 1., depth, x, y)
            });
            assert_eq!(misses(&got, &want), 0, "depth {depth} deposit {deposit}");
        }
    }
}

#[test]
fn overlapping_dabs_accumulate_their_textured_coverage_in_order() {
    let tile = mixed();
    let dabs = [14.5, 12.25, 16., 1., 22.125, 16.75, 14., 0.7];
    for hardness in [1., 0.6] {
        for depth in [1., 0.6] {
            let tex = texture(&tile, 1., depth).unwrap();
            let got = textured(&request(hardness), &dabs, None, &tex).unwrap();
            let want = oracle(hardness, &dabs, None, |x, y| {
                oracle_factor(&tile, 1., depth, x, y)
            });
            assert_eq!(misses(&got, &want), 0, "hardness {hardness} depth {depth}");
            assert_ne!(
                got,
                plain(&request(hardness), &dabs),
                "the texture changed nothing"
            );
        }
    }
}

#[test]
fn scale_reads_the_pyramid_at_one_two_and_one_half() {
    for (tile, scale) in [(mixed(), 1.), (mixed(), 2.), (even(), 0.5)] {
        for hardness in [1., 0.3] {
            for depth in [1., 0.6] {
                let tex = texture(&tile, scale, depth).unwrap();
                let got = textured(&request(hardness), &DABS, None, &tex).unwrap();
                let want = oracle(hardness, &DABS, None, |x, y| {
                    oracle_factor(&tile, scale, depth, x, y)
                });
                assert_eq!(
                    misses(&got, &want),
                    0,
                    "scale {scale} hardness {hardness} depth {depth}"
                );
            }
        }
    }
}

#[test]
fn depth_zero_and_an_opaque_white_tile_give_the_plain_stroke_bit_for_bit() {
    let shapes = [30., 0.4, -75., 0.9, 180., 0.2, -180., 1., 111.5, 0.01];
    for hardness in [0., 0.35, 1.] {
        let r = request(hardness);
        let want = plain(&r, &DABS);
        let want_shaped = shaped(&r, &DABS, &shapes);
        for (tile, scale, depth) in [
            (mixed(), 1., 0.),
            (mixed(), 2., 0.),
            (white(), 1., 1.),
            (white(), 0.37, 1.),
            (white(), 3.3, 0.8),
        ] {
            let tex = texture(&tile, scale, depth).unwrap();
            assert_eq!(
                textured(&r, &DABS, None, &tex).unwrap(),
                want,
                "{hardness} {scale} {depth}"
            );
            assert_eq!(
                textured(&r, &DABS, Some(&shapes), &tex).unwrap(),
                want_shaped,
                "shaped {hardness} {scale} {depth}"
            );
        }
    }
}

#[test]
fn a_texels_alpha_counts_as_its_luminosity_does() {
    // Transparent white, half-transparent white: t is 0, then 128 / 255.
    let tile = Tile {
        w: 2,
        h: 1,
        rgba: vec![255, 255, 255, 0, 255, 255, 255, 128],
    };
    let dab = [20.5, 15.5, 22., 1.];
    let full = plain(&request(1.), &dab);
    for depth in [1., 0.5] {
        let tex = texture(&tile, 1., depth).unwrap();
        let got = textured(&request(1.), &dab, None, &tex).unwrap();
        let mut seen = 0;
        for (i, (&g, &p)) in got.iter().zip(&full).enumerate() {
            if p != 65535 {
                continue;
            }
            seen += 1;
            let t = if i % 2 == 0 { 0. } else { 128. / 255. };
            let want = ((1. - depth * (1. - t)) * 65535.).round();
            assert!((f64::from(g) - want).abs() <= 1., "{i}: {g} against {want}");
            if depth == 1. && i % 2 == 0 {
                assert_eq!(g, 0, "a transparent texel took paint at depth 1");
            }
        }
        assert!(seen > 200);
    }
}

#[test]
fn a_textured_shaped_stroke_is_the_shaped_stroke_times_the_factor() {
    let tile = mixed();
    let shapes = [30., 0.4, -75., 0.9, 180., 0.2, -180., 1., 111.5, 0.6];
    for hardness in [1., 0.5] {
        let r = request(hardness);
        let tex = texture(&tile, 1., 0.8).unwrap();
        let got = textured(&r, &DABS, Some(&shapes), &tex).unwrap();
        let want = oracle(hardness, &DABS, Some(&shapes), |x, y| {
            oracle_factor(&tile, 1., 0.8, x, y)
        });
        assert_eq!(misses(&got, &want), 0, "hardness {hardness}");
    }
    // One shaped dab: wherever the shaped stroke is whole, the textured one is
    // the factor.
    let dab = [19.5, 14.5, 26., 1.];
    let one = shaped(&request(1.), &dab, &[30., 0.5]);
    let tex = texture(&tile, 1., 0.8).unwrap();
    let got = textured(&request(1.), &dab, Some(&[30., 0.5]), &tex).unwrap();
    let mut seen = 0;
    for (i, (&g, &s)) in got.iter().zip(&one).enumerate() {
        if s == 65535 {
            seen += 1;
            let (x, y) = ((i as u32 % W) as i64, (i as u32 / W) as i64);
            let want = (oracle_factor(&tile, 1., 0.8, x, y) * 65535.).round();
            assert!(
                (f64::from(g) - want).abs() <= 1.,
                "({x}, {y}): {g} against {want}"
            );
        } else if s == 0 {
            assert_eq!(g, 0);
        }
    }
    assert!(seen > 100);
}

#[test]
fn a_sub_canvas_at_its_origin_is_the_crop_of_the_whole_canvas() {
    // Coordinates in eighths, so shifting them by whole pixels is exact.
    let dabs = [
        10.375, 9.625, 14., 1., //
        16.75, 13.25, 11.5, 0.6, //
        22.5, 15.875, 17., 0.85, //
        31.625, 20.25, 13., 0.45,
    ];
    let (ox, oy, sw, sh) = (7_u32, 5_u32, 20_u32, 14_u32);
    let shifted: Vec<f64> = dabs
        .chunks_exact(4)
        .flat_map(|d| [d[0] - f64::from(ox), d[1] - f64::from(oy), d[2], d[3]])
        .collect();
    for (tile, scale) in [(mixed(), 1.), (mixed(), 2.), (even(), 0.5)] {
        for hardness in [1., 0.4] {
            let r = request(hardness);
            let tex = texture(&tile, scale, 0.9).unwrap();
            let whole = textured(&r, &dabs, None, &tex).unwrap();
            let part = textured_at(
                (sw, sh),
                &r,
                &shifted,
                None,
                tex.0,
                (i64::from(ox), i64::from(oy)),
            )
            .unwrap();
            let crop: Vec<u16> = (oy..oy + sh)
                .flat_map(|y| (ox..ox + sw).map(move |x| (x, y)))
                .map(|(x, y)| whole[(y * W + x) as usize])
                .collect();
            assert_eq!(part, crop, "scale {scale} hardness {hardness}");
            assert!(part.iter().any(|&v| v > 0));
        }
    }
}

#[test]
fn the_textured_overlay_is_the_overlay_rule_over_the_textured_coverage() {
    let tile = mixed();
    let tex = texture(&tile, 2., 0.75).unwrap();
    let r = request(0.7);
    let coverage = textured(&r, &DABS, None, &tex).unwrap();
    let ink = [200, 40, 91];
    let selection: Vec<u8> = (0..W * H).map(|i| ((i * 37) % 256) as u8).collect();
    let round = |x: u32, d: u32| (2 * x + d) / (2 * d);
    for mask in [None, Some(selection.as_slice())] {
        let got = overlay(&r, &DABS, tex.0, ink, mask).unwrap();
        for (i, &c) in coverage.iter().enumerate() {
            let mut a = round(u32::from(c) * 255, 65535);
            if let Some(m) = mask {
                a = round(a * u32::from(m[i]), 255);
            }
            let want: Vec<u8> = ink
                .iter()
                .map(|&v| round(u32::from(v) * a, 255) as u8)
                .chain([a as u8])
                .collect();
            assert_eq!(&got[i * 4..i * 4 + 4], want.as_slice(), "pixel {i}");
        }
    }
}

#[test]
fn texture_refusals_name_their_rule_and_leave_the_output_untouched() {
    let tile = mixed();
    for (scale, depth) in [
        (0.005, 1.),
        (10.5, 1.),
        (f64::NAN, 1.),
        (f64::INFINITY, 1.),
        (1., -0.01),
        (1., 1.01),
        (1., f64::NAN),
    ] {
        assert_eq!(
            texture(&tile, scale, depth).err().as_deref(),
            Some(RANGE),
            "{scale} {depth}"
        );
    }
    let big = vec![255_u8; 513 * 4];
    for (w, h, rgba) in [
        (0, 3, &tile.rgba),
        (5, 0, &tile.rgba),
        (513, 1, &big),
        (1, 513, &big),
    ] {
        let t = Tile {
            w,
            h,
            rgba: rgba.clone(),
        };
        assert_eq!(
            texture(&t, 1., 1.).err().as_deref(),
            Some(RANGE),
            "{w} x {h}"
        );
    }
    let mut e = ptr::null_mut();
    assert!(unsafe { dr_brush_texture_create(ptr::null(), 2, 2, 1., 1., &mut e) }.is_null());
    assert_eq!(unsafe { error(e) }, RANGE);
    // The ends of every range are accepted.
    let side = Tile {
        w: 512,
        h: 1,
        rgba: vec![128; 512 * 4],
    };
    for (t, scale, depth) in [(&tile, 0.01, 0.), (&tile, 10., 1.), (&side, 1., 0.5)] {
        assert!(texture(t, scale, depth).is_ok(), "{scale} {depth}");
    }
    let tex = texture(&tile, 1., 1.).unwrap();
    let r = request(0.5);
    assert_eq!(
        textured_at((W, H), &r, &DABS, None, ptr::null(), (0, 0)).unwrap_err(),
        RANGE
    );
    assert_eq!(
        overlay(&r, &DABS, ptr::null(), [1, 2, 3], None).unwrap_err(),
        RANGE
    );
    let pixel = json!({"version":1,"hardness":1.,"roundness":1.,"angle_degrees":0.,"pixel":true});
    let aliased = json!({"version":1,"hardness":1.,"roundness":1.,"angle_degrees":0.,
                         "pixel":false,"aliased":true});
    for r in [pixel, aliased] {
        assert_eq!(textured(&r, &DABS, None, &tex).unwrap_err(), STAMPED, "{r}");
        assert_eq!(
            overlay(&r, &DABS, tex.0, [1, 2, 3], None).unwrap_err(),
            STAMPED,
            "{r}"
        );
    }
    let bad = [10., 0.5, 10., 0.005, 10., 0.5, 10., 0.5, 10., 0.5];
    assert_eq!(
        textured(&r, &DABS, Some(&bad), &tex).unwrap_err(),
        "native brush shapes hold an angle from -180 to 180 and a roundness from 0.01 to 1 per dab"
    );
}
