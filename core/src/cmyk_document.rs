//! The CMYK document mode (#1 phases 1, 2, 4a and 4b, and the channels,
//! retouch, artwork and styles batches): the policy, the storage gate, the
//! blend-mode set and the refusal sentences.
//!
//! # What a CMYK document IS
//!
//! A `PrecisionDocument` whose `cmyk` field is `Some` (the `float` field's
//! pattern): every leaf is `Content::CmykRaster` — straight CMYKA16
//! (`cmyk_image`), ink 0 = none, 65535 = 100 % — and the policy carries the
//! CMYK ICC profile the inks are in (its BYTES kept verbatim, so a save
//! embeds exactly what was opened), the DECLARED depth (8 or 16: an 8-bit
//! file stores each code ×257 and writes `round(v / 257)`, exact for every
//! unedited sample) and the lazily built view link (`cmyk_color`).
//! `PrecisionDocument::profile` of a CMYK document is the built-in Display
//! P3: the space its VIEW projection is in, never its colour.
//!
//! The mode admits CMYK raster leaves, (phase 2) CMYK adjustment records
//! (`cmyk_adjust_layer`: Levels, Curves, Hue/Saturation and, since the
//! adjustments batch, the ops of `cmyk_adjust_ink` and `cmyk_adjust_rgb`)
//! and (phase 4a) Normal and Pass Through GROUPS of them, at any depth the
//! RGB document admits, each with an optional layer mask; clipping masks;
//! the blend modes of [`CMYK_BLEND_MODES`] on rasters, adjustment records
//! and isolated groups; and a LAYER STYLE on a raster or an isolated group —
//! Fill Opacity, every layer effect with its colours in ink (`cmyk_style`),
//! knockout and Blend Interior, persisted as DRDC 86. Not RGB records, Blend
//! If, placed sources or warp bindings (a CMYK warp is applied at once and
//! keeps none, phase 4b). Since the channels batch it holds alpha channels
//! (the RGB document's typed channels, colour-free) and vector masks on
//! rasters and groups, persisted as DRDC 85 (`cmyk_native`); since the
//! artwork batch a CMYK raster may carry a type, shape or fill description
//! (`cmyk_artwork`), persisted as DRDC 87.
//! [`PrecisionDocument::build_cmyk`] is the ONE place that is checked, and
//! `rebuilt` routes every structural edit of a CMYK document through it, so
//! an RGB operation that tries to leave anything else in a CMYK document is
//! refused there rather than producing it. `build` (the RGB body) refuses a
//! CMYK record. The tree composites through `cmyk_stack`.
//!
//! # The operation table
//!
//! `precision_capability::CMYK_FAMILIES` is the admitted set:
//!
//! | Family | Targets | Phase |
//! |---|---|---|
//! | layer duplicate, remove, move, name, visibility, opacity, label, lock | raster, adjustment, group | 1 (adjustment: 2, group: 4a) |
//! | flatten, render, sample, export, convert | document | 1 |
//! | layer add (New Layer) | document | 2 |
//! | paint (Brush, Fill, Gradient, Paint Bucket), erase | raster | 2 |
//! | fill_stroke | raster, mask | 2 |
//! | mask set | raster, adjustment, group | 2 (group: 4a) |
//! | mask remove, mask properties | mask | 2 |
//! | adjustment add, adjustment set (`cmyk_adjust_rgb::parse_cmyk`'s ops) | document | 2 (adjustments batch) |
//! | adjustment apply (the same, plus Shadows/Highlights and Replace Color) | raster | 2 (adjustments batch) |
//! | selected filters (`cmyk_filter`) and Fade (`cmyk_fade`), through their own exports | raster | adjustments batch |
//! | read plane | mask, effective mask | 2 |
//! | selection (`cmyk_select`: wand on ink, colour kinds on the view) | document, raster | selections |
//! | mask apply | mask | selections |
//! | layer group, ungroup | raster, adjustment, group / group | 4a |
//! | layer blend, clipped, merge | raster, group, adjustment | 4a |
//! | merge visible, stamp | document | 4a |
//! | layer style (effects, knockout, Blend Interior), global light | raster, group / document | styles |
//! | Blend If on Gray, Cyan, Magenta, Yellow, Black (`style_blend_if::source_weight_ink`) | raster, group | Blend If batch |
//! | profile (Assign/Convert to Profile, 8/16 Bits), proof, analysis (histogram, Auto, eyedroppers) | document | 5 |
//! | geometry resize, crop, straighten, rotate canvas, canvas size, rotate, flip | document | 4b |
//! | geometry affine, perspective, warp (applied at once, no binding: `cmyk_warp`) | raster, mask | 4b |
//! | geometry puppet, perspective warp | raster | 4b |
//! | geometry affine set, perspective set (Move, align, distribute) | raster, group, adjustment | 4b |
//! | mask transform (Transform Mask) | mask | 4b |
//! | selected pixels move, transform (`cmyk_selected_pixels`) | raster, mask | 4b |
//! | clipboard copy (raster, group), cut (raster), copy merged, paste (document) | see targets | 4c |
//! | layer via, layer stamp selected | raster / raster, group, adjustment | 4c |
//! | channel set, remove, read; read plane; fill_stroke | channel | channels |
//! | selection kinds alpha, luminance (sRGB), color (an ink plate), similar; backdrop source | document, raster | channels |
//! | selected pixels move, transform of an alpha channel or ONE ink plate (`cmyk_selected_plate`) | channel, cyan, magenta, yellow, black | channels |
//! | vector masks and paths (`precision_vector_mask`, no core refusal) | raster, group | channels |
//! | paint in every mode but Pass Through; Dodge/Burn (`cmyk_retouch_brush`) | raster | retouch batch |
//! | clone, heal (Clone Stamp, the healing family: `cmyk_retouch`) | raster | retouch batch |
//! | retouch brush (Blur, Sharpen, Sponge, Smudge, History Brush) | raster | retouch batch |
//! | selected Equalize (`equalize_apply`) | raster | retouch batch |
//! | type, shape and fill layers (`cmyk_artwork`, its own exports); Doctored | document | artwork batch |
//! | layer stack mode (Stack Modes per ink: `cmyk_stack_mode`) | raster | stacks |
//!
//! Every other family refuses through [`refused_sentence`]: "<operation> is
//! unavailable in a CMYK document" — a mode outside [`CMYK_BLEND_MODES`] as
//! "The <name> blend mode", a Blend If on Red, Green or Blue as "Blend If on
//! the <channel> channel"; Hue/Saturation runs on
//! the ink with K held, as measured in Photoshop
//! (`cmyk_adjust::hue_saturation_on_ink`). The host mirrors the
//! table in its
//! own lists (`CmykDocument` in the app: menu selectors, canvas tools and
//! MCP tools), asked before every menu item, tool and MCP handler; the core
//! refuses anything that slips past them.

