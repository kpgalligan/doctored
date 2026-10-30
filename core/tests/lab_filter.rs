//! #1 Lab phase 2c, the Filter menu on a Lab Color document
//! (`lab_filter`), through the FFI. The oracle is the same filter on an sRGB
//! document of one Lab channel's codes, `[c, c, c, α]`: every admitted
//! kernel is per-channel, so Lab channel k of the result equals channel 0 of
//! that document's result exactly, at 8 and 16 bits, everywhere and under a
//! half-canvas coverage. Add Noise equals the sRGB document of the same
//! codes exactly (the same draws per channel). Clouds of red and blue equals
//! the sRGB-codes document's Clouds of the hexes of their Lab codes computed
//! here from the CIE formulas (`lab_fixture::srgb_to_lab`). Every refusal is
//! exact at the edit and its plan (the five luma filters and Offset's fill
//! edge run since phase 2f: `lab_channel_filter.rs`). Fade 0.5 Normal after a blur equals the
//! sRGB-codes document's Fade exactly and the code midpoint within 1 where
//! both states are opaque; Fade Darken refuses and Fade Hue runs (phase 3;
//! its arm is `lab_blend.rs`'s). Bounds declared before measuring.

use std::ffi::CString;
use std::ptr;

use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_fade::dr_precision_doc_fade;
use doctored_core::ffi_lab::dr_precision_doc_convert_to_lab;
use doctored_core::ffi_selected_adjustment::dr_precision_doc_selected_edit_plan;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer};
use doctored_core::precision_image::PrecisionImage;
use serde_json::{json, Value};

use crate::common::take_err_string;
use crate::gray_ops::{free, result, selected, LIMIT};
use crate::lab_fixture::srgb_to_lab;

const W: u32 = 24;
const H: u32 = 16;

/// An sRGB document of one layer holding exactly `pixels` (W × H).
fn srgb_doc(pixels: Vec<[u16; 4]>) -> *mut PrecisionDocument {
    let image = PrecisionImage::from_rgba16(W, H, pixels, IccProfile::srgb(), None).unwrap();
    let layer = PrecisionDocumentLayer::raster("Layer", image, (0, 0), 1.0, true).unwrap();
    let doc = PrecisionDocument::from_parts(
        W,
        H,
        IccProfile::srgb(),
        Metadata::default(),
        Resolution::default(),
        vec![layer],
    )
    .unwrap();
    Box::into_raw(Box::new(doc))
}

