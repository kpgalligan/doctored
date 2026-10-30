//! Independent native channel blend/ladder/mask-apply/weighted-Auto references.
use crate::native_scalar as native;
use doctored_core::ffi_precision_edit::*;
use doctored_core::ffi_selection_u16::*;
use doctored_core::icc::IccProfile;
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer};
use doctored_core::precision_image::PrecisionImage;
use native::*;
use serde_json::{json, Value};
use std::ffi::CStr;
use std::ptr;
fn math(w: u32, h: u32, operation: Value, input: &[u16], channels: usize) -> Vec<u16> {
    let t = text(operation);
    let mut err = ptr::null_mut();
    let p = unsafe { dr_selection_u16_plan(w, h, t.as_ptr(), LIMIT, 0, &mut err) };
    assert!(!p.is_null(), "{}", unsafe { error(err) });
    let plan: Value = serde_json::from_slice(unsafe { CStr::from_ptr(p) }.to_bytes()).unwrap();
    unsafe { doctored_core::ffi::dr_string_free(p) };
    assert_eq!(
        plan["output_samples"],
        (w as usize * h as usize * channels) as u64
    );
    let mut out = vec![12345; w as usize * h as usize * channels];
    assert!(
        unsafe {
            dr_selection_u16(
                w,
                h,
                t.as_ptr(),
                input.as_ptr(),
                input.len(),
                out.as_mut_ptr(),
                out.len(),
                LIMIT,
                0,
                ptr::null(),
                &mut err,
            )
        },
        "{}",
        unsafe { error(err) }
    );
    out
}
#[test]
fn scalar_and_rgb_blends_use_independent_positive_sum_references() {
    let base = [1, 10001, 32769, 65534];
    let source = [65534, 37003, 11001, 1];
    let input = [base.as_slice(), source.as_slice()].concat();
    for (mode, formula) in [(0, 0), (1, 1), (2, 2), (8, 8)] {
        for invert in [false, true] {
            let out = math(
                4,
                1,
                json!({"kind":"blend","channels":1,"mode":mode,"opacity":0.371,"invert_base":invert,"invert_source":!invert}),
                &input,
                1,
            );
            for i in 0..4 {
                let b = f64::from(if invert { 65535 - base[i] } else { base[i] }) / 65535.;
                let s = f64::from(if !invert {
                    65535 - source[i]
                } else {
                    source[i]
                }) / 65535.;
                let mixed = match formula {
                    0 => s,
                    1 => b * s,
                    2 => b + s - b * s,
                    _ => (b - s).abs(),
                };
                assert!(out[i].abs_diff(q((b * (1. - 0.371) + mixed * 0.371) * 65535.)) <= 1);
            }
        }
    }
    let rgb = [
        1, 10001, 7, 31001, 17, 61001, 65534, 37003, 30003, 50001, 9001, 2,
    ];
    let out = math(
        2,
        1,
        json!({"kind":"blend","channels":3,"mode":1,"opacity":0.731}),
        &rgb,
        3,
    );
    for i in 0..6 {
        let b = f64::from(rgb[i]) / 65535.;
        let s = f64::from(rgb[i + 6]) / 65535.;
        assert!(out[i].abs_diff(q((b * (1. - 0.731) + b * s * 0.731) * 65535.)) <= 1);
    }
}
#[test]
fn all_rgb_modes_admit_scalar_hsl_refuses_and_zero_opacity_copies_exactly() {
    let input = [
        1, 65534, 32769, 40001, 31001, 17003, 55555, 10001, 23457, 21001, 12345, 2,
    ];
    for mode in 0..=26 {
        let out = math(
            2,
            1,
            json!({"kind":"blend","channels":3,"mode":mode,"opacity":0}),
            &input,
            3,
        );
        assert_eq!(out, input[..6]);
        let _ = math(
            2,
            1,
            json!({"kind":"blend","channels":3,"mode":mode,"opacity":0.731}),
            &input,
            3,
        );
    }
    for mode in 23..=27 {
        let request = text(json!({"kind":"blend","channels":1,"mode":mode,"opacity":1}));
        let mut out = [999; 2];
        assert!(!unsafe {
            dr_selection_u16(
                2,
                1,
                request.as_ptr(),
                input.as_ptr(),
                4,
                out.as_mut_ptr(),
                2,
                LIMIT,
                0,
                ptr::null(),
                ptr::null_mut(),
            )
        });
        assert_eq!(out, [999; 2]);
    }
}
#[test]
fn nine_native_luminosity_masks_follow_independent_power_ladder() {
    let values = [0, 1, 257, 12345, 32769, 55555, 65534, 65535];
    for band in ["lights", "darks", "midtones"] {
        for level in 1..=3 {
            let out = math(
                8,
                1,
                json!({"kind":"luminosity","band":band,"level":level}),
                &values,
                1,
            );
            for (i, &v) in values.iter().enumerate() {
                let t = f64::from(v) / 65535.;
                let e = match band {
                    "lights" => t.powi(level),
                    "darks" => (1. - t).powi(level),
                    _ => 1. - t.powi(level + 1) - (1. - t).powi(level + 1),
                };
                assert!(out[i].abs_diff(q(e * 65535.)) <= 1);
            }
        }
    }
}
#[test]
fn applying_disabled_mask_uses_effective_density_and_keeps_latent_color() {
    let source = vec![
        [101, 202, 303, 65535],
        [111, 222, 333, 1],
        [121, 242, 363, 32769],
        [131, 262, 393, 0],
    ];
    let d = doc(4, 1, source.clone());
    let d=edit(&d,json!({"op":"set_mask","layer":0,"width":2,"height":1,"x":1,"enabled":false,"density":0.625}),&[1,32769]).unwrap();
    let out = edit(&d, json!({"op":"apply_mask","layer":0}), &[]).unwrap();
    assert!(out.layer(0).unwrap().mask().is_none());
    let pixels = out.layer_source(0).unwrap().to_rgba16().ok().unwrap();
    for i in 0..4 {
        let raw = f64::from([0, 1, 32769, 0][i]) / 65535.;
        let expected = q(f64::from(source[i][3]) * (1. - 0.625 * (1. - raw)));
        assert_eq!(pixels[i][..3], source[i][..3]);
        assert!(pixels[i][3].abs_diff(expected) <= 1);
    }
    assert!(d.layer(0).unwrap().mask().is_some());
    assert_eq!(d.layer_source(0).unwrap().to_rgba16().ok().unwrap(), source);
}
#[test]
fn replace_canvas_maps_only_canvas_covered_raw_storage_and_its_selection() {
    let source = PrecisionImage::from_rgba16(
        4,
        1,
        vec![
            [1, 2, 3, 0],
            [101, 102, 103, 1],
            [201, 202, 203, 32769],
            [301, 302, 303, 65535],
        ],
        IccProfile::srgb(),
        None,
    )
    .unwrap();
    let layer = PrecisionDocumentLayer::raster("offset", source, (-1, 0), 0.3, true).unwrap();
    let d = PrecisionDocument::from_parts(
        2,
        1,
        IccProfile::srgb(),
        Default::default(),
        Default::default(),
        vec![layer],
    )
    .unwrap();
    let out = scalar(
        &d,
        "red",
        json!({"kind":"replace_canvas"}),
        true,
        &[12345, 54321, 65535, 0],
    )
    .unwrap();
    assert_eq!(
        out.layer_source(0).unwrap().to_rgba16().ok().unwrap(),
        [
            [1, 2, 3, 0],
            [12345, 102, 103, 1],
            [201, 202, 203, 32769],
            [301, 302, 303, 65535]
        ]
    );
}
#[test]
fn scalar_auto_selection_endpoints_are_independent_from_write_coverage() {
    let d = doc(
        5,
        1,
        vec![
            [0, 21, 31, 0],
            [10001, 22, 32, 1],
            [20001, 23, 33, 0],
            [30001, 24, 34, 12345],
            [65535, 25, 35, 65535],
        ],
    );
    let coverage = [0, 1, 32769, 65535, 0];
    for op in ["auto_tone", "auto_contrast", "auto_color"] {
        let command = |scope| json!({"op":"scalar_edit","target":"red","index":0,"selection":true,"operation":{"kind":"filter","edit":{"layer":0,"op":op,"params":{"clip":0},"analysis_scope":scope}}});
        let derive = |scope| {
            let req = text(command(scope));
            let mut e = ptr::null_mut();
            let p = unsafe {
                dr_precision_doc_scalar_auto_levels(
                    &d,
                    req.as_ptr(),
                    coverage.as_ptr(),
                    5,
                    LIMIT,
                    0,
                    ptr::null(),
                    &mut e,
                )
            };
            assert!(!p.is_null(), "{}", unsafe { error(e) });
            let v: Value = serde_json::from_slice(unsafe { CStr::from_ptr(p) }.to_bytes()).unwrap();
            unsafe { doctored_core::ffi::dr_string_free(p) };
            v
        };
        let whole = derive("whole_target");
        let selected = derive("selection");
        assert_eq!(whole["black"][0], 0.);
        assert_eq!(whole["white"][0], 1.);
        assert!((selected["black"][0].as_f64().unwrap() - 10001. / 65535.).abs() < 1e-6);
        assert!((selected["white"][0].as_f64().unwrap() - 30001. / 65535.).abs() < 1e-6);
        let out = edit(&d, command("selection"), &coverage).unwrap();
        let original = d.layer_source(0).unwrap().to_rgba16().ok().unwrap();
        let actual = out.layer_source(0).unwrap().to_rgba16().ok().unwrap();
        assert_eq!(actual[0], original[0]);
        assert_eq!(actual[4], original[4]);
        for i in 0..5 {
            assert_eq!(actual[i][1..], original[i][1..]);
        }
        let levels = json!({"kind":"filter","edit":{"layer":0,"op":"levels_channels","params":{"black":selected["black"],"white":selected["white"],"gamma":selected["gamma"]}}});
        let applied = scalar(&d, "red", levels, true, &coverage).unwrap();
        assert_eq!(
            applied.layer_source(0).unwrap().to_rgba16().ok().unwrap(),
            actual
        );
    }
    assert!(unsafe {
        dr_precision_doc_scalar_auto_levels(
            ptr::null(),
            ptr::null(),
            ptr::null(),
            0,
            LIMIT,
            0,
            ptr::null(),
            ptr::null_mut(),
        )
    }
    .is_null());
}

