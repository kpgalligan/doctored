//! The CMYK retouch commands' sampling plate (#1, `cmyk_retouch`): WHICH
//! ink a Clone Stamp, Healing Brush, Spot Healing or Content-Aware Fill
//! reads, its Display P3 view for the host's source overlay, and the
//! descriptor's retouch plans.
//!
//! * Current is the source layer's own ink on paper at α 0 (deliberately raw,
//!   as `retouch_sampling` states it for RGB).
//! * Current & Below and All are the ONE row compositor (`cmyk_stack`, through
//!   `cmyk_frame_image`) over the canvas on a copy of the document cut by
//!   `retouch_sampling`'s membership rule (`retouch_cut16`, whose
//!   `ignore_adjustments` drops a CMYK adjustment record above the target as
//!   it drops an RGB one): each excluded record composites at opacity 0, so
//!   it contributes nothing and no record's visibility — and so no group's
//!   isolation — moves.
//! * The view is the plate through the document's view link
//!   (`cmyk_render::view_pixel`), RGBA16 in Display P3: a display adapter, never
//!   a sampling source.
//!
//! The descriptor's plans (`cmyk_render::cmyk_descriptor`) are the core plans
//! of the very requests `scripts/cmyk-admission/measure.py` sends on the top
//! CMYK raster: a clone; a heal and a spot heal over the 104 px square at the
//! canvas centre (one 100 px dab there, `NativeBrushStroke.healBounds`'
//! padding); Dodge/Burn; a 100 px smudge across the centre 3000 px wide; the
//! History Brush from the document itself; and Equalize.

use std::sync::Arc;

use serde_json::{json, Map, Value};

use crate::brush_coverage::BrushRequest;
use crate::cmyk_composite::unit;
use crate::cmyk_image::{CmykImage, PAPER};
use crate::job_control::JobContext;
use crate::precision_document::{Content, PrecisionDocument, PrecisionDocumentLayer};
use crate::precision_image::{admit, pixel_count, PrecisionImage, BOOKKEEPING_BYTES};
use crate::precision_paint_tools::NativePaintEdit;
use crate::precision_retouch::RetouchEdit;
use crate::retouch_sampling::RetouchSampling;
use crate::selected_adjustment::SelectedEdit;

/// Record tree `nodes` with every record `excluded` marks (postorder) at
/// opacity 0 (module doc's cut copy).
fn cut_tree(
    nodes: &[PrecisionDocumentLayer],
    cursor: &mut usize,
    excluded: &[bool],
) -> Vec<PrecisionDocumentLayer> {
    let mut out = Vec::with_capacity(nodes.len());
    for node in nodes {
        let mut next = node.clone();
        match &node.content {
            Content::Group(children) => {
                next.content = Content::Group(Arc::new(cut_tree(children, cursor, excluded)));
            }
            Content::PassThrough(children) => {
                next.content = Content::PassThrough(Arc::new(cut_tree(children, cursor, excluded)));
            }
            _ => {}
        }
        if excluded.get(*cursor).copied().unwrap_or(false) {
            next.opacity = 0.0;
        }
        *cursor += 1;
        out.push(next);
    }
    out
}

impl PrecisionDocument {
    /// What rendering `mode`'s plate holds beside the plate itself.
    pub(crate) fn cmyk_plate_scratch(&self, mode: RetouchSampling) -> Result<u64, String> {
        let (w, h) = self.dimensions();
        let canvas = pixel_count(w, h)? as u64;
        Ok(canvas * 10
            + if mode == RetouchSampling::Current {
                0
            } else {
                self.cmyk_merge_scratch_bytes(((0, 0), (w, h)))?
            })
    }

    /// The sampling plate of `mode` for record `index` (module doc), canvas
    /// sized, straight ink.
    pub(crate) fn cmyk_retouch_plate(
        &self,
        index: usize,
        mode: RetouchSampling,
        ignore_adjustments: bool,
        job: JobContext<'_>,
    ) -> Result<CmykImage, String> {
        let (w, h) = self.dimensions();
        if mode == RetouchSampling::Current {
            let record = self
                .layer(index)
                .ok_or("retouch source layer is out of range")?;
            let image = record
                .cmyk_source()
                .ok_or("retouch requires a raster source layer")?;
            let (iw, ih) = image.dimensions();
            let (ox, oy) = record.offset();
            return CmykImage::from_rows_with_job(
                w,
                h,
                |y, row| {
                    row.fill(PAPER);
                    let ly = i64::from(y) - i64::from(oy);
                    if !(0..i64::from(ih)).contains(&ly) {
                        return job.check();
                    }
                    let lx0 = (-i64::from(ox)).clamp(0, i64::from(iw));
                    let lx1 = (i64::from(w) - i64::from(ox)).clamp(0, i64::from(iw));
                    if lx0 < lx1 {
                        for (lx, run) in image.row_segments(ly as u32, lx0 as u32, lx1 as u32) {
                            let at = (i64::from(lx) + i64::from(ox)) as usize;
                            row[at..at + run.len()].copy_from_slice(run);
                        }
                    }
                    job.check()
                },
                job,
            );
        }
        if self.layer(index).and_then(|l| l.cmyk_source()).is_none() {
            return Err("retouch requires a raster source layer".into());
        }
        let excluded = crate::precision_retouch_source::retouch_cut16(
            &self.layers,
            index,
            mode,
            ignore_adjustments,
            self.records,
        );
        let mut cut = self.clone();
        cut.layers = Arc::new(cut_tree(&self.layers, &mut 0, &excluded));
        cut.cmyk_frame_image(((0, 0), (w, h)), None, job)
    }

