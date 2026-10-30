//! The 8-bit regional render's shared PROGRESS figures (Phase 13A-5b-iii-b-2):
//! [`Run`], the running checkpoint counter and completed work one render
//! shares across its preparation, its nested and root stages and its loop
//! (moved out of `doc_regional_guide`, where the staged driver first used it),
//! and [`PrepWork`], the preparation work the admission walk counts per node
//! (`doc_regional_walk`) and the resolution spends (`doc_regional_tree`,
//! `doc_regional_style`). Each preparation stage runs inside its own range —
//! `job.range(done / total, (done + g) / total)` — then advances `done` by `g`
//! and reports it (stage 1, "preparing"): the typed `precision_regional_guide`
//! pattern. The contract is `regional`'s CANCELLATION MODEL.

use crate::job_control::JobContext;

/// The running figures one render shares: the checkpoint counter, the
/// completed work (shared across nested and root calls) and its total.
pub(crate) struct Run<'d> {
    pub(crate) sample: usize,
    pub(crate) done: &'d mut f64,
    pub(crate) total: f64,
}

impl Run<'_> {
    /// The range a stage of `work` units reports inside:
    /// `job.range(done / total, (done + work) / total)`.
    pub(crate) fn range<'a>(&self, job: JobContext<'a>, work: u64) -> JobContext<'a> {
        let done = *self.done;
        job.range(done / self.total, (done + work as f64) / self.total)
    }

    /// `done += work`, then reports `done / total` at stage 1 ("preparing").
    pub(crate) fn advance(&mut self, job: JobContext<'_>, work: u64) -> Result<(), String> {
        *self.done += work as f64;
        job.report(*self.done / self.total, 1)
    }
}

/// One node's preparation work, counted by the admission walk and advanced
/// exactly once by the resolution: its mask blur (`MaskProperties::
/// work_units`), its style planes (`style_scratch(..).1`) and — a
/// materialized group — its materialization (`|rect|·(1 + rows inside) + the
/// subtree's mask/plane work + the group's own mask blur`). All zero for a
/// node the render never resolves or that sits inside a materialized subtree.
#[derive(Clone, Copy, Default)]
pub(crate) struct PrepWork {
    pub(crate) mask: u64,
    pub(crate) style: u64,
    pub(crate) materialization: u64,
}
