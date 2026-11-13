//! The bounded 7B neutral document is a distinct immutable u16 type. Its
//! encoded-RGB evaluation keeps f32 through qualified isolated blend groups, using the
//! precision_render kernel. No legacy byte operation accepts this handle.
//! Unsupported styles have no
//! representable record; native readers refuse unknown feature/evaluation tags.
//!
//! Isolated Normal and pass-through groups share the float renderer.

use std::sync::Arc;

use serde_json::{json, Value};

use crate::drdc::{MAX_DRDC_BLOB_LEN, MAX_DRDC_NAME_LEN};
use crate::icc::IccProfile;
use crate::job_control::{DrJobControl, JobContext};
use crate::metadata::{Metadata, Resolution};
use crate::precision_adjustment::PrecisionAdjustmentState;
use crate::precision_image::{
    admit, pixel_count, PrecisionImage, PrecisionStorage, BOOKKEEPING_BYTES,
};
use crate::precision_mask::{PrecisionChannel, PrecisionMask};
use crate::precision_render::{
    PrecisionLayer, PrecisionRenderPlan, PrecisionScene, MAX_GROUP_DEPTH, MAX_RECORDS,
};
use crate::resources::Inventory;
use crate::style_composite::Cut;

pub const EVALUATION: &str = "encoded_rgb_f32_v1";

#[derive(Clone)]
pub(crate) enum Content {
    Raster {
        image: PrecisionImage,
        offset: (i32, i32),
    },
    Adjustment(PrecisionAdjustmentState),
    Group(Arc<Vec<PrecisionDocumentLayer>>),
    PassThrough(Arc<Vec<PrecisionDocumentLayer>>),
    /// A CMYK document's leaf (`cmyk_document`): straight CMYKA16 ink. Only
    /// `build_cmyk` admits one; every RGB path meeting it refuses through
    /// `cmyk_document::refused_sentence` rather than treating it as empty.
    CmykRaster {
        image: crate::cmyk_image::CmykImage,
        offset: (i32, i32),
    },
    /// A CMYK document's Levels or Curves record (`cmyk_adjust_layer`,
    /// #1 phase 2). Like `CmykRaster`, only `build_cmyk` admits one and an
    /// RGB path meeting it refuses by `cmyk_document::refused_sentence`.
    CmykAdjustment(crate::cmyk_adjust_layer::CmykAdjustmentState),
}

#[derive(Clone)]
pub struct PrecisionDocumentLayer {
    /// Stable identity, unique among every layer this process has minted —
    /// `Layer::id`'s twin at high depth, minted from the same counter so an
    /// id can never mean two layers in one process. Never 0, and PERSISTED
    /// in the `LIDS` table since 12E-1 (`identity_native`), so a reopen hands
    /// back the id the record had.
    pub(crate) id: u64,
    pub(crate) description: Option<Arc<String>>,
    pub(crate) retained_source: Option<crate::retained_source::RetainedSource16>,
    pub(crate) warp: Option<crate::warp::WarpBinding16>,
    /// A Generative Layer's variations (`doc_gen_variations`).
    pub(crate) generated: Option<crate::doc_gen_variations::GeneratedBinding16>,
    pub(crate) name: Arc<String>,
    pub(crate) opacity: f32,
    pub(crate) blend_mode: crate::blend::BlendMode,
    pub(crate) visible: bool,
    pub(crate) content: Content,
    pub(crate) records: usize,
    pub(crate) depth: usize,
    pub(crate) mask: Option<PrecisionMask>,
    /// A clipping-mask member: confined to the alpha footprint of the first
    /// unclipped entry below it in its level (the byte `Layer::clipped` rule).
    pub(crate) clipped: bool,
    /// Blending Options > Blend If on a raster layer or a group
    /// (`precision_blend_if`); never an identity, never on an adjustment. A
    /// Pass Through group carrying it composites isolated (`isolates`).
    pub(crate) blend_if: Option<crate::style::BlendIf>,
    /// Fill opacity and the nine effects on a raster layer or a group
    /// (`precision_style`); its `blend_if` is always `None` (the field above
    /// holds it), never an identity, and its padded plane is within
    /// `MAX_PIXELS`. A Pass Through group carrying it composites isolated.
    pub(crate) style: Option<Arc<crate::style::LayerStyle>>,
    /// Colour LABEL, 0..=7 — the byte `Layer::label` twin, panel bookkeeping
    /// only and persisted in the `LLBL` table (`doc_label`).
    pub(crate) label: u8,
    /// The four LOCK bits, `doc_lock::LOCK_*` verbatim — the byte
    /// `Layer::locks` twin (`precision_lock`). Masked to
    /// `doc_lock::LOCK_ALL` wherever a value enters, and persisted in the
    /// `LLNK` table (`precision_lock_native`).
    pub(crate) locks: u32,
    /// The LINK-GROUP id, 0 for unlinked — the byte `Layer::link` twin
    /// (`doc_align`'s rules, re-expressed over the flat record list in
    /// `precision_lock`). Persisted beside `locks` in `LLNK`.
    pub(crate) link: u32,
    /// The group DISCLOSURE, the byte `Layer::open` twin: whether the Layers
    /// panel shows a group's children. Panel bookkeeping only, meaningful on
    /// groups alone (every other record keeps `true`), and persisted in the
    /// conditional `LOPN` table (`precision_group_open_native`).
    pub(crate) open: bool,
    /// Photoshop's `fxrp`, the byte `Layer::reference` twin: (0, 0) at
    /// creation, moved by translations and mapped by Image Size, Canvas
    /// Size, Crop, the angled crop, turns, flips, a filtered Rotate Canvas
    /// and Free Transform (`precision_geometry`'s `reference_map`;
    /// `reference_point`; `doc_reference::follow_record`), and
    /// persisted in the `LREF` table (`reference_native`).
    pub(crate) reference: (i32, i32),
}

pub(crate) fn scalar_and_name(name: &str, opacity: f32) -> Result<(), String> {
    if name.len() > MAX_DRDC_NAME_LEN as usize {
        return Err("high-depth layer name exceeds the native limit".into());
    }
    if !opacity.is_finite() || !(0.0..=1.0).contains(&opacity) {
        return Err("high-depth layer opacity must be finite and in [0,1]".into());
    }
    Ok(())
}

pub(crate) fn shape(layers: &[PrecisionDocumentLayer]) -> Result<(usize, usize), String> {
    if layers.len() > MAX_RECORDS {
        return Err("too many high-depth document records".into());
    }
    let mut count = 0;
    let mut depth = 0;
    for layer in layers {
        count += layer.records;
        depth = depth.max(layer.depth);
        if count > MAX_RECORDS || depth > MAX_GROUP_DEPTH {
            return Err("high-depth document exceeds its record or group-depth limit".into());
        }
    }
    Ok((count, depth))
}

