//! The Gradient tool's own Smoothness: its stops requests take the
//! Smoothness as a fraction `k` (1 = 100 %, the default) and
//! blend the linear and Classic ramps by it through the fill layers' one
//! sampler, unbounded (a Gradient Fill layer holds its Classic term to the
//! bracketing stops; the tool's bound is unmeasured). The oracle
//! is `common::smooth_ramp` (the linear and Classic ramps blended by `k`,
//! written from the take-2 law, never read off the core) for the byte
//! `dr_doc_gradient_stops` and the 16-bit paint `gradient` at k 0, ½ and 1
//! over the drag a 0° Gradient Fill layer matches, and at k ½ for
//! the `scalar_edit` `gradient`, `dr_selection_u16`'s `gradient`, a CMYK
//! document's `gradient` (inks; alpha linear) and a 32-bit float document's
//! `gradient` (unclamped) at k ½. An explicit `smoothness` 1 paints exactly
//! what no key paints, Diamond ignores it, and a smoothness outside 0..=1 or
//! NaN is refused, as is one below 1 on a two-colour request.
use std::ffi::CString;
use std::ptr;

use doctored_core::doc::DrDocument;
use doctored_core::ffi_artwork::dr_artwork_render;
use doctored_core::ffi_gradient::dr_doc_gradient_stops;
use doctored_core::ffi_precision_edit::dr_precision_doc_edit_plan;
use doctored_core::ffi_selection_u16::dr_selection_u16_plan;
use doctored_core::icc::IccProfile;
use doctored_core::precision_document::PrecisionDocument;
use image::{Rgba, RgbaImage};
use serde_json::{json, Value};

use crate::cmyk_structure_fixture::{cmyk_layers, free as free_doc, RgbLayer};
use crate::common::{layer_pixels, smooth_ramp, take_err_string, BLEND_NORMAL};
use crate::native_scalar::{doc, edit, error, scalar, selection, text, LIMIT};

const W: u32 = 64;
const H: u32 = 2;
const REFUSAL: &str = "gradient smoothness must be finite in [0,1]";

/// Three opaque gray stops on the byte grid (`style_gradient_smoothness_align`'s).
const THREE: [(f64, f64); 3] = [(0.0, 0.0), (0.3, 1.0), (1.0, 64.0 / 255.0)];

/// The drag the fill layer's 0° ramp matches: pixel `x`'s centre reads
/// `t = x / W`, the fill layer's own `t` at that pixel.
const FILL_FROM: [f64; 2] = [0.5, 1.0];
const FILL_TO: [f64; 2] = [W as f64 + 0.5, 1.0];

/// The other paths' drag: edge to edge, so pixel `x` reads `(x + ½) / W`.
const FROM: [f64; 2] = [0.0, 1.0];
const TO: [f64; 2] = [W as f64, 1.0];

fn t_at(x: u32) -> f64 {
    (f64::from(x) + 0.5) / f64::from(W)
}

fn gray_stops_json() -> Value {
    Value::from(
        THREE
            .iter()
            .map(|&(p, v)| json!({"position": p, "color": [v, v, v, 1.0]}))
            .collect::<Vec<_>>(),
    )
}

// ------------------------------------------------------------ fill layer --

