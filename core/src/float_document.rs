//! The 32-bit floating-point document mode (Phase 11F): the policy, the tone
//! maps, the range rules and the bounded admitted-operator set.
//!
//! # What a float document IS
//!
//! Every raster source is [`PrecisionStorage::Rgba32Float`] with STRAIGHT
//! alpha. Its RGB components are LINEAR LIGHT in the document profile's own
//! primaries — the transfer curve is applied only when an SDR deliverable is
//! encoded — so a float document is `linear_light` BY CONSTRUCTION and can
//! never be `encoded` (`precision_evaluation`). Samples are finite, `|v| ≤`
//! [`FLOAT_RANGE_MAX`] and alpha is in [0, 1]. NEGATIVE components are stored
//! and are admitted by every admitted operator; only display and SDR output
//! map them to zero.
//!
//! # The three gates, each written once
//!
//! * **Storage** — `PrecisionDocument::build` is the ONE place a leaf's
//!   storage is checked against the document mode. `PrecisionDocumentLayer::raster`
//!   keeps its u16 refusal unchanged, so `from_image` on an F32 image still
//!   refuses; [`PrecisionDocumentLayer::raster_float`] is the only F32 leaf
//!   constructor and it is reachable only from `float_io` (whose
//!   `document_of` also builds the host-decoded HDR still,
//!   `float_host_intake`), the v40 reader,
//!   the typed clipboard paste, the float place of an image
//!   (`precision_retained_source`), the typed artwork
//!   publication (`artwork_document`) and Merge to HDR's result
//!   (`doc_merge_hdr`). A LAYERS paste into a float document
//!   (`layer_clipboard_float`) builds no leaf of its own: it
//!   swaps each copied record's image for its promoted f32 plane and goes
//!   through this same gate.
//! * **Finite values** — [`sanitize`] runs at IMPORT only, repairs by the
//!   rule stated on it and reports its counts as import warnings. Every
//!   render/edit output is checked and REFUSES rather than repairing, with
//!   ONE named exception (D3): FILTERED GEOMETRY (Image Size,
//!   straighten, arbitrary canvas rotation, affine and perspective
//!   transforms, warp, 32-bit export resampling, and the
//!   Free Transform of a layer's selected pixels) clamps
//!   each output RGB component to the source's own visible range
//!   (`precision_sampling::visible_range`; the layer's, for selected pixels
//!   the float's, or for an export the rendered composite's) so a negative lobe's ringing never leaves it, and drops an output alpha below
//!   2^-24 to transparent (`sampling::EXTENDED_ALPHA_FLOOR`). The
//!   kernel overshoot is a property of the filter, not of the document, so
//!   refusing it would refuse every Bicubic or Lanczos edge.
//!   The same clamp and floor, on the layer's own visible range, finish
//!   two filters whose resample has negative lobes
//!   (`float_filter_apply`):
//!   Lens Correction (Catmull-Rom),
//!   Liquify (Catmull-Rom).
//!   The Gaussian family (Gaussian Blur, Unsharp Mask's and High Pass's
//!   low-pass) shares that finish for its floor. Its kernel in a float layer
//!   is a non-negative FIR mean (`sampling_gaussian::runs_fast`), so there
//!   the clamp only absorbs rounding, as it does for the five means (Box,
//!   Motion, Radial, Lens Blur, Tilt-Shift: `SpatialKernel::combine_in`).
//! * **Operators** — [`PrecisionDocument::float_refusal`] walks the stack in
//!   the same postorder `precision_evaluation::walk` uses and names the first
//!   operator outside the admitted set. `linear_light_refusal` returns it
//!   first, so the two host guards 11E already installed refuse a float edit
//!   with no new guard anywhere.
//!
//! A float document's small edits each run on
//! the samples as stored: Apply Layer Mask (`precision_mask_apply`: alpha ·
//! coverage in f32, RGB bit for bit), Layer Via Copy / Cut
//! (`precision_layer_via`: the copy's alpha `a·c/65535`, the cut the float
//! Cut's own erase), and Assign Profile / Convert to Profile (`float_color`:
//! Assign relabels the linear samples, Convert applies the matrix between the
//! two profiles' primaries with no clamp). A profile with no matrix/TRC
//! model, a Convert with absolute intent, black-point compensation or
//! preserve-appearance, and Apply Image stay refused by name (the operator
//! table below).
//!
//! Twenty-four filters run on a raster layer's pixels, with
//! no colour clamp, through the kernels the other depths run
//! (`float_filter_apply` is the run, `float_filter_route` the dispatch; the
//! `<op>_filter` rows), plus Red Eye (`precision_redeye`). Thirteen run in
//! linear light on the stored samples: Gaussian Blur, Box Blur, Motion Blur,
//! Radial Blur, Lens Blur, Tilt-Shift, Offset, Lens Correction, Liquify,
//! Unsharp Mask, High Pass, Add Noise and Pixelate. The twelve whose
//! parameter is defined on display-encoded values run on the layer's
//! display-encoded copy (`display_encoding`): Surface Blur, Dust & Scratches,
//! Reduce Noise, Add Grain, Clouds, Difference Clouds, Emboss, Edge Detect,
//! legacy Sharpen, Presence, Smart Sharpen and the Develop Filter
//! (its Temperature, Tint and Exposure stay linear). Dehaze and the Develop Filter's Highlights,
//! Shadows, Vibrance and Saturation have no float rule and are refused by
//! name (rows of the table below).
//!
//! Those filters run on a layer mask, an alpha channel or one
//! Red/Green/Blue/Alpha plane too, through the same route on a one-channel
//! plane under `float_plane_filter`'s plane rule (coverage clamped to 0…1, a
//! colour plane unclamped); Liquify, Depth Blur and the Develop Filter
//! have no one-plane form and refuse by name there.
//!
//! The display-defined tools and the rest of the blend modes and
//! adjustments run, each under one stated linear-light rule
//! (`display_encoding`'s module doc holds R1–R5 and A verbatim): the Paint
//! Bucket, Dodge, Burn and Sponge on a layer (`float_paint_tone`,
//! `float_retouch_brush`), Dodge and Burn on a mask or alpha channel
//! (`precision_scalar_edit`), the Magic Wand, Color Range, Select Similar and
//! Quick Selection on an R1 display copy (`precision_selection`); Linear
//! Burn, Linear Light, Pin Light and Divide (`blend_float`); Hue Rotate,
//! Grayscale and Sepia in closed form, Threshold, Posterize and Gradient Map
//! display-mapped, Vibrance, Selective Color, Replace Color and the
//! Hue/Saturation settings with no closed form display-scaled, every Color
//! Lookup table, and the `document_v2` White Balance apply (`adjust_float`).
//! Refused by decision, each with its reason: the nine blend modes whose
//! formula inverts about white, Invert, Shadows/Highlights, the legacy White
//! Balance and Select and Mask's edge stage.
//!
//! the Phase 11F 32-BIT FLOAT DOCUMENT MODE: straight RGBA32 float rasters holding LINEAR LIGHT in
//! the document profile's primaries, finite with `|v| <= FLOAT_RANGE_MAX = 65504` and alpha in
//! [0,1], with NEGATIVE components stored and admitted by every admitted operator and mapped to 0
//! only at display and SDR output. Promotion into float is the clipboard's (the pixel paste and
//! the LAYERS paste promote into an existing float document) and the
//! conversion of a whole 8- or 16-bit document (`precision_conversion_float`: the LAYERS
//! paste's promotion under Preserve over the document's own records, each u16 code decoded
//! through the profile's TRC): `PrecisionDocumentLayer::raster` keeps its u16 refusal
//! (so `from_image` with an F32 image still refuses) and `raster_float` (`pub(crate)`) is the ONE
//! F32 leaf constructor (the float place of an image and the typed artwork
//! publication call it too).
//! **`PrecisionDocument::build(.., float)` is the ONE storage gate**: every
//! raster leaf's `storage()` must agree with `float.is_some()`. A float document is `linear_light`
//! BY CONSTRUCTION — `evaluation()` answers from `float` alone, `linear_light` is always `None`, no
//! `TypedTransfer` is ever allocated, `with_evaluation(LinearLight)` is `Ok(None)` and `Encoded` is
//! `Err` — and `linear_light_refusal` returns `float_refusal` first, so the two host guards 11E
//! installed cover float edits with no new guard. `sanitize` is the IMPORT-ONLY repair (NaN -> 0,
//! +/-inf -> +/-65504, out-of-range clamped, alpha clamped) and its counts become import warnings;
//! every render/edit output is checked and REFUSES instead, the one named exception being filtered
//! geometry's per-layer RGB range clamp (D3). The admitted set is
//! Normal/Multiply/Darken/Lighten/Difference (exactly the `blend.rs` kernels with no clamp of their
//! own — no kernel is copied, no clamp added or removed) plus thirteen modes whose formula a float
//! document can store (Dissolve, Darker/Lighter Color and Pin Light on their masters; Linear
//! Dodge, Subtract, Linear Burn, Linear Light, Divide, Hue, Saturation, Color, Luminosity on
//! `blend_float`'s variants), layer properties, masks/channels, isolated groups and Pass Through
//! groups (Pass Through is created, set and kept at 32 bits and composites as the
//! 8/16-bit master does — the children over a copy of the backdrop, gated back by group opacity ×
//! mask — measured against Photoshop 26.10.0; the float render runs `precision_render`'s own
//! `Kind::PassThrough` arm),
//! adjustment layers through `PrecisionAdjustment::apply_linear`, each op under its own closed form
//! fitted to Photoshop 26.10.0's 32-bit renders (`adjust_float`: Levels, `bcs`, Black
//! & White, its Tint as SetLum of the tint's numbers to the grey capped at 16, Channel Mixer,
//! Photo Filter; Exposure of either evaluation per stored
//! channel with no reference-basis matrices; Curves in point form as Photoshop's natural
//! spline continued past its ends, and in map (pencil) form as piecewise-linear maps
//! continued along their end segments; Color Lookup for a 3D table with a 0…1 domain and
//! entries in 0…1, tetrahedral with out-of-range inputs clamped, as measured against Photoshop's
//! device-link render, and every other table on its stored values; a `document_v2` White
//! Balance over `Calibrated::apply_linear`, the reference-basis matrices with NO TRC round trip;
//! the closed and display forms named above), destructive Image ▸ Adjustments for every admitted
//! layer form under the same forms (`float_adjust_apply`), layer styles
//! and Blend If (all nine effects, fill opacity, knockout, Blend Interior Effects as
//! Group and the global light, each measured in Photoshop 26.10.0 — `style_float`,
//! `style_blend_if::source_weight_float` — with an effect's blend mode admitted on the layer
//! set's terms, [`style_refusal`]), the readouts,
//! render/convert/export/native save, the tone map, Fill…/Stroke… on a layer, mask or alpha channel
//! (`fill_stroke`, `float_paint`), Move and Free Transform of a layer's selected pixels
//! (the one `selected_pixels_walk` at f32, the hole `a·(1−c)`, a filtered transform under D3) and Load Selection from a colour/luminosity plane or layer
//! transparency (`source_plane_selection`), placed images
//! (a RAW layer re-renders from its original at 32 bits (measured against Photoshop 26.10.0's placed
//! images); `retained_source_float` renders the 8/16-bit master into linear light, converting
//! first and resampling the f32 plane, as measured), text and shape layers (kept at 32
//! bits, their stored colour numbers rendered as linear light and mixed in linear light with
//! 16-bit antialias coverage, measured against Photoshop 26.10.0; `artwork_render::
//! render_float`, the description bytes carried verbatim and read by the host in the document's
//! ink); everything else refuses BY NAME through
//! `float_document::refused_sentence`. `ToneMap` (`clip` / extended `reinhard` with a white point)
//! is parsed BY HAND from a `serde_json::Map` — a `Deserialize` derive would accept `white` on the
//! unit variant — and encoded through the shared `icc_transform::PrecisionToneEncoder`, whose
//! interval search starts from a 4096-bucket index into its forward tables
//! (bit-identical to the whole-table search, `rgb_reference` the test oracle; ~4.5× fewer ns
//! per call); no curve inverter.
//! `PrecisionImage::to_rgba8` REFUSES F32 storage so a clamped preview can never be shown; display
//! is `PrecisionDocument::display_rgba8`. Decisions, shipped items and open items:
//! `designs/phase-11f-hdr-float-status.md`.

