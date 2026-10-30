//! Filters ▸ Develop Filter: the Develop panel's global controls on
//! an ordinary raster layer, as one destructive `SpatialKernel` arm on the one
//! spatial route (`SelectedFilter::Spatial` at 16 bits, `filter_op` at 8; no
//! new export). It adds no pixel math: every stage is an existing op, called
//! through its own type, in one fixed order.
//!
//! The thirteen controls ([`DEVELOP_FILTER_KEYS`]) are each −100…100 except
//! Exposure (−4…4 stops); 0 is off, and a stage whose controls are all 0 is
//! skipped, not evaluated. Samples are straight RGB in 0…1 in display terms;
//! alpha is never changed and a pixel with alpha 0 is returned as read.
//!
//! # The order
//!
//! 1. White balance (`adjust_white_balance::WhiteBalance`): Temperature `t`
//!    and Tint `τ` are RELATIVE shifts from "the picture is already neutral",
//!    because a JPEG, a scan or a composite has no as-shot white point. The
//!    op reads a kelvin as the illuminant and adapts it to D65, so `t` is one
//!    mired a unit away from D65: `T = 10⁶ / (10⁶/6504 − t)`, exactly 6504 at
//!    `t = 0` (+100 is 18,604 K and warms, −100 is 3,941 K and cools); `τ` is
//!    passed as the op's tint (+ toward magenta).
//! 2. Exposure (`adjust_tone::Exposure::gain_only`): a linear-light gain of
//!    the corrected scene before any curve, as Develop's decoder applies white
//!    balance and exposure first.
//! 3. Contrast (`adjust_mix::brightness_contrast_modern`): the global S-curve
//!    about 0.5 at power `2^(1.5·c/100)`, the modern Brightness/Contrast's.
//! 4. Highlights and Shadows (`adjust_tone::ShadowsHighlights::signed`): they
//!    recover the ends Contrast pushed, reading the toned neighbourhood: the
//!    guide is the alpha-weighted blurred luma of stage 3's output
//!    (`adjust_guide::precision_plane`, at both depths) at radius
//!    [`DevelopFilter::tone_radius`].
//! 5. Whites and Blacks (`adjust_levels::Levels`, master stage): the final
//!    endpoints, after the tone stages they bound.
//! 6. Presence (`sampling_presence::Presence`: Dehaze, Texture, Clarity), one
//!    `Presence::plane` call over the toned picture, so it equals Filters ▸
//!    Presence by construction, and Clarity's `4Y(1 − Y)` weight sees the
//!    final tones.
//! 7. Vibrance and Saturation (`adjust_color::Vibrance`), last because Dehaze
//!    and Clarity move saturation.
//!
//! The order matters: each stage reads luminance an earlier one changed
//! (`tests/filter_develop_filter.rs` shows Exposure then Contrast is not Contrast
//! then Exposure on a grey ramp).
//!
//! # The mappings
//!
//! - Highlights `h` and Shadows `s` are the op's band amounts at tone 0.5,
//!   with no Color and no midtone contrast: `a_s = s/100` for `s ≥ 0` and
//!   `s/200` below, `a_h = −h/100` for `h ≤ 0` and `−h/200` above. So +Shadows
//!   lifts and +Highlights brightens (`L^(1/2)` at full weight), and the
//!   negative side stops at half, where the exponent reaches 2.
//! - Whites `W` and Blacks `B` move the master stage's ends by at most
//!   [`ENDPOINT_SPAN`]: `W ≥ 0` sets `white = 1 − 0.2·W/100` (brighter), `W <
//!   0` sets `output_white = 1 + 0.2·W/100`; `B ≤ 0` sets `black =
//!   −0.2·B/100` (deeper), `B > 0` sets `output_black = 0.2·B/100`.
//! - Vibrance and Saturation are the op's two sliders at `v/100` and `σ/100`.
//!
//! # 32 bits
//!
//! On a 32-bit float layer each control runs under one stated rule
//! ([`DevelopFilter::split_float`] divides a request among the three domains):
//!
//! | control | rule at 32 bits |
//! |---|---|
//! | Temperature, Tint | `WhiteBalance::apply_linear` on the stored linear values |
//! | Exposure | `v·2^e` on the stored linear values |
//! | Contrast | `brightness_contrast_modern` on the display-encoded copy (a value outside 0…1 passes unchanged) |
//! | Highlights, Shadows | `ShadowsHighlights::apply_float` on the display-encoded copy (rule H): a pixel whose luma is at or above white is held, any other unclamped |
//! | Whites, Blacks | `Levels::apply_float` on the display-encoded copy, unclamped |
//! | Texture, Clarity | `Presence::plane_in` on the display-encoded copy, unclamped |
//! | Dehaze | `Presence::plane_in` on the display-encoded copy (`sampling_dehaze`'s rule D) |
//! | Vibrance, Saturation | the Scaled display form (`display_encoding`, A) with the unbound sRGB-curve `Vibrance` master (`Vibrance::apply`), on the decoded linear value (rule V) |
//!
//! The linear stages run first (they are the decoder's, before any curve);
//! the encoded copy is taken of their output (`display_encoding`), stages 3–6
//! run on it and the result is decoded back; stage 7 then runs on the linear
//! value through the Scaled form, as Image ▸ Adjustments ▸ Vibrance does at
//! float. A request whose display part is all zero takes no encoded copy.
//!
//! Not here, and not re-editable: running the filter again starts from the
//! current pixels. Develop's tone curve, shadow boost, local contrast,
//! sharpness, detail, noise reduction, profile corrections, highlight
//! recovery, moiré and baseline compensation are `CIRAWFilter` camera-space
//! processing with no core equivalent; output space and decoder are RAW-only;
//! vignette and grain are Lens Correction and Add Grain.
use crate::adjust_color::Vibrance;
use crate::adjust_guide::{precision_plane, GuidePlane};
use crate::adjust_levels::Levels;
use crate::adjust_mix::brightness_contrast_modern;
use crate::adjust_precision::plan::guide;
use crate::adjust_tone::{Exposure, ShadowsHighlights};
use crate::adjust_white_balance::WhiteBalance;
use crate::filter_kernels::SampleRange;
use crate::job_control::JobContext;
use crate::precision_image::{pixel_count, reserved_vec, BOOKKEEPING_BYTES};
use crate::sampling_presence::{Presence, REFERENCE_SIDE};
use serde_json::{json, Map, Value};

