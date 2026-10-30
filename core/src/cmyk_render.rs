//! The CMYK document's projections (#1 phases 1 and 2): the RGBA16
//! Display P3 VIEW (full and regional), one record on a transparent canvas,
//! the Layers panel thumbnail, the Channels panel's ink planes, the point
//! sample and the plans that admit each, and the descriptor.
//!
//! Every projection streams ROWS: the stack (`cmyk_stack`, the ONE row
//! compositor: raster records at opacity × mask, adjustment records) is
//! composited into one row of straight f32 ink and that row is turned into
//! its output straight away, so beside the output the only scratch is the
//! row, the stack's prepared masks and the view link's node table. The view is `cmyk_color::CmykViewLink` over the
//! STRAIGHT ink with the composite's alpha kept (a transparent pixel shows
//! as the host's transparency, not as paper). The ink planes are the
//! composite flattened onto paper (`cmyk_composite::on_paper`), shown ink
//! dark: `gray = 255 − round(255·ink)`.
//!
//! Thumbnails and planes reduced by `max_side` are BOX means over the source
//! pixels each output pixel covers (premultiplied by alpha for the colour,
//! so transparent ink never tints an edge), accumulated a source row at a
//! time into output-sized sums.

use std::sync::Arc;

use serde_json::{json, Value};

use crate::cmyk_color::CmykViewLink;
use crate::cmyk_composite::{codes, on_paper, over_row, InkPixel};
use crate::cmyk_document::CmykPolicy;
use crate::dr_image::DrImage;
use crate::job_control::{DrJobControl, JobContext};
use crate::layer_thumbnail::fitted_dims;
use crate::precision_document::PrecisionDocument;
use crate::precision_image::{admit, pixel_count, PrecisionImage, BOOKKEEPING_BYTES};
use crate::precision_render::PrecisionRenderPlan;
use crate::regional::{RegionRequest, RegionalPlan};
use crate::tile_store::TilePlane;

const OVERFLOW: &str = "CMYK projection working-memory calculation overflow";

impl PrecisionDocument {
    fn cmyk_view_policy(&self) -> Result<&Arc<CmykPolicy>, String> {
        self.require_cmyk_policy("the CMYK view")
    }

    pub(crate) fn view_pixel(view: &CmykViewLink, ink: InkPixel) -> [u16; 4] {
        let rgb = view.eval([ink[0], ink[1], ink[2], ink[3]]);
        let code = |v: f32| (v.clamp(0.0, 1.0) * 65535.0).round() as u16;
        [code(rgb[0]), code(rgb[1]), code(rgb[2]), code(ink[4])]
    }

    /// The view's plan: the RGBA16 output, one f32 ink row, the stack's
    /// masks, the view link and bookkeeping.
    pub(crate) fn cmyk_render_plan(&self) -> Result<PrecisionRenderPlan, String> {
        let (w, h) = self.dimensions();
        let pixels = pixel_count(w, h)? as u64;
        let output = pixels.checked_mul(8).ok_or(OVERFLOW)?;
        let row = u64::from(w) * 20;
        Ok(PrecisionRenderPlan {
            unique_retained_bytes: self.retained_bytes(),
            scratch_bytes: output
                + row
                + self.cmyk_stack_scratch_bytes()?
                + CmykPolicy::view_bytes()
                + BOOKKEEPING_BYTES,
            output_bytes: output,
            work_units: pixels * (self.root_layers().len() as u64 + 1),
            group_depth: 0,
        })
    }

    /// The full view (module doc), admitted by its plan.
    pub(crate) fn cmyk_render_with_context(
        &self,
        limit: u64,
        other: u64,
        job: JobContext<'_>,
    ) -> Result<PrecisionImage, String> {
        let (w, h) = self.dimensions();
        self.cmyk_view_window(0, 0, (w, h), 1, limit, other, job)
    }

