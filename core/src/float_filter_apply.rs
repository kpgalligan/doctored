//! Filters on a raster layer of a 32-bit float document: the
//! float arm of a selected filter request, `selected_edit_plan` and
//! `selected_edit_unlocked` through `float_adjust_apply`'s dispatch.
//! Twenty-six run (`float_document::admits_filter`, the operator table's
//! `<op>_filter` rows): Gaussian Blur, Box Blur, Motion Blur, Radial Blur,
//! Lens Blur, Tilt-Shift, Offset, Lens Correction, Liquify, Depth Blur, Unsharp Mask,
//! High Pass, Surface Blur, Dust & Scratches, Reduce Noise, Add Noise, Add
//! Grain, Clouds, Difference Clouds, Emboss, Edge Detect, Pixelate, Sharpen,
//! Presence, the Develop Filter (every control of both) and Smart
//! Sharpen. Every other
//! filter (the automatic tones, `gaussian_blur`) keeps the u16 arm's refusal.
//!
//! This module is the ADAPTER. The arithmetic is the kernels' own, the same
//! functions the 8- and 16-bit paths call, asked for
//! `filter_kernels::SampleRange::Extended`: `filter_kernels::evaluated_in`
//! (the Gaussian family), `ops_spatial::SpatialKernel::plane_in` /
//! `combine_in` (Box, Motion, Radial, Lens Blur, Tilt-Shift, Offset, Lens
//! Correction) and `liquify::plane_in`. In the linear domain (`# Domains`)
//! they read the layer's stored samples, which are linear light: a blur of a
//! pixel far above white spreads its energy and lifts its neighbours above 1,
//! and Unsharp Mask and High Pass may write values below 0, which a float
//! layer keeps. Colour is never clamped to 0…1. The rule each filter with a
//! display-referred parameter uses is stated where it is computed: High
//! Pass's pivot and Unsharp Mask's threshold in `filter_kernels::evaluated_in`,
//! Lens Blur's highlight weight in `sampling_disc::disc_plane`, Lens
//! Correction's vignette in `sampling_lens`.
//!
//! # Domains
//! `float_filter_route::Route::domain` names each filter's domain. A
//! [`Domain::Linear`] filter reads the stored samples as above. A
//! [`Domain::Display`] filter (a parameter defined on display-encoded values:
//! Surface Blur, Dust & Scratches, Reduce Noise, Add Grain, the two Clouds,
//! Emboss, Edge Detect, Sharpen, Presence, Smart Sharpen and the Develop
//! Filter's display part) reads an ENCODED COPY of the layer, `encode(front(p))`
//! (`display_encoding`, the route's `front` being the Develop Filter's
//! linear stages), built once before the visible range, which is then the
//! copy's; the kernel runs on the copy, and each covered pixel's result is
//! `decode_changed` back to linear light before the coverage mix, which
//! mixes in linear light as every float filter does. A component the kernel
//! left alone keeps its stored bits. The Develop Filter's Vibrance and
//! Saturation then run on that decoded linear value (`Route::back`, the
//! Scaled display form), so the encoding is built whenever the route needs
//! it (`Route::needs_encoding`) and the encoded copy only in the display
//! domain. Add Noise draws its three deltas for every pixel in index order
//! before the coverage gate, as at 16 bits.
//!
//! Gaussian Blur (`blur`, key `sigma`) from radius 7 up is the fitted kernel
//! on the linear values (`float_filter_route::Route::GaussianBlur`,
//! `gaussian_blur_filter`), as at 8 and 16 bits. Below 7 it is
//! `Neighborhood::Gaussian` at the
//! request's σ, the kernel Unsharp Mask and High Pass read. The legacy path
//! the 16-bit `blur` takes there is not used: below σ4 it calls
//! `image::imageops::blur`, which clamps f32 samples to 0…1. In a float layer
//! that neighbourhood is the FIR at every σ, cut where its dropped tails are
//! under 1e-7 of the weight: the recursive Gaussian the other depths run from
//! σ4 up has an error relative to the brightest sample, which was measured
//! and missed the float bound (`sampling_gaussian::runs_fast`). Its work
//! grows with σ (`sampling_gaussian::half_width` has the cost), which the
//! plan's `K` charges through `neighborhood_plan_in`.
//!
//! # Range
//! The layer's visible RGB range (`precision_sampling::visible_range`) is
//! computed once before the kernel and carried in `Extended { visible }`.
//! Lens Correction and Liquify resample with Catmull-Rom, whose negative
//! lobes ring, so their colour is clamped to that range and never leaves what
//! the layer holds: the D3 exception of `float_document`'s module doc. Every
//! other filter is a mean with non-negative weights (the Gaussian family,
//! Box, Motion, Radial, Lens Blur, Tilt-Shift) or an exact move (Offset) and
//! cannot leave it; the Gaussian family's finish and the five means' share
//! the clamp, where it only absorbs rounding (an f32 quotient one ulp past
//! ±65504 would otherwise refuse). An output alpha below 2^-24
//! (`sampling::EXTENDED_ALPHA_FLOOR`) is transparent black, and alpha is at
//! most 1.
//!
//! # Writeback
//! The selection's coverage gates the write once, per pixel: weight 0 copies
//! the pixel bit for bit; weight 65535 takes the filter's sample verbatim;
//! between them a sample whose alpha bits equal the source's is
//! `float_adjust_apply::mix_float` (the straight lerp, alpha kept), and any
//! other is the premultiplied mix `α = a0 + t·(a1 − a0)`, `rgb = (c0·a0·(1 −
//! t) + c1·a1·t) / α` in f64 — `selected_adjustment::mix_codes`'s rule (the
//! master) with no clamp and no quantization — transparent black below the
//! floor. Radial Blur and Offset take their frame from the coverage's
//! bounding box as at 16 bits (`ops_spatial::coverage_frame`).
//!
//! A non-finite component refuses with the u16 arm's sentence, and a result
//! outside ±65504 refuses through `PrecisionImage::rebuilt_rgba32_extended`;
//! neither is repaired. A bitwise-unchanged result and an identity filter
//! give `None`.
//!
//! # Admission
//! `scratch = 6·BOOKKEEPING + max(K + ⌈K/4⌉, n·APPLY_BYTES_PER_PIXEL) +
//! display + coverage`. `K` is the kernel's own frameless plan at
//! `CONTROLLED_JOB_WORK` (`filter_kernels::neighborhood_plan_in`,
//! `SpatialKernel::plan`, `DevelopFilter::plan`, `liquify::plan`: the one home of
//! each formula; Relief and Pixelate `n·16 + BOOKKEEPING`, Add Noise 0), which
//! includes the n×16 plane the kernel returns. `display` is the display
//! domain's encoded copy, `n·16 + ⌈n·16/4⌉`, held through both stages (0 in
//! the linear domain). The second stage holds that plane beside the
//! n×16 output vector, then, the plane dropped, the output beside the n×16
//! tiles `rebuilt_rgba32_extended` copies it into: 32 bytes a pixel either
//! way, at 5/4 (`float_adjust_apply::APPLY_BYTES_PER_PIXEL`, 40). `coverage`
//! is the borrowed canvas coverage the host builds after the plan, `canvas·2`
//! at 5/4 (`float_adjust_apply::coverage_scratch`). There is no work ceiling
//! under a job control (`sampling_gaussian::destructive_work_cap`).
//!
//! Measured 2026-10-12 (Apple M2 Max, release, one case per process,
//! `MallocLargeCache=0`, E = `ri_interval_max_phys_footprint` − pre, three
//! sets of three runs, a baseline twin reading E 0;
//! `tests/float_adjust_apply_24mp.rs::measure`): the opaque 6000×4000 float
//! layer of `float_adjust_apply`'s table, each filter through
//! `dr_precision_doc_selected_edit` under a job control. Every plan is
//! 1,026,291,456 (the second stage binds at this size).
//!
//! | case | request | max E | plan / max E |
//! |---|---|---|---|
//! | blur2 | `blur` σ 2 | 770,015,808 | 1.333 |
//! | blur8 | `blur` r 8, the fitted kernel (remeasured 2026-10-13) | 769,737,256 | 1.333 |
//! | blur25 | `blur` r 25, the same (2026-10-13) | 769,016,360 | 1.335 |
//! | blur64 | `blur` r 64, the same (2026-10-13) | 768,426,536 | 1.336 |
//! | box | `box_blur` radius 1000 | 769,983,016 | 1.333 |
//! | motion | `motion_blur` 30°, distance 24 | 769,999,400 | 1.333 |
//! | radial | `radial_blur` amount 3, Good | 770,064,936 | 1.333 |
//! | lens_blur | `lens_blur` radius 100, brightness 50 | 769,983,016 | 1.333 |
//! | offset | `offset` (100, 50), wrap | 769,999,400 | 1.333 |
//! | lens_correction | three passes and a vignette (`sampling_lens`'s record) | 769,999,400 | 1.333 |
//! | liquify | one size-600 push across 4000 px | 768,475,712 | 1.335 |
//! | unsharp_c | `unsharp_mask` under a canvas coverage built inside the interval | 818,020,952 | 1.255 |
//! | unsharp_small_c | the same on a 2000×1500 layer of that canvas (plan 186,291,456) | 144,146,552 | 1.292 |
//!
//! E is 32 bytes a pixel plus about 2 MB whatever the kernel, and the
//! coverage's 2 bytes a canvas pixel beside it. The two covered cases are why
//! the coverage is charged at 5/4: charged once it gave 1,014,291,456 against
//! ⌈1.25 × 818,020,952⌉ = 1,022,526,190, and 174,291,456 against
//! ⌈1.25 × 144,146,552⌉ = 180,183,190 on the small layer, where a larger
//! per-pixel constant would not have helped (the coverage is the canvas's,
//! the pixels the layer's). `float_adjust_apply_24mp` pins each case's
//! ⌈1.25 × max E⌉.
//!
//! Display-referred filters, measured 2026-10-12 the same way (three runs a
//! case, the baseline twin reading E 0 three times). A display-domain filter
//! holds the n×16 encoded copy beside the 32 bytes a pixel above: E is 48
//! bytes a pixel, and its plan adds the copy at 5/4 (`n·16 + ⌈n·16/4⌉`), so
//! every one of them plans 1,506,291,456 here. Add Noise and Pixelate are
//! linear and plan as the first eleven.
//!
//! | case | request | max E | plan | plan / max E |
//! |---|---|---|---|---|
//! | surface | `surface_blur` radius 5, threshold 15 | 1,152,664,392 | 1,506,291,456 | 1.307 |
//! | dust | `dust_and_scratches` radius 4, threshold 0.05 | 1,152,664,392 | 1,506,291,456 | 1.307 |
//! | reduce_noise | `reduce_noise` radius 2, luminance 0.05 | 1,152,664,392 | 1,506,291,456 | 1.307 |
//! | add_noise | `add_noise` amount 0.2 | 770,032,168 | 1,026,291,456 | 1.333 |
//! | add_grain | `add_grain` amount 60, size 1.5 | 1,152,664,416 | 1,506,291,456 | 1.307 |
//! | clouds | `clouds` | 1,152,664,392 | 1,506,291,456 | 1.307 |
//! | diff_clouds | `difference_clouds` | 1,152,664,392 | 1,506,291,456 | 1.307 |
//! | emboss | `emboss` | 1,152,664,392 | 1,506,291,456 | 1.307 |
//! | edge | `edge_detect` | 1,152,664,392 | 1,506,291,456 | 1.307 |
//! | pixelate | `pixelate` block 8 | 770,015,784 | 1,026,291,456 | 1.333 |
//! | sharpen | `sharpen` amount 1.5 | 1,152,664,392 | 1,506,291,456 | 1.307 |
//! | presence | `presence` texture 50, clarity 50 | 1,152,697,160 | 1,506,291,456 | 1.307 |
//! | develop_filter | temperature, exposure, contrast, whites, blacks, texture, clarity | 1,440,924,704 | 1,955,077,216 | 1.357 |
//!
//! `develop_filter` with Texture and Clarity plans past the 2 GiB test pin with
//! the retained document (2,340,712,375), so it was measured at an 8 GiB
//! limit (`DR_FF_LIMIT`); the footprint does not depend on the limit, and a
//! 16 GB machine's working limit admits it.
//!
//! Measured 2026-10-12 the same way (three runs a case at
//! `DR_FF_LIMIT` 8 GiB, the baseline twin reading E 0 three times): Dehaze
//! on the encoded copy and the Develop Filter with every control.
//!
//! | case | request | max E | plan | plan / max E |
//! |---|---|---|---|---|
//! | presence_dehaze | `presence` texture 50, clarity 50, dehaze 50 | 1,152,697,160 | 1,592,455,776 | 1.382 |
//! | develop_filter_all | all thirteen controls nonzero | 1,536,934,992 | 2,075,077,216 | 1.350 |
//!
//! Measured 2026-10-13 the same way (three runs at `DR_FF_LIMIT` 8 GiB,
//! the baseline twin reading E 0): Smart Sharpen on the encoded copy with
//! every stage on. Its kernel plan K (`SmartSharpen::plan_in` at the float
//! range, 774,357,724) at 5/4 is just above the apply stage's 40 bytes a
//! pixel, so the plan is 7,947,155 above the other display filters'
//! 1,506,291,456.
//!
//! | case | request | max E | plan | plan / max E |
//! |---|---|---|---|---|
//! | smart_sharpen | lens radius 4, amount 200, noise 50, both fades 50 at width 50, radius 8 | 1,152,664,416 | 1,514,238,611 | 1.314 |
use serde_json::{json, Value};

