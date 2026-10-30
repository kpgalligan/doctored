//! The photographic kernels the destructive filters share (Gaussian Blur,
//! Unsharp Mask, High Pass, Reduce Noise, Dust & Scratches: `ops_photographic`,
//! `ops_spatial`, `selected_filter`, `doc_export`). An operator is a
//! NEIGHBORHOOD plus a COMBINE step, and the two are dispatched here from one
//! `Neighborhood` value so a plane and its forecast can never disagree.
//!
//! Three operators read the same normalized Gaussian neighborhood
//! (`sampling_gaussian`): binary64 sigma in source pixels, `ceil(3*sigma)`
//! support below σ4 and the radius-independent recursive Gaussian
//! (untruncated) from σ4 up, clamped edges, premultiplied encoded-profile
//! sampling. Only their combine step differs. (A 32-bit float layer's
//! Gaussian is the FIR at every σ; `sampling_gaussian::runs_fast` says why.)
//!
//! Gaussian blur replaces the sample outright, so it filters alpha as well.
//! The other four write COLOR only: alpha is passed through exactly, and a
//! sample whose input alpha is zero is left exactly alone, so a hidden colour
//! can never be sharpened into view. Unsharp Mask and High Pass clamp their
//! straight output to [0,1] — sharpening and the High Pass pivot both
//! overshoot.
//!
//! That is the 8- and 16-bit rule, [`SampleRange::Unit`]. A 32-bit float
//! layer runs the same kernels at [`SampleRange::Extended`]: colour is
//! never clamped to 0…1, alpha is at most 1, and an output alpha below 2^-24
//! is transparent black. [`evaluated_in`] states the Gaussian family's float
//! rules where it computes them.
use crate::job_control::JobContext;
use crate::sampling::{straight_extended, ColorRange, EXTENDED_ALPHA_FLOOR};

/// What the evaluator does with its operator's neighborhood sample.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum FilterKind {
    GaussianBlur,
    UnsharpMask {
        amount: f64,
        threshold: f64,
    },
    HighPass,
    /// `luminance` is a neighborhood parameter and is carried only so the
    /// bypass rule can read it; the combine step never uses it.
    ReduceNoise {
        luminance: f64,
        detail: f64,
        color: f64,
    },
    DustAndScratches {
        threshold: f64,
    },
}

/// The spatial pass one operator needs before its pointwise combine. Every
/// other parameter an operator reads is a COMBINE parameter and stays out of
/// this value.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Neighborhood {
    /// Premultiplied separable Gaussian. `gaussian_blur`, `unsharp_mask` and
    /// `high_pass` all resolve to this one, identically.
    Gaussian { sigma: f64 },
    /// Straight per-channel lower median over the lattice disc `D(radius)`,
    /// with alpha deciding membership.
    Rank { radius: f64 },
    /// Alpha-weighted guided denoise. `detail` and `color` are combine
    /// parameters and are deliberately absent.
    Denoise {
        sigma: f64,
        luminance: f64,
        luma: bool,
        chroma: bool,
    },
}

/// Straight colour from a premultiplied neighborhood average. Alpha zero has
/// no colour to recover.
pub(crate) fn straight(value: [f32; 4]) -> [f32; 4] {
    if value[3] > 0.0 {
        [
            value[0] / value[3],
            value[1] / value[3],
            value[2] / value[3],
            value[3],
        ]
    } else {
        [0.0; 4]
    }
}

/// The sample domain a kernel finishes in: the two depths' clamps and alpha
/// floors, written once so every kernel asks the same value.
///
/// `Unit` is 8 and 16 bits. Its arms are the expressions the kernels held
/// before the float route existed, so their output is bit-identical.
/// `Extended` is a 32-bit float layer (linear light, any finite colour):
/// `visible` is the layer's own visible RGB range
/// (`precision_sampling::visible_range`), the D3 clamp of `float_document`'s
/// module doc. The kernels with a negative lobe need it (Lens Correction's
/// and Liquify's Catmull-Rom); the Gaussian's finish and the five means'
/// (`ops_spatial::SpatialKernel::combine_in`) share it, where it can only
/// absorb rounding.
#[derive(Clone, Copy, Debug)]
pub(crate) enum SampleRange {
    Unit,
    Extended { visible: ColorRange },
}