/// The controls, in the sheet's order and the order `SelectedFilter::parse`
/// lists them.
pub(crate) const DEVELOP_FILTER_KEYS: [&str; 13] = [
    "temperature",
    "tint",
    "exposure",
    "contrast",
    "highlights",
    "shadows",
    "whites",
    "blacks",
    "texture",
    "clarity",
    "dehaze",
    "vibrance",
    "saturation",
];

/// The white the relative Temperature is measured from: D65's CCT, the kelvin
/// `WhiteBalance` maps to the exact identity.
const NEUTRAL_KELVIN: f64 = 6504.0;
/// One Temperature unit is one mired (10⁶/K): equal steps in mireds are equal
/// perceived steps in warmth, which equal steps in kelvin are not, and ±100
/// lands inside the op's 1667–25000 K range on both sides.
const MIRED: f64 = 1.0e6;
/// The modern Brightness/Contrast's contrast exponent per unit: power
/// `2^(1.5·c/100)`, a midtone slope of 2.83 at +100 and 0.35 at −100, with 0
/// and 1 fixed.
const CONTRAST_STOPS: f32 = 1.5;
/// How far Whites and Blacks can move an endpoint, as a fraction of the range:
/// 0.2 (51 8-bit levels) at ±100. Wider and the two sliders become a Levels
/// dialog that clips a fifth of the range at once; narrower and they cannot
/// reach a washed-out scan's real black.
const ENDPOINT_SPAN: f64 = 0.2;
/// Highlights and Shadows read the toned neighbourhood at this radius on a
/// plane whose longer side is `REFERENCE_SIDE`, scaled with it so the look
/// belongs to the picture: Shadows/Highlights' own default radius, 30 px.
const TONE_RADIUS: f64 = 30.0;
/// The plan's work per pixel for one point pass (the stages before Presence,
/// or Vibrance after it): a few transfer functions and a 3×3, a few units.
const POINT_WORK_PER_PIXEL: u64 = 16;
/// [`DevelopFilter::plan`]'s per-pixel term for the toned plane, the n×16 straight
/// samples stages 1–5 write (the result itself when Presence is off). It is
/// live beside the guide while it is written and beside Presence's whole
/// working set after.
///
/// Record (`scripts/develop-filter-admission/measure.py`: the Release app
/// at `DR_WORKING_LIMIT_BYTES` 8 GiB, `apply_filter develop_filter` on a one-layer
/// 6000×4000 document of two-colour Clouds; `MallocLargeCache=0`, E =
/// `ri_interval_max_phys_footprint` − pre around the one call, a fresh process
/// per case, 3 runs + a baseline twin each, every twin at E 0). ALL sets all
/// thirteen controls, Texture, Clarity and Dehaze at 50; TONE the same without
/// those three.
/// - ALL, 16-bit: E 1,155,171,120 / 1,155,236,632 / 1,155,220,272 against
///   `selected_edit_plan` 1,469,320,064 (1.272×), which this plan sets.
/// - TONE, 16-bit: E 580,338,072 / 581,124,504 / 580,485,528 against
///   822,291,456 (1.415×), where the n×16 plane held beside the n×8 copy is
///   above this plan.
/// - ALL, 8-bit: E 1,251,918,712 / 1,251,738,464 / 1,251,771,256 against
///   `filter_op_scratch_bytes` 1,534,028,640 plus what the edit leaves
///   retained (192,003,794 in every run): 1,726,032,434, 1.379×,
///   `ProjectionMargin`'s rule.
/// - TONE, 8-bit: E 857,473,600 / 857,571,904 / 857,539,160; the host's
///   commit stage (`ByteFilterAdmission.commitStageBytes` 1,020,000,000) is
///   above the plan 605,242,880, so it is charged: 1,212,003,794 with the
///   retained growth, 1.413×.
const TONED_BYTES_PER_PIXEL: u64 = 16;