use crate::filter_kernels::SampleRange;
use crate::float_adjust_apply::{coverage_scratch, mix_float, APPLY_BYTES_PER_PIXEL};
use crate::float_filter_route::{Domain, PlaneOutcome, Route};
use crate::job_control::JobContext;
use crate::ops_spatial::coverage_frame;
use crate::precision_document::{Content, PrecisionDocument};
use crate::precision_image::{admit, reserved_vec, BOOKKEEPING_BYTES};
use crate::precision_sampling::visible_range;
use crate::rng::SplitMix64;
use crate::sampling::{ColorRange, EXTENDED_ALPHA_FLOOR};
use crate::selected_adjustment::{coverage_at, validate_coverage, SelectedEdit};

/// The u16 arm's sentence for a non-finite filter sample
/// (`selected_adjustment::mix_codes`).
const NONFINITE: &str = "selected filter produced a nonfinite sample";

/// The range a PLAN asks its kernel in. A plan reads only which domain it is
/// (the Gaussian's tap count), never the colours, and it runs before the
/// layer's visible range is measured.
pub(crate) const PLAN_RANGE: SampleRange = SampleRange::Extended {
    visible: ColorRange {
        min: [0.0; 3],
        max: [0.0; 3],
    },
};

/// The coverage mix of one pixel (module doc, `# Writeback`). The caller
/// answers weight 0.
fn mixed(before: [f32; 4], after: [f32; 4], weight: u16) -> [f32; 4] {
    if weight == u16::MAX {
        return after;
    }
    if after[3].to_bits() == before[3].to_bits() {
        return mix_float(before, [after[0], after[1], after[2]], weight);
    }
    let t = f64::from(weight) / 65535.;
    let (a0, a1) = (f64::from(before[3]), f64::from(after[3]));
    let alpha = a0 + t * (a1 - a0);
    if alpha < f64::from(EXTENDED_ALPHA_FLOOR) {
        return [0.0; 4];
    }
    let mut out = [0.0, 0.0, 0.0, alpha as f32];
    for c in 0..3 {
        let (old, new) = (f64::from(before[c]), f64::from(after[c]));
        out[c] = ((old * a0 * (1. - t) + new * a1 * t) / alpha) as f32;
    }
    out
}

