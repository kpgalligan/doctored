//! Closed-form independent small-system oracles for versioned soft evidence.
use doctored_core::{
    closed_form_matting::{estimate_soft_prior, ClosedFormParams, MAX_UNKNOWN_PIXELS},
    job_control::DrJobControl,
    matting_input::SoftPriorInput,
};

fn policy(constraints: Option<&[u8]>) -> SoftPriorInput<'_> {
    SoftPriorInput {
        version: 1,
        user_constraints: constraints,
        prior_weight: 0.25,
    }
}

#[test]
fn uncertain_automatic_endpoints_move_under_independent_constant_color_oracle() {
    // One constant-color window gives L = I - 11'/3. Its mean is preserved;
    // deviations shrink by lambda/(1+lambda). This is not the stencil/PCG math.
    let prior = [0, 128, 255];
    let input = policy(None);
    let result = estimate_soft_prior(
        &[[0.5; 3]; 3],
        (3, 1),
        &prior,
        input,
        &ClosedFormParams::default(),
        None,
    )
    .unwrap();
    let mean = prior.iter().map(|&v| f64::from(v)).sum::<f64>() / 3.0;
    let expected: Vec<_> = prior
        .iter()
        .map(|&v| {
            ((input.prior_weight * f64::from(v) + mean) / (1.0 + input.prior_weight)).round() as u8
        })
        .collect();
    assert_eq!(result.alpha.as_raw(), &expected);
    assert!(result.alpha.as_raw()[0] > 0);
    assert!(result.alpha.as_raw()[2] < 255);
}

#[test]
fn only_explicit_user_labels_are_exact_and_unknown_releases_them() {
    let colors = [[0.5; 3]; 3];
    let prior = [255, 128, 0];
    let labels = [0, 128, 255];
    let input = policy(Some(&labels));
    let result = estimate_soft_prior(
        &colors,
        (3, 1),
        &prior,
        input,
        &ClosedFormParams::default(),
        None,
    )
    .unwrap();
    // With endpoints pinned, the remaining equation is
    // (2/3 + lambda) * a = 1/3 + lambda * prior/255.
    let expected = ((255.0 / 3.0 + input.prior_weight * 128.0) / (2.0 / 3.0 + input.prior_weight))
        .round() as u8;
    assert_eq!(result.alpha.as_raw(), &[0, expected, 255]);
    let released = estimate_soft_prior(
        &colors,
        (3, 1),
        &prior,
        policy(Some(&[128; 3])),
        &ClosedFormParams::default(),
        None,
    )
    .unwrap();
    assert!(released.alpha.as_raw()[0] > 0);
    assert!(released.alpha.as_raw()[2] < 255);
    assert_eq!(prior, [255, 128, 0]);
    assert_eq!(labels, [0, 128, 255]);
}

#[test]
fn positive_prior_solves_single_pixel_without_constraint_classes() {
    let result = estimate_soft_prior(
        &[[0.5; 3]],
        (1, 1),
        &[79],
        policy(None),
        &ClosedFormParams::default(),
        None,
    )
    .unwrap();
    assert_eq!(result.alpha.as_raw(), &[79]);
}

#[test]
fn invalid_version_evidence_and_resource_requests_refuse() {
    let params = ClosedFormParams::default();
    for input in [
        SoftPriorInput {
            version: 2,
            ..policy(None)
        },
        SoftPriorInput {
            prior_weight: 0.0,
            ..policy(None)
        },
        SoftPriorInput {
            prior_weight: f64::NAN,
            ..policy(None)
        },
        policy(Some(&[1])),
        policy(Some(&[])),
    ] {
        assert!(estimate_soft_prior(&[[0.5; 3]], (1, 1), &[79], input, &params, None).is_err());
    }
    let n = MAX_UNKNOWN_PIXELS + 1;
    let error = estimate_soft_prior(
        &vec![[0.5; 3]; n],
        (n as u32, 1),
        &vec![128; n],
        policy(None),
        &params,
        None,
    )
    .err()
    .unwrap();
    assert!(error.contains("unknown pixels"));
    let control = DrJobControl::default();
    control.cancel();
    assert!(estimate_soft_prior(
        &[[0.5; 3]],
        (1, 1),
        &[79],
        policy(None),
        &params,
        Some(&control)
    )
    .is_err());
    assert!(estimate_soft_prior(
        &[[0.5; 3]],
        (1, 1),
        &[79],
        policy(None),
        &params,
        Some(&control)
    )
    .is_err());
}
