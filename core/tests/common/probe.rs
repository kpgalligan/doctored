//! The checkpoint-trigger cancellation probes the regional cancel suites
//! share (Phase 13A-5b-ii-b; moved out of
//! `regional_group_materialization_cancel.rs`, first written in
//! `regional_byte_preparation_cancel.rs`): the TRIGGER is the job's checkpoint
//! count (`DrJobControl::checkpoints`) or a freeze of it, never a calibrated
//! time. Nothing here reads a test file's constants — a canvas-dependent
//! predicate takes its dimensions as arguments. Since 13A-5b-iii-a it also
//! holds the freeze constant, the uncancelled CHECKPOINT TIMELINE the freeze
//! probes calibrate on, and the report-log trio (`logged`, `assert_interval`,
//! `assert_pin`) with `assert_ramp`, folded in from the preparation and
//! materialization suites. Since 13A-5b-iii-b-1 the suites partition by load
//! robustness: `probe`'s count triggers, `logged` with `assert_interval`
//! serve the default `make test`; the freeze probes, the timelines, `window`,
//! `assert_pin` (a ratio of two wall-clock readings, bench-only),
//! `assert_ramp` and every PASS/MISS line serve only the `#[ignore]`d tests
//! `make cancel-bench` runs. Since 13A-5b-iii-b-3 the
//! default count-triggered tests no longer race a poller: they take their
//! ENTRY COUNT from the worker's own report log (`entry_count`, over
//! `DrJobControl::record_reports`) and cancel with the worker PARKED at a
//! chosen checkpoint (`cancelled_parked_at`, over `DrJobControl::park_at`)
//! under a `Released` guard; `probe`, `Trigger`, `window`, `timeline` and
//! `logged` serve the bench, the default INTERVAL guards and (bench-only)
//! the PIN guards. Each regional suite's measured concurrent cancel has a
//! parked default twin, `cancelled_past_each`: the measuring body runs only
//! in its `#[ignore]`d twin.

use std::thread;
use std::time::{Duration, Instant};

use doctored_core::ffi_jobs::*;
use doctored_core::job_control::{DrJobControl, Report};

/// The model's RETURN FLOOR `T_f`.
pub const RETURN_FLOOR_S: f64 = 1e-3;

/// The model's unpolled unit: a freeze probe fires after 5 ms without a
/// checkpoint. The factor-8 and (m2) probes cap it at half the timeline's
/// longest gap (`FREEZE.min(longest / 2)`).
pub const FREEZE: Duration = Duration::from_millis(5);

/// Progress, stage and checkpoint count of the control behind `token`.
pub fn state(token: usize) -> (f64, u32, u64) {
    let control = unsafe { &*(token as *const DrJobControl) };
    (control.progress(), control.stage(), control.checkpoints())
}

/// When a probe cancels once its entry holds: `k` checkpoints later, or once
/// the count has not advanced for the given time (an unpolled unit).
#[derive(Clone, Copy)]
pub enum Trigger {
    After(u64),
    Freeze(Duration),
}

/// What a probe measured: when the cancel was issued (from the render's
/// start), the cancel-to-return latency (the return stamped by the worker),
/// the entry's checkpoint count `n0` and `k`, the worker's own progress,
/// stage and checkpoint count at its return, for a freeze probe how long the
/// count had been frozen when the cancel was issued, and `issued`, the count
/// read just before `dr_job_control_cancel`.
pub struct Probe {
    pub at: Duration,
    pub latency: Duration,
    pub n0: u64,
    pub k: u64,
    pub progress: f64,
    pub stage: u32,
    pub checkpoints: u64,
    pub frozen: Option<Duration>,
    pub issued: u64,
}

