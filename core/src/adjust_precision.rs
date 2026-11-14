//! Explicit precision-v1 preparation of the existing adjustment interpreter.
//! Every pixel operator still dispatches through Adjustment::apply_at. Curves
//! and calibrated encoding select continuous preparations; scalar tone guides
//! retain f32 coverage. Parameter coordinates/colors keep their existing schema.
//!
//! `adjust_precision.rs` prepares the explicitly versioned typed policy using the same kernels.
//! Curves chooses continuous spline evaluation and calibrated operations choose the continuous
//! forward ICC encoder; Photo Filter binds the profile's D50 PCS basis with that encoder
//! (`adjust_photo_filter`). Parse also derives the op's 32-bit float form against the profile
//! (`adjust_float::prepare`) — Levels, `bcs`, Black & White (with Tint), Channel Mixer, Photo
//! Filter, Exposure of either evaluation, Curves in point and map (pencil) form, Hue/Saturation's
//! Colorize, master Lightness alone or master Hue alone, Color Balance, every Color Lookup table,
//! a calibrated White Balance, Hue Rotate, Grayscale and Sepia in closed form and the display
//! forms (Threshold, Posterize, Gradient Map, Vibrance, Selective Color, Replace Color and the
//! other Hue/Saturation settings, each holding its own encoder), or a refusal
//! sentence — which `apply_linear` evaluates; it is never persisted, so a re-parse against another
//! profile re-derives it.

use crate::adjust::Adjustment;
use crate::adjust_curves::CurveLuts;
use crate::adjust_float::FloatForm;
use crate::adjust_guide::GuidePlane;
use crate::icc::IccProfile;
use crate::job_control::JobContext;
use crate::resources::Inventory;
use crate::retained_source_render::lut_pipeline;
use serde_json::{Map, Value};

#[path = "adjust_precision_plan.rs"]
pub(crate) mod plan;

pub(crate) type PrecisionGuide = GuidePlane;

pub struct PrecisionAdjustment {
    description: String,
    operation: String,
    kernel: Adjustment,
    identity: bool,
    float: FloatForm,
    /// The Lab form bound at parse against the Lab twin (`lab_adjust::rule`)
    /// and what it carries (`lab_adjust::LabBinding`), which `apply_at` and
    /// `guide` route the kernel through (`lab_tone`).
    lab: Option<crate::lab_adjust::LabBinding>,
}