impl SampleRange {
    /// The output alpha below which a pixel is transparent black. `Unit`:
    /// half a 16-bit code, which quantizes to 0 at either depth
    /// (`ops_spatial::box_plane` says why a colour is not divided out below
    /// it). `Extended`: `sampling::EXTENDED_ALPHA_FLOOR` (2^-24), the floor
    /// float geometry already uses, for the reasons on that constant.
    pub(crate) fn alpha_floor(&self) -> f64 {
        match self {
            Self::Unit => 0.5 / 65535.0,
            Self::Extended { .. } => f64::from(EXTENDED_ALPHA_FLOOR),
        }
    }

    /// A colour component a combine step produced (Unsharp Mask's sharpened
    /// value, High Pass's detail): clamped to 0…1 at `Unit`, kept as computed
    /// at `Extended`.
    pub(crate) fn color(&self, v: f64) -> f64 {
        match self {
            Self::Unit => v.clamp(0.0, 1.0),
            Self::Extended { .. } => v,
        }
    }

    /// Colour component `channel` (0 red, 1 green, 2 blue) of a resample
    /// whose kernel has a negative lobe (Catmull-Rom): clamped to 0…1 at
    /// `Unit`, and at `Extended` to the layer's visible range, so ringing
    /// never leaves the values the layer already holds. The five means
    /// finish their un-premultiplied colour with it too: their quotient can
    /// round an ulp past that range (`SpatialKernel::combine_in`).
    pub(crate) fn resampled(&self, channel: usize, v: f64) -> f64 {
        match self {
            Self::Unit => v.clamp(0.0, 1.0),
            Self::Extended { visible } => v.clamp(
                f64::from(visible.min[channel]),
                f64::from(visible.max[channel]),
            ),
        }
    }

    /// Colour component `channel` of a relief finish (Emboss, Edge Detect:
    /// `ops_filters::relief_plane`): clamped to 0…1 at `Unit`, and at
    /// `Extended` to `[min(0, visible.min[c]), max(1, visible.max[c])]`.
    /// The float route runs a relief on the layer's DISPLAY-ENCODED copy
    /// (`display_encoding`), whose visible range this is, so an SDR layer
    /// finishes inside 0…1 exactly as at 16 bits (an edge's overshoot is not
    /// decoded into linear values in the hundreds), and only content already
    /// above white or below zero can reach past it.
    pub(crate) fn display_bounded(&self, channel: usize, v: f64) -> f64 {
        match self {
            Self::Unit => v.clamp(0.0, 1.0),
            Self::Extended { visible } => v.clamp(
                f64::from(visible.min[channel]).min(0.0),
                f64::from(visible.max[channel]).max(1.0),
            ),
        }
    }

    /// Straight colour from a premultiplied Gaussian-family sample. `Unit`
    /// is [`straight`]. `Extended` is `sampling::straight_extended` (the
    /// master copy): transparent black below the floor, each RGB component
    /// clamped to the visible range, alpha at most 1. The float Gaussian is
    /// a mean with non-negative weights, so its straight colour already lies
    /// in that range but for rounding: the clamp changes no picture.
    pub(crate) fn straight(&self, premultiplied: [f32; 4]) -> [f32; 4] {
        match self {
            Self::Unit => straight(premultiplied),
            Self::Extended { visible } => straight_extended(premultiplied, visible),
        }
    }
}

/// Bytes and work for one operator's spatial pass. Paired with
/// [`neighborhood_plane`] — the SAME match — so an operator can never be
/// admitted by a forecast it does not spend, or refused by one it does not pay.
/// `cap` is the caller's ceiling: `MAX_DESTRUCTIVE_WORK` for a one-shot
/// filter.
pub(crate) fn neighborhood_plan(
    neighborhood: Neighborhood,
    dimensions: (u32, u32),
    cap: u64,
) -> Result<(u64, u64), String> {
    neighborhood_plan_in(neighborhood, dimensions, &SampleRange::Unit, cap)
}

