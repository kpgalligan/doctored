//! Independent alpha-compositing oracles for the unexposed sampling candidate.
use crate::common;
use common::probe::{cancelled_parked_at, entry_then_cancel, parked_line};
use doctored_core::{
    ffi_jobs::dr_job_control_set_scratch_limit,
    job_control::DrJobControl,
    sampled_matting::{estimate, MattingParams, MAX_UNKNOWN_PIXELS},
};

fn mixture(f: [f32; 3], b: [f32; 3], alpha: f32) -> [f32; 3] {
    std::array::from_fn(|channel| alpha * f[channel] + (1.0 - alpha) * b[channel])
}

#[test]
fn explicit_foreground_background_recovers_partial_alpha_and_pins_constraints() {
    let foreground = [0.14, 0.31, 0.82];
    let background = [0.91, 0.83, 0.42];
    let truth = [0.0, 0.03, 0.16, 0.34, 0.7, 0.97, 1.0];
    let colors: Vec<_> = truth
        .iter()
        .map(|a| mixture(foreground, background, *a))
        .collect();
    let trimap = [0, 128, 128, 128, 128, 128, 255];
    // Known constraints must win over contradictory prior bytes. The unknown
    // prior contains no useful alpha information, including partial glass-like
    // coverages0.16/0.34/0.7; supplied pure color evidence makes them solvable.
    let prior = [255, 255, 255, 0, 0, 0, 0];
    let result = estimate(
        &colors,
        7,
        1,
        &trimap,
        &prior,
        &MattingParams::default(),
        None,
    )
    .unwrap();
    for (i, alpha) in truth.iter().enumerate() {
        assert!(result.alpha.as_raw()[i].abs_diff((alpha * 255.0_f32).round() as u8) <= 1);
    }
    assert_eq!(result.unresolved_pixels, 0);
    for i in 1..6 {
        assert_eq!(result.foreground[i], foreground);
        assert_eq!(result.background[i], background);
        assert!(result.confidence.as_raw()[i] >= 254);
    }
    assert_eq!(prior, [255, 255, 255, 0, 0, 0, 0]);
}

#[test]
fn translucent_object_without_foreground_evidence_refuses() {
    let colors = [[0.2, 0.3, 0.4], [0.4, 0.4, 0.4], [0.5, 0.5, 0.5]];
    let error = estimate(
        &colors,
        3,
        1,
        &[0, 128, 128],
        &[0, 60, 120],
        &MattingParams::default(),
        None,
    )
    .err()
    .unwrap();
    assert!(error.contains("known foreground and background"));
}

#[test]
fn indistinguishable_colors_and_off_line_colors_preserve_prior_with_zero_confidence() {
    let prior = [0, 73, 255];
    for colors in [
        [[0.4, 0.4, 0.4]; 3],
        [[0.0, 0.0, 0.0], [0.0, 0.8, 0.0], [1.0, 1.0, 1.0]],
    ] {
        let result = estimate(
            &colors,
            3,
            1,
            &[0, 128, 255],
            &prior,
            &MattingParams::default(),
            None,
        )
        .unwrap();
        assert_eq!(result.alpha.as_raw(), &prior);
        assert_eq!(result.confidence.as_raw()[1], 0);
        assert_eq!(result.unresolved_pixels, 1);
    }
}

#[test]
fn global_color_sample_can_explain_a_pixel_when_nearest_foreground_cannot() {
    let (w, h) = (60_usize, 20_usize);
    let mut colors = vec![[0.0, 0.0, 1.0]; w * h];
    let mut trimap = vec![128; w * h];
    let prior = vec![128; w * h];
    for x in 0..w {
        trimap[x] = 255;
        colors[x] = if x >= 50 {
            [1.0, 1.0, 1.0]
        } else {
            [1.0, 0.0, 0.0]
        };
        trimap[(h - 1) * w + x] = 0;
    }
    let target = 2 * w + 5;
    colors[target] = [0.5, 0.5, 1.0];
    let result = estimate(
        &colors,
        w as u32,
        h as u32,
        &trimap,
        &prior,
        &MattingParams::default(),
        None,
    )
    .unwrap();
    assert_eq!(result.alpha.as_raw()[target], 128);
    assert_eq!(result.foreground[target], [1.0, 1.0, 1.0]);
    assert_eq!(result.background[target], [0.0, 0.0, 1.0]);
    assert!(result.confidence.as_raw()[target] > 0);
}