    /// The view at canvas pixels `(x0 + i·step, y0 + j·step)` for the
    /// `w × h` output pixels (i, j): every one exactly the full view's pixel
    /// there (`CmykStack::row_strided`), `regional`'s SAMPLING rule. The
    /// scratch is over the OUTPUT pixels.
    #[allow(clippy::too_many_arguments)]
    fn cmyk_view_window(
        &self,
        x0: u32,
        y0: u32,
        (w, h): (u32, u32),
        step: u32,
        limit: u64,
        other: u64,
        job: JobContext<'_>,
    ) -> Result<PrecisionImage, String> {
        job.check()?;
        let scratch = pixel_count(w, h)? as u64 * 8
            + u64::from(w) * 20
            + self.cmyk_stack_scratch_bytes()?
            + CmykPolicy::view_bytes()
            + BOOKKEEPING_BYTES;
        admit(self.retained_bytes(), scratch, other, limit)?;
        job.reserve(scratch)?;
        let view = self.cmyk_view_policy()?.view()?;
        let mut stack = self.cmyk_stack(None, job)?;
        let mut acc = vec![[0f32; 5]; w as usize];
        let plane = TilePlane::<[u16; 4]>::from_rows_with_job(
            w,
            h,
            |y, row| {
                let cy = i64::from(y0) + i64::from(y) * i64::from(step);
                stack.row_strided(cy, i64::from(x0), step, &mut acc);
                for (dst, ink) in row.iter_mut().zip(acc.iter()) {
                    *dst = Self::view_pixel(&view, *ink);
                }
                job.report(f64::from(y + 1) / f64::from(h), 5)
            },
            job,
        )?;
        PrecisionImage::from_rgba16_tiles_unseeded(plane, self.profile().clone())
    }

    /// The regional view's plan: a window at any divisor d ≥ 1, output
    /// `ceil(w/d) × ceil(h/d)`, its scratch over the output pixels (one
    /// output-wide ink row; the stack reads only the sampled columns).
    pub(crate) fn cmyk_render_region_plan(
        &self,
        request: RegionRequest,
    ) -> Result<RegionalPlan, String> {
        let output = request.output_dims(self.dimensions())?;
        let pixels = u64::from(output.0) * u64::from(output.1);
        Ok(RegionalPlan {
            request,
            depth: "rgba16",
            reference: "dr_precision_doc_render",
            output,
            output_bytes: pixels * 8,
            scratch_bytes: pixels * 8
                + u64::from(output.0) * 20
                + self.cmyk_stack_scratch_bytes()?
                + CmykPolicy::view_bytes()
                + BOOKKEEPING_BYTES,
            work_units: pixels * (self.root_layers().len() as u64 + 1),
            max_output_bytes: None,
            layers: Vec::new(),
            halo_pixels: 0,
            canvas: self.dimensions(),
            group_depth: 0,
            global: Vec::new(),
            levels: Vec::new(),
        })
    }

    /// The window of the full view, sample for sample at divisor 1, and at a
    /// divisor d the full view sampled at `(x + i·d, y + j·d)`.
    pub(crate) fn cmyk_render_region(
        &self,
        request: RegionRequest,
        limit: u64,
        other: u64,
        control: Option<&DrJobControl>,
    ) -> Result<PrecisionImage, String> {
        let _run = control.map(DrJobControl::begin).transpose()?;
        let job = control.map_or(JobContext::NONE, JobContext::new);
        let plan = self.cmyk_render_region_plan(request)?;
        let (w, h) = plan.output;
        let out = self.cmyk_view_window(
            request.x,
            request.y,
            (w, h),
            request.preview_divisor,
            limit,
            other,
            job,
        )?;
        job.report(1.0, 5)?;
        Ok(out)
    }

    /// Record `index`'s own ink on a transparent canvas, as the view.
    pub(crate) fn cmyk_layer_canvas_image(
        &self,
        index: usize,
        limit: u64,
        other: u64,
        control: Option<&DrJobControl>,
    ) -> Result<PrecisionImage, String> {
        let _run = control.map(DrJobControl::begin).transpose()?;
        let job = control.map_or(JobContext::NONE, JobContext::new);
        let (w, h) = self.dimensions();
        let scratch = pixel_count(w, h)? as u64 * 8
            + u64::from(w) * 20
            + CmykPolicy::view_bytes()
            + BOOKKEEPING_BYTES;
        admit(self.retained_bytes(), scratch, other, limit)?;
        job.reserve(scratch)?;
        let view = self.cmyk_view_policy()?.view()?;
        let mut stack = self.cmyk_stack(Some(index), job)?;
        let mut acc = vec![[0f32; 5]; w as usize];
        let plane = TilePlane::<[u16; 4]>::from_rows_with_job(
            w,
            h,
            |y, row| {
                stack.row(i64::from(y), 0, &mut acc);
                for (dst, ink) in row.iter_mut().zip(acc.iter()) {
                    *dst = Self::view_pixel(&view, *ink);
                }
                Ok(())
            },
            job,
        )?;
        let out = PrecisionImage::from_rgba16_tiles_unseeded(plane, self.profile().clone())?;
        job.report(1.0, 5)?;
        Ok(out)
    }

