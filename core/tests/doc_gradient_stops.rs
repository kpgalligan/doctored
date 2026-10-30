//! The byte stops gradient through `dr_doc_gradient_stops`, against a
//! closed-form expectation: the ramp parameter of each style written out for
//! an axis-aligned drag, then the colour between the two stops that bracket
//! it — the Classic ramp for Linear, Radial, Angle and Reflected
//! (`common::classic_ramp`), piecewise linear for Diamond, the
//! alpha piecewise linear in every style — composited source-over in f64. A
//! blend mode is checked against the W3C compositing formula written
//! out here.
use crate::common::{classic_ramp, layer_pixels};
use doctored_core::doc::DrDocument;
use doctored_core::doc_lock::{LOCK_PIXELS, LOCK_TRANSPARENCY};
use doctored_core::ffi_gradient::*;
use image::{Rgba, RgbaImage};
use std::f64::consts::TAU;
use std::ptr;

const SIDE: u32 = 8;
/// The drag every case uses: from (4, 4) four pixels to the right.
const P0: (f32, f32) = (4., 4.);
const P1: (f32, f32) = (8., 4.);

/// Position then straight RGBA, as the export takes them (before each
/// stop's two midpoints, [`flat`]).
type Stop = [f32; 5];

/// The export's 7 floats per stop: the stop, then its colour and opacity
/// midpoints at the default 0.5.
fn flat(stops: &[Stop]) -> Vec<f32> {
    stops
        .iter()
        .flat_map(|s| s.iter().copied().chain([0.5, 0.5]))
        .collect()
}

const STOPS: [Stop; 3] = [
    [0.0, 1.0, 0.0, 0.25, 1.0],
    [0.4, 0.0, 1.0, 0.5, 1.0],
    [1.0, 0.25, 0.5, 1.0, 1.0],
];

fn opaque(color: [u8; 4]) -> DrDocument {
    DrDocument::from_pixels(RgbaImage::from_pixel(SIDE, SIDE, Rgba(color)))
}

fn gradient(
    doc: &DrDocument,
    stops: &[Stop],
    kind: u32,
    mask: Option<&[u8]>,
) -> Option<Box<DrDocument>> {
    gradient_mode(doc, stops, kind, NORMAL, mask)
}

/// `DrBlendMode` raw values the cases name.
const NORMAL: i32 = 0;
const MULTIPLY: i32 = 1;
const SCREEN: i32 = 2;
const PASS_THROUGH: i32 = 27;

fn gradient_mode(
    doc: &DrDocument,
    stops: &[Stop],
    kind: u32,
    mode: i32,
    mask: Option<&[u8]>,
) -> Option<Box<DrDocument>> {
    let flat = flat(stops);
    let out = unsafe {
        dr_doc_gradient_stops(
            doc,
            0,
            P0.0,
            P0.1,
            P1.0,
            P1.1,
            flat.as_ptr(),
            stops.len(),
            kind,
            mode,
            1.0,
            mask.map_or(ptr::null(), <[u8]>::as_ptr),
        )
    };
    (!out.is_null()).then(|| unsafe { Box::from_raw(out) })
}

/// The ramp parameter at canvas pixel `(x, y)`'s centre for the horizontal
/// drag, per `DrGradientKind`: `u` along the drag, `v` down the canvas, both
/// over the drag's length of 4.
fn parameter(kind: u32, x: i32, y: i32) -> f64 {
    let u = (f64::from(x) + 0.5 - 4.) / 4.;
    let v = (f64::from(y) + 0.5 - 4.) / 4.;
    match kind {
        0 => u.clamp(0., 1.),
        1 => u.hypot(v).clamp(0., 1.),
        // Clockwise on the y-down canvas: straight down is a quarter turn.
        2 => (v.atan2(u) / TAU).rem_euclid(1.),
        3 => u.abs().clamp(0., 1.),
        _ => (u.abs() + v.abs()).clamp(0., 1.),
    }
}

