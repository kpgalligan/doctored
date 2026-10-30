//! Independent observed-color equations, support semantics and atomic refusal.
use crate::common;
use common::probe::{cancelled_parked_at, entry_count, parked_line};
use doctored_core::bayesian_matting::{estimate, MAX_JOINT_EVALUATIONS};
const LIMIT: u64 = 2 * 1024 * 1024 * 1024;

fn scene() -> (Vec<[f32; 3]>, Vec<u8>) {
    let colors = (0..72)
        .map(|i| {
            let a = if i < 32 {
                0.0
            } else if i >= 40 {
                1.0
            } else {
                (i - 31) as f32 / 10.0
            };
            [a, 0.25 + 0.5 * a, 0.1 + 0.7 * a]
        })
        .collect();
    let prior = (0..72).map(|i| if i < 36 { 0 } else { 255 }).collect();
    (colors, prior)
}

#[test]
fn joint_source_color_recovers_automatic_zero_detail_and_keeps_inputs() {
    let (colors, prior) = scene();
    let original = colors.clone();
    let labels = vec![128; 72];
    let out = estimate(&colors, (72, 1), &prior, &labels, LIMIT, None).unwrap();
    assert_eq!(colors, original);
    assert_eq!(labels, vec![128; 72]);
    assert_eq!(prior[32], 0);
    assert!(out.alpha.as_raw()[32] > 16);
    for i in 32..40 {
        let truth = (i - 31) as f64 / 10.0;
        assert!((f64::from(out.alpha.as_raw()[i]) / 255.0 - truth).abs() < 0.015);
        for (c, expected) in [1.0, 0.75, 0.8].into_iter().enumerate() {
            assert!((f64::from(out.foreground[i][c]) - expected).abs() < 0.015);
        }
    }
    assert!(out.joint_evaluations <= MAX_JOINT_EVALUATIONS);
    assert!(out.spatial_node_visits > 0);
}

#[test]
fn correlated_weighted_source_distributions_remain_exactly_symmetric() {
    let (mut colors, prior) = scene();
    for (i, p) in colors.iter_mut().enumerate() {
        if !(28..=44).contains(&i) {
            let d = (i % 7) as f32 / 255.0;
            p[0] = (p[0] - d).clamp(0.0, 1.0);
            p[1] = (p[1] + 2.0 * d).clamp(0.0, 1.0);
            p[2] = (p[2] - 3.0 * d).clamp(0.0, 1.0);
        }
    }
    let out = estimate(&colors, (72, 1), &prior, &[128; 72], LIMIT, None).unwrap();
    assert_eq!(out.foreground.len(), 72);
    assert!(out.foreground.iter().flatten().all(|v| v.is_finite()));
}

#[test]
fn explicit_labels_override_uncertain_prior_and_known_rgb_stays_observed() {
    let (colors, prior) = scene();
    let mut labels = vec![128; 72];
    labels[33] = 255;
    labels[39] = 0;
    let out = estimate(&colors, (72, 1), &prior, &labels, LIMIT, None).unwrap();
    assert_eq!(out.alpha.as_raw()[33], 255);
    assert_eq!(out.alpha.as_raw()[39], 0);
    assert_eq!(out.foreground[33], colors[33]);
    assert_eq!(out.foreground[39], colors[39]);
}

#[test]
fn fully_explicit_state_needs_no_invented_color_populations() {
    let colors = vec![[0.2, 0.3, 0.4]; 8];
    let prior = vec![128; 8];
    let labels = vec![0, 255, 0, 255, 0, 255, 0, 255];
    let out = estimate(&colors, (8, 1), &prior, &labels, LIMIT, None).unwrap();
    assert_eq!(out.alpha.as_raw(), &labels);
    assert_eq!(out.foreground, colors);
    assert_eq!(out.joint_evaluations, 0);
    assert_eq!(out.spatial_node_visits, 0);
}

