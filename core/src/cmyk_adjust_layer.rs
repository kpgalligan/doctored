//! Adjustments in a CMYK document (#547): the adjustment record
//! (`Content::CmykAdjustment`, a [`CmykAdjustmentState`]), its constructor,
//! and the add, add-at, set and destructive-apply operations with their
//! plans. The ink math is `cmyk_adjust`'s (Levels, Curves, drawn curves and
//! Hue/Saturation) and, since the adjustments batch, `cmyk_adjust_ink`'s and
//! `cmyk_adjust_rgb`'s (every other op CMYK admits, the
//! `CmykAdjustment::Ink` variant), all evaluated by `CmykAdjustment::apply`
//! at the document's depth; the record composites in `cmyk_stack` as
//! `acc ← lerp(acc, f(acc), opacity × mask)` on the ink, alpha kept.
//!
//! The description is the RGB record's schema verbatim —
//! `{"type":"adjust","evaluation":"precision_v1","op","params"}`, kept in
//! its canonical serialization — with the ops and keys
//! `cmyk_adjust_rgb::parse_cmyk` admits (the one dispatch). Every other op
//! refuses by name, at parse and so at every adjustment export's CMYK arm;
//! an op that applies to pixels only (Shadows/Highlights, Replace
//! Color) refuses an add, add-at or set by its kernel's `layer_refusal`. A
//! destructive Shadows/Highlights builds its guide plane over the layer from
//! `[t, t, t, α]`, `t` the ink tone (`cmyk_adjust_ink::ink_tone`), through
//! `adjust_guide::precision_plane`, and charges it (the RGB plan's own guide
//! term, `PrecisionAdjustment::guide_plan_for_description`).
//!
//! Every plan carries the host's display preparation of the result beside
//! the operation's own bytes, as phase 1's do ([`DISPLAY_BYTES_PER_PIXEL`]):
//! the measured interval of `scripts/cmyk-admission` holds both.

use std::sync::Arc;

use serde_json::{json, Map, Value};

use crate::cmyk_adjust::CmykAdjustment;
use crate::cmyk_adjust_ink::{ink_tone, InkKernel};
use crate::cmyk_composite::{code, unit};
use crate::cmyk_image::CmykImage;
use crate::job_control::JobContext;
use crate::layer_insert::InsertPosition;
use crate::precision_document::{Content, PrecisionDocument, PrecisionDocumentLayer};
use crate::precision_image::{admit, pixel_count, BOOKKEEPING_BYTES};
use crate::precision_render::MAX_RECORDS;
use crate::resources::Inventory;
use crate::selected_adjustment::coverage_at;

/// The host's display preparation of a CMYK document after an adjustment
/// edit — the view's RGBA16 render and its display conversion — a canvas
/// pixel, charged by every plan here and by a CMYK New Layer's
/// (`precision_structure`). Measured by `scripts/cmyk-admission/measure.py`
/// at 24 MP: `adjust-add-16` 351 MB, `adjust-apply-16` 592 MB (beside the
/// rebuilt plane), `new-layer-16` 352 MB; 16 missed all three and was raised
/// to 20. #547 phase 5's `new-cmyk-16` (File ▸ New in CMYK) measured 404 MB,
/// which 20 missed (ratio 1.232), so it was raised to 21. A miss raises this
/// term.
pub(crate) const DISPLAY_BYTES_PER_PIXEL: u64 = 21;

/// Per-record allowance for what `Inventory` cannot see (the `Arc` headers
/// and the allocator's rounding), `precision_adjustment`'s `RECORD_SLACK`.
const RECORD_SLACK: u64 = 1024;

/// One CMYK adjustment record's state: its canonical description and the
/// parsed kernel, both shared by every document derived from it.
#[derive(Clone)]
pub struct CmykAdjustmentState {
    description: Arc<str>,
    kernel: Arc<CmykAdjustment>,
}