use std::sync::{Arc, OnceLock};

use crate::blend::BlendMode;
use crate::cmyk_color::CmykViewLink;
use crate::cmyk_image::{CmykImage, CmykSample};
use crate::icc::IccProfile;
use crate::icc_intent::RenderingIntent;
use crate::icc_lut::{DeviceSpace, ProofProfile};
use crate::metadata::{Metadata, Resolution};
use crate::precision_document::{
    scalar_and_name, Content, PrecisionDocument, PrecisionDocumentLayer,
};
use crate::precision_mask::PrecisionChannel;
use crate::resources::Inventory;
use crate::style::LayerStyle;

/// The storage name this mode reports across the FFI and in descriptors.
pub(crate) const STORAGE_CMYK: &str = "cmyka16_unorm";

/// The one refusal sentence of the mode.
pub(crate) fn refused_sentence(operation: &str) -> String {
    format!("{operation} is unavailable in a CMYK document")
}

/// The depth a CMYK document declares (what its files are written at).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CmykDepth {
    Eight,
    Sixteen,
}

impl CmykDepth {
    pub fn bits(self) -> u32 {
        match self {
            CmykDepth::Eight => 8,
            CmykDepth::Sixteen => 16,
        }
    }

    pub fn from_bits(bits: u32) -> Option<Self> {
        match bits {
            8 => Some(CmykDepth::Eight),
            16 => Some(CmykDepth::Sixteen),
            _ => None,
        }
    }
}

/// The CMYK mode's policy: the profile (parsed and verbatim), the declared
/// depth and the view link, built once on first use.
pub struct CmykPolicy {
    pub(crate) profile: Arc<ProofProfile>,
    pub(crate) profile_bytes: Arc<[u8]>,
    pub(crate) depth: CmykDepth,
    view: OnceLock<Arc<CmykViewLink>>,
}

impl CmykPolicy {
    /// `Err` names why `bytes` cannot be a CMYK document's profile: not a
    /// profile, not CMYK, or no relative-colorimetric device-to-PCS table
    /// (the one its view and every conversion out of it read).
    pub fn new(bytes: &[u8], depth: CmykDepth) -> Result<Self, String> {
        if bytes.len() > crate::drdc::MAX_DRDC_BLOB_LEN as usize {
            return Err("the CMYK profile exceeds the native byte limit".into());
        }
        let profile =
            ProofProfile::parse(bytes).map_err(|e| format!("cannot use the CMYK profile: {e}"))?;
        if profile.device() != DeviceSpace::Cmyk {
            return Err(format!(
                "cannot use the CMYK profile: “{}” is not a CMYK profile",
                profile.name()
            ));
        }
        if profile
            .to_pcs(RenderingIntent::RelativeColorimetric)
            .is_none()
        {
            return Err(format!(
                "cannot use the CMYK profile: “{}” has no device-to-PCS table",
                profile.name()
            ));
        }
        Ok(Self {
            profile: Arc::new(profile),
            profile_bytes: Arc::from(bytes),
            depth,
            view: OnceLock::new(),
        })
    }