#[test]
fn unsupported_observations_budgets_and_missing_classes_refuse() {
    let (mut colors, prior) = scene();
    let labels = vec![128; 72];
    assert!(estimate(&colors, (72, 1), &prior, &labels, 1, None).is_err());
    assert!(estimate(&colors, (71, 1), &prior, &labels, LIMIT, None).is_err());
    assert!(estimate(&colors, (72, 1), &prior, &[17; 72], LIMIT, None).is_err());
    assert!(estimate(&colors, (72, 1), &[64; 72], &labels, LIMIT, None)
        .err()
        .unwrap()
        .contains("lacks source-derived color support"));
    colors[40][0] = f32::NAN;
    assert!(estimate(&colors, (72, 1), &prior, &labels, LIMIT, None).is_err());
}

#[test]
fn cancelled_unknown_and_fully_known_calls_remain_pure_and_release_scratch() {
    use doctored_core::job_control::DrJobControl;
    let (colors, prior) = scene();
    for labels in [vec![128; 72], vec![255; 72]] {
        let control = DrJobControl::default();
        control.cancel();
        let error = estimate(&colors, (72, 1), &prior, &labels, LIMIT, Some(&control))
            .err()
            .unwrap();
        assert!(error.contains("cancelled"));
        assert_eq!(control.scratch_bytes(), 0);
    }
    let control = DrJobControl::default();
    estimate(&colors, (72, 1), &prior, &[255; 72], LIMIT, Some(&control)).unwrap();
    assert_eq!(control.progress(), 1.0);
    assert_eq!(control.scratch_bytes(), 0);
}

/// One controlled `estimate` over borrowed inputs, for the parked probes.
fn render<'a>(
    colors: &'a [[f32; 3]],
    size: (u32, u32),
    prior: &'a [u8],
    labels: &'a [u8],
) -> impl FnOnce(usize) -> (bool, String) + Send + 'a {
    move |token| {
        let control = unsafe { &*(token as *const doctored_core::job_control::DrJobControl) };
        match estimate(colors, size, prior, labels, LIMIT, Some(control)) {
            Ok(_) => (true, String::new()),
            Err(e) => (false, e),
        }
    }
}

#[test]
fn live_cancellation_stops_both_joint_work_and_fully_explicit_copy() {
    for known in [false, true] {
        let (line, prior_line) = scene();
        let height = if known { 16384 } else { 1024 };
        let size = (72, height);
        let colors: Vec<_> = (0..height).flat_map(|_| line.iter().copied()).collect();
        let prior: Vec<_> = (0..height)
            .flat_map(|_| prior_line.iter().copied())
            .collect();
        let labels = vec![if known { 255 } else { 128 }; colors.len()];
        let parked = if known {
            // The fully explicit copy: its closing report is the last
            // checkpoint, so the one before it is the copy loop's last `every`.
            let f = entry_count(
                &|_, stage, _| stage == 5,
                render(&colors, size, &prior, &labels),
            );
            cancelled_parked_at(
                f - 1,
                &|_, s| s == 1,
                render(&colors, size, &prior, &labels),
            )
        } else {
            // The joint work: one checkpoint past its first report at 0.11.
            let n0 = entry_count(
                &|p, stage, _| stage == 6 && p >= 0.11,
                render(&colors, size, &prior, &labels),
            );
            cancelled_parked_at(
                n0 + 1,
                &|p, s| s == 6 && (0.11..1.0).contains(&p),
                render(&colors, size, &prior, &labels),
            )
        };
        eprintln!("known {known}: {}", parked_line(&parked));
        assert!(
            parked.scratch > 0,
            "parked before the working reservation was live"
        );
        assert!(colors.iter().enumerate().all(|(i, c)| *c == line[i % 72]));
        assert!(prior
            .iter()
            .enumerate()
            .all(|(i, a)| *a == prior_line[i % 72]));
        assert!(labels.iter().all(|&v| v == if known { 255 } else { 128 }));
    }
}