impl PrecisionDocumentLayer {
    pub fn raster(
        name: &str,
        image: PrecisionImage,
        offset: (i32, i32),
        opacity: f32,
        visible: bool,
    ) -> Result<Self, String> {
        scalar_and_name(name, opacity)?;
        if image.storage() != PrecisionStorage::Rgba16Unorm {
            return Err("high-depth documents require explicit RGBA16 UNORM sources".into());
        }
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
    pub fn isolated_normal(
        name: &str,
        children: Vec<Self>,
        opacity: f32,
        visible: bool,
    ) -> Result<Self, String> {
        Self::group(name, children, opacity, visible, false)
    }
    pub fn pass_through(
        name: &str,
        children: Vec<Self>,
        opacity: f32,
        visible: bool,
    ) -> Result<Self, String> {
        Self::group(name, children, opacity, visible, true)
    }
    pub(crate) fn group(
        name: &str,
        children: Vec<Self>,
        opacity: f32,
        visible: bool,
        pass_through: bool,
    ) -> Result<Self, String> {
        scalar_and_name(name, opacity)?;
        let (records, depth) = shape(&children)?;
        if records >= MAX_RECORDS || depth >= MAX_GROUP_DEPTH {
            return Err("high-depth document exceeds its record or group-depth limit".into());
        }
        Ok(Self {
            // The CONTAINER mints; its children keep their own ids.
            id: crate::doc_identity::mint_layer_id(),
            retained_source: None,
            warp: None,
            generated: None,
            description: None,
            name: Arc::new(name.into()),
            opacity,
            blend_mode: crate::blend::BlendMode::Normal,
            visible,
            content: if pass_through {
                Content::PassThrough(Arc::new(children))
            } else {
                Content::Group(Arc::new(children))
            },
            records: records + 1,
            depth: depth + 1,
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
    pub fn mask(&self) -> Option<&PrecisionMask> {
        self.mask.as_ref()
    }
    pub fn with_mask(mut self, mask: Option<PrecisionMask>) -> Self {
        self.mask = mask;
        self
    }
    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn opacity(&self) -> f32 {
        self.opacity
    }
    pub fn visible(&self) -> bool {
        self.visible
    }
    pub fn clipped(&self) -> bool {
        self.clipped
    }
    pub fn with_clipped(mut self, clipped: bool) -> Self {
        self.clipped = clipped;
        self
    }
    /// Colour label, 0..=7 (`doc_label`).
    pub fn label(&self) -> u8 {
        self.label
    }
    /// Carries a colour label; a code above the maximum is clamped off to 0
    /// by the one caller that can produce one (the strict `layer_label` edit
    /// refuses it first).
    pub fn with_label(mut self, label: u8) -> Self {
        self.label = if label > crate::doc_label::MAX_LABEL {
            0
        } else {
            label
        };
        self
    }
    /// The reference point (`Layer::reference`'s twin).
    pub fn reference(&self) -> (i32, i32) {
        self.reference
    }
    /// Carries a reference point (a reader's, an import's, a carry).
    pub fn with_reference(mut self, reference: (i32, i32)) -> Self {
        self.reference = reference;
        self
    }
    /// Carries lock bits, reserved bits masked off (`doc_lock::LOCK_ALL`).
    pub fn with_locks(mut self, locks: u32) -> Self {
        self.locks = locks & crate::doc_lock::LOCK_ALL;
        self
    }
    /// The four LOCK bits (`doc_lock::LOCK_*`), the byte `Layer::locks`
    /// twin — what `precision_lock` enforces and `LLNK` persists.
    pub fn locks(&self) -> u32 {
        self.locks
    }
    /// The LINK-GROUP id, 0 for unlinked — the byte `Layer::link` twin.
    pub fn link(&self) -> u32 {
        self.link
    }
    /// The group disclosure, the byte `Layer::open` twin; always `true` on a
    /// non-group record.
    pub fn open(&self) -> bool {
        self.open
    }
    /// Carries a group disclosure; no effect on a non-group record, so a
    /// non-group row keeps answering `true` (byte parity).
    pub(crate) fn with_open(mut self, open: bool) -> Self {
        if self.children().is_some() {
            self.open = open;
        }
        self
    }
    pub fn source(&self) -> Option<&PrecisionImage> {
        match &self.content {
            Content::Raster { image, .. } => Some(image),
            _ => None,
        }
    }
    pub fn children(&self) -> Option<&[Self]> {
        match &self.content {
            Content::Group(children) | Content::PassThrough(children) => Some(children),
            _ => None,
        }
    }
    pub fn offset(&self) -> (i32, i32) {
        match self.content {
            Content::Raster { offset, .. } | Content::CmykRaster { offset, .. } => offset,
            _ => (0, 0),
        }
    }
    /// A record holding pixels geometry resamples: an RGB raster or a CMYK
    /// document's CMYKA16 raster (#1 phase 4b).
    pub(crate) fn is_pixel_leaf(&self) -> bool {
        matches!(
            self.content,
            Content::Raster { .. } | Content::CmykRaster { .. }
        )
    }
    pub fn is_pass_through(&self) -> bool {
        matches!(self.content, Content::PassThrough(_))
    }
    /// Composites isolated wherever it sits: a `Content::Group`, or a `Content::PassThrough`
    /// carrying a style or Blend If (take1 P1/P3). A Pass Through CLIP BASE
    /// isolates by its level (`render_layers`' `isolated`), not here.
    pub(crate) fn isolates(&self) -> bool {
        match self.content {
            Content::Group(_) => true,
            Content::PassThrough(_) => self.style.is_some() || self.blend_if.is_some(),
            _ => false,
        }
    }

    /// The caller has already decided this node contributes. `start` is the
    /// postorder index of its first record, so a cut reaches nested targets.
    /// `isolated` converts a Pass Through group as an isolated Normal group:
    /// the byte `is_isolated` rule for a clip base with a visible member or a
    /// clipped member with a base (`doc_group.rs`).
    fn render_layer(
        &self,
        start: usize,
        cut: Cut<'_>,
        isolated: bool,
        probe: Option<usize>,
        float: bool,
    ) -> Result<PrecisionLayer, String> {
        // The byte opacity below 32 bits, measured against Photoshop
        // (`opacity_byte`, #2).
        let opacity = crate::opacity_byte::composite(self.opacity, float);
        let layer = match &self.content {
            Content::Raster { image, offset } => {
                PrecisionLayer::raster(image.clone(), *offset, opacity)?
            }
            Content::Adjustment(state) => PrecisionLayer::adjustment(state.clone(), opacity)?,
            Content::Group(children) => PrecisionLayer::isolated_normal(
                render_layers(children, start, cut, probe, float)?,
                opacity,
            )?,
            Content::PassThrough(children) if isolated || self.isolates() => {
                PrecisionLayer::isolated_normal(
                    render_layers(children, start, cut, probe, float)?,
                    opacity,
                )?
            }
            Content::PassThrough(children) => PrecisionLayer::pass_through(
                render_layers(children, start, cut, probe, float)?,
                opacity,
            )?,
            Content::CmykRaster { .. } | Content::CmykAdjustment(_) => {
                return Err(crate::cmyk_document::refused_sentence("RGB compositing"))
            }
        };
        Ok(layer
            .with_blend_mode(self.blend_mode)?
            .with_blend_if(self.blend_if)?
            .with_style(self.style.clone())?
            // The capture PROBE (`precision_style_knockout`), a property of
            // the record rather than of this composite.
            .with_knockout_probe(probe == Some(start + self.records - 1))
            .with_mask(self.mask.clone()))
    }
}

/// One level of the document as render layers, bottom-first, with its clip
/// units derived from `clipped` ALONE exactly as `doc_group::composite_level_into`
/// derives them: a clipped entry with no unclipped entry below it in the level
/// is baseless and converts as if unclipped; an unclipped entry is a base and
/// the consecutive clipped run above it are its members.
///
/// `start` is the postorder index of `layers[0]`'s first record; each node's
/// index is `start + records so far + node.records - 1`. `cut` gates
/// CONTRIBUTION only (`visible && !cut.excludes(index)`): a non-contributing
/// base drops its whole unit, non-contributing members are dropped. Isolation
/// of a Pass Through base reads the DOCUMENT's `visible` flags, never the cut,
/// so a sampling plate never moves pixels below its target.
fn render_layers(
    layers: &[PrecisionDocumentLayer],
    start: usize,
    cut: Cut<'_>,
    probe: Option<usize>,
    float: bool,
) -> Result<Vec<PrecisionLayer>, String> {
    let mut starts = Vec::with_capacity(layers.len());
    let mut offset = start;
    for layer in layers {
        starts.push(offset);
        offset += layer.records;
    }
    let contributes =
        |i: usize| layers[i].visible && !cut.excludes(starts[i] + layers[i].records - 1);
    let mut out = Vec::with_capacity(layers.len());
    let mut i = 0;
    while i < layers.len() {
        let entry = &layers[i];
        if entry.clipped {
            // Baseless: only reachable at the bottom of a level.
            if contributes(i) {
                out.push(entry.render_layer(starts[i], cut, false, probe, float)?);
            }
            i += 1;
            continue;
        }
        let mut end = i + 1;
        while end < layers.len() && layers[end].clipped {
            end += 1;
        }
        if contributes(i) {
            let isolated = layers[i + 1..end].iter().any(|member| member.visible);
            // A MEMBER is handed `true` only to force a Pass Through group
            // into isolation, and has no clip run of its own.
            let base = entry.render_layer(starts[i], cut, isolated, probe, float)?;
            let mut members = Vec::new();
            for j in i + 1..end {
                if contributes(j) {
                    members.push(layers[j].render_layer(starts[j], cut, true, probe, float)?);
                }
            }
            out.push(base.with_clipped_members(members)?);
        }
        i = end;
    }
    Ok(out)
}

#[derive(Clone)]
pub struct PrecisionDocument {
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) profile: Arc<IccProfile>,
    pub(crate) metadata: Metadata,
    pub(crate) resolution: Resolution,
    pub(crate) layers: Arc<Vec<PrecisionDocumentLayer>>,
    pub(crate) records: usize,
    pub(crate) channels: Arc<Vec<PrecisionChannel>>,
    pub(crate) import_warnings: Arc<Vec<String>>,
    /// The document's EVALUATION POLICY as tables (`precision_evaluation`):
    /// `None` is `encoded` — the 7B default and still every document's — and
    /// `Some` is `linear_light`. Built once and `Arc`-shared by every derived
    /// handle; its bytes are recorded by `record_resources`, so admission sees
    /// them.
    pub(crate) linear_light: Option<Arc<crate::precision_evaluation::TypedTransfer>>,
    /// The 32-bit FLOAT mode's policy (`float_document`): `None` is the u16
    /// document — every document before Phase 11F and still the default —
    /// and `Some` is an extended-range linear-light float document. Its
    /// presence is the ONE storage question `build` gates on, and
    /// `evaluation()` answers `LinearLight` from it alone, so a float
    /// document always has `linear_light: None` and never allocates a
    /// `TypedTransfer`.
    pub(crate) float: Option<Arc<crate::float_document::FloatPolicy>>,
    /// The CMYK mode's policy (`cmyk_document`): `None` for every RGB
    /// document, `Some` for a CMYK document, whose leaves are all
    /// `Content::CmykRaster`. Like `float`, its presence is a storage
    /// question `build_body` gates on.
    pub(crate) cmyk: Option<Arc<crate::cmyk_document::CmykPolicy>>,
    /// The Grayscale mode's policy (`gray_document`, #1): `None` for every
    /// other document. Its storage is the RGB document's (R = G = B in the
    /// policy's twin profile), so no `build` gate reads it: `build_body`
    /// sets `None`, `rebuilt` carries it and `with_gray` sets it.
    pub(crate) gray: Option<Arc<crate::gray_document::GrayPolicy>>,
    /// The Lab Color mode's policy (`lab_document`, #1): `None` for every
    /// other document. Its storage is the RGB document's (L\*, a\*, b\* in
    /// R, G, B, in the Lab twin), so, like `gray`, no `build` gate reads it:
    /// `build_body` sets `None`, `rebuilt` carries it and `with_lab` sets it.
    pub(crate) lab: Option<Arc<crate::lab_document::LabPolicy>>,
    /// The Indexed Color mode's policy (`indexed_document`, #1): `None` for
    /// every other document. Its storage is the RGB document of one layer
    /// holding the policy's render, so, like `gray`, no `build` gate reads
    /// it: `build_body` sets `None`, `rebuilt` carries it and `with_indexed`
    /// sets it.
    pub(crate) indexed: Option<Arc<crate::indexed_document::IndexedPolicy>>,
    /// The Bitmap mode's policy (`bitmap_document`, #1): `None` for every
    /// other document. Its storage is the 8-bit Gray document of one layer
    /// holding the packed plane's render (`gray` is `Some` beside it), so,
    /// like `gray`, no `build` gate reads it: `build_body` sets `None`,
    /// `rebuilt` carries it and `with_bitmap` sets it.
    pub(crate) bitmap: Option<Arc<crate::bitmap_document::BitmapPolicy>>,
    /// The document's LAYER COMPS (`precision_comps`, whose rules are
    /// `doc_comps`'s). Named per-layer visibility/position/appearance by
    /// stable identity, never pixels; a freshly BUILT document has none and
    /// a REBUILT one carries the list it came from.
    pub(crate) comps: Vec<crate::doc_comps::LayerComp>,
    /// The document's GUIDES and RULER ORIGIN (`precision_guide`, whose rules
    /// are `doc_guide`'s — the byte twin's two fields, `DrDocument.guides` and
    /// `DrDocument.ruler_origin`, carried verbatim). Canvas chrome that is
    /// genuinely document state: it survives save, reopen and undo, and every
    /// canvas-geometry op moves it the way it moves the pixels
    /// (`precision_geometry`). A freshly BUILT document has neither; a
    /// REBUILT one carries both.
    pub(crate) guides: Vec<crate::doc_guide::Guide>,
    /// The canvas coordinate of the ruler's zero point, `(0, 0)` — the
    /// canvas's top-left — on a freshly built document. There is exactly one
    /// and a document is never without it, which is why every canvas policy
    /// CLAMPS it where it drops a guide (`doc_guide`).
    pub(crate) ruler_origin: (f64, f64),
    /// The document's COLOUR SAMPLERS (`precision_sampler`, whose rules are
    /// `doc_sampler`'s — the byte twin's `DrDocument.color_samplers`). A
    /// freshly BUILT document has none; a REBUILT one carries them, and the
    /// geometry fold moves them.
    pub(crate) color_samplers: Vec<crate::doc_sampler::ColorSampler>,
    /// The document's PATHS (`precision_path`, whose rules are `doc_path`'s —
    /// the byte twin's `DrDocument.paths`). A freshly BUILT document has
    /// none; a REBUILT one carries them, and the geometry fold moves them.
    pub(crate) paths: crate::precision_path::TypedPaths,
    /// The document's GLOBAL LIGHT (`precision_global_light`) — the byte
    /// twin's `DrDocument.global_light`, carried verbatim. Every style effect
    /// with `use_global_light` reads it through the scene
    /// (`PrecisionScene::with_light`). A freshly BUILT document has the
    /// default 120°/30°; a REBUILT one carries the light it came from. A
    /// float document always holds the default (it refuses the edit).
    pub(crate) global_light: crate::style::GlobalLight,
    /// The BACKGROUND MARK (`doc_background`, the byte twin's
    /// `DrDocument.background`). A freshly BUILT document has none; a
    /// REBUILT one carries it.
    pub(crate) background: Option<crate::doc_background::BackgroundMark>,
}

impl PrecisionDocument {
    pub fn from_image(
        image: PrecisionImage,
        metadata: Metadata,
        resolution: Resolution,
    ) -> Result<Self, String> {
        Self::from_image_named(image, metadata, resolution, "Background")
    }

    /// `from_image` with the one raster layer called `name` instead of
    /// "Background", so a host can name the layer when it builds the document
    /// rather than through a later rename edit.
    pub fn from_image_named(
        image: PrecisionImage,
        metadata: Metadata,
        resolution: Resolution,
        name: &str,
    ) -> Result<Self, String> {
        let (width, height) = image.dimensions();
        let profile = image.profile().clone();
        Self::from_parts(
            width,
            height,
            profile,
            metadata,
            resolution,
            vec![PrecisionDocumentLayer::raster(
                name,
                image,
                (0, 0),
                1.0,
                true,
            )?],
        )
    }

    pub fn from_parts(
        width: u32,
        height: u32,
        profile: Arc<IccProfile>,
        metadata: Metadata,
        resolution: Resolution,
        layers: Vec<PrecisionDocumentLayer>,
    ) -> Result<Self, String> {
        Self::from_parts_and_channels(
            width,
            height,
            profile,
            metadata,
            resolution,
            layers,
            Vec::new(),
        )
    }
    pub fn from_parts_and_channels(
        width: u32,
        height: u32,
        profile: Arc<IccProfile>,
        metadata: Metadata,
        resolution: Resolution,
        layers: Vec<PrecisionDocumentLayer>,
        channels: Vec<PrecisionChannel>,
    ) -> Result<Self, String> {
        Self::build(
            width, height, profile, metadata, resolution, layers, channels, None,
        )
    }

    /// The ONE construction body, and the ONE storage-admission gate: every
    /// raster leaf's `storage()` must agree with `float.is_some()`. A u16
    /// document passes through it exactly as it did before Phase 11F — the
    /// gate refuses only what `PrecisionDocumentLayer::raster` already
    /// refused — and a float document is the only way an F32 leaf is ever
    /// admitted.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn build(
        width: u32,
        height: u32,
        profile: Arc<IccProfile>,
        metadata: Metadata,
        resolution: Resolution,
        layers: Vec<PrecisionDocumentLayer>,
        channels: Vec<PrecisionChannel>,
        float: Option<Arc<crate::float_document::FloatPolicy>>,
    ) -> Result<Self, String> {
        Self::build_body(
            width, height, profile, metadata, resolution, layers, channels, float, None,
        )
    }

    /// `build`'s body with the CMYK policy: a CMYK leaf or CMYK adjustment
    /// record is admitted exactly when `cmyk` is `Some`, and then no other
    /// record is (`build_cmyk`, the only caller passing one, checks the rest
    /// of the mode's shape first).
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn build_body(
        width: u32,
        height: u32,
        profile: Arc<IccProfile>,
        metadata: Metadata,
        resolution: Resolution,
        layers: Vec<PrecisionDocumentLayer>,
        channels: Vec<PrecisionChannel>,
        float: Option<Arc<crate::float_document::FloatPolicy>>,
        cmyk: Option<Arc<crate::cmyk_document::CmykPolicy>>,
    ) -> Result<Self, String> {
        pixel_count(width, height)?;
        let (records, _) = shape(&layers)?;
        if records == 0 {
            return Err("high-depth document must contain at least one record".into());
        }
        if profile.bytes().len() > MAX_DRDC_BLOB_LEN as usize {
            return Err("high-depth profile exceeds the native byte limit".into());
        }
        for packet in [&metadata.exif, &metadata.xmp, &metadata.iptc]
            .into_iter()
            .flatten()
        {
            if packet.len() > MAX_DRDC_BLOB_LEN as usize {
                return Err("high-depth metadata packet exceeds the native limit".into());
            }
        }
        fn profiles(layers: &[PrecisionDocumentLayer], profile: &Arc<IccProfile>) -> bool {
            layers.iter().all(|layer| match &layer.content {
                Content::Raster { image, .. } => Arc::ptr_eq(image.profile(), profile),
                Content::Adjustment(state) => Arc::ptr_eq(state.profile(), profile),
                Content::Group(children) | Content::PassThrough(children) => {
                    profiles(children, profile)
                }
                // Ink carries the policy's CMYK profile, not this one.
                Content::CmykRaster { .. } | Content::CmykAdjustment(_) => true,
            })
        }
        if !profiles(&layers, &profile) {
            return Err("high-depth document requires one captured profile handle".into());
        }
        fn storage_agrees(layers: &[PrecisionDocumentLayer], float: bool, cmyk: bool) -> bool {
            layers.iter().all(|layer| match &layer.content {
                Content::Raster { image, .. } => {
                    !cmyk && (image.storage() == PrecisionStorage::Rgba32Float) == float
                }
                Content::Adjustment(_) => !cmyk,
                // A CMYK group (#1 phase 4a) agrees when its children do.
                Content::Group(children) | Content::PassThrough(children) => {
                    storage_agrees(children, float, cmyk)
                }
                Content::CmykRaster { .. } | Content::CmykAdjustment(_) => cmyk,
            })
        }
        if (float.is_some() && cmyk.is_some())
            || !storage_agrees(&layers, float.is_some(), cmyk.is_some())
        {
            return Err("high-depth document storage does not match the document mode".into());
        }
        if channels.len() > crate::doc_channel::MAX_CHANNELS
            || channels
                .iter()
                .any(|channel| channel.plane.dimensions() != (width, height))
        {
            return Err(
                "typed channels must be canvas-sized and within the channel count limit".into(),
            );
        }
        let mut layers = layers;
        crate::retained_source::clean_typed_layers(&mut layers, &profile, cmyk.is_some());
        let document = Self {
            width,
            height,
            profile,
            metadata,
            resolution: resolution.sane(),
            layers: Arc::new(layers),
            records,
            channels: Arc::new(channels),
            import_warnings: Arc::new(Vec::new()),
            // A freshly built document is ENCODED; the policy is only ever
            // chosen deliberately, and a derived one carries it explicitly
            // (`rebuilt`, the profile edits, the native reader). A FLOAT
            // document is linear-light by construction and holds no transfer
            // at all (`float_document`).
            linear_light: None,
            // A freshly BUILT document carries no comp; a derived one takes
            // the list explicitly (`rebuilt`, the native reader).
            comps: Vec::new(),
            // …and no guide, with the ruler origin at the canvas's top-left,
            // for exactly that reason (`precision_guide`).
            guides: Vec::new(),
            ruler_origin: (0.0, 0.0),
            color_samplers: Vec::new(),
            paths: crate::precision_path::TypedPaths::default(),
            // …and the default light, which is what every document before
            // the typed global light rendered with.
            global_light: crate::style::GlobalLight::default(),
            background: None,
            float,
            cmyk,
            gray: None,
            lab: None,
            indexed: None,
            bitmap: None,
        };
        document.native_preflight()?;
        Ok(document)
    }

    pub fn dimensions(&self) -> (u32, u32) {
        (self.width, self.height)
    }
    pub fn profile(&self) -> &Arc<IccProfile> {
        &self.profile
    }
    pub fn metadata(&self) -> &Metadata {
        &self.metadata
    }
    pub fn resolution(&self) -> Resolution {
        self.resolution
    }
    pub fn layer_count(&self) -> usize {
        self.records
    }
    pub fn channels(&self) -> &[PrecisionChannel] {
        &self.channels
    }
    pub(crate) fn rebuilt(
        &self,
        width: u32,
        height: u32,
        layers: Vec<PrecisionDocumentLayer>,
        channels: Vec<PrecisionChannel>,
    ) -> Result<Self, String> {
        // A CMYK document's every rebuild goes through the mode's gate, so
        // an operation that would leave anything but the records the mode
        // holds in it is refused there; its alpha channels pass as they are.
        let mut next = match &self.cmyk {
            Some(policy) => Self::build_cmyk(
                width,
                height,
                self.metadata.clone(),
                self.resolution,
                layers,
                channels,
                policy.clone(),
            )?,
            None => Self::build(
                width,
                height,
                self.profile.clone(),
                self.metadata.clone(),
                self.resolution,
                layers,
                channels,
                self.float.clone(),
            )?,
        };
        next.import_warnings = self.import_warnings.clone();
        // A rebuild keeps the profile, so it keeps the policy's tables too.
        next.linear_light = self.linear_light.clone();
        // …and the comps: a rebuild is the SAME document with changed
        // records, and every comp entry binds by an id the records kept.
        next.comps.clone_from(&self.comps);
        next.background = self.background;
        // …and the guides and the ruler origin, VERBATIM: a rebuild changes
        // records, and the one caller that also changes the CANVAS
        // (`precision_geometry`) overwrites both with the moved chrome right
        // after this call, exactly as the byte geometry ops build their
        // result with the moved lists.
        next.guides.clone_from(&self.guides);
        next.ruler_origin = self.ruler_origin;
        // …and the colour samplers, by the same rule (`precision_geometry`
        // overwrites them with the moved list).
        next.color_samplers.clone_from(&self.color_samplers);
        // …and the paths, by the same rule.
        next.paths.clone_from(&self.paths);
        // …and the global light: a rebuild is the same document.
        next.global_light = self.global_light;
        // …and the Gray policy: a rebuild keeps the profile, its twin.
        next.gray.clone_from(&self.gray);
        // …and the Lab policy, by the same rule.
        next.lab.clone_from(&self.lab);
        // …and the Indexed policy (phase 1a admits no edit, so a rebuild
        // keeps the one layer its plane renders).
        next.indexed.clone_from(&self.indexed);
        // …and the Bitmap policy (phase 1a admits no edit, so a rebuild
        // keeps the one layer its plane renders).
        next.bitmap.clone_from(&self.bitmap);
        Ok(next)
    }
    pub fn root_layers(&self) -> &[PrecisionDocumentLayer] {
        &self.layers
    }
    /// Session provenance, not persisted pixel state. Keep bounded warnings
    /// visible after import instead of dropping unsupported-packet disclosures.
    pub fn with_import_warnings(mut self, warnings: Vec<String>) -> Result<Self, String> {
        if warnings.len() > 64
            || warnings
                .iter()
                .any(|v| v.len() > MAX_DRDC_NAME_LEN as usize)
        {
            return Err("too many high-depth import warnings".into());
        }
        self.import_warnings = Arc::new(warnings);
        Ok(self)
    }

    /// Postorder, bottom-first indexing matches the established layer panel:
    /// children precede their group record; a group itself has no source pixels.
    pub fn layer(&self, index: usize) -> Option<&PrecisionDocumentLayer> {
        fn find<'a>(
            layers: &'a [PrecisionDocumentLayer],
            index: &mut usize,
        ) -> Option<&'a PrecisionDocumentLayer> {
            for layer in layers {
                if *index >= layer.records {
                    *index -= layer.records;
                    continue;
                }
                if let Content::Group(children) | Content::PassThrough(children) = &layer.content {
                    if *index < layer.records - 1 {
                        return find(children, index);
                    }
                }
                return Some(layer);
            }
            None
        }
        find(&self.layers, &mut { index })
    }
    pub fn layer_source(&self, index: usize) -> Option<&PrecisionImage> {
        self.layer(index)?.source()
    }