/// Runs `render`; a SPINNING poller waits until `entry(progress, stage,
/// checkpoints)` holds, reads `n0`, fires on `trigger` and cancels. The
/// render must return "operation cancelled", no image and no scratch, and the
/// worker's own state at return must satisfy `verdict(progress, stage)` with
/// `checkpoints ≥ n0 + k`; otherwise the poller fell behind the render and
/// the probe fails naming the stall.
pub fn probe<F>(
    entry: &dyn Fn(f64, u32, u64) -> bool,
    trigger: Trigger,
    verdict: &dyn Fn(f64, u32) -> bool,
    render: F,
) -> Probe
where
    F: FnOnce(usize) -> (bool, String) + Send,
{
    const STALL: &str = "the poller stalled past the stage";
    let control = unsafe { dr_job_control_create() };
    let token = control as usize;
    let out = thread::scope(|scope| {
        let start = Instant::now();
        let worker = scope.spawn(move || {
            let (ok, message) = render(token);
            (ok, message, Instant::now(), state(token))
        });
        let deadline = start + Duration::from_secs(240);
        loop {
            let (progress, stage, checkpoints) = state(token);
            if entry(progress, stage, checkpoints) {
                break;
            }
            assert!(!worker.is_finished(), "{STALL}: returned before the entry");
            assert!(Instant::now() < deadline, "the stage never started");
            std::hint::spin_loop();
        }
        let n0 = state(token).2;
        let (k, frozen) = match trigger {
            Trigger::After(k) => {
                while state(token).2 < n0 + k {
                    assert!(!worker.is_finished(), "{STALL}: returned before n0 + {k}");
                    assert!(Instant::now() < deadline, "the checkpoints stopped");
                    std::hint::spin_loop();
                }
                (k, None)
            }
            Trigger::Freeze(freeze) => {
                let (mut last, mut since) = (n0, Instant::now());
                let frozen = loop {
                    let n = state(token).2;
                    if n != last {
                        (last, since) = (n, Instant::now());
                    } else if since.elapsed() >= freeze {
                        break since.elapsed();
                    }
                    assert!(!worker.is_finished(), "{STALL}: returned before a freeze");
                    assert!(Instant::now() < deadline, "the render never froze");
                    std::hint::spin_loop();
                };
                (0, Some(frozen))
            }
        };
        let issued = state(token).2;
        let at = Instant::now();
        unsafe { dr_job_control_cancel(control) };
        let (ok, message, returned, (progress, stage, checkpoints)) = worker.join().unwrap();
        assert!(!ok, "{STALL}: the render completed before the cancel");
        assert_eq!(message, "operation cancelled");
        assert!(
            verdict(progress, stage) && checkpoints >= n0 + k,
            "{STALL}: at return progress {progress}, stage {stage}, {checkpoints} checkpoints \
             (entry {n0} + k {k})"
        );
        Probe {
            at: at - start,
            latency: returned.saturating_duration_since(at),
            n0,
            k,
            progress,
            stage,
            checkpoints,
            frozen,
            issued,
        }
    });
    assert_eq!(unsafe { dr_job_control_scratch_bytes(control) }, 0);
    unsafe { dr_job_control_free(control) };
    out
}

// ------------------------------------------------------- parked probes --
//
// The blocking checkpoint seam (13A-5b-iii-b-3): no poller races the worker.

/// Cancels and releases the control on drop: held by every test that can
/// panic while a worker is parked or about to park, so the join after an
/// unwind returns at the worker's next checkpoint (or its fall-through park)
/// instead of hanging. On the normal path the worker has already returned.
pub struct Released<'a>(pub &'a DrJobControl);

impl Drop for Released<'_> {
    fn drop(&mut self) {
        self.0.cancel();
        self.0.release();
    }
}

/// Releases the control on drop: held by `cancelled_parked_at`'s worker
/// across `render`, so a panicking render still wakes the waiter at
/// once instead of leaving it blocked for the full 240 s.
pub struct ReleaseOnDrop<'a>(pub &'a DrJobControl);

impl Drop for ReleaseOnDrop<'_> {
    fn drop(&mut self) {
        self.0.release();
    }
}

/// What a parked cancel saw: the checkpoint it parked at, the worker's own
/// progress and stage while parked, `scratch` — the live reservation
/// (`DrJobControl::scratch_bytes`) read while parked — and `latency` —
/// release to the worker's return — which is PRINTED only (load policy,
/// 13A-5b-iii-b-1).
pub struct Parked {
    pub count: u64,
    pub progress: f64,
    pub stage: u32,
    pub scratch: u64,
    pub latency: Duration,
}

/// The worker's own report log of one uncancelled run of `render` (under
/// `DrJobControl::record_reports`); the render must complete.
pub fn reports<F>(render: F) -> Vec<Report>
where
    F: FnOnce(usize) -> (bool, String) + Send,
{
    let control = unsafe { dr_job_control_create() };
    let token = control as usize;
    unsafe { &*control }.record_reports();
    let (ok, message) = render(token);
    let log = unsafe { &*control }.reports();
    unsafe { dr_job_control_free(control) };
    assert!(ok, "{message}");
    log
}

