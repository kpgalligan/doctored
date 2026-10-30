//! The retouch SAMPLING PLATE through the byte regional tree (Phase
//! 13A-5c-2b): `flattened_cut(group_pixels, cut)` byte for byte, for a stack
//! holding groups and layer styles, without a canvas-sized f32 accumulator.
//!
//! **Contract.** `inpaint_source::plate_arm` routes here (`Tree`) every cut
//! stack whose contributing, non-group entries carry no guide-reading
//! Shadows/Highlights (the one operator the regional tree STAGES; those keep
//! the full-frame fallback). The regional admission
//! (`DrDocument::regional_admission`) runs ONCE over the whole canvas at
//! divisor 1 with the cut, and its tree is resolved ONCE over the canvas
//! clip — every mask derived, every style's planes rendered and every
//! materialized group rendered exactly as the regional render does — so D5 /
//! Shallow snapshots are taken per SAMPLE, materialization runs over the
//! CANVAS clip and the group budget is charged once, as the reference
//! charges it. The resolved tree is then sampled SAMPLE-MAJOR in tiles of
//! at most [`TILE_WIDTH`] × [`TILE_HEIGHT`], each tile's f32 accumulator
//! quantized by the full projection's own quantizer
//! (`doc_evaluation::encode_projection_with_job`) and copied into the plate.
//!
//! **Bounds.** Reservation: the caller's `base` (its live working set, the
//! plate's RGBA8 output included), plus `20 · tile_pixels` (the 16-byte f32
//! tile accumulator and its 4-byte RGBA8 encode), plus the admission's
//! `preparation_scratch` (masks, style planes, materializations and one
//! description parse — the regional plan's own terms). Cancellation: the
//! admission's description parse and walk poll the job (13A-5c-1), the
//! preparation polls inside each mask blur, plane render and
//! materialization (13A-5a / 13A-5b-i / 13A-5b-ii-a/b), and the tile loop
//! polls at the head of every tile and of every tile ROW, the encode once
//! per row — so a cancel inside the loop returns within one tile row of ROOT
//! samples through the WHOLE tree (≤ 1024 samples, `c_tree_row`). The plate
//! reports no progress (13A-5c-2a's decision): every report lands at the
//! range's start.
//!
//! `Tree` (13A-5c-2b): groups and layer styles go through the byte REGIONAL tree —
//! `regional_cut_plate` admits once over the canvas with the cut (`regional_admission`), resolves
//! once over the canvas clip and samples ≤ 1024×64 tiles (`TILE_WIDTH`/`TILE_HEIGHT`, `tile_pixels`
//! the one tile figure), reserving `base + 20·tile + preparation`, bound `T_f + c_tree_row` (one
//! full tile row of ROOT samples through the whole tree).

use image::RgbaImage;

use crate::doc::DrDocument;
use crate::doc_evaluation::encode_projection_with_job;
use crate::doc_group::RenderBudget;
use crate::doc_regional_progress::Run;
use crate::job_control::JobContext;
use crate::regional::RegionRequest;
use crate::style_composite::{CompositeEnv, Cut};
use crate::style_knockout::KnockoutBackdrop;

/// A tile's width: 1024 samples, so one tile row is the loop's cancel unit.
pub(crate) const TILE_WIDTH: u32 = 1024;
/// A tile's height: 64 rows (1024 × 64 = 64K accumulator pixels, 1 MiB).
pub(crate) const TILE_HEIGHT: u32 = 64;

/// min(TILE_WIDTH, w) · min(TILE_HEIGHT, h) — the plate's, the plan's and the tests' one tile figure.
pub(crate) fn tile_pixels(canvas: (u32, u32)) -> u64 {
    u64::from(canvas.0.min(TILE_WIDTH)) * u64::from(canvas.1.min(TILE_HEIGHT))
}

impl DrDocument {
    /// `flattened_cut(group_pixels, cut)` byte for byte, through the regional tree resolved ONCE
    /// over the canvas clip and sampled per tile; `base` is the caller's already-reserved working
    /// set. Errors: the admission's own (`regional`'s sentences), "operation cancelled",
    /// `DrJobControl::reserve`'s; a staged level is refused "the sampling plate admits no staged
    /// level" (unreachable through `plate_arm`).
    pub(crate) fn regional_cut_plate(
        &self,
        cut: Cut<'_>,
        group_pixels: u64,
        base: u64,
        control: JobContext<'_>,
    ) -> Result<RgbaImage, String> {
        control.check()?;
        let (width, height) = (self.width, self.height);
        let request = RegionRequest {
            x: 0,
            y: 0,
            width,
            height,
            preview_divisor: 1,
        };
        let admission =
            self.regional_admission(request, u64::MAX, group_pixels, cut, base, control)?;
        if !admission.plan.levels.is_empty() {
            return Err("the sampling plate admits no staged level".into());
        }
        let tile = tile_pixels((width, height));
        control.reserve(
            base.saturating_add(tile.saturating_mul(20))
                .saturating_add(admission.preparation_scratch),
        )?;
        let transfer = admission.transfer;
        let colors = self.style_colors();
        let env = CompositeEnv {
            cut,
            job: control.checkpoints_only(),
            ..self.composite_env(colors.as_ref(), transfer)
        };
        let clip = (0, 0, i64::from(width), i64::from(height));
        let budget = RenderBudget::new(group_pixels);
        let mut done = 0f64;
        let root = {
            let mut run = Run {
                sample: 0,
                done: &mut done,
                total: admission.plan.work_units.max(1) as f64,
            };
            admission.level.resolve(
                &self.layers,
                env,
                clip,
                &budget,
                control.checkpoints_only(),
                &mut run,
            )?
        };
        let mut out = RgbaImage::new(width, height);
        for y in (0..height).step_by(TILE_HEIGHT as usize) {
            for x in (0..width).step_by(TILE_WIDTH as usize) {
                control.check()?;
                let w = (width - x).min(TILE_WIDTH);
                let h = (height - y).min(TILE_HEIGHT);
                let mut acc = vec![[0f32; 4]; w as usize * h as usize];
                // SAMPLE-MAJOR: each root sample evaluates the whole tree, so
                // one tile row (≤ 1024 root samples) is the cancel unit.
                for j in 0..h {
                    control.check()?;
                    for i in 0..w {
                        let ai = j as usize * w as usize + i as usize;
                        root.sample(
                            &mut acc,
                            ai,
                            transfer,
                            (i64::from(x + i), i64::from(y + j)),
                            KnockoutBackdrop::NONE,
                        );
                    }
                }
                let encoded = encode_projection_with_job(&acc, w, h, transfer, control)?;
                let span = w as usize * 4;
                for row in 0..h as usize {
                    let dest = ((y as usize + row) * width as usize + x as usize) * 4;
                    let src = row * span;
                    out.as_mut()[dest..dest + span]
                        .copy_from_slice(&encoded.as_raw()[src..src + span]);
                }
            }
        }
        control.check()?;
        control.reserve(base)?;
        Ok(out)
    }
}