impl PrecisionAdjustment {
    /// Includes canonical JSON, parsed values, profile copies and forward
    /// encoding tables. It is an admission reservation, not retained usage.
    pub fn prepare_scratch_bytes(meta: &str, profile: &IccProfile) -> Result<u64, String> {
        check_length(meta)?;
        // A serde Value array can double capacity while the previous allocation
        // still exists. Include that reallocation peak even for an input later
        // refused by the operator's structural limits.
        Ok(meta.len() as u64 * 64 + profile.bytes().len() as u64 * 4 + 4 * 1024 * 1024)
    }
    /// The description prepared against `profile`. On the Lab twin
    /// (`lab_document::is_lab_twin`, only a Lab document holds it) the op's
    /// Lab form is bound, or its Lab refusal answered (`lab_adjust::rule`).
    pub fn parse(meta: &str, profile: &IccProfile) -> Result<Self, String> {
        Self::prepare(meta, profile, crate::lab_document::is_lab_twin(profile))
    }
    /// [`Self::parse`] with no Lab rule and no Lab form, whatever the
    /// profile: the scalar mask and alpha-channel path
    /// (`precision_scalar_filter::evaluate`) feeds `[g, g, g]` and keeps the
    /// Rec.709 luma of the result, so a Lab form would be wrong there.
    pub(crate) fn parse_unbound(meta: &str, profile: &IccProfile) -> Result<Self, String> {
        Self::prepare(meta, profile, false)
    }
    fn prepare(meta: &str, profile: &IccProfile, bind_lab: bool) -> Result<Self, String> {
        check_length(meta)?;
        let mut value: Value = serde_json::from_str(meta)
            .map_err(|e| format!("invalid high-depth adjustment JSON: {e}"))?;
        let root = value
            .as_object_mut()
            .ok_or("high-depth adjustment must be an object")?;
        keys(root, &["type", "evaluation", "op", "params"])?;
        if root.get("type").and_then(Value::as_str) != Some("adjust")
            || root.get("evaluation").and_then(Value::as_str) != Some("precision_v1")
        {
            return Err(
                "high-depth adjustment requires type:adjust and evaluation:precision_v1".into(),
            );
        }
        let op = root
            .get("op")
            .and_then(Value::as_str)
            .ok_or("high-depth adjustment requires an operation")?
            .to_string();
        let params = root
            .entry("params")
            .or_insert_with(|| Value::Object(Map::new()))
            .as_object_mut()
            .ok_or("adjustment params must be an object")?;
        let form = if bind_lab {
            Some(crate::lab_adjust::rule(&op, params)?)
        } else {
            None
        };
        parameter_keys(&op, params)?;
        if matches!(op.as_str(), "exposure" | "white_balance") {
            params
                .entry("evaluation")
                .or_insert_with(|| Value::String("document_v2".into()));
        }
        // A Lab document's colour ops run on the sample's colour in the op's
        // RGB space (`lab_adjust::color_space`: ProPhoto RGB, or sRGB for the
        // two lookup ops; `lab_tone`'s module doc, "Color"), so the twin ↔
        // space pipelines bind first and the kernel is prepared against the
        // space from the request colours in its numbers
        // (`lab_adjust::kernel_params`, which reuses the bound twin → space
        // pipeline). The float form stays the twin's (Lab has no 32-bit, and
        // under the twin every display form is refused, so it builds no
        // encoder).
        let (kernel, lab) = if form == Some(crate::lab_tone::LabForm::Color) {
            let space = crate::lab_adjust::color_space(&op);
            let color = color_pipelines(profile, &space)?;
            let request = crate::lab_adjust::kernel_params(&op, params, &space, &color[0])?;
            let kernel = precision_kernel(&op, &request, &space)?;
            let lab = crate::lab_adjust::LabBinding {
                form: crate::lab_tone::LabForm::Color,
                channels: None,
                chroma: 1.0,
                color: Some(color),
            };
            (kernel, Some(lab))
        } else {
            let kernel = precision_kernel(&op, params, profile)?;
            let lab = match form {
                Some(form) => Some(bind(form, &op, params, &kernel, profile)?),
                None => None,
            };
            (kernel, lab)
        };
        let identity = is_identity(&kernel);
        let float = crate::adjust_float::prepare(&kernel, profile);
        let description = value.to_string();
        check_length(&description)?;
        Ok(Self {
            description,
            operation: op,
            kernel,
            identity,
            float,
            lab,
        })
    }
    pub fn description(&self) -> &str {
        &self.description
    }
    pub fn operation(&self) -> &str {
        &self.operation
    }
    pub fn is_identity(&self) -> bool {
        self.identity
    }
}

/// The op's kernel as the precision path prepares it against `profile`: the
/// one preparation of both the main kernel and a Lab Split form's channel
/// kernel (`lab_adjust::LabBinding`).
fn precision_kernel(
    op: &str,
    params: &Map<String, Value>,
    profile: &IccProfile,
) -> Result<Adjustment, String> {
    let parsed = Adjustment::from_op(op, params)
        .ok_or_else(|| format!("invalid high-depth {op} adjustment parameters"))?;
    Ok(match parsed {
        Adjustment::Curves(_) => Adjustment::Curves(Box::new(
            CurveLuts::parse_precision(params).ok_or("invalid continuous curves")?,
        )),
        Adjustment::Calibrated(op) => Adjustment::Calibrated(op.prepare_precision(profile)?),
        Adjustment::PhotoFilter(op) => Adjustment::PhotoFilter(op.prepare_precision(profile)?),
        Adjustment::Vibrance(op) => Adjustment::Vibrance(op.prepare_precision(profile)?),
        // Photoshop's 16-bit bins read the 15-bit grid (`adjust_math::posterize`).
        Adjustment::Posterize { levels, .. } => Adjustment::Posterize { levels, top: 65535 },
        // Photoshop's 16-bit Threshold reads its luma on the 15-bit grid
        // (`adjust_math::threshold`).
        Adjustment::Threshold { level, .. } => Adjustment::Threshold { level, top: 65535 },
        // Photoshop rounds Levels' stages to codes and runs Black & White's
        // Tint on integers only at 8 bits (#2 phase 4).
        Adjustment::Levels(mut op) => {
            op.use_precision();
            Adjustment::Levels(op)
        }
        Adjustment::BlackAndWhite(mut op) => {
            op.use_precision();
            Adjustment::BlackAndWhite(op)
        }
        Adjustment::GradientMap(mut op) => {
            op.use_precision();
            Adjustment::GradientMap(op)
        }
        Adjustment::ColorLookup(op) => {
            op.validate_precision()?;
            Adjustment::ColorLookup(op)
        }
        other => other,
    })
}