#[test]
fn spatial_ranking_uses_nearest_boundary_samples_when_color_lines_are_equivalent() {
    let w = 31_usize;
    let mut colors = vec![[0.0; 3]; w * 3];
    let mut trimap = vec![128; w * 3];
    for x in 0..w {
        let value = 0.25 + x as f32 * 0.02;
        colors[x] = [value; 3];
        colors[w + x] = [value * 0.5; 3];
        trimap[x] = 255;
        trimap[2 * w + x] = 0;
    }
    let result = estimate(
        &colors,
        w as u32,
        3,
        &trimap,
        &vec![0; w * 3],
        &MattingParams::default(),
        None,
    )
    .unwrap();
    for (x, color) in colors.iter().take(w).enumerate() {
        assert!(result.alpha.as_raw()[w + x].abs_diff(128) <= 1);
        assert_eq!(result.foreground[w + x], *color);
    }
}

#[test]
fn malformed_constraints_parameters_and_excess_work_refuse_without_panics() {
    let colors = [[0.0; 3], [0.5; 3], [1.0; 3]];
    let params = MattingParams::default();
    for trimap in [[0, 127, 255], [0, 129, 255]] {
        assert!(estimate(&colors, 3, 1, &trimap, &[0; 3], &params, None).is_err());
    }
    assert!(estimate(&colors, 2, 2, &[0, 128, 255], &[0; 3], &params, None).is_err());
    assert!(estimate(&colors, 3, 1, &[0, 128, 255], &[], &params, None).is_err());
    let mut invalid = params;
    invalid.minimum_separation = f32::NAN;
    assert!(estimate(&colors, 3, 1, &[0, 128, 255], &[0; 3], &invalid, None).is_err());
    let mut invalid_colors = colors;
    invalid_colors[1][0] = f32::INFINITY;
    assert!(estimate(
        &invalid_colors,
        3,
        1,
        &[0, 128, 255],
        &[0; 3],
        &params,
        None
    )
    .is_err());
    for value in [-0.01, 1.01] {
        invalid_colors[1][0] = value;
        assert!(estimate(
            &invalid_colors,
            3,
            1,
            &[0, 128, 255],
            &[0; 3],
            &params,
            None,
        )
        .is_err());
    }
    let colors = vec![[0.0; 3]; MAX_UNKNOWN_PIXELS + 1];
    let labels = vec![128; colors.len()];
    let error = estimate(
        &colors,
        colors.len() as u32,
        1,
        &labels,
        &labels,
        &params,
        None,
    )
    .err()
    .unwrap();
    assert!(error.contains("unknown pixels; limit"));
}

#[test]
fn excessive_boundary_samples_refuse_before_scratch_is_reserved() {
    let w = 800_usize;
    let colors = vec![[0.5; 3]; w * w];
    let trimap: Vec<u8> = (0..w * w)
        .map(|i| {
            if i % 2 == 0 {
                128
            } else if i < w * w / 2 {
                255
            } else {
                0
            }
        })
        .collect();
    let token = DrJobControl::default();
    let error = estimate(
        &colors,
        w as u32,
        w as u32,
        &trimap,
        &trimap,
        &MattingParams::default(),
        Some(&token),
    )
    .err()
    .unwrap();
    assert!(error.contains("boundary exceeds"));
    assert_eq!(token.scratch_bytes(), 0);
}

#[test]
fn memory_admission_and_pre_cancel_release_scratch() {
    let colors = [[0.0; 3], [0.5; 3], [1.0; 3]];
    let trimap = [0, 128, 255];
    let token = DrJobControl::default();
    unsafe {
        assert!(dr_job_control_set_scratch_limit(&token, 1));
    }
    assert!(estimate(
        &colors,
        3,
        1,
        &trimap,
        &trimap,
        &MattingParams::default(),
        Some(&token)
    )
    .err()
    .unwrap()
    .contains("working memory"));
    assert_eq!(token.scratch_bytes(), 0);
    let token = DrJobControl::default();
    token.cancel();
    assert!(estimate(
        &colors,
        3,
        1,
        &trimap,
        &trimap,
        &MattingParams::default(),
        Some(&token)
    )
    .err()
    .unwrap()
    .contains("cancelled"));
    assert_eq!(token.scratch_bytes(), 0);
}

#[test]
fn cancellation_during_sample_search_returns_promptly_and_preserves_inputs() {
    let (w, h) = (600_usize, 500_usize);
    let mut colors = vec![[0.6, 0.4, 0.2]; w * h];
    let mut trimap = vec![128; w * h];
    for x in 0..w {
        colors[x] = [0.9, 0.8, 0.7];
        colors[(h - 1) * w + x] = [0.1, 0.05, 0.0];
        trimap[x] = 255;
        trimap[(h - 1) * w + x] = 0;
    }
    let render = |token: usize| {
        let control = unsafe { &*(token as *const DrJobControl) };
        match estimate(
            &colors,
            w as u32,
            h as u32,
            &trimap,
            &trimap,
            &MattingParams::default(),
            Some(control),
        ) {
            Ok(_) => (true, String::new()),
            Err(error) => (false, error),
        }
    };
    let (entry, next) = entry_then_cancel(&|progress, _, _| progress >= 0.2, render);
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
    eprintln!("sample search cancellation: {}", parked_line(&parked));
    assert_eq!(trimap[w], 128);
    assert_eq!(colors[w], [0.6, 0.4, 0.2]);
}