use std::sync::Arc;

use image::RgbaImage;
use serde::Serialize;
use serde_json::{json, Map, Value};

use crate::blend::BlendMode;
use crate::icc::IccProfile;
use crate::icc_transform::PrecisionToneEncoder;
use crate::job_control::{DrJobControl, JobContext};
use crate::precision_document::{Content, PrecisionDocument, PrecisionDocumentLayer};
use crate::precision_image::{
    admit, pixel_count, PrecisionImage, PrecisionStorage, BOOKKEEPING_BYTES,
};
use crate::precision_quantize::PrecisionDither;
use crate::resources::Inventory;

/// The largest magnitude a stored float sample may have: 65504, the largest
/// finite IEEE half. A float TIFF may be written at half width by another
/// tool, and a document whose samples all survive that round trip is one a
/// host can hand on; a larger bound would buy range no interchange format in
/// this build can carry.
pub const FLOAT_RANGE_MAX: f32 = 65504.0;

/// The storage name this mode reports across the FFI, in descriptors and in
/// the readout JSON.
pub const STORAGE_FLOAT: &str = "rgba32_float";

/// Reinhard's default white point, in linear multiples of diffuse white:
/// 2 stops of headroom, which is what a `clip` map would lose on a scene
/// with ordinary specular highlights.
const DEFAULT_WHITE: f32 = 4.0;

