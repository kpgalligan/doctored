//! Filters on one plane of a 32-bit float document: a layer
//! mask, a saved alpha channel, or one Red, Green, Blue or Alpha component of
//! an `Rgba32Float` layer, through `precision_scalar_edit`'s `scalar_edit`
//! request. The kernels are the layer route's own (`float_filter_route::
//! Route`: `parse`, `kernel_plan`, `plane`, `evaluate`), run on a one-channel
//! plane; this module adds no kernel and no route arm. A Grayscale document
//! comes here only for its layer mask (#1 phase 2c).
//!
//! # The plane rule
//! - **Read.** The plane is read as an opaque gray pixel `[v, v, v, 1]`, at
//!   stored alpha 0 too (the 16-bit adapter's rule and the colour-plane
//!   Fill's, `float_paint`'s `# Admission`).
//! - **Colour (R/G/B of a float layer).** `v` is the stored linear component.
//!   The route's own `Domain` applies: Linear reads `v`; Display encodes the
//!   gray triple (`DisplayEncoding::encode`), runs the kernel on the copy and
//!   `decode_changed`s it back. The range is `SampleRange::Extended { visible
//!   }`, where `visible` is `visible_range` over the read triples (or over the
//!   encoded copy). Nothing is clamped.
//! - **Coverage (mask, alpha channel, the layer's Alpha).** `v = code/65535`,
//!   or the stored alpha. It is never encoded in either domain, because
//!   coverage is not light (R4m). The range is Extended over the plane's own
//!   visible range, and the result is clamped to 0…1.
//! - **Result.** If `r == g == b` bit for bit, the result is `r`. Otherwise it
//!   is the Rec. 709 luma `LUMA_R·r + LUMA_G·g + LUMA_B·b` (`blend.rs`),
//!   computed in f64 and rounded once to f32.
//! - **Write.** Weight 0 keeps the stored bits; weight 65535 takes the
//!   result. Between those it is `v0·(1−t) + v1·t` in f64, rounded once (a
//!   mask or channel through the existing u16 walk). A non-finite result
//!   refuses with `float_filter_apply`'s sentence. A colour result outside
//!   ±65504 refuses through `rebuilt_rgba32_extended`. No alpha floor, as in
//!   the colour-plane Fill. An unchanged plane is `None`.
//! - **Frame.** Radial Blur and Offset take their frame from the selection's
//!   bounding box on the plane (`ops_spatial::coverage_frame`), as in the
//!   existing walk.
//! - **Add Noise on coverage.** On a Coverage plane, Add Noise takes the
//!   16-bit mask's rule, not the route's light-relative `v·(1 + δ)`: the
//!   additive, clamped `SelectedFilter::Noise` draw. A mask or channel calls
//!   `precision_scalar_filter::evaluate` itself (the 16-bit document's mask
//!   result exactly). The float Alpha runs `SelectedFilter::evaluate` on
//!   `[v, v, v, 1]` in index order with the request's seed. A Colour plane
//!   keeps the route's Add Noise.
//!
//! A Colour plane's Add Noise draws for every pixel in index order before the
//! coverage gate, as `float_filter_apply` does, so a selection never changes
//! the noise a pixel gets.
//!
//! # Layer-only filters
//! Liquify, Depth Blur and the Develop Filter have no one-plane form and
//! refuse by name ([`layer_only_refusal`]) on every plane, in the plan and in
//! the run, BEFORE `Route::parse`: its identity bypass and
//! `develop_filter_masks::FLOAT_REFUSAL` would otherwise answer first.
//!
//! # Admission
//! `n` is the plane's pixel count and `K` the route's `kernel_plan` (for Add
//! Noise on a Coverage plane, `precision_scalar_filter::plan`'s figure). The
//! kernel stage holds `K` beside the f32 input and output planes,
//! `⌈5/4·(K + n·8)⌉`. On a mask or channel (the u16 walk) that is added to
//! `ScalarEdit::plan`'s unchanged base. On a float Red/Green/Blue/Alpha it
//! REPLACES the base's `n·32` u16 term and is charged as a max against the
//! apply stage, as `float_filter_plan` does: `max(kernel stage,
//! n·APPLY_BYTES_PER_PIXEL) + display`. The apply stage is the pixel copy
//! beside the rebuilt tiles, 32 bytes a pixel at 5/4
//! (`float_adjust_apply::APPLY_BYTES_PER_PIXEL`); the output plane is dropped
//! before the rebuild, so it is never held beside both. `display` is the
//! encoded copy of a Display-domain colour plane, `⌈5/4·n·16⌉`, else 0; that
//! case asks `display_encoding` in the plan too, so the plan refuses as the
//! run does.
//!
//! Measured 2026-10-20 (Apple M2 Max, release, one case per process,
//! `MallocLargeCache=0`, E = `ri_interval_max_phys_footprint` − pre, two
//! sets of three runs a case, the baseline twin reading E 0 in each set;
//! `examples/float_plane_filter_admission.rs`): the opaque 6000×4000 float
//! layer of `tests/selected_color_planes_24mp.rs`, no selection.
//! `selected_color_planes_24mp` pins each case's ⌈1.25 × max E⌉, and the Red
//! blur's plan stays at or under the whole-layer `blur` σ 2 plan,
//! 1,026,291,456 (`float_filter_apply`'s table).
//!
//! | case | request | max E | plan | ⌈1.25 × max E⌉ | plan / max E |
//! |---|---|---|---|---|---|
//! | red_blur | `blur` σ 2 on Red | 576,373,168 | 968,388,608 | 720,466,460 | 1.680 |
//! | red_surface | `surface_blur` r 5, t 15 on Red | 960,578,256 | 1,448,388,608 | 1,200,722,820 | 1.508 |
//! | mask_blur | `blur` σ 2 on a canvas mask | 624,394,696 | 1,501,770,278 | 780,493,370 | 2.405 |
//!
//! The mask's plan keeps `ScalarEdit::plan`'s u16 base and adds the kernel
//! stage, so it charges more than it holds.
use std::sync::Arc;

