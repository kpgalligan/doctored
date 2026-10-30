//! The Grayscale document mode (#547): the policy, the edit gate and the
//! refusal sentences (`designs/grayscale-mode.md`, the ADR of the closed
//! mode).
//!
//! # What a Gray document IS
//!
//! A `PrecisionDocument` whose `gray` field is `Some` (the `cmyk` field's
//! pattern). Its storage is the RGB precision document unchanged, holding
//! R = G = B: u16 at 8 and 16 bits (an 8-bit document stores each code ×257,
//! and every conversion into it rounds to that grid) and the linear-light
//! float at 32 bits (R = G = B = relative luminance `Y`). The policy carries
//! the Gray ICC profile (its BYTES kept verbatim, so a later save can embed
//! exactly what was opened), the declared depth and the profile's RGB TWIN
//! (`icc_builtin::twin_blob`): sRGB's primaries with the Gray profile's
//! `kTRC` as all three curves. `PrecisionDocument::profile` and every
//! layer's profile ARE that twin, so a neutral pixel's luminance is the Gray
//! profile's `kTRC(code)` and the view, the render, the sample and the
//! compositor are the RGB ones, unchanged.
//!
//! # The gate
//!
//! [`admit_edit`], asked first by `ffi_precision_edit::request`, admits the
//! structure edits (phase 1a) and the layer structure (phase 1b): New
//! Layer, groups, clipping, merges, Stamp Visible, layer masks, Fill
//! Opacity and knockout, the blend modes of [`GRAY_BLEND_MODES`] (the 21,
//! measured against Photoshop 26.10 by
//! `scripts/photoshop-probe/gray_mode_take.py`) and Edit ▸ Fill; the paint
//! tools (phase 2a-ii: Brush and Fill's `paint`, the Eraser's
//! `erase_native`, the Paint Bucket and the Gradient, their modes in the
//! same list), whose inks the paint path greys; the retouch tools (phase
//! 2c: Dodge, Burn, Sponge, Blur, Sharpen, Smudge, Clone Stamp, Healing,
//! Spot Healing and Patch; Clone Stamp's mode in the same list) on R = G = B
//! through their RGB kernels (`gray_filter`'s module doc); Stroke…
//! (`stroke_layer_edge`, its mode in the same list, its ink greyed as a
//! paint ink), Content-Aware Fill and Import Channel (phase 2e). A mode
//! outside that list refuses permanently ([`blend_refusal`]), as does
//! Remove Red Eye. The adjustments (phase 2b) and the filters (phase 2c) are
//! `gray_adjust`'s and `gray_filter`'s gate, asked by the selected-edit
//! export. It admits every layer effect (phase 2d: its modes in the same
//! list, its colours stored as their gray, `gray_style`), Blend If on Gray
//! only, the global light, and the alpha channels as structure and as
//! paint, Fill, filter, read and statistics targets wherever the mask is
//! one; the Gray channel target is the layer, and the colour planes (Red,
//! Green, Blue, Alpha and the ink plates) refuse for good
//! ([`PLANE_REFUSAL`]). It admits geometry (phase 4: Image and Canvas Size,
//! Crop, Perspective Crop, Straighten, the rotations and flips, Free
//! Transform, the warps, Puppet and Perspective Warp), align/distribute, the
//! selected pixels, the source selections, Duplicate to Document, Layer Via
//! and Stamp Selected, all through the RGB kernels on R = G = B, which keep
//! the equality; a warp's binding is baked in the same edit
//! (`precision_warp`), and the clipboard is `gray_clipboard`'s. Assign and
//! Convert to Profile and Bits/Channel are `gray_profile`'s own edit (phase
//! 5a), and the RGB profile exports refuse a Gray document
//! ([`RGB_PROFILE_EDIT_REFUSAL`]). The `proof` family is admitted (phase
//! 5b): a Gray composite is RGB on R = G = B in the twin, so the RGB proof
//! takes it unchanged. Type, shape and fill layers publish through
//! `artwork_document` (phase 6a-i: their colours greyed and their pixels
//! folded by `gray_artwork`), and paths and vector masks are carried. Links
//! and layer comps (`gray_comps`) and the legacy `PaintEdit` ops (`erase`,
//! `clone`, `heal`, `rasterize`) are admitted (phase 6a-ii): `clone` and
//! `heal` keep R = G = B, `erase` touches alpha only and `rasterize`'s ink
//! is greyed as a paint ink. Stack Modes and the scalar edit of the Gray
//! layer (`scalar_edit` target `gray`, Apply Image's write) are admitted
//! (phase 6b); the neural, generative and Merge to HDR results are folded by
//! `gray_artwork`, and so is a placed image (the close: Place folds its
//! rendered pixels and keeps no retained source).
//!
//! Every edit the parsers accept is admitted or refused here for good, by
//! name: an RGB-only operation through [`refused_sentence`], a scalar read
//! or write of the layer itself through [`scalar_target_refusal`], a colour
//! plane through [`PLANE_REFUSAL`]. A request RGB rejects too (an unknown
//! op, a malformed style) is left to the parser, so it gets RGB's error.
//! `precision_capability::GRAY_FAMILIES` is the admitted family table, and
//! the host mirrors it in its own lists (`GrayDocument` in the app), asked
//! before every menu item, tool and MCP handler.
//!
//! # Luminance
//!
//! Mode ▸ Grayscale and every paint ink on a Gray document discard colour by
//! one rule: the relative PCS luminance of the colour through its profile
//! ([`GrayPolicy::luminance_code`] for u16 storage, encoded through the
//! twin; [`GrayPolicy::linear_luminance`] for linear-light float).
//! `gray_convert::gray_image` and `precision_paint::prepare_colors` /
//! `float_paint_ink::linear_inks` call these two, so a Fill of a colour
//! equals that colour converted by Mode ▸ Grayscale.