impl PrecisionDocument {
    /// `selected_edit_plan`'s float arm for a filter (module doc,
    /// `# Admission`): refused as the run refuses, before any allocation the
    /// plan charges.
    pub(crate) fn float_filter_plan(&self, edit: &SelectedEdit) -> Result<Value, String> {
        let image = self
            .layer_source(edit.layer)
            .ok_or("selected edit requires a raster layer")?;
        let dimensions = image.dimensions();
        let count = u64::from(dimensions.0) * u64::from(dimensions.1);
        let (cw, ch) = self.dimensions();
        let canvas = u64::from(cw) * u64::from(ch);
        let route = Route::parse(edit, || None)?;
        let kernel = match &route {
            Some(route) => route.kernel_plan(dimensions)?,
            None => 0,
        };
        // The display domain's encoded copy is held through both stages,
        // at 5/4 like everything else here.
        if route.as_ref().is_some_and(Route::needs_encoding) {
            self.display_encoding()?;
        }
        let display = match &route {
            Some(route) if route.domain() == Domain::Display => {
                let copy = count.checked_mul(16).ok_or("filter scratch overflow")?;
                copy + copy.div_ceil(4)
            }
            _ => 0,
        };
        let scratch = kernel
            .checked_add(kernel.div_ceil(4))
            .zip(count.checked_mul(APPLY_BYTES_PER_PIXEL))
            .map(|(kernel, apply)| kernel.max(apply))
            .and_then(|stage| stage.checked_add(6 * BOOKKEEPING_BYTES))
            .and_then(|n| n.checked_add(display))
            .and_then(|n| n.checked_add(coverage_scratch(canvas)?))
            .ok_or("filter scratch overflow")?;
        Ok(
            json!({"retained_bytes":self.retained_bytes(),"scratch_bytes":scratch,
            "output_bytes":count * 16,
            "input_samples":canvas,"input_width":cw,"input_height":ch}),
        )
    }

