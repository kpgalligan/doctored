//! The route of one admitted float filter: which kernel builds
//! its plane, which combine turns a plane sample into a pixel, and in which
//! DOMAIN both run. `float_filter_apply` is the run; this is the dispatch.
//!
//! A filter whose parameters are stated in linear light, or that has none,
//! runs on the layer's stored samples ([`Domain::Linear`]): Gaussian Blur
//! (the fitted kernel, `gaussian_blur_filter`: [`Route::GaussianBlur`]),
//! the rest of the Gaussian family, the means, Offset, Lens Correction, Liquify, Pixelate (the cell's
//! light) and Add Noise (`v·(1 + δ)`). A filter whose parameter is defined on
//! display-encoded values runs on the layer's display-encoded copy
//! ([`Domain::Display`], `display_encoding`): Surface Blur, Dust & Scratches,
//! Reduce Noise, Add Grain, Clouds, Difference Clouds, Emboss, Edge Detect,
//! legacy Sharpen, Presence (Dehaze included), Smart Sharpen and the display
//! part of the Develop Filter (Highlights, Shadows and Dehaze included).
//! Each kernel module states its filter's rule where it computes it.
//!
//! Smart Sharpen runs whole on the display-encoded copy, as legacy
//! Sharpen and Reduce Noise do: its Reduce Noise level, Lens Blur's
//! overshoot and the tonal widths mean their 16-bit values on every value
//! inside 0…1, the fade classifier reads the encoded luma clamped to 0…1 (a
//! pixel above white has `L = 1`), and nothing is clamped, so a value above
//! white is sharpened in code and decoded back (`sampling_smart_sharpen`).
//!
//! Every control of Presence and the Develop Filter runs at 32 bits.
//! The Develop Filter's Vibrance and Saturation are its BACK
//! stage ([`Route::back`]): they run after the decode, on the linear value,
//! through the Scaled display form (`display_encoding`, A), so a route with a
//! back stage needs the encoding even when it takes no encoded copy
//! ([`Route::needs_encoding`]).
//!
//! Depth Blur is linear: its disc, Haze, Warmth and Brightness run on
//! the stored samples, and its plane is the final sample. It needs the
//! encoding all the same: its depth guide is the display-encoded value, and
//! Saturation, Grain and Output Depth Map Only run on encoded values
//! (`depth_blur`'s module doc), so [`Route::plane`] hands it the encoding.
use crate::adjust_color::Vibrance;
use crate::depth_blur::DepthBlur;
use crate::display_encoding::DisplayEncoding;
use crate::filter_kernels::premultiplied;
use crate::filter_kernels::{evaluated_in, neighborhood_plan_in, neighborhood_plane_in};
use crate::filter_kernels::{FilterKind, Neighborhood, SampleRange};
use crate::float_adjust_apply::NOT_U16;
use crate::float_filter_apply::PLAN_RANGE;
use crate::job_control::JobContext;
use crate::liquify::LiquifyStroke;
use crate::ops_filters::{noise_delta, noise_relative, pixelate_plane_f32, relief_plane};
use crate::ops_spatial::{Frame, SpatialKernel};
use crate::precision_document::PrecisionDocument;
use crate::precision_image::BOOKKEEPING_BYTES;
use crate::rng::SplitMix64;
use crate::sampling_develop_filter::{DevelopFilter, LinearFront};
use crate::sampling_gaussian::{destructive_work_cap, CONTROLLED_JOB_WORK};
use crate::selected_adjustment::SelectedEdit;
use crate::selected_filter::SelectedFilter;

/// Where a route's kernel reads and writes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Domain {
    /// The layer's stored linear samples.
    Linear,
    /// The layer's display-encoded copy, decoded back at the write.
    Display,
}