use std::sync::Arc;

use serde_json::{Map, Value};

use crate::blend::BlendMode;
use crate::gray_working::GrayWorkingSpace;
use crate::icc::IccProfile;
use crate::icc_lut::{DeviceSpace, ProofProfile};
use crate::icc_transform::PrecisionToneEncoder;
use crate::metadata::{Metadata, Resolution};
use crate::precision_document::PrecisionDocument;
use crate::precision_image::{admit, pixel_count, PrecisionImage};

/// The storage-free name this mode reports in descriptors.
pub(crate) const COLOR_MODE_GRAY: &str = "gray";

/// The edits a Gray document admits: CMYK phase 1's structure edits and the
/// group disclosure (`layer_open`, phase 1a), then phase 1b's layer
/// structure — New Layer, groups, clipping, merges, Stamp Visible, Merge
/// Group and the layer masks; phase 2d's global light and the alpha
/// channels' New, Delete, Rename, Duplicate and Channel Options, and phase
/// 2e's Import Channel; phase 4's geometry, Perspective Crop, align and
/// distribute, Duplicate to Document, Layer Via and Stamp Selected; phase
/// 6a-ii's links and layer comps; phase 6b's Stack Modes. The
/// conditional edits (blend modes, styles, Blend If, Fill, the scalar
/// targets) are [`admit_edit`]'s own arms.
const ADMITTED_EDITS: &[&str] = &[
    "global_light",
    "set_channel",
    "remove_channel",
    "rename_channel",
    "duplicate_channel",
    "channel_properties",
    "import_channel",
    "rename_layer",
    "layer_visible",
    "layer_label",
    "layer_lock",
    "layer_open",
    "move_layer_to",
    "arrange_layer",
    "remove_layers",
    "flatten",
    "add_layer",
    "group_layers",
    "ungroup_layer",
    "layer_clipped",
    "merge_layers",
    "merge_down",
    "merge_visible",
    "stamp_visible",
    "merge_group",
    "set_mask",
    "remove_mask",
    "mask_properties",
    "apply_mask",
    // Phase 4: geometry, Perspective Crop, align/distribute, Duplicate to
    // Document, Layer Via and Stamp Selected, on R = G = B through the RGB
    // kernels.
    "affine",
    "perspective",
    "affine_set",
    "perspective_set",
    "resize",
    "crop",
    "canvas_size",
    "straighten",
    "rotate_canvas",
    "rotate90",
    "rotate180",
    "rotate270",
    "flip_horizontal",
    "flip_vertical",
    "perspective_crop",
    "align_set",
    "distribute_set",
    "duplicate_to_new_document",
    "layer_via",
    "stamp_selected",
    // Phase 6a-ii: links and layer comps (`gray_comps`).
    "link_layers",
    "unlink_layers",
    "layer_comp",
    // Phase 6b: Stack Modes, on R = G = B through the RGB stack kernel (a
    // per-channel statistic of equal channels stays equal).
    "stack_mode",
];

