//! Typed adjustment state and document operations. Color and guide math live
//! only in adjust_precision; this module owns captured-profile identity,
//! immutable layer replacement, admission and the explicit u16 write boundary.
//!
//! Shared prepared operators capture profile identity; plans include preparation, guides, output
//! and caller-retained state. `AdjustmentAdmission` is the ONE adjustment charge (measured records
//! plus the peak single preparation allowance) for the native save plan and reopen and
//! for the three re-preparation plans: `promote_structure_plan` and the float→16
//! `narrow_structure_plan` `plan_add` each freshly prepared record, and `color_plan` `plan_rebind`s
//! each record by `rebind_retained_bound` (0 for the same profile handle; else the old record's
//! measured retention plus 4× the destination profile's bytes when `binds_profile`, i.e.
//! `Calibrated` or Photo Filter), charging the records ONCE for assign and convert alike
//! because `convert_layers` reuses `rebind_layers`' records. The two
//! structure-plan walks also call `next`
//! before each adjustment they prepare, seeded with the plan's retained bytes and (8→16) the live
//! source shapes, against the op's own limit. The 7E host uses guarded typed dispatch; public
//! workflow qualification is separate from these core numerical checks.

use std::sync::Arc;

use crate::adjust_precision::PrecisionAdjustment;
use crate::icc::IccProfile;
use crate::job_control::{DrJobControl, JobContext};
use crate::precision_document::{Content, PrecisionDocument, PrecisionDocumentLayer};
use crate::precision_image::{
    admit, process_working_limit, PrecisionRun, PrecisionStorage, BOOKKEEPING_BYTES,
};
use crate::precision_render::MAX_RECORDS;
use crate::resources::Inventory;
use crate::tile_store::TILE_EDGE;

#[derive(Clone)]
pub struct PrecisionAdjustmentState {
    pub(crate) kernel: Arc<PrecisionAdjustment>,
    profile: Arc<IccProfile>,
}

impl PrecisionAdjustmentState {
    /// Callers reserve this bound alongside old state before invoking new().
    /// It includes prepared tables and transient JSON/profile preparation.
    pub fn prepare_scratch_bytes(
        description: &str,
        profile: &Arc<IccProfile>,
    ) -> Result<u64, String> {
        PrecisionAdjustment::prepare_scratch_bytes(description, profile)
    }
    pub fn new(description: &str, profile: &Arc<IccProfile>) -> Result<Self, String> {
        let bound = Self::prepare_scratch_bytes(description, profile)?;
        admit(0, bound, 0, process_working_limit())?;
        Ok(Self {
            kernel: Arc::new(PrecisionAdjustment::parse(description, profile)?),
            profile: profile.clone(),
        })
    }
    pub fn description(&self) -> &str {
        self.kernel.description()
    }
    pub fn profile(&self) -> &Arc<IccProfile> {
        &self.profile
    }
    pub fn rebind_scratch_bytes(&self, profile: &Arc<IccProfile>) -> Result<u64, String> {
        if Arc::ptr_eq(profile, &self.profile) {
            Ok(0)
        } else {
            Self::prepare_scratch_bytes(self.description(), profile)
        }
    }
    /// This record under `profile`. An abstract Color Lookup look is
    /// re-baked through it first (`icc_lookup::rebake_meta`), which is
    /// how the typed assign, convert and paste paths re-bake one.
    pub fn rebind(&self, profile: &Arc<IccProfile>) -> Result<Self, String> {
        if Arc::ptr_eq(profile, &self.profile) {
            return Ok(self.clone());
        }
        match crate::icc_lookup::rebake_meta(self.description(), profile)? {
            Some(rebaked) => Self::new(&rebaked, profile),
            None => Self::new(self.description(), profile),
        }
    }
    pub(crate) fn record_resources(&self, inventory: &mut Inventory) {
        inventory.object(&*self.kernel, "precision_adjustment", false);
        self.kernel.record_resources(inventory);
        inventory.object(&*self.profile, "profile", false);
        self.profile.record_resources(inventory);
    }
    /// The operator's measured retained bytes plus `RECORD_SLACK`. A fresh
    /// `Inventory` per call (as `kernel.retained_bytes` builds), so records
    /// never dedup against each other; the profile is excluded because the
    /// plan and the reader already charge it as 3-4 x its serialized bytes.
    pub(crate) fn record_retained_bytes(&self) -> u64 {
        std::mem::size_of::<PrecisionAdjustment>() as u64
            + self.kernel.retained_bytes()
            + RECORD_SLACK
    }
    /// An upper bound on what `rebind(profile)` retains, from this
    /// record alone, without preparing: 0 when `rebind` clones the shared
    /// kernel (the same profile handle); otherwise this record's measured
    /// retention plus, for an operator that binds the profile, 4× the
    /// destination profile's bytes. Why that suffices: only `Calibrated` and
    /// Photo Filter preparation bind the profile (`binds_profile`); their
    /// basis object and ENCODER tables are fixed-size and every prepared
    /// state of either already holds them, so the only part that varies with
    /// the profile is the sampled-TRC payload, at most 3× the profile's
    /// bytes. Every other kernel parses to the same tables under any profile.
    pub(crate) fn rebind_retained_bound(&self, profile: &Arc<IccProfile>) -> u64 {
        if Arc::ptr_eq(profile, &self.profile) {
            return 0;
        }
        let payload = if self.kernel.binds_profile() {
            (profile.bytes().len() as u64).saturating_mul(4)
        } else {
            0
        };
        self.record_retained_bytes().saturating_add(payload)
    }
}

