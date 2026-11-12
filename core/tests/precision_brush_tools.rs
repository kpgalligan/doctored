//! Native paint qualification through C ABI with independent numeric references.
use crate::common::classic_ramp;
use crate::native_blend_oracle as hsl;
use crate::native_scalar as native;
use doctored_core::ffi_brush::*;
use doctored_core::ffi_precision_edit::*;
use doctored_core::icc::IccProfile;
use doctored_core::job_control::DrJobControl;
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer};
use doctored_core::precision_image::PrecisionImage;
use native::*;
use serde_json::{json, Value};
use std::{collections::HashSet, ffi::CStr, ptr};

fn brush_request(hardness: f64, pixel: bool) -> Value {
    json!({"version":1,"hardness":hardness,"roundness":1.,"angle_degrees":0.,"pixel":pixel})
}
fn brush(w: u32, h: u32, request: Value, dabs: &[f64]) -> Vec<u16> {
    let t = text(request);
    let mut e = ptr::null_mut();
    let p = unsafe {
        dr_brush_u16_plan(
            w,
            h,
            t.as_ptr(),
            dabs.as_ptr(),
            dabs.len() / 4,
            LIMIT,
            0,
            &mut e,
        )
    };
    assert!(!p.is_null(), "{}", unsafe { error(e) });
    let plan: Value = serde_json::from_slice(unsafe { CStr::from_ptr(p) }.to_bytes()).unwrap();
    unsafe { doctored_core::ffi::dr_string_free(p) };
    assert_eq!(plan["output_samples"], u64::from(w) * u64::from(h));
    let mut out = vec![777; w as usize * h as usize];
    assert!(
        unsafe {
            dr_brush_u16(
                w,
                h,
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
        },
        "{}",
        unsafe { error(e) }
    );
    out
}
fn paint(mode: u32, color: [f64; 4], opacity: f64) -> Value {
    json!({"op":"paint_blend","layer":0,"color":color,"color_space":"document","opacity":opacity,"blend_mode":mode})
}
fn samples(d: &PrecisionDocument) -> Vec<[u16; 4]> {
    d.layer_source(0).unwrap().to_rgba16().unwrap()
}
fn placed(
    canvas: (u32, u32),
    size: (u32, u32),
    offset: (i32, i32),
    pixels: Vec<[u16; 4]>,
) -> PrecisionDocument {
    let image =
        PrecisionImage::from_rgba16(size.0, size.1, pixels, IccProfile::srgb(), None).unwrap();
    PrecisionDocument::from_parts(
        canvas.0,
        canvas.1,
        IccProfile::srgb(),
        Default::default(),
        Default::default(),
        vec![PrecisionDocumentLayer::raster("raw", image, offset, 1., true).unwrap()],
    )
    .unwrap()
}
#[test]
fn native_soft_dabs_have_more_than_byte_levels_and_one_stroke_round() {
    let out = brush(
        2048,
        1,
        brush_request(0., false),
        &[1024., 0.5, 2048., 0.731],
    );
    assert!(out.iter().copied().collect::<HashSet<_>>().len() > 900);
    for (x, &v) in out.iter().enumerate() {
        let r = ((x as f64 + 0.5) - 1024.).abs() / 1024.;
        assert_eq!(v, q((1. - 3. * r * r + 2. * r * r * r) * 0.731 * 65535.));
    }
    let dabs = [0.5, 0.5, 1., 0.00001, 0.5, 0.5, 1., 0.00001];
    assert_eq!(brush(1, 1, brush_request(1., true), &dabs), [1]);
    let mut request = brush_request(1., true);
    request["opacity"] = json!(0.4);
    assert_eq!(
        brush(
            1,
            1,
            request,
            &[0.5, 0.5, 1., 0.0000107, 0.5, 0.5, 1., 0.0000107]
        ),
        [1]
    );
    assert_eq!(
        brush(3, 1, brush_request(1., true), &[1.5, 0.5, 1., 1.]),
        [0, 65535, 0]
    );
    assert_eq!(
        brush(3, 1, brush_request(1., true), &[1., 0.5, 1., 1.]),
        [32768, 32768, 0]
    );
}
#[test]
fn native_hard_ellipse_area_and_rotated_soft_geometry() {
    let disk = brush(12, 12, brush_request(1., false), &[6., 6., 8., 1.]);
    let area = disk.iter().map(|&v| f64::from(v) / 65535.).sum::<f64>();
    assert!((area - std::f64::consts::PI * 16.).abs() < 0.3, "{area}");
    let a = brush(
        11,
        11,
        json!({"version":1,"hardness":0.3,"roundness":0.4,"angle_degrees":0.,"pixel":false}),
        &[5.5, 5.5, 9., 0.61],
    );
    let b = brush(
        11,
        11,
        json!({"version":1,"hardness":0.3,"roundness":0.4,"angle_degrees":90.,"pixel":false}),
        &[5.5, 5.5, 9., 0.61],
    );
    for y in 0..11 {
        for x in 0..11 {
            assert_eq!(a[y * 11 + x], b[x * 11 + y]);
        }
    }
}
#[test]
fn native_positive_tip_angle_tilts_counterclockwise_on_screen() {
    // Canvas y increases downward. At +30 degrees the major axis points
    // upper-right: (8,-4) lies in its hard center, while (8,+4) misses the
    // narrow ellipse. These fixed screen positions distinguish the sign.
    for angle in [30., -30.] {
        let out = brush(
            41,
            41,
            json!({"version":1,"hardness":0.65,"roundness":0.3,"angle_degrees":angle,"pixel":false}),
            &[20.5, 20.5, 32., 1.],
        );
        let upper_right = out[16 * 41 + 28];
        let lower_right = out[24 * 41 + 28];
        if angle > 0. {
            assert_eq!((upper_right, lower_right), (65535, 0));
        } else {
            assert_eq!((upper_right, lower_right), (0, 65535));
        }
    }
}
fn separable(mode: u32, b: f64, s: f64) -> f64 {
    let burn = |b: f64, s: f64| {
        if b == 1. {
            1.
        } else if s == 0. {
            0.
        } else {
            1. - ((1. - b) / s).min(1.)
        }
    };
    let dodge = |b: f64, s: f64| {
        if b == 0. {
            0.
        } else if s == 1. {
            1.
        } else {
            (b / (1. - s)).min(1.)
        }
    };
    let hard = |b: f64, s: f64| {
        if s <= 0.5 {
            2. * b * s
        } else {
            1. - 2. * (1. - b) * (1. - s)
        }
    };
    match mode {
        0 => s,
        1 => b * s,
        2 => 1. - (1. - b) * (1. - s),
        3 => hard(s, b),
        4 => {
            if s <= 0.5 {
                b - (1. - 2. * s) * b * (1. - b)
            } else {
                let d = if b <= 0.25 {
                    16. * b * b * b - 12. * b * b + 4. * b
                } else {
                    b.sqrt()
                };
                b + (2. * s - 1.) * (d - b)
            }
        }
        5 => hard(b, s),
        6 => b.min(s),
        7 => b.max(s),
        8 => (b - s).abs(),
        9 => b + s - 2. * b * s,
        10 => dodge(b, s),
        11 => burn(b, s),
        12 => (b + s).min(1.),
        13 => (b - s).max(0.),
        15 => (b + s - 1.).max(0.),
        18 => {
            if s <= 0.5 {
                burn(b, 2. * s)
            } else {
                dodge(b, 2. * s - 1.)
            }
        }
        19 => (b + 2. * s - 1.).clamp(0., 1.),
        20 => {
            if s <= 0.5 {
                b.min(2. * s)
            } else {
                b.max(2. * s - 1.)
            }
        }
        21 => {
            if b + s >= 1. {
                1.
            } else {
                0.
            }
        }
        22 => {
            if s == 0. {
                1.
            } else {
                (b / s).min(1.)
            }
        }
        _ => panic!("mode"),
    }
}
fn reference(mode: u32, base: [u16; 4], source: [f64; 4], coverage: u16, opacity: f64) -> [u16; 4] {
    let b = base.map(|v| f64::from(v) / 65535.);
    let s = [source[0], source[1], source[2]];
    let a = source[3] * f64::from(coverage) / 65535. * opacity;
    // Pixel(0,0) has hash threshold0 in the specified deterministic dissolve.
    if mode == 14 {
        return [q(s[0] * 65535.), q(s[1] * 65535.), q(s[2] * 65535.), 65535];
    }
    let rgb = if mode >= 23 {
        hsl::hsl(mode as i32, [b[0], b[1], b[2]], s)
    } else if mode == 16 || mode == 17 {
        let luminance = |c: [f64; 3]| 0.3 * c[0] + 0.59 * c[1] + 0.11 * c[2];
        let back = [b[0], b[1], b[2]];
        if (mode == 16 && luminance(s) < luminance(back))
            || (mode == 17 && luminance(s) > luminance(back))
        {
            s
        } else {
            back
        }
    } else {
        std::array::from_fn(|c| separable(mode, b[c], s[c]))
    };
    let alpha = a + b[3] * (1. - a);
    let mut out = [0; 4];
    for c in 0..3 {
        out[c] = q(
            (a * ((1. - b[3]) * s[c] + b[3] * rgb[c]) + (1. - a) * b[3] * b[c]) / alpha * 65535.,
        );
    }
    out[3] = q(alpha * 65535.);
    out
}
#[test]
fn every_native_paint_mode_matches_independent_partial_alpha_reference() {
    for base in [
        [11001, 41003, 53009, 25001],
        [65535, 0, 40001, 1],
        [1, 2, 3, 65535],
    ] {
        let d = doc(1, 1, vec![base]);
        for mode in 0..=26 {
            let color = [0.713, 0.291, 0.431, 0.671];
            let coverage = 32769;
            let out = edit(&d, paint(mode, color, 0.731), &[coverage]).unwrap_or_else(|| d.clone());
            let expected = reference(mode, base, color, coverage, 0.731);
            for (c, &value) in samples(&out)[0].iter().enumerate() {
                assert!(
                    value.abs_diff(expected[c]) <= 1,
                    "mode{mode},c{c}: {value} != {}",
                    expected[c]
                );
            }
        }
    }
}
#[test]
fn tiny_source_alpha_and_separate_selection_avoid_intermediate_rounding() {
    let d = doc(1, 1, vec![[65535, 0, 0, 1]]);
    let out = edit(&d, paint(0, [0., 0., 1., 1. / 65535.], 1.), &[1]).unwrap();
    assert_eq!(samples(&out), [[65534, 0, 1, 1]]);
    let d = doc(2, 1, vec![[0, 0, 0, 65535]; 2]);
    let mut request = paint(0, [1., 0., 0., 1.], 0.75);
    request["selection"] = json!(true);
    let out = edit(&d, request, &[40000, 65535, 1, 65535]).unwrap();
    assert_eq!(samples(&out)[0], [0, 0, 0, 65535]);
    let out = edit(
        &d,
        json!({"op":"erase_native","layer":0,"opacity":0.75,"selection":true}),
        &[40000, 65535, 1, 65535],
    )
    .unwrap();
    assert_eq!(samples(&out)[0], [0, 0, 0, 65535]);
    assert_eq!(samples(&out)[1][3], 16384);
}
#[test]
fn exact_changed_ink_expansion_preserves_raw_pixels_and_independent_mask() {
    let d = placed(
        (7, 2),
        (2, 1),
        (2, 0),
        vec![[1, 2, 3, 0], [10001, 20003, 30007, 40009]],
    );
    let d=edit(&d,json!({"op":"set_mask","layer":0,"width":3,"height":1,"x":-1,"y":1,"density":0.7,"feather":0.,"linked":false,"enabled":true}),&[1,30001,65535]).unwrap();
    let mut coverage = [0; 14];
    coverage[5] = 65535;
    let out = edit(&d, paint(0, [0.2, 0.3, 0.4, 1.], 1.), &coverage).unwrap();
    assert_eq!(out.layer(0).unwrap().offset(), (2, 0));
    assert_eq!(out.layer_source(0).unwrap().dimensions(), (4, 1));
    assert_eq!(&samples(&out)[..2], samples(&d));
    assert_eq!(samples(&out)[2], [0; 4]);
    let mask = out.layer(0).unwrap().mask().unwrap();
    assert_eq!(mask.plane().samples(), [1, 30001, 65535]);
    assert_eq!(mask.properties().offset, (-1, 1));
    assert!(edit(
        &d,
        json!({"op":"erase_native","layer":0,"opacity":1.}),
        &coverage
    )
    .is_none());
    assert!(edit(&d, paint(0, [1., 1., 1., 0.], 1.), &coverage).is_none());
}
#[test]
fn bucket_uses_raw_native_matching_including_absent_canvas_storage() {
    let d = placed(
        (5, 1),
        (2, 1),
        (2, 0),
        vec![[10001, 10001, 10001, 65535], [10002, 10002, 10002, 65535]],
    );
    let request = json!({"op":"bucket_fill","layer":0,"color":[1.,0.,0.,1.],"color_space":"document","opacity":1.,"x":0,"y":0,"tolerance":0,"contiguous":true});
    let out = edit(&d, request.clone(), &[65535; 5]).unwrap();
    assert_eq!(out.layer(0).unwrap().offset(), (0, 0));
    assert_eq!(out.layer_source(0).unwrap().dimensions(), (4, 1));
    assert_eq!(samples(&out)[..2], [[65535, 0, 0, 65535]; 2]);
    assert_eq!(&samples(&out)[2..], samples(&d));
    let mut request = request;
    request["contiguous"] = json!(false);
    let out = edit(&d, request.clone(), &[65535; 5]).unwrap();
    assert_eq!(out.layer_source(0).unwrap().dimensions(), (5, 1));
    assert_eq!(samples(&out)[4], [65535, 0, 0, 65535]);
    request["x"] = json!(2);
    request["tolerance"] = json!(0);
    let out = edit(&d, request.clone(), &[65535; 5]).unwrap();
    assert_eq!(samples(&out)[1], samples(&d)[1]);
    request["tolerance"] = json!(1);
    let out = edit(&d, request, &[65535; 5]).unwrap();
    assert_eq!(samples(&out), [[65535, 0, 0, 65535]; 2]);
}
#[test]
fn bucket_excludes_zero_coverage_seed_but_admits_native_code_one() {
    let original = vec![[0, 0, 0, 65535]; 3];
    let d = doc(3, 1, original.clone());
    for contiguous in [false, true] {
        let mut request = json!({"op":"bucket_fill","layer":0,"color":[1.,0.,0.,1.],"color_space":"document","opacity":1.,"x":0,"y":0,"tolerance":0,"contiguous":contiguous});
        assert!(edit(&d, request.clone(), &[0, 65535, 0]).is_none());
        let next = edit(&d, request.clone(), &[1, 65535, 0]).unwrap();
        assert_eq!(
            samples(&next),
            [[1, 0, 0, 65535], [65535, 0, 0, 65535], original[2]]
        );
        request["selection"] = json!(true);
        for input in [
            [65535, 65535, 65535, 0, 65535, 0],
            [0, 65535, 65535, 65535, 65535, 0],
        ] {
            assert!(edit(&d, request.clone(), &input).is_none());
        }
        // Both seed gates are positive even when their product is below one
        // u16 code. The selected matching neighbor must still receive ink.
        let next = edit(&d, request, &[1, 65535, 0, 1, 65535, 0]).unwrap();
        assert_eq!(
            samples(&next),
            [original[0], [65535, 0, 0, 65535], original[2]]
        );
        assert_eq!(samples(&d), original);
    }
}
/// The two-stop Classic ramp's fraction at `t` (`common::classic_ramp`).
fn smooth(t: f64) -> f64 {
    classic_ramp(&[(0.0, 0.0), (1.0, 1.0)], t)
}

#[test]
fn color_and_scalar_gradients_share_continuous_canvas_field() {
    let d = doc(4, 1, vec![[1001, 3003, 5005, 27001]; 4]);
    for radial in [false, true] {
        let out=edit(&d,json!({"op":"gradient","layer":0,"start":[0.,0.2,0.9,0.1],"end":[1.,0.8,0.3,0.7],"color_space":"document","opacity":0.731,"from":[0.,0.5],"to":[4.,0.5],"radial":radial}),&[1,30001,65535,0]).unwrap();
        for (i, &coverage) in [1, 30001, 65535, 0].iter().enumerate() {
            if coverage == 0 {
                assert_eq!(samples(&out)[i], samples(&d)[i]);
                continue;
            }
            let t = (i as f64 + 0.5) / 4.;
            // A linear or radial drag's colour follows the Classic ramp;
            // alpha stays linear.
            let s = smooth(t);
            let color = [s, 0.2 + 0.6 * s, 0.9 - 0.6 * s, 0.1 + 0.6 * t];
            let expected = reference(0, samples(&d)[i], color, coverage, 0.731);
            for (a, b) in samples(&out)[i].iter().zip(expected) {
                assert!(a.abs_diff(b) <= 1);
            }
        }
        for target in ["red", "green", "blue", "alpha"] {
            let out=scalar(&d,target,json!({"kind":"gradient","from":[0.,0.5],"to":[4.,0.5],"radial":radial,"start":1,"end":60001,"opacity":0.731}),true,&[1,30001,65535,0]).unwrap();
            let c = match target {
                "red" => 0,
                "green" => 1,
                "blue" => 2,
                _ => 3,
            };
            for i in 0..4 {
                let t = (i as f64 + 0.5) / 4.;
                let s = smooth(t);
                let coverage = f64::from([1, 30001, 65535, 0][i]) / 65535. * 0.731;
                let value =
                    q(f64::from(samples(&d)[i][c]) * (1. - coverage)
                        + (1. + 60000. * s) * coverage);
                assert_eq!(samples(&out)[i][c], value);
            }
        }
    }
}
#[test]
fn six_dodge_burn_curves_preserve_alpha_and_hidden_rgb() {
    let base = [[1, 17003, 51001, 31001], [11001, 22003, 33007, 0]];
    let d = doc(2, 1, base.to_vec());
    for range in 0..3 {
        for burn in [false, true] {
            let out=edit(&d,json!({"op":"dodge_burn","layer":0,"exposure":0.731,"range":range,"burn":burn,"selection":true}),&[40001,65535,32003,65535]).unwrap();
            let weight = 40001. * 32003. / 65535_f64.powi(2);
            let e = 0.731;
            let k = e / 3.;
            for (c, &component) in base[0][..3].iter().enumerate() {
                let v = f64::from(component) / 65535.;
                let target = match (range, burn) {
                    (0, false) => k + v * (1. - k),
                    (0, true) => ((v - k) / (1. - k)).max(0.),
                    (1, false) => v.powf(1. / (1. + e)),
                    (1, true) => v.powf(1. + e),
                    (2, false) => (v / (1. - k)).min(1.),
                    _ => v * (1. - k),
                };
                assert!(
                    samples(&out)[0][c].abs_diff(q((v * (1. - weight) + target * weight) * 65535.))
                        <= 1
                );
            }
            assert_eq!(samples(&out)[0][3], base[0][3]);
            assert_eq!(samples(&out)[1], base[1]);
        }
    }
}
#[test]
fn scalar_gradient_alpha_interpolates_independently_and_preserves_offcanvas_mask() {
    let d = doc(3, 1, vec![[1, 2, 3, 4]; 3]);
    let d = edit(
        &d,
        json!({"op":"set_mask","layer":0,"width":5,"height":1,"x":-1,"y":0}),
        &[7, 10001, 20003, 30007, 9],
    )
    .unwrap();
    let op = json!({"kind":"gradient","from":[0.,0.5],"to":[3.,0.5],"radial":false,"start":60001,"end":101,"start_alpha":0.71,"end_alpha":0.00001,"opacity":0.61});
    let next = scalar(&d, "mask", op.clone(), false, &[]).unwrap();
    let mask = next.layer(0).unwrap().mask().unwrap().plane().samples();
    assert_eq!(mask[0], 7);
    assert_eq!(mask[4], 9);
    for i in 0..3 {
        let t = (i as f64 + 0.5) / 3.;
        let alpha = (0.71 * (1. - t) + 0.00001 * t) * 0.61;
        let expected = q(f64::from([10001, 20003, 30007][i]) * (1. - alpha)
            + (60001. + (101. - 60001.) * smooth(t)) * alpha);
        assert_eq!(mask[i + 1], expected);
    }
    let d = edit(
        &d,
        json!({"op":"set_channel","name":"gradient"}),
        &[10001, 20003, 30007],
    )
    .unwrap();
    let next = scalar(&d, "channel", op, true, &[1, 32769, 65535]).unwrap();
    let channel = next.channels()[0].plane().samples();
    for i in 0..3 {
        let t = (i as f64 + 0.5) / 3.;
        let alpha =
            (0.71 * (1. - t) + 0.00001 * t) * 0.61 * f64::from([1, 32769, 65535][i]) / 65535.;
        let expected = q(f64::from([10001, 20003, 30007][i]) * (1. - alpha)
            + (60001. + (101. - 60001.) * smooth(t)) * alpha);
        assert_eq!(channel[i], expected);
    }
}
#[test]
fn native_edit_refusals_cancellation_and_zero_coverage_preserve_capture() {
    let d = doc(2, 1, vec![[101, 202, 303, 404]; 2]);
    assert!(edit(&d, paint(0, [1.; 4], 1.), &[0, 0]).is_none());
    let original = samples(&d).to_vec();
    for request in [
        paint(27, [1.; 4], 1.),
        paint(0, [1.; 4], 2.),
        json!({"op":"gradient","layer":0,"start":[0.,0.,0.,1.],"end":[1.,1.,1.,1.],"color_space":"document","opacity":1.,"from":[0.,0.],"to":[0.,0.],"radial":false}),
        json!({"op":"dodge_burn","layer":0,"exposure":1.,"range":3,"burn":false}),
        json!({"op":"erase_native","layer":0,"opacity":1.,"blend_mode":1}),
    ] {
        let t = text(request);
        assert!(
            unsafe { dr_precision_doc_edit_plan(&d, t.as_ptr(), LIMIT, 0, ptr::null_mut()) }
                .is_null()
        );
    }
    let t = text(paint(0, [1.; 4], 1.));
    let coverage = [65535; 2];
    let control = DrJobControl::default();
    control.cancel();
    assert!(unsafe {
        dr_precision_doc_edit(
            &d,
            t.as_ptr(),
            coverage.as_ptr(),
            2,
            LIMIT,
            0,
            &control,
            ptr::null_mut(),
        )
    }
    .is_null());
    assert!(unsafe {
        dr_precision_doc_edit(
            &d,
            t.as_ptr(),
            coverage.as_ptr(),
            1,
            LIMIT,
            0,
            ptr::null(),
            ptr::null_mut(),
        )
    }
    .is_null());
    assert!(unsafe {
        dr_precision_doc_edit(
            &d,
            t.as_ptr(),
            coverage.as_ptr(),
            2,
            1,
            0,
            ptr::null(),
            ptr::null_mut(),
        )
    }
    .is_null());
    assert_eq!(samples(&d), original);
}
#[test]
fn brush_refusals_and_new_exports_null_sweep_leave_output_untouched() {
    unsafe {
        assert!(
            dr_brush_u16_plan(0, 0, ptr::null(), ptr::null(), 0, LIMIT, 0, ptr::null_mut())
                .is_null()
        );
        assert!(!dr_brush_u16(
            0,
            0,
            ptr::null(),
            ptr::null(),
            0,
            ptr::null_mut(),
            0,
            LIMIT,
            0,
            ptr::null(),
            ptr::null_mut()
        ));
        assert!(!dr_brush_overlay_rgba8(
            0,
            0,
            ptr::null(),
            ptr::null(),
            0,
            ptr::null(),
            ptr::null(),
            0,
            ptr::null_mut(),
            0,
            LIMIT,
            0,
            ptr::null(),
            ptr::null_mut()
        ));
    }
    let t = text(brush_request(0., false));
    let dabs = [0.5, 0.5, 1., 1.];
    let mut out = [777];
    for (w, h, count, len, limit) in [
        (1, 1, 50001, 1, LIMIT),
        (1, 1, usize::MAX, 1, LIMIT),
        (1, 1, 1, 0, LIMIT),
        (1, 1, 1, 1, 1),
        (0, 1, 1, 1, LIMIT),
    ] {
        assert!(!unsafe {
            dr_brush_u16(
                w,
                h,
                t.as_ptr(),
                dabs.as_ptr(),
                count,
                out.as_mut_ptr(),
                len,
                limit,
                0,
                ptr::null(),
                ptr::null_mut(),
            )
        });
        assert_eq!(out, [777]);
    }
    for dab in [[f64::NAN, 0., 1., 1.], [0., 0., -1., 1.], [0., 0., 1., 2.]] {
        assert!(!unsafe {
            dr_brush_u16(
                1,
                1,
                t.as_ptr(),
                dab.as_ptr(),
                1,
                out.as_mut_ptr(),
                1,
                LIMIT,
                0,
                ptr::null(),
                ptr::null_mut(),
            )
        });
        assert_eq!(out, [777]);
    }
    // 2,000 soft 1024-px dabs: 2.1·10⁹ box pixels, past the 2·10⁹ work cap.
    let excessive = [512., 512., 1024., 1.].repeat(2000);
    assert!(unsafe {
        dr_brush_u16_plan(
            1024,
            1024,
            t.as_ptr(),
            excessive.as_ptr(),
            2000,
            LIMIT,
            0,
            ptr::null_mut(),
        )
    }
    .is_null());
    let control = DrJobControl::default();
    control.cancel();
    assert!(!unsafe {
        dr_brush_u16(
            1,
            1,
            t.as_ptr(),
            dabs.as_ptr(),
            1,
            out.as_mut_ptr(),
            1,
            LIMIT,
            0,
            &control,
            ptr::null_mut(),
        )
    });
    assert_eq!(out, [777]);
    let d = doc(1, 1, vec![[1, 2, 3, 4]]);
    let t = text(paint(27, [1.; 4], 1.));
    assert!(
        unsafe { dr_precision_doc_edit_plan(&d, t.as_ptr(), LIMIT, 0, ptr::null_mut()) }.is_null()
    );
}
#[test]
fn active_brush_cancellation_after_dab_progress_is_atomic() {
    let control = DrJobControl::default();
    let t = text(brush_request(0.5, false));
    let dabs = [256., 256., 500., 0.01].repeat(800);
    let mut out = vec![12345; 512 * 512];
    std::thread::scope(|scope| {
        let worker = scope.spawn(|| unsafe {
            dr_brush_u16(
                512,
                512,
                t.as_ptr(),
                dabs.as_ptr(),
                800,
                out.as_mut_ptr(),
                out.len(),
                LIMIT,
                0,
                &control,
                ptr::null_mut(),
            )
        });
        while control.progress() == 0. {
            std::thread::yield_now();
        }
        control.cancel();
        assert!(!worker.join().unwrap());
    });
    assert!(out.iter().all(|&v| v == 12345));
    assert_eq!(control.scratch_bytes(), 0);
}
#[test]
fn canonical_clone_and_healing_keep_tiny_selection_until_native_writeback() {
    let d = doc(2, 1, vec![[65535, 0, 0, 1], [0, 0, 65535, 1]]);
    let out = edit(
        &d,
        json!({"op":"retouch_clone","layer":0,"matrix":[1.,0.,0.,1.,1.,0.],"selection":true}),
        &[1, 0, 65535, 65535],
    )
    .unwrap();
    assert_eq!(samples(&out)[0], [65534, 0, 1, 1]);
    let mut base = vec![[40000, 40000, 40000, 65535]; 25];
    base[12] = [1000, 1000, 1000, 65535];
    let target = PrecisionImage::from_rgba16(5, 5, base.clone(), IccProfile::srgb(), None).unwrap();
    let source = PrecisionImage::from_rgba16(
        5,
        5,
        vec![[10000, 10000, 10000, 65535]; 25],
        IccProfile::srgb(),
        None,
    )
    .unwrap();
    let d = PrecisionDocument::from_parts(
        5,
        5,
        IccProfile::srgb(),
        Default::default(),
        Default::default(),
        vec![
            PrecisionDocumentLayer::raster("target", target, (0, 0), 1., true).unwrap(),
            PrecisionDocumentLayer::raster("source", source, (0, 0), 1., true).unwrap(),
        ],
    )
    .unwrap();
    let mut input = vec![0; 50];
    for y in 1..4 {
        for x in 1..4 {
            input[y * 5 + x] = 65535;
            input[25 + y * 5 + x] = 1;
        }
    }
    let out=edit(&d,json!({"op":"retouch_heal","layer":0,"source_layer":1,"selection":true,"x":1,"y":1,"width":3,"height":3}),&input).unwrap();
    assert_eq!(samples(&out)[12], [1001, 1001, 1001, 65535]);
    for (i, &pixel) in base.iter().enumerate() {
        if i != 12 {
            assert_eq!(samples(&out)[i], pixel);
        }
    }
    input[25..].fill(0);
    assert!(edit(
        &d,
        json!({"op":"retouch_heal","layer":0,"source_layer":1,"selection":true}),
        &input
    )
    .is_none());
}
#[test]
fn canonical_spot_heal_keeps_subbyte_final_selection() {
    let mut base = vec![[40000, 40000, 40000, 65535]; 1024];
    base[16 * 32 + 16] = [1000, 1000, 1000, 65535];
    let d = doc(32, 32, base.clone());
    let mut input = vec![0; 2048];
    for y in 15..18 {
        for x in 15..18 {
            input[y * 32 + x] = 65535;
            input[1024 + y * 32 + x] = 1;
        }
    }
    let next=edit(&d,json!({"op":"spot_heal","layer":0,"selection":true,"x":15,"y":15,"width":3,"height":3,"ring":12,"seed":17}),&input).unwrap();
    assert_eq!(samples(&next)[16 * 32 + 16], [1001, 1001, 1001, 65535]);
    for (i, pixel) in samples(&next).iter().enumerate() {
        if i != 16 * 32 + 16 {
            assert_eq!(*pixel, base[i]);
        }
    }
}