#[test]
fn collinear_boundary_tree_search_has_internal_cancellation_checkpoints() {
    let (w, h) = (5_usize, 40_000_usize);
    let mut colors = vec![[0.5; 3]; w * h];
    let mut trimap = vec![128; w * h];
    for y in 0..h {
        colors[y * w] = [1.0; 3];
        colors[y * w + w - 1] = [0.0; 3];
        trimap[y * w] = 255;
        trimap[y * w + w - 1] = 0;
    }
    let render = |token: usize| {
        let control = unsafe { &*(token as *const DrJobControl) };
        match estimate(
            &colors,
            w as u32,
            h as u32,
            &trimap,
            &trimap,
            &MattingParams::default(),
            Some(control),
        ) {
            Ok(_) => (true, String::new()),
            Err(error) => (false, error),
        }
    };
    let (entry, next) = entry_then_cancel(&|progress, _, _| progress >= 0.151, render);
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
    eprintln!(
        "collinear tree search cancellation: {}",
        parked_line(&parked)
    );
    assert_eq!(trimap[1], 128);
}

mod bench {
    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    #[ignore = "make cancel-bench: a wall-clock verdict"]
    fn cancellation_during_sample_search_returns_promptly_and_preserves_inputs() {
        let (w, h) = (600_usize, 500_usize);
        let mut colors = vec![[0.6, 0.4, 0.2]; w * h];
        let mut trimap = vec![128; w * h];
        for x in 0..w {
            colors[x] = [0.9, 0.8, 0.7];
            colors[(h - 1) * w + x] = [0.1, 0.05, 0.0];
            trimap[x] = 255;
            trimap[(h - 1) * w + x] = 0;
        }
        let token = DrJobControl::default();
        std::thread::scope(|scope| {
            let worker = scope.spawn(|| {
                estimate(
                    &colors,
                    w as u32,
                    h as u32,
                    &trimap,
                    &trimap,
                    &MattingParams::default(),
                    Some(&token),
                )
            });
            let deadline = Instant::now() + Duration::from_secs(10);
            while token.progress() < 0.2 {
                assert!(Instant::now() < deadline, "worker did not begin search");
                std::thread::sleep(Duration::from_millis(1));
            }
            let start = Instant::now();
            token.cancel();
            assert!(worker.join().unwrap().err().unwrap().contains("cancelled"));
            eprintln!(
                "sample search cancellation: {:.3} ms",
                start.elapsed().as_secs_f64() * 1000.0
            );
            assert!(start.elapsed() < Duration::from_millis(500));
        });
        assert_eq!(token.scratch_bytes(), 0);
        assert_eq!(trimap[w], 128);
        assert_eq!(colors[w], [0.6, 0.4, 0.2]);
    }

    #[test]
    #[ignore = "make cancel-bench: a wall-clock verdict"]
    fn collinear_boundary_tree_search_has_internal_cancellation_checkpoints() {
        let (w, h) = (5_usize, 40_000_usize);
        let mut colors = vec![[0.5; 3]; w * h];
        let mut trimap = vec![128; w * h];
        for y in 0..h {
            colors[y * w] = [1.0; 3];
            colors[y * w + w - 1] = [0.0; 3];
            trimap[y * w] = 255;
            trimap[y * w + w - 1] = 0;
        }
        let token = DrJobControl::default();
        std::thread::scope(|scope| {
            let worker = scope.spawn(|| {
                estimate(
                    &colors,
                    w as u32,
                    h as u32,
                    &trimap,
                    &trimap,
                    &MattingParams::default(),
                    Some(&token),
                )
            });
            let deadline = Instant::now() + Duration::from_secs(10);
            while token.progress() < 0.151 {
                assert!(
                    Instant::now() < deadline,
                    "worker did not begin collinear searches"
                );
                std::thread::sleep(Duration::from_millis(1));
            }
            let start = Instant::now();
            token.cancel();
            assert!(worker.join().unwrap().err().unwrap().contains("cancelled"));
            eprintln!(
                "collinear tree search cancellation: {:.3} ms",
                start.elapsed().as_secs_f64() * 1000.0
            );
            assert!(start.elapsed() < Duration::from_millis(500));
        });
        assert_eq!(token.scratch_bytes(), 0);
        assert_eq!(trimap[1], 128);
    }
}