/// The Gradient Fill layer's render of `THREE` at Smoothness `k`: red, 16-bit.
fn fill_render(k: f64) -> Vec<u16> {
    let stops: Vec<Value> = THREE
        .iter()
        .map(|&(p, v)| {
            let code = (v * 255.0).round() as u8;
            json!({"position": p, "color": format!("#{code:02x}{code:02x}{code:02x}"),
                "opacity": 1})
        })
        .collect();
    let colors: Vec<[f64; 3]> = THREE.iter().map(|s| [s.1; 3]).collect();
    let component = json!([{"kind": "gradient", "gradient": {"stops": stops, "style": "linear",
        "angle": 0, "scale": 1, "reverse": false, "align_with_layer": true, "smoothness": k},
        "colors": colors}])
    .to_string();
    let components = CString::new(component).unwrap();
    let profile = IccProfile::srgb();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_artwork_render(
            W,
            H,
            components.as_ptr(),
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
    assert!(!out.is_null(), "{}", take_err_string(err));
    let image = *unsafe { Box::from_raw(out) };
    image.to_rgba16().unwrap().iter().map(|p| p[0]).collect()
}

/// The byte tool over `stops` (position, straight RGBA) at Smoothness `k`.
fn byte(
    stops: &[(f64, [f64; 4])],
    kind: u32,
    from: [f64; 2],
    to: [f64; 2],
    k: f32,
) -> Option<Box<DrDocument>> {
    let doc = DrDocument::from_pixels(RgbaImage::from_pixel(W, H, Rgba([0, 0, 0, 255])));
    let flat: Vec<f32> = stops
        .iter()
        .flat_map(|(p, c)| {
            [
                *p as f32,
                c[0] as f32,
                c[1] as f32,
                c[2] as f32,
                c[3] as f32,
                0.5,
                0.5,
            ]
        })
        .collect();
    let out = unsafe {
        dr_doc_gradient_stops(
            &doc,
            0,
            from[0] as f32,
            from[1] as f32,
            to[0] as f32,
            to[1] as f32,
            flat.as_ptr(),
            stops.len(),
            kind,
            BLEND_NORMAL,
            k,
            ptr::null(),
        )
    };
    (!out.is_null()).then(|| unsafe { Box::from_raw(out) })
}

fn three_rgba() -> Vec<(f64, [f64; 4])> {
    THREE.iter().map(|&(p, v)| (p, [v, v, v, 1.0])).collect()
}

fn paint_request(stops: Value, from: [f64; 2], to: [f64; 2], style: &str) -> Value {
    json!({"op":"gradient","layer":0,"start":[0.,0.,0.,1.],"end":[0.25,0.25,0.25,1.],
        "color_space":"document","opacity":1.,"from":from,"to":to,"radial":false,
        "style":style,"stops":stops})
}

fn samples(d: &PrecisionDocument) -> Vec<[u16; 4]> {
    d.layer_source(0).unwrap().to_rgba16().ok().unwrap()
}

/// The tool keeps the UNBOUNDED spline (only a Gradient Fill layer
/// holds the Classic term to its bracketing stops, unmeasured for the tool),
/// so its oracle is `common::smooth_ramp` at the fill layer's `t = x / W`,
/// and at 50 % it parts from the fill layer where the spline overshoots stop
/// 1 (at 100 % the stored clamp at white hides the same overshoot).
#[test]
fn the_tools_ramp_is_the_unbounded_law_at_smoothness_0_50_and_100() {
    let n = (W * H) as usize;
    let d = doc(W, H, vec![[0, 0, 0, 65535]; n]);
    let coverage = vec![65535u16; n];
    let mut renders = Vec::new();
    for k in [0.0, 0.5, 1.0] {
        let fill = fill_render(k);
        let painted = byte(&three_rgba(), 0, FILL_FROM, FILL_TO, k as f32).expect("a gradient");
        let pixels = layer_pixels(&*painted, 0);
        let mut request = paint_request(gray_stops_json(), FILL_FROM, FILL_TO, "linear");
        request["smoothness"] = json!(k);
        let sixteen = samples(&edit(&d, request, &coverage).expect("a 16-bit gradient"));
        let mut apart = 0.0f64;
        for y in 0..H {
            for x in 0..W {
                let at = (y * W + x) as usize;
                let t = f64::from(x) / f64::from(W);
                let want = smooth_ramp(&THREE, t, k).clamp(0.0, 1.0) * 65535.0;
                apart = apart.max((f64::from(sixteen[at][0]) - f64::from(fill[at])).abs());
                for c in 0..3 {
                    let got = f64::from(pixels[at * 4 + c]);
                    assert!(
                        (got - want / 257.0).abs() <= 1.0,
                        "k {k} ({x},{y}) byte channel {c}: {got} against {}",
                        want / 257.0
                    );
                    let got = f64::from(sixteen[at][c]);
                    assert!(
                        (got - want).abs() <= 2.0,
                        "k {k} ({x},{y}) 16-bit channel {c}: {got} against {want}"
                    );
                }
            }
        }
        if k == 0.5 {
            assert!(
                apart >= 10.0,
                "the tool parts from the bracket-held fill by {apart} 16-bit units"
            );
        }
        renders.push(fill);
    }
    assert_ne!(renders[0], renders[2], "Smoothness 0 and 100 % differ");
}

// --------------------------------------------------------- the other paths --

const K: f64 = 0.5;

#[test]
fn the_scalar_and_selection_plane_gradients_take_the_smoothness() {
    let n = (W * H) as usize;
    let d = doc(W, H, vec![[1001, 3003, 5005, 65535]; n]);
    let gray = [
        (0.0, [0.1, 0.1, 0.1, 1.0]),
        (0.3, [0.9, 0.9, 0.9, 0.5]),
        (1.0, [0.4, 0.4, 0.4, 0.8]),
    ];
    let stops: Vec<Value> = gray
        .iter()
        .map(|(p, c)| json!({"position": p, "color": c}))
        .collect();
    let want = |x: u32| {
        let g: Vec<(f64, f64)> = gray.iter().map(|s| (s.0, s.1[0])).collect();
        let a: Vec<(f64, f64)> = gray.iter().map(|s| (s.0, s.1[3])).collect();
        (
            smooth_ramp(&g, t_at(x), K).clamp(0.0, 1.0),
            smooth_ramp(&a, t_at(x), 0.0),
        )
    };
    let operation = json!({"kind":"gradient","from":FROM,"to":TO,"radial":false,
        "start":0,"end":65535,"opacity":1.0,"style":"linear","stops":stops,"smoothness":K});
    let out = scalar(&d, "green", operation, false, &[]).expect("a scalar gradient");
    let pixels = samples(&out);
    for x in 0..W {
        let (g, a) = want(x);
        let w = 3003.0 * (1.0 - a) + g * 65535.0 * a;
        let got = f64::from(pixels[x as usize][1]);
        assert!((got - w).abs() <= 1.5, "scalar at {x}: {got} against {w}");
    }
    let input = vec![4242u16; n];
    let out = selection(
        W,
        H,
        json!({"kind":"gradient","from":FROM,"to":TO,"style":"linear",
            "stops":stops,"opacity":1.0,"smoothness":K}),
        &input,
    );
    for x in 0..W {
        let (g, a) = want(x);
        let w = 4242.0 * (1.0 - a) + g * 65535.0 * a;
        let got = f64::from(out[x as usize]);
        assert!(
            (got - w).abs() <= 1.5,
            "selection at {x}: {got} against {w}"
        );
    }
}

#[test]
fn a_cmyk_gradient_takes_the_smoothness_on_its_inks() {
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
        .map(|(p, ink)| json!({"position": p, "ink": ink}))
        .collect();
    let request = json!({"op":"gradient","layer":0,"opacity":1,"from":FROM,"to":TO,
        "style":"linear","stops":stops,"smoothness":K});
    let stroke = vec![65535u16; (W * H) as usize];
    let out = edit(unsafe { &*doc }, request, &stroke).expect("a CMYK gradient");
    let samples = out.cmyk_layer_samples(0).expect("a CMYK record");
    for x in 0..W {
        let s = samples[x as usize];
        for c in 0..5 {
            let ch: Vec<(f64, f64)> = inks.iter().map(|(p, i)| (*p, i[c])).collect();
            let w = if c == 4 {
                smooth_ramp(&ch, t_at(x), 0.0)
            } else {
                smooth_ramp(&ch, t_at(x), K).clamp(0.0, 1.0)
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
fn a_float_gradient_takes_the_smoothness_unclamped() {
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
    let json_stops: Vec<Value> = stops
        .iter()
        .map(|(p, c)| json!({"position": p, "color": c}))
        .collect();
    let mut request = paint_request(Value::from(json_stops), FROM, TO, "linear");
    request["smoothness"] = json!(K);
    let coverage = vec![65535u16; (W * H) as usize];
    let out = edit(&d, request, &coverage).expect("a float gradient");
    let image = out.layer(0).unwrap().source().unwrap();
    for x in 0..W {
        let got = image.sample_at(x, 0).expect("a sample");
        for (c, &v) in got.iter().take(3).enumerate() {
            let ch: Vec<(f64, f64)> = stops.iter().map(|s| (s.0, s.1[c])).collect();
            let w = smooth_ramp(&ch, t_at(x), K);
            assert!(
                (f64::from(v) - w).abs() <= 1e-4,
                "channel {c} at {x}: {v} against {w}"
            );
        }
    }
}

// ------------------------------------------------- defaults and Diamond --

#[test]
fn an_explicit_full_smoothness_is_no_key_and_diamond_ignores_it() {
    let n = (W * H) as usize;
    let d = doc(W, H, vec![[0, 0, 0, 65535]; n]);
    let coverage = vec![65535u16; n];
    let bare = paint_request(gray_stops_json(), FROM, TO, "linear");
    let mut full = bare.clone();
    full["smoothness"] = json!(1.0);
    assert_eq!(
        samples(&edit(&d, bare, &coverage).unwrap()),
        samples(&edit(&d, full, &coverage).unwrap()),
        "explicit 1 is no key"
    );
    let diamond = |k: f32| {
        layer_pixels(
            &*byte(&three_rgba(), 4, [32.0, 1.0], [64.0, 1.0], k).expect("a gradient"),
            0,
        )
    };
    assert_eq!(diamond(0.3), diamond(1.0), "Diamond keeps the linear ramp");
}

// --------------------------------------------------------------- refusals --

#[test]
fn a_smoothness_outside_0_to_1_is_refused() {
    for bad in [1.5f32, -0.1, f32::NAN] {
        assert!(
            byte(&three_rgba(), 0, FROM, TO, bad).is_none(),
            "byte smoothness {bad}"
        );
    }
    let n = (W * H) as usize;
    let d = doc(W, H, vec![[1, 2, 3, 65535]; n]);
    let refused = |request: Value, sentence: &str| {
        let t = text(request.clone());
        let mut e = ptr::null_mut();
        let p = unsafe { dr_precision_doc_edit_plan(&d, t.as_ptr(), LIMIT, 0, &mut e) };
        assert!(p.is_null(), "{request} was planned");
        let message = unsafe { error(e) };
        assert!(
            message.ends_with(sentence),
            "{request}: {message} is not {sentence}"
        );
    };
    let mut paint = paint_request(gray_stops_json(), FROM, TO, "linear");
    paint["smoothness"] = json!(1.5);
    refused(paint, REFUSAL);
    refused(
        json!({"op":"scalar_edit","target":"red","index":0,"selection":false,
            "operation":{"kind":"gradient","from":FROM,"to":TO,"radial":false,
                "start":0,"end":65535,"style":"linear","stops":gray_stops_json(),
                "smoothness":1.5}}),
        REFUSAL,
    );
    let t = text(
        json!({"kind":"gradient","from":FROM,"to":TO,"stops":gray_stops_json(),
        "smoothness":1.5}),
    );
    let mut e = ptr::null_mut();
    let p = unsafe { dr_selection_u16_plan(W, H, t.as_ptr(), LIMIT, 0, &mut e) };
    assert!(p.is_null(), "a selection gradient was planned");
    assert!(unsafe { error(e) }.ends_with(REFUSAL));
    // Below 1 on a two-colour request: the smoothness needs stops.
    refused(
        json!({"op":"gradient","layer":0,"start":[0.,0.,0.,1.],"end":[1.,1.,1.,1.],
            "color_space":"document","opacity":1.,"from":FROM,"to":TO,"radial":false,
            "smoothness":0.5}),
        "native gradient smoothness requires stops",
    );
    refused(
        json!({"op":"scalar_edit","target":"red","index":0,"selection":false,
            "operation":{"kind":"gradient","from":FROM,"to":TO,"radial":false,
                "start":0,"end":65535,"smoothness":0.5}}),
        "scalar gradient smoothness requires stops",
    );
}