/// How a float document's linear samples become SDR numbers. Applied per RGB
/// channel; alpha is coverage and never passes through a map.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(tag = "operator", rename_all = "snake_case")]
pub enum ToneMap {
    /// `y = clamp(v, 0, 1)` — the honest default: it shows exactly the part
    /// of the scene an SDR file can hold and says nothing about the rest.
    Clip,
    /// `u = max(v, 0); y = min(u·(1 + u/W²)/(1 + u), 1)` — the extended
    /// Reinhard curve, which maps `W` exactly to 1.0 and compresses
    /// everything below it monotonically.
    Reinhard { white: f32 },
}

impl ToneMap {
    /// The wire form. Read BY HAND rather than through a `Deserialize`
    /// derive: an internally tagged enum accepts unknown keys on a unit
    /// variant even under `deny_unknown_fields`, so `{"operator":"clip",
    /// "white":4}` would be silently accepted as a clip — the caller would
    /// believe a white point was in force when none was.
    pub fn parse(json: &str) -> Result<Self, String> {
        let value: Value =
            serde_json::from_str(json).map_err(|e| format!("tone map is not valid JSON: {e}"))?;
        let object = value
            .as_object()
            .ok_or("a tone map must be a JSON object")?;
        Self::from_map(object)
    }

    /// [`Self::parse`] over an already decoded object — what the FFI and the
    /// native table read.
    pub(crate) fn from_map(object: &Map<String, Value>) -> Result<Self, String> {
        let operator = object
            .get("operator")
            .and_then(Value::as_str)
            .ok_or("a tone map requires an \"operator\" name")?;
        let map = match operator {
            "clip" => Self::Clip,
            "reinhard" => {
                let white = match object.get("white") {
                    None | Some(Value::Null) => DEFAULT_WHITE,
                    Some(value) => value
                        .as_f64()
                        .filter(|v| v.is_finite())
                        .ok_or("tone map \"white\" must be a finite number")?
                        as f32,
                };
                if !(1.0..=FLOAT_RANGE_MAX).contains(&white) {
                    return Err(format!(
                        "tone map \"white\" must be in [1, {FLOAT_RANGE_MAX}]"
                    ));
                }
                Self::Reinhard { white }
            }
            other => {
                return Err(format!(
                    "tone map operator \"{other}\" is not one of clip or reinhard"
                ))
            }
        };
        for key in object.keys() {
            let admitted =
                key == "operator" || (key == "white" && matches!(map, Self::Reinhard { .. }));
            if !admitted {
                return Err(format!("unknown tone map field \"{key}\""));
            }
        }
        Ok(map)
    }

    /// The canonical wire form; `white` appears only for `reinhard`.
    pub fn to_json(self) -> String {
        json!(self).to_string()
    }

    /// The operator's own name.
    pub fn name(self) -> &'static str {
        match self {
            Self::Clip => "clip",
            Self::Reinhard { .. } => "reinhard",
        }
    }

    /// The native table's operator code.
    pub(crate) fn code(self) -> u32 {
        match self {
            Self::Clip => 0,
            Self::Reinhard { .. } => 1,
        }
    }

    /// The white point as written to the native table; `clip` writes the
    /// default so the slot is never an uninitialized number.
    pub(crate) fn white(self) -> f32 {
        match self {
            Self::Clip => DEFAULT_WHITE,
            Self::Reinhard { white } => white,
        }
    }

    /// The table read back. Refuses an unknown operator and a white point
    /// outside the parser's own range, so a file cannot carry a map the
    /// editor would refuse to set.
    pub(crate) fn from_code(code: u32, white: f32) -> Result<Self, String> {
        match code {
            0 => Ok(Self::Clip),
            1 if white.is_finite() && (1.0..=FLOAT_RANGE_MAX).contains(&white) => {
                Ok(Self::Reinhard { white })
            }
            1 => Err("invalid native tone map white point".into()),
            _ => Err("unsupported native tone map operator".into()),
        }
    }

    /// One LINEAR RGB triple through this map. Alpha is never passed here.
    pub fn apply(self, rgb: [f32; 3]) -> [f32; 3] {
        rgb.map(|v| match self {
            Self::Clip => v.clamp(0.0, 1.0),
            Self::Reinhard { white } => {
                let u = v.max(0.0);
                let y = u * (1.0 + u / (white * white)) / (1.0 + u);
                y.min(1.0)
            }
        })
    }
}

/// A float document's policy: the chosen tone map plus the document
/// profile's forward encoder, built ONCE per document exactly as the 11E
/// transfer tables are.
#[derive(Clone)]
pub struct FloatPolicy {
    tone_map: ToneMap,
    encoder: PrecisionToneEncoder,
}

impl FloatPolicy {
    /// `Err` — a refusal — for a profile with no matrix/TRC model and for one
    /// `validate_precision` rejects: the SDR encode has to run the forward
    /// curve, so a profile this build cannot model cannot carry float.
    pub fn for_profile(profile: &Arc<IccProfile>, tone_map: ToneMap) -> Result<Self, String> {
        let model = profile.model().ok_or_else(|| {
            format!(
                "“{}” has no matrix/TRC model this build can decode, so it cannot carry a 32-bit float document",
                profile.name()
            )
        })?;
        model.validate_precision()?;
        Ok(Self {
            tone_map,
            encoder: PrecisionToneEncoder::new(model)?,
        })
    }

    pub fn tone_map(&self) -> ToneMap {
        self.tone_map
    }

    /// The document profile's forward encoder: E of the display encoding
    /// (`display_encoding`), which a display-referred filter and the
    /// display-defined rules (R1–R5, A) read; neither reads the tone map.
    pub(crate) fn encoder(&self) -> &PrecisionToneEncoder {
        &self.encoder
    }

    /// One LINEAR triple as a CONTINUOUS encoded triple in [0, 1]: the tone
    /// map, then the document profile's own forward curve through the shared
    /// [`PrecisionToneEncoder`]. There is no curve inverter here either.
    pub fn encode_sdr(&self, rgb: [f32; 3]) -> [f32; 3] {
        self.encoder.rgb(self.tone_map.apply(rgb).map(f64::from))
    }