use crate::blend::{LUMA_B, LUMA_G, LUMA_R};
use crate::display_encoding::DisplayEncoding;
use crate::filter_kernels::SampleRange;
use crate::float_adjust_apply::APPLY_BYTES_PER_PIXEL;
use crate::float_filter_route::{Domain, PlaneOutcome, Route};
use crate::icc::IccProfile;
use crate::job_control::JobContext;
use crate::ops_spatial::{coverage_frame, Frame};
use crate::precision_document::{Content, PrecisionDocument};
use crate::precision_image::reserved_vec;
use crate::precision_sampling::visible_range;
use crate::rng::SplitMix64;
use crate::selected_adjustment::{coverage_at, SelectedEdit};
use crate::selected_filter::SelectedFilter;

/// `float_filter_apply`'s sentence for a non-finite filter sample (its master
/// is `selected_adjustment::mix_codes`'s).
const NONFINITE: &str = "selected filter produced a nonfinite sample";
const DEPTH_BLUR_REFUSAL: &str =
    "depth blur edits layer pixels only: its depth guide, haze, warmth and saturation read the layer's colour";
const DEVELOP_FILTER_REFUSAL: &str =
    "the develop filter edits layer pixels only at 32 bits: its temperature, tint, vibrance and saturation act on colour, which one plane does not hold";
const FLOAT_ONLY: &str = "a 32-bit float filter requires f32 storage";
const OVERFLOW: &str = "filter scratch overflow";

/// Which side of the plane rule a plane takes (module doc).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FloatPlane {
    /// A mask, an alpha channel or a float layer's Alpha: clamped to 0…1.
    Coverage,
    /// A float layer's Red, Green or Blue: linear light, unclamped.
    Color,
}

/// Some(sentence) for an op with no one-plane form: liquify, depth_blur,
/// develop_filter (module doc, `# Layer-only filters`).
pub(crate) fn layer_only_refusal(op: &str) -> Option<&'static str> {
    match op {
        "liquify" => Some(crate::precision_scalar_filter::LIQUIFY_REFUSAL),
        "depth_blur" => Some(DEPTH_BLUR_REFUSAL),
        "develop_filter" => Some(DEVELOP_FILTER_REFUSAL),
        _ => None,
    }
}

/// The plane sample as the opaque gray pixel the kernels read.
fn gray(v: f32) -> [f32; 4] {
    [v, v, v, 1.0]
}