/// A Color form's two pipelines, [twin → `space`, `space` → twin]
/// (`lab_adjust::LabBinding`), each from `lut_pipeline` (the twin is a LUT
/// side, so both build).
pub(crate) fn color_pipelines(
    profile: &IccProfile,
    space: &IccProfile,
) -> Result<Box<[crate::icc_intent::ColorPipeline; 2]>, String> {
    let refused = || format!("the Lab twin's {} conversion did not build", space.name());
    let built = |from: &IccProfile, to: &IccProfile| match lut_pipeline(from, to) {
        Ok(Some(pipeline)) => Ok(pipeline),
        _ => Err(refused()),
    };
    Ok(Box::new([built(profile, space)?, built(space, profile)?]))
}

/// The Lab binding of `form` (`lab_adjust::LabBinding`) for every form but
/// Color (`prepare` binds it with [`color_pipelines`]): Split's channel
/// kernel from `params` without the composite keys, Chroma's saturation
/// scale.
fn bind(
    form: crate::lab_tone::LabForm,
    op: &str,
    params: &Map<String, Value>,
    kernel: &Adjustment,
    profile: &IccProfile,
) -> Result<crate::lab_adjust::LabBinding, String> {
    use crate::lab_tone::LabForm;
    let channels = if form == LabForm::Split {
        let mut stages = params.clone();
        for key in crate::lab_adjust::composite_keys(op) {
            stages.remove(*key);
        }
        Some(Box::new(precision_kernel(op, &stages, profile)?))
    } else {
        None
    };
    let chroma = match kernel {
        Adjustment::Bcs {
            saturation_scale, ..
        }
        | Adjustment::BcsModern {
            saturation_scale, ..
        } if form == LabForm::Chroma => *saturation_scale,
        _ => 1.0,
    };
    Ok(crate::lab_adjust::LabBinding {
        form,
        channels,
        chroma,
        color: None,
    })
}

