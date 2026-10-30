//! Stack Modes on the byte document: the thin adapter over
//! `stack_stat`, whose module doc is the rule and the admission record.
//!
//! The members' stored RGBA8 codes are read row by row through
//! `LayerRaster::row_segments` into one extent-wide row per member, and
//! `stack_stat::stack_row` writes the output plane; no member is made
//! contiguous. The result is ONE new raster named by the request at the
//! extent's origin, directly above `above` (above its whole subtree when it
//! is a group) at `above`'s level, unclipped, and every member that was
//! visible is hidden. Nothing else changes: a non-member, `above` included,
//! keeps its visibility. Lock-exempt, because it adds an entry and changes
//! visibility only (`doc_structure::stamp_visible`'s exemption).
//!
//! Refusals, each by a sentence: `StackModeRequest::members`' own; a member
//! out of range, a group or an adjustment layer (named by index); `above` out
//! of range; an extent that is empty, past the pixel cap or past the native
//! document capacity; the working limit; a cancel; and members with no pixel
//! anywhere. Never `Ok(None)`.

use image::RgbaImage;

use crate::doc::{DrDocument, LayerKind, MAX_PIXELS};
use crate::doc_geometry_set::is_adjustment;
use crate::job_control::{DrJobControl, JobContext};
use crate::precision_image::admit;
use crate::retained_source::SourcePlan;
use crate::stack_stat::{
    fill_row, plan_scratch, stack_row, union_extent, StackModeRequest, StackSample, StackScratch,
    NO_PIXELS,
};

/// What one request resolves to against a document.
struct Terms {
    members: Vec<usize>,
    /// The union of the members' pixel rectangles.
    extent: (i32, i32, u32, u32),
    plan: SourcePlan,
}

impl DrDocument {
    fn stack_mode_terms(&self, request: &StackModeRequest) -> Result<Terms, String> {
        let members = request.members()?;
        for &index in &members {
            let layer = self
                .layers
                .get(index)
                .ok_or_else(|| format!("stack mode layer {index} is out of range"))?;
            if layer.kind != LayerKind::Raster {
                return Err(format!(
                    "Stack Mode requires raster layers; layer {index} is a group"
                ));
            }
            if is_adjustment(layer.meta.as_deref()) {
                return Err(format!(
                    "Stack Mode requires raster layers; layer {index} is an adjustment layer"
                ));
            }
        }
        if request.above >= self.layers.len() {
            return Err(format!(
                "stack mode cannot insert above layer {}: it is out of range",
                request.above
            ));
        }
        let extent = union_extent(members.iter().map(|&index| {
            let layer = &self.layers[index];
            let (w, h) = layer.pixels.dimensions();
            (layer.offset.0, layer.offset.1, w, h)
        }))
        .ok_or("the stacked layers have no pixel extent")?;
        let pixels = u64::from(extent.2) * u64::from(extent.3);
        if pixels > MAX_PIXELS {
            return Err("the stacked layers' extent exceeds 100,000,000 pixels".into());
        }
        if !self.native_layers_fit_dimensions(
            self.layers
                .iter()
                .map(|l| l.pixels.dimensions())
                .chain(std::iter::once((extent.2, extent.3))),
        ) {
            return Err("the stack mode result exceeds the native document capacity".into());
        }
        let scratch = plan_scratch(
            pixels,
            u64::from(extent.2),
            4,
            members.len() as u64,
            self.cloned_structure_bytes(),
        )
        .ok_or("stack mode byte calculation overflow")?;
        let mut inventory = crate::resources::Inventory::default();
        inventory.document(self);
        let plan = SourcePlan {
            retained_bytes: inventory.unique_bytes(),
            scratch_bytes: scratch,
            work_units: pixels.saturating_mul(members.len() as u64),
        };
        Ok(Terms {
            members,
            extent,
            plan,
        })
    }

