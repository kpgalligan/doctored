//! Gradient stop midpoints in the Gradient tool's stops requests: the
//! byte `dr_doc_gradient_stops` (7 floats a stop), the 16-bit paint
//! `gradient`, the `scalar_edit` `gradient`, `dr_selection_u16`'s
//! `gradient`, a CMYK document's `gradient` (inks) and a 32-bit float
//! document's `gradient`, each through its FFI entry and each against
//! `common::midpoint_t` (the take-2 description's move of `t` inside its
//! bracket, written in the test helpers) under the Classic ramp
//! (`common::smooth_ramp` at 1) for the colour and the linear ramp for the
//! alpha. Every case drags Linear from the left edge to the right one, so
//! pixel `x` reads `t = (x + ½) / W`. A request whose midpoints are all 0.5
//! paints exactly what one without them paints, and a midpoint outside
//! 0.05..=0.95 is refused.
use std::ptr;

use doctored_core::doc::DrDocument;
use doctored_core::ffi_gradient::dr_doc_gradient_stops;
use doctored_core::ffi_precision_edit::dr_precision_doc_edit_plan;
use doctored_core::ffi_selection_u16::dr_selection_u16_plan;
use doctored_core::icc::IccProfile;
use doctored_core::precision_document::PrecisionDocument;
use image::{Rgba, RgbaImage};
use serde_json::{json, Value};

use crate::cmyk_structure_fixture::{cmyk_layers, free as free_doc, RgbLayer};
use crate::common::{layer_pixels, midpoint_t, smooth_ramp, BLEND_NORMAL};
use crate::native_scalar::{doc, edit, error, scalar, selection, text, LIMIT};

const W: u32 = 16;
const H: u32 = 2;
const FROM: [f64; 2] = [0.0, 1.0];
const TO: [f64; 2] = [16.0, 1.0];
const REFUSAL: &str = "gradient stop midpoints must be finite in [0.05,0.95]";

/// Three stops: position and straight RGBA (opaque).
const STOPS: [(f64, [f64; 4]); 3] = [
    (0.0, [1.0, 0.0, 0.25, 1.0]),
    (0.3, [0.0, 1.0, 0.5, 1.0]),
    (1.0, [0.25, 0.5, 1.0, 1.0]),
];
/// White stops whose alpha carries the ramp.
const ALPHAS: [(f64, [f64; 4]); 3] = [
    (0.0, [1.0, 1.0, 1.0, 1.0]),
    (0.6, [1.0, 1.0, 1.0, 0.2]),
    (1.0, [1.0, 1.0, 1.0, 0.9]),
];
const MIDS: [f64; 3] = [0.25, 0.75, 0.5];
const OMIDS: [f64; 3] = [0.3, 0.8, 0.5];

fn t_at(x: u32) -> f64 {
    (f64::from(x) + 0.5) / f64::from(W)
}

fn channel(stops: &[(f64, [f64; 4])], c: usize) -> Vec<(f64, f64)> {
    stops.iter().map(|s| (s.0, s.1[c])).collect()
}

/// Component `c` of `stops` at `t`: the colour through `mids` on the Classic
/// ramp, alpha (`c` 3) through `omids` on the linear one.
fn want(stops: &[(f64, [f64; 4])], mids: &[f64], omids: &[f64], t: f64, c: usize) -> f64 {
    let ch = channel(stops, c);
    if c == 3 {
        smooth_ramp(&ch, midpoint_t(&ch, omids, t), 0.0)
    } else {
        smooth_ramp(&ch, midpoint_t(&ch, mids, t), 1.0).clamp(0.0, 1.0)
    }
}

/// The JSON stops, each midpoint written only off 0.5 unless `explicit`.
fn stops_json(stops: &[(f64, [f64; 4])], mids: &[f64], omids: &[f64], explicit: bool) -> Value {
    Value::Array(
        stops
            .iter()
            .enumerate()
            .map(|(k, (p, c))| {
                let mut s = json!({"position": p, "color": c});
                if explicit || mids[k] != 0.5 {
                    s["midpoint"] = json!(mids[k]);
                }
                if explicit || omids[k] != 0.5 {
                    s["opacity_midpoint"] = json!(omids[k]);
                }
                s
            })
            .collect(),
    )
}