/// What a route's plane stage answered.
pub(crate) enum PlaneOutcome {
    /// One sample per pixel for the combine.
    Samples(Vec<[f32; 4]>),
    /// No plane: the combine computes each pixel from its source alone.
    Pointwise,
    /// The filter changes nothing (a Liquify field that is the identity).
    Identity,
}

/// One admitted float filter after its parameters are parsed.
pub(crate) enum Route {
    /// Filter ▸ Blur ▸ Gaussian Blur (`blur`) at a radius the fitted law
    /// runs (`gaussian_blur_filter`): the same kernel as 8 and 16 bits
    /// on the layer's linear values, nothing clamped.
    GaussianBlur {
        radius: f32,
    },
    /// The Gaussian family, Dust & Scratches and Reduce Noise.
    Photo {
        neighborhood: Neighborhood,
        kind: FilterKind,
    },
    /// Legacy Sharpen: Unsharp Mask amount 1, threshold 0 at `sigma` on the
    /// display-encoded copy (`selected_filter`'s module doc). Its own arm, so
    /// a user's `unsharp_mask` with the same numbers stays linear.
    Sharpen {
        sigma: f32,
    },
    /// Box, Motion, Radial, Lens Blur, Tilt-Shift, Offset, Lens Correction,
    /// Surface Blur, Add Grain, Clouds, Difference Clouds, Presence, Smart
    /// Sharpen.
    Spatial(SpatialKernel),
    /// The Develop Filter: `front` its linear stages (Temperature, Tint,
    /// Exposure), `display` the stages on the encoded copy and `back` stage
    /// 7 (Vibrance, Saturation) after the decode (`DevelopFilter::split_float`).
    DevelopFilter {
        front: LinearFront,
        display: DevelopFilter,
        back: Option<Vibrance>,
    },
    /// Emboss (`emboss`) or Edge Detect.
    Relief {
        emboss: bool,
    },
    Pixelate {
        block: u32,
    },
    Noise {
        amount: f32,
        seed: u64,
    },
    Liquify(Vec<LiquifyStroke>),
    /// Filter ▸ Neural Filters ▸ Depth Blur (`depth_blur`).
    DepthBlur(Box<DepthBlur>),
}

impl Route {
    /// The route of `edit`, with `SelectedFilter::parse`'s keys, ranges and
    /// defaults, so a request means the same at every depth. `frame` is asked
    /// only for Radial Blur and Offset, which act within the selection's
    /// bounding box (`None`: the whole plane, and what a plan passes, since
    /// the frameless plan bounds any frame); it replaces a parsed `region_*`,
    /// which is the 8-bit host's, as at 16 bits. `Ok(None)` is an exact
    /// bypass. `Err`: an op the float mode does not run (the u16 arm's
    /// sentence) or a parameter refusal.
    pub(crate) fn parse(
        edit: &SelectedEdit,
        frame: impl FnOnce() -> Option<Frame>,
    ) -> Result<Option<Self>, String> {
        if !crate::float_document::admits_filter(&edit.op) {
            return Err(NOT_U16.into());
        }
        let filter = SelectedFilter::parse(&edit.op, &edit.params)?;
        let frame = if filter.framed() { frame() } else { None };
        let filter = filter.with_frame(frame);
        if filter.identity() {
            return Ok(None);
        }
        Ok(Some(match filter {
            SelectedFilter::LegacyBlur {
                sigma,
                sharpen: false,
            } if crate::gaussian_blur_filter::runs(f64::from(sigma)) => {
                Self::GaussianBlur { radius: sigma }
            }
            SelectedFilter::LegacyBlur {
                sigma,
                sharpen: false,
            } => Self::Photo {
                neighborhood: Neighborhood::Gaussian {
                    sigma: f64::from(sigma),
                },
                kind: FilterKind::GaussianBlur,
            },
            SelectedFilter::LegacyBlur {
                sigma,
                sharpen: true,
            } => Self::Sharpen { sigma },
            SelectedFilter::Photo {
                neighborhood, kind, ..
            } => Self::Photo { neighborhood, kind },
            SelectedFilter::Relief { emboss } => Self::Relief { emboss },
            SelectedFilter::Pixelate { block } => Self::Pixelate { block },
            SelectedFilter::Noise { amount, seed } => Self::Noise { amount, seed },
            SelectedFilter::Spatial(SpatialKernel::DevelopFilter(develop_filter)) => {
                let (linear, display, finish) = develop_filter.split_float();
                Self::DevelopFilter {
                    front: linear.linear_front()?,
                    display,
                    back: finish.finish_op()?,
                }
            }
            SelectedFilter::Spatial(kernel) => Self::Spatial(kernel),
            SelectedFilter::Liquify(strokes) => Self::Liquify(strokes),
            SelectedFilter::DepthBlur(depth) => Self::DepthBlur(depth),
            SelectedFilter::DevelopFilterMasked(_) => {
                return Err(crate::develop_filter_masks::FLOAT_REFUSAL.into())
            }
            SelectedFilter::LevelsChannels(_) => return Err(NOT_U16.into()),
        }))
    }

