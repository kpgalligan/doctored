//! Explicit single-operation cancellation/progress shared across the FFI worker
//! and its owner. No thread-local state or callbacks: kernels borrow a context,
//! checkpoint returns an ordinary error, and unwinding Result drops scratch.
//! Progress describes completed pipeline work, not an elapsed-time prediction.
//!
//! Hosts retain the token and immutable handles/buffers until every worker/poll/cancel call ends;
//! only the owning host frees it. `JobContext` is explicitly borrowed through planning, distance
//! transforms, PatchMatch and Poisson; cancellation returns an ordinary error and drops working
//! buffers. The old synchronous exports run the same kernels with no token. Since 13A-5b-iii-b-3
//! `DrJobControl` also carries a TEST-ONLY `#[doc(hidden)]` checkpoint seam that no FFI export or
//! kernel reaches: `park_at(n)` parks the worker at the checkpoint whose post-increment count is
//! `n` (before its cancellation test) until `release()` (idempotent; a release before the park
//! makes it fall through), `parked()` / `wait_parked(timeout)` (wakes on the park, on a release or
//! at the timeout — never spins), and `record_reports()` / `reports()`, the worker's own log of
//! every `report` call (`Report { checkpoints, progress, stage }`). Hot-path cost: one relaxed
//! load + compare per checkpoint, one relaxed load per report. `EVERY_INTERVAL` (`pub(crate)`, 4096,
//! since 13B-3e-3b) is `JobContext::every`'s poll interval; a walk that hoists those checks to its
//! runs steps by it so its checkpoint count stays the flat walk's. `JobContext::with_held_bytes`
//! adds bytes a caller keeps alive under a nested call to every nested `reserve`, before the
//! floor.
//!
//! `JobContext` is explicitly borrowed through planning, PatchMatch and Poisson; no thread-local
//! cancellation, partial results or eventual-result-only cancellation. Legacy entry points use the
//! same kernels with a no-control context.

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Condvar, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

/// The interval (a power of two) at which [`JobContext::every`] polls: a
/// check at every flat index that is a multiple of it. A walk that hoists
/// those checks to its runs steps by this constant so its checkpoint count
/// stays the flat walk's.
pub(crate) const EVERY_INTERVAL: usize = 4096;

/// A thread-safe token. The host retains it until all worker/query calls finish.
#[derive(Default)]
pub struct DrJobControl {
    cancelled: AtomicBool,
    started: AtomicBool,
    progress: AtomicU32,
    stage: AtomicU32,
    scratch: AtomicU64,
    scratch_limit: AtomicU64,
    // The host's WHOLE working limit; 0 = unknown.
    working_limit: AtomicU64,
    checkpoints: AtomicU64,
    // The test-only checkpoint seam (Phase 13A-5b-iii-b-3): the park count
    // (0 = never), the park state with its condition variable, and the
    // report recorder. No FFI export or kernel ever reaches them.
    park_at: AtomicU64,
    park: Mutex<Park>,
    changed: Condvar,
    recording: AtomicBool,
    reports: Mutex<Vec<Report>>,
}

/// One `DrJobControl::report` call as the recorder saw it (test-only seam,
/// Phase 13A-5b-iii-b-3): the checkpoint count when the report landed (a
/// `JobContext::report`'s own `check()` counts BEFORE it), the progress
/// after the report's `fetch_max`, and the reported stage.
#[doc(hidden)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Report {
    pub checkpoints: u64,
    pub progress: f64,
    pub stage: u32,
}

/// The park state: `parked` once the armed checkpoint was reached, `released`
/// once `release` ran (before or after the park).
#[derive(Default)]
struct Park {
    parked: bool,
    released: bool,
}