    /// The plan of [`Self::stack_mode`] as the shared plan JSON
    /// (`retained_bytes`, `scratch_bytes`, `work_units`). `Err` with the
    /// same sentence for every refusal the edit gives before it reads a pixel.
    pub fn stack_mode_plan(&self, request: &StackModeRequest) -> Result<serde_json::Value, String> {
        Ok(self.stack_mode_terms(request)?.plan.json())
    }

    /// A Stack Mode (module doc). Runs `geometry_edit`'s prologue — begin the
    /// run, plan, admit against `limit` beside `other_retained`, reserve,
    /// check — before any pixel work, then one checkpoint and one progress
    /// report per extent row. `Err` for every refusal and for a cancel; never
    /// `Ok(None)`.
    pub fn stack_mode(
        &self,
        request: &StackModeRequest,
        limit: u64,
        other_retained: u64,
        control: Option<&DrJobControl>,
    ) -> Result<Option<Self>, String> {
        let _run = control.map(DrJobControl::begin).transpose()?;
        let job = control.map_or(JobContext::NONE, JobContext::new);
        let terms = self.stack_mode_terms(request)?;
        admit(
            terms.plan.retained_bytes,
            terms.plan.scratch_bytes,
            other_retained,
            limit,
        )?;
        job.reserve(terms.plan.scratch_bytes)?;
        job.check()?;
        let refused = || "could not allocate the stack mode output".to_string();
        let (ex, ey, w, h) = terms.extent;
        let width = w as usize;
        let mut plane: Vec<u8> = Vec::new();
        let bytes = width
            .checked_mul(h as usize)
            .and_then(|n| n.checked_mul(4))
            .ok_or_else(refused)?;
        plane.try_reserve_exact(bytes).map_err(|_| refused())?;
        plane.resize(bytes, 0);
        let mut rows: Vec<Vec<[u8; 4]>> = Vec::new();
        for _ in &terms.members {
            let mut row = Vec::new();
            row.try_reserve_exact(width).map_err(|_| refused())?;
            row.resize(width, <[u8; 4]>::TRANSPARENT);
            rows.push(row);
        }
        let mut scratch = StackScratch::new(terms.members.len())?;
        let mut any = false;
        for y in 0..h {
            job.check()?;
            for (row, &index) in rows.iter_mut().zip(&terms.members) {
                let layer = &self.layers[index];
                let (lw, lh) = layer.pixels.dimensions();
                let ly = i64::from(ey) + i64::from(y) - i64::from(layer.offset.1);
                let at = usize::try_from(i64::from(layer.offset.0) - i64::from(ex));
                match at {
                    Ok(at) if lw > 0 && (0..i64::from(lh)).contains(&ly) => {
                        fill_row(row, at, layer.pixels.row_segments(ly as u32, 0, lw));
                    }
                    _ => row.fill(<[u8; 4]>::TRANSPARENT),
                }
            }
            let views: Vec<&[[u8; 4]]> = rows.iter().map(Vec::as_slice).collect();
            let start = y as usize * width * 4;
            let out = plane[start..start + width * 4].as_chunks_mut::<4>().0;
            any |= stack_row(request.mode, &views, out, &mut scratch);
            job.report(f64::from(y + 1) / f64::from(h) * 0.9, 6)?;
        }
        drop(rows);
        if !any {
            return Err(NO_PIXELS.into());
        }
        let image = RgbaImage::from_raw(w, h, plane).ok_or_else(refused)?;
        let at = crate::doc_group::subtree(&self.layers, request.above).end;
        let mut doc = self
            .adding_image_layer_with_job(
                request.above,
                image,
                &request.name,
                Some(job.range(0.9, 1.0)),
            )?
            .ok_or("the stack mode result could not be inserted")?;
        doc.layers[at].offset = (ex, ey);
        // Members sit at or below `above`, so the insert renumbered none.
        for &index in &terms.members {
            doc.layers[index].visible = false;
        }
        let doc = crate::doc_group::validated(doc)
            .ok_or("the stack mode result could not be inserted")?;
        job.report(1.0, 5)?;
        Ok(Some(doc))
    }
}
