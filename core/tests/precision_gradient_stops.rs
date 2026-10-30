//! Stops gradients at 16 bits: the paint tool's `gradient`, the
//! `scalar_edit` `gradient` and `dr_selection_u16`'s `gradient`, each through
//! its FFI entry and each against a closed-form expectation: the ramp
//! parameter of the style written out for an axis-aligned drag, then the
//! colour between the two stops that bracket it: the Classic ramp
//! for Linear, Radial, Angle and Reflected (`common::classic_ramp`),
//! piecewise linear for Diamond, the alpha piecewise linear in every style.
use crate::common::classic_ramp;
use crate::native_scalar::{doc, edit, error, q, scalar, selection, text, LIMIT};
use doctored_core::ffi_precision_edit::*;
use doctored_core::precision_document::PrecisionDocument;
use serde_json::{json, Value};
use std::f64::consts::TAU;
use std::ptr;

const SIDE: u32 = 8;
/// The drag every case uses: from (4, 4) four pixels to the right.
const FROM: [f64; 2] = [4., 4.];
const TO: [f64; 2] = [8., 4.];
const STYLES: [&str; 5] = ["linear", "radial", "angle", "reflected", "diamond"];

/// Three stops: position and straight RGBA.
const STOPS: [(f64, [f64; 4]); 3] = [
    (0.0, [1.0, 0.0, 0.25, 1.0]),
    (0.4, [0.0, 1.0, 0.5, 1.0]),
    (1.0, [0.25, 0.5, 1.0, 1.0]),
];

fn stops_json(stops: &[(f64, [f64; 4])]) -> Value {
    Value::Array(
        stops
            .iter()
            .map(|(p, c)| json!({"position": p, "color": c}))
            .collect(),
    )
}

/// The ramp parameter at pixel `(x, y)`'s centre for the horizontal drag
/// `FROM → TO`, written per style: `u` runs along the drag, `v` down the
/// canvas, both over the drag's length of 4.
fn parameter(style: &str, x: u32, y: u32) -> f64 {
    let u = (f64::from(x) + 0.5 - FROM[0]) / 4.;
    let v = (f64::from(y) + 0.5 - FROM[1]) / 4.;
    match style {
        "linear" => u.clamp(0., 1.),
        "radial" => u.hypot(v).clamp(0., 1.),
        // Clockwise on the y-down canvas: straight down is a quarter turn.
        "angle" => (v.atan2(u) / TAU).rem_euclid(1.),
        "reflected" => u.abs().clamp(0., 1.),
        _ => (u.abs() + v.abs()).clamp(0., 1.),
    }
}

/// The colour of `stops` at `t` under `style`: piecewise linear, the colour
/// then the Classic ramp for Linear, Radial, Angle and Reflected.
fn ramp(stops: &[(f64, [f64; 4])], t: f64, style: &str) -> [f64; 4] {
    let mut out = linear_ramp(stops, t);
    if matches!(style, "linear" | "radial" | "angle" | "reflected") {
        for (c, v) in out.iter_mut().take(3).enumerate() {
            let channel: Vec<(f64, f64)> = stops.iter().map(|s| (s.0, s.1[c])).collect();
            *v = classic_ramp(&channel, t);
        }
    }
    out
}

/// Piecewise-linear colour of `stops` at `t`.
fn linear_ramp(stops: &[(f64, [f64; 4])], t: f64) -> [f64; 4] {
    if t <= stops[0].0 {
        return stops[0].1;
    }
    for pair in stops.windows(2) {
        let ((p0, c0), (p1, c1)) = (pair[0], pair[1]);
        if t < p1 {
            let f = (t - p0) / (p1 - p0);
            return std::array::from_fn(|i| c0[i] + (c1[i] - c0[i]) * f);
        }
    }
    stops[stops.len() - 1].1
}

fn samples(d: &PrecisionDocument) -> Vec<[u16; 4]> {
    d.layer_source(0).unwrap().to_rgba16().ok().unwrap()
}

fn paint_request(style: Option<&str>, stops: Value) -> Value {
    let mut request = json!({"op":"gradient","layer":0,"start":[1.,0.,0.25,1.],
        "end":[0.25,0.5,1.,1.],"color_space":"document","opacity":1.,
        "from":FROM,"to":TO,"radial":false,"stops":stops});
    if let Some(style) = style {
        request["style"] = json!(style);
    }
    request
}