    /// One straight float sample as its DISPLAY sample: RGB through
    /// [`Self::encode_sdr`], alpha kept. The ONE per-pixel display rule,
    /// read by `tone_mapped_u16` and the float layer thumbnail's box pass.
    pub(crate) fn display_sample(&self, pixel: [f32; 4]) -> [f32; 4] {
        let rgb = self.encode_sdr([pixel[0], pixel[1], pixel[2]]);
        [rgb[0], rgb[1], rgb[2], pixel[3]]
    }

    /// The policy's unique heap payload — the encoder's three tables. Counted
    /// by `PrecisionDocument::record_resources`, so admission sees them.
    pub(crate) fn record_resources(&self, inventory: &mut Inventory) {
        self.encoder.record_resources(inventory);
    }

    /// The same policy under a different map; the encoder's tables are
    /// Arc-shared, so this costs nothing.
    pub(crate) fn with_tone_map(&self, tone_map: ToneMap) -> Self {
        Self {
            tone_map,
            ..self.clone()
        }
    }
}

/// The IMPORT repair rule, and — beside filtered geometry's range clamp
/// (D3, module doc) — the only place a float sample is ever repaired rather
/// than refused: NaN becomes 0, ±infinity becomes
/// ±[`FLOAT_RANGE_MAX`], any other out-of-range magnitude is clamped to it,
/// and an alpha sample is clamped to [0, 1]. Returns
/// `(non_finite_replaced, clamped)` for the caller's disclosure.
///
/// `channels` is the file's samples per pixel; the alpha sample is the
/// second of two or the fourth of four, and 1/3 samples have none.
pub fn sanitize(samples: &mut [f32], channels: usize) -> (u64, u64) {
    let (mut non_finite, mut clamped) = (0_u64, 0_u64);
    let alpha_slot = match channels {
        2 => Some(1),
        4 => Some(3),
        _ => None,
    };
    for (index, value) in samples.iter_mut().enumerate() {
        if value.is_nan() {
            *value = 0.0;
            non_finite += 1;
        } else if value.is_infinite() {
            *value = if *value > 0.0 {
                FLOAT_RANGE_MAX
            } else {
                -FLOAT_RANGE_MAX
            };
            non_finite += 1;
        }
        let is_alpha = alpha_slot.is_some_and(|slot| index % channels == slot);
        let bounded = if is_alpha {
            (*value).clamp(0.0, 1.0)
        } else {
            (*value).clamp(-FLOAT_RANGE_MAX, FLOAT_RANGE_MAX)
        };
        if bounded != *value {
            *value = bounded;
            clamped += 1;
        }
    }
    (non_finite, clamped)
}

/// The blend modes a float document admits, on a layer, as Fill…/Stroke…
/// and paint ink and on a layer effect alike (decision B: one rule for
/// all three). Normal, Multiply, Darken, Lighten and Difference are the
/// `blend.rs` kernels whose W3C formulas are defined on the whole real line
/// and carry no clamp of their own. Dissolve, Darker Color
/// and Lighter Color run on their unchanged masters, and Linear Dodge (Add),
/// Subtract, Hue, Saturation, Color and Luminosity on the float variants
/// (`blend_float::blend_kind_at`). Linear Burn `cb+cs−1` and Linear
/// Light `cb+2cs−1` run unclamped, Pin Light on its master, and Divide as
/// `clamp(cb / max(cs, 1/65504), −65504, 65504)`. A sum outside ±65504
/// refuses downstream, as Linear Dodge's does. The nine modes whose formula
/// inverts about white or divides by `1 − v` stay refused
/// ([`BLEND_NO_MEANING`]).
pub fn admitted_blend(mode: BlendMode) -> bool {
    matches!(
        mode,
        BlendMode::Normal
            | BlendMode::Multiply
            | BlendMode::Darken
            | BlendMode::Lighten
            | BlendMode::Difference
            | BlendMode::Dissolve
            | BlendMode::DarkerColor
            | BlendMode::Addition
            | BlendMode::LighterColor
            | BlendMode::Subtract
            | BlendMode::Hue
            | BlendMode::Saturation
            | BlendMode::Color
            | BlendMode::Luminosity
            | BlendMode::LinearBurn
            | BlendMode::LinearLight
            | BlendMode::PinLight
            | BlendMode::Divide
    )
}

/// Why the nine refused blend modes refuse (decision B) — on a layer,
/// as ink and on a layer effect.
const BLEND_NO_MEANING: &str = "each formula inverts about white (1 − v) or divides by 1 − v, so above 1.0 it has no meaning: Screen of 2.0 over 2.0 is 0, Color Dodge divides by zero or less, Hard Mix writes only 0 or 1";

/// The ONE refusal wording for everything outside the admitted set.
pub(crate) fn refused_sentence(operator: &str) -> String {
    format!("{operator} is unavailable in a 32-bit float document")
}

