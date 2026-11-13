//! The 8-bit regional compositor's RUN-MAJOR walk (13A-latency): one run of
//! consecutive output samples of one row, evaluated UNIT-MAJOR — every unit
//! of a level over the whole run, bottom-first — instead of the whole tree
//! once per sample. The unstaged window loop (`doc_regional`) and the
//! retouch sampling plate's tile loop (`doc_regional_plate`) call it for a
//! level without D5's Background.
//!
//! **Why it is exact.** Without D5 every unit of the root level is sampled
//! against `KnockoutBackdrop::NONE`, and `ResolvedUnit::sample` reads and
//! writes only `acc[ai]` (every backdrop it births is a one-element copy of
//! that sample): no sample reads another's state, so the order ACROSS samples
//! is free, and the order of units PER sample — bottom-first — is kept. D5's
//! Background is not: its later units read the sample as it stood right after
//! the first unit, so a Background level keeps `ResolvedLevel::sample` per
//! sample (the callers' test).
//!
//! **Why it is faster.** A PLAIN unit — no positional clip run, a raster or
//! adjustment base without a style or a staged guide — hoists what
//! `ResolvedNode::composite` decides per sample (the source, the blend kind,
//! the layer rect against the run) out of the run and reads a raster's row
//! through `LayerRaster::row_segments`, one tile lookup per tile column
//! instead of one per sample. Its per-sample statements are exactly
//! `composite`'s plain raster and adjustment arms: `doc::composite_layer_sample`
//! and `doc::adjust_backdrop_sample`, with the same arguments in the same
//! order, the mask byte through `ResolvedNode::mask_byte`. Every other unit
//! runs `ResolvedUnit::sample` per sample of the run, unchanged. No pixel
//! math lives here: index arithmetic only.

use crate::adjust::Adjustment;
use crate::blend::blend_kind;
use crate::doc::{adjust_backdrop_sample, composite_layer_sample};
use crate::doc_evaluation::ByteTransfer;
use crate::doc_regional_tree::{ResolvedLevel, ResolvedNode, ResolvedUnit, Source};
use crate::style_knockout::KnockoutBackdrop;

/// `count` consecutive output samples of one row, written at
/// `acc[first..first + count]`: sample `i` reads canvas pixel `(origin.0 +
/// i·step, origin.1)` (`RegionRequest::sample`'s lattice, `step` the preview
/// divisor; 1 for the plate).
#[derive(Clone, Copy)]
pub(crate) struct SampleRun {
    pub(crate) first: usize,
    pub(crate) count: usize,
    pub(crate) origin: (i64, i64),
    pub(crate) step: i64,
}

impl SampleRun {
    /// The canvas pixel sample `i` reads.
    fn at(self, i: usize) -> (i64, i64) {
        (self.origin.0 + i as i64 * self.step, self.origin.1)
    }

    /// The samples `lo..hi` whose canvas x lies in `[x0, x0 + width)`, or
    /// `None` when none does: `lo` the least `i ≥ 0` with `origin.0 + i·step
    /// ≥ x0`, `hi` the least with `origin.0 + i·step ≥ x0 + width`, both
    /// clamped to the run.
    fn span(self, x0: i64, width: i64) -> Option<(usize, usize)> {
        let first_at_or_past = |x: i64| {
            let ahead = x - self.origin.0;
            if ahead <= 0 {
                0
            } else {
                usize::try_from((ahead + self.step - 1) / self.step).unwrap_or(usize::MAX)
            }
        };
        let lo = first_at_or_past(x0).min(self.count);
        let hi = first_at_or_past(x0 + width).min(self.count);
        (lo < hi).then_some((lo, hi))
    }
}

impl ResolvedLevel<'_> {
    /// [`ResolvedLevel::sample`] for every sample of `run` against
    /// `KnockoutBackdrop::NONE`, unit-major (module doc). Never for a level
    /// with D5's Background, whose later units read a per-sample snapshot.
    pub(crate) fn sample_run(
        &self,
        acc: &mut [[f32; 4]],
        run: SampleRun,
        transfer: Option<&ByteTransfer>,
    ) {
        debug_assert!(!self.background, "a D5 level is sampled per sample");
        for unit in &self.units {
            if !unit.plain_run(acc, run, transfer) {
                for i in 0..run.count {
                    unit.sample(
                        acc,
                        run.first + i,
                        transfer,
                        run.at(i),
                        KnockoutBackdrop::NONE,
                    );
                }
            }
        }
    }
}

