//! Native destructive filter dispatch. Photographic neighborhoods/combine are
//! `filter_kernels`'. The 16-bit `blur` (Filter ▸ Blur ▸ Gaussian Blur) runs
//! the fitted kernel, `gaussian_blur_filter`, wherever its law runs
//! (`gaussian_blur_filter::runs`). The legacy sharpen (and `blur` below that
//! law) runs the image-crate algorithm in halo tiles below
//! `LEGACY_FAST_SIGMA`, and the radius-independent `sampling_gaussian_fast`
//! from it upward. The legacy sharpen control remains a sigma,
//! not a gain.
//!
//! Photographic filters share `filter_kernels`; legacy blur/sharpen call the image crate in admitted
//! halo tiles below σ4 and preserve the historical sigma/alpha formula. A 32-bit float layer's
//! legacy Sharpen (`float_filter_route::Route::Sharpen`) is Unsharp Mask amount 1, threshold 0 at
//! σ = min(amount, 5) on the layer's DISPLAY-ENCODED copy (`display_encoding`): the FIR Gaussian
//! `blur`'s float route runs, with no code quantization and no clamp, then decoded back, so an
//! SDR picture sharpens as at 16 bits and a value above white keeps its light; the user's own
//! `unsharp_mask` stays in linear light. The 16-bit layer path
//! evaluates the legacy blur/sharpen and Surface Blur in `BAND_ROWS` (128) row bands
//! (`band_scratch` / `plane_band`, `legacy_blur` the one-band wrapper of `legacy_band`), so its plan
//! charges one band of the f32 plane instead of n×16 B, with bit-identical output; the scalar
//! mask/channel path and the byte `filter_op` keep whole-plane `scratch` / `plane`.
//!
//! Liquify is the `Liquify` arm: a stroke list parsed by
//! `liquify::parse_strokes`, planned by `liquify::plan` (its own stroke-work
//! refusal, no work ceiling) and run as `liquify::plane`, whose `None` (an
//! identity field) leaves every pixel as it was. It is a layer-pixel edit
//! only: `precision_scalar_filter` refuses it for a mask or channel.
//!
//! Depth Blur is the `DepthBlur` arm: the request parsed by
//! `depth_blur::DepthBlur::parse` (the host's depth map rides in the params,
//! as Liquify's strokes do), planned by `DepthBlur::plan` (no work ceiling)
//! and run as `DepthBlur::plane_in` at `Unit`, whose plane is the final
//! sample.
//!
//! Develop Filter's masks are the `DevelopFilterMasked` arm: a
//! `develop_filter` request whose `masks` is not empty, parsed by
//! `develop_filter_masks::DevelopFilterMasked::parse`, planned by its `plan` and run
//! as its `plane_in` at `Unit`, whose plane is the final sample (as
//! `DepthBlur`'s is). Without masks `develop_filter` stays the `DevelopFilter` spatial
//! arm, bit for bit.
use crate::depth_blur::DepthBlur;
use crate::develop_filter_masks::DevelopFilterMasked;
use crate::filter_kernels::{evaluated, neighborhood_plan, neighborhood_plane};
use crate::filter_kernels::{FilterKind, Neighborhood, SampleRange};
use crate::icc_transform::PrecisionToneEncoder;
use crate::job_control::JobContext;
use crate::liquify::LiquifyStroke;
use crate::ops_spatial::{Frame, SpatialKernel};
use crate::precision_image::{reserved_vec, PrecisionImage, PrecisionRun, BOOKKEEPING_BYTES};
use crate::rng::SplitMix64;
use crate::sampling_develop_filter::{DevelopFilter, DEVELOP_FILTER_KEYS};
use crate::sampling_gaussian::destructive_work_cap;
use crate::sampling_gaussian_fast::{self as fast, LEGACY_FAST_SIGMA};
use crate::sampling_lens::LensCorrection;
use crate::sampling_offset::OffsetEdge;
use crate::sampling_presence::Presence;
use crate::sampling_radial::{RadialMethod, RadialQuality};
use crate::sampling_smart_sharpen::{SharpenRemove, SmartSharpen, TonalFade, SMART_SHARPEN_KEYS};
use crate::sampling_surface::{surface_band, surface_band_plan};
use crate::selected_auto_levels::AutoLevels;
use serde_json::{Map, Value};
use std::ops::Range;

/// Rows per band: the legacy Gaussian's tile height, so a band runs exactly
/// the tiles `legacy_blur` runs.
pub(crate) const BAND_ROWS: u32 = 128;

/// A 16-bit image's vignette gain in its own profile's linear light: decode
/// through the profile's matrix/TRC model, scale by 2^stops, encode back with
/// `PrecisionToneEncoder` (the `raw_local_develop` vignette pattern), so
/// Adobe RGB and ProPhoto planes are corrected in their own basis rather than
/// sRGB's. Refuses `adjust_calibrated::PROFILE_REFUSAL` without a model.
/// The gain of the 16-bit lens arm below and of Auto-Align's Vignette
/// Removal (`precision_align_vignette`).
pub(crate) fn profile_linear_gain(
    image: &PrecisionImage,
) -> Result<impl Fn([f32; 3], f32) -> [f32; 3] + '_, String> {
    let model = image
        .profile()
        .model()
        .ok_or(crate::adjust_calibrated::PROFILE_REFUSAL)?;
    let encoder = PrecisionToneEncoder::new(model)?;
    Ok(move |rgb: [f32; 3], stops: f32| {
        let exposure = crate::adjust_tone::Exposure::gain_only(stops);
        encoder.rgb(exposure.apply_linear(model.decode_rgb(rgb)))
    })
}