/// The checkpoint count of the FIRST logged report satisfying
/// `entry(progress, stage, checkpoints)` in an uncancelled run of `render`.
pub fn entry_count<F>(entry: &dyn Fn(f64, u32, u64) -> bool, render: F) -> u64
where
    F: FnOnce(usize) -> (bool, String) + Send,
{
    reports(render)
        .iter()
        .find(|r| entry(r.progress, r.stage, r.checkpoints))
        .map(|r| r.checkpoints)
        .unwrap_or_else(|| panic!("the entry never came"))
}

/// `entry_count`'s twin run, cut short: a watcher cancels the twin
/// once its report log holds a report AFTER the first one satisfying
/// `entry(progress, stage, checkpoints)`, so the render may return either
/// cancelled or completed. Returns that entry report and the first report
/// logged after it (`None` when the render ended first). Counts are
/// deterministic, so a late watcher costs only time.
pub fn entry_then_cancel<F>(
    entry: &dyn Fn(f64, u32, u64) -> bool,
    render: F,
) -> (Report, Option<Report>)
where
    F: FnOnce(usize) -> (bool, String) + Send,
{
    let control = unsafe { dr_job_control_create() };
    let token = control as usize;
    let seam = unsafe { &*control };
    seam.record_reports();
    let find = |log: &[Report]| {
        log.iter()
            .position(|r| entry(r.progress, r.stage, r.checkpoints))
    };
    let (ok, message) = thread::scope(|scope| {
        let worker = scope.spawn(move || render(token));
        let deadline = Instant::now() + Duration::from_secs(240);
        while !worker.is_finished() {
            let log = seam.reports();
            if find(&log).is_some_and(|i| i + 1 < log.len()) {
                break;
            }
            if Instant::now() >= deadline {
                seam.cancel();
                panic!("the twin neither reported past its entry nor returned in 240 s");
            }
            thread::sleep(Duration::from_millis(1));
        }
        seam.cancel();
        worker.join().unwrap()
    });
    let log = seam.reports();
    unsafe { dr_job_control_free(control) };
    assert!(ok || message.contains("cancel"), "{message}");
    let at = find(&log).unwrap_or_else(|| panic!("the entry never came"));
    (log[at], log.get(at + 1).copied())
}

/// Runs `render` with the worker PARKED at checkpoint `count` (its
/// post-increment count, before its cancellation test), reads the worker's
/// state while parked, cancels, releases and joins. The render must return
/// "operation cancelled", no image and no scratch; the parked state must
/// satisfy `verdict(progress, stage)` at exactly `count` checkpoints. A count
/// the render never reaches fails at once: the worker releases on return.
pub fn cancelled_parked_at<F>(count: u64, verdict: &dyn Fn(f64, u32) -> bool, render: F) -> Parked
where
    F: FnOnce(usize) -> (bool, String) + Send,
{
    let control = unsafe { dr_job_control_create() };
    let token = control as usize;
    let seam = unsafe { &*control };
    seam.park_at(count);
    let out = thread::scope(|scope| {
        let worker = scope.spawn(move || {
            let _release = ReleaseOnDrop(unsafe { &*(token as *const DrJobControl) });
            let out = render(token);
            let returned = Instant::now();
            unsafe { &*(token as *const DrJobControl) }.release();
            (out, returned, state(token))
        });
        let _guard = Released(seam);
        assert!(
            seam.wait_parked(Duration::from_secs(240)),
            "the worker did not park at {count}: it returned first (the count lies past the \
             render's last checkpoint) or 240 s elapsed"
        );
        let (progress, stage, checkpoints) = state(token);
        let scratch = seam.scratch_bytes();
        let at = Instant::now();
        unsafe { dr_job_control_cancel(control) };
        seam.release();
        let ((ok, message), returned, _) = worker.join().unwrap();
        assert!(
            !ok,
            "the render completed after a cancel at checkpoint {count}"
        );
        assert_eq!(message, "operation cancelled");
        assert!(
            verdict(progress, stage),
            "parked at checkpoint {count}: progress {progress}, stage {stage} fails the verdict"
        );
        assert_eq!(
            checkpoints, count,
            "the worker parked at the wrong checkpoint"
        );
        Parked {
            count,
            progress,
            stage,
            scratch,
            latency: returned.saturating_duration_since(at),
        }
    });
    assert_eq!(unsafe { dr_job_control_scratch_bytes(control) }, 0);
    unsafe { dr_job_control_free(control) };
    out
}

