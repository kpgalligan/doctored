//! #1 phase 2c, the Filter menu on a Grayscale document (`gray_filter`),
//! through the FFI. The oracle is the RGB TWIN (the Gray document converted
//! to RGB through its own profile bytes) running the same filter: every gray
//! result is R = G = B; where the twin's pixel is R = G = B the gray pixel
//! equals it (exactly at 8 and 16 bits, bit for bit at 32); where a kernel
//! made colour (Lens Correction's fringes, the Develop Filter's
//! Temperature, Depth Blur's Warmth) the gray is the twin's Rec. 709 luma,
//! computed here in f64 from the standard's weights, within 1 code at 8 and
//! 16 bits and 1e-6 relative at 32; Add Noise equals the twin's channel 0
//! (its one full-amplitude draw), and at 16 bits on a uniform document the
//! standard deviation of its change is within 10 % of the uniform draw's
//! `amount/√3`. Blur under a half-canvas coverage equals the twin. Clouds of
//! red and blue equals the twin's Clouds of the greys computed here (sRGB's
//! published Y row over the IEC decode, then `Y^(1/2.2)`, 8-bit hex).
//! Liquify (phase 4) equals the twin's. Fade 0.5 Normal after a blur equals the
//! twin's Fade; Fade Hue refuses with the Gray blend sentence, and Color Burn
//! at 32 bits with the float sentence. Bounds declared before measuring.

use std::ptr;

use doctored_core::ffi_fade::dr_precision_doc_fade;
use doctored_core::precision_document::PrecisionDocument;
use serde_json::{json, Value};

use crate::gray_fixture::{srgb_decode, SRGB_Y_ROW};
use crate::gray_ops::{
    assert_neutral, free, gray_doc, layer, result, selected, twin, uniform_gray, values, LIMIT,
    LUMA,
};

const W: u32 = 24;
const H: u32 = 16;

/// A depth map over the whole plane: near on the left, far on the right.
fn depth_map() -> Value {
    let samples: Vec<f64> = (0..W * H)
        .map(|i| f64::from(W - 1 - i % W) / f64::from(W - 1))
        .collect();
    json!({"width": W, "height": H, "samples": samples, "frame": [0, 0, W, H]})
}

/// Every selected filter but `liquify` and `levels_channels`, with
/// parameters that change the pattern.
fn filters() -> Vec<(&'static str, Value)> {
    vec![
        ("blur", json!({"sigma":2})),
        ("gaussian_blur", json!({"radius":3})),
        ("box_blur", json!({"radius":2})),
        ("surface_blur", json!({"radius":3,"threshold":15})),
        ("motion_blur", json!({"angle":30,"distance":5})),
        (
            "radial_blur",
            json!({"amount":10,"method":"spin","quality":"draft"}),
        ),
        (
            "unsharp_mask",
            json!({"radius":1.5,"amount":1.2,"threshold":0.0157}),
        ),
        ("sharpen", json!({})),
        ("smart_sharpen", json!({})),
        ("high_pass", json!({"radius":3})),
        ("dust_and_scratches", json!({"radius":2,"threshold":0})),
        ("reduce_noise", json!({})),
        ("pixelate", json!({"block":3})),
        ("emboss", json!({})),
        ("edge_detect", json!({})),
        (
            "offset",
            json!({"horizontal":3,"vertical":-2,"undefined_areas":"wrap"}),
        ),
        ("lens_blur", json!({"radius":3})),
        ("tilt_shift", json!({})),
        ("add_noise", json!({"amount":0.2,"seed":7})),
        ("add_grain", json!({"amount":40,"seed":3})),
        ("clouds", json!({"seed":3})),
        ("difference_clouds", json!({"seed":5})),
        (
            "lens_correction",
            json!({"chromatic_red_cyan":50,"distortion":20}),
        ),
        ("presence", json!({"texture":40,"clarity":30})),
        ("develop_filter", json!({"temperature":40,"clarity":30})),
        (
            "depth_blur",
            json!({"blur_strength":60,"warmth":30,"depth":depth_map()}),
        ),
    ]
}

fn run(
    doc: *const PrecisionDocument,
    op: &str,
    params: &Value,
) -> Result<*mut PrecisionDocument, String> {
    let request = json!({"layer":0,"op":op,"params":params});
    selected(doc, &request, &[]).map(|d| d.unwrap_or_else(|| panic!("{op} changed nothing")))
}