#[test]
fn paint_gradient_follows_each_style_between_its_stops() {
    let n = (SIDE * SIDE) as usize;
    // Opaque stops over an opaque layer: each pixel becomes the ramp colour.
    let d = doc(SIDE, SIDE, vec![[7000, 8000, 9000, 65535]; n]);
    let coverage = vec![65535u16; n];
    for style in STYLES {
        let out = edit(
            &d,
            paint_request(Some(style), stops_json(&STOPS)),
            &coverage,
        )
        .expect("a painted gradient");
        let pixels = samples(&out);
        for y in 0..SIDE {
            for x in 0..SIDE {
                let t = parameter(style, x, y);
                // The angle ramp wraps from the last stop to the first along
                // the drag itself; a pixel centre never lies on that ray here.
                // The Classic ramp may overshoot a stop; a stored sample is
                // clamped.
                let want = ramp(&STOPS, t, style).map(|v| v.clamp(0., 1.));
                let got = pixels[(y * SIDE + x) as usize];
                for c in 0..3 {
                    assert!(
                        (f64::from(got[c]) - want[c] * 65535.).abs() <= 1.5,
                        "{style} ({x},{y}) channel {c}: {} against {}",
                        got[c],
                        want[c] * 65535.
                    );
                }
                assert_eq!(got[3], 65535);
            }
        }
    }
}

#[test]
fn paint_gradient_stop_alpha_and_selection_scale_the_ink() {
    let n = (SIDE * SIDE) as usize;
    let before = [20000u16, 30000, 40000, 65535];
    let d = doc(SIDE, SIDE, vec![before; n]);
    let stops = [
        (0.0, [1.0, 1.0, 1.0, 0.0]),
        (0.5, [1.0, 0.0, 0.0, 1.0]),
        (1.0, [0.0, 0.0, 1.0, 0.25]),
    ];
    let stroke = vec![65535u16; n];
    let selected: Vec<u16> = (0..n).map(|i| [65535u16, 0, 32768][i % 3]).collect();
    let mut request = paint_request(Some("linear"), stops_json(&stops));
    request["selection"] = json!(true);
    request["opacity"] = json!(0.5);
    let out = edit(&d, request, &[stroke, selected.clone()].concat()).expect("a gradient");
    let pixels = samples(&out);
    for y in 0..SIDE {
        for x in 0..SIDE {
            let i = (y * SIDE + x) as usize;
            let color = ramp(&stops, parameter("linear", x, y), "linear");
            // Source-over an opaque pixel: lerp by the ink's alpha.
            let alpha = color[3] * 0.5 * f64::from(selected[i]) / 65535.;
            for c in 0..3 {
                let want = f64::from(before[c]) * (1. - alpha) + color[c] * 65535. * alpha;
                assert!(
                    (f64::from(pixels[i][c]) - want).abs() <= 1.5,
                    "({x},{y}) channel {c}: {} against {want}",
                    pixels[i][c]
                );
            }
            if selected[i] == 0 {
                assert_eq!(pixels[i], before);
            }
        }
    }
}

#[test]
fn absent_style_takes_the_radial_flag_and_two_stops_match_the_two_colour_request() {
    let n = (SIDE * SIDE) as usize;
    let d = doc(SIDE, SIDE, vec![[100, 200, 300, 65535]; n]);
    let coverage = vec![65535u16; n];
    let two = [(0.0, [1.0, 0.0, 0.25, 1.0]), (1.0, [0.25, 0.5, 1.0, 1.0])];
    for radial in [false, true] {
        let mut stops = paint_request(None, stops_json(&two));
        stops["radial"] = json!(radial);
        let mut legacy = stops.clone();
        legacy.as_object_mut().unwrap().remove("stops");
        let a = samples(&edit(&d, stops, &coverage).unwrap());
        let b = samples(&edit(&d, legacy, &coverage).unwrap());
        for (i, (a, b)) in a.iter().zip(&b).enumerate() {
            for c in 0..4 {
                assert!(a[c].abs_diff(b[c]) <= 1, "radial {radial} pixel {i}");
            }
        }
    }
}

#[test]
fn scalar_gradient_stops_write_gray_by_stop_alpha() {
    let n = (SIDE * SIDE) as usize;
    let d = doc(SIDE, SIDE, vec![[1001, 3003, 5005, 27001]; n]);
    let stops = [
        (0.0, [0.1, 0.1, 0.1, 1.0]),
        (0.3, [0.9, 0.9, 0.9, 0.5]),
        (1.0, [0.4, 0.4, 0.4, 0.0]),
    ];
    let selected: Vec<u16> = (0..n).map(|i| [65535u16, 12345, 0][i % 3]).collect();
    for style in STYLES {
        let operation = json!({"kind":"gradient","from":FROM,"to":TO,"radial":false,
            "start":6554,"end":26214,"opacity":0.8,"style":style,"stops":stops_json(&stops)});
        let out = scalar(&d, "green", operation, true, &selected).expect("a scalar gradient");
        let pixels = samples(&out);
        for y in 0..SIDE {
            for x in 0..SIDE {
                let i = (y * SIDE + x) as usize;
                let color = ramp(&stops, parameter(style, x, y), style);
                let a = 0.8 * color[3] * f64::from(selected[i]) / 65535.;
                let want = 3003. * (1. - a) + color[0] * 65535. * a;
                assert!(
                    (f64::from(pixels[i][1]) - want).abs() <= 1.5,
                    "{style} ({x},{y}): {} against {want}",
                    pixels[i][1]
                );
                assert_eq!(pixels[i][0], 1001);
                assert_eq!(pixels[i][2], 5005);
                assert_eq!(pixels[i][3], 27001);
            }
        }
    }
}