/// The plane rule's Result: `r` when the three components agree bit for
/// bit, else their Rec. 709 luma in f64, rounded once.
pub(crate) fn result_of(p: [f32; 4]) -> f32 {
    if p[0].to_bits() == p[1].to_bits() && p[1].to_bits() == p[2].to_bits() {
        return p[0];
    }
    (f64::from(LUMA_R) * f64::from(p[0])
        + f64::from(LUMA_G) * f64::from(p[1])
        + f64::from(LUMA_B) * f64::from(p[2])) as f32
}

/// Whether the request is Add Noise on a Coverage plane, which takes the
/// 16-bit mask's additive rule (module doc).
fn coverage_noise(edit: &SelectedEdit, plane: FloatPlane) -> bool {
    plane == FloatPlane::Coverage && edit.op == "add_noise"
}

/// The filter's stage scratch; refuses as the run refuses. When `stored_f32`
/// it REPLACES `ScalarEdit::plan`'s `n·32` u16 term; otherwise (mask/channel)
/// it is added to the base (module doc, `# Admission`).
pub(crate) fn plan(
    edit: &SelectedEdit,
    dimensions: (u32, u32),
    plane: FloatPlane,
    stored_f32: bool,
    doc: &PrecisionDocument,
) -> Result<u64, String> {
    if let Some(reason) = layer_only_refusal(&edit.op) {
        return Err(reason.into());
    }
    let n = u64::from(dimensions.0) * u64::from(dimensions.1);
    let (kernel, display) = if coverage_noise(edit, plane) {
        (
            crate::precision_scalar_filter::plan(edit, dimensions, doc.profile())?,
            0,
        )
    } else {
        match Route::parse(edit, || None)? {
            None => (0, 0),
            Some(route) => {
                let kernel = route.kernel_plan(dimensions)?;
                let display = if plane == FloatPlane::Color && route.needs_encoding() {
                    doc.display_encoding()?;
                    if route.domain() == Domain::Display {
                        let copy = n.checked_mul(16).ok_or(OVERFLOW)?;
                        copy + copy.div_ceil(4)
                    } else {
                        0
                    }
                } else {
                    0
                };
                (kernel, display)
            }
        }
    };
    let held = n
        .checked_mul(8)
        .and_then(|planes| planes.checked_add(kernel))
        .ok_or(OVERFLOW)?;
    let kernel_stage = held.checked_add(held.div_ceil(4)).ok_or(OVERFLOW)?;
    if !stored_f32 {
        return Ok(kernel_stage);
    }
    n.checked_mul(APPLY_BYTES_PER_PIXEL)
        .map(|apply| apply.max(kernel_stage))
        .and_then(|stage| stage.checked_add(display))
        .ok_or_else(|| OVERFLOW.into())
}

