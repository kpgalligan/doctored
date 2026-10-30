//! The Lab Color document mode (#547): the policy, the edit gate and the
//! refusal sentences. Decisions and the Photoshop record: `designs/lab-mode.md`.
//!
//! # What a Lab document IS
//!
//! A `PrecisionDocument` whose `lab` field is `Some` (the `gray` field's
//! pattern). Its storage is the RGB precision document unchanged, u16 only:
//! R, G and B hold L\*, a\* and b\* (D50, the ICC PCS white) in the
//! ICC v4 16-bit Lab encoding — L\* = 100·v, a\*/b\* = 255·v − 128, so a\* =
//! b\* = 0 is 0x8080. An 8-bit document stores each code ×257, and every
//! conversion into it rounds samples and alpha to that grid (Gray's rule).
//! There is no 32-bit Lab.
//! `PrecisionDocument::profile` and every layer's profile ARE the Lab twin
//! ([`lab_twin`], `icc_builtin::lab_twin_blob`): an RGB-data, Lab-PCS
//! profile with identity tables, so `IccProfile::parse` keeps it as `RgbLut`,
//! the host draws the core's Display P3 view of it
//! (`lab_convert::lab_display`: the view, thumbnails, `render`), and Normal
//! compositing — opacity, masks, groups, clipping — is the RGB
//! compositor's linear mix of Lab codes; every
//! other mode composites by its Lab arm (`lab_blend`).
//!
//! # The gate
//!
//! [`admit_edit`], asked first by `ffi_precision_edit::request`, decides by
//! these rules:
//!
//! - **Codes, no Lab arm.** Structure, masks, channels, geometry, warps,
//!   selected pixels, Layer Via, Stack Modes, links and comps, the legacy
//!   `erase`, `clone`, `heal` and `rasterize` ops, `import_channel`, and the
//!   scalar edits and reads of a mask, an alpha channel or the Lightness, a
//!   and b planes run the RGB kernel on the L\*a\*b\* codes; a scalar edit
//!   asks `lab_filter::scalar_refusal`.
//! - **Colours as codes.** Every ink, fill, stroke and pad colour is authored
//!   sRGB and stored by Mode ▸ Lab Color's rule
//!   (`precision_paint::prepare_colors`, `lab_artwork`).
//! - **Blend modes.** [`LAB_BLEND_MODES`] is every mode a Lab document
//!   offers, each with a Lab arm; [`LAB_NOT_OFFERED`] refuses for good.
//! - **Selections.** `alpha`, `color` (the planes), every Color Range range,
//!   `quick`, `refine_edge` and the Magic Wand are admitted, and Select ▸
//!   Similar and Grow at 16 bits (`lab_select`, #683); `luminance`
//!   answers [`LUMINANCE_REFUSAL`]; Similar and Grow at 8 bits refuse naming
//!   #683 ([`SELECTION_REFUSALS`]).
//! - **No document profile.** The ops of [`NO_PROFILE_OPS`] refuse for good,
//!   and the four profile exports answer
//!   [`PROFILE_EDIT_REFUSAL`]; the document converts through Image ▸ Mode.
//!   The proof family proofs the Display P3 view ([`PROOF_VIEW_REFUSAL`]).
//! - **Everything else** reaches its own parser and answers as on RGB, so
//!   no refusal names a phase. Adjustments, filters, Remove, the neural and
//!   generative tools and Auto-Align run through their own exports and Lab
//!   modules (`lab_adjust`, `lab_filter`, `lab_neural`, `lab_stack`); Sky
//!   Replacement and Auto-Blend Layers refuse for good.
//!
//! `precision_capability::LAB_FAMILIES` is the admitted family table, and
//! the host mirrors it in its own lists (`LabDocument` in the app), asked
//! before every menu item, tool and MCP handler.

use std::sync::{Arc, OnceLock};

use serde_json::{Map, Value};

use crate::blend::BlendMode;
use crate::icc::IccProfile;
use crate::lab_blend::LabGrid;
use crate::metadata::{Metadata, Resolution};
use crate::precision_document::PrecisionDocument;
use crate::precision_image::{admit, pixel_count, PrecisionImage};