    /// `dr_precision_doc_retouch_source` on CMYK: `mode`'s plate as the
    /// view (Display P3 RGBA16, a display adapter for the host's source
    /// overlay), admitted by its plan.
    pub(crate) fn cmyk_retouch_view(
        &self,
        index: usize,
        mode: RetouchSampling,
        ignore_adjustments: bool,
        limit: u64,
        other: u64,
        job: JobContext<'_>,
    ) -> Result<PrecisionImage, String> {
        let (w, h) = self.dimensions();
        let scratch = self.cmyk_plate_scratch(mode)?
            + pixel_count(w, h)? as u64 * 8
            + crate::cmyk_document::CmykPolicy::view_bytes()
            + 4 * BOOKKEEPING_BYTES;
        admit(self.retained_bytes(), scratch, other, limit)?;
        job.reserve(scratch)?;
        let plate =
            self.cmyk_retouch_plate(index, mode, ignore_adjustments, job.range(0.0, 0.8))?;
        let view = self.require_cmyk_policy("the CMYK view")?.view()?;
        let plane = crate::tile_store::TilePlane::<[u16; 4]>::from_rows_with_job(
            w,
            h,
            |y, row| {
                let mut x = 0usize;
                for (_, run) in plate.row_segments(y, 0, w) {
                    for s in run {
                        row[x] = Self::view_pixel(&view, s.map(unit));
                        x += 1;
                    }
                }
                job.check()
            },
            job.range(0.8, 1.0),
        )?;
        PrecisionImage::from_rgba16_tiles_unseeded(plane, self.profile().clone())
    }

    /// The descriptor's retouch plans on the top CMYK raster (module doc);
    /// `null` where the request refuses or no CMYK raster exists.
    pub(crate) fn cmyk_retouch_plans(&self) -> Map<String, Value> {
        let mut out = Map::new();
        let top = (0..self.layer_count())
            .rev()
            .find(|&i| self.layer(i).and_then(|l| l.cmyk_source()).is_some());
        let (w, h) = self.dimensions();
        let (cx, cy) = (i64::from(w / 2), i64::from(h / 2));
        let side = 104i64.min(i64::from(w)).min(i64::from(h));
        let (bx, by) = ((cx - 52).max(0), (cy - 52).max(0));
        let number = |value: Option<u64>| value.map_or(Value::Null, Value::from);
        let retouch = |request: Value| -> Value {
            let Some(layer) = top else {
                return Value::Null;
            };
            let mut request = request;
            request["layer"] = json!(layer);
            number(
                serde_json::from_value::<RetouchEdit>(request)
                    .ok()
                    .and_then(|edit| self.retouch_plan(&edit).ok())
                    .map(|plan| plan.scratch_bytes),
            )
        };
        let bounds = json!({"x":bx,"y":by,"width":side,"height":side});
        let with = |op: &str| {
            let mut request = bounds.clone();
            request["op"] = json!(op);
            request
        };
        out.insert(
            "cmyk_clone_scratch_bytes".into(),
            retouch(json!({"op":"retouch_clone"})),
        );
        out.insert(
            "cmyk_heal_scratch_bytes".into(),
            retouch(with("retouch_heal")),
        );
        out.insert(
            "cmyk_spot_heal_scratch_bytes".into(),
            retouch(with("spot_heal")),
        );
        let Some(layer) = top else {
            for key in ["dodge", "smudge", "history_brush", "equalize"] {
                out.insert(format!("cmyk_{key}_scratch_bytes"), Value::Null);
            }
            return out;
        };
        let scratch = |plan: Result<Value, String>| {
            number(plan.ok().and_then(|p| p["scratch_bytes"].as_u64()))
        };
        let dodge = NativePaintEdit::parse(
            &json!({"op":"dodge_burn","layer":layer,"exposure":0.5,"range":1,"burn":false})
                .to_string(),
        );
        out.insert(
            "cmyk_dodge_scratch_bytes".into(),
            scratch(dodge.and_then(|edit| self.cmyk_dodge_burn_plan(&edit))),
        );
        // A 100 px stroke across the centre, a dab every 50 px (the
        // footprint and the stage are the endpoints' and the diameter's). A
        // 200 px hard tip over 3000 px walks past `MAX_SMUDGE_WORK` (about
        // 465 million units), so the measured stroke is refused; 100 px is
        // about 203 million.
        let reach = (cx - 60).clamp(0, 1500);
        let dabs: Vec<f64> = (0..=(2 * reach / 50).max(1))
            .flat_map(|k| {
                [
                    (cx - reach + k * 50).min(cx + reach) as f64,
                    cy as f64,
                    100.0,
                    1.0,
                ]
            })
            .collect();
        let request = BrushRequest::parse(
            r#"{"version":1,"hardness":1.0,"roundness":1.0,"angle_degrees":0.0,"pixel":false}"#,
        );
        out.insert(
            "cmyk_smudge_scratch_bytes".into(),
            scratch(request.and_then(|r| self.cmyk_smudge_plan(layer, &r, &dabs, false, false))),
        );
        out.insert(
            "cmyk_history_brush_scratch_bytes".into(),
            scratch(self.cmyk_history_brush_plan(layer, self, layer, false)),
        );
        let equalize = SelectedEdit {
            layer,
            op: "equalize".into(),
            params: Map::new(),
            description: None,
            analysis_scope: Default::default(),
        };
        out.insert(
            "cmyk_equalize_scratch_bytes".into(),
            scratch(self.equalize_plan(&equalize)),
        );
        out
    }
}
