//! Filters ▸ Develop Filter's masks: ordered local adjustments on a
//! layer's pixels at 8 and 16 bits, the `develop_filter` request's `masks` array.
//! A request with no `masks` (or `masks: []`) never reaches this module: it is
//! `SpatialKernel::DevelopFilter`, bit for bit. A masked request is the boxed
//! `SelectedFilter::DevelopFilterMasked` / `ops_spatial::ByteFilter::DevelopFilterMasked`
//! arm (the `DepthBlur` pattern; `SpatialKernel` stays `Copy`).
//!
//! It adds no pixel math. The coverage is a RAW local's (`raw_local_coverage::
//! Command` into an RZLM `RawLocalMask`), feathered and weighted by density
//! through `raw_local_effective::EffectiveMask`, narrowed by a
//! `raw_local_range::LocalRange`; the value is `sampling_develop_filter::DevelopFilter`
//! and the blend `blend::gate_source_into`, all in Develop's per-local order.
//!
//! # The order
//!
//! 1. The global controls: `acc` = the thirteen-control `DevelopFilter` plane of
//!    the source (or the source itself when they are all 0).
//! 2. Each mask in request order, one live at a time:
//!    - its coverage: the shape (`Command::apply`), then each brush stroke as a
//!      `Command::Brush` over that prior, then `Command::Invert` when `invert`;
//!      a host-made `map` is read bilinearly (`sampling_radial::sample_exact`)
//!      to `round(v·65535)`;
//!    - `EffectiveMask::prepare` with the local's density and feather (no range
//!      and no tone: the range multiplies below, and the value is `DevelopFilter`);
//!    - the value: the local's twelve controls (Vibrance is global only) as a
//!      `DevelopFilter` plane over `acc`, so overlapping locals compound, as
//!      Develop's tone-only locals do; then its Sharpness and Noise Reduction
//!      (`local_detail::LocalDetail::plane_in`) over that plane, or over `acc`
//!      when the twelve are all 0;
//!    - the weight `EffectiveMask::at(i) × range.coverage(lab(base))`, the
//!      base being the layer's OWN samples quantized to u16, so a local never
//!      moves its own range (Develop's rule, `raw_local_range`'s module doc);
//!    - `gate_source_into(acc[i], value[i], weight)` where the weight is above
//!      0 and the value differs.
//!
//! # The mappings
//!
//! Coordinates are 0…1 of the target's width and height. A radial's `radius`
//! is `[rx, ry]`, `rx` in widths and `ry` in heights, each in (0, 2]; a brush
//! dab's `radius` is a share of the longer side. Every pixel-unit shape and
//! stroke passes `Command::validate`, so the brush limits and sentences are
//! Develop's (4096 dabs and 256 million evaluated pixels per stroke). Feather
//! 0…100 is `feather/100 · 0.04 · max(w, h)` pixels, at most 256
//! (`LocalSettings`' ceiling). A colour range's `sample: [x, y]` resolves to
//! `raw_local_range::window_mean` of the base about that point; `lab` is
//! taken as given. The range reads Lab through `range_profile` (the
//! document's ICC, base64, injected by the host), required when any mask has
//! a range.
//!
//! Generated shapes (`subject`, `people`, `sky`) are resolved by the app into
//! `map` shapes before the filter runs; [`DevelopFilterMasked::plan`] charges an
//! unresolved one at [`MAX_MAP_SAMPLES`] and [`DevelopFilterMasked::plane_in`]
//! refuses it ([`UNRESOLVED_REFUSAL`]).
//!
//! # The plan
//!
//! Scratch = max(global plan, 16n + maxₗ(LOCAL_BYTES_PER_PIXEL·n +
//! `LocalSettings` feather plan + max(the local's `DevelopFilter` plan, [16n when
//! that runs] + its `LocalDetail` plan) + map bytes)) + 2 bookkeeping MiB:
//! `acc` (n×16) is live beside one local at a time, whose coverage (the RZLM
//! n×2 beside a brush's n×4 field and its n×2 output) is
//! [`LOCAL_BYTES_PER_PIXEL`], and the detail stage reads the `DevelopFilter`
//! value plane. Work is the global's and each local's `DevelopFilter` and detail
//! work, the feather blur's and every stroke's evaluated pixels. A mask with
//! no detail plans exactly as before it existed. The measured record is on
//! [`LOCAL_BYTES_PER_PIXEL`].
//!
//! The request's parsing (each mask's keys, shape, strokes, range and
//! controls) is `develop_filter_masks_request`.
use crate::blend::gate_source_into;
use crate::develop_filter_masks_request::{controls, parse_local, FilterLocal, RangeColor, Shape};
use crate::filter_kernels::SampleRange;
use crate::icc::IccProfile;
use crate::icc_transform::MatrixTrc;
use crate::job_control::JobContext;
use crate::precision_image::{pixel_count, reserved_vec, BOOKKEEPING_BYTES};
use crate::raw_local_coverage::{BrushMode, Command, Dab};
use crate::raw_local_effective::{EffectiveMask, LocalSettings};
use crate::raw_local_mask::RawLocalMask;
use crate::raw_local_range::{lab, window_mean};
use crate::sampling_develop_filter::DevelopFilter;
use crate::sampling_radial::sample_exact;
use serde_json::{Map, Value};
use std::sync::Arc;