/// Filters ▸ Develop Filter: each control in its own units (module doc);
/// 0 is off.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct DevelopFilter {
    pub temperature: f64,
    pub tint: f64,
    pub exposure: f64,
    pub contrast: f64,
    pub highlights: f64,
    pub shadows: f64,
    pub whites: f64,
    pub blacks: f64,
    pub texture: f64,
    pub clarity: f64,
    pub dehaze: f64,
    pub vibrance: f64,
    pub saturation: f64,
}

/// The ops of the stages that run, each `None` when its controls are 0.
struct Stages {
    white_balance: Option<WhiteBalance>,
    exposure: Option<Exposure>,
    contrast: Option<f32>,
    tone: Option<ShadowsHighlights>,
    levels: Option<Levels>,
    vibrance: Option<Vibrance>,
}

/// Stages 1 and 2 of a 32-bit float request, prepared once and applied to
/// stored linear values ([`DevelopFilter::linear_front`]): White Balance's
/// linear matrix (`WhiteBalance::apply_linear`), then the exposure gain
/// `v·2^e` (`Exposure::apply_linear` of `gain_only`), in binary64 with one
/// rounding to f32, which for Exposure alone is the f32 product `v·2^e`.
#[derive(Clone)]
pub(crate) struct LinearFront {
    white_balance: Option<WhiteBalance>,
    exposure: Option<Exposure>,
}

impl LinearFront {
    /// Neither stage runs.
    pub(crate) fn identity(&self) -> bool {
        self.white_balance.is_none() && self.exposure.is_none()
    }

    /// One straight linear RGB triple through the prepared stages.
    pub(crate) fn apply(&self, rgb: [f32; 3]) -> [f32; 3] {
        if self.identity() {
            return rgb;
        }
        let mut v = rgb.map(f64::from);
        if let Some(op) = &self.white_balance {
            v = op.apply_linear(v);
        }
        if let Some(op) = &self.exposure {
            v = op.apply_linear(v);
        }
        v.map(|x| x as f32)
    }
}

/// One op's parameters as the map its `parse` reads.
fn params(value: Value) -> Map<String, Value> {
    match value {
        Value::Object(map) => map,
        _ => Map::new(),
    }
}

impl Stages {
    /// Stages 1–3 on one straight RGB triple.
    fn front(&self, rgb: [f32; 3]) -> [f32; 3] {
        let mut rgb = rgb;
        if let Some(op) = &self.white_balance {
            rgb = op.apply(rgb);
        }
        if let Some(op) = &self.exposure {
            rgb = op.apply(rgb);
        }
        if let Some(power) = self.contrast {
            rgb = brightness_contrast_modern(rgb, 1.0, power, 1.0);
        }
        rgb
    }