impl PrecisionAdjustment {
    /// The byte kernel's own answer (`Adjustment::is_pointwise`): the one
    /// kernel both depths dispatch through decides it.
    pub(crate) fn is_pointwise(&self) -> bool {
        self.kernel.is_pointwise()
    }
    /// The byte kernel's own guide radius (`Adjustment::guide_radius`).
    pub(crate) fn guide_radius(&self) -> Option<f32> {
        self.kernel.guide_radius()
    }
    /// Whether preparation reads the captured profile and can REFUSE it: only
    /// `Calibrated` operators (`prepare_precision`), which is why Assign asks
    /// it (`precision_color`, which then asks `calibrated_refusal` for the
    /// layer's [`Self::operation`]: Exposure needs only its exponents and is
    /// admitted where White Balance is refused, #2). Photo Filter and Vibrance also bind the profile
    /// but never refuse one ([`Self::binds_profile`]); every other kernel parses to the
    /// same tables under any profile. `#[doc(hidden)] pub` as a black-box test
    /// seam (`adjust_photo_filter_tests`).
    #[doc(hidden)]
    pub fn reads_profile(&self) -> bool {
        matches!(self.kernel, Adjustment::Calibrated(_))
    }
    /// Whether the prepared kernel holds tables bound to the captured
    /// profile: `Calibrated`, and Photo Filter's PCS basis, which
    /// never refuses a profile and so is not `reads_profile`. The rebind
    /// retention bound asks this (`rebind_retained_bound`). `#[doc(hidden)]
    /// pub` as a black-box test seam (`adjust_photo_filter_tests`).
    #[doc(hidden)]
    pub fn binds_profile(&self) -> bool {
        matches!(
            self.kernel,
            Adjustment::Calibrated(_) | Adjustment::PhotoFilter(_) | Adjustment::Vibrance(_)
        )
    }
    pub fn apply_at(&self, rgb: [f32; 3], guide: f32, xy: (i64, i64)) -> [f32; 3] {
        use crate::lab_tone::{self, LabForm};
        if self.identity {
            return rgb;
        }
        let Some(lab) = &self.lab else {
            return self.kernel.apply_at(rgb, guide, xy);
        };
        let kernel = |v| self.kernel.apply_at(v, guide, xy);
        match (lab.form, &lab.channels, &lab.color) {
            (LabForm::Split, Some(channels), _) => {
                lab_tone::split(rgb, &kernel, &|v| channels.apply_at(v, guide, xy))
            }
            (LabForm::Chroma, _, _) => lab_tone::chroma(rgb, &kernel, lab.chroma),
            (LabForm::Desaturate, _, _) => lab_tone::chroma(rgb, &|v| v, 0.0),
            (LabForm::Color, _, Some(p)) => {
                lab_tone::color(rgb, &|v| p[0].color(v), &kernel, &|v| p[1].color(v))
            }
            (form, _, _) => lab_tone::apply(form, rgb, &kernel),
        }
    }
    /// The kernel evaluated in LINEAR LIGHT over extended-range components —
    /// the 32-bit float document's one adjustment path (Phase 11F), under
    /// the float form parse derived (`adjust_float`: each op's
    /// own closed form, fitted to Photoshop 26.10.0's 32-bit renders). A
    /// refused form answers its sentence FIRST, even at identity, which is
    /// how `float_document::float_refusal` learns what an entry carries;
    /// then an identity description returns `rgb` bit for bit.
    ///
    /// `xy` is the pixel's position on `apply_at`'s convention (the canvas
    /// on the layer path, the image on the destructive one), read only by a
    /// display form's Gradient Map dither.
    pub fn apply_linear(&self, rgb: [f32; 3], xy: (i64, i64)) -> Result<[f32; 3], String> {
        if let FloatForm::Refused(reason) = &self.float {
            return Err(reason.clone());
        }
        if self.identity {
            return Ok(rgb);
        }
        crate::adjust_float::apply(&self.float, &self.kernel, rgb, xy)
    }
    pub(crate) fn guide(
        &self,
        w: u32,
        h: u32,
        rgba_at: &dyn Fn(usize) -> [f32; 4],
        job: JobContext<'_>,
    ) -> Result<Option<PrecisionGuide>, String> {
        match &self.kernel {
            Adjustment::ShadowsHighlights(op) if op.reads_guide() => {
                // A Lab Color form's guide is the luma of the sample's
                // colour in the op's space (ProPhoto RGB for
                // Shadows/Highlights), as its kernel reads that colour
                // (`lab_tone::color`).
                if let Some(p) = self.lab.as_ref().and_then(|lab| lab.color.as_ref()) {
                    let in_space = |i: usize| {
                        let s = rgba_at(i);
                        let c = p[0].color([s[0], s[1], s[2]]).map(|v| v.clamp(0.0, 1.0));
                        [c[0], c[1], c[2], s[3]]
                    };
                    return crate::adjust_guide::precision_plane(w, h, op.radius(), &in_space, job)
                        .map(Some);
                }
                crate::adjust_guide::precision_plane(w, h, op.radius(), rgba_at, job).map(Some)
            }
            _ => Ok(None),
        }
    }
    pub fn guide_scratch_bytes(&self, w: u32, h: u32) -> Result<u64, String> {
        self.guide_plan(w, h).map(|p| p.0)
    }
    pub fn guide_work_units(&self, w: u32, h: u32) -> Result<u64, String> {
        self.guide_plan(w, h).map(|p| p.1)
    }
    fn guide_plan(&self, w: u32, h: u32) -> Result<(u64, u64), String> {
        match &self.kernel {
            Adjustment::ShadowsHighlights(op) if op.reads_guide() => plan::guide(w, h, op.radius()),
            _ => Ok((0, 0)),
        }
    }
    /// Selective serde inspection skips unrelated LUT/table payloads without
    /// constructing a Value tree or prepared tables on the host UI thread.
    pub fn guide_plan_for_description(meta: &str, w: u32, h: u32) -> Result<(u64, u64), String> {
        check_length(meta)?;
        plan::description(meta, w, h)
    }
    /// An ADMISSION input, not only a report: native save plans and native
    /// reopen charge `PrecisionAdjustmentState::record_retained_bytes`, which
    /// is measured here, so every heap field of the kernel must be recorded.
    pub(crate) fn record_resources(&self, inventory: &mut Inventory) {
        inventory.string(&self.description, "precision_adjustment", false);
        inventory.string(&self.operation, "precision_adjustment", false);
        match &self.float {
            FloatForm::Refused(reason) => inventory.string(reason, "precision_adjustment", false),
            FloatForm::Display { encoder, .. } => encoder.record_resources(inventory),
            _ => {}
        }
        record_kernel(&self.kernel, inventory);
        if let Some(lab) = &self.lab {
            if let Some(channels) = &lab.channels {
                inventory.object(&**channels, "precision_adjustment", false);
                record_kernel(channels, inventory);
            }
            if let Some(pipelines) = &lab.color {
                inventory.object(&**pipelines, "precision_adjustment", false);
                for pipeline in pipelines.iter() {
                    pipeline.record_resources(inventory);
                }
            }
        }
    }
    pub fn retained_bytes(&self) -> u64 {
        let mut inventory = Inventory::default();
        self.record_resources(&mut inventory);
        inventory.unique_bytes()
    }
}