/// The storage-free name this mode reports in descriptors.
pub(crate) const COLOR_MODE_LAB: &str = "lab";

/// The selections a Lab document refuses, one row per (key, fate): the key,
/// the depths it refuses at, and `Some(issue)` while it awaits a Lab
/// tolerance rule a Photoshop take has yet to fit, `None` where it refuses
/// for good. A key absent from the table is admitted at both depths. The
/// keys are `select_magic_wand`, `select_similar` (global) and `select_grow`
/// (`select_similar` with `contiguous` true; a key, never a tool name).
/// `lab_select_682_take.py`'s take fitted the wand at both depths and
/// Similar and Grow at 16 bits only (`lab_select`'s module doc).
const SELECTION_REFUSALS: &[(&str, &[LabDepth], Option<&str>)] = &[
    ("select_similar", &[LabDepth::Eight], Some("#683")),
    ("select_grow", &[LabDepth::Eight], Some("#683")),
];

/// The [`SELECTION_REFUSALS`] key of a source selection's `kind` and
/// `contiguous`, or `None` for a kind the table never names.
pub(crate) fn selection_key(kind: &str, contiguous: bool) -> Option<&'static str> {
    match kind {
        "magic_wand" => Some("select_magic_wand"),
        "similar" if contiguous => Some("select_grow"),
        "similar" => Some("select_similar"),
        _ => None,
    }
}

/// `key`'s refusal at `depth` from [`SELECTION_REFUSALS`], or `None` when it
/// is admitted there. A row that lists one depth names it ("an 8-bit" or
/// "a 16-bit" Lab Color document); `select_grow` reads `select_similar with
/// contiguous true (Grow)`. Awaiting a take: `"{label} is unavailable in
/// {document} until its Lab tolerance rule is measured ({issue})."`;
/// refused for good: `"{label} is unavailable in {document}."`.
pub(crate) fn selection_refusal(key: &str, depth: LabDepth) -> Option<String> {
    let (_, depths, issue) = SELECTION_REFUSALS
        .iter()
        .find(|(name, depths, _)| *name == key && depths.contains(&depth))?;
    let label = if key == "select_grow" {
        "select_similar with contiguous true (Grow)"
    } else {
        key
    };
    let document = match (depths.len(), depth) {
        (1, LabDepth::Eight) => "an 8-bit Lab Color document",
        (1, LabDepth::Sixteen) => "a 16-bit Lab Color document",
        _ => "a Lab Color document",
    };
    Some(match issue {
        Some(issue) => format!(
            "{label} is unavailable in {document} until its Lab tolerance rule is measured \
             ({issue})."
        ),
        None => format!("{label} is unavailable in {document}."),
    })
}

/// The profile edits a Lab document refuses for good: it has no document
/// profile (phase 5b; [`refused_sentence`]).
const NO_PROFILE_OPS: &[&str] = &["profile", "color_assign", "color_convert"];

/// The four profile exports' refusal on a Lab document (phase 5b):
/// `dr_precision_doc_assign_profile`, `_convert_to_profile`,
/// `_convert_to_profile_with` and `_assign_profile_rasterizing`.
pub(crate) const PROFILE_EDIT_REFUSAL: &str = "a Lab Color document has no document profile: \
     it converts through Image ▸ Mode (dr_precision_doc_convert_lab_to_rgb, \
     dr_precision_doc_convert_to_cmyk, dr_precision_doc_convert_to_gray)";

/// `soft_proof::Proofer::new`'s refusal of the twin as a document profile
/// (phase 5b): the host proofs the Display P3 view instead.
pub(crate) const PROOF_VIEW_REFUSAL: &str =
    "a Lab Color projection proofs through its Display P3 view (dr_precision_image_lab_display)";