/// The route's result for every sample of `values` under the plane rule
/// (module doc): the kernel stage. `weight` is the write's gate, read only
/// to skip the combine where nothing is written (a weight-0 sample answers
/// its own value); Add Noise draws everywhere first. `encoding` is the
/// document's display encoding for a Colour plane whose route needs it.
/// `Ok(None)`: the route's plane is the identity.
fn filtered_values(
    route: &Route,
    values: &[f32],
    dimensions: (u32, u32),
    plane: FloatPlane,
    encoding: Option<&DisplayEncoding<'_>>,
    weight: impl Fn(usize) -> u16,
    job: JobContext<'_>,
) -> Result<Option<Vec<f32>>, String> {
    let n = values.len();
    let encoded = match (encoding, plane, route.domain()) {
        (Some(encoding), FloatPlane::Color, Domain::Display) => {
            let mut copy: Vec<[f32; 4]> = reserved_vec(n)?;
            for (i, &v) in values.iter().enumerate() {
                job.every(i)?;
                copy.push(encoding.encode(route.front(gray(v))));
            }
            Some(copy)
        }
        _ => None,
    };
    let visible = match &encoded {
        Some(copy) => visible_range(copy.iter().copied(), job.range(0., 0.05))?,
        None => visible_range(values.iter().map(|&v| gray(v)), job.range(0., 0.05))?,
    };
    let range = SampleRange::Extended { visible };
    let outcome = match &encoded {
        Some(copy) => route.plane(
            |i| copy[i],
            dimensions,
            &range,
            encoding,
            job.range(0.05, 0.65),
        )?,
        None => route.plane(
            |i| gray(values[i]),
            dimensions,
            &range,
            encoding,
            job.range(0.05, 0.65),
        )?,
    };
    let kernel_plane = match outcome {
        PlaneOutcome::Samples(samples) => Some(samples),
        PlaneOutcome::Pointwise => None,
        PlaneOutcome::Identity => return Ok(None),
    };
    let draws_everywhere = matches!(route, Route::Noise { .. });
    let mut rng = SplitMix64::new(route.seed());
    let mut out = reserved_vec(n)?;
    for (i, &v) in values.iter().enumerate() {
        job.every(i)?;
        let before = gray(v);
        let drawn = draws_everywhere.then(|| route.evaluate(before, None, &range, &mut rng));
        if weight(i) == 0 {
            out.push(v);
            continue;
        }
        let source = encoded.as_ref().map_or(before, |copy| copy[i]);
        let sample = match drawn {
            Some(sample) => sample,
            None => route.evaluate(
                source,
                kernel_plane.as_ref().map(|samples| samples[i]),
                &range,
                &mut rng,
            ),
        };
        if sample.iter().any(|c| !c.is_finite()) {
            return Err(NONFINITE.into());
        }
        let sample = match (encoding, &encoded) {
            (Some(encoding), Some(_)) => route.back(
                encoding.decode_changed(route.front(before), source, sample),
                encoding,
            ),
            (Some(encoding), None) => route.back(sample, encoding),
            _ => sample,
        };
        let result = result_of(sample);
        if !result.is_finite() {
            return Err(NONFINITE.into());
        }
        out.push(match plane {
            FloatPlane::Coverage => result.clamp(0.0, 1.0),
            FloatPlane::Color => result,
        });
    }
    job.check()?;
    Ok(Some(out))
}

/// Add Noise on a float layer's Alpha (module doc): `SelectedFilter`'s
/// additive, clamped draw on `[v, v, v, 1]` for every sample in index order
/// with the request's seed, then the Result rule, clamped.
fn alpha_noise(
    edit: &SelectedEdit,
    values: &[f32],
    job: JobContext<'_>,
) -> Result<Vec<f32>, String> {
    let filter = SelectedFilter::parse(&edit.op, &edit.params)?;
    let mut rng = SplitMix64::new(filter.seed());
    let mut out = reserved_vec(values.len())?;
    for (i, &v) in values.iter().enumerate() {
        job.every(i)?;
        let sample = filter.evaluate(i, gray(v), None, &mut rng);
        if sample.iter().any(|c| !c.is_finite()) {
            return Err(NONFINITE.into());
        }
        out.push(result_of(sample).clamp(0.0, 1.0));
    }
    job.check()?;
    Ok(out)
}

/// Mask/channel: u16 codes in, `65535·clamp(result, 0, 1)` per sample out;
/// `Ok(None)` = identity. A sample the filter left bit for bit keeps its
/// code. Add Noise is `precision_scalar_filter::evaluate`'s, the 16-bit
/// document's mask result exactly (module doc); `profile` is the document's,
/// which that adapter's image carries.
pub(crate) fn coverage_codes(
    edit: &SelectedEdit,
    codes: &[u16],
    dimensions: (u32, u32),
    frame: Option<Frame>,
    profile: &Arc<IccProfile>,
    job: JobContext<'_>,
) -> Result<Option<Vec<f64>>, String> {
    if let Some(reason) = layer_only_refusal(&edit.op) {
        return Err(reason.into());
    }
    if coverage_noise(edit, FloatPlane::Coverage) {
        return crate::precision_scalar_filter::evaluate(
            edit,
            codes,
            dimensions,
            profile,
            &[],
            frame,
            job,
        )
        .map(Some);
    }
    let Some(route) = Route::parse(edit, || frame)? else {
        return Ok(None);
    };
    let mut values = reserved_vec(codes.len())?;
    for (i, &code) in codes.iter().enumerate() {
        job.every(i)?;
        values.push(f32::from(code) / 65535.0);
    }
    let Some(filtered) = filtered_values(
        &route,
        &values,
        dimensions,
        FloatPlane::Coverage,
        None,
        |_| u16::MAX,
        job,
    )?
    else {
        return Ok(None);
    };
    let mut out = reserved_vec(codes.len())?;
    for (i, (&result, &v)) in filtered.iter().zip(&values).enumerate() {
        job.every(i)?;
        out.push(if result.to_bits() == v.to_bits() {
            f64::from(codes[i])
        } else {
            f64::from(result) * 65535.0
        });
    }
    Ok(Some(out))
}