/// The blend modes a Gray document offers, Normal first, in the layer menu's
/// order, at 8 and 16 bits under Dot Gain 20 % and Gray Gamma 2.2 alike
/// (measured against Photoshop 26.10: `gray-mode-547/take1`,
/// `scripts/photoshop-probe/gray_mode_take.py`): every mode but Darker
/// Color, Lighter Color, Hue, Saturation, Color and Luminosity, which refuse
/// permanently by name ([`blend_refusal`]). Pass
/// Through is a group's, as in RGB. A gray document composites them through
/// the one compositor (`blend.rs`) on R = G = B.
pub(crate) const GRAY_BLEND_MODES: &[BlendMode] = &[
    BlendMode::Normal,
    BlendMode::Dissolve,
    BlendMode::Darken,
    BlendMode::Multiply,
    BlendMode::ColorBurn,
    BlendMode::LinearBurn,
    BlendMode::Lighten,
    BlendMode::Screen,
    BlendMode::ColorDodge,
    BlendMode::Addition,
    BlendMode::Overlay,
    BlendMode::SoftLight,
    BlendMode::HardLight,
    BlendMode::VividLight,
    BlendMode::LinearLight,
    BlendMode::PinLight,
    BlendMode::HardMix,
    BlendMode::Difference,
    BlendMode::Exclusion,
    BlendMode::Subtract,
    BlendMode::Divide,
];

/// `mode`'s refusal on a Gray document (`float` for a 32-bit one), or
/// `None` for a mode it offers: Pass Through (a group's rule decides), a
/// mode of [`GRAY_BLEND_MODES`] — at 32 bits only one the float document
/// admits (`float_document::admitted_blend`), the others refusing with the
/// float document's own sentence. A mode outside the list is a permanent
/// refusal (no phase lifts it).
pub(crate) fn blend_refusal(mode: BlendMode, float: bool) -> Option<String> {
    if mode == BlendMode::PassThrough {
        return None;
    }
    if !GRAY_BLEND_MODES.contains(&mode) {
        return Some(format!(
            "The {} blend mode is unavailable in a Grayscale document.",
            crate::cmyk_document::mode_name(mode)
        ));
    }
    (float && !crate::float_document::admitted_blend(mode))
        .then(|| crate::float_document::blend_refusal(mode))
}

/// The mode's refusal of an operation an RGB document offers and a Gray
/// document does not: `"{operation} is unavailable in a Grayscale document:
/// Doctored offers it on RGB documents only."` Asked only for operations an
/// RGB document accepts (the families `precision_capability::require_gray`
/// does not list, such as `color_convert` and `color_assign`); a request RGB
/// rejects too is left to its parser.
pub(crate) fn refused_sentence(operation: &str) -> String {
    format!(
        "{operation} is unavailable in a Grayscale document: Doctored offers it on RGB documents only."
    )
}

/// The refusal of a scalar edit, statistics or plane read of the Gray layer
/// itself where only a mask or an alpha channel is admitted (no target, or a
/// target other than `mask`, `channel`, `effective_mask` for a read, `gray`
/// for a write, or a colour plane): the Gray channel IS the layer.
fn scalar_target_refusal(op: &str) -> String {
    format!(
        "{op} of the Gray layer is unavailable in a Grayscale document: its one colour channel, Gray, is the layer, so a scalar read takes target mask or channel."
    )
}