/// [`neighborhood_plan`] in `range`, paired with [`neighborhood_plane_in`].
/// Only the Gaussian reads the range: a float layer's is the FIR at every σ
/// (`sampling_gaussian::plan_bounded`).
pub(crate) fn neighborhood_plan_in(
    neighborhood: Neighborhood,
    dimensions: (u32, u32),
    range: &SampleRange,
    cap: u64,
) -> Result<(u64, u64), String> {
    match neighborhood {
        Neighborhood::Gaussian { sigma } => {
            crate::sampling_gaussian::plan_bounded(dimensions.0, dimensions.1, sigma, range, cap)
        }
        Neighborhood::Rank { radius } => {
            crate::sampling_rank::plan_bounded(dimensions.0, dimensions.1, radius, cap)
        }
        Neighborhood::Denoise {
            sigma,
            luma,
            chroma,
            ..
        } => crate::sampling_denoise::plan_bounded(
            dimensions.0,
            dimensions.1,
            sigma,
            luma,
            chroma,
            cap,
        ),
    }
}

/// The ONE place an operator's spatial pass runs. `read` yields the STRAIGHT
/// sample and each arm applies its own domain convention — the Gaussian
/// premultiplies here rather than in each of its three call sites.
pub(crate) fn neighborhood_plane(
    neighborhood: Neighborhood,
    dimensions: (u32, u32),
    cap: u64,
    read: impl Fn(usize) -> [f32; 4],
    job: JobContext<'_>,
) -> Result<Vec<[f32; 4]>, String> {
    neighborhood_plane_in(neighborhood, dimensions, cap, &SampleRange::Unit, read, job)
}

/// [`neighborhood_plane`] in `range` (see [`neighborhood_plan_in`]). The
/// Denoise arm reads it too: its luminance model is not clamped to 0…1 at
/// `Extended` (`sampling_denoise::guided_bounded_in`).
pub(crate) fn neighborhood_plane_in(
    neighborhood: Neighborhood,
    dimensions: (u32, u32),
    cap: u64,
    range: &SampleRange,
    read: impl Fn(usize) -> [f32; 4],
    job: JobContext<'_>,
) -> Result<Vec<[f32; 4]>, String> {
    match neighborhood {
        Neighborhood::Gaussian { sigma } => crate::sampling_gaussian::blur_bounded(
            dimensions.0,
            dimensions.1,
            sigma,
            cap,
            range,
            premultiplied(read),
            job,
        ),
        Neighborhood::Rank { radius } => {
            crate::sampling_rank::median_bounded(dimensions.0, dimensions.1, radius, cap, read, job)
        }
        Neighborhood::Denoise {
            sigma,
            luminance,
            luma,
            chroma,
        } => crate::sampling_denoise::guided_bounded_in(
            dimensions.0,
            dimensions.1,
            sigma,
            luminance,
            luma,
            chroma,
            cap,
            range,
            read,
            job,
        ),
    }
}

/// The Gaussian's domain: `read`'s straight sample premultiplied.
pub(crate) fn premultiplied(read: impl Fn(usize) -> [f32; 4]) -> impl Fn(usize) -> [f32; 4] {
    move |index| {
        let p = read(index);
        [p[0] * p[3], p[1] * p[3], p[2] * p[3], p[3]]
    }
}

/// The operator's own output for one pixel. `source` is the straight input
/// sample; `neighborhood` is that operator's own spatial pass over the same
/// plane — the premultiplied Gaussian average for the three Gaussian-family
/// operators. The 8- and 16-bit form: [`evaluated_in`] at
/// [`SampleRange::Unit`].
pub(crate) fn evaluated(kind: FilterKind, source: [f32; 4], neighborhood: [f32; 4]) -> [f32; 4] {
    evaluated_in(kind, source, neighborhood, &SampleRange::Unit)
}