/// The edits a Lab document admits: phase 1's structure edits (Gray phase
/// 1a's and the group disclosure for the groups RGB → Lab carries), then
/// phase 1b's layer structure — New Layer, groups, clipping, merges, Stamp
/// Visible, Merge Group, the layer masks — the alpha channels' New,
/// Delete, Rename, Duplicate and Channel Options, and (phase 2e) Global
/// Light, which the admitted effects read. The conditional edits
/// (blend modes, styles, Fill, the scalar targets) are [`admit_edit`]'s own
/// arms.
const ADMITTED_EDITS: &[&str] = &[
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
    "set_channel",
    "remove_channel",
    "rename_channel",
    "duplicate_channel",
    "channel_properties",
    "global_light",
    // Phase 4a: the resamples and transforms, each the RGB kernel on the
    // L*a*b* codes (module doc).
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
    // Phase 4a-ii: Align and Distribute, the RGB kernels on the L*a*b* codes
    // (module doc).
    "align_set",
    "distribute_set",
    // Phase 4b-i: Layer via Copy and Cut and Stamp Selected, the RGB kernels
    // on the L*a*b* codes (module doc).
    "layer_via",
    "stamp_selected",
    // Phase 4c: Duplicate to New Document keeps the Lab policy
    // (`doc_duplicate_document`).
    "duplicate_to_new_document",
    // Phase 6b-i: Stack Modes and Reduce Noise from Stack, per channel on
    // the L*a*b* codes (`stack_stat`, module doc).
    "stack_mode",
    // Phase 6a-ii: links and layer comps (`lab_comps`).
    "link_layers",
    "unlink_layers",
    "layer_comp",
    // The close: Import Channel writes an alpha channel's samples, colour-free.
    "import_channel",
];

/// The blend modes a Lab document offers, Normal first, in the layer menu's
/// order: every mode offered at both 8 and 16 bits, measured against
/// Photoshop 26.10.0 by phase 2's take (`scripts/photoshop-probe/lab_mode_take.py`,
/// `lab-mode-547/take1`), which dropped Subtract and Divide. Darker Color,
/// Lighter Color and the four HSL modes joined in phase 3, when
/// `lab_blend_take.py`'s take2 confirmed an arm for each. Since 2f's second
/// part this list is also Edit ▸ Stroke…'s (`stroke_layer_edge`). Pass
/// Through is a group's, as in RGB. A Lab document composites them through
/// the one compositor's Lab arms (`lab_blend`, beside `blend.rs`) on the
/// L\*a\*b\* codes, each arm measured against Photoshop (`lab_blend`).
pub(crate) const LAB_BLEND_MODES: &[BlendMode] = &[
    BlendMode::Normal,
    BlendMode::Dissolve,
    BlendMode::Multiply,
    BlendMode::LinearBurn,
    BlendMode::DarkerColor,
    BlendMode::Screen,
    BlendMode::Addition,
    BlendMode::LighterColor,
    BlendMode::Overlay,
    BlendMode::SoftLight,
    BlendMode::HardLight,
    BlendMode::VividLight,
    BlendMode::LinearLight,
    BlendMode::PinLight,
    BlendMode::HardMix,
    BlendMode::Hue,
    BlendMode::Saturation,
    BlendMode::Color,
    BlendMode::Luminosity,
];

/// The blend modes a Lab document does not offer at 8 or 16 bits, in menu
/// order, measured against Photoshop 26.10.0 by phase 2's take: they refuse
/// for good ([`blend_refusal`]'s for-good sentence).
pub(crate) const LAB_NOT_OFFERED: &[BlendMode] = &[
    BlendMode::Darken,
    BlendMode::ColorBurn,
    BlendMode::Lighten,
    BlendMode::ColorDodge,
    BlendMode::Difference,
    BlendMode::Exclusion,
    BlendMode::Subtract,
    BlendMode::Divide,
];

/// `mode`'s refusal on a Lab document, or `None` for Pass Through (a
/// group's rule decides) and a mode of [`LAB_BLEND_MODES`]. A mode of
/// [`LAB_NOT_OFFERED`] refuses for good. Every other mode is in
/// the list; a mode in neither (none today) would refuse because Doctored
/// does not yet composite it in Lab.
pub(crate) fn blend_refusal(mode: BlendMode) -> Option<String> {
    if mode == BlendMode::PassThrough || LAB_BLEND_MODES.contains(&mode) {
        return None;
    }
    let name = crate::cmyk_document::mode_name(mode);
    Some(if LAB_NOT_OFFERED.contains(&mode) {
        format!("The {name} blend mode is unavailable in a Lab Color document.")
    } else {
        format!(
            "The {name} blend mode is unavailable in a Lab Color document; Doctored does not \
             yet composite it in Lab."
        )
    })
}