    /// Box-reduces a `w × h` source, row by row, into a fitted RGBA8 image.
    /// `row(y, acc)` fills one source row of straight ink; `pixel` turns one
    /// box mean (straight ink, alpha) into RGBA8.
    #[allow(clippy::too_many_arguments)]
    fn cmyk_reduced(
        (w, h): (u32, u32),
        max_side: u32,
        limit: u64,
        retained: u64,
        other: u64,
        job: JobContext<'_>,
        mut row: impl FnMut(u32, &mut [InkPixel]),
        pixel: impl Fn(InkPixel) -> [u8; 4],
    ) -> Result<DrImage, String> {
        let (tw, th) = fitted_dims(w, h, max_side);
        let out_pixels = pixel_count(tw, th)? as u64;
        let scratch = out_pixels * (5 * 8 + 8 + 4)
            + u64::from(w) * 20
            + CmykPolicy::view_bytes()
            + BOOKKEEPING_BYTES;
        admit(retained, scratch, other, limit)?;
        job.reserve(scratch)?;
        let mut sums = vec![[0f64; 5]; out_pixels as usize];
        let mut counts = vec![0u32; out_pixels as usize];
        let mut acc = vec![[0f32; 5]; w as usize];
        for y in 0..h {
            job.check()?;
            row(y, &mut acc);
            let oy = (u64::from(y) * u64::from(th) / u64::from(h)) as usize;
            for (x, ink) in acc.iter().enumerate() {
                let ox = (x as u64 * u64::from(tw) / u64::from(w)) as usize;
                let at = oy * tw as usize + ox;
                let a = f64::from(ink[4]);
                for c in 0..4 {
                    sums[at][c] += f64::from(ink[c]) * a;
                }
                sums[at][4] += a;
                counts[at] += 1;
            }
        }
        let mut bytes = Vec::with_capacity(out_pixels as usize * 4);
        for (sum, count) in sums.iter().zip(counts.iter()) {
            let n = f64::from((*count).max(1));
            let a = sum[4] / n;
            let ink: InkPixel = std::array::from_fn(|c| {
                if c == 4 {
                    a as f32
                } else if sum[4] > 0.0 {
                    (sum[c] / sum[4]) as f32
                } else {
                    0.0
                }
            });
            bytes.extend_from_slice(&pixel(ink));
        }
        DrImage::from_rgba(tw, th, bytes).ok_or_else(|| "CMYK thumbnail has no pixels".into())
    }

    /// Record `index`'s Layers-panel thumbnail: its own pixels (its own
    /// extent, full opacity) through the view, fitted to `max_side`.
    pub fn cmyk_layer_thumbnail_rgba8(
        &self,
        index: usize,
        max_side: u32,
        limit: u64,
        other: u64,
        control: Option<&DrJobControl>,
    ) -> Result<DrImage, String> {
        let _run = control.map(DrJobControl::begin).transpose()?;
        let job = control.map_or(JobContext::NONE, JobContext::new);
        let view = self.cmyk_view_policy()?.view()?;
        let image = self
            .layer(index)
            .and_then(|layer| layer.cmyk_source())
            .ok_or_else(|| format!("CMYK layer index {index} is out of range"))?;
        let dims = image.dimensions();
        let out = Self::cmyk_reduced(
            dims,
            max_side,
            limit,
            self.retained_bytes(),
            other,
            job,
            |y, acc| {
                acc.fill([0.0; 5]);
                for (x, run) in image.row_segments(y, 0, dims.0) {
                    over_row(acc, x as usize, run, 1.0);
                }
            },
            |ink| {
                let p = Self::view_pixel(&view, ink);
                p.map(|v| ((u32::from(v) * 255 + 32767) / 65535) as u8)
            },
        )?;
        job.report(1.0, 5)?;
        Ok(out)
    }