    pub(crate) fn record_resources(&self, inventory: &mut Inventory) {
        fn nodes(layers: &Arc<Vec<PrecisionDocumentLayer>>, inventory: &mut Inventory) {
            inventory.object(&**layers, "precision_document_structure", false);
            inventory.vec(layers, "precision_document_structure", false);
            for layer in layers.iter() {
                if let Some(source) = &layer.retained_source {
                    source.record_resources(inventory);
                }
                if let Some(warp) = &layer.warp {
                    warp.record_resources(inventory);
                }
                if let Some(generated) = &layer.generated {
                    generated.record_resources(inventory);
                }
                if let Some(description) = &layer.description {
                    inventory.object(&**description, "artwork_description", false);
                    inventory.string(description, "artwork_description", false);
                }
                if let Some(mask) = &layer.mask {
                    mask.record_resources(inventory);
                }
                inventory.object(&*layer.name, "precision_document_strings", false);
                inventory.string(&layer.name, "precision_document_strings", false);
                match &layer.content {
                    Content::Raster { image, .. } => image.record_resources(inventory),
                    Content::Adjustment(state) => state.record_resources(inventory),
                    Content::Group(children) | Content::PassThrough(children) => {
                        nodes(children, inventory)
                    }
                    Content::CmykRaster { image, .. } => image.record_resources(inventory),
                    Content::CmykAdjustment(state) => state.record_resources(inventory),
                }
            }
        }
        nodes(&self.layers, inventory);
        // A live vector mask's raster part is one more mask plane of its
        // record (`precision_vector_mask`); one that IS the slot's plane was
        // recorded by the walk above.
        for (_, part) in self.vector_mask_raster_parts() {
            part.record_resources(inventory);
        }
        inventory.object(&*self.channels, "precision_channels", false);
        inventory.vec(&self.channels, "precision_channels", false);
        for channel in self.channels.iter() {
            channel.record_resources(inventory);
        }
        inventory.object(&*self.profile, "profile", false);
        self.profile.record_resources(inventory);
        if let Some(transfer) = &self.linear_light {
            inventory.object(&**transfer, "linear_light_tables", false);
            transfer.record_resources(inventory);
        }
        if let Some(policy) = &self.float {
            inventory.object(&**policy, "float_policy", false);
            policy.record_resources(inventory);
        }
        if let Some(policy) = &self.cmyk {
            inventory.object(&**policy, "cmyk_policy", false);
            policy.record_resources(inventory);
        }
        if let Some(policy) = &self.gray {
            inventory.object(&**policy, "gray_policy", false);
            policy.record_resources(inventory);
        }
        if let Some(policy) = &self.lab {
            inventory.object(&**policy, "lab_policy", false);
            policy.record_resources(inventory);
        }
        if let Some(policy) = &self.indexed {
            inventory.object(&**policy, "indexed_policy", false);
            policy.record_resources(inventory);
        }
        if let Some(policy) = &self.bitmap {
            inventory.object(&**policy, "bitmap_policy", false);
            policy.record_resources(inventory);
        }
        inventory.object(&*self.import_warnings, "precision_provenance", false);
        inventory.vec(&self.import_warnings, "precision_provenance", false);
        for warning in self.import_warnings.iter() {
            inventory.string(warning, "precision_provenance", false);
        }
        for bytes in [&self.metadata.exif, &self.metadata.xmp, &self.metadata.iptc]
            .into_iter()
            .flatten()
        {
            inventory.record(
                bytes.as_ptr() as usize,
                bytes.len(),
                "capture_metadata",
                false,
            );
        }
    }
    pub fn retained_bytes(&self) -> u64 {
        let mut inventory = Inventory::default();
        self.record_resources(&mut inventory);
        inventory.unique_bytes()
    }