/// The byte cap of a masked `develop_filter` request's JSON text (the
/// `params_json` of `dr_image_filter_op`): sixteen 1024² generated maps in
/// base64 are about 22 MB at most, but a map's long side is 1024, so a 3:2
/// picture's is 1024 × 683, about 0.93 MB of base64, and sixteen of those
/// with brush strokes fit in 16 MiB.
pub(crate) const MAX_REQUEST_BYTES: usize = 16 << 20;
/// The most samples one generated map may carry (1024 × 1024).
pub(crate) const MAX_MAP_SAMPLES: u64 = 1 << 20;
/// The most masks one request may carry: a RAW layer's local count
/// (`raw_local_limits::MAX_RAW_LOCALS`).
const MAX_MASKS: usize = crate::raw_local_limits::MAX_RAW_LOCALS;
/// One live local's coverage per target pixel: the RZLM (n×2) beside a brush
/// stroke's f32 field (n×4) and the stroke's new RZLM (n×2). Fixed before
/// measuring; a miss raises it.
///
/// Record (`scripts/develop-filter-admission/measure.py` MASK: the Release
/// app at `DR_WORKING_LIMIT_BYTES` 8 GiB, `apply_filter develop_filter` with a
/// radial (Exposure, Clarity), a 50-dab brush (feather 50, Dehaze) and a shadow
/// luminance range (Shadows) on a one-layer 6000×4000 document of two-colour
/// Clouds; `MallocLargeCache=0`, E = `ri_interval_max_phys_footprint` − pre
/// around the one call, a fresh process per case, 3 runs + a baseline twin
/// each). The kernel plan is 2,345,721,452 (`filter_develop_filter_masks.rs`'s
/// pin), the brush local's.
/// - 8-bit: E 1,501,479,944 / 1,501,398,048 / 1,501,430,816 against
///   `filter_op_scratch_bytes` 2,937,394,695 plus the retained growth
///   185,941,778: 3,123,336,473, 2.080×. Twins 16,384 / 0 / 0.
/// - 16-bit: E 1,404,961,800 / 1,405,010,952 / 1,405,043,720 against
///   `selected_edit_plan` 2,592,012,908, 1.845×. Twins 0 / 0 / 0.
///
/// MASKD (the same method: one full-frame radial with Clarity
/// 50, Sharpness 60 and Noise Reduction 60; kernel plan 1,833,060,664, the
/// detail stage's, pinned beside MASK's). No constant was raised.
/// - 8-bit: E 1,681,540,272 / 1,681,818,824 / 1,681,704,088 against
///   2,296,568,710 plus the retained growth 192,003,858: 2,488,572,568,
///   1.480×. Twins 0 / 0 / 0.
/// - 16-bit: E 1,585,546,344 / 1,585,562,728 / 1,585,480,808 against
///   2,079,352,120, 1.311×. Twins 0 / 0 / 0.
const LOCAL_BYTES_PER_PIXEL: u64 = 8;
/// Feather 100 is this share of the target's longer side, in pixels.
const FEATHER_SHARE: f64 = 0.04;
/// `LocalSettings`' feather ceiling, in pixels.
const MAX_FEATHER_PIXELS: f64 = 256.0;