// ------------------------------------------------------------------ byte --

fn byte(stops: &[(f64, [f64; 4])], mids: &[f64], omids: &[f64]) -> Option<Box<DrDocument>> {
    let doc = DrDocument::from_pixels(RgbaImage::from_pixel(W, H, Rgba([0, 0, 0, 255])));
    let flat: Vec<f32> = stops
        .iter()
        .enumerate()
        .flat_map(|(k, (p, c))| {
            [
                *p as f32,
                c[0] as f32,
                c[1] as f32,
                c[2] as f32,
                c[3] as f32,
                mids[k] as f32,
                omids[k] as f32,
            ]
        })
        .collect();
    let out = unsafe {
        dr_doc_gradient_stops(
            &doc,
            0,
            FROM[0] as f32,
            FROM[1] as f32,
            TO[0] as f32,
            TO[1] as f32,
            flat.as_ptr(),
            stops.len(),
            0,
            BLEND_NORMAL,
            1.0,
            ptr::null(),
        )
    };
    (!out.is_null()).then(|| unsafe { Box::from_raw(out) })
}

#[test]
fn the_byte_tool_moves_its_colour_and_alpha_through_the_midpoints() {
    let painted = byte(&STOPS, &MIDS, &[0.5; 3]).expect("a gradient");
    let pixels = layer_pixels(&*painted, 0);
    for y in 0..H {
        for x in 0..W {
            let at = ((y * W + x) * 4) as usize;
            for c in 0..3 {
                let w = want(&STOPS, &MIDS, &[0.5; 3], t_at(x), c) * 255.0;
                assert!(
                    (f64::from(pixels[at + c]) - w).abs() <= 1.0,
                    "({x},{y}) channel {c}: {} against {w}",
                    pixels[at + c]
                );
            }
        }
    }
    // White over black: red is the alpha.
    let painted = byte(&ALPHAS, &[0.5; 3], &OMIDS).expect("a gradient");
    let pixels = layer_pixels(&*painted, 0);
    for x in 0..W {
        let w = want(&ALPHAS, &[0.5; 3], &OMIDS, t_at(x), 3) * 255.0;
        let got = f64::from(pixels[(x * 4) as usize]);
        assert!((got - w).abs() <= 1.0, "alpha at {x}: {got} against {w}");
    }
    // All 0.5 is the plain gradient; out of range and NaN are refused.
    let plain = byte(&STOPS, &[0.5; 3], &[0.5; 3]).unwrap();
    assert_ne!(
        layer_pixels(&*plain, 0),
        layer_pixels(&*byte(&STOPS, &MIDS, &[0.5; 3]).unwrap(), 0)
    );
    for bad in [0.04, 0.96, f64::NAN] {
        assert!(byte(&STOPS, &[bad, 0.5, 0.5], &[0.5; 3]).is_none(), "{bad}");
        assert!(byte(&STOPS, &[0.5; 3], &[0.5, bad, 0.5]).is_none(), "{bad}");
    }
}

// ---------------------------------------------------------------- 16-bit --

fn paint_request(stops: Value) -> Value {
    json!({"op":"gradient","layer":0,"start":[1.,0.,0.25,1.],"end":[0.25,0.5,1.,1.],
        "color_space":"document","opacity":1.,"from":FROM,"to":TO,"radial":false,
        "style":"linear","stops":stops})
}

fn samples(d: &PrecisionDocument) -> Vec<[u16; 4]> {
    d.layer_source(0).unwrap().to_rgba16().ok().unwrap()
}