impl PrecisionDocument {
    /// One component k (0..=3) of an Rgba32Float layer: k < 3 Color, k == 3
    /// Coverage (module doc). `selection` is the canvas coverage (empty: the
    /// whole plane). The caller has planned, admitted and reserved.
    pub(crate) fn float_plane_filter_unlocked(
        &self,
        layer: usize,
        component: usize,
        edit: &SelectedEdit,
        selection: &[u16],
        job: JobContext<'_>,
    ) -> Result<Option<PrecisionDocument>, String> {
        if let Some(reason) = layer_only_refusal(&edit.op) {
            return Err(reason.into());
        }
        let entry = self
            .layer(layer)
            .ok_or("native scalar layer is out of range")?;
        let image = entry
            .source()
            .ok_or("native color plane requires a raster layer")?;
        let dimensions = image.dimensions();
        let (w, h) = dimensions;
        let offset = entry.offset();
        let canvas = self.dimensions();
        let weight_at = |i: usize| coverage_at(selection, canvas, dimensions, offset, i);
        let plane = if component == 3 {
            FloatPlane::Coverage
        } else {
            FloatPlane::Color
        };
        let n = w as usize * h as usize;
        let mut values: Vec<f32> = reserved_vec(n)?;
        for run in image.runs_rgba32().ok_or(FLOAT_ONLY)? {
            for p in run {
                job.every(values.len())?;
                values.push(p[component]);
            }
        }
        let filtered = if coverage_noise(edit, plane) {
            Some(alpha_noise(edit, &values, job.range(0., 0.7))?)
        } else {
            // The frame is derived now, on this layer's own raster, never
            // recorded; no coverage is the whole plane.
            let frame = || {
                if selection.is_empty() {
                    None
                } else {
                    coverage_frame(dimensions, weight_at)
                }
            };
            let Some(route) = Route::parse(edit, frame)? else {
                job.report(1., 5)?;
                return Ok(None);
            };
            let encoding = if plane == FloatPlane::Color && route.needs_encoding() {
                Some(self.display_encoding()?)
            } else {
                None
            };
            filtered_values(
                &route,
                &values,
                dimensions,
                plane,
                encoding.as_ref(),
                weight_at,
                job.range(0., 0.7),
            )?
        };
        let Some(mut out) = filtered else {
            job.report(1., 5)?;
            return Ok(None);
        };
        let mut changed = false;
        for (i, (written, &v0)) in out.iter_mut().zip(&values).enumerate() {
            job.every(i)?;
            if i & 65535 == 0 {
                job.report(0.7 + 0.2 * i as f64 / n as f64, 6)?;
            }
            *written = match weight_at(i) {
                0 => v0,
                u16::MAX => *written,
                weight => {
                    let t = f64::from(weight) / 65535.;
                    (f64::from(v0) * (1. - t) + f64::from(*written) * t) as f32
                }
            };
            changed |= written.to_bits() != v0.to_bits();
        }
        drop(values);
        job.check()?;
        if !changed {
            job.report(1., 5)?;
            return Ok(None);
        }
        let mut pixels: Vec<[f32; 4]> = reserved_vec(n)?;
        for run in image.runs_rgba32().ok_or(FLOAT_ONLY)? {
            pixels.extend_from_slice(run);
        }
        for (i, (p, &v)) in pixels.iter_mut().zip(&out).enumerate() {
            job.every(i)?;
            p[component] = v;
        }
        // The output plane is not held beside the rebuilt tiles (module doc,
        // `# Admission`).
        drop(out);
        let image = image.rebuilt_rgba32_extended(w, h, pixels, self.profile().clone())?;
        let result = self.replacing_content(layer, Content::Raster { image, offset })?;
        job.report(1., 5)?;
        Ok(result)
    }
}