/// Straight RGBA of `stops` at `t` under `kind`: piecewise linear, the
/// colour then the Classic ramp for Linear (0), Radial (1), Angle
/// (2) and Reflected (3).
fn ramp(stops: &[Stop], t: f64, kind: u32) -> [f64; 4] {
    let mut out = linear_ramp(stops, t);
    if kind <= 3 {
        for (c, v) in out.iter_mut().take(3).enumerate() {
            let channel: Vec<(f64, f64)> = stops
                .iter()
                .map(|s| (f64::from(s[0]), f64::from(s[1 + c])))
                .collect();
            *v = classic_ramp(&channel, t);
        }
    }
    out
}

/// Piecewise-linear straight RGBA of `stops` at `t`.
fn linear_ramp(stops: &[Stop], t: f64) -> [f64; 4] {
    let color = |s: &Stop| [s[1], s[2], s[3], s[4]].map(f64::from);
    if t <= f64::from(stops[0][0]) {
        return color(&stops[0]);
    }
    for pair in stops.windows(2) {
        let (p0, p1) = (f64::from(pair[0][0]), f64::from(pair[1][0]));
        if t < p1 {
            let f = (t - p0) / (p1 - p0);
            let (a, b) = (color(&pair[0]), color(&pair[1]));
            return std::array::from_fn(|i| a[i] + (b[i] - a[i]) * f);
        }
    }
    color(&stops[stops.len() - 1])
}

#[test]
fn each_style_follows_its_field_between_the_stops() {
    let doc = opaque([10, 20, 30, 255]);
    for kind in 0..5u32 {
        let out = gradient(&doc, &STOPS, kind, None).expect("a painted gradient");
        let pixels = layer_pixels(&*out, 0);
        for y in 0..SIDE as i32 {
            for x in 0..SIDE as i32 {
                // The Classic ramp may overshoot a stop; a stored byte is
                // clamped.
                let want = ramp(&STOPS, parameter(kind, x, y), kind).map(|v| v.clamp(0., 1.));
                let at = ((y as u32 * SIDE + x as u32) * 4) as usize;
                for c in 0..3 {
                    assert!(
                        (f64::from(pixels[at + c]) - want[c] * 255.).abs() <= 1.0,
                        "kind {kind} ({x},{y}) channel {c}: {} against {}",
                        pixels[at + c],
                        want[c] * 255.
                    );
                }
                assert_eq!(pixels[at + 3], 255);
            }
        }
    }
}

#[test]
fn stop_alpha_and_selection_coverage_scale_the_ink() {
    let before = [200u8, 100, 50, 255];
    let doc = opaque(before);
    let stops: [Stop; 3] = [
        [0.0, 1.0, 1.0, 1.0, 0.0],
        [0.5, 1.0, 0.0, 0.0, 1.0],
        [1.0, 0.0, 0.0, 1.0, 0.25],
    ];
    let mask: Vec<u8> = (0..SIDE * SIDE)
        .map(|i| [255u8, 0, 128][(i % 3) as usize])
        .collect();
    let out = gradient(&doc, &stops, 0, Some(&mask)).expect("a masked gradient");
    let pixels = layer_pixels(&*out, 0);
    for y in 0..SIDE as i32 {
        for x in 0..SIDE as i32 {
            let i = (y as u32 * SIDE + x as u32) as usize;
            let color = ramp(&stops, parameter(0, x, y), 0);
            let alpha = color[3] * f64::from(mask[i]) / 255.;
            for c in 0..3 {
                let want = f64::from(before[c]) * (1. - alpha) + color[c] * 255. * alpha;
                assert!(
                    (f64::from(pixels[i * 4 + c]) - want).abs() <= 1.0,
                    "({x},{y}) channel {c}: {} against {want}",
                    pixels[i * 4 + c]
                );
            }
            if mask[i] == 0 {
                assert_eq!(&pixels[i * 4..i * 4 + 4], &before);
            }
        }
    }
}