pub(crate) const FLOAT_REFUSAL: &str =
    "develop_filter masks are not available on a 32-bit float layer";
pub(crate) const SCALAR_REFUSAL: &str = "develop_filter masks edit layer pixels only";
pub(crate) const UNRESOLVED_REFUSAL: &str =
    "develop_filter subject, people and sky masks are made by the app before the filter runs";

/// The request keys beside the thirteen global controls.
pub(crate) const REQUEST_KEYS: [&str; 2] = ["masks", "range_profile"];
/// Whether `params` carries masks the filter must run: `masks` present and
/// not `[]`. The scalar route refuses these by name.
pub(crate) fn has_masks(params: &Map<String, Value>) -> bool {
    params
        .get("masks")
        .is_some_and(|m| m.as_array().is_none_or(|a| !a.is_empty()))
}

impl FilterLocal {
    /// Changes nothing: density 0, or every control and the detail at 0.
    fn identity(&self) -> bool {
        self.density == 0.0 || (self.adjust.identity() && self.detail.identity())
    }

    fn settings(&self, dimensions: (u32, u32)) -> LocalSettings {
        let long = f64::from(dimensions.0.max(dimensions.1));
        LocalSettings {
            enabled: true,
            density: self.density,
            feather: (self.feather / 100.0 * FEATHER_SHARE * long).min(MAX_FEATHER_PIXELS),
            range: None,
            tone: None,
        }
    }

    /// The shape as a pixel-unit command; `None` for a map or an unresolved
    /// generated shape.
    fn shape_command(&self, (w, h): (u32, u32)) -> Option<Command> {
        let (fw, fh) = (f64::from(w), f64::from(h));
        let at = |p: [f64; 2]| [p[0] * fw, p[1] * fh];
        Some(match &self.shape {
            Shape::Full => Command::Full {},
            Shape::Empty => Command::Empty {},
            Shape::Radial {
                center,
                radius,
                hardness,
            } => Command::Radial {
                center: at(*center),
                radius: [radius[0] * fw, radius[1] * fh],
                hardness: *hardness,
            },
            Shape::Linear { from, to } => Command::Linear {
                from: at(*from),
                to: at(*to),
            },
            Shape::Unresolved | Shape::Map { .. } => return None,
        })
    }

