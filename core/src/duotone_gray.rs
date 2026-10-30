//! A Duotone document into Grayscale and Bitmap (#1's Duotone close,
//! `designs/duotone-mode.md`).
//!
//! * **The rule.** Image ▸ Mode ▸ Grayscale on a Duotone document renders the
//!   inks into the gray (measured against Photoshop, take1's `gray.tif`): per
//!   u16 gray code `g`, the inks' mix at `k = 1 − g/65535` (`duotone_ink`'s
//!   ONE mix, through the Gray profile's tone curve as the view decodes it),
//!   its PCS luminance `Y` clamped to [0, 1], encoded through the document's
//!   own Gray profile and depth (`GrayPolicy::encode` then `code`, the rule
//!   `GrayPolicy::luminance_code` applies to an RGB pixel). The result is the
//!   same Gray document with the inks dropped: same profile, same depth.
//! * **The gate**, fixed before comparing: on each of take1's cases, every
//!   pixel's 8-bit result within one code of each Photoshop code recorded for
//!   its source code (`decided.json`'s `gray_tif_by_code`), the two-ink case
//!   first, then the three-ink case with nothing refit
//!   (`tests/duotone_photoshop_take.rs`).
//! * **The table** is 65 536 u16 entries ([`GRAY_TABLE_BYTES`]), built once a
//!   call, and every raster leaf's R code is mapped through it into `[t, t,
//!   t, a]` (`gray_convert::gray_rows_u16`, alpha kept) by the render's own
//!   tree assembly (`duotone_convert`'s `duotone_leaves_mapped`): masks,
//!   groups, blend modes, opacity, alpha channels, comps, guides, paths,
//!   samplers, the light, metadata and resolution all carry; a description
//!   is dropped, as the render drops it.
//! * **Blockers** are the render's (`first_render_blocker`): the plan's
//!   `refusal` with `flatten_required`, and `flatten` runs Flatten Image
//!   first.
//! * **The Grayscale plan**: the raster pixels (the canvas when flattening)
//!   × `RENDER_BYTES_PER_PIXEL`, the table, Flatten Image's render scratch
//!   when flattening, the result's display preparation (canvas ×
//!   `TO_DUOTONE_BYTES_PER_PIXEL`, the measured term) and bookkeeping. Held
//!   by `scripts/duotone-admission/measure.py`'s `to-gray-from-duotone`.
//! * **Bitmap** is that Grayscale rule, then Doctored's Gray → Bitmap
//!   (`bitmap_convert`), composed and unmeasured against Photoshop. The
//!   refusal, `flatten_required`, `flattens`, `width` and `height` are those
//!   of the same document with its inks dropped (Bitmap's own blockers, its
//!   8 Bits/Channel step); `flatten` runs Flatten Image on the Duotone
//!   document first, then the Grayscale stage, then Bitmap without
//!   `flatten`, admitted beside the held source. Its plan is the Grayscale
//!   stage's without the display term plus Bitmap's. Bitmap's own
//!   conversion begins the job control (`convert_to_bitmap`), so the stages
//!   before it poll the control's cancel without moving progress.

use std::sync::Arc;

use serde_json::{json, Value};

use crate::duotone_convert::{first_render_blocker, split_flatten, RENDER_BYTES_PER_PIXEL};
use crate::duotone_document::TO_DUOTONE_BYTES_PER_PIXEL;
use crate::gray_convert::gray_rows_u16;
use crate::job_control::{DrJobControl, JobContext};
use crate::precision_document::PrecisionDocument;
use crate::precision_image::{admit, BOOKKEEPING_BYTES};

/// The Grayscale table: one u16 gray code per source code.
pub(crate) const GRAY_TABLE_BYTES: u64 = 65_536 * 2;

/// The refusal of a document that is not Duotone.
const NOT_DUOTONE_GRAY: &str = "the Duotone Grayscale conversion takes a Duotone document";

/// The refusal of a document that is not Duotone, Bitmap's.
const NOT_DUOTONE_BITMAP: &str = "the Duotone Bitmap conversion takes a Duotone document";

/// A plan's byte count.
fn plan_bytes(plan: &Value) -> u64 {
    plan["working_memory_plan_bytes"].as_u64().unwrap_or(0)
}