fn luma(p: [f64; 4]) -> f64 {
    LUMA[0] * p[0] + LUMA[1] * p[1] + LUMA[2] * p[2]
}

/// The gray result `a` against the twin's `b` (module doc).
fn assert_folded(a: *const PrecisionDocument, b: *const PrecisionDocument, op: &str, depth: u32) {
    let what = format!("{op} at {depth}");
    let (ga, gb) = (layer(a, 0, depth), layer(b, 0, depth));
    let (va, vb) = (values(a, 0, depth), values(b, 0, depth));
    assert_neutral(&ga, &what);
    let mut coloured = 0;
    for i in 0..ga.len() {
        assert_eq!(ga[i][3], gb[i][3], "{what} pixel {i}: alpha");
        if op == "add_noise" {
            assert_eq!(ga[i][0], gb[i][0], "{what} pixel {i}: channel 0's draw");
        } else if gb[i][0] == gb[i][1] && gb[i][1] == gb[i][2] {
            assert_eq!(ga[i], gb[i], "{what} pixel {i}");
        } else {
            coloured += 1;
            let want = luma(vb[i]);
            let got = va[i][0];
            if depth == 32 {
                assert!(
                    (got - want).abs() <= 1e-6 * want.abs().max(1e-6),
                    "{what} pixel {i}: {got} against the luma {want}"
                );
            } else {
                assert!(
                    (got - want).abs() <= 1.0,
                    "{what} pixel {i}: {got} against the luma {want}"
                );
            }
        }
    }
    if matches!(op, "lens_correction" | "develop_filter" | "depth_blur") {
        assert!(coloured > 0, "{what}: the twin made no colour to fold");
    }
}

#[test]
fn every_filter_is_the_twins_result_folded_to_one_channel() {
    for depth in [8, 16, 32] {
        let gray = gray_doc(W, H, depth);
        let rgb = twin(gray);
        assert_eq!(
            layer(rgb, 0, depth),
            layer(gray, 0, depth),
            "the twin keeps every sample"
        );
        for (op, params) in filters() {
            let what = format!("{op} at {depth}");
            // An op the float document refuses refuses alike on the Gray one.
            let b = match run(rgb, op, &params) {
                Ok(b) => b,
                Err(e) if depth == 32 => {
                    assert_eq!(run(gray, op, &params).unwrap_err(), e, "{what}");
                    continue;
                }
                Err(e) => panic!("{what} twin: {e}"),
            };
            let a = run(gray, op, &params).unwrap_or_else(|e| panic!("{what}: {e}"));
            assert_folded(a, b, op, depth);
            assert!(
                unsafe { &*a }.is_gray(),
                "{what}: the result is a Grayscale document"
            );
            free(a);
            free(b);
        }
        free(rgb);
        free(gray);
    }
}

#[test]
fn add_noise_has_the_amounts_full_amplitude() {
    let (w, h) = (64, 64);
    let gray = uniform_gray(w, h, 16, 0.5);
    let amount = 0.2;
    let out = run(gray, "add_noise", &json!({"amount":amount,"seed":11})).unwrap();
    let before = values(gray, 0, 16);
    let after = values(out, 0, 16);
    let deltas: Vec<f64> = before
        .iter()
        .zip(&after)
        .map(|(b, a)| (a[0] - b[0]) / 65535.0)
        .collect();
    let n = deltas.len() as f64;
    let mean = deltas.iter().sum::<f64>() / n;
    let std = (deltas.iter().map(|d| (d - mean).powi(2)).sum::<f64>() / n).sqrt();
    let want = amount / 3f64.sqrt();
    assert!(
        (std - want).abs() <= 0.1 * want,
        "std {std} against the uniform draw's {want}"
    );
    free(out);
    free(gray);
}

#[test]
fn blur_under_a_half_canvas_coverage_equals_the_twin() {
    let gray = gray_doc(W, H, 16);
    let rgb = twin(gray);
    let coverage: Vec<u16> = (0..W * H)
        .map(|i| {
            if i % W < W / 2 {
                65535
            } else {
                (i * 997 % 65536) as u16 / 4
            }
        })
        .collect();
    let request = json!({"layer":0,"op":"blur","params":{"sigma":2}});
    let a = selected(gray, &request, &coverage).unwrap().unwrap();
    let b = selected(rgb, &request, &coverage).unwrap().unwrap();
    assert_neutral(&layer(a, 0, 16), "covered blur");
    assert_eq!(layer(a, 0, 16), layer(b, 0, 16));
    for doc in [a, b, rgb, gray] {
        free(doc);
    }
}