impl DrJobControl {
    /// Request cooperative cancellation; idempotent and safe during work.
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
    }
    /// Whether cancellation has been requested.
    pub fn cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }
    /// Return an ordinary cancellation error at a bounded kernel checkpoint.
    pub fn checkpoint(&self) -> Result<(), String> {
        let n = self.checkpoints.fetch_add(1, Ordering::Relaxed) + 1;
        if n == self.park_at.load(Ordering::Relaxed) {
            self.park();
        }
        if self.cancelled() {
            Err("operation cancelled".into())
        } else {
            Ok(())
        }
    }
    /// Checkpoints polled so far (each `checkpoint()` counts before its test):
    /// a load-independent trigger for cancellation probes (Phase 13A-5b-ii-a).
    #[doc(hidden)]
    pub fn checkpoints(&self) -> u64 {
        self.checkpoints.load(Ordering::Relaxed)
    }
    /// Completed work in [0,1], monotone over this token's lifetime.
    pub fn progress(&self) -> f64 {
        f64::from(self.progress.load(Ordering::Relaxed)) / 1_000_000.0
    }
    /// 0 ready, 1 preparing, 2 planning, 3 inpainting, 4 blending, 5 complete, 6 rendering.
    pub fn stage(&self) -> u32 {
        self.stage.load(Ordering::Relaxed)
    }
    /// Conservative live scratch reservation; zero after the worker returns.
    pub fn scratch_bytes(&self) -> u64 {
        self.scratch.load(Ordering::Relaxed)
    }
    pub(crate) fn report(&self, progress: f64, stage: u32) {
        self.progress.fetch_max(
            (progress.clamp(0.0, 1.0) * 1_000_000.0) as u32,
            Ordering::Relaxed,
        );
        self.stage.store(stage, Ordering::Relaxed);
        if self.recording.load(Ordering::Relaxed) {
            let report = Report {
                checkpoints: self.checkpoints.load(Ordering::Relaxed),
                progress: self.progress(),
                stage,
            };
            lock(&self.reports).push(report);
        }
    }
    pub(crate) fn reserve(&self, bytes: u64) -> Result<(), String> {
        self.checkpoint()?;
        let limit = self.scratch_limit.load(Ordering::Acquire);
        let working = self.working_limit.load(Ordering::Acquire);
        if limit > 0 && bytes > limit && working > 0 && bytes >= working {
            return Err(format!("operation needs up to {} MiB of working memory on its own, at least the {} MiB working limit, so closing other documents will not help. Work on a smaller image or region.", bytes.div_ceil(1024 * 1024), working / (1024 * 1024)));
        }
        if limit > 0 && bytes > limit {
            return Err(format!("operation needs up to {} MiB of working memory; {} MiB is available under the working-memory limit. Close other documents or reduce the requested region.", bytes.div_ceil(1024*1024),limit/(1024*1024)));
        }
        self.scratch.store(bytes, Ordering::Relaxed);
        Ok(())
    }
    pub(crate) fn set_scratch_limit(&self, bytes: u64) -> bool {
        if self.started.load(Ordering::Acquire) {
            return false;
        }
        self.scratch_limit.store(bytes, Ordering::Release);
        true
    }
    pub(crate) fn set_working_limit(&self, bytes: u64) -> bool {
        if self.started.load(Ordering::Acquire) {
            return false;
        }
        self.working_limit.store(bytes, Ordering::Release);
        true
    }
    pub(crate) fn begin(&self) -> Result<JobRun<'_>, String> {
        self.started
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| "job control has already been used".to_string())?;
        self.checkpoint()?;
        self.report(0.0, 1);
        Ok(JobRun(self))
    }

    // ---- the test-only checkpoint seam (Phase 13A-5b-iii-b-3) ----
    //
    // `#[doc(hidden)]` and never reached by any FFI export or kernel:
    // `dr_job_control_*` in `ffi_jobs.rs` is untouched, and only black-box
    // tests arm a park or enable the recorder. `begin()`'s `report(0.0, 1)`
    // and `ffi_jobs.rs`'s closing `report(1.0, 5)` are recorded like every
    // other report. The hot path pays one relaxed load + compare per
    // checkpoint and one relaxed load per report; a lock is taken only when
    // the count matches or the recorder is on.
    //
    // WAKE RULE: `wait_parked` wakes on the park, on a `release`, or at its
    // timeout — never by spinning — so a worker that returns without reaching
    // the count (and releases on return) wakes a waiter at once.

    /// Arm the park: the checkpoint whose POST-increment count equals `count`
    /// parks BEFORE its cancellation test, until `release`. 0 never parks.
    #[doc(hidden)]
    pub fn park_at(&self, count: u64) {
        self.park_at.store(count, Ordering::Relaxed);
    }

    /// Whether the armed checkpoint has been reached (a park that fell
    /// through after an early `release` counts too).
    #[doc(hidden)]
    pub fn parked(&self) -> bool {
        lock(&self.park).parked
    }

    /// Block until the worker parks, `release` runs, or `timeout` elapses;
    /// answers `parked` — true once parked, false on a release without a park
    /// or on the timeout.
    #[doc(hidden)]
    pub fn wait_parked(&self, timeout: Duration) -> bool {
        let guard = lock(&self.park);
        let (guard, _) = self
            .changed
            .wait_timeout_while(guard, timeout, |p| !(p.parked || p.released))
            .unwrap_or_else(PoisonError::into_inner);
        guard.parked
    }

    /// Release a parked worker (idempotent); a release BEFORE the park makes
    /// the park fall through.
    #[doc(hidden)]
    pub fn release(&self) {
        lock(&self.park).released = true;
        self.changed.notify_all();
    }

    /// Enable the report log.
    #[doc(hidden)]
    pub fn record_reports(&self) {
        self.recording.store(true, Ordering::Relaxed);
    }

    /// Every `report` call since `record_reports`, in order.
    #[doc(hidden)]
    pub fn reports(&self) -> Vec<Report> {
        lock(&self.reports).clone()
    }

    fn park(&self) {
        let mut guard = lock(&self.park);
        guard.parked = true;
        self.changed.notify_all();
        let _released = self
            .changed
            .wait_while(guard, |p| !p.released)
            .unwrap_or_else(PoisonError::into_inner);
    }
}