    pub(crate) fn scene(&self) -> Result<(PrecisionScene, u64), String> {
        self.scene_cut(Cut::NONE)
    }
    /// The scene with one record marked as the knockout-capture PROBE
    /// (`precision_style_knockout::level_backdrop`).
    pub(crate) fn scene_probe(&self, probe: usize) -> Result<(PrecisionScene, u64), String> {
        self.scene_for(Cut::NONE, Some(probe))
    }
    fn scene_cut(&self, cut: Cut<'_>) -> Result<(PrecisionScene, u64), String> {
        self.scene_for(cut, None)
    }
    fn scene_for(
        &self,
        cut: Cut<'_>,
        probe: Option<usize>,
    ) -> Result<(PrecisionScene, u64), String> {
        // A CMYK document has no RGB scene: its render is `cmyk_render`'s,
        // and every RGB path that asks for a scene is refused here.
        if self.cmyk.is_some() {
            return Err(crate::cmyk_document::refused_sentence("RGB compositing"));
        }
        // The evaluation policy's transfer, and with it the staleness net: a
        // `linear_light` document whose tables no longer belong to its
        // profile cannot build a scene at all (`precision_evaluation`).
        let transfer = self.transfer()?.cloned();
        // D5 at 16 bits: the Background snapshot is paid for only when some
        // contributing style actually asks for a knockout
        // (`precision_style_knockout`).
        let background =
            crate::precision_style_knockout::document_wants_background16(&self.layers, 0, cut)
                && crate::precision_style_knockout::background_record(
                    &self.layers,
                    (self.width, self.height),
                    cut,
                    self.float.is_some(),
                    self.background,
                );
        let scene = PrecisionScene::new(
            self.width,
            self.height,
            self.profile.clone(),
            render_layers(&self.layers, 0, cut, probe, self.float.is_some())?,
            transfer,
            self.float.is_some(),
        )?
        .with_light(self.global_light)
        .with_knockout_seed(crate::precision_style_knockout::KnockoutSeed::Projection {
            background,
        })
        .with_lab(self.lab_grid());
        // Scene planning accounts visible shared pixels/profile and its own
        // translated nodes. Add only the document's extra metadata, structure
        // and hidden allocations; shared hidden/visible pixels stay deduplicated.
        let mut inventory = Inventory::default();
        inventory.object(&*self.profile, "profile", false);
        self.profile.record_resources(&mut inventory);
        // The scene's own plan counts the transfer, so it belongs in the
        // SHARED baseline here rather than in `extra`.
        if let Some(transfer) = &self.linear_light {
            inventory.object(&**transfer, "linear_light_tables", false);
            transfer.record_resources(&mut inventory);
        }
        // The float policy is deliberately NOT in this baseline: the scene
        // carries only the float FLAG, so its plan does not count the
        // policy's encoder tables and they must land in `extra`.
        // Contribution, not `visible` alone: a layer the cut excludes is absent
        // from the scene's own inventory, so its bytes must stay in `extra`.
        fn visible(
            layers: &[PrecisionDocumentLayer],
            start: usize,
            cut: Cut<'_>,
            inventory: &mut Inventory,
        ) {
            let mut offset = start;
            for layer in layers {
                let node_start = offset;
                let index = offset + layer.records - 1;
                offset += layer.records;
                if !layer.visible || cut.excludes(index) {
                    continue;
                }
                if let Some(description) = &layer.description {
                    inventory.object(&**description, "artwork_description", false);
                    inventory.string(description, "artwork_description", false);
                }
                if let Some(mask) = &layer.mask {
                    mask.record_resources(inventory);
                }
                match &layer.content {
                    Content::Raster { image, .. } => image.record_resources(inventory),
                    Content::Adjustment(state) => state.record_resources(inventory),
                    Content::Group(children) | Content::PassThrough(children) => {
                        visible(children, node_start, cut, inventory)
                    }
                    Content::CmykRaster { image, .. } => image.record_resources(inventory),
                    Content::CmykAdjustment(state) => state.record_resources(inventory),
                }
            }
        }
        visible(&self.layers, 0, cut, &mut inventory);
        let shared = inventory.unique_bytes();
        self.record_resources(&mut inventory);
        Ok((scene, inventory.unique_bytes() - shared))
    }
    /// The projection's storage: a float document renders — and stores — in
    /// extended-range f32, so nothing in its pipeline quantizes.
    pub(crate) fn render_storage(&self) -> PrecisionStorage {
        if self.float.is_some() {
            PrecisionStorage::Rgba32Float
        } else {
            PrecisionStorage::Rgba16Unorm
        }
    }
    pub fn render_plan(&self) -> Result<PrecisionRenderPlan, String> {
        if self.cmyk.is_some() {
            return self.cmyk_render_plan();
        }
        let (scene, extra) = self.scene()?;
        let mut plan = scene.plan(self.render_storage())?;
        plan.unique_retained_bytes += extra;
        Ok(plan)
    }
    /// Working memory of turning this document's render into display bytes:
    /// float → `float_document::display_scratch`, u16 →
    /// `precision_image::rgba8_preview_scratch`, over the canvas. A Lab
    /// document's display is two stages, the Lab view
    /// (`lab_convert::view_scratch_bytes`) and then the preview of it with the
    /// view held, each bounded by this figure in the host
    /// (`PreparedDocumentEdit`), so it is the larger of the two (#1 Lab
    /// phase 4a-ii: the preview alone left a small Lab canvas's view
    /// refused).
    pub(crate) fn display_scratch_bytes(&self) -> Result<u64, String> {
        if self.float.is_some() {
            crate::float_document::display_scratch(self.width, self.height)
        } else if self.is_duotone() {
            // #1 Duotone phase 1a: the view, then its preview, Lab's shape.
            crate::duotone_document::display_scratch_bytes(self.width, self.height)
        } else if self.is_lab() {
            let preview = crate::precision_image::rgba8_preview_scratch(self.width, self.height)?;
            let view = crate::lab_convert::view_scratch_bytes(self.width, self.height)?;
            let held = (u64::from(self.width) * u64::from(self.height))
                .checked_mul(crate::lab_convert::VIEW_BYTES_PER_PIXEL)
                .and_then(|bytes| bytes.checked_add(preview))
                .ok_or("display working-memory calculation overflow")?;
            Ok(view.max(held))
        } else {
            crate::precision_image::rgba8_preview_scratch(self.width, self.height)
        }
    }
    /// What creating the display preparation's buffers costs beyond their
    /// bytes, charged on top of its peak max(render, output + display):
    /// page tables (one 16 KiB table per 32 MiB mapped, peak / 2048) and the
    /// output's clipped edge tiles rounding up to the 32 KiB malloc quantum.
    pub(crate) fn display_overhead_bytes(&self, plan: &PrecisionRenderPlan) -> Result<u64, String> {
        const PAGE_TABLE_RATIO: u64 = 2048;
        const EDGE_TILE_SLACK: u64 = 32 * 1024;
        let overflow = || "display overhead calculation overflow".to_string();
        let staged = plan
            .output_bytes
            .checked_add(self.display_scratch_bytes()?)
            .ok_or_else(overflow)?;
        let peak = plan.scratch_bytes.max(staged);
        let edges = u64::from(self.width.div_ceil(crate::tile_store::TILE_EDGE))
            + u64::from(self.height.div_ceil(crate::tile_store::TILE_EDGE)).saturating_sub(1);
        peak.div_ceil(PAGE_TABLE_RATIO)
            .checked_add(edges.checked_mul(EDGE_TILE_SLACK).ok_or_else(overflow)?)
            .ok_or_else(overflow)
    }
    pub fn render(
        &self,
        limit: u64,
        other_retained: u64,
        control: Option<&DrJobControl>,
    ) -> Result<PrecisionImage, String> {
        let _run = control.map(DrJobControl::begin).transpose()?;
        self.render_with_context(
            limit,
            other_retained,
            control.map_or(JobContext::NONE, JobContext::new),
        )
    }