    pub fn name(&self) -> &str {
        self.profile.name()
    }

    pub fn depth(&self) -> CmykDepth {
        self.depth
    }

    pub fn profile_bytes(&self) -> &[u8] {
        &self.profile_bytes
    }

    /// The view link (`cmyk_color`), built on first use and then shared.
    pub(crate) fn view(&self) -> Result<Arc<CmykViewLink>, String> {
        if let Some(view) = self.view.get() {
            return Ok(view.clone());
        }
        let built = Arc::new(CmykViewLink::build(&self.profile)?);
        let _ = self.view.set(built);
        self.view
            .get()
            .cloned()
            .ok_or_else(|| "the CMYK view could not be built".to_string())
    }

    /// What building the view link costs (its node table), charged by every
    /// render plan before the first render builds it.
    pub(crate) fn view_bytes() -> u64 {
        crate::cmyk_color::CmykViewLink::planned_bytes()
    }

    pub(crate) fn record_resources(&self, inventory: &mut Inventory) {
        inventory.record(
            self.profile_bytes.as_ptr() as usize,
            self.profile_bytes.len(),
            "profile",
            false,
        );
        inventory.record(
            Arc::as_ptr(&self.profile) as usize,
            self.profile.bytes() as usize,
            "profile",
            false,
        );
        if let Some(view) = self.view.get() {
            inventory.record(view.nodes_ptr(), view.bytes() as usize, "cmyk_view", false);
        }
    }
}

/// The blend modes a CMYK document offers (phase 4a), Normal first, in menu
/// order — all 27. A mode outside it refuses by name ([`blend_refusal`]);
/// Pass Through is a group's, as in RGB.
pub(crate) const CMYK_BLEND_MODES: &[BlendMode] = &[
    BlendMode::Normal,
    BlendMode::Dissolve,
    BlendMode::Darken,
    BlendMode::Multiply,
    BlendMode::ColorBurn,
    BlendMode::LinearBurn,
    BlendMode::DarkerColor,
    BlendMode::Lighten,
    BlendMode::Screen,
    BlendMode::ColorDodge,
    BlendMode::Addition,
    BlendMode::LighterColor,
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
    BlendMode::Hue,
    BlendMode::Saturation,
    BlendMode::Color,
    BlendMode::Luminosity,
];

/// A blend mode's name as the Layers menu spells it.
pub(crate) fn mode_name(mode: BlendMode) -> &'static str {
    match mode {
        BlendMode::Normal => "Normal",
        BlendMode::Multiply => "Multiply",
        BlendMode::Screen => "Screen",
        BlendMode::Overlay => "Overlay",
        BlendMode::SoftLight => "Soft Light",
        BlendMode::HardLight => "Hard Light",
        BlendMode::Darken => "Darken",
        BlendMode::Lighten => "Lighten",
        BlendMode::Difference => "Difference",
        BlendMode::Exclusion => "Exclusion",
        BlendMode::ColorDodge => "Color Dodge",
        BlendMode::ColorBurn => "Color Burn",
        BlendMode::Addition => "Linear Dodge (Add)",
        BlendMode::Subtract => "Subtract",
        BlendMode::Dissolve => "Dissolve",
        BlendMode::LinearBurn => "Linear Burn",
        BlendMode::DarkerColor => "Darker Color",
        BlendMode::LighterColor => "Lighter Color",
        BlendMode::VividLight => "Vivid Light",
        BlendMode::LinearLight => "Linear Light",
        BlendMode::PinLight => "Pin Light",
        BlendMode::HardMix => "Hard Mix",
        BlendMode::Divide => "Divide",
        BlendMode::Hue => "Hue",
        BlendMode::Saturation => "Saturation",
        BlendMode::Color => "Color",
        BlendMode::Luminosity => "Luminosity",
        BlendMode::PassThrough => "Pass Through",
    }
}

/// `mode`'s refusal on a CMYK record, or `None` for a mode the mode offers
/// ([`CMYK_BLEND_MODES`]).
pub(crate) fn blend_refusal(mode: BlendMode) -> Option<String> {
    (!CMYK_BLEND_MODES.contains(&mode))
        .then(|| refused_sentence(&format!("The {} blend mode", mode_name(mode))))
}