/// The load-independent default twin of a measured per-stage cancel.
/// One uncancelled `reports(twin)` — `twin` renders an independently built
/// copy of the fixture, never the document `render` reads, because a
/// completed render fills the shared plane caches a parked run would then
/// skip — gives, for each `open`, `n`, the checkpoint count of the first
/// report with progress above `open`; each `open` is then cancelled with
/// `render`'s worker PARKED at `n + 1` (`cancelled_parked_at`, verdict
/// progress above `open`). A run cancelled before a cached plane completes
/// caches nothing, so with ONE open `render` may read a shared document; with
/// several, a run parked in a later stage has completed the earlier stages'
/// planes, so `render` must build a fresh fixture per call (pass a
/// fresh-fixture `twin` as both). Results in `opens` order.
pub fn cancelled_past_each<T, F>(opens: &[f64], twin: T, render: F) -> Vec<Parked>
where
    T: FnOnce(usize) -> (bool, String) + Send,
    F: Fn(usize) -> (bool, String) + Send + Copy,
{
    let log = reports(twin);
    opens
        .iter()
        .map(|&open| {
            let n = log
                .iter()
                .find(|r| r.progress > open)
                .map(|r| r.checkpoints)
                .unwrap_or_else(|| panic!("the render never reported past {open}"));
            cancelled_parked_at(n + 1, &|p, _| p > open, render)
        })
        .collect()
}

/// The parked cancel's figures, for the printed line.
pub fn parked_line(p: &Parked) -> String {
    format!(
        "parked at checkpoint {} (progress {}, stage {}), released {:.3} ms → return",
        p.count,
        p.progress,
        p.stage,
        p.latency.as_secs_f64() * 1e3
    )
}

/// The live-signal probe (i): cancel `k` checkpoints after `entry`.
pub fn cancelled_after_checkpoints<F>(
    entry: &dyn Fn(f64, u32, u64) -> bool,
    k: u64,
    verdict: &dyn Fn(f64, u32) -> bool,
    render: F,
) -> Probe
where
    F: FnOnce(usize) -> (bool, String) + Send,
{
    probe(entry, Trigger::After(k), verdict, render)
}

/// The live-signal probe (ii): after `entry`, cancel once the checkpoint
/// count has not advanced for `freeze`.
pub fn cancelled_in_freeze<F>(
    entry: &dyn Fn(f64, u32, u64) -> bool,
    freeze: Duration,
    verdict: &dyn Fn(f64, u32) -> bool,
    render: F,
) -> Probe
where
    F: FnOnce(usize) -> (bool, String) + Send,
{
    probe(entry, Trigger::Freeze(freeze), verdict, render)
}

/// The flip into a preparation blur (13A-5b-iii-b-2): every driver report
/// during preparation — the unstaged fill's chunks and each stage range's
/// advance — carries stage 1 ("preparing"), and an inner blur's `feather`
/// rows report stage 6, so the FIRST stage-6 report is the first inner blur
/// report (the 1→6 flip, per stage; without a preparation blur, the loop's
/// first row). Inside `rendered_group` the first such report is a child's or
/// the group's own mask's (the group's own planes render only after it
/// returns).
pub fn at_the_blur(_: f64, stage: u32, _: u64) -> bool {
    stage == 6
}

/// A cancel inside a preparation blur whose range ends at `end`: stage 6, and
/// progress not past the range's end.
pub fn inside_the_blur(end: f64) -> impl Fn(f64, u32) -> bool {
    move |progress, stage| stage == 6 && progress <= end + 1e-6
}

/// Past the acc fill's declared polls of a canvas-sized fixture: since
/// 13A-5b-iii-b-2 the fill polls `every` once and reports once (a checkpoint)
/// per 4096-sample chunk, `2·⌈w·h/4096⌉`; callers pass `&past_the_fill(W, H)`.
pub fn past_the_fill(w: u32, h: u32) -> impl Fn(f64, u32, u64) -> bool {
    let fill = 2 * (u64::from(w) * u64::from(h)).div_ceil(4096);
    move |_, _, checkpoints| checkpoints >= fill
}