    pub(crate) fn render_with_context(
        &self,
        limit: u64,
        other_retained: u64,
        job: JobContext<'_>,
    ) -> Result<PrecisionImage, String> {
        if self.cmyk.is_some() {
            return self.cmyk_render_with_context(limit, other_retained, job);
        }
        self.render_cut_with_context(Cut::NONE, limit, other_retained, job)
    }

    /// The ordinary render with a contribution cut: the retouch and backdrop
    /// plates. The cut changes contribution only, so the uncut `render_plan`
    /// still bounds it (same group depth, no more records).
    pub(crate) fn render_cut_with_context(
        &self,
        cut: Cut<'_>,
        limit: u64,
        other_retained: u64,
        job: JobContext<'_>,
    ) -> Result<PrecisionImage, String> {
        job.check()?;
        admit(0, BOOKKEEPING_BYTES, other_retained, limit)?;
        job.reserve(BOOKKEEPING_BYTES)?;
        let (scene, extra) = self.scene_cut(cut)?;
        let other = other_retained
            .checked_add(extra)
            .ok_or("high-depth retained byte calculation overflow")?;
        scene.render_with_context(self.render_storage(), limit, other, job)
    }

    /// The uncut render with the knockout SEED replaced
    /// (`precision_style_knockout`) over the merge's `frame` rather than the
    /// canvas (`precision_merge_extent`). The ONLY caller is
    /// `precision_merge::merged_pixels`, which renders EVERY members document
    /// through `Handed` so D5 can never fire in a derived document. The
    /// output is unseeded (13B-3d-3a): the merged layer is
    /// installed and read through tiles.
    pub(crate) fn render_seeded_with_context(
        &self,
        seed: crate::precision_style_knockout::KnockoutSeed,
        frame: crate::precision_merge_extent::Frame,
        limit: u64,
        other_retained: u64,
        job: JobContext<'_>,
    ) -> Result<PrecisionImage, String> {
        job.check()?;
        admit(0, BOOKKEEPING_BYTES, other_retained, limit)?;
        job.reserve(BOOKKEEPING_BYTES)?;
        let (scene, extra) = self.scene_cut(Cut::NONE)?;
        let other = other_retained
            .checked_add(extra)
            .ok_or("high-depth retained byte calculation overflow")?;
        scene
            .with_frame(frame.0, frame.1)?
            .with_knockout_seed(seed)
            .render_with_context(self.render_storage(), limit, other, job)
    }