/// Per-record allowance for what `Inventory` cannot see: the `Arc` header
/// around each kernel and the allocator's rounding of its strings and boxed
/// tables. ~1 KiB per record keeps a full `MAX_RECORDS` stack under 1 MiB of
/// slack while dominating those few dozen bytes per record by far.
const RECORD_SLACK: u64 = 1024;
const RETENTION_OVERFLOW: &str = "native adjustment retention overflow";

/// The ONE adjustment-retention rule the native plan (writer) and native
/// reopen (reader) share: `Σ record_retained_bytes + max preparation
/// allowance`. Preparation is transient — one `new()` runs at a time and
/// its scratch shrinks to the measured record — so only the largest single
/// allowance is ever live beside the records already prepared.
#[derive(Default)]
pub(crate) struct AdjustmentAdmission {
    /// Reader: retained bytes before any adjustment is prepared.
    pub(crate) base: u64,
    /// Reader: decode scratch.
    pub(crate) scratch: u64,
    /// Reader: the caller's other retained bytes.
    pub(crate) other: u64,
    /// Reader: the caller's working limit.
    pub(crate) limit: u64,
    /// Σ `record_retained_bytes` so far.
    pub(crate) prepared: u64,
    /// The largest single `prepare_scratch_bytes`.
    pub(crate) peak: u64,
}