pub(crate) enum SelectedFilter {
    Photo {
        neighborhood: Neighborhood,
        kind: FilterKind,
        bypass: bool,
    },
    LegacyBlur {
        sigma: f32,
        sharpen: bool,
    },
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
    LevelsChannels(AutoLevels),
    Spatial(SpatialKernel),
    Liquify(Vec<LiquifyStroke>),
    DepthBlur(Box<DepthBlur>),
    DevelopFilterMasked(Box<DevelopFilterMasked>),
}
impl SelectedFilter {
    pub fn parse(op: &str, p: &Map<String, Value>) -> Result<Self, String> {
        let allowed: &[&str] = match op {
            "blur" => &["sigma"],
            "sharpen" => &["amount"],
            "gaussian_blur" | "high_pass" => &["radius"],
            "unsharp_mask" => &["radius", "amount", "threshold"],
            "reduce_noise" => &["radius", "luminance", "detail", "color"],
            "dust_and_scratches" => &["radius", "threshold"],
            "pixelate" => &["block"],
            "add_noise" => &["amount", "seed"],
            "edge_detect" | "emboss" => &[],
            "levels_channels" => &["black", "white", "gamma"],
            "box_blur" => &["radius"],
            "surface_blur" => &["radius", "threshold"],
            "add_grain" => &["amount", "size", "roughness", "seed"],
            "radial_blur" => &[
                "amount",
                "method",
                "quality",
                "center_x",
                "center_y",
                "region_x",
                "region_y",
                "region_width",
                "region_height",
            ],
            "clouds" | "difference_clouds" => &[
                "seed",
                "foreground_color",
                "background_color",
                "high_contrast",
            ],
            "offset" => &[
                "horizontal",
                "vertical",
                "undefined_areas",
                "fill",
                "region_x",
                "region_y",
                "region_width",
                "region_height",
            ],
            "motion_blur" => &["angle", "distance"],
            "lens_blur" => &["radius", "brightness", "threshold"],
            "tilt_shift" => &[
                "radius",
                "center_x",
                "center_y",
                "angle",
                "focus",
                "transition",
            ],
            "lens_correction" => &[
                "distortion",
                "chromatic_red_cyan",
                "chromatic_blue_yellow",
                "vignette_amount",
                "vignette_midpoint",
                "vertical_perspective",
                "horizontal_perspective",
                "angle",
                "scale",
                "profile",
            ],
            "presence" => &["texture", "clarity", "dehaze"],
            "develop_filter" => &DEVELOP_FILTER_KEYS,
            "smart_sharpen" => &SMART_SHARPEN_KEYS,
            "liquify" => &["strokes"],
            // `DepthBlur::parse` holds its own strict key list.
            "depth_blur" => return Ok(Self::DepthBlur(Box::new(DepthBlur::parse(p)?))),
            _ => return Err(format!("unsupported selected filter {op}")),
        };
        // `develop_filter` also takes its masks (`develop_filter_masks`).
        let extra: &[&str] = match op {
            "develop_filter" => &crate::develop_filter_masks::REQUEST_KEYS,
            _ => &[],
        };
        if let Some(key) = p
            .keys()
            .find(|k| !allowed.contains(&k.as_str()) && !extra.contains(&k.as_str()))
        {
            return Err(format!("unexpected {op} parameter {key}"));
        }
        let number = |name: &str, default: f64, lo: f64, hi: f64| -> Result<f64, String> {
            let n = p.get(name).map_or(Ok(default), |v| {
                v.as_f64().ok_or_else(|| format!("{name} must be numeric"))
            })?;
            if !n.is_finite() || !(lo..=hi).contains(&n) {
                return Err(format!("{name} must be finite in [{lo},{hi}]"));
            }
            Ok(n)
        };
        let integer = |name: &str, default: f64, lo: f64, hi: f64| -> Result<u32, String> {
            let n = number(name, default, lo, hi)?;
            if n != n.round() {
                return Err(format!("{name} must be an integer"));
            }
            Ok(n as u32)
        };
        let seed = || -> Result<u64, String> {
            Ok(match p.get("seed") {
                None => 1,
                Some(v) => v
                    .as_u64()
                    .or_else(|| v.as_i64().map(|v| v as u64))
                    .ok_or("seed must be a 64-bit integer")?,
            })
        };
        let choice = |name: &str, default: &'static str, allowed: &[&'static str]| {
            let Some(v) = p.get(name) else {
                return Ok(default);
            };
            v.as_str()
                .and_then(|s| allowed.iter().copied().find(|a| *a == s))
                .ok_or_else(|| format!("{name} must be one of {}", allowed.join(", ")))
        };
        let flag = |name: &str| -> Result<bool, String> {
            p.get(name).map_or(Ok(false), |v| {
                v.as_bool()
                    .ok_or_else(|| format!("{name} must be true or false"))
            })
        };
        let color = |name: &str, default: [u8; 3]| -> Result<[u8; 3], String> {
            p.get(name).map_or(Ok(default), |v| {
                v.as_str()
                    .and_then(crate::style_names::parse_color)
                    .ok_or_else(|| format!("{name} must be a #rrggbb colour"))
            })
        };
        // A pixel offset: any integer the largest image could need, either way.
        let signed = |name: &str| -> Result<i64, String> {
            let limit = crate::doc::MAX_PIXELS as f64;
            let n = number(name, 0., -limit, limit)?;
            if n != n.round() {
                return Err(format!("{name} must be an integer"));
            }
            Ok(n as i64)
        };
        // The 8-bit host's frame (Radial Blur, Offset): all four or none.
        let region = || -> Result<Option<Frame>, String> {
            let keys = ["region_x", "region_y", "region_width", "region_height"];
            let present = keys.iter().filter(|k| p.contains_key(**k)).count();
            if present == 0 {
                return Ok(None);
            }
            if present != keys.len() {
                return Err(
                    "region needs all of region_x, region_y, region_width and region_height".into(),
                );
            }
            let max = f64::from(u32::MAX);
            Ok(Some(Frame {
                x: integer("region_x", 0., 0., max)?,
                y: integer("region_y", 0., 0., max)?,
                width: integer("region_width", 0., 0., max)?,
                height: integer("region_height", 0., 0., max)?,
            }))
        };
        Ok(match op {
            "blur" => {
                let sigma = number("sigma", 4., f64::from(f32::MIN_POSITIVE), 64.)? as f32;
                Self::LegacyBlur {
                    sigma,
                    sharpen: false,
                }
            }
            "sharpen" => {
                let amount = number(
                    "amount",
                    1.,
                    f64::from(f32::MIN_POSITIVE),
                    f64::from(f32::MAX),
                )?;
                Self::LegacyBlur {
                    sigma: amount.min(5.) as f32,
                    sharpen: true,
                }
            }
            "edge_detect" | "emboss" => Self::Relief {
                emboss: op == "emboss",
            },
            "pixelate" => {
                let block = number("block", 8., 1., 1024.)?;
                if block != block.round() {
                    return Err("block must be an integer".into());
                }
                Self::Pixelate {
                    block: block as u32,
                }
            }
            "add_noise" => {
                let seed = seed()?;
                Self::Noise {
                    amount: number("amount", 0.2, f64::from(f32::MIN_POSITIVE), 1.)? as f32,
                    seed,
                }
            }
            // Surface Blur's threshold is in 8-bit LEVELS, unlike the
            // normalized thresholds above: it is the tent's scale, not a gate.
            "box_blur" => Self::Spatial(SpatialKernel::BoxBlur {
                radius: integer("radius", 5., 1., 1000.)?,
            }),
            "surface_blur" => Self::Spatial(SpatialKernel::SurfaceBlur {
                radius: integer("radius", 5., 1., 100.)?,
                threshold: number("threshold", 15., 2., 255.)?,
            }),
            "add_grain" => Self::Spatial(SpatialKernel::AddGrain {
                amount: number("amount", 25., 0., 100.)?,
                size: number("size", 1., 0.5, 4.)?,
                roughness: number("roughness", 50., 0., 100.)?,
                seed: seed()?,
            }),
            "radial_blur" => Self::Spatial(SpatialKernel::RadialBlur {
                amount: integer("amount", 10., 1., 100.)?,
                method: match choice("method", "spin", &["spin", "zoom"])? {
                    "zoom" => RadialMethod::Zoom,
                    _ => RadialMethod::Spin,
                },
                quality: match choice("quality", "good", &["draft", "good", "best"])? {
                    "draft" => RadialQuality::Draft,
                    "best" => RadialQuality::Best,
                    _ => RadialQuality::Good,
                },
                center: (
                    number("center_x", 0.5, 0., 1.)?,
                    number("center_y", 0.5, 0., 1.)?,
                ),
                frame: region()?,
            }),
            "clouds" | "difference_clouds" => Self::Spatial(SpatialKernel::Clouds {
                seed: seed()?,
                foreground: color("foreground_color", [0, 0, 0])?,
                background: color("background_color", [255, 255, 255])?,
                high_contrast: flag("high_contrast")?,
                difference: op == "difference_clouds",
            }),
            // The host resolves Set to Background into
            // `transparent` (a layer) or `fill` (one value for a mask, colour
            // plane or channel); the kernel never sees a colour.
            "offset" => {
                let mode = choice(
                    "undefined_areas",
                    "wrap",
                    &["transparent", "repeat", "wrap", "fill"],
                )?;
                if mode != "fill" && p.contains_key("fill") {
                    return Err("fill belongs only to undefined_areas fill".into());
                }
                if mode == "fill" && !p.contains_key("fill") {
                    return Err("undefined_areas fill needs fill".into());
                }
                Self::Spatial(SpatialKernel::Offset {
                    horizontal: signed("horizontal")?,
                    vertical: signed("vertical")?,
                    edge: match mode {
                        "transparent" => OffsetEdge::Transparent,
                        "repeat" => OffsetEdge::Repeat,
                        "fill" => OffsetEdge::Fill(number("fill", 1., 0., 1.)? as f32),
                        _ => OffsetEdge::Wrap,
                    },
                    frame: region()?,
                })
            }
            "motion_blur" => Self::Spatial(SpatialKernel::MotionBlur {
                angle: number("angle", 0., -360., 360.)?,
                distance: integer("distance", 10., 1., 2000.)?,
            }),
            // Lens Blur's threshold is in 8-bit LEVELS, like Surface Blur's.
            "lens_blur" => Self::Spatial(SpatialKernel::LensBlur {
                radius: integer("radius", 15., 1., 100.)?,
                brightness: number("brightness", 0., 0., 100.)?,
                threshold: number("threshold", 255., 0., 255.)?,
            }),
            "tilt_shift" => Self::Spatial(SpatialKernel::TiltShift {
                radius: integer("radius", 15., 1., 100.)?,
                center: (
                    number("center_x", 0.5, 0., 1.)?,
                    number("center_y", 0.5, 0., 1.)?,
                ),
                angle: number("angle", 0., -90., 90.)?,
                focus: number("focus", 0.15, 0., 1.)?,
                transition: number("transition", 0.25, 0., 1.)?,
            }),
            // Lens Correction's sliders: −100…100 each, the vignette midpoint
            // 0…100, the angle in degrees (positive turns the picture
            // counterclockwise), the scale in percent.
            "lens_correction" => Self::Spatial(SpatialKernel::LensCorrection(LensCorrection {
                distortion: number("distortion", 0., -100., 100.)?,
                chromatic_red_cyan: number("chromatic_red_cyan", 0., -100., 100.)?,
                chromatic_blue_yellow: number("chromatic_blue_yellow", 0., -100., 100.)?,
                vignette_amount: number("vignette_amount", 0., -100., 100.)?,
                vignette_midpoint: number("vignette_midpoint", 50., 0., 100.)?,
                vertical_perspective: number("vertical_perspective", 0., -100., 100.)?,
                horizontal_perspective: number("horizontal_perspective", 0., -100., 100.)?,
                angle: number("angle", 0., -180., 180.)?,
                scale: number("scale", 100., 50., 150.)?,
                // A resolved Lensfun profile (`dr_lens_match`'s `profile`),
                // strict: `LensProfile::from_json` names what it refuses.
                profile: p
                    .get("profile")
                    .map(crate::lens_model::LensProfile::from_json)
                    .transpose()?,
            })),
            // Presence's three amounts are −100…100 each and 0 is off; all
            // three at 0 are the identity.
            "presence" => Self::Spatial(SpatialKernel::Presence(Presence {
                texture: number("texture", 0., -100., 100.)?,
                clarity: number("clarity", 0., -100., 100.)?,
                dehaze: number("dehaze", 0., -100., 100.)?,
            })),
            // Develop Filter's controls are −100…100 each, Exposure −4…4
            // stops (Develop's range); 0 is off and all thirteen at 0 are the
            // identity. A non-empty `masks` is the masked arm
            // (`develop_filter_masks`); `range_profile` is read only there.
            "develop_filter" if crate::develop_filter_masks::has_masks(p) => {
                Self::DevelopFilterMasked(Box::new(DevelopFilterMasked::parse(p)?))
            }
            "develop_filter" => {
                let unit = |name: &str| number(name, 0., -100., 100.);
                Self::Spatial(SpatialKernel::DevelopFilter(DevelopFilter {
                    temperature: unit("temperature")?,
                    tint: unit("tint")?,
                    exposure: number("exposure", 0., -4., 4.)?,
                    contrast: unit("contrast")?,
                    highlights: unit("highlights")?,
                    shadows: unit("shadows")?,
                    whites: unit("whites")?,
                    blacks: unit("blacks")?,
                    texture: unit("texture")?,
                    clarity: unit("clarity")?,
                    dehaze: unit("dehaze")?,
                    vibrance: unit("vibrance")?,
                    saturation: unit("saturation")?,
                }))
            }
            // Smart Sharpen's controls: amount in percent, radius in pixels,
            // Reduce Noise and the fades and tonal widths in percent, the
            // tonal radii in whole pixels. It is never an identity.
            "smart_sharpen" => {
                let section = |prefix: &str| -> Result<TonalFade, String> {
                    Ok(TonalFade {
                        fade: number(&format!("{prefix}_fade"), 0., 0., 100.)?,
                        tonal_width: number(&format!("{prefix}_tonal_width"), 50., 0., 100.)?,
                        radius: integer(&format!("{prefix}_radius"), 1., 1., 100.)?,
                    })
                };
                Self::Spatial(SpatialKernel::SmartSharpen(SmartSharpen {
                    amount: number("amount", 200., 1., 500.)?,
                    radius: number("radius", 1., 0.1, 64.)?,
                    noise: number("noise", 10., 0., 100.)?,
                    remove: match choice("remove", "lens", &["gaussian", "lens", "motion"])? {
                        "gaussian" => SharpenRemove::Gaussian,
                        "motion" => SharpenRemove::Motion,
                        _ => SharpenRemove::Lens,
                    },
                    angle: number("angle", 0., -180., 180.)?,
                    shadows: section("shadow")?,
                    highlights: section("highlight")?,
                }))
            }
            "liquify" => Self::Liquify(crate::liquify::parse_strokes(p)?),
            "levels_channels" => {
                let array = |name: &str, default: f32| -> Result<[f32; 3], String> {
                    let Some(v) = p.get(name) else {
                        return Ok([default; 3]);
                    };
                    let a = v
                        .as_array()
                        .filter(|a| a.len() == 3)
                        .ok_or("levels channel parameters need three numbers")?;
                    let mut result = [0.; 3];
                    for c in 0..3 {
                        result[c] =
                            a[c].as_f64().ok_or("channel parameter must be numeric")? as f32;
                    }
                    Ok(result)
                };
                let levels = AutoLevels {
                    black: array("black", 0.)?,
                    white: array("white", 1.)?,
                    gamma: array("gamma", 1.)?,
                };
                for c in 0..3 {
                    if !(levels.black[c] >= 0.
                        && levels.black[c] < levels.white[c]
                        && levels.white[c] <= 1.
                        && (0.1..=10.).contains(&levels.gamma[c]))
                    {
                        return Err("invalid channel levels range or gamma".into());
                    }
                }
                Self::LevelsChannels(levels)
            }
            _ => {
                let default_radius = match op {
                    "unsharp_mask" => 1.5,
                    "high_pass" => 3.,
                    "dust_and_scratches" => 2.,
                    _ => 1.,
                };
                let radius = number("radius", default_radius, 0., 64.)?;
                let mut neighborhood = Neighborhood::Gaussian { sigma: radius };
                let mut bypass = radius == 0.;
                let kind = match op {
                    "gaussian_blur" => FilterKind::GaussianBlur,
                    "unsharp_mask" => {
                        let amount = number("amount", 1., 0., 5.)?;
                        bypass |= amount == 0.;
                        FilterKind::UnsharpMask {
                            amount,
                            threshold: number("threshold", 0., 0., 1.)?,
                        }
                    }
                    "high_pass" => FilterKind::HighPass,
                    "dust_and_scratches" => {
                        let threshold = number("threshold", 0., 0., 1.)?;
                        bypass |= radius < 1. || threshold == 1.;
                        neighborhood = Neighborhood::Rank { radius };
                        FilterKind::DustAndScratches { threshold }
                    }
                    "reduce_noise" => {
                        let luminance = number("luminance", 0.01, 0., 0.25)?;
                        let detail = number("detail", 0.5, 0., 1.)?;
                        let color = number("color", 0.5, 0., 1.)?;
                        let (luma, chroma) =
                            crate::sampling_denoise::halves(luminance, detail, color);
                        bypass |= !(luma || chroma);
                        neighborhood = Neighborhood::Denoise {
                            sigma: radius,
                            luminance,
                            luma,
                            chroma,
                        };
                        FilterKind::ReduceNoise {
                            luminance,
                            detail,
                            color,
                        }
                    }
                    _ => unreachable!(),
                };
                Self::Photo {
                    neighborhood,
                    kind,
                    bypass,
                }
            }
        })
    }
    /// The run-time frame of Radial Blur and Offset (the selection's
    /// bounding box in plane pixels, or `None` for the whole plane), replacing
    /// any parsed `region_*`; every other filter is returned unchanged.
    pub(crate) fn with_frame(self, frame: Option<Frame>) -> Self {
        match self {
            Self::Spatial(SpatialKernel::RadialBlur {
                amount,
                method,
                quality,
                center,
                ..
            }) => Self::Spatial(SpatialKernel::RadialBlur {
                amount,
                method,
                quality,
                center,
                frame,
            }),
            Self::Spatial(SpatialKernel::Offset {
                horizontal,
                vertical,
                edge,
                ..
            }) => Self::Spatial(SpatialKernel::Offset {
                horizontal,
                vertical,
                edge,
                frame,
            }),
            other => other,
        }
    }
    /// Whether the filter acts within a frame (`with_frame` changes it).
    pub(crate) fn framed(&self) -> bool {
        matches!(
            self,
            Self::Spatial(SpatialKernel::RadialBlur { .. } | SpatialKernel::Offset { .. })
        )
    }
    pub fn identity(&self) -> bool {
        match self {
            Self::Photo { bypass, .. } => *bypass,
            Self::Pixelate { block } => *block == 1,
            Self::LevelsChannels(l) => l.identity(),
            Self::Spatial(k) => k.identity(),
            Self::DepthBlur(d) => d.identity(),
            Self::DevelopFilterMasked(m) => m.identity(),
            _ => false,
        }
    }
    pub fn seed(&self) -> u64 {
        if let Self::Noise { seed, .. } = self {
            *seed
        } else {
            0
        }
    }
    /// Whole-plane scratch, refused when the plan's work exceeds `cap`
    /// (`CONTROLLED_JOB_WORK` for a plan, `destructive_work_cap(job)` for a
    /// run). The Liquify arm ignores `cap`: `liquify::plan` bounds its
    /// own work below every ceiling a caller holds (`liquify::MAX_WORK`).
    pub fn scratch(&self, dimensions: (u32, u32), cap: u64) -> Result<u64, String> {
        if self.identity() {
            return Ok(0);
        }
        let n = u64::from(dimensions.0) * u64::from(dimensions.1);
        match self {
            Self::Photo { neighborhood, .. } => {
                Ok(neighborhood_plan(*neighborhood, dimensions, cap)?.0)
            }
            Self::LegacyBlur { sigma, sharpen } => legacy_plan(dimensions, *sigma, *sharpen, cap),
            Self::Relief { .. } => Ok(n * 16 + BOOKKEEPING_BYTES),
            Self::Pixelate { .. } => Ok(n * 16 + BOOKKEEPING_BYTES),
            Self::Noise { .. } | Self::LevelsChannels(_) => Ok(0),
            // A profile's vignetting is applied in the image profile's own
            // linear basis at 16 bits (`plane`), whose encoder tables are held
            // beside the kernel's plane.
            Self::Spatial(SpatialKernel::LensCorrection(lens)) if lens.profile_vignetting() => {
                Ok(lens.plan(dimensions, cap)?.0 + PrecisionToneEncoder::scratch_bytes())
            }
            Self::Spatial(k) => Ok(k.plan(dimensions, cap)?.0),
            Self::Liquify(strokes) => crate::liquify::plan(dimensions, strokes),
            Self::DepthBlur(d) => Ok(d.plan(dimensions)?.0),
            Self::DevelopFilterMasked(m) => Ok(m.plan(dimensions, cap)?.0),
        }
    }
    /// Scratch of one band of the banded plane, or None for a filter
    /// that only runs whole-plane. Some only for the legacy blur and sharpen
    /// and for Surface Blur, whose planes are row-local outside a fixed halo.
    /// Refuses with the same work errors as `scratch` at the same `cap`: the
    /// work bound covers all bands.
    pub fn band_scratch(&self, dimensions: (u32, u32), cap: u64) -> Result<Option<u64>, String> {
        Ok(match self {
            Self::LegacyBlur { sigma, sharpen } => {
                Some(legacy_band_scratch(dimensions, *sigma, *sharpen, cap)?)
            }
            Self::Spatial(SpatialKernel::SurfaceBlur { radius, threshold }) => {
                Some(surface_band_plan(dimensions, *radius, *threshold, BAND_ROWS, cap)?.0)
            }
            _ => None,
        })
    }
    /// Rows `rows` of exactly what `plane` returns, bit for bit:
    /// `rows.len()·width` samples, row-major. `rows.start` is a multiple of
    /// `BAND_ROWS`. Reports progress 0..1 over its own rows only; the caller
    /// ranges `job` per band.
    pub fn plane_band(
        &self,
        image: &PrecisionImage,
        rows: Range<u32>,
        job: JobContext<'_>,
    ) -> Result<Vec<[f32; 4]>, String> {
        if !rows.start.is_multiple_of(BAND_ROWS) {
            return Err("selected filter band must start on a band boundary".into());
        }
        match self {
            Self::LegacyBlur { sigma, sharpen } => legacy_band(image, *sigma, *sharpen, rows, job),
            Self::Spatial(SpatialKernel::SurfaceBlur { radius, threshold }) => surface_band(
                |i| image.pixel(i),
                image.dimensions(),
                *radius,
                *threshold,
                rows,
                BAND_ROWS,
                destructive_work_cap(job),
                job,
            ),
            _ => Err("selected filter has no banded plane".into()),
        }
    }
    /// The filter's plane over `image`. `linear` is a linear-light 16-bit
    /// document's transfer, which only Liquify reads (its resample mixes in
    /// linear light, 11E D7); the other filters edit the stored codes (D9).
    pub(crate) fn plane(
        &self,
        image: &PrecisionImage,
        linear: Option<&dyn crate::sampling_linear::LinearUnit>,
        job: JobContext<'_>,
    ) -> Result<Option<Vec<[f32; 4]>>, String> {
        let (w, h) = image.dimensions();
        let n = w as usize * h as usize;
        Ok(match self {
            Self::Photo { neighborhood, .. } => Some(neighborhood_plane(
                *neighborhood,
                (w, h),
                destructive_work_cap(job),
                |i| image.pixel(i),
                job,
            )?),
            Self::LegacyBlur { sigma, sharpen } => Some(legacy_blur(image, *sigma, *sharpen, job)?),
            Self::Relief { emboss } => Some(crate::ops_filters::relief_plane(
                |i| image.pixel(i),
                (w, h),
                *emboss,
                &SampleRange::Unit,
                job,
            )?),
            Self::Pixelate { block } => {
                let mut out = reserved_vec(n)?;
                out.resize(n, [0.; 4]);
                for cy in (0..h).step_by(*block as usize) {
                    for cx in (0..w).step_by(*block as usize) {
                        job.check()?;
                        let ch = (*block).min(h - cy);
                        let cw = (*block).min(w - cx);
                        let mut ca = [0_u64; 3];
                        let mut alpha = 0_u64;
                        for y in cy..cy + ch {
                            job.check()?;
                            for (_, run) in image.row_segments(y, cx, cx + cw) {
                                let PrecisionRun::Rgba16(run) = run else {
                                    return Err("selected edit requires u16 source".into());
                                };
                                for p in run {
                                    alpha += u64::from(p[3]);
                                    for c in 0..3 {
                                        ca[c] += u64::from(p[c]) * u64::from(p[3]);
                                    }
                                }
                            }
                        }
                        let mean = crate::ops_filters::pixelate_mean(
                            ca,
                            alpha,
                            u64::from(cw) * u64::from(ch),
                        )
                        .map(|v| (v / 65535.) as f32);
                        for y in cy..cy + ch {
                            job.check()?;
                            for x in cx..cx + cw {
                                out[y as usize * w as usize + x as usize] = mean;
                            }
                        }
                    }
                }
                Some(out)
            }
            Self::Noise { .. } | Self::LevelsChannels(_) => None,
            // A 16-bit plane's Lensfun vignetting: decode through the image
            // profile's matrix/TRC model, scale by 2^stops in that linear
            // light, encode back (the `raw_local_develop` vignette pattern),
            // so Adobe RGB and ProPhoto planes are corrected in their own
            // basis rather than sRGB's.
            Self::Spatial(SpatialKernel::LensCorrection(lens)) if lens.profile_vignetting() => {
                let gain = profile_linear_gain(image)?;
                Some(lens.plane_with_gain(
                    |i| image.pixel(i),
                    (w, h),
                    destructive_work_cap(job),
                    &crate::filter_kernels::SampleRange::Unit,
                    job,
                    Some(&gain),
                )?)
            }
            Self::Spatial(k) => {
                Some(k.plane(|i| image.pixel(i), (w, h), destructive_work_cap(job), job)?)
            }
            Self::Liquify(strokes) => {
                crate::liquify::plane(|i| image.pixel(i), (w, h), strokes, linear, job)?
            }
            Self::DepthBlur(d) => {
                Some(d.plane_in(|i| image.pixel(i), (w, h), &SampleRange::Unit, None, job)?)
            }
            Self::DevelopFilterMasked(m) => {
                Some(m.plane_in(|i| image.pixel(i), (w, h), destructive_work_cap(job), job)?)
            }
        })
    }
    pub fn evaluate(
        &self,
        i: usize,
        source: [f32; 4],
        plane: Option<&[[f32; 4]]>,
        rng: &mut SplitMix64,
    ) -> [f32; 4] {
        match self {
            Self::Photo { kind, .. } => evaluated(*kind, source, plane.unwrap()[i]),
            Self::LegacyBlur { sharpen, .. } => {
                let low = plane.unwrap()[i];
                if !sharpen {
                    return low;
                }
                // Legacy sharpen quantizes its Gaussian before the subtraction.
                let mut result = std::array::from_fn(|c| {
                    ((2. * (source[c] * 65535.).round() - (low[c] * 65535.).round())
                        .clamp(0., 65535.))
                        / 65535.
                });
                if result[3] == 0. {
                    result = [0.; 4];
                }
                result
            }
            Self::Relief { .. } | Self::Pixelate { .. } => plane.unwrap()[i],
            Self::Spatial(k) => k.combine(source, plane.unwrap()[i]),
            // No plane: the field was the identity.
            Self::Liquify(_) => plane.map_or(source, |plane| plane[i]),
            // The plane is the final sample.
            Self::DepthBlur(_) | Self::DevelopFilterMasked(_) => {
                plane.map_or(source, |plane| plane[i])
            }
            Self::Noise { amount, .. } => {
                let mut result = source;
                for c in result.iter_mut().take(3) {
                    *c = (*c + crate::ops_filters::noise_delta(rng, *amount)).clamp(0., 1.);
                }
                result
            }
            Self::LevelsChannels(levels) => {
                if source[3] == 0. {
                    return source;
                }
                // `adjust_levels::stage_curve`, the toe included, so the
                // typed Auto commands match the byte ones.
                let mut result = source;
                for c in 0..3 {
                    result[c] = crate::adjust_levels::stage_curve(
                        (source[c] - levels.black[c]) / (levels.white[c] - levels.black[c]),
                        1. / levels.gamma[c],
                        crate::adjust_levels::levels_toe(levels.gamma[c]).as_ref(),
                    );
                }
                result
            }
        }
    }
}
// Halo conservatively exceeds the image crate's sigma-to-kernel half-width.
// Running the actual library kernel preserves its edge and SIMD policy without
// cloning that implementation. Each 128px core tile is cancellation bounded.
// The byte `sampling::blur` reads it for its bands below `LEGACY_FAST_SIGMA`.
pub(crate) fn legacy_halo(sigma: f32) -> u32 {
    (4. * sigma + 4.).ceil() as u32
}
fn legacy_plan(dimensions: (u32, u32), sigma: f32, sharpen: bool, cap: u64) -> Result<u64, String> {
    legacy_rows_scratch(dimensions, sigma, sharpen, dimensions.1, cap)
}
/// One band's scratch; the work check is still the whole plane's.
fn legacy_band_scratch(
    dimensions: (u32, u32),
    sigma: f32,
    sharpen: bool,
    cap: u64,
) -> Result<u64, String> {
    legacy_rows_scratch(dimensions, sigma, sharpen, BAND_ROWS, cap)
}
/// Whether this legacy filter runs the fitted Gaussian Blur: the
/// blur, not the sharpen, at a radius its law runs.
fn runs_fitted(sigma: f32, sharpen: bool) -> bool {
    !sharpen && crate::gaussian_blur_filter::runs(f64::from(sigma))
}
/// The whole plane's work refusal above `cap`, and the scratch of an output of
/// `rows` rows plus one window of the fitted Gaussian Blur
/// (`gaussian_blur_filter::plan`), one halo tile, or (at σ ≥
/// `LEGACY_FAST_SIGMA`) one fast window.
fn legacy_rows_scratch(
    dimensions: (u32, u32),
    sigma: f32,
    sharpen: bool,
    rows: u32,
    cap: u64,
) -> Result<u64, String> {
    if runs_fitted(sigma, sharpen) {
        let (scratch, work) =
            crate::gaussian_blur_filter::plan(dimensions, f64::from(sigma), rows)?;
        if work > cap {
            return Err("legacy Gaussian exceeds bounded pixel-work limit".into());
        }
        return Ok(scratch);
    }
    if sigma >= LEGACY_FAST_SIGMA {
        let (scratch, work) = fast::plan(dimensions, f64::from(sigma), rows)?;
        if work > cap {
            return Err("legacy Gaussian exceeds bounded pixel-work limit".into());
        }
        return Ok(scratch);
    }
    let (w, h) = dimensions;
    let halo = legacy_halo(sigma);
    let tw = w.min(128 + 2 * halo);
    let th = h.min(128 + 2 * halo);
    let tiles = u64::from(w.div_ceil(128)) * u64::from(h.div_ceil(128));
    let work = tiles * u64::from(tw) * u64::from(th) * u64::from(2 * halo + 1) * 8;
    if work > cap {
        return Err("legacy Gaussian exceeds bounded pixel-work limit".into());
    }
    Ok(u64::from(w) * u64::from(rows) * 16
        + u64::from(tw) * u64::from(th) * 96
        + 4 * BOOKKEEPING_BYTES)
}
fn legacy_blur(
    image: &PrecisionImage,
    sigma: f32,
    sharpen: bool,
    job: JobContext<'_>,
) -> Result<Vec<[f32; 4]>, String> {
    let h = image.dimensions().1;
    legacy_band(image, sigma, sharpen, 0..h, job)
}
/// Rows `rows` of `legacy_blur`'s plane: the same halo tiles for the tile rows
/// in `rows` (`rows.start` is tile-aligned), with band-local progress. The
/// blur where the fitted law runs is `gaussian_blur_filter::band`'s windows;
/// otherwise from `LEGACY_FAST_SIGMA` up the tiles are
/// `sampling_gaussian_fast`'s windows. Both are read premultiplied and
/// divided back out by the same rule.
fn legacy_band(
    image: &PrecisionImage,
    sigma: f32,
    sharpen: bool,
    rows: Range<u32>,
    job: JobContext<'_>,
) -> Result<Vec<[f32; 4]>, String> {
    let (w, h) = image.dimensions();
    legacy_plan((w, h), sigma, sharpen, destructive_work_cap(job))?;
    if !rows.start.is_multiple_of(BAND_ROWS) || rows.start >= rows.end || rows.end > h {
        return Err("legacy Gaussian band rows lie outside the plane".into());
    }
    let divide = |low: &mut Vec<[f32; 4]>| {
        for p in low.iter_mut() {
            *p = if (p[3] * 65535.).round() > 0. {
                [p[0] / p[3], p[1] / p[3], p[2] / p[3], p[3]]
            } else {
                [0.; 4]
            };
        }
    };
    if runs_fitted(sigma, sharpen) {
        let mut low = crate::gaussian_blur_filter::band(
            (w, h),
            f64::from(sigma),
            rows,
            &SampleRange::Unit,
            |i| {
                let p = image.pixel(i);
                [p[0] * p[3], p[1] * p[3], p[2] * p[3], p[3]]
            },
            job,
        )?;
        divide(&mut low);
        return Ok(low);
    }
    if sigma >= LEGACY_FAST_SIGMA {
        let mut low = fast::band(
            (w, h),
            f64::from(sigma),
            rows,
            |i| {
                let p = image.pixel(i);
                [p[0] * p[3], p[1] * p[3], p[2] * p[3], p[3]]
            },
            job,
        )?;
        divide(&mut low);
        return Ok(low);
    }
    let len = rows.end - rows.start;
    let halo = legacy_halo(sigma);
    let mut out = reserved_vec(w as usize * len as usize)?;
    out.resize(w as usize * len as usize, [0.; 4]);
    for y in rows.clone().step_by(128) {
        for x in (0..w).step_by(128) {
            job.check()?;
            let left = x.saturating_sub(halo);
            let top = y.saturating_sub(halo);
            let right = (x + 128 + halo).min(w);
            let bottom = (y + 128 + halo).min(h);
            let input = image::Rgba32FImage::from_fn(right - left, bottom - top, |px, py| {
                let p = image.pixel((top + py) as usize * w as usize + (left + px) as usize);
                image::Rgba([p[0] * p[3], p[1] * p[3], p[2] * p[3], p[3]])
            });
            let low = image::imageops::blur(&input, sigma);
            job.check()?;
            for py in y..(y + 128).min(rows.end) {
                for px in x..(x + 128).min(w) {
                    let p = low.get_pixel(px - left, py - top).0;
                    out[(py - rows.start) as usize * w as usize + px as usize] =
                        if (p[3] * 65535.).round() > 0. {
                            [p[0] / p[3], p[1] / p[3], p[2] / p[3], p[3]]
                        } else {
                            [0.; 4]
                        };
                }
            }
            job.report(
                ((y - rows.start) as f64 * w as f64 + x as f64) / (len as f64 * w as f64),
                6,
            )?;
        }
    }
    Ok(out)
}