/// What a float document evaluates today. These are the host-facing operator
/// names, so an MCP caller can match them.
const ADMITTED: &[&str] = &[
    "normal_composite",
    "multiply_blend",
    "darken_blend",
    "lighten_blend",
    "difference_blend",
    "dissolve_blend",
    "darker_color_blend",
    "linear_dodge_blend",
    "lighter_color_blend",
    "subtract_blend",
    "hue_blend",
    "saturation_blend",
    "color_blend",
    "luminosity_blend",
    // Decision B: unclamped, Pin Light on its master, Divide floored.
    "linear_burn_blend",
    "linear_light_blend",
    "pin_light_blend",
    "divide_blend",
    "isolated_normal_group",
    "pass_through_group",
    "layer_style",
    "fill_opacity",
    "global_light",
    "drop_shadow_effect",
    "inner_shadow_effect",
    "outer_glow_effect",
    "inner_glow_effect",
    "stroke_effect",
    "color_overlay_effect",
    "gradient_overlay_effect",
    // The tile's codes / 255 as linear values (VERBATIM).
    "pattern_overlay_effect",
    "bevel_emboss_effect",
    "satin_effect",
    "blend_if",
    "knockout",
    "blend_interior_as_group",
    "layer_opacity",
    "layer_name",
    "layer_visible",
    "layer_clipped",
    "layer_label",
    "layer_merge",
    "layer_merge_visible",
    "layer_lock",
    "layer_link",
    "layer_duplicate",
    "layer_move",
    "layer_group",
    "layer_ungroup",
    "layer_remove",
    "layer_add",
    "layer_stamp",
    // #1 phase 4c: Stamp Selected (`precision_merge_selected`).
    "layer_stamp_selected",
    "layer_flatten",
    // Apply Layer Mask, alpha · coverage in f32 (`precision_mask_apply`).
    "apply_layer_mask",
    // Layer Via Copy / Cut (`precision_layer_via`'s f32 arm).
    "layer_via",
    // Stack Modes in linear f64, stored unclamped (`precision_stack_mode`).
    "layer_stack_mode",
    // Assign Profile, the linear samples relabelled (`float_color`).
    "color_assign",
    // Convert to Profile in linear light with no clamp (`float_color`).
    "color_convert",
    "geometry_canvas_size",
    "geometry_crop",
    "geometry_rotate",
    "geometry_flip",
    "geometry_resize",
    "geometry_straighten",
    "geometry_rotate_canvas",
    "geometry_affine",
    "geometry_perspective",
    "geometry_warp",
    // Puppet Warp, the float warp's D3 finish (`precision_puppet`).
    "geometry_puppet",
    // Perspective Warp, the same finish (`precision_perspective_warp`).
    "geometry_perspective_warp",
    "geometry_affine_set",
    "geometry_perspective_set",
    "mask_set",
    "mask_remove",
    "mask_properties",
    "mask_transform",
    "channel_set",
    "channel_remove",
    "channel_read",
    "read_plane",
    "exposure_adjustment",
    "white_balance_adjustment",
    "levels_adjustment",
    "black_and_white_adjustment",
    "channel_mixer_adjustment",
    "photo_filter_adjustment",
    "bcs_adjustment",
    "curves_adjustment",
    "hue_saturation_adjustment",
    "color_balance_adjustment",
    "color_lookup_adjustment",
    // Decision A: closed forms on linear values, or a display form.
    "hue_rotate_adjustment",
    "grayscale_adjustment",
    "sepia_adjustment",
    "threshold_adjustment",
    "posterize_adjustment",
    "vibrance_adjustment",
    "selective_color_adjustment",
    "gradient_map_adjustment",
    "replace_color_adjustment",
    "levels_apply",
    "black_and_white_apply",
    "channel_mixer_apply",
    "photo_filter_apply",
    "exposure_apply",
    "curves_apply",
    "color_balance_apply",
    "color_lookup_apply",
    "bcs_apply",
    "hue_saturation_apply",
    "white_balance_apply",
    "hue_rotate_apply",
    "grayscale_apply",
    "sepia_apply",
    "threshold_apply",
    "posterize_apply",
    "vibrance_apply",
    "selective_color_apply",
    "gradient_map_apply",
    "replace_color_apply",
    // Filters on a raster layer (`float_filter_apply`).
    "blur_filter",
    "box_blur_filter",
    "motion_blur_filter",
    "radial_blur_filter",
    "lens_blur_filter",
    "tilt_shift_filter",
    "offset_filter",
    "lens_correction_filter",
    "liquify_filter",
    "unsharp_mask_filter",
    "high_pass_filter",
    // Display-referred parameters through the
    // display encoding (`display_encoding`, `float_filter_route`).
    "surface_blur_filter",
    "dust_and_scratches_filter",
    "reduce_noise_filter",
    "add_noise_filter",
    "add_grain_filter",
    "clouds_filter",
    "difference_clouds_filter",
    "emboss_filter",
    "edge_detect_filter",
    "pixelate_filter",
    "sharpen_filter",
    "presence_filter",
    "develop_filter_filter",
    "smart_sharpen_filter",
    // Filter ▸ Neural Filters ▸ Depth Blur (`depth_blur`).
    "depth_blur_filter",
    "sample",
    "histogram",
    "render",
    "convert",
    "export",
    "native_save",
    "set_tone_map",
    "set_evaluation",
    "clipboard_copy",
    "clipboard_copy_merged",
    "clipboard_cut",
    "clipboard_paste",
    "fill_stroke",
    "paint",
    "erase",
    "gradient",
    "clone",
    "heal",
    "spot_heal",
    "content_aware_fill",
    // Red Eye on the display-encoded rect (`float_retouch`).
    "red_eye",
    "blur_brush",
    "sharpen_brush",
    "smudge",
    "history_brush",
    // Display-defined tools under `display_encoding`'s R1–R5.
    "paint_bucket",
    "dodge_burn",
    "sponge",
    "magic_wand",
    "color_range",
    "select_similar",
    "quick_selection",
    "selected_pixels_transform",
    "selected_pixels_move",
    "source_plane_selection",
    "placed_image",
    "text_layer",
    "shape_layer",
];

/// The first part of `style` a 32-bit float document cannot hold, as its
/// complete sentence, or `None` — the same answer [`refused_operator`] gives
/// for one already attached and the layers paste gives for one arriving, so
/// the edit, the render and the paste cannot word it two ways (decision viii
/// of 12E-3b). Measured against Photoshop 26.10.0
/// (`scripts/photoshop-probe/float_styles.py`): a 32-bit Layer Style holds
/// all nine effects, fill opacity, knockout, Blend Interior Effects as Group
/// and Blend If, and its effect menu exactly the layer menu's modes; the
/// one part refused is an effect blend mode outside [`admitted_blend`] —
/// every effect's `blend`, both lit sides of a bevel, enabled or not:
/// ``the {mode} blend mode on the {type} layer effect`` (decision B:
/// the effect set is the layer set, and the table's row carries the reason).
pub(crate) fn style_refusal(style: &crate::style::LayerStyle) -> Option<String> {
    style.effects.iter().find_map(|effect| {
        effect_blend_modes(effect)
            .into_iter()
            .flatten()
            .find(|mode| !admitted_blend(*mode))
            .map(|mode| {
                refused_sentence(&format!(
                    "the {} blend mode on the {} layer effect",
                    crate::style::blend_mode_name(mode),
                    effect.kind().type_name()
                ))
            })
    })
}

/// Every blend mode `effect` carries: its `blend`, or a bevel's two lit
/// sides.
pub(crate) fn effect_blend_modes(effect: &crate::style::Effect) -> [Option<BlendMode>; 2] {
    use crate::style::Effect;
    match effect {
        Effect::DropShadow(e) => [Some(e.blend), None],
        Effect::InnerShadow(e) => [Some(e.blend), None],
        Effect::OuterGlow(e) => [Some(e.blend), None],
        Effect::InnerGlow(e) => [Some(e.blend), None],
        Effect::Stroke(e) => [Some(e.blend), None],
        Effect::ColorOverlay(e) => [Some(e.blend), None],
        Effect::GradientOverlay(e) => [Some(e.blend), None],
        Effect::PatternOverlay(e) => [Some(e.blend), None],
        Effect::BevelEmboss(e) => [Some(e.highlight_blend), Some(e.shadow_blend)],
        Effect::Satin(e) => [Some(e.blend), None],
    }
}