/// A cancel inside the materialization before any blur: stage 1 and progress
/// at the materialization range's `start` (the chain reports nothing).
pub fn inside_the_chain(start: f64) -> impl Fn(f64, u32) -> bool {
    move |progress, stage| stage == 1 && (progress - start).abs() <= 1e-6
}

/// A cancel inside a blur the materialization's chain runs (a child's, or the
/// group's own mask): its reports land at the range's `start` at stage 6.
pub fn inside_the_inner_blur(start: f64) -> impl Fn(f64, u32) -> bool {
    move |progress, stage| stage == 6 && (progress - start).abs() <= 1e-6
}

/// The fraction done when an UNSTAGED byte render's sample loop starts: the
/// plan's work less the loop's one pass per row over the output,
/// `(work − output_pixels·layers.len()) / work` (13A-5b-iii-b-2).
pub fn loop_start(plan: &serde_json::Value) -> f64 {
    let work = plan["work_units"].as_u64().unwrap() as f64;
    let output = &plan["output"];
    let pixels =
        output["width"].as_u64().unwrap() as f64 * output["height"].as_u64().unwrap() as f64;
    let rows = plan["layers"].as_array().unwrap().len() as f64;
    (work - pixels * rows) / work
}

/// The probe's checkpoint figures, for the printed line.
pub fn checkpoints_line(p: &Probe) -> String {
    let frozen = p.frozen.map_or(String::new(), |f| {
        format!(", frozen {:.3} ms", f.as_secs_f64() * 1e3)
    });
    format!(
        "entry checkpoint {}, k {}, issued at {}, {} at return (≥ n0 + k){frozen}; progress {} \
         stage {} at return",
        p.n0, p.k, p.issued, p.checkpoints, p.progress, p.stage
    )
}

pub fn verdict(latency: Duration, bound: f64) -> &'static str {
    if latency.as_secs_f64() <= bound {
        "PASS"
    } else {
        "MISS"
    }
}

pub fn past(latency: Duration, unit: f64) -> f64 {
    (latency.as_secs_f64() - RETURN_FLOOR_S).max(0.0) / unit
}

/// Uncancelled calibration: runs `render` while a SPINNING poller waits
/// until `entry(progress, stage, checkpoints)` first holds, then until
/// `exit` first holds; returns (time from the render's start, checkpoint
/// count) at each. The render must complete.
pub fn window<F>(
    entry: &dyn Fn(f64, u32, u64) -> bool,
    exit: &dyn Fn(f64, u32, u64) -> bool,
    render: F,
) -> ((Duration, u64), (Duration, u64))
where
    F: FnOnce(usize) -> (bool, String) + Send,
{
    let control = unsafe { dr_job_control_create() };
    let token = control as usize;
    let out = thread::scope(|scope| {
        let start = Instant::now();
        let worker = scope.spawn(move || render(token));
        let deadline = start + Duration::from_secs(240);
        let seen = |signal: &dyn Fn(f64, u32, u64) -> bool, what: &str| loop {
            let (progress, stage, checkpoints) = state(token);
            if signal(progress, stage, checkpoints) {
                break (start.elapsed(), checkpoints);
            }
            assert!(
                !worker.is_finished(),
                "the render completed before the {what}"
            );
            assert!(Instant::now() < deadline, "the {what} never came");
            std::hint::spin_loop();
        };
        let opened = seen(entry, "entry");
        let closed = seen(exit, "exit");
        let (ok, message) = worker.join().unwrap();
        assert!(ok, "{message}");
        (opened, closed)
    });
    unsafe { dr_job_control_free(control) };
    out
}

/// The checkpoint count's TIMELINE of one uncancelled run: (time from the
/// render's start, count) at every count change the poller observed, from
/// the entry through the exit (13A-5b-iii-a).
pub struct Timeline(pub Vec<(Duration, u64)>);