    /// The domain the route's kernel runs in (module doc). Exhaustive, so a
    /// new arm or kernel must state its own.
    pub(crate) fn domain(&self) -> Domain {
        match self {
            Self::Photo { kind, .. } => match kind {
                FilterKind::GaussianBlur
                | FilterKind::UnsharpMask { .. }
                | FilterKind::HighPass => Domain::Linear,
                FilterKind::ReduceNoise { .. } | FilterKind::DustAndScratches { .. } => {
                    Domain::Display
                }
            },
            Self::GaussianBlur { .. } => Domain::Linear,
            Self::Sharpen { .. } | Self::Relief { .. } => Domain::Display,
            Self::Spatial(kernel) => match kernel {
                SpatialKernel::BoxBlur { .. }
                | SpatialKernel::RadialBlur { .. }
                | SpatialKernel::Offset { .. }
                | SpatialKernel::MotionBlur { .. }
                | SpatialKernel::LensBlur { .. }
                | SpatialKernel::TiltShift { .. }
                | SpatialKernel::LensCorrection(_) => Domain::Linear,
                SpatialKernel::SurfaceBlur { .. }
                | SpatialKernel::AddGrain { .. }
                | SpatialKernel::Clouds { .. }
                | SpatialKernel::Presence(_)
                | SpatialKernel::SmartSharpen(_) => Domain::Display,
                // Never built: `parse` routes it to `Self::DevelopFilter`.
                SpatialKernel::DevelopFilter(_) => Domain::Display,
            },
            Self::DevelopFilter { display, .. } => {
                if display.identity() {
                    Domain::Linear
                } else {
                    Domain::Display
                }
            }
            Self::Pixelate { .. } | Self::Noise { .. } | Self::Liquify(_) => Domain::Linear,
            Self::DepthBlur(_) => Domain::Linear,
        }
    }

    /// Whether the run needs the display encoding: the display domain's
    /// copy, the Develop Filter's back stage, or Depth Blur (module doc).
    pub(crate) fn needs_encoding(&self) -> bool {
        self.domain() == Domain::Display
            || matches!(
                self,
                Self::DevelopFilter { back: Some(_), .. } | Self::DepthBlur(_)
            )
    }

    /// The back stage of one decoded linear pixel: the Develop Filter's
    /// Vibrance and Saturation through `encoding`'s Scaled form (module
    /// doc) on a visible pixel; every other route and an alpha-0 pixel the
    /// identity.
    pub(crate) fn back(&self, linear: [f32; 4], encoding: &DisplayEncoding<'_>) -> [f32; 4] {
        match self {
            Self::DevelopFilter { back: Some(op), .. } if linear[3] > 0.0 => {
                let rgb = encoding.scaled([linear[0], linear[1], linear[2]], |c| op.apply(c));
                [rgb[0], rgb[1], rgb[2], linear[3]]
            }
            _ => linear,
        }
    }