impl AdjustmentAdmission {
    /// Writer: charge one record's measured retention and track the peak
    /// preparation allowance.
    pub(crate) fn plan_add(
        &mut self,
        state: &PrecisionAdjustmentState,
        profile: &Arc<IccProfile>,
    ) -> Result<(), String> {
        self.prepared = self
            .prepared
            .checked_add(state.record_retained_bytes())
            .ok_or(RETENTION_OVERFLOW)?;
        self.peak = self
            .peak
            .max(PrecisionAdjustmentState::prepare_scratch_bytes(
                state.description(),
                profile,
            )?);
        Ok(())
    }
    /// Writer, for a planned `rebind(profile)`: charge the rebound
    /// record's retention bound (`rebind_retained_bound`) and track the peak
    /// rebind preparation allowance.
    pub(crate) fn plan_rebind(
        &mut self,
        state: &PrecisionAdjustmentState,
        profile: &Arc<IccProfile>,
    ) -> Result<(), String> {
        self.prepared = self
            .prepared
            .checked_add(state.rebind_retained_bound(profile))
            .ok_or(RETENTION_OVERFLOW)?;
        self.peak = self.peak.max(state.rebind_scratch_bytes(profile)?);
        Ok(())
    }
    /// Reader, before one `new()`: admit and reserve the records prepared so
    /// far plus this record's preparation `bound`. Also used by the 8→16 and
    /// float→16 structure-plan walks, seeded with the plan's retained
    /// bytes, before each adjustment they prepare.
    pub(crate) fn next(&self, bound: u64, job: JobContext<'_>) -> Result<(), String> {
        let retained = self
            .base
            .checked_add(self.prepared)
            .and_then(|v| v.checked_add(bound))
            .ok_or(RETENTION_OVERFLOW)?;
        admit(retained, self.scratch, self.other, self.limit)?;
        job.reserve(
            retained
                .checked_add(self.scratch)
                .ok_or(RETENTION_OVERFLOW)?,
        )
    }
    /// Reader, after preparing one CMYK adjustment record
    /// (`cmyk_adjust_layer`, #1 phase 2): its measured retention `bytes`
    /// joins the prepared total — [`Self::record`]'s charge for a record
    /// that holds no RGB operator.
    pub(crate) fn record_bytes(&mut self, bytes: u64, job: JobContext<'_>) -> Result<(), String> {
        self.prepared = self.prepared.checked_add(bytes).ok_or(RETENTION_OVERFLOW)?;
        job.reserve(
            self.base
                .checked_add(self.prepared)
                .and_then(|v| v.checked_add(self.scratch))
                .ok_or(RETENTION_OVERFLOW)?,
        )
    }
    /// Reader, after one `new()`: the preparation allowance shrinks to the
    /// record's measured retention.
    pub(crate) fn record(
        &mut self,
        state: &PrecisionAdjustmentState,
        job: JobContext<'_>,
    ) -> Result<(), String> {
        self.prepared = self
            .prepared
            .checked_add(state.record_retained_bytes())
            .ok_or(RETENTION_OVERFLOW)?;
        job.reserve(
            self.base
                .checked_add(self.prepared)
                .and_then(|v| v.checked_add(self.scratch))
                .ok_or(RETENTION_OVERFLOW)?,
        )
    }
    /// The writer's charge: every measured record plus the peak allowance.
    pub(crate) fn total(&self) -> Result<u64, String> {
        self.prepared
            .checked_add(self.peak)
            .ok_or_else(|| RETENTION_OVERFLOW.into())
    }
}

impl PrecisionDocumentLayer {
    pub fn adjustment(
        name: &str,
        description: &str,
        opacity: f32,
        visible: bool,
        profile: &Arc<IccProfile>,
    ) -> Result<Self, String> {
        crate::precision_document::scalar_and_name(name, opacity)?;
        Self::prepared_adjustment(
            name,
            PrecisionAdjustmentState::new(description, profile)?,
            opacity,
            visible,
        )
    }
    pub(crate) fn prepared_adjustment(
        name: &str,
        state: PrecisionAdjustmentState,
        opacity: f32,
        visible: bool,
    ) -> Result<Self, String> {
        crate::precision_document::scalar_and_name(name, opacity)?;
        Ok(Self {
            // A new adjustment record is a new entry (`doc_identity`).
            id: crate::doc_identity::mint_layer_id(),
            name: Arc::new(name.into()),
            retained_source: None,
            warp: None,
            generated: None,
            description: None,
            opacity,
            blend_mode: crate::blend::BlendMode::Normal,
            visible,
            content: Content::Adjustment(state),
            records: 1,
            depth: 0,
            clipped: false,
            blend_if: None,
            style: None,
            mask: None,
            label: 0,
            locks: 0,
            link: 0,
            open: true,
            reference: (0, 0),
        })
    }
}

impl PrecisionDocument {
    pub fn adjustment_description(&self, index: usize) -> Option<&str> {
        match &self.layer(index)?.content {
            Content::Adjustment(state) => Some(state.description()),
            Content::CmykAdjustment(state) => Some(state.description()),
            _ => None,
        }
    }