/// The refusal of the RGB Assign and Convert to Profile exports on a Gray
/// document (`ffi_precision_color`, `ffi_output_color`): its profile and
/// depth edits are `gray_profile`'s (#547 phase 5a).
pub(crate) const RGB_PROFILE_EDIT_REFUSAL: &str = "a Grayscale document's Assign Profile, Convert to Profile and Bits/Channel are its own Gray profile edit (dr_precision_doc_gray_profile_edit)";

/// The refusal of a colour-plane target (Red, Green, Blue, Alpha or an ink
/// plate) of `paint_blend`, a retouch tool, a scalar edit, its statistics or
/// a plane read, for good: the Gray row is the composite row, so the
/// Gray channel target IS the layer.
const PLANE_REFUSAL: &str = "A colour plane is unavailable in a Grayscale document: its one colour channel, Gray, is the layer, and Doctored offers the Red, Green, Blue and Alpha targets on RGB documents only.";

/// `value[key]` as a blend mode's refusal (`float` for a 32-bit document);
/// an absent or unknown mode is left to the edit's own parser.
fn mode_refusal(value: &Value, key: &str, float: bool) -> Option<String> {
    let raw = value[key].as_i64()?;
    let mode = BlendMode::from_c(i32::try_from(raw).ok()?)?;
    blend_refusal(mode, float)
}