#[test]
fn the_16_bit_paint_tool_moves_through_the_midpoints() {
    let n = (W * H) as usize;
    let d = doc(W, H, vec![[0, 0, 0, 65535]; n]);
    let coverage = vec![65535u16; n];
    let paint = |stops: Value| samples(&edit(&d, paint_request(stops), &coverage).unwrap());
    let pixels = paint(stops_json(&STOPS, &MIDS, &[0.5; 3], false));
    for y in 0..H {
        for x in 0..W {
            let got = pixels[(y * W + x) as usize];
            for (c, &v) in got.iter().take(3).enumerate() {
                let w = want(&STOPS, &MIDS, &[0.5; 3], t_at(x), c) * 65535.0;
                assert!(
                    (f64::from(v) - w).abs() <= 1.5,
                    "({x},{y}) channel {c}: {v} against {w}"
                );
            }
        }
    }
    let pixels = paint(stops_json(&ALPHAS, &[0.5; 3], &OMIDS, false));
    for x in 0..W {
        let w = want(&ALPHAS, &[0.5; 3], &OMIDS, t_at(x), 3) * 65535.0;
        let got = f64::from(pixels[x as usize][0]);
        assert!((got - w).abs() <= 1.5, "alpha at {x}: {got} against {w}");
    }
    assert_eq!(
        paint(stops_json(&STOPS, &[0.5; 3], &[0.5; 3], true)),
        paint(stops_json(&STOPS, &[0.5; 3], &[0.5; 3], false)),
        "explicit 0.5 is no key"
    );
}

#[test]
fn the_scalar_and_selection_plane_gradients_move_through_the_midpoints() {
    let n = (W * H) as usize;
    let d = doc(W, H, vec![[1001, 3003, 5005, 65535]; n]);
    let gray = [
        (0.0, [0.1, 0.1, 0.1, 1.0]),
        (0.3, [0.9, 0.9, 0.9, 0.5]),
        (1.0, [0.4, 0.4, 0.4, 0.8]),
    ];
    let operation = json!({"kind":"gradient","from":FROM,"to":TO,"radial":false,
        "start":0,"end":65535,"opacity":1.0,"style":"linear",
        "stops":stops_json(&gray, &MIDS, &OMIDS, false)});
    let out = scalar(&d, "green", operation, false, &[]).expect("a scalar gradient");
    let pixels = samples(&out);
    for x in 0..W {
        let (g, a) = (
            want(&gray, &MIDS, &OMIDS, t_at(x), 0),
            want(&gray, &MIDS, &OMIDS, t_at(x), 3),
        );
        let w = 3003.0 * (1.0 - a) + g * 65535.0 * a;
        let got = f64::from(pixels[x as usize][1]);
        assert!((got - w).abs() <= 1.5, "scalar at {x}: {got} against {w}");
    }
    let input = vec![4242u16; n];
    let out = selection(
        W,
        H,
        json!({"kind":"gradient","from":FROM,"to":TO,"style":"linear",
            "stops":stops_json(&gray, &MIDS, &OMIDS, false),"opacity":1.0}),
        &input,
    );
    for x in 0..W {
        let (g, a) = (
            want(&gray, &MIDS, &OMIDS, t_at(x), 0),
            want(&gray, &MIDS, &OMIDS, t_at(x), 3),
        );
        let w = 4242.0 * (1.0 - a) + g * 65535.0 * a;
        let got = f64::from(out[x as usize]);
        assert!(
            (got - w).abs() <= 1.5,
            "selection at {x}: {got} against {w}"
        );
    }
}

// --------------------------------------------------------- CMYK and float --