impl PrecisionDocument {
    /// The Grayscale table of this Duotone document (module doc): the
    /// inks' luminance per gray code, encoded through the document's own Gray
    /// profile at its depth.
    fn duotone_gray_table(&self, job: JobContext<'_>) -> Result<Vec<u16>, String> {
        let policy = self.gray_policy().ok_or(NOT_DUOTONE_GRAY)?;
        let inks = policy.duotone().ok_or(NOT_DUOTONE_GRAY)?;
        let splines = inks.coverages();
        let decode = |code: f64| policy.decode_unit(code);
        let mut table: Vec<u16> = Vec::new();
        table
            .try_reserve_exact(65_536)
            .map_err(|_| "could not allocate the Duotone Grayscale table")?;
        for g in 0..=u16::MAX {
            let k = 1.0 - f64::from(g) / 65535.0;
            let y = inks.mix_xyz(&splines, k, &decode)[1].clamp(0.0, 1.0);
            table.push(policy.code(policy.encode(y)));
            if g % 4096 == 0 {
                job.check()?;
            }
        }
        Ok(table)
    }

    /// The Grayscale stage's scratch (module doc); `display` adds the
    /// result's display preparation.
    fn duotone_gray_scratch(&self, flattens: bool, display: bool) -> u64 {
        let canvas = u64::from(self.width) * u64::from(self.height);
        let pixels = if flattens {
            canvas
        } else {
            Self::gray_raster_pixels(self.root_layers())
        };
        let flattening = if flattens {
            self.render_plan().map(|p| p.scratch_bytes).unwrap_or(0)
        } else {
            0
        };
        let shown = if display {
            canvas.saturating_mul(TO_DUOTONE_BYTES_PER_PIXEL)
        } else {
            0
        };
        pixels
            .saturating_mul(RENDER_BYTES_PER_PIXEL)
            .saturating_add(GRAY_TABLE_BYTES)
            .saturating_add(flattening)
            .saturating_add(shown)
            .saturating_add(BOOKKEEPING_BYTES)
    }

    /// Duotone → Grayscale's plan:
    /// `{"working_memory_plan_bytes","refusal","flatten_required","flattens"}`.
    pub fn convert_duotone_to_gray_plan(&self, flatten: bool) -> Result<Value, String> {
        let blocker = first_render_blocker(self.root_layers());
        let refusal = if self.is_duotone() {
            blocker.clone().filter(|_| !flatten)
        } else {
            Some(NOT_DUOTONE_GRAY.to_string())
        };
        let flattens = blocker.is_some() && flatten;
        Ok(json!({
            "working_memory_plan_bytes": self.duotone_gray_scratch(flattens, true),
            "refusal": refusal,
            "flatten_required": blocker.is_some(),
            "flattens": flattens,
        }))
    }

    /// Duotone → Grayscale (module doc): the same Gray document, each raster
    /// leaf through the inks' luminance, the inks dropped. Refuses a document
    /// that is not Duotone, a blocker without `flatten`, scratch past `limit`
    /// and a cancel.
    pub fn convert_duotone_to_gray(
        &self,
        flatten: bool,
        limit: u64,
        other: u64,
        control: Option<&DrJobControl>,
    ) -> Result<Self, String> {
        if !self.is_duotone() {
            return Err(NOT_DUOTONE_GRAY.into());
        }
        let blocker = first_render_blocker(self.root_layers());
        if let (Some(reason), false) = (&blocker, flatten) {
            return Err(reason.clone());
        }
        let flattens = blocker.is_some();
        let _run = control.map(DrJobControl::begin).transpose()?;
        let job = control.map_or(JobContext::NONE, JobContext::new);
        let scratch = self.duotone_gray_scratch(flattens, true);
        admit(self.retained_bytes(), scratch, other, limit)?;
        job.reserve(scratch)?;
        let flattened;
        let source = if flattens {
            flattened = self
                .flatten_image(limit, other, job.range(0.0, 0.3))?
                .ok_or("flattening the image changed nothing")?;
            &flattened
        } else {
            self
        };
        let out = source.duotone_gray_stage(limit, other, job.range(0.3, 1.0))?;
        job.report(1.0, 5)?;
        Ok(out)
    }