    /// The Channels panel's ink plane `plane` (0–3 = C, M, Y, K) of the
    /// composite on paper, opaque gray, ink dark (module doc); `max_side` 0
    /// is full size.
    pub fn cmyk_plane_view(
        &self,
        plane: u32,
        max_side: u32,
        limit: u64,
        other: u64,
        control: Option<&DrJobControl>,
    ) -> Result<DrImage, String> {
        let _run = control.map(DrJobControl::begin).transpose()?;
        let job = control.map_or(JobContext::NONE, JobContext::new);
        self.cmyk_view_policy()?;
        if plane > 3 {
            return Err(
                "a CMYK ink plane is 0 (cyan), 1 (magenta), 2 (yellow) or 3 (black)".into(),
            );
        }
        let stack_scratch = self.cmyk_stack_scratch_bytes()?;
        admit(self.retained_bytes(), stack_scratch, other, limit)?;
        let mut stack = self.cmyk_stack(None, job)?;
        let c = plane as usize;
        let out = Self::cmyk_reduced(
            self.dimensions(),
            max_side,
            limit,
            self.retained_bytes(),
            other + stack_scratch,
            job,
            |y, acc| {
                stack.row(i64::from(y), 0, acc);
                // The plane reads the composite ON PAPER: carry the flattened
                // ink at full alpha so the box mean is over printed ink.
                for ink in acc.iter_mut() {
                    let paper = on_paper(*ink);
                    *ink = [paper[0], paper[1], paper[2], paper[3], 1.0];
                }
            },
            |ink| {
                let gray = 255 - (ink[c].clamp(0.0, 1.0) * 255.0).round() as u8;
                [gray, gray, gray, 255]
            },
        )?;
        job.report(1.0, 5)?;
        Ok(out)
    }

    /// The straight composite's ink at `(x, y)` (mean over the `reach`
    /// square, outside positions omitted — `PrecisionImage::sample`'s rule).
    /// The stack's prepared masks (canvas-sized, a feathered one blurred)
    /// and the one ink row are admitted against `limit` beside `other`
    /// first, as `cmyk_plane_view` admits them.
    pub fn cmyk_sample(
        &self,
        x: i32,
        y: i32,
        reach: u32,
        limit: u64,
        other: u64,
    ) -> Result<Value, String> {
        self.cmyk_view_policy()?;
        if reach > 2 {
            return Err("precision sample reach must be 0, 1 or 2".into());
        }
        let (w, h) = self.dimensions();
        let scratch = self
            .cmyk_stack_scratch_bytes()?
            .checked_add(u64::from(w) * 20)
            .ok_or(OVERFLOW)?;
        admit(self.retained_bytes(), scratch, other, limit)?;
        let mut stack = self.cmyk_stack(None, JobContext::NONE)?;
        let r = i64::from(reach);
        let mut sum = [0f64; 5];
        let mut count = 0u64;
        let mut acc = vec![[0f32; 5]; w as usize];
        for py in i64::from(y) - r..=i64::from(y) + r {
            if py < 0 || py >= i64::from(h) {
                continue;
            }
            stack.row(py, 0, &mut acc);
            for px in i64::from(x) - r..=i64::from(x) + r {
                if px < 0 || px >= i64::from(w) {
                    continue;
                }
                let code = codes(acc[px as usize]);
                for (slot, v) in sum.iter_mut().zip(code) {
                    *slot += f64::from(v);
                }
                count += 1;
            }
        }
        if count == 0 {
            return Err("CMYK sample is outside the image".into());
        }
        let mean: [f64; 5] = sum.map(|v| v / count as f64);
        let ink16: [u16; 4] = std::array::from_fn(|c| mean[c].round() as u16);
        let percent: [f64; 4] =
            std::array::from_fn(|c| (mean[c] / 65535.0 * 1000.0).round() / 10.0);
        let total = (mean[..4].iter().sum::<f64>() / 65535.0 * 1000.0).round() / 10.0;
        Ok(
            json!({"x":x,"y":y,"reach":reach,"ink_percent":percent,"ink16":ink16,
            "alpha16":mean[4].round() as u16,"total_ink_percent":total,"sample_count":count,
            "storage":crate::cmyk_document::STORAGE_CMYK}),
        )
    }