#[test]
fn an_offset_layer_samples_the_canvas_field() {
    // A 4×2 layer at canvas (3, 5) in an 8×8 canvas.
    let mut doc = DrDocument::from_pixels(RgbaImage::from_pixel(4, 2, Rgba([0, 0, 0, 255])));
    doc.width = SIDE;
    doc.height = SIDE;
    doc.layers[0].offset = (3, 5);
    let out = gradient(&doc, &STOPS, 4, None).expect("a painted gradient");
    let pixels = layer_pixels(&*out, 0);
    for ly in 0..2 {
        for lx in 0..4 {
            let want = ramp(&STOPS, parameter(4, lx + 3, ly + 5), 4);
            let at = ((ly * 4 + lx) * 4) as usize;
            for c in 0..3 {
                assert!((f64::from(pixels[at + c]) - want[c] * 255.).abs() <= 1.0);
            }
        }
    }
}

#[test]
fn a_transparent_layer_takes_the_stop_colour_and_alpha() {
    let doc = DrDocument::from_pixels(RgbaImage::new(SIDE, 1));
    let stops: [Stop; 2] = [[0.0, 1.0, 0.0, 0.0, 1.0], [1.0, 0.0, 0.0, 1.0, 0.0]];
    let flat = flat(&stops);
    let out = unsafe {
        dr_doc_gradient_stops(
            &doc,
            0,
            0.5,
            0.5,
            7.5,
            0.5,
            flat.as_ptr(),
            2,
            0,
            NORMAL,
            1.0,
            ptr::null(),
        )
    };
    assert!(!out.is_null());
    let out = unsafe { Box::from_raw(out) };
    let pixels = layer_pixels(&*out, 0);
    // The first pixel centre is the first stop, the last the transparent one.
    assert_eq!(&pixels[..4], &[255, 0, 0, 255]);
    assert_eq!(pixels[(SIDE as usize - 1) * 4 + 3], 0);
    // Halfway: half alpha.
    let mid = pixels[3 * 4 + 3];
    assert!(
        (f64::from(mid) - 255. * 4. / 7.).abs() <= 1.0,
        "alpha {mid}"
    );
}

#[test]
fn locks_gate_the_paint() {
    let mut locked = opaque([10, 20, 30, 255]);
    locked.layers[0].locks = LOCK_PIXELS;
    assert!(gradient(&locked, &STOPS, 0, None).is_none());
    // A transparency lock keeps every alpha; colour still changes.
    let mut frozen =
        DrDocument::from_pixels(RgbaImage::from_pixel(SIDE, SIDE, Rgba([9, 9, 9, 128])));
    frozen.layers[0].locks = LOCK_TRANSPARENCY;
    let out = gradient(&frozen, &STOPS, 0, None).expect("a recoloured layer");
    let pixels = layer_pixels(&*out, 0);
    assert!(pixels.chunks_exact(4).all(|p| p[3] == 128));
    assert!(pixels.chunks_exact(4).any(|p| p[..3] != [9, 9, 9]));
}