/// The complete sentence Select and Mask's Decontaminate Colors is refused
/// with in a 32-bit float document — the operator table's own row
/// (`precision_foreground`). The edge stage itself runs on the R1 display
/// copy (`display_encoding`'s rule E).
pub(crate) fn decontaminate_refusal() -> String {
    refused_sentence(REFUSED_DECONTAMINATE)
}

/// The complete sentence a blend mode outside [`admitted_blend`] is refused
/// with in a 32-bit float document — a layer's mode ([`refused_operator`])
/// and an ink's mode (`float_paint`) word it the same way; the operator
/// table's row carries the reason ([`BLEND_NO_MEANING`]).
pub(crate) fn blend_refusal(mode: BlendMode) -> String {
    refused_sentence(&format!(
        "the {} blend mode",
        crate::style::blend_mode_name(mode)
    ))
}

/// The refused rows the per-layer walk names directly. They are
/// constants so the sentence a document carries and the row a host reads are
/// literally the same string, whatever order the table is written in.
const REFUSED_DECONTAMINATE: &str = "Select and Mask's Decontaminate Colors";
/// The three adjustments refused by decision (decision A): the
/// operator each refusal sentence names (`adjust_float`) and its table row.
pub(crate) const REFUSED_INVERT: &str = "the invert adjustment";
pub(crate) const REFUSED_SHADOWS_HIGHLIGHTS: &str = "the shadows_highlights adjustment";
pub(crate) const REFUSED_LEGACY_WHITE_BALANCE: &str =
    "the white_balance adjustment with legacy evaluation";
/// Auto Tone, Auto Contrast, Auto Color and the Levels Auto buttons, refused
/// at 32 bits: the row [`auto_refusal`] names.
pub(crate) const REFUSED_AUTO: &str = "automatic tone correction (Auto Tone, Auto Contrast, Auto Color and the Levels dialog's Auto buttons)";

/// The complete sentence every automatic tone correction is refused with in
/// a 32-bit float document — the backdrop `auto` (`precision_scoped_source`)
/// and the selected `auto_tone` / `auto_contrast` / `auto_color` edits
/// (`float_adjust_apply`); the operator table's row carries the reason.
pub(crate) fn auto_refusal() -> String {
    refused_sentence(REFUSED_AUTO)
}

/// Whether a float document applies `op` destructively: its `<op>_apply`
/// row is admitted (`float_adjust_apply`).
pub(crate) fn admits_apply(op: &str) -> bool {
    ADMITTED
        .iter()
        .any(|row| row.strip_suffix("_apply") == Some(op))
}

/// Whether a float document runs the selected filter `op` on a raster
/// layer: its `<op>_filter` row is admitted (`float_filter_apply`). `op`
/// is the selected edit's own name (`blur`, `box_blur`, …).
pub(crate) fn admits_filter(op: &str) -> bool {
    ADMITTED
        .iter()
        .any(|row| row.strip_suffix("_filter") == Some(op))
}

/// The complete sentence a destructive apply of `op` is refused with when
/// its LAYER form is admitted but [`admits_apply`] is not, by name.
pub(crate) fn apply_refusal(op: &str) -> String {
    refused_sentence(&format!("applying the {op} adjustment"))
}

/// What it refuses, each with the acceptance criterion that would admit it.
/// The status doc restates this table; it is written here so the operator
/// table a host shows and the refusal a document carries cannot drift apart.
const REFUSED: &[(&str, &str)] = &[
    // Decision B: on a layer, as ink and on a layer effect.
    (
        "a blend mode whose formula inverts about white (Screen, Overlay, Soft Light, Hard Light, Color Dodge, Color Burn, Vivid Light, Hard Mix, Exclusion)",
        BLEND_NO_MEANING,
    ),
    // Decision A: refused by decision, as a layer and as an apply.
    (
        REFUSED_INVERT,
        "1 − v inverts about white, so above 1.0 it has no meaning: 8.0 would become −7.0",
    ),
    (
        REFUSED_SHADOWS_HIGHLIGHTS,
        "its luma curve L^e is defined on 0 to 1 and reverses above white (a lift darkens 8.0), and its Color and Midtone Contrast pivot about 0 to 1 midtones, so above 1.0 it has no meaning; the Develop Filter's Highlights and Shadows run at 32 bits, held at white",
    ),
    (
        REFUSED_LEGACY_WHITE_BALANCE,
        "its matrix acts through the sRGB curve on sRGB-encoded 0 to 1 numbers whatever the document profile, so a linear value above 1.0 has no encoded form for it to read; the document_v2 White Balance applies the same Bradford adaptation to linear values and is admitted",
    ),
    // Assign and Convert run for a matrix/TRC profile (`float_color`).
    (
        "Assign Profile or Convert to Profile with a profile that has no matrix/TRC model, and Assign Profile with one whose tone curves fail the precision check",
        "a float document's linear values are defined by its profile's primaries and its display and SDR export encode through its tone curves, so the profile needs a matrix/TRC model whose curves are finite and monotone",
    ),
    // The one float conversion is the matrix in linear light.
    (
        "Convert to Profile with absolute colorimetric intent, black-point compensation or preserve-appearance adjustments",
        "each needs a rule for linear extended-range values (a white-point shift above white, a black-point scale below zero, a bake of values outside 0 to 1) that is not decided; relative colorimetric with keep-parameters converts with no clamp",
    ),
    // Calculations, Luminosity Masks and Contact Sheet read planes or the display; Apply Image writes pixels.
    (
        "Apply Image",
        "its blend runs on u16 planes clamped to 0 to 1, so writing the result into float pixels needs a rule for values above white and below zero",
    ),
    (
        REFUSED_DECONTAMINATE,
        "its foreground solve has no float arm: it reads and writes RGBA16 codes clamped to 0 to 1, so a fringe above white would be clipped",
    ),
    // The three are disabled in 32-bit mode.
    (
        REFUSED_AUTO,
        "Auto Tone, Auto Contrast and Auto Color are unavailable on a 32-bit document: each reads its black and white points off the clipped ends of a 0 to 1 histogram, and a value above 1.0 lies past every end",
    ),
];