/// The heap fields of one prepared kernel: the main one and a Lab Split
/// form's channel kernel.
fn record_kernel(kernel: &Adjustment, inventory: &mut Inventory) {
    macro_rules! boxed {
        ($v:expr) => {
            inventory.object(&**$v, "precision_adjustment", false)
        };
    }
    match kernel {
        Adjustment::Calibrated(v) => v.record_resources(inventory),
        Adjustment::PhotoFilter(v) => v.record_resources(inventory),
        Adjustment::Vibrance(v) => v.record_resources(inventory),
        Adjustment::Curves(v) => {
            boxed!(v);
            v.record_resources(inventory);
        }
        Adjustment::ColorLookup(v) => {
            boxed!(v);
            v.record_resources(inventory);
        }
        Adjustment::GradientMap(v) => {
            boxed!(v);
            v.record_resources(inventory);
        }
        Adjustment::Levels(v) => boxed!(v),
        Adjustment::HueSaturation(v) => boxed!(v),
        Adjustment::ColorBalance(v) => boxed!(v),
        Adjustment::BlackAndWhite(v) => boxed!(v),
        Adjustment::SelectiveColor(v) => boxed!(v),
        Adjustment::ShadowsHighlights(v) => boxed!(v),
        _ => {}
    }
}

fn check_length(meta: &str) -> Result<(), String> {
    if meta.len() > crate::drdc::MAX_DRDC_META_LEN as usize {
        Err("high-depth adjustment description exceeds 16 MiB".into())
    } else {
        Ok(())
    }
}
fn keys(values: &Map<String, Value>, allowed: &[&str]) -> Result<(), String> {
    if let Some(key) = values.keys().find(|key| !allowed.contains(&key.as_str())) {
        return Err(format!("unknown high-depth adjustment parameter {key}"));
    }
    Ok(())
}
fn nested_keys(
    values: &Map<String, Value>,
    names: &[&str],
    allowed: &[&str],
) -> Result<(), String> {
    for name in names {
        if let Some(v) = values.get(*name) {
            keys(
                v.as_object()
                    .ok_or("nested adjustment parameter must be an object")?,
                allowed,
            )?;
        }
    }
    Ok(())
}
fn parameter_keys(op: &str, p: &Map<String, Value>) -> Result<(), String> {
    let allowed: &[&str] = match op {
        "bcs" => &["brightness", "contrast", "saturation", "legacy"],
        "levels" => &[
            "black",
            "white",
            "gamma",
            "output_black",
            "output_white",
            "r",
            "g",
            "b",
        ],
        "hue_rotate" => &["degrees"],
        "threshold" => &["level"],
        "posterize" => &["levels"],
        "curves" => &["r", "g", "b", "rgb", "r_map", "g_map", "b_map", "rgb_map"],
        "invert" | "grayscale" | "sepia" => &[],
        "exposure" => &["exposure", "offset", "gamma", "evaluation"],
        "white_balance" => &["temperature", "tint", "evaluation"],
        "vibrance" => &["vibrance", "saturation"],
        "hue_saturation" => &[
            "hue",
            "saturation",
            "lightness",
            "bands",
            "colorize",
            "colorize_hue",
            "colorize_saturation",
            "colorize_lightness",
        ],
        "color_balance" => &["shadows", "midtones", "highlights", "preserve_luminosity"],
        "black_and_white" => &[
            "reds",
            "yellows",
            "greens",
            "cyans",
            "blues",
            "magentas",
            "tint",
            "tint_color",
        ],
        "photo_filter" => &["color", "density", "preserve_luminosity"],
        "channel_mixer" => &["red", "green", "blue", "gray", "monochrome"],
        "selective_color" => &[
            "reds", "yellows", "greens", "cyans", "blues", "magentas", "whites", "neutrals",
            "blacks", "method",
        ],
        "shadows_highlights" => &[
            "shadows",
            "highlights",
            "radius",
            "color",
            "midtone_contrast",
        ],
        "gradient_map" => &["gradient", "dither", "method"],
        "replace_color" => &["colors", "fuzziness", "hue", "saturation", "lightness"],
        "color_lookup" => &[
            "kind",
            "size",
            "source_size",
            "domain_min",
            "domain_max",
            "title",
            "table",
            "strength",
            "abstract_profile",
        ],
        _ => return Err(format!("unsupported high-depth adjustment {op}")),
    };
    keys(p, allowed)?;
    match op {
        "levels" => nested_keys(
            p,
            &["r", "g", "b"],
            &["black", "white", "gamma", "output_black", "output_white"],
        ),
        "color_balance" => nested_keys(
            p,
            &["shadows", "midtones", "highlights"],
            &["cyan_red", "magenta_green", "yellow_blue"],
        ),
        "channel_mixer" => nested_keys(
            p,
            &["red", "green", "blue", "gray"],
            &["r", "g", "b", "constant"],
        ),
        "selective_color" => nested_keys(p, &allowed[..9], &["c", "m", "y", "k"]),
        "shadows_highlights" => nested_keys(p, &["shadows", "highlights"], &["amount", "tone"]),
        "hue_saturation" => {
            if let Some(bands) = p.get("bands") {
                let bands = bands.as_object().ok_or("bands must be an object")?;
                let names = crate::adjust_math::HUE_WHEEL;
                keys(bands, &names)?;
                nested_keys(
                    bands,
                    &names,
                    &[
                        "hue",
                        "saturation",
                        "lightness",
                        "center",
                        "inner",
                        "falloff",
                    ],
                )?;
            }
            Ok(())
        }
        "gradient_map" => {
            if let Some(g) = p.get("gradient") {
                let g = g.as_object().ok_or("gradient must be an object")?;
                keys(
                    g,
                    &[
                        "stops",
                        "style",
                        "angle",
                        "scale",
                        "reverse",
                        "align_with_layer",
                        "smoothness",
                    ],
                )?;
                if let Some(stops) = g.get("stops").and_then(Value::as_array) {
                    for stop in stops {
                        keys(
                            stop.as_object().ok_or("gradient stop must be an object")?,
                            &[
                                "position",
                                "color",
                                "opacity",
                                "midpoint",
                                "opacity_midpoint",
                            ],
                        )?;
                    }
                }
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

fn is_identity(kernel: &Adjustment) -> bool {
    match kernel {
        Adjustment::Bcs {
            brightness,
            contrast_slope,
            saturation_scale,
        } => *brightness == 0.0 && *contrast_slope == 1.0 && *saturation_scale == 1.0,
        Adjustment::BcsModern {
            gamma,
            contrast_power,
            saturation_scale,
        } => *gamma == 1.0 && *contrast_power == 1.0 && *saturation_scale == 1.0,
        Adjustment::Levels(v) => v.is_identity(),
        Adjustment::HueRotate { matrix } => {
            *matrix == [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]
        }
        Adjustment::Curves(v) => v.is_identity(),
        Adjustment::Calibrated(v) => v.is_identity(),
        Adjustment::Exposure(v) => v.is_identity(),
        Adjustment::WhiteBalance(v) => v.is_identity(),
        Adjustment::Vibrance(v) => v.is_identity(),
        Adjustment::HueSaturation(v) => v.is_identity(),
        Adjustment::ColorBalance(v) => v.is_identity(),
        Adjustment::PhotoFilter(v) => v.is_identity(),
        Adjustment::ChannelMixer(v) => v.is_identity(),
        Adjustment::SelectiveColor(v) => v.is_identity(),
        Adjustment::ShadowsHighlights(v) => v.is_identity(),
        Adjustment::ColorLookup(v) => v.is_identity(),
        Adjustment::ReplaceColor(v) => v.is_identity(),
        _ => false,
    }
}