impl Timeline {
    /// The gap from the LAST recorded change with count ≤ `count` to the next
    /// recorded change: the unit a cancel issued at `count` froze in.
    pub fn gap_at(&self, count: u64) -> Option<Duration> {
        let i = self.0.iter().rposition(|&(_, n)| n <= count)?;
        let next = self.0.get(i + 1)?;
        Some(next.0.saturating_sub(self.0[i].0))
    }

    /// The longest gap between two recorded changes: (the count before it,
    /// its duration).
    pub fn longest(&self) -> Option<(u64, Duration)> {
        self.0
            .windows(2)
            .map(|pair| (pair[0].1, pair[1].0.saturating_sub(pair[0].0)))
            .max_by_key(|&(_, gap)| gap)
    }
}

/// Uncancelled calibration on the worker's OWN run: runs `render` while a
/// SPINNING poller waits until `entry(progress, stage, checkpoints)` first
/// holds, then records every change of the checkpoint count until `exit`
/// first holds (`window`'s rules; the render must complete).
pub fn timeline<F>(
    entry: &dyn Fn(f64, u32, u64) -> bool,
    exit: &dyn Fn(f64, u32, u64) -> bool,
    render: F,
) -> Timeline
where
    F: FnOnce(usize) -> (bool, String) + Send,
{
    let control = unsafe { dr_job_control_create() };
    let token = control as usize;
    let out = thread::scope(|scope| {
        let start = Instant::now();
        let worker = scope.spawn(move || render(token));
        let deadline = start + Duration::from_secs(240);
        let mut changes = loop {
            let (progress, stage, checkpoints) = state(token);
            if entry(progress, stage, checkpoints) {
                break vec![(start.elapsed(), checkpoints)];
            }
            assert!(
                !worker.is_finished(),
                "the render completed before the entry"
            );
            assert!(Instant::now() < deadline, "the entry never came");
            std::hint::spin_loop();
        };
        loop {
            let (progress, stage, checkpoints) = state(token);
            if changes.last().is_some_and(|&(_, n)| n != checkpoints) {
                changes.push((start.elapsed(), checkpoints));
            }
            if exit(progress, stage, checkpoints) {
                break;
            }
            assert!(
                !worker.is_finished(),
                "the render completed before the exit"
            );
            assert!(Instant::now() < deadline, "the exit never came");
            std::hint::spin_loop();
        }
        let (ok, message) = worker.join().unwrap();
        assert!(ok, "{message}");
        Timeline(changes)
    });
    unsafe { dr_job_control_free(control) };
    out
}

// --------------------------------------------------------- report logs --
//
// The reports-once guards' REPORT LOG and its signatures (13A-5b-i, moved
// here from `regional_byte_preparation_cancel.rs` and
// `regional_group_materialization_cancel.rs` in 13A-5b-iii-a).

fn progress(control: *mut DrJobControl) -> f64 {
    unsafe { dr_job_control_progress(control) }
}

/// Runs `render` uncancelled while a SPINNING logger records every distinct
/// progress value with the time it was first seen (from the render's start);
/// returns the log and `tf`, the worker's own return stamp. The logger reads
/// progress BEFORE testing `is_finished()` and once more after it, so the
/// final value is always logged.
pub fn logged<F>(render: F) -> (Vec<(Duration, f64)>, Duration)
where
    F: FnOnce(usize) -> (bool, String) + Send,
{
    let control = unsafe { dr_job_control_create() };
    let token = control as usize;
    let out = thread::scope(|scope| {
        let start = Instant::now();
        let worker = scope.spawn(move || {
            let (ok, message) = render(token);
            (ok, message, Instant::now())
        });
        let deadline = start + Duration::from_secs(240);
        let mut log: Vec<(Duration, f64)> = Vec::new();
        let push = |log: &mut Vec<(Duration, f64)>, value: f64| {
            if log.last().is_none_or(|&(_, last)| last != value) {
                log.push((start.elapsed(), value));
            }
        };
        loop {
            let value = progress(control);
            push(&mut log, value);
            if worker.is_finished() {
                push(&mut log, progress(control));
                break;
            }
            assert!(Instant::now() < deadline, "the render never finished");
            std::hint::spin_loop();
        }
        let (ok, message, returned) = worker.join().unwrap();
        assert!(ok, "{message}");
        (log, returned.saturating_duration_since(start))
    });
    unsafe { dr_job_control_free(control) };
    out
}

