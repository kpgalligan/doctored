//! Native extent, exact user constraints, admission and cancellation checks.
use crate::common;
use common::probe::{entry_then_cancel, parked_line, Parked, Released};
use doctored_core::{
    closed_form_matting::ClosedFormParams, ffi_jobs::dr_job_control_set_scratch_limit,
    job_control::DrJobControl, matting_input::SoftPriorInput, tiled_soft_matting::estimate,
};
use std::time::{Duration, Instant};

fn input(labels: Option<&[u8]>) -> SoftPriorInput<'_> {
    SoftPriorInput {
        version: 1,
        user_constraints: labels,
        prior_weight: 1.0,
    }
}

#[test]
fn constant_fractional_prior_is_exact_across_tiles_and_partial_last_tile() {
    // L annihilates constants independently of image colors. The positive
    // prior term therefore has the unique constant minimum at every pixel.
    let (w, h) = (517, 259);
    let n = w * h;
    let colors: Vec<_> = (0..n)
        .map(|i| [((i * 31) % 256) as f32 / 255.0; 3])
        .collect();
    let result = estimate(
        &colors,
        (w as u32, h as u32),
        &vec![73; n],
        input(None),
        &ClosedFormParams::default(),
        None,
    )
    .unwrap();
    assert_eq!(result.alpha.dimensions(), (517, 259));
    assert_eq!(result.tile_count, 6);
    assert!(result.alpha.as_raw().iter().all(|&a| a == 73));
    assert!(result.estimated_stencil_terms <= 20_000_000_000);
    assert!(result.processed_tile_pixels > n as u64);
}

#[test]
fn labels_on_tile_seams_and_outer_boundaries_remain_exact() {
    let (w, h) = (519, 3);
    let n = w * h;
    let mut labels = vec![128; n];
    for (i, value) in [
        (0, 255),
        (255, 0),
        (256, 255),
        (511, 0),
        (512, 255),
        (n - 1, 0),
    ] {
        labels[i] = value;
    }
    let result = estimate(
        &vec![[0.5; 3]; n],
        (w as u32, h as u32),
        &vec![99; n],
        input(Some(&labels)),
        &ClosedFormParams::default(),
        None,
    )
    .unwrap();
    for (i, &label) in labels.iter().enumerate() {
        if label != 128 {
            assert_eq!(result.alpha.as_raw()[i], label);
        }
    }
}

#[test]
fn incomplete_local_solve_and_invalid_limits_return_no_output() {
    let colors: Vec<_> = (0..600).map(|i| [(i % 97) as f32 / 97.0; 3]).collect();
    let prior: Vec<_> = (0..600).map(|i| (i % 256) as u8).collect();
    let params = ClosedFormParams {
        max_iterations: 1,
        residual_rms_tolerance: 1e-9,
        ..ClosedFormParams::default()
    };
    let error = estimate(&colors, (300, 2), &prior, input(None), &params, None)
        .err()
        .unwrap();
    assert!(error.contains("tile 0") && error.contains("did not converge"));
    let invalid = ClosedFormParams {
        max_iterations: 5000,
        ..params
    };
    assert!(estimate(&colors, (300, 2), &prior, input(None), &invalid, None).is_err());
    for cancel in [false, true] {
        let token = DrJobControl::default();
        unsafe {
            assert!(dr_job_control_set_scratch_limit(&token, 1));
        }
        if cancel {
            token.cancel();
        }
        assert!(estimate(
            &colors,
            (300, 2),
            &prior,
            input(None),
            &ClosedFormParams::default(),
            Some(&token)
        )
        .is_err());
        assert_eq!(token.scratch_bytes(), 0);
    }
}

#[test]
fn cancellation_after_local_work_starts_never_publishes_partial_tiles() {
    let (w, h) = (513, 513);
    let n = w * h;
    let colors: Vec<_> = (0..n).map(|i| [(i % 97) as f32 / 97.0; 3]).collect();
    let prior: Vec<_> = (0..n).map(|i| (i % 256) as u8).collect();
    let render = |token: usize| {
        let control = unsafe { &*(token as *const DrJobControl) };
        match estimate(
            &colors,
            (w as u32, h as u32),
            &prior,
            input(None),
            &ClosedFormParams::default(),
            Some(control),
        ) {
            Ok(_) => (true, String::new()),
            Err(error) => (false, error),
        }
    };
    let (entry, next) = entry_then_cancel(&|progress, _, _| progress >= 0.01, render);
    let n0 = entry.checkpoints;
    assert!(
        next.expect("the entry's kernel never reported again")
            .checkpoints
            > n0 + 3,
        "the entry's kernel polls fewer than three checkpoints before its next report"
    );
    // The error names the refused tile ("tiled matting tile 0 at 0,0 refused:
    // operation cancelled"), so the park runs on the seam directly.
    let control = DrJobControl::default();
    control.park_at(n0 + 3);
    let parked = std::thread::scope(|scope| {
        let worker = scope.spawn(|| {
            let out = estimate(
                &colors,
                (w as u32, h as u32),
                &prior,
                input(None),
                &ClosedFormParams::default(),
                Some(&control),
            );
            let returned = Instant::now();
            control.release();
            (out.err(), returned)
        });
        let _guard = Released(&control);
        assert!(
            control.wait_parked(Duration::from_secs(240)),
            "the worker did not park at {}",
            n0 + 3
        );
        let (count, progress, stage, scratch) = (
            control.checkpoints(),
            control.progress(),
            control.stage(),
            control.scratch_bytes(),
        );
        let at = Instant::now();
        control.cancel();
        control.release();
        let (error, returned) = worker.join().unwrap();
        let error = error.expect("the solve completed after a parked cancel");
        assert!(error.contains("cancelled"), "{error}");
        assert_eq!(count, n0 + 3, "the worker parked at the wrong checkpoint");
        assert!(
            stage == entry.stage && progress == entry.progress,
            "parked at checkpoint {count}: progress {progress}, stage {stage} left the entry's kernel"
        );
        Parked {
            count,
            progress,
            stage,
            scratch,
            latency: returned.saturating_duration_since(at),
        }
    });
    assert_eq!(control.scratch_bytes(), 0);
    eprintln!("tiled local solve cancellation: {}", parked_line(&parked));
    assert_eq!(prior[255], 255);
}

mod bench {
    use super::*;

    #[test]
    #[ignore = "make cancel-bench: a wall-clock verdict"]
    fn cancellation_after_local_work_starts_never_publishes_partial_tiles() {
        use std::time::{Duration, Instant};
        let (w, h) = (513, 513);
        let n = w * h;
        let colors: Vec<_> = (0..n).map(|i| [(i % 97) as f32 / 97.0; 3]).collect();
        let prior: Vec<_> = (0..n).map(|i| (i % 256) as u8).collect();
        let token = DrJobControl::default();
        std::thread::scope(|scope| {
            let worker = scope.spawn(|| {
                estimate(
                    &colors,
                    (w as u32, h as u32),
                    &prior,
                    input(None),
                    &ClosedFormParams::default(),
                    Some(&token),
                )
            });
            let deadline = Instant::now() + Duration::from_secs(10);
            while token.progress() < 0.01 {
                assert!(Instant::now() < deadline, "local solve did not start");
                std::thread::sleep(Duration::from_millis(1));
            }
            let start = Instant::now();
            token.cancel();
            let error = worker.join().unwrap().err().unwrap();
            assert!(error.contains("cancelled"));
            assert!(start.elapsed() < Duration::from_millis(500));
        });
        assert_eq!(token.scratch_bytes(), 0);
        assert_eq!(prior[255], 255);
    }
}