/// `value[key]` as a blend mode's refusal; an absent or unknown mode is left
/// to the edit's own parser (`gray_document`'s rule).
fn mode_refusal(value: &Value, key: &str) -> Option<String> {
    let raw = value[key].as_i64()?;
    let mode = BlendMode::from_c(i32::try_from(raw).ok()?)?;
    blend_refusal(mode)
}

/// The `luminance` source selection's refusal (phase 4b-i): a Lab
/// document's luminosity is its Lightness channel, the stored red codes, so
/// Load Selection reads it as the `color` red selection.
pub(crate) const LUMINANCE_REFUSAL: &str =
    "A Lab Color document's luminosity is its Lightness channel: select from color red (the L* codes).";

/// The one refusal sentence of the mode: an operation of [`NO_PROFILE_OPS`]
/// answers `"{operation} is unavailable in a Lab Color document, which has no
/// document profile."`; any other
/// `lab_adjust::rgb_only_sentence`'s `"{operation} is unavailable in a Lab
/// Color document: Doctored offers it on RGB documents only."`. The
/// selections' sentences are [`selection_refusal`]'s.
pub(crate) fn refused_sentence(operation: &str) -> String {
    if NO_PROFILE_OPS.contains(&operation) {
        return format!(
            "{operation} is unavailable in a Lab Color document, which has no document profile."
        );
    }
    crate::lab_adjust::rgb_only_sentence(operation)
}

/// A scalar read, statistic or edit of a target a Lab document has no plane
/// for (a layer's own pixels: `target` layer, or none): the targets it takes.
fn scalar_target_refusal(op: &str) -> String {
    format!(
        "{op} on a Lab Color document takes target mask, channel, red (Lightness), green (a), \
         blue (b) or alpha."
    )
}