/// The operator table as JSON, for the host's Tone Map sheet and the MCP
/// reply: `{"admitted":[..],"refused":[{"operator":..,"reason":..}],"tone_maps":[..]}`.
pub fn float_operators_json() -> String {
    json!({
        "admitted": ADMITTED,
        "refused": REFUSED
            .iter()
            .map(|(operator, reason)| json!({"operator": operator, "reason": reason}))
            .collect::<Vec<_>>(),
        "tone_maps": ["clip", "reinhard"],
        "range": {"max_abs": FLOAT_RANGE_MAX, "negatives": "stored and admitted; display and SDR output map them to 0"},
        "storage": STORAGE_FLOAT,
    })
    .to_string()
}

/// The refusal this float ENTRY carries, as its complete sentence, or `None`
/// — the `precision_evaluation::refused_operator` shape over the float
/// admitted set. `pub(crate)` for the layers paste into a float document
/// (`layer_clipboard_float`), which asks it of each copied record as it would
/// land.
pub(crate) fn refused_operator(layer: &PrecisionDocumentLayer) -> Option<String> {
    if let Some(reason) = layer.style.as_deref().and_then(style_refusal) {
        return Some(reason);
    }
    if !admitted_blend(layer.blend_mode) {
        return Some(blend_refusal(layer.blend_mode));
    }
    if let Content::Adjustment(state) = &layer.content {
        // The kernel itself answers: `apply_linear` refuses an op with no
        // float form (`adjust_float`) and its `Err` already names the op.
        if let Err(reason) = state.kernel.apply_linear([0.0; 3], (0, 0)) {
            return Some(reason);
        }
    }
    None
}

/// The first refused entry in postorder, bottom-first — the numbering
/// `PrecisionDocument::layer` uses.
fn walk(layers: &[PrecisionDocumentLayer]) -> Option<String> {
    for layer in layers {
        if let Some(children) = layer.children() {
            if let Some(reason) = walk(children) {
                return Some(reason);
            }
        }
        if let Some(reason) = refused_operator(layer) {
            return Some(reason);
        }
    }
    None
}

impl PrecisionDocumentLayer {
    /// The ONE F32 leaf constructor, reachable only from `float_io`'s
    /// `document_of` (float TIFF, OpenEXR, Radiance and the
    /// host-decoded HDR still through `float_host_intake`), the v40
    /// native reader, the typed clipboard paste and New from Clipboard
    /// (`precision_clipboard`), the float place of an image
    /// (`precision_retained_source`), a retouch onto a new layer
    /// (`precision_retouch_output`'s `retouch_insert`), the typed artwork
    /// publication (`artwork_document`), Merge to HDR's result
    /// (`doc_merge_hdr`) and Select and Mask's New Layer outputs
    /// (`precision_cutout`) and the 32-bit PSD open's rasters and
    /// stored composite (`psd_import_float`). The layers paste into a float document and
    /// the conversion of a document into float (`precision_conversion_float`)
    /// do not call it: a record keeps its own struct and takes a promoted
    /// f32 image (`layer_clipboard_float`). `raster` is the master copy of this struct
    /// literal and keeps its u16 refusal, so every existing caller — and
    /// `from_image` with an F32 image — is unchanged; only the storage gate
    /// differs.
    pub(crate) fn raster_float(
        name: &str,
        image: PrecisionImage,
        offset: (i32, i32),
        opacity: f32,
        visible: bool,
    ) -> Result<Self, String> {
        crate::precision_document::scalar_and_name(name, opacity)?;
        if image.storage() != PrecisionStorage::Rgba32Float {
            return Err("a 32-bit float document requires explicit RGBA32 float sources".into());
        }
        Ok(Self {
            id: crate::doc_identity::mint_layer_id(),
            retained_source: None,
            warp: None,
            generated: None,
            description: None,
            name: Arc::new(name.into()),
            opacity,
            blend_mode: BlendMode::Normal,
            visible,
            content: Content::Raster { image, offset },
            records: 1,
            depth: 0,
            mask: None,
            clipped: false,
            blend_if: None,
            style: None,
            label: 0,
            locks: 0,
            link: 0,
            open: true,
            reference: (0, 0),
        })
    }
}

impl PrecisionDocument {
    /// The float document constructor: the policy is built once from the
    /// profile, then `build` gates every leaf's storage against it. The only
    /// callers are `float_io::document_of` (the float TIFF, OpenEXR and
    /// Radiance opens and the host HDR still, `float_host_intake`), the v40
    /// native reader,
    /// `precision_clipboard`'s `from_clipboard`, Merge to HDR's result
    /// (`doc_merge_hdr`), the conversion of a document into float
    /// (`precision_conversion_float`) and the 32-bit PSD open
    /// (`psd_import_float`).
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn from_parts_float(
        width: u32,
        height: u32,
        profile: Arc<IccProfile>,
        metadata: crate::metadata::Metadata,
        resolution: crate::metadata::Resolution,
        layers: Vec<PrecisionDocumentLayer>,
        channels: Vec<crate::precision_mask::PrecisionChannel>,
        tone_map: ToneMap,
    ) -> Result<Self, String> {
        let policy = FloatPolicy::for_profile(&profile, tone_map)?;
        Self::build(
            width,
            height,
            profile,
            metadata,
            resolution,
            layers,
            channels,
            Some(Arc::new(policy)),
        )
    }

    /// True when this is a 32-bit float document.
    pub fn is_float(&self) -> bool {
        self.float.is_some()
    }

    /// The document's tone map; `None` on a u16 document.
    pub fn tone_map(&self) -> Option<ToneMap> {
        self.float.as_ref().map(|policy| policy.tone_map())
    }

    /// The document under `map`. `Ok(None)` when it is already there — the
    /// core's no-op rule — and `Err` on a u16 document, which has no map.
    pub fn with_tone_map(&self, map: ToneMap) -> Result<Option<Self>, String> {
        let policy = self
            .float
            .as_ref()
            .ok_or("a tone map applies only to a 32-bit float document")?;
        if policy.tone_map() == map {
            return Ok(None);
        }
        let mut out = self.clone();
        out.float = Some(Arc::new(policy.with_tone_map(map)));
        Ok(Some(out))
    }

    /// The first operator this stack carries that the float mode does not
    /// evaluate; `None`, before any walk, on a u16 document.
    pub fn float_refusal(&self) -> Option<String> {
        self.float.as_ref()?;
        walk(self.root_layers())
    }

    /// F32 linear samples as TRC-encoded RGBA16: the map, then the document
    /// profile's forward curve, then the one `round(v · 65535)`
    /// `from_float_samples_unseeded` already owns. Alpha passes through
    /// untouched.
    pub(crate) fn tone_mapped_u16(
        &self,
        image: &PrecisionImage,
        job: JobContext<'_>,
    ) -> Result<PrecisionImage, String> {
        let policy = self
            .float
            .as_ref()
            .ok_or("tone mapping requires a 32-bit float document")?;
        encoded_u16(policy, image, job)
    }

