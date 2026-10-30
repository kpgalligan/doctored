//! Independent continuous ICC references. These tests deliberately exercise
//! sub-byte values; byte identity tests remain in color_tests. The forward
//! encoder's one-grid-interval bound is separate from the 2e-6 compositor gate.

use doctored_core::icc_transform::{Curve, MatrixTrc, PrecisionToneEncoder, PrecisionTransform};

const ENCODE_BOUND: f64 = 1.0 / 65536.0 + 6.0e-8;
fn model(curve: Curve) -> MatrixTrc {
    MatrixTrc::new(
        [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
        std::array::from_fn(|_| curve.clone()),
    )
    .unwrap()
}

#[test]
fn severe_gamma_ramps_keep_the_declared_forward_encoding_bound() {
    for gamma in [0.2_f64, 1.0, 1.8, 2.2, 2.4, 5.0] {
        let encoder = PrecisionToneEncoder::new(&model(Curve::Gamma(gamma))).unwrap();
        let mut max_error = 0.0_f64;
        for code in 0..=65535 {
            let encoded = code as f64 / 65535.0;
            let actual = encoder.rgb([encoded.powf(gamma); 3]);
            max_error = max_error.max((f64::from(actual[0]) - encoded).abs());
            assert_eq!(actual[0], actual[1]);
            assert_eq!(actual[0], actual[2]);
        }
        eprintln!("gamma {gamma}: maximum encoded error {max_error:.10}");
        assert!(max_error <= ENCODE_BOUND, "gamma {gamma}: {max_error}");
    }
}

#[test]
fn all_parametric_kinds_match_independent_piecewise_references() {
    for kind in 0..=4 {
        // Kinds 0/1 are quadratic, 2 has a pedestal and terminal plateau;
        // 3/4 have a continuous toe (with an additional pedestal for 4).
        let curve = Curve::Para {
            kind,
            g: 2.0,
            a: 1.0,
            b: 0.0,
            c: if kind == 2 { 0.02 } else { 0.25 },
            d: 0.25,
            e: if kind == 4 { 0.02 } else { 0.0 },
            f: if kind == 4 { 0.02 } else { 0.0 },
        };
        let encoder = PrecisionToneEncoder::new(&model(curve)).unwrap();
        let mut max_error = 0.0_f64;
        for i in 0..=65535 {
            let linear = i as f64 / 65535.0;
            let reference = match kind {
                0 | 1 => linear.sqrt(),
                2 => (linear - 0.02).max(0.0).sqrt(),
                3 if linear < 0.0625 => linear * 4.0,
                3 => linear.sqrt(),
                4 if linear < 0.0825 => ((linear - 0.02) * 4.0).max(0.0),
                4 => (linear - 0.02).sqrt(),
                _ => unreachable!(),
            };
            let actual = encoder.rgb([linear; 3])[0];
            max_error = max_error.max((f64::from(actual) - reference).abs());
        }
        eprintln!("parametric kind {kind}: maximum encoded error {max_error:.10}");
        assert!(max_error <= ENCODE_BOUND, "kind {kind}: {max_error}");
    }
}

#[test]
fn sampled_curves_choose_the_lowest_preimage_including_terminal_plateaus() {
    let plateau =
        PrecisionToneEncoder::new(&model(Curve::Sampled(vec![0, 32768, 32768, 65535]))).unwrap();
    let midpoint = 32768.0 / 65535.0;
    for (linear, expected) in [(0.0, 0.0), (midpoint, 1.0 / 3.0), (1.0, 1.0)] {
        assert!((f64::from(plateau.rgb([linear; 3])[0]) - expected).abs() <= ENCODE_BOUND);
    }
    let terminal =
        PrecisionToneEncoder::new(&model(Curve::Sampled(vec![0, 65535, 65535]))).unwrap();
    assert_eq!(terminal.rgb([1.0; 3]), [0.5; 3]);
    let nonlinear =
        PrecisionToneEncoder::new(&model(Curve::Sampled(vec![0, 1024, 8192, 65535]))).unwrap();
    let knots = [0.0, 1024.0 / 65535.0, 8192.0 / 65535.0, 1.0];
    let mut maximum = 0.0_f64;
    for i in 0..=65535 {
        let linear = i as f64 / 65535.0;
        let interval = if linear <= knots[1] {
            0
        } else if linear <= knots[2] {
            1
        } else {
            2
        };
        let reference = (interval as f64
            + (linear - knots[interval]) / (knots[interval + 1] - knots[interval]))
            / 3.0;
        maximum = maximum.max((f64::from(nonlinear.rgb([linear; 3])[0]) - reference).abs());
    }
    eprintln!("sampled curve: maximum encoded error {maximum:.10}");
    assert!(maximum <= ENCODE_BOUND);
}

#[test]
fn exact_identity_and_destination_validation_do_not_use_byte_tolerances() {
    let original = model(Curve::Gamma(2.2));
    let near = model(Curve::Gamma(2.20001));
    assert!(original.approx_eq(&near));
    assert!(!original.exact_eq(&near));
    let identity = PrecisionTransform::new(&original, &original).unwrap();
    let input = [1.0 / 65535.0, 0.4000003, 0.8999997];
    assert_eq!(identity.color(input), input);
    let converted = PrecisionTransform::new(&original, &near).unwrap();
    let actual = converted.color(input);
    assert_ne!(actual, input);
    for (input, actual) in input.into_iter().zip(actual) {
        let reference = f64::from(input).powf(2.2 / 2.20001);
        assert!((f64::from(actual) - reference).abs() <= ENCODE_BOUND);
    }
    assert!(identity.retained_bytes() < converted.retained_bytes());
    assert!(
        converted.retained_bytes() <= PrecisionTransform::prepare_scratch_bytes(&original, &near)
    );

    let valid = model(Curve::Identity);
    let invalid = [
        Curve::Gamma(0.0),
        Curve::Gamma(f64::NAN),
        Curve::Sampled(vec![0, 40000, 30000, 65535]),
        Curve::Para {
            kind: 3,
            g: 2.4,
            a: 1.0,
            b: -0.1,
            c: 1.0,
            d: 0.0,
            e: 0.0,
            f: 0.0,
        },
        Curve::Para {
            kind: 1,
            g: 1000.0,
            a: 1000.0,
            b: 0.0,
            c: 0.0,
            d: 0.0,
            e: 0.0,
            f: 0.0,
        },
        Curve::Para {
            kind: 3,
            g: 2.0,
            a: 1.0,
            b: 0.0,
            c: 0.8,
            d: 0.5,
            e: 0.0,
            f: 0.0,
        },
    ];
    // Only an ENCODE destination is validated. A failing source decodes
    // through `Curve::eval`'s repairs, so its output is finite and in [0, 1];
    // an exact identity encodes nothing (a NaN model is never exactly itself).
    let grid = [0.0_f32, 1.0 / 65535.0, 0.25, 0.5, 0.75, 0.9999, 1.0];
    for curve in invalid {
        let invalid = model(curve);
        let out = PrecisionTransform::new(&invalid, &valid).unwrap();
        for r in grid {
            for g in grid {
                for b in grid {
                    for v in out.color([r, g, b]) {
                        assert!(v.is_finite() && (0.0..=1.0).contains(&v), "{v}");
                    }
                }
            }
        }
        assert!(PrecisionTransform::new(&valid, &invalid).is_err());
        let own = PrecisionTransform::new(&invalid, &invalid);
        assert_eq!(own.is_ok(), invalid.exact_eq(&invalid));
        if let Ok(own) = own {
            for x in grid {
                assert_eq!(own.color([x, x, x]), [x, x, x]);
            }
        }
    }
}

/// `rgb` narrows its interval search to a 4096-bucket index; every
/// output must equal the former whole-table search (`rgb_reference`) BIT
/// FOR BIT over every model kind, a dense ramp, every exact table entry with
/// its neighbours, every bucket bound with its neighbours and the special
/// values. Timings are printed, never asserted.
#[test]
fn the_bucket_indexed_search_is_bit_identical_to_the_whole_table_search() {
    use doctored_core::icc::IccProfile;
    use std::time::Instant;

    let mut models: Vec<(String, MatrixTrc)> = Vec::new();
    for gamma in [0.2_f64, 1.0, 1.8, 2.2, 2.4, 5.0] {
        models.push((format!("gamma {gamma}"), model(Curve::Gamma(gamma))));
    }
    models.push(("identity".into(), model(Curve::Identity)));
    for table in [
        vec![0, 32768, 32768, 65535],
        vec![0, 65535, 65535],
        vec![0, 1024, 8192, 65535],
    ] {
        models.push((format!("sampled {table:?}"), model(Curve::Sampled(table))));
    }
    for kind in 0..=4 {
        let curve = Curve::Para {
            kind,
            g: 2.0,
            a: 1.0,
            b: 0.0,
            c: if kind == 2 { 0.02 } else { 0.25 },
            d: 0.25,
            e: if kind == 4 { 0.02 } else { 0.0 },
            f: if kind == 4 { 0.02 } else { 0.0 },
        };
        models.push((format!("parametric kind {kind}"), model(curve)));
    }
    models.push((
        "srgb profile".into(),
        IccProfile::srgb()
            .model()
            .expect("sRGB is matrix/TRC")
            .clone(),
    ));
    models.push((
        "mixed".into(),
        MatrixTrc::new(
            [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
            [
                Curve::Gamma(2.2),
                Curve::Sampled(vec![0, 1024, 8192, 65535]),
                Curve::Para {
                    kind: 3,
                    g: 2.4,
                    a: 1.0 / 1.055,
                    b: 0.055 / 1.055,
                    c: 1.0 / 12.92,
                    d: 0.04045,
                    e: 0.0,
                    f: 0.0,
                },
            ],
        )
        .unwrap(),
    ));

    let mut inputs: Vec<f64> = (0..=1u32 << 20)
        .map(|i| f64::from(i) / f64::from(1u32 << 20))
        .collect();
    for b in 0..=4096u32 {
        let bound = f64::from(b) / 4096.0;
        inputs.extend([bound, bound.next_up(), bound.next_down()]);
    }
    inputs.extend([
        -1.0,
        -0.0,
        0.0,
        1.0,
        1.5,
        f64::NAN,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::MIN_POSITIVE,
        1.0 - f64::EPSILON,
    ]);
    let mut exact = Vec::new();
    for gamma in [0.2_f64, 1.0, 1.8, 2.2, 2.4, 5.0] {
        for i in 0..=65536u32 {
            let entry = (f64::from(i) / 65536.0).powf(gamma);
            exact.extend([entry, entry.next_up(), entry.next_down()]);
        }
    }
    let bits = |v: [f32; 3]| v.map(f32::to_bits);
    let mut timings = Vec::new();
    for (name, destination) in &models {
        let encoder = PrecisionToneEncoder::new(destination).unwrap();
        for (set, values) in [("inputs", &inputs), ("table entries", &exact)] {
            for (i, &v) in values.iter().enumerate() {
                // Rotate the three channels so each sees every value.
                let linear = [
                    v,
                    values[(i + 1) % values.len()],
                    values[(i + 2) % values.len()],
                ];
                assert_eq!(
                    bits(encoder.rgb(linear)),
                    bits(encoder.rgb_reference(linear)),
                    "{name} {set}: {linear:?}"
                );
            }
        }
        let time = |f: &dyn Fn([f64; 3]) -> [f32; 3]| {
            let mut runs = Vec::new();
            for _ in 0..3 {
                let start = Instant::now();
                let mut sink = 0.0f32;
                for &v in &inputs {
                    sink += f([v; 3])[0];
                }
                std::hint::black_box(sink);
                runs.push(start.elapsed().as_secs_f64() * 1e9 / inputs.len() as f64);
            }
            runs.sort_by(f64::total_cmp);
            runs[1]
        };
        let indexed = time(&|l| encoder.rgb(l));
        let reference = time(&|l| encoder.rgb_reference(l));
        timings.push((name.clone(), indexed, reference));
    }
    for (name, indexed, reference) in timings {
        eprintln!(
            "{name}: rgb {indexed:.1} ns/call, rgb_reference {reference:.1} ns/call (medians of 3)"
        );
    }
}