/// The gate `ffi_precision_edit::request` asks first on a Lab document: `Ok`
/// for an edit of [`ADMITTED_EDITS`] or an admitted conditional edit — a
/// layer's blend mode, a Brush stroke, Fill or a selection's Stroke…
/// (`paint_blend`), a Paint Bucket (`bucket_fill`), a Gradient or a layer
/// edge's Stroke… (`stroke_layer_edge`, 2f's second part) in
/// [`LAB_BLEND_MODES`], the mode-less
/// `paint` and `erase_native` (phase 2a; every ink becomes Lab codes in
/// `precision_paint::prepare_colors`) and the legacy `erase`, `clone` and
/// `heal` (phase 6a-ii), the selected pixels of any target (phase 6a-ii
/// adds the planes), the retouch tools and Content-Aware
/// Fill (phase 2e: Dodge and Burn, the Blur, Sharpen, Smudge and Sponge
/// brushes, Healing, Spot Healing, Content-Aware Fill, and a Clone Stamp in
/// [`LAB_BLEND_MODES`]; Red Eye since 2f's third part), each of them on a colour
/// or alpha plane too (phase 2e), the `alpha`, `color`, `color_range`
/// (phases 4b-i to 6b-i), `quick` and `refine_edge` (phase 6b-ii) source
/// selections, `magic_wand` and `similar` wherever [`selection_refusal`]
/// admits them at the document's `depth` (#683), a layer style (phase 2e: its effects are
/// `LayerStyleEdit::target`'s, `lab_style`; patterns since 2f's second
/// part) and a layer's Blend If whose
/// channels are Lightness, a and b (phase 2f: `lab_style::blend_if_refusal`
/// names any other), the scalar
/// statistics and plane read of a layer mask, an alpha channel or the red,
/// green, blue and alpha planes (the L\*, a\* and b\* codes and alpha), and
/// the scalar edit of any of them unless `lab_filter::scalar_refusal`
/// answers (phase 2e for the planes: the mask rule); since the close also
/// `import_channel` and the legacy `rasterize`. A scalar op on any other
/// target answers `scalar_target_refusal`, a profile edit of
/// [`NO_PROFILE_OPS`] its sentence, and every other op (an unknown one, a
/// malformed style, an unknown selection kind) reaches its own parser, which
/// answers as on RGB.
pub(crate) fn admit_edit(text: &str, depth: LabDepth) -> Result<(), String> {
    let value: Value =
        serde_json::from_str(text).map_err(|e| format!("invalid high-depth edit request: {e}"))?;
    let op = value["op"].as_str().unwrap_or("");
    if ADMITTED_EDITS.contains(&op) {
        return Ok(());
    }
    let target = value["target"].as_str();
    let refusal = match op {
        "layer_blend" | "paint_blend" | "bucket_fill" | "gradient" | "retouch_clone"
        | "stroke_layer_edge" => mode_refusal(&value, "blend_mode"),
        // The legacy `rasterize` op's colours become Lab codes in
        // `precision_paint::prepare_colors`, as `paint`'s do.
        "paint" | "erase" | "clone" | "heal" | "erase_native" | "rasterize" => None,
        "dodge_burn" | "retouch_brush" | "retouch_heal" | "spot_heal" | "content_aware_fill"
        | "red_eye" => None,
        "layer_style" => {
            // Null is Clear Layer Style, admitted; a style that is not an
            // object is left to the style's own parser, as on RGB.
            let style = &value["style"];
            if style.is_object() {
                crate::lab_style::blend_if_refusal(&style["blend_if"])
            } else {
                None
            }
        }
        "layer_blend_if" => crate::lab_style::blend_if_refusal(&value["blend_if"]),
        // Phase 4b-i: the selected pixels of a layer, its mask or an alpha
        // channel; phase 6b-i: the Lightness, a, b and alpha planes' too.
        "move_selected" | "transform_selected" | "puppet_selected" | "perspective_selected" => None,
        // Phase 4b-i: the colour-free source selections; phase 4b-ii:
        // sampled Color Range (`lab_select`), phase 6b-i every other range
        // (`selection_tools`); phase 6b-ii Quick Selection on the codes'
        // L*a*b* and Select and Mask's edge stage on the codes; the wand,
        // Similar and Grow by `lab_select`'s rule where `SELECTION_REFUSALS`
        // admits them (#683).
        "selection" => match value["operation"]["kind"].as_str() {
            Some("alpha" | "color" | "color_range" | "quick" | "refine_edge") => None,
            Some("luminance") => Some(LUMINANCE_REFUSAL.to_string()),
            Some(kind @ ("magic_wand" | "similar")) => {
                let contiguous = value["operation"]["contiguous"].as_bool().unwrap_or(false);
                selection_key(kind, contiguous).and_then(|key| selection_refusal(key, depth))
            }
            // Any other kind is left to the selection's own parser.
            _ => None,
        },
        "scalar_edit" | "scalar_statistics" | "read_plane" => match target {
            Some("mask" | "channel" | "red" | "green" | "blue" | "alpha")
                if op == "scalar_edit" =>
            {
                crate::lab_filter::scalar_refusal(&value["operation"])
            }
            Some("mask" | "channel" | "red" | "green" | "blue" | "alpha") => None,
            Some("effective_mask") if op == "read_plane" => None,
            _ => Some(scalar_target_refusal(op)),
        },
        // The profile edits refuse for good; any other op reaches its own
        // parser, which answers as on RGB.
        _ if NO_PROFILE_OPS.contains(&op) => Some(refused_sentence(op)),
        _ => None,
    };
    refusal.map_or(Ok(()), Err)
}

/// The depth a Lab document declares.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LabDepth {
    Eight,
    Sixteen,
}

impl LabDepth {
    pub(crate) fn bits(self) -> u32 {
        match self {
            LabDepth::Eight => 8,
            LabDepth::Sixteen => 16,
        }
    }