    /// The kernel's own scratch for a plane of `dimensions`, planned at
    /// `CONTROLLED_JOB_WORK` (a plan carries no work refusal). Relief and
    /// Pixelate hold one n×16 plane; Add Noise and an identity display part
    /// of the Develop Filter hold none.
    pub(crate) fn kernel_plan(&self, dimensions: (u32, u32)) -> Result<u64, String> {
        let n = u64::from(dimensions.0) * u64::from(dimensions.1);
        match self {
            Self::GaussianBlur { radius } => Ok(crate::gaussian_blur_filter::plan(
                dimensions,
                f64::from(*radius),
                dimensions.1,
            )?
            .0),
            Self::Photo { neighborhood, .. } => Ok(neighborhood_plan_in(
                *neighborhood,
                dimensions,
                &PLAN_RANGE,
                CONTROLLED_JOB_WORK,
            )?
            .0),
            Self::Sharpen { sigma } => Ok(neighborhood_plan_in(
                Neighborhood::Gaussian {
                    sigma: f64::from(*sigma),
                },
                dimensions,
                &PLAN_RANGE,
                CONTROLLED_JOB_WORK,
            )?
            .0),
            // Smart Sharpen's Gaussian is the float FIR at every σ, which
            // only a plan in the float range charges.
            Self::Spatial(SpatialKernel::SmartSharpen(smart)) => Ok(smart
                .plan_in(dimensions, CONTROLLED_JOB_WORK, &PLAN_RANGE)?
                .0),
            Self::Spatial(kernel) => Ok(kernel.plan(dimensions, CONTROLLED_JOB_WORK)?.0),
            Self::DevelopFilter { display, .. } => {
                if display.identity() {
                    Ok(0)
                } else {
                    Ok(display.plan(dimensions, CONTROLLED_JOB_WORK)?.0)
                }
            }
            Self::Relief { .. } | Self::Pixelate { .. } => n
                .checked_mul(16)
                .and_then(|v| v.checked_add(BOOKKEEPING_BYTES))
                .ok_or_else(|| "filter scratch overflow".into()),
            Self::Noise { .. } => Ok(0),
            Self::Liquify(strokes) => crate::liquify::plan(dimensions, strokes),
            Self::DepthBlur(depth) => Ok(depth.plan(dimensions)?.0),
        }
    }

    /// The sample the domain's copy is taken of: the Develop Filter's
    /// linear stages on a visible pixel, every other route the identity.
    pub(crate) fn front(&self, linear: [f32; 4]) -> [f32; 4] {
        match self {
            Self::DevelopFilter { front, .. } if linear[3] > 0.0 && !front.identity() => {
                let rgb = front.apply([linear[0], linear[1], linear[2]]);
                [rgb[0], rgb[1], rgb[2], linear[3]]
            }
            _ => linear,
        }
    }