impl CmykAdjustmentState {
    /// Parses `description` (module doc). `Err` names why: malformed JSON,
    /// the wrong schema, or an op or key the CMYK mode refuses.
    pub fn new(description: &str) -> Result<Self, String> {
        if description.len() > crate::drdc::MAX_DRDC_META_LEN as usize {
            return Err("high-depth adjustment description exceeds 16 MiB".into());
        }
        let mut value: Value = serde_json::from_str(description)
            .map_err(|e| format!("invalid high-depth adjustment JSON: {e}"))?;
        let root = value
            .as_object_mut()
            .ok_or("high-depth adjustment must be an object")?;
        if let Some(key) = root
            .keys()
            .find(|k| !matches!(k.as_str(), "type" | "evaluation" | "op" | "params"))
        {
            return Err(format!("unknown high-depth adjustment parameter {key}"));
        }
        if root.get("type").and_then(Value::as_str) != Some("adjust")
            || root.get("evaluation").and_then(Value::as_str) != Some("precision_v1")
        {
            return Err(
                "high-depth adjustment requires type:adjust and evaluation:precision_v1".into(),
            );
        }
        let op = root
            .get("op")
            .and_then(Value::as_str)
            .ok_or("high-depth adjustment requires an operation")?
            .to_string();
        let params = root
            .entry("params")
            .or_insert_with(|| Value::Object(Map::new()))
            .as_object()
            .ok_or("adjustment params must be an object")?;
        let kernel = crate::cmyk_adjust_rgb::parse_cmyk(&op, params)?;
        Ok(Self {
            description: Arc::from(value.to_string()),
            kernel: Arc::new(kernel),
        })
    }

    /// The bytes parsing `description` may hold at its peak: the JSON tree
    /// (a `Value` array can double its capacity mid-growth) and bookkeeping.
    pub(crate) fn prepare_scratch_bytes(description: &str) -> u64 {
        description.len() as u64 * 64 + BOOKKEEPING_BYTES
    }

    pub fn description(&self) -> &str {
        &self.description
    }

    pub(crate) fn kernel(&self) -> &CmykAdjustment {
        &self.kernel
    }

    /// The op the record holds, as the inspection reports it.
    pub(crate) fn operation(&self) -> &'static str {
        match *self.kernel {
            CmykAdjustment::Levels { .. } => "levels",
            CmykAdjustment::Curves { .. } | CmykAdjustment::CurveMaps { .. } => "curves",
            CmykAdjustment::HueSaturation(_) => "hue_saturation",
            CmykAdjustment::Ink(ref kernel) => kernel.operation(),
        }
    }

    /// The record's measured retention: the description, the kernel and
    /// its curves, plus the per-record slack.
    pub(crate) fn retained_bytes(&self) -> u64 {
        self.description.len() as u64
            + std::mem::size_of::<CmykAdjustment>() as u64
            + self.kernel.heap_bytes()
            + RECORD_SLACK
    }

    pub(crate) fn record_resources(&self, inventory: &mut Inventory) {
        inventory.record(
            self.description.as_ptr() as usize,
            self.description.len(),
            "cmyk_adjustment",
            false,
        );
        inventory.object(&*self.kernel, "cmyk_adjustment", false);
    }
}

impl PrecisionDocumentLayer {
    /// The ONE CMYK adjustment record constructor.
    pub fn cmyk_adjustment(
        name: &str,
        state: CmykAdjustmentState,
        opacity: f32,
        visible: bool,
    ) -> Result<Self, String> {
        crate::precision_document::scalar_and_name(name, opacity)?;
        Ok(Self {
            id: crate::doc_identity::mint_layer_id(),
            retained_source: None,
            warp: None,
            generated: None,
            description: None,
            name: Arc::new(name.into()),
            opacity,
            blend_mode: crate::blend::BlendMode::Normal,
            visible,
            content: Content::CmykAdjustment(state),
            records: 1,
            depth: 0,
            mask: None,
            clipped: false,
            blend_if: None,
            style: None,
            label: 0,
            locks: 0,
            link: 0,
            open: true,
            reference: (0, 0),
        })
    }
}

impl PrecisionDocument {
    /// Record `index`'s CMYK adjustment state; `None` for every other record.
    pub(crate) fn cmyk_adjustment_state(&self, index: usize) -> Option<&CmykAdjustmentState> {
        match &self.layer(index)?.content {
            Content::CmykAdjustment(state) => Some(state),
            _ => None,
        }
    }

