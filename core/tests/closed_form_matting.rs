//! Independent dense normal-equation oracles for the local matting solver.
use crate::common;
use common::probe::{cancelled_parked_at, entry_then_cancel, parked_line};
use doctored_core::{
    closed_form_matting::{estimate, ClosedFormParams, MAX_UNKNOWN_PIXELS},
    ffi_jobs::dr_job_control_set_scratch_limit,
    job_control::DrJobControl,
};

/// Pivoted dense Gaussian elimination is intentionally independent from the
/// production centered3x3 Cholesky and stencil/PCG implementations.
#[allow(clippy::needless_range_loop)]
fn dense_solve(mut a: Vec<Vec<f64>>, mut b: Vec<f64>) -> Vec<f64> {
    let n = b.len();
    for k in 0..n {
        let pivot = (k..n)
            .max_by(|&i, &j| a[i][k].abs().total_cmp(&a[j][k].abs()))
            .unwrap();
        a.swap(k, pivot);
        b.swap(k, pivot);
        let d = a[k][k];
        assert!(d.abs() > 1e-14);
        for j in k..n {
            a[k][j] /= d;
        }
        b[k] /= d;
        for i in 0..n {
            if i != k {
                let factor = a[i][k];
                for j in k..n {
                    a[i][j] -= factor * a[k][j];
                }
                b[i] -= factor * b[k];
            }
        }
    }
    b
}

#[allow(clippy::needless_range_loop)]
fn dense_oracle(colors: &[[f32; 3]], w: usize, h: usize, trimap: &[u8], epsilon: f64) -> Vec<f64> {
    let n = w * h;
    let mut laplacian = vec![vec![0.; n]; n];
    let (ww, wh) = (w.min(3), h.min(3));
    for y in 0..=h - wh {
        for x in 0..=w - ww {
            let pixels: Vec<_> = (0..wh)
                .flat_map(|dy| (0..ww).map(move |dx| (y + dy) * w + x + dx))
                .collect();
            let rows: Vec<_> = pixels
                .iter()
                .map(|&i| {
                    [
                        f64::from(colors[i][0]),
                        f64::from(colors[i][1]),
                        f64::from(colors[i][2]),
                        1.,
                    ]
                })
                .collect();
            let mut gram = vec![vec![0.; 4]; 4];
            for row in &rows {
                for i in 0..4 {
                    for j in 0..4 {
                        gram[i][j] += row[i] * row[j];
                    }
                }
            }
            for i in 0..3 {
                gram[i][i] += epsilon;
            }
            for (j, row) in rows.iter().enumerate() {
                let fit = dense_solve(gram.clone(), row.to_vec());
                for (i, other) in rows.iter().enumerate() {
                    laplacian[pixels[i]][pixels[j]] += (if i == j { 1. } else { 0. })
                        - other.iter().zip(&fit).map(|(a, b)| a * b).sum::<f64>();
                }
            }
        }
    }
    let unknown: Vec<_> = (0..n).filter(|&i| trimap[i] == 128).collect();
    let matrix: Vec<Vec<_>> = unknown
        .iter()
        .map(|&i| unknown.iter().map(|&j| laplacian[i][j]).collect())
        .collect();
    let rhs: Vec<_> = unknown
        .iter()
        .map(|&i| {
            -(0..n)
                .filter(|&j| trimap[j] == 255)
                .map(|j| laplacian[i][j])
                .sum::<f64>()
        })
        .collect();
    let solved = dense_solve(matrix, rhs);
    let mut alpha: Vec<_> = trimap
        .iter()
        .map(|&v| if v == 255 { 1. } else { 0. })
        .collect();
    for (&i, &a) in unknown.iter().zip(&solved) {
        alpha[i] = a.clamp(0., 1.);
    }
    alpha
}