/// The gate `ffi_precision_edit::request` asks first on a Gray document
/// (`float` = a 32-bit one): `Ok` for an edit of [`ADMITTED_EDITS`] or an
/// admitted conditional edit — a layer's blend mode, a Brush stroke or Fill
/// (`paint_blend`), a Paint Bucket (`bucket_fill`) or a Gradient in
/// [`GRAY_BLEND_MODES`], the mode-less `paint` and `erase_native` (phase
/// 2a-ii; every ink is greyed by `precision_paint::prepare_colors` or
/// `float_paint_ink::linear_inks`) and phase 6a-ii's legacy `erase`, `clone`,
/// `heal` and `rasterize` (u16 storage only, `PaintEdit`'s own rule), the
/// retouch tools of phase 2c (Dodge and Burn, the Blur, Sharpen and Sponge
/// brushes, Healing, Spot Healing and a Clone Stamp in [`GRAY_BLEND_MODES`];
/// none on a colour plane; Remove Red Eye refuses for good), phase 2e's
/// Stroke… (`stroke_layer_edge`, its mode in [`GRAY_BLEND_MODES`]),
/// Content-Aware Fill (not on a colour plane) and Import Channel (in
/// [`ADMITTED_EDITS`]), a layer style (phase 2d: its effect modes and colours
/// are `LayerStyleEdit::target`'s, `gray_style`) and a Blend If with
/// no channel but Gray, the scalar edit and statistics of a layer mask or an
/// alpha channel and the plane read of either (a colour plane refuses for
/// good, [`PLANE_REFUSAL`]), phase 6b's scalar edit of the Gray layer
/// itself (target `gray`, Apply Image's write; its statistics and plane read
/// still refuse), phase 4's selected pixels (move, transform,
/// puppet and perspective warp) of a layer, its mask or an alpha channel and
/// the source selections but a colour channel's (both refusing a colour
/// plane by [`PLANE_REFUSAL`]). A scalar target the gate does not admit
/// refuses by [`scalar_target_refusal`]; a `layer_style` whose `style` is
/// neither null nor an object, and an op no arm names, are left to the
/// parser (`Ok`), so the request gets the error RGB gives it.
pub(crate) fn admit_edit(text: &str, float: bool) -> Result<(), String> {
    let value: Value =
        serde_json::from_str(text).map_err(|e| format!("invalid high-depth edit request: {e}"))?;
    let op = value["op"].as_str().unwrap_or("");
    if ADMITTED_EDITS.contains(&op) {
        return Ok(());
    }
    let target = value["target"].as_str();
    let refusal = match op {
        "layer_blend" => mode_refusal(&value, "blend_mode", float),
        "paint_blend" if value.get("plane").is_some() => Some(PLANE_REFUSAL.to_string()),
        "paint_blend" | "bucket_fill" | "gradient" | "stroke_layer_edge" => {
            mode_refusal(&value, "blend_mode", float)
        }
        "paint" | "erase" | "clone" | "heal" | "rasterize" | "erase_native" => None,
        "dodge_burn" | "retouch_brush" | "retouch_heal" | "spot_heal" | "retouch_clone"
        | "content_aware_fill"
            if value.get("plane").is_some() =>
        {
            Some(PLANE_REFUSAL.to_string())
        }
        "dodge_burn" | "retouch_brush" | "retouch_heal" | "spot_heal" | "content_aware_fill" => {
            None
        }
        "retouch_clone" => mode_refusal(&value, "blend_mode", float),
        "red_eye" => Some(crate::gray_filter::RED_EYE_REFUSAL.to_string()),
        "layer_style" => {
            // Null is Clear Layer Style, admitted.
            let style = &value["style"];
            if !style.is_null() && !style.is_object() {
                // Malformed: the parser's own error, as on RGB.
                None
            } else {
                crate::gray_style::blend_if_refusal(&style["blend_if"])
            }
        }
        "layer_blend_if" => crate::gray_style::blend_if_refusal(&value["blend_if"]),
        "move_selected" | "transform_selected" | "puppet_selected" | "perspective_selected" => {
            match target {
                Some(
                    "red" | "green" | "blue" | "alpha" | "cyan" | "magenta" | "yellow" | "black",
                ) => Some(PLANE_REFUSAL.to_string()),
                _ => None,
            }
        }
        "selection" if value["operation"]["kind"].as_str() == Some("color") => {
            Some(PLANE_REFUSAL.to_string())
        }
        "selection" => None,
        "scalar_edit" | "scalar_statistics" | "read_plane" => match target {
            Some("mask" | "channel") => None,
            Some("effective_mask") if op == "read_plane" => None,
            // Phase 6b: Apply Image's layer write (`ScalarTarget::Gray`).
            Some("gray") if op == "scalar_edit" => None,
            Some("red" | "green" | "blue" | "alpha" | "cyan" | "magenta" | "yellow" | "black") => {
                Some(PLANE_REFUSAL.to_string())
            }
            _ => Some(scalar_target_refusal(op)),
        },
        // Every op `Edit::parse` and `RetouchEdit::parse` accept is in
        // `ADMITTED_EDITS` or has an arm above; any other is the parser's
        // own error, as on RGB.
        _ => None,
    };
    refusal.map_or(Ok(()), Err)
}

/// The depth a Gray document declares.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum GrayDepth {
    Eight,
    Sixteen,
    ThirtyTwo,
}

impl GrayDepth {
    pub(crate) fn bits(self) -> u32 {
        match self {
            GrayDepth::Eight => 8,
            GrayDepth::Sixteen => 16,
            GrayDepth::ThirtyTwo => 32,
        }
    }

    pub(crate) fn from_bits(bits: u32) -> Option<Self> {
        match bits {
            8 => Some(GrayDepth::Eight),
            16 => Some(GrayDepth::Sixteen),
            32 => Some(GrayDepth::ThirtyTwo),
            _ => None,
        }
    }
}

/// The Gray mode's policy (module doc).
#[derive(Clone)]
pub struct GrayPolicy {
    icc: Arc<[u8]>,
    name: String,
    depth: GrayDepth,
    twin: Arc<IccProfile>,
    encoder: PrecisionToneEncoder,
    /// The Duotone description (#547 Duotone phase 1a, `duotone_document`):
    /// a Duotone document IS a Gray document whose policy carries one. Every
    /// path that carries the policy (`with_depth`, `with_gray`'s twin
    /// rebinding, `rebuilt`, a clone) carries it; [`GrayPolicy::new`] starts
    /// without one.
    duotone: Option<Arc<crate::duotone_ink::DuotoneInks>>,
}

