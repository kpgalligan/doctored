use doctored_core::bayesian_color::{estimate, Gaussian, MAX_EVALUATIONS, OBSERVATION_VARIANCE};

fn isotropic(mean: [f64; 3], variance: f64) -> Gaussian {
    Gaussian {
        mean,
        covariance: [
            [variance, 0.0, 0.0],
            [0.0, variance, 0.0],
            [0.0, 0.0, variance],
        ],
    }
}

#[test]
fn known_colors_recover_fractional_alpha_from_both_binary_priors() {
    let f = isotropic([0.8, 0.2, 0.6], OBSERVATION_VARIANCE);
    let b = isotropic([0.1, 0.7, 0.2], OBSERVATION_VARIANCE);
    let alpha = 0.37;
    let color = [0.359, 0.515, 0.348];
    for prior in [0.0, 1.0] {
        let result = estimate(color, prior, &f, &b, None).unwrap();
        assert!((result.alpha - alpha).abs() < 0.001);
        for c in 0..3 {
            assert!((result.foreground[c] - f.mean[c]).abs() < 0.001);
            assert!((result.background[c] - b.mean[c]).abs() < 0.001);
        }
        assert_eq!(result.evaluations, MAX_EVALUATIONS);
    }
}

#[test]
fn pure_inputs_and_explicit_constraints_preserve_exact_endpoints() {
    let f = isotropic([0.2, 0.6, 0.8], 0.01);
    let b = isotropic([0.7, 0.1, 0.3], 0.01);
    for (alpha, color) in [(0.0, b.mean), (1.0, f.mean)] {
        let result = estimate(color, alpha, &f, &b, None).unwrap();
        assert_eq!(result.alpha, alpha);
        assert_eq!(
            if alpha == 1.0 {
                result.foreground
            } else {
                result.background
            },
            color
        );
    }
    let color = [0.3, 0.4, 0.5];
    for alpha in [0.0, 0.123456789, 1.0] {
        let result = estimate(color, 1.0 - alpha, &f, &b, Some(alpha)).unwrap();
        assert_eq!(result.alpha, alpha);
        assert_eq!(result.evaluations, 1);
        if alpha == 1.0 {
            assert_eq!(result.foreground, color);
        } else if alpha == 0.0 {
            assert_eq!(result.background, color);
        }
    }
}

#[test]
fn clipped_colors_are_scored_using_the_actual_published_values() {
    let f = isotropic([0.9, 0.1, 0.9], 0.04);
    let b = isotropic([0.1, 0.9, 0.1], OBSERVATION_VARIANCE);
    let color = [1.0, 0.0, 1.0];
    let result = estimate(color, 0.4, &f, &b, Some(0.1)).unwrap();
    assert_eq!(result.foreground, [1.0, 0.0, 1.0]);
    assert!(result
        .foreground
        .into_iter()
        .chain(result.background)
        .all(|v| (0.0..=1.0).contains(&v)));
    let mut energy = 0.5 * (0.1_f64 - 0.4).powi(2) / 0.25;
    for (c, observed) in color.into_iter().enumerate() {
        let residual = observed - 0.1 * result.foreground[c] - 0.9 * result.background[c];
        energy += 0.5 * residual.powi(2) / OBSERVATION_VARIANCE;
        energy += 0.5 * (result.foreground[c] - f.mean[c]).powi(2) / 0.04;
        energy += 0.5 * (result.background[c] - b.mean[c]).powi(2) / OBSERVATION_VARIANCE;
        energy += 0.5 * (0.04 * OBSERVATION_VARIANCE).ln();
    }
    assert!((result.objective - energy).abs() < 1e-8);
}

#[test]
fn isotropic_solution_agrees_with_independent_dense_scalar_oracle() {
    let variance = 0.002;
    let f = isotropic([0.7, 0.3, 0.5], variance);
    let b = isotropic([0.2, 0.6, 0.4], variance);
    let color = [0.45, 0.47, 0.43];
    let prior = 0.8;
    let result = estimate(color, prior, &f, &b, None).unwrap();
    // In the isotropic case the analytically minimized data/color energy is
    // residual squared divided by the scalar effective variance. This dense
    // one-dimensional oracle uses neither matrix factorization nor search.
    let energy = |alpha: f64| {
        let residual: f64 = (0..3)
            .map(|c| (color[c] - alpha * f.mean[c] - (1.0 - alpha) * b.mean[c]).powi(2))
            .sum();
        0.5 * residual / (OBSERVATION_VARIANCE + variance * (alpha * alpha + (1.0 - alpha).powi(2)))
            + 0.5 * (alpha - prior).powi(2) / 0.25
            + 3.0 * variance.ln()
    };
    let minimum = (0..=10000)
        .map(|i| energy(i as f64 / 10000.0))
        .fold(f64::INFINITY, f64::min);
    assert!((result.objective - energy(result.alpha)).abs() < 1e-10);
    assert!(result.objective - minimum < 0.0001);
    assert!(result.evaluations <= MAX_EVALUATIONS);
}

#[test]
fn correlated_covariance_changes_the_conditional_color_across_channels() {
    let f = Gaussian {
        mean: [0.5; 3],
        covariance: [[0.02, 0.01, 0.0], [0.01, 0.02, 0.0], [0.0, 0.0, 0.01]],
    };
    let b = isotropic([0.5; 3], 0.01);
    let result = estimate([0.6, 0.5, 0.5], 0.5, &f, &b, Some(0.5)).unwrap();
    assert!(result.foreground[1] > 0.5);
    assert!(result.background[1] < 0.5);
    assert_eq!(result.foreground[2], 0.5);
    assert_eq!(result.background[2], 0.5);
}

#[test]
fn malformed_and_nonpositive_covariances_refuse() {
    let good = isotropic([0.5; 3], OBSERVATION_VARIANCE);
    for value in [f64::NAN, f64::INFINITY, -0.1, 1.1] {
        assert!(estimate([value, 0.5, 0.5], 0.5, &good, &good, None).is_err());
        assert!(estimate([0.5; 3], value, &good, &good, None).is_err());
        assert!(estimate([0.5; 3], 0.5, &good, &good, Some(value)).is_err());
        assert!(estimate([0.5; 3], 0.5, &isotropic([value; 3], 0.01), &good, None).is_err());
    }
    for variance in [0.0, -0.1, f64::NAN, f64::INFINITY] {
        assert!(estimate([0.5; 3], 0.5, &isotropic([0.5; 3], variance), &good, None).is_err());
    }
    let mut asymmetric = good;
    asymmetric.covariance[0][1] = 0.000001;
    assert!(estimate([0.5; 3], 0.5, &asymmetric, &good, None).is_err());
    let mut indefinite = good;
    indefinite.covariance[0][1] = 0.1;
    indefinite.covariance[1][0] = 0.1;
    assert!(estimate([0.5; 3], 0.5, &indefinite, &good, None).is_err());
}