    /// Stages 4 and 5 on stage 3's output, `guide` its blurred luma; at
    /// `extended` Highlights and Shadows are `ShadowsHighlights::apply_float`
    /// and Whites and Blacks `Levels::apply_float` (no clamp).
    fn back(&self, rgb: [f32; 3], guide: f32, extended: bool) -> [f32; 3] {
        let mut rgb = rgb;
        if let Some(op) = &self.tone {
            rgb = if extended {
                op.apply_float(rgb, guide)
            } else {
                op.apply(rgb, guide)
            };
        }
        if let Some(op) = &self.levels {
            rgb = if extended {
                op.apply_float(rgb)
            } else {
                op.apply(rgb)
            };
        }
        rgb
    }

    /// Stage 7.
    fn finish(&self, rgb: [f32; 3]) -> [f32; 3] {
        match &self.vibrance {
            Some(op) => op.apply(rgb),
            None => rgb,
        }
    }
}

impl DevelopFilter {
    /// All thirteen at 0: the exact bypass.
    pub(crate) fn identity(&self) -> bool {
        self.controls().iter().all(|v| *v == 0.0)
    }

    fn controls(&self) -> [f64; 13] {
        [
            self.temperature,
            self.tint,
            self.exposure,
            self.contrast,
            self.highlights,
            self.shadows,
            self.whites,
            self.blacks,
            self.texture,
            self.clarity,
            self.dehaze,
            self.vibrance,
            self.saturation,
        ]
    }

    /// Stage 6's setting.
    fn presence(&self) -> Presence {
        Presence {
            texture: self.texture,
            clarity: self.clarity,
            dehaze: self.dehaze,
        }
    }

    /// Whether stage 4 runs, and so whether the guide is built.
    fn reads_guide(&self) -> bool {
        self.highlights != 0.0 || self.shadows != 0.0
    }

    /// Whether stage 7 runs.
    fn finishes(&self) -> bool {
        self.vibrance != 0.0 || self.saturation != 0.0
    }

    /// The guide's radius on a plane of `dimensions`: [`TONE_RADIUS`] scaled
    /// by `max(w, h) / REFERENCE_SIDE` (Presence's rule), kept within the
    /// guide's 1…1000.
    pub(crate) fn tone_radius(dimensions: (u32, u32)) -> f32 {
        let unit = f64::from(dimensions.0.max(dimensions.1)) / REFERENCE_SIDE;
        (TONE_RADIUS * unit).clamp(1.0, 1000.0) as f32
    }

    /// The temperature `WhiteBalance` reads for a Temperature of `t` (module
    /// doc): exactly [`NEUTRAL_KELVIN`] at 0.
    fn kelvin(t: f64) -> f64 {
        if t == 0.0 {
            NEUTRAL_KELVIN
        } else {
            MIRED / (MIRED / NEUTRAL_KELVIN - t)
        }
    }

    /// Stage 1's op, or `None` when Temperature and Tint are 0.
    fn white_balance(&self) -> Result<Option<WhiteBalance>, String> {
        if self.temperature == 0.0 && self.tint == 0.0 {
            return Ok(None);
        }
        let map = params(json!({
            "temperature": Self::kelvin(self.temperature),
            "tint": self.tint,
        }));
        WhiteBalance::parse(&map)
            .map(Some)
            .ok_or_else(|| "develop_filter could not build its white balance stage".into())
    }

    /// Stage 2's op, or `None` when Exposure is 0.
    fn exposure_stage(&self) -> Option<Exposure> {
        (self.exposure != 0.0).then(|| Exposure::gain_only(self.exposure as f32))
    }

    /// The request split for a 32-bit float layer (module doc, `# 32 bits`):
    /// `(linear, display, finish)`, the first holding Temperature, Tint and
    /// Exposure (run on stored linear values by [`Self::linear_front`]), the
    /// second Contrast, Highlights, Shadows, Whites, Blacks, Texture, Clarity
    /// and Dehaze (run on the display-encoded copy by [`Self::plane_in`]), the
    /// third Vibrance and Saturation (stage 7, [`Self::finish_op`], run on the
    /// decoded linear value). Each is zero where another holds the value.
    pub(crate) fn split_float(&self) -> (DevelopFilter, DevelopFilter, DevelopFilter) {
        let zero = DevelopFilter {
            temperature: 0.0,
            tint: 0.0,
            exposure: 0.0,
            contrast: 0.0,
            highlights: 0.0,
            shadows: 0.0,
            whites: 0.0,
            blacks: 0.0,
            texture: 0.0,
            clarity: 0.0,
            dehaze: 0.0,
            vibrance: 0.0,
            saturation: 0.0,
        };
        let linear = DevelopFilter {
            temperature: self.temperature,
            tint: self.tint,
            exposure: self.exposure,
            ..zero
        };
        let display = DevelopFilter {
            temperature: 0.0,
            tint: 0.0,
            exposure: 0.0,
            vibrance: 0.0,
            saturation: 0.0,
            ..*self
        };
        let finish = DevelopFilter {
            vibrance: self.vibrance,
            saturation: self.saturation,
            ..zero
        };
        (linear, display, finish)
    }