    pub(crate) fn from_bits(bits: u32) -> Option<Self> {
        match bits {
            8 => Some(LabDepth::Eight),
            16 => Some(LabDepth::Sixteen),
            _ => None,
        }
    }
}

/// True when `profile` is the Lab twin by bytes (`icc_builtin::lab_twin_blob`):
/// only a Lab document holds it, so an adjustment parsed against it binds its
/// Lab form (`adjust_precision::PrecisionAdjustment::parse`).
pub(crate) fn is_lab_twin(profile: &IccProfile) -> bool {
    &profile.bytes()[..] == crate::icc_builtin::lab_twin_blob()
}

/// The Lab twin, parsed once and shared by every Lab document (module doc).
pub(crate) fn lab_twin() -> Arc<IccProfile> {
    static TWIN: OnceLock<Arc<IccProfile>> = OnceLock::new();
    TWIN.get_or_init(|| {
        // The blob is this crate's own constant and `lab_document` tests
        // parse it, so a failure here is a build defect, not an input.
        Arc::new(
            IccProfile::parse(crate::icc_builtin::lab_twin_blob())
                .expect("the built-in Lab twin parses"),
        )
    })
    .clone()
}

/// The Lab mode's policy (module doc).
#[derive(Clone)]
pub struct LabPolicy {
    depth: LabDepth,
    twin: Arc<IccProfile>,
}

impl LabPolicy {
    pub(crate) fn for_depth(depth: LabDepth) -> Self {
        Self {
            depth,
            twin: lab_twin(),
        }
    }

    pub(crate) fn depth(&self) -> LabDepth {
        self.depth
    }

    /// The Lab twin every layer of the document is in (module doc).
    pub(crate) fn twin(&self) -> &Arc<IccProfile> {
        &self.twin
    }

    /// One unit sample at the declared depth: the 8-bit document's ×257 grid
    /// at 8 bits, the nearest u16 code at 16 (`GrayPolicy::code`'s rule).
    pub(crate) fn code(&self, unit: f64) -> u16 {
        let unit = if unit.is_finite() { unit } else { 0.0 };
        let wide = (unit.clamp(0.0, 1.0) * 65535.0).round() as u32;
        match self.depth {
            LabDepth::Eight => ((wide + 128) / 257 * 257) as u16,
            LabDepth::Sixteen => wide as u16,
        }
    }

    pub(crate) fn record_resources(&self, inventory: &mut crate::resources::Inventory) {
        inventory.object(&*self.twin, "profile", false);
        self.twin.record_resources(inventory);
    }
}

impl PrecisionDocument {
    /// True for a Lab Color document.
    pub fn is_lab(&self) -> bool {
        self.lab.is_some()
    }

    /// The Lab policy; `None` for every other document.
    pub(crate) fn lab_policy(&self) -> Option<&Arc<LabPolicy>> {
        self.lab.as_ref()
    }

    /// The grid `lab_blend`'s arms composite this Lab document on;
    /// `None` for every other document.
    pub(crate) fn lab_grid(&self) -> Option<LabGrid> {
        self.lab.as_ref().map(|policy| match policy.depth() {
            LabDepth::Eight => LabGrid::Eight,
            LabDepth::Sixteen => LabGrid::Sixteen,
        })
    }

    /// TEST SEAM: this document as a Lab document at `bits` (8 or 16)
    /// under [`LabPolicy::for_depth`] — `with_lab`'s refusals, plus any other
    /// depth. Lets a black-box test build a Lab document from its own codes
    /// (`from_parts` in the Lab twin).
    #[doc(hidden)]
    pub fn with_lab_depth(self, bits: u32) -> Result<Self, String> {
        let depth =
            LabDepth::from_bits(bits).ok_or("A Lab Color document is 8 or 16 bits per channel.")?;
        self.with_lab(Arc::new(LabPolicy::for_depth(depth)))
    }