/// The 8-bit hex of `rgb`'s gray in Gray Gamma 2.2, written from the
/// standards: sRGB's Y row over the IEC decode, then `Y^(1/2.2)`.
fn grey_hex(rgb: [f64; 3]) -> String {
    let y: f64 = (0..3).map(|c| SRGB_Y_ROW[c] * srgb_decode(rgb[c])).sum();
    let g = (y.powf(1.0 / 2.2) * 255.0).round() as u8;
    format!("#{g:02x}{g:02x}{g:02x}")
}

#[test]
fn clouds_of_red_and_blue_are_the_twins_clouds_of_their_greys() {
    let red = grey_hex([1.0, 0.0, 0.0]);
    let blue = grey_hex([0.0, 0.0, 1.0]);
    for depth in [8, 16, 32] {
        let gray = gray_doc(W, H, depth);
        let rgb = twin(gray);
        for op in ["clouds", "difference_clouds"] {
            let what = format!("{op} at {depth}");
            let a = run(
                gray,
                op,
                &json!({"seed":4,"foreground_color":"#ff0000","background_color":"#0000ff"}),
            )
            .unwrap_or_else(|e| panic!("{what}: {e}"));
            let b = run(
                rgb,
                op,
                &json!({"seed":4,"foreground_color":red,"background_color":blue}),
            )
            .unwrap_or_else(|e| panic!("{what} twin: {e}"));
            assert_neutral(&layer(a, 0, depth), &what);
            assert_eq!(layer(a, 0, depth), layer(b, 0, depth), "{what}");
            free(a);
            free(b);
        }
        free(rgb);
        free(gray);
    }
}

#[test]
fn liquify_is_admitted_and_equals_the_twin() {
    let gray = gray_doc(W, H, 16);
    let rgb = twin(gray);
    let strokes = json!({"strokes":[
        {"tool":"forward_warp","size":8,"strength":0.8,"points":[[4.0,4.0],[9.0,6.0]]}]});
    let a = run(gray, "liquify", &strokes).unwrap();
    let b = run(rgb, "liquify", &strokes).unwrap();
    assert_neutral(&layer(a, 0, 16), "liquify");
    assert_eq!(layer(a, 0, 16), layer(b, 0, 16), "liquify");
    free(a);
    free(b);
    free(rgb);
    free(gray);
}

fn fade(
    after: *const PrecisionDocument,
    before: *const PrecisionDocument,
    mode: i32,
    opacity: f64,
) -> Result<Option<*mut PrecisionDocument>, String> {
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_fade(
            after,
            0,
            before,
            mode,
            opacity,
            ptr::null(),
            0,
            false,
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    result(out, err)
}

#[test]
fn fade_after_a_blur_equals_the_twins_and_refuses_what_gray_lacks() {
    for depth in [16, 32] {
        let gray = gray_doc(W, H, depth);
        let rgb = twin(gray);
        let blur = json!({"sigma":2});
        let a = run(gray, "blur", &blur).unwrap();
        let b = run(rgb, "blur", &blur).unwrap();
        let fa = fade(a, gray, 0, 0.5).unwrap().unwrap();
        let fb = fade(b, rgb, 0, 0.5).unwrap().unwrap();
        assert_neutral(&layer(fa, 0, depth), "fade");
        assert_eq!(layer(fa, 0, depth), layer(fb, 0, depth), "fade at {depth}");
        assert_eq!(
            fade(a, gray, 23, 0.5).unwrap_err(),
            "The Hue blend mode is unavailable in a Grayscale document."
        );
        if depth == 32 {
            assert_eq!(
                fade(a, gray, 11, 0.5).unwrap_err(),
                "the color_burn blend mode is unavailable in a 32-bit float document"
            );
        } else {
            let burned = fade(a, gray, 11, 0.5).unwrap().unwrap();
            assert_neutral(&layer(burned, 0, depth), "color burn fade");
            free(burned);
        }
        for doc in [fa, fb, a, b, rgb, gray] {
            free(doc);
        }
    }
    // An RGB document keeps every mode.
    let gray = gray_doc(W, H, 16);
    let rgb = twin(gray);
    let b = run(rgb, "blur", &json!({"sigma":2})).unwrap();
    let hue = fade(b, rgb, 23, 0.5).unwrap_or_else(|e| panic!("{}", e));
    if let Some(hue) = hue {
        free(hue);
    }
    for doc in [b, rgb, gray] {
        free(doc);
    }
}