impl GrayPolicy {
    /// `Err` names why `icc` cannot be a Gray document's profile: not a
    /// profile, not Gray, no `kTRC`, or a twin this build cannot encode into.
    pub(crate) fn new(icc: &[u8], depth: GrayDepth) -> Result<Self, String> {
        if icc.len() > crate::drdc::MAX_DRDC_BLOB_LEN as usize {
            return Err("the Gray profile exceeds the native byte limit".into());
        }
        let profile =
            ProofProfile::parse(icc).map_err(|e| format!("cannot use the Gray profile: {e}"))?;
        if profile.device() != DeviceSpace::Gray {
            return Err(format!(
                "cannot use the Gray profile: “{}” is not a Gray profile",
                profile.name()
            ));
        }
        if profile.gray_curve().is_none() {
            return Err(format!(
                "cannot use the Gray profile: “{}” has no gray tone curve",
                profile.name()
            ));
        }
        let twin = crate::icc_builtin::twin_blob(icc)
            .map_err(|e| format!("cannot use the Gray profile: {e}"))?;
        let twin = Arc::new(IccProfile::parse(&twin).ok_or_else(|| {
            format!(
                "cannot use the Gray profile: “{}” has no usable RGB twin",
                profile.name()
            )
        })?);
        let model = twin.model().ok_or_else(|| {
            format!(
                "cannot use the Gray profile: “{}” has no matrix/TRC twin",
                profile.name()
            )
        })?;
        let encoder = PrecisionToneEncoder::new(model)
            .map_err(|e| format!("cannot use the Gray profile: {e}"))?;
        Ok(Self {
            icc: Arc::from(icc),
            name: profile.name().to_string(),
            depth,
            twin,
            encoder,
            duotone: None,
        })
    }

    /// The built-in Gray profile of `space` (`icc_builtin::gray_blob`).
    pub(crate) fn for_space(space: GrayWorkingSpace, depth: GrayDepth) -> Result<Self, String> {
        Self::new(crate::icc_builtin::gray_blob(space), depth)
    }

    /// The Gray ICC bytes, verbatim.
    pub(crate) fn bytes(&self) -> &[u8] {
        &self.icc
    }

    pub(crate) fn name(&self) -> &str {
        &self.name
    }

    pub(crate) fn depth(&self) -> GrayDepth {
        self.depth
    }

    /// The RGB twin every layer of the document is in (module doc).
    pub(crate) fn twin(&self) -> &Arc<IccProfile> {
        &self.twin
    }

    /// Relative luminance `y` as a unit gray code, through the twin's encoder
    /// (the inverse of the Gray profile's `kTRC`).
    pub(crate) fn encode(&self, y: f64) -> f64 {
        f64::from(self.encoder.rgb([y, y, y])[0])
    }

    /// Linear twin components as unit codes: the encoder's `rgb`, the
    /// Duotone view's last step (`duotone_document`).
    pub(crate) fn encode_linear(&self, linear: [f64; 3]) -> [f32; 3] {
        self.encoder.rgb(linear)
    }

    /// A unit gray code's relative luminance through the Gray profile's
    /// `kTRC` (the twin's `MatrixTrc::decode_rgb`): the inverse of
    /// [`GrayPolicy::encode`]. `new` refused a twin with no model, so the
    /// fallback (the code itself) is never taken.
    pub(crate) fn decode_unit(&self, code: f64) -> f64 {
        let c = code.clamp(0.0, 1.0) as f32;
        self.twin
            .model()
            .map_or(code, |model| model.decode_rgb([c, c, c])[0])
    }

    /// The Duotone description, if the document is a Duotone one.
    pub(crate) fn duotone(&self) -> Option<&Arc<crate::duotone_ink::DuotoneInks>> {
        self.duotone.as_ref()
    }

    /// The same policy with `duotone` as its description (`None` drops it).
    pub(crate) fn with_duotone(
        &self,
        duotone: Option<Arc<crate::duotone_ink::DuotoneInks>>,
    ) -> Self {
        Self {
            duotone,
            ..self.clone()
        }
    }