    /// Stage 7's op, `None` when Vibrance and Saturation are 0: the
    /// construction [`Self::stages`] uses.
    pub(crate) fn finish_op(&self) -> Result<Option<Vibrance>, String> {
        if !self.finishes() {
            return Ok(None);
        }
        let map = params(json!({
            "vibrance": self.vibrance / 100.0,
            "saturation": self.saturation / 100.0,
        }));
        Vibrance::parse(&map)
            .map(Some)
            .ok_or_else(|| "develop_filter could not build its vibrance stage".into())
    }

    /// Stages 1 and 2 prepared for stored linear values (module doc, `#
    /// 32 bits`).
    pub(crate) fn linear_front(&self) -> Result<LinearFront, String> {
        Ok(LinearFront {
            white_balance: self.white_balance()?,
            exposure: self.exposure_stage(),
        })
    }

    /// The ops, refused only if a mapping left an op's range, which the
    /// parsed ranges rule out.
    fn stages(&self, dimensions: (u32, u32)) -> Result<Stages, String> {
        let refused = |stage: &str| format!("develop_filter could not build its {stage} stage");
        let white_balance = self.white_balance()?;
        let exposure = self.exposure_stage();
        let contrast =
            (self.contrast != 0.0).then(|| (CONTRAST_STOPS * self.contrast as f32 / 100.0).exp2());
        // The negative side stops at half (module doc), inside `sliders`.
        let tone = self.reads_guide().then(|| {
            ShadowsHighlights::sliders(self.shadows, self.highlights, Self::tone_radius(dimensions))
        });
        let levels = if self.whites != 0.0 || self.blacks != 0.0 {
            let mut map = Map::new();
            let (white, black) = (self.whites / 100.0, self.blacks / 100.0);
            if white >= 0.0 {
                map.insert("white".into(), json!(1.0 - ENDPOINT_SPAN * white));
            } else {
                map.insert("output_white".into(), json!(1.0 + ENDPOINT_SPAN * white));
            }
            if black <= 0.0 {
                map.insert("black".into(), json!(-ENDPOINT_SPAN * black));
            } else {
                map.insert("output_black".into(), json!(ENDPOINT_SPAN * black));
            }
            let mut levels = Levels::parse(&map).ok_or_else(|| refused("whites and blacks"))?;
            // A stage on continuous tones at both depths, never the Levels
            // adjustment's 8-bit code rounding (#578 phase 4).
            levels.use_precision();
            Some(levels)
        } else {
            None
        };
        let vibrance = self.finish_op()?;
        Ok(Stages {
            white_balance,
            exposure,
            contrast,
            tone,
            levels,
            vibrance,
        })
    }

    /// `(scratch, work)`, refused before any allocation when the work is above
    /// `cap`. The work is one point pass (two when Presence runs and stage 7
    /// follows it), the guide's and Presence's. The scratch is the toned
    /// plane beside the larger of the guide (built first and held while the
    /// toned plane is written) and Presence's own working set (which runs
    /// after the guide is freed), plus two bookkeeping allowances.
    pub(crate) fn plan(&self, dimensions: (u32, u32), cap: u64) -> Result<(u64, u64), String> {
        let n = pixel_count(dimensions.0, dimensions.1)? as u64;
        let (guide_scratch, guide_work) = if self.reads_guide() {
            guide(dimensions.0, dimensions.1, Self::tone_radius(dimensions))?
        } else {
            (0, 0)
        };
        let presence = self.presence();
        let (presence_scratch, presence_work) = if presence.identity() {
            (0, 0)
        } else {
            presence.plan(dimensions, u64::MAX)?
        };
        let passes = 1 + u64::from(!presence.identity() && self.finishes());
        let work = n
            .saturating_mul(POINT_WORK_PER_PIXEL)
            .saturating_mul(passes)
            .saturating_add(guide_work)
            .saturating_add(presence_work);
        if work > cap {
            return Err("develop_filter exceeds the bounded pixel-work limit".into());
        }
        Ok((
            n.saturating_mul(TONED_BYTES_PER_PIXEL)
                .saturating_add(guide_scratch.max(presence_scratch))
                .saturating_add(2 * BOOKKEEPING_BYTES),
            work,
        ))
    }