/// [`evaluated`] in `range`. At `Unit`, Unsharp Mask and High Pass clamp an
/// overshoot to [0, 1].
///
/// At `Extended` (a 32-bit float layer, linear light) nothing is clamped to
/// 0…1. The neighbourhood is the FIR Gaussian at every σ
/// (`sampling_gaussian::runs_fast`): a mean with non-negative weights, so it
/// never overshoots the brightest sample it reads and never goes below the
/// darkest, and a highlight far above white spreads its whole energy. The
/// three arms take its straight colour from `SampleRange::straight`, which
/// is the finish float geometry uses (`sampling::straight_extended`):
///
/// - Below 2^-24 of mean alpha the sample is transparent black. The Gaussian
///   has no floor of its own, and far from any visible pixel its mean alpha
///   is a rounding residue; a 16-bit code rounds that to 0, a float sample
///   would store it and divide a colour by it.
/// - Alpha is at most 1 (a sum of alphas that rounding can leave an ulp
///   above it).
///
/// Gaussian Blur returns that sample. Unsharp Mask and High Pass use it as
/// `low`, and where its alpha is 0 there is no low-pass colour to subtract,
/// so they return the source unchanged, as they do for a source with alpha 0.
pub(crate) fn evaluated_in(
    kind: FilterKind,
    source: [f32; 4],
    neighborhood: [f32; 4],
    range: &SampleRange,
) -> [f32; 4] {
    let extended = matches!(range, SampleRange::Extended { .. });
    match kind {
        FilterKind::GaussianBlur => range.straight(neighborhood),
        FilterKind::UnsharpMask { amount, threshold } => {
            if source[3] <= 0.0 {
                return source;
            }
            let low = range.straight(neighborhood);
            if extended && low[3] <= 0.0 {
                return source;
            }
            let mut out = source;
            for c in 0..3 {
                // The edge test: only detail whose contrast exceeds the
                // threshold is amplified, which is what keeps film grain and
                // sensor noise from being sharpened along with real edges.
                //
                // In a float layer both sides of the test are stored linear
                // values, so the threshold compares LINEAR differences: the
                // same number gates a smaller visible step in the highlights
                // than in the shadows. The sharpened value is not clamped
                // there; a value below 0 or above 1 is one a float layer
                // keeps.
                let detail = f64::from(source[c]) - f64::from(low[c]);
                if detail.abs() > threshold {
                    out[c] = range.color(f64::from(source[c]) + amount * detail) as f32;
                }
            }
            out
        }
        FilterKind::HighPass => {
            if source[3] <= 0.0 {
                return source;
            }
            let low = range.straight(neighborhood);
            if extended && low[3] <= 0.0 {
                return source;
            }
            let mut out = source;
            for c in 0..3 {
                // The detail image around a neutral pivot, exactly as the
                // frequency-separation workflow expects: a constant plane at
                // any radius returns the pivot and nothing else.
                //
                // The pivot is the STORED value 0.5 at every depth. In a float
                // layer that is linear 0.5, which displays lighter than
                // mid-grey, and the result is not clamped. One constant at
                // every depth is what keeps `source = low + (high − 0.5)`
                // true whatever the storage, which is the identity frequency
                // separation is built on; a display-referred pivot (linear
                // 0.2140) would make the recombination depend on the depth.
                // It is also what Photoshop 26.10.0's 32-bit High Pass
                // measured (the record of 71d0f663).
                out[c] = range.color(0.5 + f64::from(source[c]) - f64::from(low[c])) as f32;
            }
            out
        }
        FilterKind::ReduceNoise { detail, color, .. } => {
            if source[3] <= 0.0 {
                return source;
            }
            // The plane holds the opponent CORRECTION, so an inactive half is
            // an exact zero here and needs no branch. `luminance` decided what
            // counted as noise when that plane was built and has no part in
            // the mix, which is why dragging Preserve Details or Colour Noise
            // reuses it.
            //
            // In a float layer the float route hands this the layer's
            // DISPLAY-ENCODED copy (`display_encoding`), so Luminance,
            // Preserve Details and Colour Noise mean what they mean at 16
            // bits on every value inside 0…1, and the correction is not
            // clamped (`sampling_denoise::applied_in`): a value above white
            // is smoothed in code and decoded back above white.
            crate::sampling_denoise::applied_in(
                source,
                (1.0 - detail) * f64::from(neighborhood[0]),
                color * f64::from(neighborhood[1]),
                color * f64::from(neighborhood[2]),
                range,
            )
        }
        FilterKind::DustAndScratches { threshold } => {
            if source[3] <= 0.0 {
                return source;
            }
            let mut out = source;
            for c in 0..3 {
                // Strict `>`, exactly as Unsharp Mask's edge test: a channel is
                // skipped only where the median already equals the sample, so
                // threshold 0 is the plain median bit for bit rather than a
                // bypass. A selected element is a member of the input plane, so
                // there is nothing to clamp.
                //
                // In a float layer both sides are DISPLAY-ENCODED codes (the
                // float route runs the median on the encoded copy,
                // `display_encoding`), so the threshold gates the same visible
                // step it gates at 16 bits, and a median above white is a
                // member of the plane, decoded back to the light it held.
                if (f64::from(neighborhood[c]) - f64::from(source[c])).abs() > threshold {
                    out[c] = neighborhood[c];
                }
            }
            out
        }
    }
}