#[test]
fn agrees_with_independent_dense_color_intercept_elimination() {
    for (w, h) in [(6_usize, 4_usize), (1, 9), (2, 7)] {
        let mut colors = Vec::new();
        let mut trimap = Vec::new();
        for y in 0..h {
            for x in 0..w {
                let i = y * w + x;
                colors.push([
                    0.15 + 0.7 * i as f32 / (w * h) as f32,
                    0.2 + 0.1 * (i as f32 * 0.4).sin(),
                    0.7 - 0.3 * y as f32 / h as f32,
                ]);
                trimap.push(if i < w {
                    0
                } else if i >= (h - 1) * w {
                    255
                } else {
                    128
                });
            }
        }
        let params = ClosedFormParams {
            residual_rms_tolerance: 1e-9,
            ..ClosedFormParams::default()
        };
        let oracle = dense_oracle(&colors, w, h, &trimap, params.epsilon);
        for prior in [vec![0; w * h], vec![255; w * h]] {
            let result =
                estimate(&colors, w as u32, h as u32, &trimap, &prior, &params, None).unwrap();
            assert!(result.residual_rms <= params.residual_rms_tolerance);
            for (actual, expected) in result.alpha.as_raw().iter().zip(&oracle) {
                assert!(
                    actual.abs_diff((expected * 255.).round() as u8) <= 1,
                    "{w}x{h}: {actual} versus {expected}"
                );
            }
        }
    }
}

#[test]
fn local_color_model_recovers_soft_alpha_from_an_uninformative_prior() {
    let (w, h) = (41_usize, 5_usize);
    let foreground = [0.17, 0.32, 0.81];
    let background = [0.86, 0.72, 0.13];
    let mut colors = Vec::new();
    let mut truth = Vec::new();
    let mut trimap = Vec::new();
    for _ in 0..h {
        for x in 0..w {
            let alpha = ((x as f64 - 8.) / 24.).clamp(0., 1.);
            truth.push((alpha * 255.).round() as u8);
            colors.push(std::array::from_fn(|c| {
                (alpha * foreground[c] + (1. - alpha) * background[c]) as f32
            }));
            trimap.push(if x <= 8 {
                0
            } else if x >= 32 {
                255
            } else {
                128
            });
        }
    }
    let result = estimate(
        &colors,
        w as u32,
        h as u32,
        &trimap,
        &vec![128; w * h],
        &ClosedFormParams::default(),
        None,
    )
    .unwrap();
    assert!(result
        .alpha
        .as_raw()
        .iter()
        .zip(&truth)
        .all(|(a, b)| a.abs_diff(*b) <= 1));
    assert!(result.iterations > 0);
    assert_eq!(trimap[9], 128);
}

#[test]
fn known_labels_are_exact_and_an_all_known_input_needs_no_iterations() {
    let colors = vec![[0.5; 3]; 6];
    let trimap = [0, 255, 0, 255, 0, 255];
    let result = estimate(
        &colors,
        3,
        2,
        &trimap,
        &[73; 6],
        &ClosedFormParams::default(),
        None,
    )
    .unwrap();
    assert_eq!(result.alpha.as_raw(), &trimap);
    assert_eq!(result.iterations, 0);
    assert_eq!(result.unknown_pixels, 0);
}

#[test]
fn missing_foreground_evidence_and_nonconvergence_publish_no_matte() {
    let colors: Vec<_> = (0..25).map(|i| [i as f32 / 24.; 3]).collect();
    let mut trimap = vec![128; 25];
    trimap[0] = 0;
    let error = estimate(
        &colors,
        25,
        1,
        &trimap,
        &trimap,
        &ClosedFormParams::default(),
        None,
    )
    .err()
    .unwrap();
    assert!(error.contains("known foreground and background"));
    trimap[24] = 255;
    let params = ClosedFormParams {
        max_iterations: 1,
        residual_rms_tolerance: 1e-9,
        ..ClosedFormParams::default()
    };
    let error = estimate(&colors, 25, 1, &trimap, &[0; 25], &params, None)
        .err()
        .unwrap();
    assert!(
        error.contains("did not converge after 1 iterations"),
        "{error}"
    );
    assert!(error.contains("RMS residual") && error.contains("relative"));
}

#[test]
fn bad_labels_colors_dimensions_parameters_and_work_limits_refuse() {
    let colors = [[0.; 3], [0.5; 3], [1.; 3]];
    let p = ClosedFormParams::default();
    assert!(estimate(&colors, 3, 1, &[0, 127, 255], &[0; 3], &p, None).is_err());
    assert!(estimate(&colors, 2, 2, &[0, 128, 255], &[0; 3], &p, None).is_err());
    let mut invalid = colors;
    invalid[1][0] = f32::NAN;
    assert!(estimate(&invalid, 3, 1, &[0, 128, 255], &[0; 3], &p, None).is_err());
    let invalid = ClosedFormParams { epsilon: 0., ..p };
    assert!(estimate(&colors, 3, 1, &[0, 128, 255], &[0; 3], &invalid, None).is_err());
    let n = MAX_UNKNOWN_PIXELS + 1;
    let error = estimate(
        &vec![[0.5; 3]; n],
        n as u32,
        1,
        &vec![128; n],
        &vec![128; n],
        &p,
        None,
    )
    .err()
    .unwrap();
    assert!(error.contains("unknown pixels; limit"));
}