    /// This document as a Lab document under `policy`. Refuses a CMYK or
    /// Gray document, float storage (no 32-bit Lab), and a document whose
    /// profile is not the Lab twin (by bytes; every layer shares the
    /// document's profile handle, `build`'s rule). The policy adopts the
    /// document's own profile handle as its twin.
    pub(crate) fn with_lab(mut self, policy: Arc<LabPolicy>) -> Result<Self, String> {
        if self.is_cmyk() {
            return Err("a CMYK document cannot be a Lab Color document".into());
        }
        if self.is_gray() {
            return Err("a Grayscale document cannot be a Lab Color document".into());
        }
        if self.is_float() {
            return Err("a Lab Color document is 8 or 16 bits per channel".into());
        }
        if self.profile.bytes() != policy.twin().bytes() {
            return Err("a Lab Color document's layers must be in the Lab twin".into());
        }
        let policy = if Arc::ptr_eq(policy.twin(), &self.profile) {
            policy
        } else {
            Arc::new(LabPolicy {
                twin: self.profile.clone(),
                ..(*policy).clone()
            })
        };
        self.lab = Some(policy);
        Ok(self)
    }

    /// This document with no Lab policy: the RGB document its storage is.
    pub(crate) fn without_lab(&self) -> Self {
        let mut out = self.clone();
        out.lab = None;
        out
    }

    /// File ▸ New ▸ Lab Color: one layer of the sRGB fill converted by Mode ▸
    /// Lab Color's rule (`lab_convert`). Request `{"depth":8|16,"fill_srgb":
    /// [r,g,b,a] in 0…1,"layer_name","ppi"}`; an unknown key or any other
    /// depth refuses.
    pub fn new_lab(
        width: u32,
        height: u32,
        request: &str,
        limit: u64,
        other: u64,
    ) -> Result<Self, String> {
        let value: Value = serde_json::from_str(request)
            .map_err(|e| format!("new Lab Color document request is not valid JSON: {e}"))?;
        let object: &Map<String, Value> = value
            .as_object()
            .ok_or("new Lab Color document request must be a JSON object")?;
        for key in object.keys() {
            if !matches!(key.as_str(), "depth" | "fill_srgb" | "layer_name" | "ppi") {
                return Err(format!(
                    "new Lab Color document request has an unknown key \"{key}\""
                ));
            }
        }
        let depth = match object.get("depth").and_then(Value::as_u64) {
            Some(bits) => LabDepth::from_bits(bits as u32)
                .ok_or("A Lab Color document is 8 or 16 bits per channel.")?,
            None => return Err("new Lab Color document depth must be 8 or 16".into()),
        };
        let fill: Vec<f64> = object
            .get("fill_srgb")
            .and_then(Value::as_array)
            .map(|items| items.iter().filter_map(Value::as_f64).collect())
            .unwrap_or_default();
        if fill.len() != 4 || fill.iter().any(|v| !(0.0..=1.0).contains(v)) {
            return Err("new Lab Color document fill_srgb must be four numbers in 0…1".into());
        }
        let name = object
            .get("layer_name")
            .and_then(Value::as_str)
            .unwrap_or("Background");
        let ppi = object.get("ppi").and_then(Value::as_f64).unwrap_or(72.0) as f32;
        // The new document's display preparation the host runs right after,
        // charged as New Grayscale's 16-bit charge (`gray_io::open_scratch`
        // with no file): the same RGBA16 storage shape.
        pixel_count(width, height)?;
        let charge = crate::gray_io::open_scratch(width, height, 0)?;
        admit(0, charge, other, limit)?;
        let policy = LabPolicy::for_depth(depth);
        let pipeline = crate::lab_convert::to_lab_pipeline(&IccProfile::srgb())?;
        let lab = pipeline.color([fill[0] as f32, fill[1] as f32, fill[2] as f32]);
        let sample = [
            policy.code(f64::from(lab[0])),
            policy.code(f64::from(lab[1])),
            policy.code(f64::from(lab[2])),
            policy.code(fill[3]),
        ];
        let image = PrecisionImage::filled_rgba16(width, height, sample, policy.twin().clone())?;
        let document = PrecisionDocument::from_image_named(
            image,
            Metadata::default(),
            Resolution { x: ppi, y: ppi },
            name,
        )?;
        document.with_lab(Arc::new(policy))
    }
}