#[test]
fn a_cmyk_gradient_moves_its_ink_and_alpha_through_the_midpoints() {
    let doc = cmyk_layers(
        W,
        H,
        vec![RgbLayer {
            name: "Clear",
            w: W,
            h: H,
            offset: (0, 0),
            pixels: vec![[0, 0, 0, 0]; (W * H) as usize],
        }],
    );
    let inks = [
        (0.0, [0.0, 0.2, 0.9, 0.0, 1.0]),
        (0.3, [0.5, 0.5, 0.0, 0.3, 0.4]),
        (1.0, [1.0, 0.0, 0.1, 0.8, 0.9]),
    ];
    let stops: Vec<Value> = inks
        .iter()
        .enumerate()
        .map(|(k, (p, ink))| {
            let mut s = json!({"position": p, "ink": ink});
            if MIDS[k] != 0.5 {
                s["midpoint"] = json!(MIDS[k]);
            }
            if OMIDS[k] != 0.5 {
                s["opacity_midpoint"] = json!(OMIDS[k]);
            }
            s
        })
        .collect();
    let request = json!({"op":"gradient","layer":0,"opacity":1,"from":FROM,"to":TO,
        "style":"linear","stops":stops});
    let stroke = vec![65535u16; (W * H) as usize];
    let out = edit(unsafe { &*doc }, request, &stroke).expect("a CMYK gradient");
    let samples = out.cmyk_layer_samples(0).expect("a CMYK record");
    for x in 0..W {
        let t = t_at(x);
        let s = samples[x as usize];
        for c in 0..5 {
            let ch: Vec<(f64, f64)> = inks.iter().map(|(p, i)| (*p, i[c])).collect();
            let w = if c == 4 {
                smooth_ramp(&ch, midpoint_t(&ch, &OMIDS, t), 0.0)
            } else {
                smooth_ramp(&ch, midpoint_t(&ch, &MIDS, t), 1.0).clamp(0.0, 1.0)
            } * 65535.0;
            assert!(
                (f64::from(s[c]) - w).abs() <= 2.0,
                "component {c} at {x}: {} against {w}",
                s[c]
            );
        }
    }
    free_doc(doc);
}

#[test]
fn a_float_gradient_moves_through_the_midpoints() {
    // Stop values of 0 and 1 are the same in linear light and encoded.
    let d = crate::float_fixture::float_doc(
        W,
        H,
        &IccProfile::srgb(),
        &[vec![[0.0, 0.0, 0.0, 1.0]; (W * H) as usize]],
    );
    let stops = [
        (0.0, [0.0, 1.0, 0.0, 1.0]),
        (0.4, [1.0, 0.0, 1.0, 1.0]),
        (1.0, [0.0, 1.0, 1.0, 1.0]),
    ];
    let coverage = vec![65535u16; (W * H) as usize];
    let out = edit(
        &d,
        paint_request(stops_json(&stops, &MIDS, &[0.5; 3], false)),
        &coverage,
    )
    .expect("a float gradient");
    let image = out.layer(0).unwrap().source().unwrap();
    for x in 0..W {
        let got = image.sample_at(x, 0).expect("a sample");
        for (c, &v) in got.iter().take(3).enumerate() {
            // A float sample keeps the Classic ramp's overshoot: unclamped.
            let ch = channel(&stops, c);
            let w = smooth_ramp(&ch, midpoint_t(&ch, &MIDS, t_at(x)), 1.0);
            assert!(
                (f64::from(v) - w).abs() <= 1e-4,
                "channel {c} at {x}: {v} against {w}"
            );
        }
    }
}

// --------------------------------------------------------------- refusals --

#[test]
fn a_midpoint_outside_5_to_95_percent_is_refused() {
    let n = (W * H) as usize;
    let d = doc(W, H, vec![[1, 2, 3, 65535]; n]);
    for (mids, omids) in [([0.04, 0.5, 0.5], [0.5; 3]), ([0.5; 3], [0.5, 0.951, 0.5])] {
        let stops = stops_json(&STOPS, &mids, &omids, false);
        for request in [
            paint_request(stops.clone()),
            json!({"op":"scalar_edit","target":"red","index":0,"selection":false,
                "operation":{"kind":"gradient","from":FROM,"to":TO,"radial":false,
                    "start":0,"end":65535,"style":"linear","stops":stops.clone()}}),
        ] {
            let t = text(request.clone());
            let mut e = ptr::null_mut();
            let p = unsafe { dr_precision_doc_edit_plan(&d, t.as_ptr(), LIMIT, 0, &mut e) };
            assert!(p.is_null(), "{request} was planned");
            assert!(
                unsafe { error(e) }.ends_with(REFUSAL),
                "{request}: the midpoint sentence"
            );
        }
        let t = text(json!({"kind":"gradient","from":FROM,"to":TO,"stops":stops}));
        let mut e = ptr::null_mut();
        let p = unsafe { dr_selection_u16_plan(W, H, t.as_ptr(), LIMIT, 0, &mut e) };
        assert!(p.is_null(), "a selection gradient was planned");
        assert!(unsafe { error(e) }.ends_with(REFUSAL));
    }
}