    /// The Grayscale stage on a document with no blocker, admission the
    /// caller's: the table, the map, the inks dropped.
    fn duotone_gray_stage(
        &self,
        limit: u64,
        other: u64,
        job: JobContext<'_>,
    ) -> Result<Self, String> {
        let policy = self.gray_policy().ok_or(NOT_DUOTONE_GRAY)?.clone();
        let table = self.duotone_gray_table(job.range(0.0, 0.05))?;
        let code = |pixel: [u16; 4]| table[usize::from(pixel[0])];
        let out =
            self.duotone_leaves_mapped(limit, other, job.range(0.05, 1.0), &mut |image, part| {
                gray_rows_u16(image, &policy, &code, part)
            })?;
        out.with_gray(Arc::new(policy.with_duotone(None)))
    }

    /// This Duotone document with its inks dropped: the Gray document its
    /// storage is, every sample shared.
    fn duotone_twin(&self, refusal: &str) -> Result<Self, String> {
        let policy = self
            .gray_policy()
            .filter(|p| p.duotone().is_some())
            .ok_or(refusal)?;
        self.clone().with_gray(Arc::new(policy.with_duotone(None)))
    }

    /// Duotone → Bitmap's plan (module doc), under Bitmap's options:
    /// `{"working_memory_plan_bytes","refusal","flatten_required","flattens",
    /// "width","height"}`.
    pub fn convert_duotone_to_bitmap_plan(&self, options: &str) -> Result<Value, String> {
        let twin = match self.duotone_twin(NOT_DUOTONE_BITMAP) {
            Ok(twin) => twin,
            Err(reason) => {
                // Malformed options are the `Err` on any document.
                let mut plan = self.convert_to_bitmap_plan(options)?;
                plan["refusal"] = json!(reason);
                return Ok(plan);
            }
        };
        let mut plan = twin.convert_to_bitmap_plan(options)?;
        let flattens = plan["flattens"].as_bool().unwrap_or(false);
        plan["working_memory_plan_bytes"] = json!(self
            .duotone_gray_scratch(flattens, false)
            .saturating_add(plan_bytes(&plan)));
        Ok(plan)
    }

    /// Duotone → Bitmap (module doc): Flatten Image when `flatten` asks for
    /// it, the Grayscale stage, then Gray → Bitmap of the options without
    /// `flatten`, under `control`.
    pub fn convert_duotone_to_bitmap(
        &self,
        options: &str,
        limit: u64,
        other: u64,
        control: Option<&DrJobControl>,
    ) -> Result<Self, String> {
        let (_, rest) = split_flatten(options)?;
        let twin = self.duotone_twin(NOT_DUOTONE_BITMAP)?;
        let plan = twin.convert_to_bitmap_plan(options)?;
        if let Some(reason) = plan["refusal"].as_str() {
            return Err(reason.to_string());
        }
        let flattens = plan["flattens"].as_bool().unwrap_or(false);
        let scratch = self.duotone_gray_scratch(flattens, false);
        admit(self.retained_bytes(), scratch, other, limit)?;
        // Bitmap's conversion begins the run itself (`convert_to_bitmap`), so
        // the stages before it poll the control outside a begun run, which
        // the control accepts (`gray_profile`'s Convert precedent), without
        // moving progress.
        let job = control
            .map_or(JobContext::NONE, JobContext::new)
            .checkpoints_only();
        job.reserve(scratch)?;
        // The flattened copy ends with its block, before Bitmap runs, so the
        // `beside` admission below holds only what is still alive.
        let gray = if flattens {
            let flattened = self
                .flatten_image(limit, other, job)?
                .ok_or("flattening the image changed nothing")?;
            flattened.duotone_gray_stage(limit, other, job)?
        } else {
            self.duotone_gray_stage(limit, other, job)?
        };
        // Bitmap runs while this document is still held (the caller's
        // source), so it is admitted beside it.
        let beside = other.saturating_add(self.retained_bytes());
        gray.convert_to_bitmap(&rest, limit, beside, control)
    }
}