    /// Preparation planning does not build dense operator tables. None appends;
    /// Some sets an adjustment, or applies destructively to a raster leaf.
    pub fn adjustment_scratch_bytes(
        &self,
        index: Option<usize>,
        description: &str,
        destructive: bool,
    ) -> Result<u64, String> {
        let preparation =
            PrecisionAdjustmentState::prepare_scratch_bytes(description, &self.profile)?;
        let mut scratch = preparation
            .checked_add(2 * BOOKKEEPING_BYTES)
            .ok_or("adjustment scratch overflow")?;
        match (index, destructive) {
            (None, false) => {
                if self.records >= MAX_RECORDS {
                    return Err("too many high-depth document records".into());
                }
            }
            (Some(index), false) => {
                if !matches!(
                    self.layer(index).map(|layer| &layer.content),
                    Some(Content::Adjustment(_))
                ) {
                    return Err(
                        "setting a high-depth adjustment requires an adjustment layer".into(),
                    );
                }
            }
            (Some(index), true) => {
                let image = self
                    .layer_source(index)
                    .ok_or("destructive high-depth adjustment requires a raster source")?;
                let (w, h) = image.dimensions();
                let count = u64::from(w) * u64::from(h);
                // The float arm's own plan (`float_adjust_apply`), so this
                // report and its admission agree.
                if image.storage() == PrecisionStorage::Rgba32Float {
                    return self.float_apply_scratch(description, count, 0);
                }
                let (guide_scratch, _) =
                    PrecisionAdjustment::guide_plan_for_description(description, w, h)?;
                scratch = scratch
                    .checked_add(count * 8)
                    .and_then(|n| n.checked_add(guide_scratch))
                    .ok_or("adjustment scratch overflow")?;
            }
            (None, true) => {
                return Err("destructive adjustment requires a raster layer index".into())
            }
        }
        Ok(scratch)
    }

    pub fn add_adjustment(
        &self,
        name: &str,
        description: &str,
        opacity: f32,
        limit: u64,
        other: u64,
        control: Option<&DrJobControl>,
    ) -> Result<Option<Self>, String> {
        let _run = control.map(DrJobControl::begin).transpose()?;
        let job = control.map_or(JobContext::NONE, JobContext::new);
        crate::precision_document::scalar_and_name(name, opacity)?;
        let scratch = self.adjustment_scratch_bytes(None, description, false)?;
        admit(self.retained_bytes(), scratch, other, limit)?;
        job.reserve(scratch)?;
        let state = PrecisionAdjustmentState::new(description, &self.profile)?;
        job.check()?;
        let mut layers = self.layers.as_ref().clone();
        layers.push(PrecisionDocumentLayer::prepared_adjustment(
            name, state, opacity, true,
        )?);
        let next = self.rebuilt(
            self.width,
            self.height,
            layers,
            self.channels.as_ref().clone(),
        )?;
        job.report(1.0, 5)?;
        Ok(Some(next))
    }

    pub fn set_adjustment(
        &self,
        index: usize,
        description: &str,
        limit: u64,
        other: u64,
        control: Option<&DrJobControl>,
    ) -> Result<Option<Self>, String> {
        let _run = control.map(DrJobControl::begin).transpose()?;
        let job = control.map_or(JobContext::NONE, JobContext::new);
        let scratch = self.adjustment_scratch_bytes(Some(index), description, false)?;
        admit(self.retained_bytes(), scratch, other, limit)?;
        job.reserve(scratch)?;
        let state = PrecisionAdjustmentState::new(description, &self.profile)?;
        job.check()?;
        let out = if self.adjustment_description(index) == Some(state.description()) {
            None
        } else {
            self.replacing_content(index, Content::Adjustment(state))?
        };
        job.report(1.0, 5)?;
        Ok(out)
    }

    /// Apply the shared operator/guide to one original raster, preserving its
    /// alpha exactly and leaving hidden RGB at alpha zero untouched. Sampling
    /// uses image coordinates, matching the established destructive path.
    /// The LOCK gate (`precision_lock`): a destructive adjustment rewrites
    /// the target's pixels, so it runs under a `Pixels` edit.
    pub fn apply_adjustment(
        &self,
        index: usize,
        description: &str,
        limit: u64,
        other: u64,
        control: Option<&DrJobControl>,
    ) -> Result<Option<Self>, String> {
        let _run = control.map(DrJobControl::begin).transpose()?;
        let job = control.map_or(JobContext::NONE, JobContext::new);
        self.under_locks(index, crate::doc_lock::EditKind::Pixels, |doc| {
            doc.applying_adjustment_unlocked(index, description, limit, other, job)
        })
    }