    /// `selected_edit_unlocked`'s float arm for a filter (module doc).
    pub(crate) fn float_filter_unlocked(
        &self,
        edit: &SelectedEdit,
        coverage: &[u16],
        limit: u64,
        other: u64,
        job: JobContext<'_>,
    ) -> Result<Option<Self>, String> {
        let scratch = self.float_filter_plan(edit)?["scratch_bytes"]
            .as_u64()
            .unwrap_or(u64::MAX);
        admit(self.retained_bytes(), scratch, other, limit)?;
        job.reserve(scratch)?;
        let job = job.with_reservation_floor(scratch);
        validate_coverage(self, coverage)?;
        let layer = self.layer(edit.layer).ok_or("invalid selected layer")?;
        let image = layer
            .source()
            .ok_or("selected edit requires raster source")?;
        let dimensions = image.dimensions();
        let (w, h) = dimensions;
        let canvas = self.dimensions();
        let weight_at = |i: usize| coverage_at(coverage, canvas, dimensions, layer.offset(), i);
        // The frame is derived now, on this layer's own raster, never
        // recorded; no coverage is the whole plane.
        let frame = || {
            if coverage.is_empty() {
                None
            } else {
                coverage_frame(dimensions, weight_at)
            }
        };
        let Some(route) = Route::parse(edit, frame)? else {
            job.report(1., 5)?;
            return Ok(None);
        };
        let float_only = "a 32-bit float filter requires f32 storage";
        let n = w as usize * h as usize;
        // The display encoding when the route needs it, and the encoded copy
        // of the layer in the display domain only (module doc, `# Domains`),
        // taken once of the route's front.
        let encoding = if route.needs_encoding() {
            Some(self.display_encoding()?)
        } else {
            None
        };
        let encoded = match (&encoding, route.domain()) {
            (Some(encoding), Domain::Display) => {
                let mut copy: Vec<[f32; 4]> = reserved_vec(n)?;
                for run in image.runs_rgba32().ok_or(float_only)? {
                    for p in run {
                        job.every(copy.len())?;
                        copy.push(encoding.encode(route.front(*p)));
                    }
                }
                Some(copy)
            }
            _ => None,
        };
        let visible = match &encoded {
            Some(copy) => visible_range(copy.iter().copied(), job.range(0., 0.05))?,
            None => visible_range(
                image
                    .runs_rgba32()
                    .ok_or(float_only)?
                    .flat_map(|run| run.iter().copied()),
                job.range(0., 0.05),
            )?,
        };
        let range = SampleRange::Extended { visible };
        let outcome = match &encoded {
            Some(copy) => route.plane(
                |i| copy[i],
                dimensions,
                &range,
                encoding.as_ref(),
                job.range(0.05, 0.65),
            )?,
            None => route.plane(
                |i| image.pixel(i),
                dimensions,
                &range,
                encoding.as_ref(),
                job.range(0.05, 0.65),
            )?,
        };
        let plane = match outcome {
            PlaneOutcome::Samples(plane) => Some(plane),
            PlaneOutcome::Pointwise => None,
            PlaneOutcome::Identity => {
                job.report(1., 5)?;
                return Ok(None);
            }
        };
        // Add Noise draws its deltas for every pixel in index order before
        // the coverage gate, as at 16 bits, so a selection never changes
        // the noise a pixel gets.
        let draws_everywhere = matches!(route, Route::Noise { .. });
        let fold = self.is_gray().then_some(draws_everywhere); // #1 2c, `gray_filter`
        let mut rng = SplitMix64::new(route.seed());
        let mut pixels = reserved_vec(n)?;
        let mut changed = false;
        let mut i = 0;
        for run in image.runs_rgba32().ok_or(float_only)? {
            for before in run {
                job.every(i)?;
                let weight = weight_at(i);
                let drawn =
                    draws_everywhere.then(|| route.evaluate(*before, None, &range, &mut rng));
                let after = if weight == 0 {
                    *before
                } else {
                    let source = encoded.as_ref().map_or(*before, |copy| copy[i]);
                    let sample = match drawn {
                        Some(sample) => sample,
                        None => route.evaluate(
                            source,
                            plane.as_ref().map(|plane| plane[i]),
                            &range,
                            &mut rng,
                        ),
                    };
                    if sample.iter().any(|v| !v.is_finite()) {
                        return Err(NONFINITE.into());
                    }
                    let sample = match (&encoding, &encoded) {
                        (Some(encoding), Some(_)) => route.back(
                            encoding.decode_changed(route.front(*before), source, sample),
                            encoding,
                        ),
                        (Some(encoding), None) => route.back(sample, encoding),
                        _ => sample,
                    };
                    let sample = fold.map_or(sample, |n| crate::gray_filter::fold(sample, n));
                    mixed(*before, sample, weight)
                };
                i += 1;
                changed |= after.map(f32::to_bits) != before.map(f32::to_bits);
                pixels.push(after);
            }
        }
        // Neither the kernel's plane nor the encoded copy is held while the
        // output is tiled.
        drop(plane);
        drop(encoded);
        job.check()?;
        let out = if changed {
            let image = image.rebuilt_rgba32_extended(w, h, pixels, self.profile().clone())?;
            self.replacing_content(
                edit.layer,
                Content::Raster {
                    image,
                    offset: layer.offset(),
                },
            )?
        } else {
            None
        };
        job.report(1., 5)?;
        Ok(out)
    }
}