#[test]
fn refusals_answer_null() {
    let doc = opaque([10, 20, 30, 255]);
    // One stop, and thirty-three.
    assert!(gradient(&doc, &STOPS[..1], 0, None).is_none());
    let many: Vec<Stop> = (0..33)
        .map(|i| [i as f32 / 32., 0.5, 0.5, 0.5, 1.0])
        .collect();
    assert!(gradient(&doc, &many, 0, None).is_none());
    assert!(gradient(&doc, &many[..32], 0, None).is_some());
    // Positions out of order, a position or a colour out of range, NaN.
    let unordered: [Stop; 2] = [[0.6, 0., 0., 0., 1.], [0.2, 1., 1., 1., 1.]];
    assert!(gradient(&doc, &unordered, 0, None).is_none());
    let far: [Stop; 2] = [[0.0, 0., 0., 0., 1.], [1.5, 1., 1., 1., 1.]];
    assert!(gradient(&doc, &far, 0, None).is_none());
    let bright: [Stop; 2] = [[0.0, 0., 0., 0., 1.], [1.0, 1.5, 1., 1., 1.]];
    assert!(gradient(&doc, &bright, 0, None).is_none());
    let nan: [Stop; 2] = [[0.0, 0., f32::NAN, 0., 1.], [1.0, 1., 1., 1., 1.]];
    assert!(gradient(&doc, &nan, 0, None).is_none());
    // An unknown kind.
    assert!(gradient(&doc, &STOPS, 5, None).is_none());
    // A mode the fill refuses: unknown, and the group-only Pass Through.
    assert!(gradient_mode(&doc, &STOPS, 0, 99, None).is_none());
    assert!(gradient_mode(&doc, &STOPS, 0, -1, None).is_none());
    assert!(gradient_mode(&doc, &STOPS, 0, PASS_THROUGH, None).is_none());
    // Every stop transparent: nothing changes.
    let clear: [Stop; 2] = [[0.0, 1., 0., 0., 0.], [1.0, 0., 0., 1., 0.]];
    assert!(gradient(&doc, &clear, 0, None).is_none());
    // Equal endpoints, a NULL stops pointer, a NULL document.
    let flat = flat(&STOPS);
    unsafe {
        assert!(dr_doc_gradient_stops(
            &doc,
            0,
            1.,
            1.,
            1.,
            1.,
            flat.as_ptr(),
            3,
            0,
            0,
            1.0,
            ptr::null()
        )
        .is_null());
        assert!(dr_doc_gradient_stops(
            &doc,
            0,
            0.,
            0.,
            1.,
            1.,
            ptr::null(),
            3,
            0,
            0,
            1.0,
            ptr::null()
        )
        .is_null());
        assert!(dr_doc_gradient_stops(
            ptr::null(),
            0,
            0.,
            0.,
            1.,
            1.,
            flat.as_ptr(),
            3,
            0,
            0,
            1.0,
            ptr::null()
        )
        .is_null());
        // An index out of range.
        assert!(dr_doc_gradient_stops(
            &doc,
            4,
            0.,
            0.,
            1.,
            1.,
            flat.as_ptr(),
            3,
            0,
            0,
            1.0,
            ptr::null()
        )
        .is_null());
    }
}

/// One 8-bit level: the core composites in f32 and rounds once, the oracle
/// below in f64, so a value on a rounding boundary may land either side.
const BLEND_TOLERANCE: f64 = 1.0;

/// A separable blend function `B(cb, cs)`.
type Blend = fn(f64, f64) -> f64;

/// The W3C compositing step for a separable blend `b`, straight colours in
/// 0..1: backdrop `(cb, ab)`, source `(cs, sa)`. Returns colour and alpha.
fn w3c(b: Blend, cb: [f64; 3], ab: f64, cs: [f64; 3], sa: f64) -> ([f64; 3], f64) {
    let ao = sa + ab * (1. - sa);
    let co = std::array::from_fn(|c| {
        (sa * (1. - ab) * cs[c] + sa * ab * b(cb[c], cs[c]) + (1. - sa) * ab * cb[c]) / ao
    });
    (co, ao)
}

fn multiply(cb: f64, cs: f64) -> f64 {
    cb * cs
}

fn screen(cb: f64, cs: f64) -> f64 {
    cb + cs - cb * cs
}