/// A pattern whose L\*, a\* and b\* all vary, opaque but for a band of
/// partial alpha on the right, through Mode ▸ Lab Color at `depth`.
fn lab_doc(depth: u32) -> *mut PrecisionDocument {
    let pixels = (0..W * H)
        .map(|i| {
            let (x, y) = (i % W, i / W);
            let r = (x * 65535 / (W - 1)) as u16;
            let g = ((y * 4096 + x * 977) % 65536) as u16;
            let b = (((W - 1 - x) * 2731 + y * 3001) % 65536) as u16;
            let a = if x >= W - 5 {
                (20000 + y * 2500) as u16
            } else {
                65535
            };
            [r, g, b, a]
        })
        .collect();
    let rgb = srgb_doc(pixels);
    let options = CString::new(format!(r#"{{"depth":{depth}}}"#)).unwrap();
    let mut err = ptr::null_mut();
    let lab = unsafe {
        dr_precision_doc_convert_to_lab(rgb, options.as_ptr(), LIMIT, 0, ptr::null(), &mut err)
    };
    assert!(!lab.is_null(), "to Lab: {}", take_err_string(err));
    free(rgb);
    lab
}

fn codes(doc: *const PrecisionDocument) -> Vec<[u16; 4]> {
    unsafe { &*doc }
        .layer_source(0)
        .unwrap()
        .to_rgba16()
        .unwrap()
}

fn run(
    doc: *const PrecisionDocument,
    op: &str,
    params: &Value,
    coverage: &[u16],
) -> Result<*mut PrecisionDocument, String> {
    let request = json!({"layer":0,"op":op,"params":params});
    selected(doc, &request, coverage).map(|d| d.expect("the filter changed nothing"))
}

fn plan(doc: *const PrecisionDocument, op: &str, params: &Value) -> Result<(), String> {
    let text = CString::new(json!({"layer":0,"op":op,"params":params}).to_string()).unwrap();
    let mut err = ptr::null_mut();
    let out =
        unsafe { dr_precision_doc_selected_edit_plan(doc, text.as_ptr(), LIMIT, 0, &mut err) };
    if out.is_null() {
        Err(take_err_string(err))
    } else {
        unsafe { dr_string_free(out) };
        Ok(())
    }
}

/// The per-channel filters, with parameters that change the pattern.
fn filters() -> Vec<(&'static str, Value)> {
    vec![
        ("blur", json!({"sigma":2})),
        ("gaussian_blur", json!({"radius":3})),
        ("box_blur", json!({"radius":2})),
        ("motion_blur", json!({"angle":30,"distance":5})),
        (
            "radial_blur",
            json!({"amount":10,"method":"spin","quality":"draft"}),
        ),
        ("tilt_shift", json!({})),
        ("surface_blur", json!({"radius":3,"threshold":15})),
        ("dust_and_scratches", json!({"radius":2,"threshold":0})),
        (
            "unsharp_mask",
            json!({"radius":1.5,"amount":1.2,"threshold":0.0157}),
        ),
        ("high_pass", json!({"radius":3})),
        ("sharpen", json!({})),
        ("pixelate", json!({"block":3})),
        (
            "offset",
            json!({"horizontal":3,"vertical":-2,"undefined_areas":"wrap"}),
        ),
    ]
}

/// The left half uncovered, the right half a ramp to full.
fn half_coverage() -> Vec<u16> {
    (0..W * H)
        .map(|i| {
            let x = i % W;
            if x < W / 2 {
                0
            } else {
                ((x - W / 2 + 1) * 65535 / (W / 2)) as u16
            }
        })
        .collect()
}

#[test]
fn every_channel_is_the_filters_one_channel_result() {
    for depth in [8, 16] {
        let lab = lab_doc(depth);
        let source = codes(lab);
        for coverage in [Vec::new(), half_coverage()] {
            for (op, params) in filters() {
                let what = format!("{op} at {depth}, coverage {}", !coverage.is_empty());
                let out =
                    run(lab, op, &params, &coverage).unwrap_or_else(|e| panic!("{what}: {e}"));
                let got = codes(out);
                assert_ne!(got, source, "{what} changed nothing");
                for k in 0..3 {
                    let one: Vec<[u16; 4]> =
                        source.iter().map(|p| [p[k], p[k], p[k], p[3]]).collect();
                    let rgb = srgb_doc(one);
                    let ran = run(rgb, op, &params, &coverage).unwrap();
                    let want = codes(ran);
                    free(ran);
                    for (i, (g, w)) in got.iter().zip(&want).enumerate() {
                        assert_eq!(g[k], w[0], "{what} pixel {i} channel {k}");
                        assert_eq!(g[3], w[3], "{what} pixel {i} alpha");
                    }
                    free(rgb);
                }
                free(out);
            }
        }
        free(lab);
    }
}

#[test]
fn add_noise_draws_each_channel_as_rgb_does() {
    for depth in [8, 16] {
        let lab = lab_doc(depth);
        let params = json!({"amount":0.2,"seed":7});
        let out = run(lab, "add_noise", &params, &[]).unwrap();
        let rgb = srgb_doc(codes(lab));
        let want = run(rgb, "add_noise", &params, &[]).unwrap();
        assert_eq!(codes(out), codes(want), "add_noise at {depth}");
        assert_ne!(codes(out), codes(lab));
        for doc in [want, rgb, out, lab] {
            free(doc);
        }
    }
}

/// The 8-bit `#LLaabb` of an sRGB colour's Lab codes, from the CIE formulas.
fn lab_hex(rgb: [f64; 3], depth: u32) -> String {
    let lab = srgb_to_lab(rgb);
    let unit = [
        lab[0] / 100.0,
        (lab[1] + 128.0) / 255.0,
        (lab[2] + 128.0) / 255.0,
    ];
    let bytes = unit.map(|v| {
        let wide = (v.clamp(0.0, 1.0) * 65535.0).round() as u32;
        if depth == 8 {
            ((wide + 128) / 257) as u8
        } else {
            (f64::from(wide) / 257.0).round() as u8
        }
    });
    format!("#{:02x}{:02x}{:02x}", bytes[0], bytes[1], bytes[2])
}

#[test]
fn clouds_paint_the_lab_codes_of_their_colours() {
    for depth in [8, 16] {
        let lab = lab_doc(depth);
        let out = run(
            lab,
            "clouds",
            &json!({"seed":3,"foreground_color":"#ff0000","background_color":"#0000ff"}),
            &[],
        )
        .unwrap();
        let rgb = srgb_doc(codes(lab));
        let want = run(
            rgb,
            "clouds",
            &json!({"seed":3,"foreground_color":lab_hex([1.0, 0.0, 0.0], depth),
                "background_color":lab_hex([0.0, 0.0, 1.0], depth)}),
            &[],
        )
        .unwrap();
        assert_eq!(codes(out), codes(want), "clouds at {depth}");
        // The defaults are black and white's Lab codes.
        let plain = run(lab, "clouds", &json!({"seed":3}), &[]).unwrap();
        let plain_want = run(
            rgb,
            "clouds",
            &json!({"seed":3,"foreground_color":lab_hex([0.0; 3], depth),
                "background_color":lab_hex([1.0; 3], depth)}),
            &[],
        )
        .unwrap();
        assert_eq!(codes(plain), codes(plain_want), "default clouds at {depth}");
        for doc in [plain_want, plain, want, rgb, out, lab] {
            free(doc);
        }
    }
}

#[test]
fn refused_filters_name_their_reason() {
    let lab = lab_doc(16);
    let unavailable = |name: &str| format!("{name} is unavailable in a Lab Color document.");
    let raw = |name: &str| {
        format!(
            "{name} is unavailable in a Lab Color document: it is a Develop Filter control, and the Develop Filter is unavailable in Lab."
        )
    };
    let cases = [
        ("lens_correction", json!({"distortion":20}), unavailable("Lens Correction")),
        ("develop_filter", json!({}), unavailable("Develop Filter")),
        ("difference_clouds", json!({"seed":5}), unavailable("Difference Clouds")),
        ("presence", json!({"clarity":40}), raw("Presence")),
        ("add_grain", json!({"amount":40,"seed":3}), raw("Add Grain")),
        (
            "depth_blur",
            json!({"blur_strength":60,"depth":{"width":W,"height":H,
                "samples":(0..W * H).map(|i| f64::from(i % W) / f64::from(W - 1)).collect::<Vec<f64>>(),"frame":[0, 0, W, H]}}),
            "Depth Blur is unavailable in a Lab Color document: Doctored offers it on RGB documents only."
                .to_string(),
        ),
    ];
    for (op, params, sentence) in &cases {
        assert_eq!(&run(lab, op, params, &[]).unwrap_err(), sentence, "{op}");
        assert_eq!(&plan(lab, op, params).unwrap_err(), sentence, "{op} plan");
    }
    free(lab);
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
fn fade_after_a_blur_lerps_the_codes_and_refuses_lab_modes() {
    let lab = lab_doc(16);
    let rgb = srgb_doc(codes(lab));
    let blur = json!({"sigma":2});
    let a = run(lab, "blur", &blur, &[]).unwrap();
    let b = run(rgb, "blur", &blur, &[]).unwrap();
    let faded = fade(a, lab, 0, 0.5).unwrap().unwrap();
    let want = fade(b, rgb, 0, 0.5).unwrap().unwrap();
    assert_eq!(codes(faded), codes(want));
    let (before, after) = (codes(lab), codes(a));
    let mut opaque = 0;
    for (i, p) in codes(faded).iter().enumerate() {
        if before[i][3] != 65535 || after[i][3] != 65535 {
            continue;
        }
        opaque += 1;
        for c in 0..3 {
            let mid = (f64::from(before[i][c]) + f64::from(after[i][c])) / 2.0;
            assert!(
                (f64::from(p[c]) - mid).abs() <= 1.0,
                "pixel {i} channel {c}: {} vs {mid}",
                p[c]
            );
        }
    }
    assert!(opaque > 100, "{opaque} opaque pixels");
    assert_eq!(
        fade(a, lab, 6, 0.5).unwrap_err(),
        "The Darken blend mode is unavailable in a Lab Color document."
    );
    let hue = fade(a, lab, 23, 0.5)
        .unwrap()
        .expect("Fade Hue changed nothing");
    assert!(unsafe { &*hue }.is_lab());
    for doc in [hue, want, faded, b, a, rgb, lab] {
        free(doc);
    }
}