    /// The DISPLAY path for a float projection: the document's map, then the
    /// existing undithered quantizer. This is the only way a float sample
    /// reaches an RGBA8 buffer — `PrecisionImage::to_rgba8` refuses F32
    /// storage precisely so a clamped preview can never be shown instead.
    pub fn display_rgba8(
        &self,
        image: &PrecisionImage,
        limit: u64,
        other: u64,
        control: Option<&DrJobControl>,
    ) -> Result<RgbaImage, String> {
        let _run = control.map(DrJobControl::begin).transpose()?;
        let job = control.map_or(JobContext::NONE, JobContext::new);
        job.check()?;
        let (width, height) = image.dimensions();
        let scratch = display_scratch(width, height)?;
        admit(image.retained_bytes(), scratch, other, limit)?;
        job.reserve(scratch)?;
        let out = self.display_rgba8_with_context(image, job)?;
        job.report(1.0, 5)?;
        Ok(out)
    }

    /// [`Self::display_rgba8`]'s work under a caller's job, its scratch
    /// ([`display_scratch`]) already admitted and reserved: the tone-mapped
    /// u16 copy, then the undithered quantizer.
    pub(crate) fn display_rgba8_with_context(
        &self,
        image: &PrecisionImage,
        job: JobContext<'_>,
    ) -> Result<RgbaImage, String> {
        let mapped = self.tone_mapped_u16(image, job)?;
        mapped.quantize_with_context(PrecisionDither::None, job)
    }

    /// The deliberate conversion to the u16 mode: render, apply the
    /// document's own map, and keep the result as one raster. The result is
    /// `linear_light` — it carries the policy exactly as
    /// `precision_conversion` carries it across a depth change, and a
    /// one-raster stack has nothing for that policy's walk to refuse.
    pub fn float_to_u16(
        &self,
        limit: u64,
        other: u64,
        control: Option<&DrJobControl>,
    ) -> Result<Self, String> {
        let _run = control.map(DrJobControl::begin).transpose()?;
        let job = control.map_or(JobContext::NONE, JobContext::new);
        job.check()?;
        if !self.is_float() {
            return Err("converting to rgba16_unorm requires a 32-bit float document".into());
        }
        let rendered = self.render_with_context(limit, other, job.range(0.0, 0.8))?;
        let mapped = self.tone_mapped_u16(&rendered, job.range(0.8, 0.95))?;
        drop(rendered);
        let out =
            self.flattened_u16_document(mapped, crate::doc_evaluation::Evaluation::LinearLight)?;
        job.report(1.0, 5)?;
        Ok(out)
    }

    /// `image` — this document's flattened appearance as RGBA16 codes on the
    /// same canvas — as a one-raster u16 document "Background" evaluating
    /// under `evaluation`: the same profile, metadata, resolution, channels,
    /// guides, ruler origin, colour samplers, paths and global light. The
    /// ONE statement of that carry, read by `float_to_u16` (linear light)
    /// and HDR Toning (`doc_hdr_toning`).
    pub(crate) fn flattened_u16_document(
        &self,
        image: PrecisionImage,
        evaluation: crate::doc_evaluation::Evaluation,
    ) -> Result<Self, String> {
        let (width, height) = self.dimensions();
        let layer = PrecisionDocumentLayer::raster("Background", image, (0, 0), 1.0, true)?;
        let mut out = Self::build(
            width,
            height,
            self.profile().clone(),
            self.metadata().clone(),
            self.resolution(),
            vec![layer],
            self.channels().to_vec(),
            None,
        )?;
        out.import_warnings = self.import_warnings.clone();
        // One raster on the SAME canvas, so the guides and the ruler origin
        // cross verbatim (`precision_guide`, `precision_conversion`'s rule).
        out.guides = self.guides().to_vec();
        out.ruler_origin = self.ruler_origin();
        out.color_samplers = self.color_samplers.clone();
        out.paths = self.paths.clone();
        out.global_light = self.global_light;
        if let Some(switched) = out.with_evaluation(evaluation)? {
            out = switched;
        }
        Ok(out)
    }
}

/// F32 linear samples as encoded RGBA16 under `policy`: its map, then its
/// profile's forward curve, then the one `round(v · 65535)`
/// `from_float_samples_unseeded` already owns; alpha passes through. The ONE
/// encode loop: a float document's display (`tone_mapped_u16`) and an f32
/// master's render in an 8- or 16-bit document (`retained_source_render`'s
/// `float_master_codes`, under a Clip policy) both run it.
pub(crate) fn encoded_u16(
    policy: &FloatPolicy,
    image: &PrecisionImage,
    job: JobContext<'_>,
) -> Result<PrecisionImage, String> {
    encoded_u16_gained(policy, image, 1.0, job)
}

/// [`encoded_u16`] with each RGB sample multiplied by `gain` BEFORE the map
/// — a view exposure (`float_view`) — and alpha untouched. The product is
/// held to the finite f32 range so no infinity reaches the map. A gain of 1.0
/// multiplies every finite sample by exactly one, so it is `encoded_u16` bit
/// for bit; this is the loop both run.
pub(crate) fn encoded_u16_gained(
    policy: &FloatPolicy,
    image: &PrecisionImage,
    gain: f32,
    job: JobContext<'_>,
) -> Result<PrecisionImage, String> {
    let runs = image
        .runs_rgba32()
        .ok_or("tone mapping requires rgba32_float storage")?;
    let (width, height) = image.dimensions();
    let gained = move |v: f32| (v * gain).clamp(f32::MIN, f32::MAX);
    let samples = runs.flatten().map(|pixel| {
        policy.display_sample([
            gained(pixel[0]),
            gained(pixel[1]),
            gained(pixel[2]),
            pixel[3],
        ])
    });
    PrecisionImage::from_float_samples_unseeded(
        width,
        height,
        image.profile().clone(),
        PrecisionStorage::Rgba16Unorm,
        samples,
        job,
    )
}

/// The working memory of [`PrecisionDocument::display_rgba8_with_context`]
/// over a `width` x `height` image: bookkeeping plus 12 bytes a pixel (the
/// tone-mapped u16 copy and the quantized bytes). The ONE statement of that
/// charge, read by `display_rgba8` and the float layer thumbnail.
pub(crate) fn display_scratch(width: u32, height: u32) -> Result<u64, String> {
    let count = pixel_count(width, height)? as u64;
    count
        .checked_mul(12)
        .and_then(|bytes| bytes.checked_add(BOOKKEEPING_BYTES))
        .ok_or_else(|| "display working-memory calculation overflow".to_string())
}