#[test]
fn a_blend_mode_composites_each_stop_through_the_w3c_step() {
    // A semi-transparent backdrop, so all three terms of the formula count.
    let before = [200u8, 100, 50, 160];
    let doc = DrDocument::from_pixels(RgbaImage::from_pixel(SIDE, SIDE, Rgba(before)));
    let stops: [Stop; 3] = [
        [0.0, 1.0, 0.5, 0.25, 1.0],
        [0.5, 0.25, 1.0, 0.5, 0.5],
        [1.0, 0.5, 0.25, 1.0, 1.0],
    ];
    let mask: Vec<u8> = (0..SIDE * SIDE)
        .map(|i| [255u8, 0, 128][(i % 3) as usize])
        .collect();
    let modes: [(i32, Blend); 2] = [(MULTIPLY, multiply), (SCREEN, screen)];
    for (mode, blend) in modes {
        let out = gradient_mode(&doc, &stops, 0, mode, Some(&mask)).expect("a blended gradient");
        let pixels = layer_pixels(&*out, 0);
        let cb = [before[0], before[1], before[2]].map(|v| f64::from(v) / 255.);
        let ab = f64::from(before[3]) / 255.;
        for y in 0..SIDE as i32 {
            for x in 0..SIDE as i32 {
                let i = (y as u32 * SIDE + x as u32) as usize;
                let got = &pixels[i * 4..i * 4 + 4];
                if mask[i] == 0 {
                    assert_eq!(got, &before, "mode {mode} ({x},{y}) outside the selection");
                    continue;
                }
                let color = ramp(&stops, parameter(0, x, y), 0);
                let sa = color[3] * f64::from(mask[i]) / 255.;
                let (co, ao) = w3c(blend, cb, ab, [color[0], color[1], color[2]], sa);
                for c in 0..3 {
                    assert!(
                        (f64::from(got[c]) - co[c] * 255.).abs() <= BLEND_TOLERANCE,
                        "mode {mode} ({x},{y}) channel {c}: {} against {}",
                        got[c],
                        co[c] * 255.
                    );
                }
                assert!(
                    (f64::from(got[3]) - ao * 255.).abs() <= BLEND_TOLERANCE,
                    "mode {mode} ({x},{y}) alpha: {} against {}",
                    got[3],
                    ao * 255.
                );
            }
        }
    }
}

#[test]
fn multiply_over_an_opaque_colour_is_the_product_and_normal_is_unchanged() {
    let before = [200u8, 100, 50, 255];
    let doc = opaque(before);
    let flat: [Stop; 2] = [[0.0, 0.5, 0.5, 0.5, 1.0], [1.0, 0.5, 0.5, 0.5, 1.0]];
    let out = gradient_mode(&doc, &flat, 0, MULTIPLY, None).expect("a multiplied gradient");
    for pixel in layer_pixels(&*out, 0).chunks_exact(4) {
        for c in 0..3 {
            let want = f64::from(before[c]) * 0.5;
            assert!((f64::from(pixel[c]) - want).abs() <= BLEND_TOLERANCE);
        }
        assert_eq!(pixel[3], 255);
    }
    // Normal through the mode argument is the call every earlier case makes.
    let normal = gradient_mode(&doc, &STOPS, 3, NORMAL, None).expect("a painted gradient");
    let plain = gradient(&doc, &STOPS, 3, None).expect("a painted gradient");
    assert_eq!(layer_pixels(&*normal, 0), layer_pixels(&*plain, 0));
    // Multiply by white changes nothing: an exact no-op answers NULL.
    let white: [Stop; 2] = [[0.0, 1., 1., 1., 1.], [1.0, 1., 1., 1., 1.]];
    assert!(gradient_mode(&doc, &white, 0, MULTIPLY, None).is_none());
}

#[test]
fn a_transparency_lock_keeps_alpha_under_a_blend_mode() {
    let mut frozen =
        DrDocument::from_pixels(RgbaImage::from_pixel(SIDE, SIDE, Rgba([200, 100, 50, 128])));
    frozen.layers[0].locks = LOCK_TRANSPARENCY;
    let flat: [Stop; 2] = [[0.0, 0.5, 0.5, 0.5, 1.0], [1.0, 0.5, 0.5, 0.5, 1.0]];
    let out = gradient_mode(&frozen, &flat, 0, MULTIPLY, None).expect("a recoloured layer");
    // Frozen alpha: the colour is the W3C step against the same colour seen
    // opaque, lerp(Cb, Cb·Cs, sa) with sa = 1, and alpha does not move.
    for pixel in layer_pixels(&*out, 0).chunks_exact(4) {
        for (c, before) in [200.0f64, 100., 50.].into_iter().enumerate() {
            assert!((f64::from(pixel[c]) - before * 0.5).abs() <= BLEND_TOLERANCE);
        }
        assert_eq!(pixel[3], 128);
    }
}