#[test]
fn selection_plane_gradient_paints_coverage_over_one_canvas_plane() {
    let n = (SIDE * SIDE) as usize;
    let input: Vec<u16> = (0..n).map(|i| (i * 997 % 65536) as u16).collect();
    let stops = [
        (0.0, [0.0, 0.0, 0.0, 1.0]),
        (0.6, [1.0, 1.0, 1.0, 0.5]),
        (1.0, [0.25, 0.25, 0.25, 1.0]),
    ];
    for style in STYLES {
        let out = selection(
            SIDE,
            SIDE,
            json!({"kind":"gradient","from":FROM,"to":TO,"style":style,
                "stops":stops_json(&stops),"opacity":0.75}),
            &input,
        );
        for y in 0..SIDE {
            for x in 0..SIDE {
                let i = (y * SIDE + x) as usize;
                let color = ramp(&stops, parameter(style, x, y), style);
                let a = 0.75 * color[3];
                let want = f64::from(input[i]) * (1. - a) + color[0] * 65535. * a;
                assert!(
                    (f64::from(out[i]) - want).abs() <= 1.5,
                    "{style} ({x},{y}): {} against {want}",
                    out[i]
                );
            }
        }
    }
    // Opaque black-to-white linear with opacity 1 replaces the plane exactly
    // at both ends of the ramp.
    let out = selection(
        SIDE,
        1,
        json!({"kind":"gradient","from":[0.5,0.5],"to":[7.5,0.5],
            "stops":[{"position":0.,"color":[0.,0.,0.,1.]},{"position":1.,"color":[1.,1.,1.,1.]}]}),
        &[4242; SIDE as usize],
    );
    assert_eq!(out[0], 0);
    assert_eq!(out[SIDE as usize - 1], 65535);
    // Between them the Classic ramp: the style is Linear when absent.
    assert_eq!(
        out[3],
        q(65535. * classic_ramp(&[(0., 0.), (1., 1.)], 3. / 7.))
    );
}

#[test]
fn stops_requests_are_refused_by_rule() {
    let n = (SIDE * SIDE) as usize;
    let d = doc(SIDE, SIDE, vec![[1, 2, 3, 65535]; n]);
    let one = stops_json(&STOPS[..1]);
    let many = Value::Array(
        (0..33)
            .map(|i| json!({"position": f64::from(i) / 32., "color": [0., 0., 0., 1.]}))
            .collect(),
    );
    let unordered = json!([{"position":0.5,"color":[0.,0.,0.,1.]},
        {"position":0.25,"color":[1.,1.,1.,1.]}]);
    let out_of_range = json!([{"position":0.,"color":[0.,0.,0.,1.]},
        {"position":1.5,"color":[1.,1.,1.,1.]}]);
    let mut requests = Vec::new();
    for bad in [one, many, unordered, out_of_range] {
        requests.push(paint_request(Some("linear"), bad.clone()));
        requests.push(
            json!({"op":"scalar_edit","target":"red","index":0,"selection":false,
            "operation":{"kind":"gradient","from":FROM,"to":TO,"radial":false,
                "start":0,"end":65535,"stops":bad}}),
        );
    }
    // A style without stops has nothing to shape.
    let mut bare = paint_request(Some("angle"), json!([]));
    bare.as_object_mut().unwrap().remove("stops");
    requests.push(bare);
    requests.push(
        json!({"op":"scalar_edit","target":"red","index":0,"selection":false,
        "operation":{"kind":"gradient","from":FROM,"to":TO,"radial":false,
            "start":0,"end":65535,"style":"diamond"}}),
    );
    // A selection-plane gradient is not a scalar morphology.
    requests.push(
        json!({"op":"scalar_edit","target":"red","index":0,"selection":false,
        "operation":{"kind":"morphology","operation":{"kind":"gradient","from":FROM,"to":TO,
            "stops":stops_json(&STOPS)}}}),
    );
    for request in requests {
        let t = text(request.clone());
        let mut e = ptr::null_mut();
        let p = unsafe { dr_precision_doc_edit_plan(&d, t.as_ptr(), LIMIT, 0, &mut e) };
        assert!(p.is_null(), "{request} was planned");
        assert!(!unsafe { error(e) }.is_empty(), "{request} named no reason");
    }
}