    /// Each stroke as a pixel-unit `Command::Brush`.
    fn stroke_commands(&self, (w, h): (u32, u32)) -> impl Iterator<Item = Command> + '_ {
        let (fw, fh) = (f64::from(w), f64::from(h));
        let long = fw.max(fh);
        self.strokes.iter().map(move |(add, dabs)| Command::Brush {
            mode: if *add {
                BrushMode::Add
            } else {
                BrushMode::Subtract
            },
            dabs: dabs
                .iter()
                .map(|d| Dab {
                    center: [d.center[0] * fw, d.center[1] * fh],
                    radius: d.radius * long,
                    hardness: d.hardness,
                    opacity: d.opacity,
                })
                .collect(),
        })
    }

    /// The coverage before feather and density, one RZLM under `job`.
    fn coverage(
        &self,
        dimensions: (u32, u32),
        job: JobContext<'_>,
    ) -> Result<RawLocalMask, String> {
        let (w, h) = dimensions;
        let steps = 1 + self.strokes.len() + usize::from(self.invert);
        let share = |k: usize| job.range(k as f64 / steps as f64, (k + 1) as f64 / steps as f64);
        let bytes = match (&self.shape, self.shape_command(dimensions)) {
            (_, Some(command)) => command.apply(None, w, h, share(0))?,
            (
                Shape::Map {
                    width,
                    height,
                    samples,
                },
                None,
            ) => map_coverage(dimensions, (*width, *height), samples, share(0))?,
            _ => return Err(UNRESOLVED_REFUSAL.into()),
        };
        let mut mask = RawLocalMask::from_blob(Arc::new(bytes))?;
        for (k, stroke) in self.stroke_commands(dimensions).enumerate() {
            let bytes = stroke.apply(Some(&mask), w, h, share(1 + k))?;
            mask = RawLocalMask::from_blob(Arc::new(bytes))?;
        }
        if self.invert {
            let bytes = Command::Invert {}.apply(Some(&mask), w, h, share(steps - 1))?;
            mask = RawLocalMask::from_blob(Arc::new(bytes))?;
        }
        Ok(mask)
    }
}

/// A host map read bilinearly onto the target's pixel centres
/// (`sample_exact`, the master bilinear read), each sample `round(v·65535)`.
fn map_coverage(
    (w, h): (u32, u32),
    (mw, mh): (u32, u32),
    samples: &[u8],
    job: JobContext<'_>,
) -> Result<Vec<u8>, String> {
    let mut bytes = RawLocalMask::empty_bytes(w, h)?;
    let read = |i: usize| {
        let v = f32::from(samples[i]) / 255.0;
        [v, v, v, 1.0]
    };
    let (sx, sy) = (f64::from(mw) / f64::from(w), f64::from(mh) / f64::from(h));
    let header = crate::raw_local_mask::RAW_LOCAL_HEADER;
    for y in 0..h {
        job.check()?;
        for x in 0..w {
            let i = y as usize * w as usize + x as usize;
            job.every(i)?;
            let q = ((f64::from(x) + 0.5) * sx, (f64::from(y) + 0.5) * sy);
            let v = sample_exact(&read, (mw as usize, mh as usize), q, true)[0];
            let code = (v.clamp(0.0, 1.0) * 65535.0).round() as u16;
            bytes[header + i * 2..header + i * 2 + 2].copy_from_slice(&code.to_le_bytes());
        }
        job.report((y + 1) as f64 / h as f64, 3)?;
    }
    Ok(bytes)
}

/// Filters ▸ Develop Filter with masks: the global controls, then each
/// local in order (module doc).
pub(crate) struct DevelopFilterMasked {
    global: DevelopFilter,
    locals: Vec<FilterLocal>,
    model: Option<MatrixTrc>,
}

impl DevelopFilterMasked {
    /// The whole `develop_filter` request with a non-empty `masks`: the thirteen
    /// global keys (`SelectedFilter::parse`'s ranges), at most [`MAX_MASKS`]
    /// strict mask objects and the optional `range_profile`, required (with a
    /// matrix/TRC model) when any mask has a range.
    pub(crate) fn parse(p: &Map<String, Value>) -> Result<Self, String> {
        let mut global = p.clone();
        let masks = global.remove("masks");
        let profile = global.remove("range_profile");
        let global = controls(global)?;
        let masks = masks
            .as_ref()
            .and_then(Value::as_array)
            .ok_or("masks must be an array of mask objects")?;
        if masks.len() > MAX_MASKS {
            return Err(format!("develop_filter takes at most {MAX_MASKS} masks"));
        }
        let locals = masks
            .iter()
            .enumerate()
            .map(|(i, m)| parse_local(i, m))
            .collect::<Result<Vec<_>, _>>()?;
        let model = match profile {
            None => None,
            Some(v) => {
                let bytes = v
                    .as_str()
                    .and_then(crate::adjust_lut::base64_decode)
                    .ok_or("range_profile must be a base64 ICC profile")?;
                IccProfile::parse(&bytes)
                    .ok_or("range_profile must be a base64 ICC profile")?
                    .model()
                    .cloned()
            }
        };
        if locals.iter().any(|l| l.range.is_some()) && model.is_none() {
            return Err(crate::adjust_calibrated::PROFILE_REFUSAL.into());
        }
        Ok(Self {
            global,
            locals,
            model,
        })
    }