    /// The plan of an adjustment edit on a CMYK document,
    /// `{"retained_bytes","scratch_bytes"}`: `index` `None` appends, `Some`
    /// sets an adjustment record or (destructive) applies to a raster.
    /// Refuses what the edit would refuse, the description's op included.
    pub(crate) fn cmyk_adjustment_plan(
        &self,
        index: Option<usize>,
        description: &str,
        destructive: bool,
    ) -> Result<Value, String> {
        self.require_cmyk_policy("A CMYK adjustment")?;
        let state = CmykAdjustmentState::new(description)?;
        if let (CmykAdjustment::Ink(kernel), false) = (state.kernel(), destructive) {
            if let Some(refusal) = kernel.layer_refusal() {
                return Err(refusal);
            }
        }
        let (w, h) = self.dimensions();
        let canvas = pixel_count(w, h)? as u64;
        let mut scratch = CmykAdjustmentState::prepare_scratch_bytes(description)
            + 2 * BOOKKEEPING_BYTES
            + canvas * DISPLAY_BYTES_PER_PIXEL;
        match (index, destructive) {
            (None, false) => {
                if self.records >= MAX_RECORDS {
                    return Err("too many high-depth document records".into());
                }
            }
            (Some(index), false) => {
                if self.cmyk_adjustment_state(index).is_none() {
                    return Err("setting a CMYK adjustment requires an adjustment layer".into());
                }
            }
            (Some(index), true) => {
                let image = self
                    .layer(index)
                    .and_then(PrecisionDocumentLayer::cmyk_source)
                    .ok_or("a destructive CMYK adjustment requires a CMYK raster layer")?;
                let (lw, lh) = image.dimensions();
                let layer = pixel_count(lw, lh)? as u64;
                // The rebuilt plane and the caller's canvas coverage.
                scratch += layer * 10 + canvas * 2;
                // Shadows/Highlights' guide plane over the layer.
                if guided(state.kernel()).is_some() {
                    scratch +=
                        crate::adjust_precision::PrecisionAdjustment::guide_plan_for_description(
                            description,
                            lw,
                            lh,
                        )?
                        .0;
                }
            }
            (None, true) => {
                return Err("destructive adjustment requires a raster layer index".into())
            }
        }
        Ok(json!({"retained_bytes":self.retained_bytes(),"scratch_bytes":scratch}))
    }

    fn cmyk_admit_plan(
        &self,
        index: Option<usize>,
        description: &str,
        destructive: bool,
        limit: u64,
        other: u64,
        job: JobContext<'_>,
    ) -> Result<(), String> {
        let plan = self.cmyk_adjustment_plan(index, description, destructive)?;
        let scratch = plan["scratch_bytes"]
            .as_u64()
            .ok_or("invalid CMYK adjustment plan")?;
        admit(self.retained_bytes(), scratch, other, limit)?;
        job.reserve(scratch)
    }

    /// Appends a CMYK adjustment record at the top of the stack
    /// (`dr_precision_doc_add_adjustment`'s CMYK arm).
    pub(crate) fn cmyk_add_adjustment(
        &self,
        name: &str,
        description: &str,
        opacity: f32,
        limit: u64,
        other: u64,
        job: JobContext<'_>,
    ) -> Result<Option<Self>, String> {
        crate::precision_document::scalar_and_name(name, opacity)?;
        self.cmyk_admit_plan(None, description, false, limit, other, job)?;
        let state = CmykAdjustmentState::new(description)?;
        job.check()?;
        let mut layers = self.layers.as_ref().clone();
        layers.push(PrecisionDocumentLayer::cmyk_adjustment(
            name, state, opacity, true,
        )?);
        let next = self.rebuilt(self.width, self.height, layers, Vec::new())?;
        job.report(1.0, 5)?;
        Ok(Some(next))
    }