#[test]
fn memory_and_pre_cancel_refuse_and_release_reservations() {
    for cancelled in [false, true] {
        let token = DrJobControl::default();
        unsafe {
            assert!(dr_job_control_set_scratch_limit(&token, 1));
        }
        if cancelled {
            token.cancel();
        }
        let error = estimate(
            &[[0.; 3], [0.5; 3], [1.; 3]],
            3,
            1,
            &[0, 128, 255],
            &[0; 3],
            &ClosedFormParams::default(),
            Some(&token),
        )
        .err()
        .unwrap();
        assert!(error.contains(if cancelled {
            "cancelled"
        } else {
            "working memory"
        }));
        assert_eq!(token.scratch_bytes(), 0);
    }
}

#[test]
fn cancellation_during_iterative_solve_is_prompt_and_never_publishes() {
    let (w, h) = (400_usize, 300_usize);
    let mut colors = Vec::new();
    let mut trimap = Vec::new();
    for y in 0..h {
        for x in 0..w {
            let t = x as f32 / (w - 1) as f32;
            colors.push([
                0.1 + 0.7 * t,
                0.2 + 0.5 * t,
                0.3 + 0.4 * t + 0.03 * (y as f32 * 0.08).sin(),
            ]);
            trimap.push(if x == 0 {
                0
            } else if x == w - 1 {
                255
            } else {
                128
            });
        }
    }
    let prior = vec![0; w * h];
    let render = |token: usize| {
        let control = unsafe { &*(token as *const DrJobControl) };
        match estimate(
            &colors,
            w as u32,
            h as u32,
            &trimap,
            &prior,
            &ClosedFormParams::default(),
            Some(control),
        ) {
            Ok(_) => (true, String::new()),
            Err(error) => (false, error),
        }
    };
    let (entry, next) = entry_then_cancel(&|progress, _, _| progress >= 0.202, render);
    let n0 = entry.checkpoints;
    assert!(
        next.expect("the entry's kernel never reported again")
            .checkpoints
            > n0 + 3,
        "the entry's kernel polls fewer than three checkpoints before its next report"
    );
    let parked = cancelled_parked_at(
        n0 + 3,
        &|progress, stage| stage == entry.stage && progress == entry.progress,
        render,
    );
    eprintln!("closed-form solve cancellation: {}", parked_line(&parked));
    assert_eq!(prior[10], 0);
}

mod bench {
    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    #[ignore = "make cancel-bench: a wall-clock verdict"]
    fn cancellation_during_iterative_solve_is_prompt_and_never_publishes() {
        let (w, h) = (400_usize, 300_usize);
        let mut colors = Vec::new();
        let mut trimap = Vec::new();
        for y in 0..h {
            for x in 0..w {
                let t = x as f32 / (w - 1) as f32;
                colors.push([
                    0.1 + 0.7 * t,
                    0.2 + 0.5 * t,
                    0.3 + 0.4 * t + 0.03 * (y as f32 * 0.08).sin(),
                ]);
                trimap.push(if x == 0 {
                    0
                } else if x == w - 1 {
                    255
                } else {
                    128
                });
            }
        }
        let token = DrJobControl::default();
        let prior = vec![0; w * h];
        std::thread::scope(|scope| {
            let worker = scope.spawn(|| {
                estimate(
                    &colors,
                    w as u32,
                    h as u32,
                    &trimap,
                    &prior,
                    &ClosedFormParams::default(),
                    Some(&token),
                )
            });
            let deadline = Instant::now() + Duration::from_secs(10);
            while token.progress() < 0.202 {
                assert!(Instant::now() < deadline, "iterative solve did not start");
                std::thread::sleep(Duration::from_millis(1));
            }
            let start = Instant::now();
            token.cancel();
            let error = worker.join().unwrap().err().unwrap();
            assert!(error.contains("cancelled"), "{error}");
            eprintln!(
                "closed-form solve cancellation: {:.3} ms",
                start.elapsed().as_secs_f64() * 1000.
            );
            assert!(start.elapsed() < Duration::from_millis(500));
        });
        assert_eq!(token.scratch_bytes(), 0);
        assert_eq!(prior[10], 0);
    }
}