/// What a layer style holds that a CMYK record cannot, by name: a Blend If
/// on Red, Green or Blue (`precision_blend_if::channel_refusal`). Every
/// effect (a Pattern Overlay's tile converts to ink, `cmyk_style::effect_inks`),
/// knockout, Blend Interior and a Blend If on Gray, Cyan, Magenta, Yellow or
/// Black are admitted.
pub(crate) fn style_refusal(style: &LayerStyle) -> Option<String> {
    style
        .blend_if
        .and_then(|b| crate::precision_blend_if::channel_refusal(b, true))
}

/// The first record (postorder) the mode cannot hold, as its refusal.
fn first_refusal(layers: &[PrecisionDocumentLayer]) -> Option<String> {
    for layer in layers {
        if let Some(children) = layer.children() {
            if let Some(reason) = first_refusal(children) {
                return Some(reason);
            }
        }
        let what = match &layer.content {
            Content::CmykRaster { .. }
            | Content::CmykAdjustment(_)
            | Content::Group(_)
            | Content::PassThrough(_) => None,
            Content::Raster { .. } => Some("An RGB layer"),
            Content::Adjustment(_) => Some("An RGB adjustment layer"),
        }
        .or(layer.retained_source.as_ref().map(|_| "A placed image"))
        .or(layer.warp.as_ref().map(|_| "A warp"));
        if let Some(what) = what {
            return Some(refused_sentence(what));
        }
        if let Some(reason) = layer
            .blend_if
            .and_then(|b| crate::precision_blend_if::channel_refusal(b, true))
        {
            return Some(reason);
        }
        if let Some(reason) = layer.style.as_deref().and_then(style_refusal) {
            return Some(reason);
        }
        if let Some(reason) = blend_refusal(layer.blend_mode) {
            return Some(reason);
        }
    }
    None
}

impl PrecisionDocumentLayer {
    /// The ONE CMYK leaf constructor.
    pub fn raster_cmyk(
        name: &str,
        image: CmykImage,
        offset: (i32, i32),
        opacity: f32,
        visible: bool,
    ) -> Result<Self, String> {
        scalar_and_name(name, opacity)?;
        Ok(Self {
            id: crate::doc_identity::mint_layer_id(),
            retained_source: None,
            warp: None,
            generated: None,
            description: None,
            name: Arc::new(name.into()),
            opacity,
            blend_mode: crate::blend::BlendMode::Normal,
            visible,
            content: Content::CmykRaster { image, offset },
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

    /// A CMYK leaf's image; `None` for every other record.
    pub fn cmyk_source(&self) -> Option<&CmykImage> {
        match &self.content {
            Content::CmykRaster { image, .. } => Some(image),
            _ => None,
        }
    }
}

impl PrecisionDocument {
    /// The CMYK document constructor and the mode's storage gate (module
    /// doc): CMYK raster leaves, CMYK adjustment records and groups of them,
    /// each refused BY NAME where it holds anything the mode cannot.
    /// Alpha channels (the channels batch) are the RGB document's typed
    /// channels unchanged: canvas-sized u16 planes, colour-free.
    pub(crate) fn build_cmyk(
        width: u32,
        height: u32,
        metadata: Metadata,
        resolution: Resolution,
        layers: Vec<PrecisionDocumentLayer>,
        channels: Vec<PrecisionChannel>,
        policy: Arc<CmykPolicy>,
    ) -> Result<Self, String> {
        if let Some(reason) = first_refusal(&layers) {
            return Err(reason);
        }
        Self::build_body(
            width,
            height,
            IccProfile::display_p3(),
            metadata,
            resolution,
            layers,
            channels,
            None,
            Some(policy),
        )
    }

    /// True for a CMYK document.
    pub fn is_cmyk(&self) -> bool {
        self.cmyk.is_some()
    }

    /// The CMYK policy; `None` for an RGB document.
    pub fn cmyk_policy(&self) -> Option<&Arc<CmykPolicy>> {
        self.cmyk.as_ref()
    }

    /// The policy, or `operation`'s refusal on an RGB document.
    pub(crate) fn require_cmyk_policy(&self, operation: &str) -> Result<&Arc<CmykPolicy>, String> {
        self.cmyk
            .as_ref()
            .ok_or_else(|| format!("{operation} requires a CMYK document"))
    }

    /// Record `index`'s stored samples, row-major. A test seam.
    #[doc(hidden)]
    pub fn cmyk_layer_samples(&self, index: usize) -> Option<Vec<CmykSample>> {
        self.layer(index)?.cmyk_source()?.to_contiguous().ok()
    }
}
