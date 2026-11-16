//! Brush dynamics' tip shape in the native rasterizer (`dr_brush_u16_shaped`,
//! `brush_coverage`, THE SHAPED STROKE), black-box through the C ABI against an
//! oracle written here from the rule alone: each dab's own rotated ellipse (its
//! major axis turned by +angle on the y-down canvas, its minor radius the
//! radius times its roundness), the soft smoothstep falloff sampled at the
//! pixel centre, the hard tip's 8×8 sample mean, source-over accumulation in dab
//! order and one rounding to a u16 code.
use crate::native_scalar::{error, text, LIMIT};
use doctored_core::ffi_brush::*;
use serde_json::{json, Value};
use std::ptr;

const W: u32 = 40;
const H: u32 = 30;

fn request(hardness: f64, roundness: f64, angle: f64) -> Value {
    json!({"version":1,"hardness":hardness,"roundness":roundness,"angle_degrees":angle,
           "pixel":false})
}

/// `dr_brush_u16`, or its refusal.
fn plain(request: &Value, dabs: &[f64]) -> Result<Vec<u16>, String> {
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
    if ok {
        Ok(out)
    } else {
        Err(unsafe { error(e) })
    }
}

/// `dr_brush_u16_shaped` (`None` passes NULL shapes), or its refusal; a
/// refusal must leave the output untouched.
fn shaped(request: &Value, dabs: &[f64], shapes: Option<&[f64]>) -> Result<Vec<u16>, String> {
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
            shapes.map_or(ptr::null(), <[f64]>::as_ptr),
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

/// The oracle's coverage of pixel (x, y) by one dab at `angle` and `roundness`,
/// before its deposit.
fn oracle_pixel(dab: &[f64], hardness: f64, angle: f64, roundness: f64, x: u32, y: u32) -> f64 {
    let (cx, cy, r) = (dab[0], dab[1], dab[2] / 2.);
    let (s, c) = angle.to_radians().sin_cos();
    // The ellipse's normalized distance of a canvas point.
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

/// The oracle's stroke: each dab at its own shape, source-over in order, one rounding.
fn oracle(hardness: f64, dabs: &[f64], shapes: &[f64]) -> Vec<u16> {
    let mut field = vec![0_f64; (W * H) as usize];
    for (dab, shape) in dabs.chunks_exact(4).zip(shapes.chunks_exact(2)) {
        for y in 0..H {
            for x in 0..W {
                let c = oracle_pixel(dab, hardness, shape[0], shape[1], x, y) * dab[3];
                let old = &mut field[(y * W + x) as usize];
                *old += c * (1. - *old);
            }
        }
    }
    field.iter().map(|v| (v * 65535.).round() as u16).collect()
}

const DABS: [f64; 20] = [
    10.3, 9.7, 14., 1., //
    16.8, 13.2, 11.5, 0.6, //
    22.4, 15.9, 17., 0.85, //
    27.1, 18.6, 9.25, 1., //
    31.6, 20.3, 13., 0.45,
];

#[test]
fn shapes_equal_to_the_request_give_dr_brush_u16_bit_for_bit() {
    for hardness in [0., 0.35, 1.] {
        for (angle, roundness) in [(0., 1.), (37., 0.45), (-120., 0.08)] {
            let r = request(hardness, roundness, angle);
            let shapes: Vec<f64> = (0..DABS.len() / 4)
                .flat_map(|_| [angle, roundness])
                .collect();
            let want = plain(&r, &DABS).unwrap();
            assert!(want.iter().any(|&v| v > 0));
            assert_eq!(
                shaped(&r, &DABS, Some(&shapes)).unwrap(),
                want,
                "{hardness} {angle}"
            );
        }
    }
}

#[test]
fn mixed_shapes_match_the_oracle_soft_and_hard() {
    let shapes = [
        30., 0.4, //
        -75., 0.9, //
        180., 0.2, //
        -180., 1., //
        111.5, 0.01,
    ];
    for hardness in [0.2, 0.7, 1.] {
        // The request's own shape is none of the dabs', so a rasterizer reading it fails.
        let r = request(hardness, 0.6, 12.);
        let got = shaped(&r, &DABS, Some(&shapes)).unwrap();
        let want = oracle(hardness, &DABS, &shapes);
        let differing = got.iter().zip(&want).filter(|(a, b)| a != b).count();
        assert_eq!(
            differing, 0,
            "hardness {hardness}: {differing} pixels differ"
        );
        assert!(got.iter().filter(|&&v| v > 0).count() > 100);
        assert_ne!(got, plain(&r, &DABS).unwrap(), "the shapes changed nothing");
    }
}

#[test]
fn shaped_refusals_name_their_rule_and_leave_the_output_untouched() {
    let good: Vec<f64> = (0..DABS.len() / 4).flat_map(|_| [10., 0.5]).collect();
    let range =
        "native brush shapes hold an angle from -180 to 180 and a roundness from 0.01 to 1 per dab";
    let soft = request(0.5, 1., 0.);
    assert_eq!(shaped(&soft, &DABS, None).unwrap_err(), range);
    for (index, value) in [
        (0, 180.5),
        (0, -181.),
        (0, f64::NAN),
        (1, 0.005),
        (1, 1.5),
        (3, f64::INFINITY),
    ] {
        let mut bad = good.clone();
        bad[2 + index] = value;
        assert_eq!(
            shaped(&soft, &DABS, Some(&bad)).unwrap_err(),
            range,
            "{index} {value}"
        );
    }
    let stamped = "native brush shapes require a stamped tip that is not aliased";
    let pixel = json!({"version":1,"hardness":1.,"roundness":1.,"angle_degrees":0.,"pixel":true});
    let aliased = json!({"version":1,"hardness":1.,"roundness":1.,"angle_degrees":0.,
                         "pixel":false,"aliased":true});
    for r in [pixel, aliased] {
        assert_eq!(shaped(&r, &DABS, Some(&good)).unwrap_err(), stamped, "{r}");
    }
    // The ends of both ranges are accepted.
    let ends: Vec<f64> = [-180., 0.01, 180., 1., 0., 0.5, 90., 0.25, -90., 0.75].to_vec();
    assert!(shaped(&soft, &DABS, Some(&ends)).is_ok());
}