use crate::native_blend_oracle as blend_oracle;
#[test]
fn native_hsl_planes_match_independent_w3c_color_equations() {
    let pairs = [
        ([1, 31001, 65003], [55007, 21003, 7]),
        ([65534, 27001, 12345], [31001, 32769, 19003]),
        ([10001; 3], [10003, 40003, 20001]),
    ];
    for (base, source) in pairs {
        for mode in 23..=26 {
            for opacity in [0.371, 1.] {
                let input = [base.as_slice(), source.as_slice()].concat();
                let out = math(
                    1,
                    1,
                    json!({"kind":"blend","channels":3,"mode":mode,"opacity":opacity}),
                    &input,
                    3,
                );
                let b = base.map(|v| f64::from(v) / 65535.);
                let s = source.map(|v| f64::from(v) / 65535.);
                let result = blend_oracle::hsl(mode, b, s);
                for c in 0..3 {
                    let expected = q((b[c] * (1. - opacity) + result[c] * opacity) * 65535.);
                    assert!(
                        out[c].abs_diff(expected) <= 1,
                        "mode{mode} opacity{opacity} channel{c}: {} !={expected}",
                        out[c]
                    );
                }
            }
        }
    }
}

#[test]
fn compound_channel_capacity_preflights_without_mutation_and_is_null_safe() {
    unsafe {
        let d = doc(3, 1, vec![[1, 2, 3, 4]; 3]);
        let mut e = ptr::null_mut();
        assert!(dr_precision_doc_channel_capacity(&d, 9, &mut e));
        assert!(e.is_null());
        assert!(d.channels().is_empty());
        assert!(!dr_precision_doc_channel_capacity(&d, 257, &mut e));
        assert!(error(e).contains("limit"));
        assert!(!dr_precision_doc_channel_capacity(
            &d,
            usize::MAX,
            ptr::null_mut()
        ));
        assert!(!dr_precision_doc_channel_capacity(
            ptr::null(),
            9,
            ptr::null_mut()
        ));
    }
}