    /// The same policy at `depth`.
    pub(crate) fn with_depth(&self, depth: GrayDepth) -> Self {
        Self {
            depth,
            ..self.clone()
        }
    }

    /// One unit gray code at the declared depth: the 8-bit document's ×257
    /// grid at 8 bits, the nearest u16 code otherwise.
    pub(crate) fn code(&self, unit: f64) -> u16 {
        let wide = (unit.clamp(0.0, 1.0) * 65535.0).round() as u32;
        match self.depth {
            GrayDepth::Eight => ((wide + 128) / 257 * 257) as u16,
            _ => wide as u16,
        }
    }

    /// The gray code (at the declared depth) of `rgb`, a colour in `from`
    /// (unit channels): its relative PCS luminance encoded through the twin
    /// (module doc, "Luminance").
    pub(crate) fn luminance_code(&self, from: &IccProfile, rgb: [f64; 3]) -> u16 {
        self.code(self.encode(crate::cmyk_color::document_xyz(from, rgb)[1]))
    }

    /// The relative luminance `Y` of `rgb`, LINEAR channels in `from`: the
    /// profile's Y row (module doc, "Luminance"). Refuses a profile with no
    /// matrix/TRC model.
    pub(crate) fn linear_luminance(from: &IccProfile, rgb: [f64; 3]) -> Result<f64, String> {
        let model = from
            .model()
            .ok_or("32-bit Grayscale requires an RGB matrix/TRC source profile")?;
        Ok(model.linear_to_pcs(rgb)[1])
    }

    pub(crate) fn record_resources(&self, inventory: &mut crate::resources::Inventory) {
        inventory.record(self.icc.as_ptr() as usize, self.icc.len(), "profile", false);
        inventory.record(
            &self.encoder as *const PrecisionToneEncoder as usize,
            PrecisionToneEncoder::scratch_bytes() as usize,
            "profile",
            false,
        );
        inventory.object(&*self.twin, "profile", false);
        self.twin.record_resources(inventory);
    }
}

impl PrecisionDocument {
    /// True for a Grayscale document.
    pub fn is_gray(&self) -> bool {
        self.gray.is_some()
    }

    /// The Gray policy; `None` for every other document.
    pub(crate) fn gray_policy(&self) -> Option<&Arc<GrayPolicy>> {
        self.gray.as_ref()
    }

    /// This document as a Gray document under `policy`. Refuses a CMYK
    /// document, a document whose profile is not the policy's twin (by
    /// bytes; every layer shares the document's profile handle, `build`'s
    /// rule), storage that disagrees with the depth (float ⇔ 32), an
    /// adjustment record the Gray table refuses (`gray_adjust`), and a style
    /// or Blend If the mode cannot hold (`gray_style::check_records`). The
    /// policy adopts the document's own profile handle as its twin.
    pub(crate) fn with_gray(mut self, policy: Arc<GrayPolicy>) -> Result<Self, String> {
        if self.is_cmyk() {
            return Err("a CMYK document cannot be a Grayscale document".into());
        }
        if self.profile.bytes() != policy.twin().bytes() {
            return Err("a Grayscale document's layers must be in its Gray profile's twin".into());
        }
        if self.is_float() != (policy.depth() == GrayDepth::ThirtyTwo) {
            return Err("a Grayscale document's storage does not match its depth".into());
        }
        crate::gray_adjust::check_records(&self.layers)?;
        crate::gray_style::check_records(&self.layers, &self.profile, &policy)?;
        let policy = if Arc::ptr_eq(policy.twin(), &self.profile) {
            policy
        } else {
            Arc::new(GrayPolicy {
                twin: self.profile.clone(),
                ..(*policy).clone()
            })
        };
        self.gray = Some(policy);
        Ok(self)
    }

    /// This document with no Gray policy: the RGB document its storage is.
    pub(crate) fn without_gray(&self) -> Self {
        let mut out = self.clone();
        out.gray = None;
        out
    }