/// INTERVAL: no logged value strictly between `pre` and `target`.
pub fn assert_interval(log: &[(Duration, f64)], pre: f64, target: f64, what: &str) {
    assert!(pre + 1e-6 < target - 1e-6, "{what}: an empty interval");
    for &(at, v) in log {
        assert!(
            !(pre + 1e-6 < v && v < target - 1e-6),
            "{what}: progress {v} logged at {:.3} ms lies strictly between the pre-stage value \
             {pre} and the first report {target}",
            at.as_secs_f64() * 1e3
        );
    }
}

/// PIN: 1.0 is first logged at `t1`; the worker returns at `tf ≤ 1.5·t1`.
pub fn assert_pin(log: &[(Duration, f64)], tf: Duration, what: &str) {
    let t1 = log
        .iter()
        .find(|&&(_, v)| v >= 1.0 - 1e-6)
        .map(|&(at, _)| at)
        .unwrap_or_else(|| panic!("{what}: progress never reached 1.0"));
    let pinned = tf.saturating_sub(t1);
    assert!(
        pinned.as_secs_f64() <= 0.5 * t1.as_secs_f64(),
        "{what}: progress pinned at 1.0 for {:.3} ms of a {:.3} ms render (reached at {:.3} ms)",
        pinned.as_secs_f64() * 1e3,
        tf.as_secs_f64() * 1e3,
        t1.as_secs_f64() * 1e3
    );
}

/// RAMP (13A-5b-iii-a): a stage reporting INSIDE its range `[start, end]`.
/// The SCAN is the log from the first value above `start + 1e-6` through the
/// first value at or above `end − 1e-6`, inclusive (`DrJobControl::report`
/// truncates to 1e-6, so the range's end and the advance log as
/// `floor(end·1e6)/1e6 ≤ end`). Before any scan (13A-5b-iii-b-1) the first
/// value above the start must lie strictly below `end − 1e-6`: past the
/// advance it names a logger stall, AT the advance it names both a stall and
/// a stage reporting nothing inside its range — which a stage polling under
/// `checkpoints_only` (its only value in the range the advance) fails
/// deterministically. That clause subsumes the old trailing "one value
/// strictly inside" check: with `first` strictly above the start and `last`
/// the first value at or past the end, the scan holds a strictly-inside value
/// iff `log[first] < end − 1e-6`. The scan must then be non-decreasing, every
/// scanned value within `[start − 1e-6, end + 1e-6]`.
pub fn assert_ramp(log: &[(Duration, f64)], start: f64, end: f64, what: &str) {
    assert!(start + 1e-6 < end - 1e-6, "{what}: an empty range");
    let first = log
        .iter()
        .position(|&(_, v)| v > start + 1e-6)
        .unwrap_or_else(|| panic!("{what}: progress never rose above the range's start {start}"));
    let (t, v) = (log[first].0.as_secs_f64() * 1e3, log[first].1);
    if v > end + 1e-6 {
        panic!(
            "{what}: the logger caught no value inside ({start}, {end}) — the first value above \
             the start, {v} at {t:.3} ms, is already past the end and its advance (a logger \
             stall, not a leak)"
        );
    } else if v >= end - 1e-6 {
        panic!(
            "{what}: no logged value lies strictly inside ({start}, {end}) — the first value \
             above the start, {v} at {t:.3} ms, is the range's advance: a logger stalled to the \
             advance, or a stage that reports nothing inside its range (polling under \
             checkpoints_only); re-run serially — a deterministic failure is the latter"
        );
    }
    let last = log[first..]
        .iter()
        .position(|&(_, v)| v >= end - 1e-6)
        .map(|i| first + i)
        .unwrap_or_else(|| panic!("{what}: progress never reached the range's end {end}"));
    let scan = &log[first..=last];
    for pair in scan.windows(2) {
        assert!(
            pair[1].1 >= pair[0].1,
            "{what}: progress fell from {} to {} at {:.3} ms",
            pair[0].1,
            pair[1].1,
            pair[1].0.as_secs_f64() * 1e3
        );
    }
    for &(at, v) in scan {
        assert!(
            start - 1e-6 <= v && v <= end + 1e-6,
            "{what}: progress {v} logged at {:.3} ms lies outside the range [{start}, {end}]",
            at.as_secs_f64() * 1e3
        );
    }
}