    /// The phase-2 edit plans the descriptor reports (`get_precision`'s
    /// CMYK `plans`, `scripts/cmyk-admission`'s figures): each edit's
    /// `scratch_bytes` on the top raster record — a canvas-wide `paint_blend`,
    /// a two-stop gradient, a bucket fill seeded at the origin, a destructive
    /// Curves, a destructive Hue/Saturation (`{"hue":40}`) — and a Curves
    /// record's add, a Hue/Saturation record's add, New Layer's, and (phase
    /// 4a) Merge Visible's and Stamp Visible's (`cmyk_merge`); since phase 5
    /// also New in CMYK of this canvas and profile, the proof through the
    /// document's own profile, the composite's tone analysis and the ⌥
    /// clipping display. `None`
    /// where the edit refuses (no raster record, a lock, a profile with no
    /// `B2A` for Hue/Saturation). Since the selection batch also the
    /// selections', Stroke…'s, Apply Layer Mask's and the cutout's.
    fn cmyk_edit_plans(&self) -> Value {
        let top = (0..self.layer_count())
            .rev()
            .find(|&i| self.layer(i).and_then(|l| l.cmyk_source()).is_some());
        let curves = r#"{"type":"adjust","evaluation":"precision_v1","op":"curves","params":{"cmyk":[[0,0],[128,150],[255,255]]}}"#;
        let huesat = r#"{"type":"adjust","evaluation":"precision_v1","op":"hue_saturation","params":{"hue":40}}"#;
        let paint = |request: String| -> Option<u64> {
            crate::cmyk_paint::CmykPaintEdit::parse(&request)
                .and_then(|edit| edit.plan(self))
                .ok()
                .and_then(|plan| plan["scratch_bytes"].as_u64())
        };
        let ink = "[0.2,0.4,0.6,0.1,1]";
        let (w, h) = self.dimensions();
        let on_top = |make: &dyn Fn(usize) -> String| top.and_then(|i| paint(make(i)));
        let add_layer = crate::precision_structure::StructureEdit::parse(&format!(
            r#"{{"op":"add_layer","name":"Layer","anchor":{},"position":"top"}}"#,
            self.layer_count().saturating_sub(1)
        ))
        .and_then(|edit| edit.scratch_bytes(self))
        .ok();
        // Phase 4a: Merge Visible's and Stamp Visible's plans (`cmyk_merge`).
        let structure = |request: String| {
            crate::precision_structure::StructureEdit::parse(&request)
                .and_then(|edit| edit.scratch_bytes(self))
                .ok()
        };
        let merge_visible = structure(r#"{"op":"merge_visible"}"#.to_string());
        let stamp_visible = structure(format!(
            r#"{{"op":"stamp_visible","name":"Stamp","anchor":{}}}"#,
            self.layer_count().saturating_sub(1)
        ));
        // #1 phase 5: New in CMYK of this canvas and profile, the proof through the
        // document's own profile, the composite's tone analysis and the ⌥ display.
        let policy = self.cmyk_policy();
        let canvas = u64::from(w) * u64::from(h);
        let new_cmyk = policy.map(|p| crate::cmyk_profile::new_cmyk_scratch(canvas, p));
        let proof = policy.and_then(|p| {
            let (bytes, refusal) = self.cmyk_proof_scratch(p.profile_bytes(), "{}");
            refusal.is_none().then_some(bytes)
        });
        let analysis = self
            .cmyk_tone_analysis_plan("{}")
            .ok()
            .and_then(|plan| plan["working_memory_plan_bytes"].as_u64());
        let clip = self.cmyk_clip_scratch().ok();
        let mut plans = json!({
            "cmyk_new_scratch_bytes": new_cmyk,
            "cmyk_proof_scratch_bytes": proof,
            "cmyk_tone_analysis_scratch_bytes": analysis,
            "cmyk_clip_display_scratch_bytes": clip,
            "cmyk_paint_scratch_bytes": on_top(&|i| format!(
                r#"{{"op":"paint_blend","layer":{i},"ink":{ink},"opacity":1}}"#)),
            "cmyk_gradient_scratch_bytes": on_top(&|i| format!(
                r#"{{"op":"gradient","layer":{i},"opacity":1,"from":[0,0],"to":[{w},{h}],"stops":[{{"position":0,"ink":{ink}}},{{"position":1,"ink":[0,0,0,1,1]}}]}}"#)),
            "cmyk_bucket_scratch_bytes": on_top(&|i| format!(
                r#"{{"op":"bucket_fill","layer":{i},"ink":{ink},"opacity":1,"x":0,"y":0,"tolerance":0,"contiguous":true}}"#)),
            "cmyk_adjust_apply_scratch_bytes": top.and_then(|i| self
                .cmyk_adjustment_plan(Some(i), curves, true).ok()
                .and_then(|plan| plan["scratch_bytes"].as_u64())),
            "cmyk_adjust_add_scratch_bytes": self.cmyk_adjustment_plan(None, curves, false).ok()
                .and_then(|plan| plan["scratch_bytes"].as_u64()),
            "cmyk_huesat_apply_scratch_bytes": top.and_then(|i| self
                .cmyk_adjustment_plan(Some(i), huesat, true).ok()
                .and_then(|plan| plan["scratch_bytes"].as_u64())),
            "cmyk_huesat_add_scratch_bytes": self.cmyk_adjustment_plan(None, huesat, false).ok()
                .and_then(|plan| plan["scratch_bytes"].as_u64()),
            "cmyk_add_layer_scratch_bytes": add_layer,
            "cmyk_merge_scratch_bytes": merge_visible,
            "cmyk_stamp_scratch_bytes": stamp_visible,
            // The styles batch (#1): Merge Down of the top raster onto the
            // record below it — a knockout or grouped one seeded (D7,
            // `cmyk_merge`) — as `merge_down` admits it.
            "cmyk_merge_down_scratch_bytes": top
                .filter(|&i| i > 0)
                .and_then(|i| self.merge_scratch_bytes(&[i - 1, i]).ok()),
        });
        // The selection batch: the source selections (composite), Stroke… on
        // the top raster's edge (width 10, outside), Apply Layer Mask on the
        // topmost masked raster and Select and Mask's New Layer. The stroke's
        // is the scan-free bound over the layer's stored extent: this runs on
        // every new document state.
        let select = |operation: &str| -> Option<u64> {
            crate::precision_selection::SelectionRequest::parse(&format!(
                r#"{{"op":"selection","operation":{operation}}}"#
            ))
            .and_then(|request| request.cmyk_plan(self))
            .ok()
            .and_then(|plan| plan["scratch_bytes"].as_u64())
        };
        let stroke = top.and_then(|i| {
            crate::cmyk_stroke::CmykLayerEdgeStroke::parse(&format!(
                r#"{{"op":"stroke_layer_edge","layer":{i},"width":10,"location":"outside","ink":{ink},"opacity":1}}"#
            ))
            .and_then(|edit| edit.scratch_bound(self))
            .ok()
        });
        let masked = (0..self.layer_count()).rev().find(|&i| {
            self.layer(i)
                .is_some_and(|l| l.cmyk_source().is_some() && l.mask().is_some())
        });
        let cutout = |strength: f32| {
            self.cmyk_cutout_plan(strength)
                .ok()
                .and_then(|plan| plan["scratch_bytes"].as_u64())
        };
        let more = json!({
            "cmyk_wand_scratch_bytes": select(r#"{"kind":"magic_wand","x":0,"y":0,"tolerance":0}"#),
            "cmyk_range_scratch_bytes": select(
                r#"{"kind":"color_range","params":{"mode":"sampled","samples":[[0,0,0]],"tolerance":20}}"#),
            "cmyk_quick_scratch_bytes": select(r#"{"kind":"quick","params":{"seeds":[[0,0]]}}"#),
            "cmyk_refine_scratch_bytes": select(
                r#"{"kind":"refine_edge","edge_radius":8,"smart_radius":true}"#),
            "cmyk_stroke_edge_scratch_bytes": stroke,
            "cmyk_apply_mask_scratch_bytes": masked.and_then(|i| self.apply_mask_plan(i).ok()),
            "cmyk_cutout_scratch_bytes": cutout(0.0),
            "cmyk_cutout_decontaminate_scratch_bytes": cutout(1.0),
            "cmyk_fade_scratch_bytes": self.cmyk_fade_scratch_bytes(),
            // The channels batch: a plate load, the composite's luminosity,
            // Similar and the wand over a backdrop.
            "cmyk_plate_load_scratch_bytes": select(r#"{"kind":"color","channel":"cyan"}"#),
            "cmyk_luminance_scratch_bytes": select(r#"{"kind":"luminance"}"#),
            "cmyk_similar_scratch_bytes": select(
                r#"{"kind":"similar","tolerance":0,"contiguous":true}"#),
            "cmyk_backdrop_wand_scratch_bytes": crate::precision_selection::SelectionRequest::parse(
                &format!(r#"{{"op":"selection","source":"backdrop","below":{},"operation":{{"kind":"magic_wand","x":0,"y":0,"tolerance":0}}}}"#,
                    self.layer_count()))
                .and_then(|request| request.cmyk_plan(self)).ok()
                .and_then(|plan| plan["scratch_bytes"].as_u64()),
        });
        if let (Value::Object(plans), Value::Object(more)) = (&mut plans, more) {
            plans.extend(more);
            plans.extend(self.cmyk_filter_descriptor_plans());
        }
        // Phase 4b: the geometry plans (`cmyk_geometry_plans`).
        if let (Value::Object(plans), Value::Object(more)) =
            (&mut plans, self.cmyk_geometry_plans())
        {
            plans.extend(more);
        }
        if let (Value::Object(plans), Value::Object(more)) =
            (&mut plans, self.cmyk_clipboard_plans())
        {
            plans.extend(more);
        }
        if let Value::Object(plans) = &mut plans {
            plans.extend(self.cmyk_retouch_plans()); // #1: the retouch batch's plans.
        }
        plans
    }

    /// The descriptor of a CMYK document: the RGB descriptor's keys a host
    /// capture reads, with the CMYK mode's own values.
    pub(crate) fn cmyk_descriptor(&self) -> Result<Value, String> {
        let policy = self.require_cmyk_policy("the CMYK descriptor")?;
        let render = self.cmyk_render_plan();
        let native = self.native_preflight()?;
        let save = self.cmyk_save_scratch_bytes().ok();
        let mut out = json!({"width":self.width,"height":self.height,
            "storage":crate::cmyk_document::STORAGE_CMYK,"color_mode":"cmyk",
            "cmyk_profile":policy.name(),"cmyk_depth":policy.depth().bits(),
            "view_profile":self.profile().name(),"evaluation":"cmyk_normal_v1","tone_map":Value::Null,
            "range":"normalized","resolution":{"x_ppi":self.resolution.x,"y_ppi":self.resolution.y},
            "layer_count":self.records,"channel_count":self.channels.len(),"inspection_bytes":0,
            "metadata_bytes":{"exif":self.metadata.exif.as_ref().map_or(0, |p|p.len()),"xmp":self.metadata.xmp.as_ref().map_or(0, |p|p.len()),"iptc":self.metadata.iptc.as_ref().map_or(0, |p|p.len())},
            "profile_bytes":policy.profile_bytes().len(),"retained_bytes":self.retained_bytes(),
            "render_scratch_bytes":render.as_ref().ok().map(|p|p.scratch_bytes),
            "render_output_bytes":render.as_ref().ok().map(|p|p.output_bytes),
            "display_scratch_bytes":self.display_scratch_bytes().ok(),
            "display_overhead_bytes":render.as_ref().ok().and_then(|p|self.display_overhead_bytes(p).ok()),
            "render_refusal":render.as_ref().err(),
            "native_scratch_bytes":native.write_scratch_bytes,"reopen_retained_bytes":native.reopen_retained_bytes,
            "decode_scratch_bytes":native.decode_scratch_bytes,"native_open_scratch_bytes":native.decode_scratch_bytes,
            "export_scratch_bytes":save,"cmyk_save_scratch_bytes":save,
            "cmyk_flatten_scratch_bytes":self.cmyk_flatten_scratch_bytes(),
            "cmyk_psd_save_scratch_bytes":self.cmyk_psd_save_scratch_bytes().ok(),
            "cmyk_open_bytes_per_pixel":crate::cmyk_io::OPEN_BYTES_PER_PIXEL,
            "supported_operations":crate::precision_capability::cmyk_operations(),
            "unsupported_operations":["everything else"]});
        if let (Some(out), Value::Object(plans)) = (out.as_object_mut(), self.cmyk_edit_plans()) {
            out.extend(plans);
        }
        Ok(out)
    }
}