/// A poisoned seam lock must still park, release and record.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// RAII reservation cleanup also covers error/panic paths.
pub(crate) struct JobRun<'a>(&'a DrJobControl);
impl Drop for JobRun<'_> {
    fn drop(&mut self) {
        self.0.scratch.store(0, Ordering::Relaxed);
    }
}

/// Explicit context borrowed by the same kernels legacy synchronous calls use.
#[derive(Clone, Copy)]
pub(crate) struct JobContext<'a> {
    control: Option<&'a DrJobControl>,
    start: f64,
    end: f64,
    reservation_floor: u64,
    held: u64,
}
impl<'a> JobContext<'a> {
    pub(crate) const NONE: Self = Self {
        control: None,
        start: 0.0,
        end: 1.0,
        reservation_floor: 0,
        held: 0,
    };
    pub(crate) fn new(control: &'a DrJobControl) -> Self {
        Self {
            control: Some(control),
            ..Self::NONE
        }
    }
    pub(crate) fn check(self) -> Result<(), String> {
        self.control.map_or(Ok(()), DrJobControl::checkpoint)
    }
    /// Whether cancellation has been requested, read from the flag only: no
    /// checkpoint, no park, no report; `NONE` answers `false`. The query for
    /// a caller mapping a failed sub-step's error that must not turn a cancel
    /// into its own sentence.
    pub(crate) fn cancelled(self) -> bool {
        self.control.is_some_and(DrJobControl::cancelled)
    }
    /// Whether a host job control is attached (`NONE` answers `false`): a
    /// controlled call can be cancelled, so a one-shot destructive pass under
    /// it carries no work ceiling (`sampling_gaussian::destructive_work_cap`).
    pub(crate) fn is_controlled(self) -> bool {
        self.control.is_some()
    }
    /// Dense numerical passes poll every [`EVERY_INTERVAL`] cells; expensive
    /// PatchMatch distances poll every 64 targets. Rows additionally poll on
    /// each pass.
    pub(crate) fn every(self, index: usize) -> Result<(), String> {
        if index & (EVERY_INTERVAL - 1) == 0 {
            self.check()?;
        }
        Ok(())
    }
    pub(crate) fn report(self, fraction: f64, stage: u32) -> Result<(), String> {
        self.check()?;
        if let Some(control) = self.control {
            control.report(self.start + (self.end - self.start) * fraction, stage);
        }
        Ok(())
    }
    pub(crate) fn range(self, start: f64, end: f64) -> Self {
        Self {
            control: self.control,
            start: self.start + (self.end - self.start) * start,
            end: self.start + (self.end - self.start) * end,
            reservation_floor: self.reservation_floor,
            held: self.held,
        }
    }
    /// The same checkpoints with every report landing at the range's start: a
    /// preparation stage whose work the plan does not count (or which reports
    /// elsewhere) polls cancellation without moving progress (Phase 13A-5a).
    pub(crate) fn checkpoints_only(self) -> Self {
        Self {
            end: self.start,
            ..self
        }
    }
    /// Keep a compound operation's already-live buffers in nested stage reports.
    pub(crate) fn with_reservation_floor(self, bytes: u64) -> Self {
        Self {
            reservation_floor: self.reservation_floor.max(bytes),
            ..self
        }
    }
    /// Bytes a caller keeps alive UNDER a nested call that does not know
    /// about them (`doc_retouch_output`'s temporary plate raster): added to
    /// every nested `reserve`, before the floor. Additive, unlike the floor,
    /// because the nested figure is measured without them.
    pub(crate) fn with_held_bytes(self, bytes: u64) -> Self {
        Self {
            held: self.held.saturating_add(bytes),
            ..self
        }
    }
    pub(crate) fn reserve(self, bytes: u64) -> Result<(), String> {
        self.control.map_or(Ok(()), |control| {
            control.reserve(bytes.saturating_add(self.held).max(self.reservation_floor))
        })
    }
}