impl ResolvedUnit<'_> {
    /// The run of a PLAIN unit (module doc), or `false` — nothing written —
    /// for any other, which the caller samples per sample.
    fn plain_run(
        &self,
        acc: &mut [[f32; 4]],
        run: SampleRun,
        transfer: Option<&ByteTransfer>,
    ) -> bool {
        let base = &self.base;
        if self.clip_run || base.style.is_some() || base.guide.is_some() {
            return false;
        }
        match &base.source {
            Source::Raster => raster_run(base, self.opacity, acc, run, transfer),
            Source::Adjustment(adjustment) => {
                adjustment_run(base, adjustment, self.opacity, acc, run, transfer)
            }
            _ => return false,
        }
        true
    }
}

/// `ResolvedNode::composite`'s plain raster arm over a run: nothing outside
/// the layer's rect (`pixel` is `None` there), else the mask byte and
/// `composite_layer_sample` on the stored pixel.
fn raster_run(
    node: &ResolvedNode<'_>,
    opacity: f32,
    acc: &mut [[f32; 4]],
    run: SampleRun,
    transfer: Option<&ByteTransfer>,
) {
    let layer = node.layer;
    let (lw, lh) = layer.pixels.dimensions();
    let (ox, oy) = (i64::from(layer.offset.0), i64::from(layer.offset.1));
    let ly = run.origin.1 - oy;
    if ly < 0 || ly >= i64::from(lh) {
        return;
    }
    let Some((lo, hi)) = run.span(ox, i64::from(lw)) else {
        return;
    };
    let kind = blend_kind(node.blend);
    // The layer columns the run's samples `lo..hi` read: the first, then
    // every `step`-th, the last one inside the layer.
    let lx0 = run.at(lo).0 - ox;
    let lx1 = run.at(hi - 1).0 - ox + 1;
    let (mut i, mut lx) = (lo, lx0);
    for (sx, segment) in layer.pixels.row_segments(ly as u32, lx0 as u32, lx1 as u32) {
        let raw = segment.as_flattened();
        let end = i64::from(sx) + segment.len() as i64;
        while lx < end {
            let canvas_xy = run.at(i);
            let coverage = node
                .mask_byte(canvas_xy)
                .map_or(1.0, |m| f32::from(m) / 255.0);
            let li = (lx - i64::from(sx)) as usize * 4;
            composite_layer_sample(
                acc,
                run.first + i,
                raw,
                li,
                coverage,
                opacity,
                kind,
                transfer,
                canvas_xy,
            );
            lx += run.step;
            i += 1;
        }
    }
    debug_assert_eq!(i, hi, "the segments cover the run's span");
}

/// `ResolvedNode::composite`'s adjustment arm over a run, with no staged
/// guide (its guide value is 0.0): unmasked, canvas-wide at `k = opacity`;
/// masked, nothing outside the layer's rect and `k = coverage · opacity`
/// inside it; nothing where `k ≤ 0`.
fn adjustment_run(
    node: &ResolvedNode<'_>,
    adjustment: &Adjustment,
    opacity: f32,
    acc: &mut [[f32; 4]],
    run: SampleRun,
    transfer: Option<&ByteTransfer>,
) {
    let kind = blend_kind(node.blend);
    let samples = match &node.mask {
        None => 0..run.count,
        Some(_) => {
            let layer = node.layer;
            let (lw, lh) = layer.pixels.dimensions();
            let ly = run.origin.1 - i64::from(layer.offset.1);
            if ly < 0 || ly >= i64::from(lh) {
                return;
            }
            match run.span(i64::from(layer.offset.0), i64::from(lw)) {
                Some((lo, hi)) => lo..hi,
                None => return,
            }
        }
    };
    for i in samples {
        let canvas_xy = run.at(i);
        let coverage = node
            .mask_byte(canvas_xy)
            .map_or(1.0, |m| f32::from(m) / 255.0);
        let k = coverage * opacity;
        if k <= 0.0 {
            continue;
        }
        adjust_backdrop_sample(
            &mut acc[run.first + i],
            adjustment,
            k,
            0.0,
            kind,
            transfer,
            canvas_xy,
        );
    }
}