    /// Changes nothing: the global controls all 0 and every local an
    /// identity (density 0 or every control at 0).
    pub(crate) fn identity(&self) -> bool {
        self.global.identity() && self.locals.iter().all(FilterLocal::identity)
    }

    /// `(scratch, work)` on a target of `dimensions` (module doc, `# The
    /// plan`), refused before any allocation: a target over the RAW local
    /// limit, a stroke Develop refuses (in its words), or work above `cap`
    /// (`DevelopFilter`'s sentence).
    pub(crate) fn plan(&self, dimensions: (u32, u32), cap: u64) -> Result<(u64, u64), String> {
        crate::raw_local_mask::dimensions(dimensions.0, dimensions.1)?;
        let n = pixel_count(dimensions.0, dimensions.1)? as u64;
        let (global_scratch, mut work) = if self.global.identity() {
            (0, 0)
        } else {
            self.global.plan(dimensions, u64::MAX)?
        };
        let mut local_peak = 0_u64;
        for local in self.locals.iter().filter(|l| !l.identity()) {
            if let Some(command) = local.shape_command(dimensions) {
                command.validate(dimensions.0, dimensions.1)?;
            }
            for stroke in local.stroke_commands(dimensions) {
                work = work.saturating_add(stroke.validate(dimensions.0, dimensions.1)?);
            }
            let (feather_scratch, feather_work) = local.settings(dimensions).plan_at(dimensions)?;
            let adjusts = !local.adjust.identity();
            let (adjust_scratch, adjust_work) = if adjusts {
                local.adjust.plan(dimensions, u64::MAX)?
            } else {
                (0, 0)
            };
            // The detail stage reads the value plane (n×16) when the twelve
            // controls run, else `acc` itself.
            let (detail_scratch, detail_work) = local.detail.plan(dimensions, u64::MAX)?;
            let value_scratch = if local.detail.identity() {
                adjust_scratch
            } else {
                adjust_scratch.max(
                    n.saturating_mul(if adjusts { 16 } else { 0 })
                        .saturating_add(detail_scratch),
                )
            };
            let map = match &local.shape {
                Shape::Map { samples, .. } => samples.len() as u64,
                Shape::Unresolved => MAX_MAP_SAMPLES,
                _ => 0,
            };
            work = work
                .saturating_add(feather_work)
                .saturating_add(adjust_work)
                .saturating_add(detail_work);
            local_peak = local_peak.max(
                n.saturating_mul(LOCAL_BYTES_PER_PIXEL)
                    .saturating_add(feather_scratch)
                    .saturating_add(value_scratch)
                    .saturating_add(map),
            );
        }
        if work > cap {
            return Err("develop_filter exceeds the bounded pixel-work limit".into());
        }
        let locals = if local_peak == 0 {
            0
        } else {
            n.saturating_mul(16).saturating_add(local_peak)
        };
        Ok((
            global_scratch
                .max(locals)
                .saturating_add(2 * BOOKKEEPING_BYTES),
            work,
        ))
    }