    /// The final straight samples (RGB and alpha in 0…1 at `Unit`), the stages in the
    /// module doc's order. The guide, the toned pass, Presence and stage 7
    /// after it each have an equal share of the progress. `Unit` is the 8-
    /// and 16-bit plane. At `Extended` it is the display part of a
    /// 32-bit request (module doc, `# 32 bits`): Contrast, Whites and Blacks
    /// (`Levels::apply_float`) and Presence (`Presence::plane_in`) on the
    /// display-encoded copy, Highlights and Shadows by rule H
    /// (`ShadowsHighlights::apply_float`) and Dehaze by rule D. A control
    /// that runs outside the encoded copy here (the linear stages, before
    /// the encoding, and Vibrance and Saturation, after the decode) is an
    /// `Err`.
    pub(crate) fn plane_in(
        &self,
        read: impl Fn(usize) -> [f32; 4],
        dimensions: (u32, u32),
        cap: u64,
        range: &SampleRange,
        job: JobContext<'_>,
    ) -> Result<Vec<[f32; 4]>, String> {
        let extended = matches!(range, SampleRange::Extended { .. });
        if extended {
            if self.temperature != 0.0 || self.tint != 0.0 || self.exposure != 0.0 {
                return Err(
                    "develop_filter white balance and exposure run on linear light, before the display encoding"
                        .into(),
                );
            }
            if self.finishes() {
                return Err(
                    "develop_filter vibrance and saturation run on linear light, after the display decode"
                        .into(),
                );
            }
        }
        self.plan(dimensions, cap)?;
        let stages = self.stages(dimensions)?;
        let n = pixel_count(dimensions.0, dimensions.1)?;
        let presence = self.presence();
        let finish_after = !presence.identity() && self.finishes();
        let count = 1
            + usize::from(stages.tone.is_some())
            + usize::from(!presence.identity())
            + usize::from(finish_after);
        let mut stage = 0_usize;
        let mut share = || {
            let range = job.range(
                stage as f64 / count as f64,
                (stage + 1) as f64 / count as f64,
            );
            stage += 1;
            range
        };
        let front = |i: usize| {
            let p = read(i);
            let rgb = stages.front([p[0], p[1], p[2]]);
            [rgb[0], rgb[1], rgb[2], p[3]]
        };
        let guide: Option<GuidePlane> = match &stages.tone {
            Some(_) => Some(precision_plane(
                dimensions.0,
                dimensions.1,
                Self::tone_radius(dimensions),
                &front,
                share(),
            )?),
            None => None,
        };
        let toned_job = share();
        let (w, h) = (dimensions.0 as usize, dimensions.1 as usize);
        let mut toned: Vec<[f32; 4]> = reserved_vec(n)?;
        for row in 0..h {
            for i in row * w..(row + 1) * w {
                toned_job.every(i)?;
                let p = read(i);
                if p[3] <= 0.0 {
                    toned.push(p);
                    continue;
                }
                let f = front(i);
                let at = guide
                    .as_ref()
                    .map_or(0.0, |g| g.at((i - row * w) as u32, row as u32));
                let mut rgb = stages.back([f[0], f[1], f[2]], at, extended);
                if presence.identity() {
                    rgb = stages.finish(rgb);
                }
                toned.push([rgb[0], rgb[1], rgb[2], p[3]]);
            }
            toned_job.report((row + 1) as f64 / h as f64, 6)?;
        }
        drop(guide);
        if presence.identity() {
            return Ok(toned);
        }
        let mut out = presence.plane_in(|i| toned[i], dimensions, cap, range, share())?;
        drop(toned);
        if !finish_after {
            return Ok(out);
        }
        let finish_job = share();
        for (row, pixels) in out.chunks_mut(w).enumerate() {
            for (x, p) in pixels.iter_mut().enumerate() {
                finish_job.every(row * w + x)?;
                if p[3] <= 0.0 {
                    continue;
                }
                let rgb = stages.finish([p[0], p[1], p[2]]);
                *p = [rgb[0], rgb[1], rgb[2], p[3]];
            }
            finish_job.report((row + 1) as f64 / h as f64, 6)?;
        }
        Ok(out)
    }
}