    /// A CMYK adjustment record at the slot `position` names relative to
    /// `anchor` (`dr_precision_doc_add_adjustment_at`'s CMYK arm); the slot
    /// rule is `layer_insert`'s.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn cmyk_add_adjustment_at(
        &self,
        name: &str,
        description: &str,
        opacity: f32,
        anchor: usize,
        position: InsertPosition,
        limit: u64,
        other: u64,
        job: JobContext<'_>,
    ) -> Result<Option<Self>, String> {
        crate::precision_document::scalar_and_name(name, opacity)?;
        let slot = self.insert_slot(anchor, position)?;
        self.cmyk_admit_plan(None, description, false, limit, other, job)?;
        let state = CmykAdjustmentState::new(description)?;
        job.check()?;
        let record = PrecisionDocumentLayer::cmyk_adjustment(name, state, opacity, true)?;
        let next = self.inserting_record(record, slot)?;
        job.report(1.0, 5)?;
        Ok(Some(next))
    }

    /// Replaces record `index`'s description; the same canonical
    /// description is `Ok(None)`.
    pub(crate) fn cmyk_set_adjustment(
        &self,
        index: usize,
        description: &str,
        limit: u64,
        other: u64,
        job: JobContext<'_>,
    ) -> Result<Option<Self>, String> {
        self.cmyk_admit_plan(Some(index), description, false, limit, other, job)?;
        let state = CmykAdjustmentState::new(description)?;
        job.check()?;
        let out = match self.cmyk_adjustment_state(index) {
            Some(old) if old.description() == state.description() => None,
            _ => self.replacing_content(index, Content::CmykAdjustment(state))?,
        };
        job.report(1.0, 5)?;
        Ok(out)
    }

    /// Applies the description to record `index`'s ink in place of a
    /// record (Image ▸ Adjustments ▸ Levels…/Curves…/Hue/Saturation… on a
    /// CMYK layer):
    /// `ink' = ink + (f(ink) − ink)·t` per ink with `t` the canvas
    /// `coverage` at the pixel (empty is the whole layer), alpha kept, a
    /// pixel at alpha 0 untouched. An identity description or an unchanged
    /// result is `Ok(None)`. Runs under a `Pixels` lock edit.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn cmyk_apply_adjustment(
        &self,
        index: usize,
        description: &str,
        coverage: &[u16],
        limit: u64,
        other: u64,
        job: JobContext<'_>,
    ) -> Result<Option<Self>, String> {
        crate::selected_adjustment::validate_coverage(self, coverage)?;
        self.under_locks(index, crate::doc_lock::EditKind::Pixels, |doc| {
            doc.cmyk_applying_unlocked(index, description, coverage, limit, other, job)
        })
    }

    fn cmyk_applying_unlocked(
        &self,
        index: usize,
        description: &str,
        coverage: &[u16],
        limit: u64,
        other: u64,
        job: JobContext<'_>,
    ) -> Result<Option<Self>, String> {
        self.cmyk_admit_plan(Some(index), description, true, limit, other, job)?;
        let state = CmykAdjustmentState::new(description)?;
        job.check()?;
        if state.kernel().is_identity() {
            job.report(1.0, 5)?;
            return Ok(None);
        }
        let layer = self
            .layer(index)
            .ok_or("high-depth layer index is out of range")?;
        let image = layer
            .cmyk_source()
            .ok_or("a destructive CMYK adjustment requires a CMYK raster layer")?;
        let offset = layer.offset();
        let (w, h) = image.dimensions();
        let canvas = self.dimensions();
        let depth_bits = self.require_cmyk_policy("A CMYK adjustment")?.depth.bits();
        let tone = state.kernel();
        // Shadows/Highlights reads its guide plane: the ink tone over the
        // layer, alpha-weighted (module doc).
        let guide = match guided(state.kernel()) {
            Some((_, radius)) => Some(crate::adjust_guide::precision_plane(
                w,
                h,
                radius,
                &|i| {
                    let (x, y) = ((i % w as usize) as u32, (i / w as usize) as u32);
                    let s = image.sample_at(x, y).unwrap_or([0; 5]);
                    let t = ink_tone([0, 1, 2, 3].map(|c| unit(s[c])));
                    [t, t, t, unit(s[4])]
                },
                job.range(0.02, 0.35),
            )?),
            None => None,
        };
        let guided = guided(state.kernel()).map(|(kernel, _)| kernel);
        let mut changed = false;
        let out = CmykImage::from_rows_with_job(
            w,
            h,
            |y, row| {
                let mut x = 0usize;
                for (_, run) in image.row_segments(y, 0, w) {
                    for sample in run {
                        let i = y as usize * w as usize + x;
                        let weight = coverage_at(coverage, canvas, (w, h), offset, i);
                        row[x] = if sample[4] == 0 || weight == 0 {
                            *sample
                        } else {
                            let ink = [0, 1, 2, 3].map(|c| unit(sample[c]));
                            let adjusted = match (guided, &guide) {
                                (Some(kernel), Some(plane)) => {
                                    kernel.apply_guided(ink, plane.at(x as u32, y))
                                }
                                _ => tone.apply(ink, depth_bits),
                            };
                            let t = f32::from(weight) / 65535.0;
                            let mixed: [u16; 4] = std::array::from_fn(|c| {
                                if t >= 1.0 {
                                    code(adjusted[c])
                                } else {
                                    code(ink[c] + (adjusted[c] - ink[c]) * t)
                                }
                            });
                            [mixed[0], mixed[1], mixed[2], mixed[3], sample[4]]
                        };
                        changed |= row[x] != *sample;
                        x += 1;
                    }
                }
                job.every(y as usize * w as usize)
            },
            job,
        )?;
        let result = if changed {
            self.replacing_content(index, Content::CmykRaster { image: out, offset })?
        } else {
            None
        };
        job.report(1.0, 5)?;
        Ok(result)
    }
}

/// The kernel and guide radius of an op that reads a guide plane
/// (Shadows/Highlights with an amount); `None` for every other op.
fn guided(kernel: &CmykAdjustment) -> Option<(&dyn InkKernel, f32)> {
    match kernel {
        CmykAdjustment::Ink(kernel) => kernel
            .guide_radius()
            .map(|radius| (kernel.as_ref(), radius)),
        _ => None,
    }
}