    /// The final straight samples at `Unit` (8 and 16 bits): the global
    /// plane, then each local blended in order (module doc). An unresolved
    /// generated shape refuses ([`UNRESOLVED_REFUSAL`]).
    pub(crate) fn plane_in(
        &self,
        read: impl Fn(usize) -> [f32; 4],
        dimensions: (u32, u32),
        cap: u64,
        job: JobContext<'_>,
    ) -> Result<Vec<[f32; 4]>, String> {
        self.plan(dimensions, cap)?;
        let live: Vec<&FilterLocal> = self.locals.iter().filter(|l| !l.identity()).collect();
        if live.iter().any(|l| matches!(l.shape, Shape::Unresolved)) {
            return Err(UNRESOLVED_REFUSAL.into());
        }
        let (w, h) = dimensions;
        let n = pixel_count(w, h)?;
        let base = |i: usize| -> [u16; 3] {
            let p = read(i);
            [0, 1, 2].map(|c| (p[c].clamp(0.0, 1.0) * 65535.0).round() as u16)
        };
        let steps = usize::from(!self.global.identity()) + live.len();
        let mut step = 0_usize;
        let mut share = || {
            let range = job.range(
                step as f64 / steps.max(1) as f64,
                (step + 1) as f64 / steps.max(1) as f64,
            );
            step += 1;
            range
        };
        let unit = SampleRange::Unit;
        let mut acc = if self.global.identity() {
            let mut acc = reserved_vec(n)?;
            for i in 0..n {
                job.every(i)?;
                acc.push(read(i));
            }
            acc
        } else {
            self.global
                .plane_in(&read, dimensions, cap, &unit, share())?
        };
        for local in live {
            let local_job = share();
            let range = match &local.range {
                None => None,
                Some(range) => {
                    let model = self
                        .model
                        .as_ref()
                        .ok_or(crate::adjust_calibrated::PROFILE_REFUSAL)?;
                    let sampled = match &range.color {
                        Some((RangeColor::Sample(at), ..)) => {
                            let x = ((at[0] * f64::from(w)) as u32).min(w - 1);
                            let y = ((at[1] * f64::from(h)) as u32).min(h - 1);
                            window_mean(model, dimensions, (x, y), |px, py| {
                                base(py as usize * w as usize + px as usize)
                            })
                        }
                        _ => [50.0, 0.0, 0.0],
                    };
                    let range = range.resolved(sampled);
                    range.validate()?;
                    Some((range, model))
                }
            };
            let mask = local.coverage(dimensions, local_job.range(0.0, 0.3))?;
            let effective = EffectiveMask::prepare(
                &mask,
                local.settings(dimensions),
                local_job.range(0.3, 0.4),
            )?;
            let value_job = local_job.range(0.4, 0.9);
            let value = match (local.adjust.identity(), local.detail.identity()) {
                (true, _) => local
                    .detail
                    .plane_in(|i| acc[i], dimensions, cap, value_job)?,
                (false, adjust_only) => {
                    let split = if adjust_only { 1.0 } else { 0.5 };
                    let value = local.adjust.plane_in(
                        |i| acc[i],
                        dimensions,
                        cap,
                        &unit,
                        value_job.range(0.0, split),
                    )?;
                    if adjust_only {
                        value
                    } else {
                        local.detail.plane_in(
                            |i| value[i],
                            dimensions,
                            cap,
                            value_job.range(split, 1.0),
                        )?
                    }
                }
            };
            let blend = local_job.range(0.9, 1.0);
            // The per-local blend of `raw_local_develop::develop` (the master
            // copy): the effective coverage times the range of the base's own
            // sample, through `gate_source_into` where the value differs.
            for (i, out) in acc.iter_mut().enumerate() {
                blend.every(i)?;
                let mut weight = effective.at(i)?;
                if weight == 0.0 {
                    continue;
                }
                if let Some((range, model)) = &range {
                    weight *= range.coverage(lab(model, base(i)));
                    if weight == 0.0 {
                        continue;
                    }
                }
                if value[i] != *out {
                    gate_source_into(out, value[i], weight);
                }
            }
            blend.report(1.0, 6)?;
        }
        Ok(acc)
    }
}