    /// The kernel's plane over the straight samples `read` yields (the
    /// stored ones, or the encoded copy in the display domain). `encoding` is
    /// the document's display encoding when [`Self::needs_encoding`] built it;
    /// only Depth Blur's plane reads it.
    pub(crate) fn plane(
        &self,
        read: impl Fn(usize) -> [f32; 4],
        dimensions: (u32, u32),
        range: &SampleRange,
        encoding: Option<&DisplayEncoding<'_>>,
        job: JobContext<'_>,
    ) -> Result<PlaneOutcome, String> {
        let cap = destructive_work_cap(job);
        Ok(match self {
            Self::GaussianBlur { radius } => {
                let (_, work) =
                    crate::gaussian_blur_filter::plan(dimensions, f64::from(*radius), 0)?;
                if work > cap {
                    return Err("legacy Gaussian exceeds bounded pixel-work limit".into());
                }
                PlaneOutcome::Samples(crate::gaussian_blur_filter::band(
                    dimensions,
                    f64::from(*radius),
                    0..dimensions.1,
                    range,
                    premultiplied(read),
                    job,
                )?)
            }
            Self::Photo { neighborhood, .. } => PlaneOutcome::Samples(neighborhood_plane_in(
                *neighborhood,
                dimensions,
                cap,
                range,
                read,
                job,
            )?),
            Self::Sharpen { sigma } => PlaneOutcome::Samples(neighborhood_plane_in(
                Neighborhood::Gaussian {
                    sigma: f64::from(*sigma),
                },
                dimensions,
                cap,
                range,
                read,
                job,
            )?),
            Self::Spatial(kernel) => {
                PlaneOutcome::Samples(kernel.plane_in(read, dimensions, cap, range, job)?)
            }
            Self::DevelopFilter { display, .. } => {
                if display.identity() {
                    PlaneOutcome::Pointwise
                } else {
                    PlaneOutcome::Samples(display.plane_in(read, dimensions, cap, range, job)?)
                }
            }
            Self::Relief { emboss } => {
                PlaneOutcome::Samples(relief_plane(read, dimensions, *emboss, range, job)?)
            }
            Self::Pixelate { block } => {
                PlaneOutcome::Samples(pixelate_plane_f32(read, dimensions, *block, job)?)
            }
            Self::Noise { .. } => PlaneOutcome::Pointwise,
            Self::Liquify(strokes) => {
                match crate::liquify::plane_in(read, dimensions, strokes, range, job)? {
                    Some(plane) => PlaneOutcome::Samples(plane),
                    None => PlaneOutcome::Identity,
                }
            }
            Self::DepthBlur(depth) => {
                PlaneOutcome::Samples(depth.plane_in(read, dimensions, range, encoding, job)?)
            }
        })
    }

    /// One output pixel from its straight source (in the route's domain) and
    /// its plane sample (`None` for a pointwise route). Add Noise draws its
    /// three deltas from `rng` in channel order, as every depth does.
    pub(crate) fn evaluate(
        &self,
        source: [f32; 4],
        sample: Option<[f32; 4]>,
        range: &SampleRange,
        rng: &mut SplitMix64,
    ) -> [f32; 4] {
        let plane = sample.unwrap_or(source);
        match self {
            Self::GaussianBlur { .. } => range.straight(plane),
            Self::Photo { kind, .. } => evaluated_in(*kind, source, plane, range),
            Self::Sharpen { .. } => evaluated_in(
                FilterKind::UnsharpMask {
                    amount: 1.0,
                    threshold: 0.0,
                },
                source,
                plane,
                range,
            ),
            Self::Spatial(kernel) => kernel.combine_in(source, plane, range),
            Self::DevelopFilter { .. } => match sample {
                Some(sample) => sample,
                None => self.front(source),
            },
            Self::Relief { .. } | Self::Pixelate { .. } | Self::Liquify(_) => plane,
            Self::DepthBlur(_) => plane,
            Self::Noise { amount, .. } => {
                let mut out = source;
                for c in out.iter_mut().take(3) {
                    *c = noise_relative(*c, noise_delta(rng, *amount));
                }
                out
            }
        }
    }

    /// Add Noise's seed; 0 for every other route (none draws).
    pub(crate) fn seed(&self) -> u64 {
        match self {
            Self::Noise { seed, .. } => *seed,
            _ => 0,
        }
    }
}

impl PrecisionDocument {
    /// The display encoding of this 32-bit float document (`display_encoding`):
    /// its profile's model and the float policy's forward encoder.
    pub(crate) fn display_encoding(&self) -> Result<DisplayEncoding<'_>, String> {
        let policy = self
            .float
            .as_deref()
            .ok_or("a display encoding needs a 32-bit float document")?;
        let model = self
            .profile()
            .model()
            .ok_or("the document profile has no matrix/TRC model to encode through")?;
        DisplayEncoding::new(model, policy.encoder())
    }
}