    /// File ▸ New ▸ Grayscale: one opaque-or-transparent layer of the sRGB
    /// fill's luminance in the Gray profile (`icc`, else the request's
    /// `space`; neither refuses). Request `{"depth":8|16|32,"space"?,
    /// "fill_srgb":[r,g,b,a] in 0…1,"layer_name","ppi"}`. At 32 bits the
    /// 16-bit blank is promoted to float.
    pub fn new_gray(
        width: u32,
        height: u32,
        icc: Option<&[u8]>,
        request: &str,
        limit: u64,
        other: u64,
    ) -> Result<Self, String> {
        let value: Value = serde_json::from_str(request)
            .map_err(|e| format!("new Grayscale document request is not valid JSON: {e}"))?;
        let object: &Map<String, Value> = value
            .as_object()
            .ok_or("new Grayscale document request must be a JSON object")?;
        for key in object.keys() {
            if !matches!(
                key.as_str(),
                "depth" | "space" | "fill_srgb" | "layer_name" | "ppi"
            ) {
                return Err(format!(
                    "new Grayscale document request has an unknown key \"{key}\""
                ));
            }
        }
        let depth = object
            .get("depth")
            .and_then(Value::as_u64)
            .and_then(|bits| GrayDepth::from_bits(bits as u32))
            .ok_or("new Grayscale document depth must be 8, 16 or 32")?;
        let policy = match (icc, object.get("space").and_then(Value::as_str)) {
            (Some(bytes), _) => GrayPolicy::new(bytes, depth)?,
            (None, Some(name)) => GrayPolicy::for_space(
                GrayWorkingSpace::from_name(name)
                    .ok_or_else(|| format!("\"{name}\" is not a Gray working space"))?,
                depth,
            )?,
            (None, None) => {
                return Err("a new Grayscale document needs a Gray profile or space".into())
            }
        };
        let fill: Vec<f64> = object
            .get("fill_srgb")
            .and_then(Value::as_array)
            .map(|items| items.iter().filter_map(Value::as_f64).collect())
            .unwrap_or_default();
        if fill.len() != 4 || fill.iter().any(|v| !(0.0..=1.0).contains(v)) {
            return Err("new Grayscale document fill_srgb must be four numbers in 0…1".into());
        }
        let name = object
            .get("layer_name")
            .and_then(Value::as_str)
            .unwrap_or("Background");
        let ppi = object.get("ppi").and_then(Value::as_f64).unwrap_or(72.0) as f32;
        // The new document's display preparation the host runs right after,
        // charged as a gray open charges it (`gray_io::open_scratch` with no
        // file; `scripts/gray-admission` `new-gray-16`). A float pixel holds
        // twice a u16 pixel's bytes (16 B/px against 8), so at 32 bits every
        // per-pixel term of that preparation doubles: `new-gray-32` measured
        // 1,155 MB at 24 MP, twice `new-gray-16`'s 578 MB.
        pixel_count(width, height)?;
        let charge = crate::gray_io::open_scratch(width, height, 0)?;
        let charge = if depth == GrayDepth::ThirtyTwo {
            charge.saturating_mul(2)
        } else {
            charge
        };
        admit(0, charge, other, limit)?;
        let srgb = IccProfile::srgb();
        let y = crate::cmyk_color::document_xyz(&srgb, [fill[0], fill[1], fill[2]])[1];
        let sixteen = policy.with_depth(match depth {
            GrayDepth::Eight => GrayDepth::Eight,
            _ => GrayDepth::Sixteen,
        });
        let code = sixteen.code(policy.encode(y));
        let alpha = sixteen.code(fill[3]);
        let image = PrecisionImage::filled_rgba16(
            width,
            height,
            [code, code, code, alpha],
            policy.twin().clone(),
        )?;
        let document = PrecisionDocument::from_image_named(
            image,
            Metadata::default(),
            Resolution { x: ppi, y: ppi },
            name,
        )?;
        let document = if depth == GrayDepth::ThirtyTwo {
            document.promote_to_float(limit, other, None)?
        } else {
            document
        };
        document.with_gray(Arc::new(policy))
    }
}