    fn applying_adjustment_unlocked(
        &self,
        index: usize,
        description: &str,
        limit: u64,
        other: u64,
        job: JobContext<'_>,
    ) -> Result<Option<Self>, String> {
        if self
            .layer_source(index)
            .is_some_and(|image| image.storage() == PrecisionStorage::Rgba32Float)
        {
            return self.float_applying_adjustment_unlocked(index, description, limit, other, job);
        }
        let scratch = self.adjustment_scratch_bytes(Some(index), description, true)?;
        admit(self.retained_bytes(), scratch, other, limit)?;
        job.reserve(scratch)?;
        let state = PrecisionAdjustmentState::new(description, &self.profile)?;
        job.check()?;
        if state.kernel.is_identity() {
            job.report(1.0, 5)?;
            return Ok(None);
        }
        let layer = self.layer(index).ok_or("invalid adjustment raster index")?;
        let image = layer
            .source()
            .ok_or("adjustment target is not a raster source")?;
        let (w, h) = image.dimensions();
        let count = w as usize * h as usize;
        let guide = state
            .kernel
            .guide(w, h, &|i| image.pixel(i), job.range(0.05, 0.4))?;
        job.report(0.4, 6)?;
        let samples = (0..count).map(|i| {
            let source = image.pixel(i);
            if source[3] == 0.0 {
                return source;
            }
            let x = (i % w as usize) as u32;
            let y = (i / w as usize) as u32;
            let g = guide.as_ref().map_or(0.0, |guide| guide.at(x, y));
            let adjusted = state.kernel.apply_at(
                [source[0], source[1], source[2]],
                g,
                (i64::from(x), i64::from(y)),
            );
            [adjusted[0], adjusted[1], adjusted[2], source[3]]
        });
        // 13B-3d-2: the output rebuilds over the target's tiles (every
        // unchanged tile shared) and the changed scan walks both images in
        // TILE-BAND order (each 256-row band, each 256-column span, each
        // row), so no view is bound. A tile-aligned span is ONE run in
        // either representation, so the runs pair exactly. `changed |=` is
        // order-independent and `job.every` reads only the visit counter
        // `i`, so the checkpoints are the flat walk's (a row across every
        // tile column costs ~20% at 24 MP; 13B-3c-2a's histogram).
        let output = image.rebuilt_float_samples(
            w,
            h,
            self.profile.clone(),
            PrecisionStorage::Rgba16Unorm,
            samples,
            job,
        )?;
        if output.storage() != PrecisionStorage::Rgba16Unorm
            || image.storage() != PrecisionStorage::Rgba16Unorm
        {
            return Err("destructive high-depth adjustment requires u16 storage".into());
        }
        let mut changed = false;
        let mut i = 0;
        for band in (0..h).step_by(TILE_EDGE as usize) {
            for x0 in (0..w).step_by(TILE_EDGE as usize) {
                let x1 = x0.saturating_add(TILE_EDGE).min(w);
                for y in band..band.saturating_add(TILE_EDGE).min(h) {
                    let pairs = output
                        .row_segments(y, x0, x1)
                        .zip(image.row_segments(y, x0, x1));
                    for ((_, a_run), (_, b_run)) in pairs {
                        let (PrecisionRun::Rgba16(a_run), PrecisionRun::Rgba16(b_run)) =
                            (a_run, b_run)
                        else {
                            return Err(
                                "destructive high-depth adjustment requires u16 storage".into()
                            );
                        };
                        for (a, b) in a_run.iter().zip(b_run) {
                            job.every(i)?;
                            changed |= a != b;
                            i += 1;
                        }
                    }
                }
            }
        }
        let out = if changed {
            self.replacing_content(
                index,
                Content::Raster {
                    image: output,
                    offset: layer.offset(),
                },
            )?
        } else {
            None
        };
        job.report(1.0, 5)?;
        Ok(out)
    }
}
