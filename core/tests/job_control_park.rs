//! Phase 13A-5b-iii-b-3: the black-box contract of the test-only blocking
//! checkpoint seam on `DrJobControl` — `park_at` / `parked` / `wait_parked` /
//! `release` and the report recorder `record_reports` / `reports` — which the
//! default count-triggered cancel tests of the regional suites use instead of
//! a spinning poller. Every failure path must fail FAST: `wait_parked` wakes
//! on the park, on a release or at its timeout, and every test whose worker
//! may be parked when an assertion runs holds a `Released` guard (cancel then
//! release on drop), so a failed assertion never hangs the join.

use std::ffi::c_char;
use std::ptr;
use std::thread;
use std::time::{Duration, Instant};

use doctored_core::doc::DrDocument;
use doctored_core::ffi::*;
use doctored_core::ffi_doc::*;
use doctored_core::ffi_regional::*;
use doctored_core::job_control::{DrJobControl, Report};
use image::{Rgba, RgbaImage};

use crate::common;
use common::probe::*;
use common::take_err_string;

const N: u64 = 40;
const M: u64 = 25;
const WAIT: Duration = Duration::from_secs(240);

/// `count` checkpoints on `control`, stopping at the first cancellation.
fn checkpoints(control: &DrJobControl, count: u64) -> Result<(), String> {
    for _ in 0..count {
        control.checkpoint()?;
    }
    Ok(())
}

/// A 64×64 Background under one more raster.
fn two_rasters() -> DrDocument {
    DrDocument::from_pixels(RgbaImage::from_fn(64, 64, |x, y| {
        Rgba([(x * 4) as u8, (y * 4) as u8, 90, 255])
    }))
    .adding_image_layer(
        0,
        RgbaImage::from_fn(64, 64, |x, y| Rgba([200, (x ^ y) as u8, 30, (x * 3) as u8])),
        "Top",
    )
    .expect("add layer")
}

/// The whole-canvas regional render of a clone of `doc` on `token`.
fn whole(doc: &DrDocument) -> impl FnOnce(usize) -> (bool, String) + Send {
    let address = Box::into_raw(Box::new(doc.clone())) as usize;
    move |token: usize| {
        let mut err: *mut c_char = ptr::null_mut();
        let out = unsafe {
            dr_doc_render_region(
                address as *const DrDocument,
                0,
                0,
                64,
                64,
                1,
                u64::MAX,
                token as *const DrJobControl,
                &mut err,
            )
        };
        unsafe { dr_doc_free(address as *mut DrDocument) };
        let ok = !out.is_null();
        if ok {
            unsafe { dr_image_free(out) };
        }
        (ok, take_err_string(err))
    }
}

#[test]
fn a_worker_parks_exactly_at_the_armed_count() {
    let control = DrJobControl::default();
    control.park_at(N);
    thread::scope(|scope| {
        let worker = scope.spawn(|| checkpoints(&control, N + M));
        let _guard = Released(&control);
        assert!(control.wait_parked(WAIT), "the worker did not park at {N}");
        assert!(control.parked());
        assert_eq!(control.checkpoints(), N, "parked at the wrong checkpoint");
        assert!(!worker.is_finished(), "a parked worker returned");
        control.cancel();
        control.release();
        assert_eq!(worker.join().unwrap(), Err("operation cancelled".into()));
    });
    assert_eq!(
        control.checkpoints(),
        N,
        "the cancel was seen at the parked checkpoint"
    );
}

#[test]
fn a_release_before_the_count_makes_the_park_fall_through() {
    let control = DrJobControl::default();
    control.park_at(N);
    control.release();
    assert_eq!(checkpoints(&control, N + M), Ok(()));
    assert!(control.parked(), "the armed checkpoint was reached");
    assert_eq!(control.checkpoints(), N + M);
}

#[test]
fn an_unarmed_control_never_parks() {
    for armed in [false, true] {
        let control = DrJobControl::default();
        if armed {
            control.park_at(0);
        }
        assert_eq!(checkpoints(&control, N + M), Ok(()));
        assert!(!control.parked());
        assert!(!control.wait_parked(Duration::from_millis(50)));
        assert_eq!(control.checkpoints(), N + M);
    }
}

#[test]
fn the_recorder_logs_every_report_of_a_regional_render() {
    let log = reports(whole(&two_rasters()));
    assert!(!log.is_empty(), "no report was recorded");
    assert_eq!(
        log[0],
        Report {
            checkpoints: 1,
            progress: 0.0,
            stage: 1
        },
        "the first report is begin()'s"
    );
    for pair in log.windows(2) {
        assert!(
            pair[1].checkpoints >= pair[0].checkpoints,
            "the count fell: {pair:?}"
        );
        assert!(
            pair[1].progress >= pair[0].progress,
            "progress fell: {pair:?}"
        );
    }
    let last = log.last().unwrap();
    assert_eq!(
        (last.progress, last.stage),
        (1.0, 5),
        "the last report is the close"
    );
}

#[test]
fn a_parked_then_cancelled_render_returns_nothing() {
    let p = cancelled_parked_at(2, &|_, _| true, whole(&two_rasters()));
    assert_eq!(p.count, 2);
    eprintln!("13A-5b-iii-b-3 parked regional render: {}", parked_line(&p));
}

#[test]
fn a_worker_that_never_reaches_the_count_wakes_the_waiter() {
    let control = DrJobControl::default();
    control.park_at(N);
    thread::scope(|scope| {
        let worker = scope.spawn(|| {
            let out = checkpoints(&control, M);
            control.release();
            out
        });
        let _guard = Released(&control);
        let deadline = Instant::now() + WAIT;
        assert!(
            !control.wait_parked(WAIT),
            "a worker short of the count parked"
        );
        assert!(
            Instant::now() < deadline,
            "the wait timed out instead of waking"
        );
        assert!(!control.parked());
        assert_eq!(worker.join().unwrap(), Ok(()));
    });
    assert_eq!(control.checkpoints(), M);
}

#[test]
fn dropping_the_guard_cancels_and_releases_a_parked_worker() {
    let control = DrJobControl::default();
    control.park_at(N);
    thread::scope(|scope| {
        let worker = scope.spawn(|| checkpoints(&control, N + M));
        let _guard = Released(&control);
        assert!(control.wait_parked(WAIT), "the worker did not park at {N}");
        drop(Released(&control));
        assert_eq!(worker.join().unwrap(), Err("operation cancelled".into()));
    });
    assert_eq!(control.checkpoints(), N);
}