    /// Bounded host capture/preflight data. It deliberately excludes layer
    /// names, import warnings and canonical operator payloads, which can be
    /// large even when all retained document pixels are shared.
    pub fn descriptor(&self) -> Result<Value, String> {
        if self.cmyk.is_some() {
            return self.cmyk_descriptor();
        }
        let render = self.render_plan();
        let native = self.native_preflight()?;
        let (color_mode, gray) = match &self.gray {
            // A Bitmap document is 8-bit Gray underneath: its mode first,
            // the `gray` key kept.
            Some(policy) if self.bitmap.is_some() => (
                crate::bitmap_document::COLOR_MODE_BITMAP,
                json!({"profile":policy.name(),"depth":policy.depth().bits()}),
            ),
            Some(policy) => (
                crate::gray_document::COLOR_MODE_GRAY,
                json!({"profile":policy.name(),"depth":policy.depth().bits()}),
            ),
            None if self.lab.is_some() => (crate::lab_document::COLOR_MODE_LAB, Value::Null),
            None if self.indexed.is_some() => {
                (crate::indexed_document::COLOR_MODE_INDEXED, Value::Null)
            }
            None => ("rgb", Value::Null),
        };
        let storage = if self.float.is_some() {
            crate::float_document::STORAGE_FLOAT
        } else if self.cmyk.is_some() {
            crate::cmyk_document::STORAGE_CMYK
        } else {
            "rgba16_unorm"
        };
        Ok(
            json!({"width":self.width,"height":self.height,"storage":storage,"evaluation":self.evaluation().typed_tag(),
            "tone_map":self.tone_map().map(crate::float_document::ToneMap::name),
            "range":if self.float.is_some() {"extended"} else {"normalized"},"resolution":{"x_ppi":self.resolution.x,"y_ppi":self.resolution.y},"layer_count":self.records,"channel_count":self.channels.len(),"inspection_bytes":crate::precision_inspect::inspection_bytes(self),"metadata_bytes":{"exif":self.metadata.exif.as_ref().map_or(0, |p|p.len()),"xmp":self.metadata.xmp.as_ref().map_or(0, |p|p.len()),"iptc":self.metadata.iptc.as_ref().map_or(0, |p|p.len())},"profile_bytes":self.profile.bytes().len(),"retained_bytes":self.retained_bytes(),"render_scratch_bytes":render.as_ref().ok().map(|plan|plan.scratch_bytes),"render_output_bytes":render.as_ref().ok().map(|plan|plan.output_bytes),"display_scratch_bytes":self.display_scratch_bytes().ok(),"display_overhead_bytes":render.as_ref().ok().and_then(|plan|self.display_overhead_bytes(plan).ok()),"render_refusal":render.as_ref().err(),"native_scratch_bytes":native.write_scratch_bytes,"reopen_retained_bytes":native.reopen_retained_bytes,"decode_scratch_bytes":native.decode_scratch_bytes,"native_open_scratch_bytes":native.decode_scratch_bytes,"color_mode":color_mode,"supported_operations":["normal_composite","isolated_normal_group","basic_layer_blend","pass_through_group","duplicate_layer","layer_opacity","native_save","png16","tiff16","adjustment","assign_profile","profile_conversion","mask","channel","read_plane","affine","perspective","resize","crop","rotate90","rotate180","rotate270","flip_horizontal","flip_vertical","paint","erase","clone","heal","retouch_sampling","retouch_transform","retouch_destination_expansion","spot_heal","red_eye","content_aware_fill","rasterize","clipping","blend_if","style"],"unsupported_operations":["group_transform","other_blend_modes","rgba_plane_edit","mask_paint","channel_paint","live_source_rasterization"]}),
        )
        .map(|mut descriptor: Value| {
            if self.gray.is_some() {
                descriptor["gray"] = gray;
                descriptor["supported_operations"] =
                    json!(crate::precision_capability::gray_operations());
            }
            // #1 Duotone phase 1a: the storage stays Gray (`color_mode`
            // "gray"); the description's type and ink count beside it.
            if let Some(inks) = self.gray.as_ref().and_then(|p| p.duotone()) {
                descriptor["duotone"] = json!({"type": inks.type_name(), "inks": inks.inks().len()});
            }
            if self.bitmap.is_some() {
                descriptor["supported_operations"] =
                    json!(crate::precision_capability::bitmap_operations());
            }
            if let Some(policy) = &self.lab {
                descriptor["lab"] = json!({"depth":policy.depth().bits()});
                // Phase 2f: the pass filters' plans (`lab_channel_filter`).
                descriptor["lab"]["filter_plans"] =
                    Value::Object(self.lab_filter_descriptor_plans());
                descriptor["supported_operations"] =
                    json!(crate::precision_capability::lab_operations());
            }
            if let Some(policy) = &self.indexed {
                descriptor["indexed"] = json!({
                    "colors": policy.palette().len(),
                    "transparent_index": policy.transparent(),
                });
                descriptor["supported_operations"] =
                    json!(crate::precision_capability::indexed_operations());
            }
            descriptor
        })
    }

    /// Detailed inspection is separate from frequent capture/admission queries.
    /// Operator payloads are never copied here: native serialization owns them.
    pub fn info(&self) -> Result<Value, String> {
        let mut info = self.descriptor()?;
        info["layers"] = Value::Array(crate::precision_inspect::layer_info(self));
        info["channels"]=json!(self.channels.iter().enumerate().map(|(index,c)|json!({"index":index,"id":c.id(),"name":c.name(),"dimensions":c.plane().dimensions(),"overlay_color":c.metadata().overlay_color,"overlay_opacity":c.metadata().overlay_opacity,"color_indicates_selected":c.metadata().color_indicates_selected,"provenance":c.metadata().provenance.as_deref(),"spot":c.metadata().spot.map(|s|json!({"lab":s.lab(),"solidity":s.solidity()}))})).collect::<Vec<_>>());
        info["profile"] = match &self.cmyk {
            Some(policy) => json!(policy.name()),
            None => json!(self.profile.name()),
        };
        info["import_warnings"] = json!(*self.import_warnings);
        info["global_light"] =
            json!({"angle":self.global_light.angle,"altitude":self.global_light.altitude});
        Ok(info)
    }
}
