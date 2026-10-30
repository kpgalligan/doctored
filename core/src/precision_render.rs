//! Bounded Phase 7A neutral-compositing experiment. This tree is deliberately
//! separate from DrDocument and cannot enter legacy RGBA8 editing operations.
//! Raster and qualified isolated blend groups use blend::composite_source_into, the
//! existing f32 W3C kernel. Groups retain straight f32 buffers through every
//! isolation boundary; only the explicitly requested final store quantizes.
//! Profiles are the same captured Arc handle, colors are encoded [0,1], alpha
//! is coverage. No transfer curve conversion or HDR interpretation is inferred.
//!
//! `precision_render.rs` multiplies `source_weight_16` (`source_weight_float` in a 32-bit float
//! scene, through `StyleEnv::blend_if_weight`) into the effective alpha AFTER
//! `checked_alpha` in the Raster and Group arms; a clip base records its UNWEIGHTED footprint,
//! weights only its own `buf` alpha, and its members land with that alpha frozen relative to the
//! footprint (`clip_unit`'s Open / Settle / Close, #578, the one rule of every clip unit).
//!
//! # Admission
//! A float render adopts its top accumulator as the result and tiles it, so
//! finalization holds 32 B/px; it is charged `F32_FINALIZATION_BYTES_PER_PIXEL`
//! (41, fitted: the smallest integer ≥ 40 whose plan covers ⌈1.25 × max E⌉
//! at both sizes; 40 misses R by 1.6 MB at 24 MP). Method: Release, one case
//! per process, three runs, `MallocLargeCache=0`, process limit 8 GiB, E =
//! `ri_interval_max_phys_footprint` − `ri_phys_footprint` read after
//! `proc_reset_footprint_interval`; an opaque float layer (`float_adjust_apply_24mp.rs`'s
//! formula) saved to `.rz` and reopened. R = `render(8 GiB, 0, None)`, A = `render_plan()`
//! scratch; F = `{"op":"flatten"}` through `dr_precision_doc_edit`, A = its plan's scratch.
//! Max E (bytes) and A: R 24 MP (6000×4000) 770,097,704 / 985,048,576 (1.2791×), R 100 MP
//! (10000×10000) 3,208,087,808 / 4,101,048,576 (1.2783×); F 24 MP 770,048,552 /
//! 1,374,295,572, F 100 MP 3,207,940,376 / 5,706,295,572. Baseline twins (B0 nothing, B1 the
//! reopen alone) read 0. Floors pinned in `tests/float_above_cache_bound.rs`.

use std::sync::Arc;

use crate::blend::{
    composite_source_into, dissolve_threshold, gate_source_into, mix_adjusted_rgb, BlendKind,
    BlendMode,
};
use crate::clip_unit::{clip_pass, ClipStep};
use crate::icc::IccProfile;
use crate::job_control::{DrJobControl, JobContext};
use crate::precision_adjustment::PrecisionAdjustmentState;
use crate::precision_image::{
    admit, pixel_count, reserved_vec, PrecisionImage, PrecisionStorage, BOOKKEEPING_BYTES,
};
use crate::precision_mask::{PrecisionMask, PreparedMask};
use crate::precision_style_knockout::{knockout_scratch, TypedCapture};
use crate::precision_style_render::{
    blend_if_weight_at, composite_styled_into, style_scratch, StyleEnv, StyledLayer, StyledPixels,
};
use crate::resources::Inventory;
use crate::style::{BlendIf, GlobalLight, LayerStyle};
use crate::style_float::FloatStyle;

pub const MAX_RECORDS: usize = 1024;
pub const MAX_GROUP_DEPTH: usize = 16;
/// The INTERACTIVE work budget. It is no longer a refusal for work
/// that runs under a job control: a render, a mask feather, an adjustment, a
/// selection op or a commit is bounded by memory admission and its Cancel.
/// What it still bounds: an `interactive` geometry or source-transform
/// preview (`precision_geometry_budget::work_limit`, `doc_source_geometry`,
/// `doc_retained_source` and `precision_retained_source`'s preview arms; the
/// host answers that refusal with its reduced proxy or the commit request as
/// a cancellable preview job), the uncontrolled mask feather a content box or
/// a hit test would prepare (`precision_content_box`, `precision_layer_hit`:
/// over it the feather counts as no mask), and the two whole-plane selection
/// test oracles (`precision_selection_plane::reference_apply`,
/// `precision_selection_refine::reference_refine`).
pub const MAX_WORK_UNITS: u64 = 2_000_000_000;
/// The normalized f32 source-over kernel is not a subnormal-alpha solver.
/// A 2^-96 floor admits multiplied u16 mask/alpha while keeping effective
/// alpha arithmetic away from underflow. Storage still preserves smaller alpha;
/// rendering refuses it (including opacity underflow) instead of damaging RGB.
pub const MIN_RENDER_ALPHA: f32 = 1.0 / 79_228_162_514_264_337_593_543_950_336.0;

/// A float render's finalization per pixel: the adopted top
/// accumulator (16 B) and the tiles `tiles_of` copies it into (16 B) are held
/// together until the accumulator drops, 32 B/px, charged at 5/4 and fitted
/// to the measured peaks (module doc, `# Admission`).
pub(crate) const F32_FINALIZATION_BYTES_PER_PIXEL: u64 = 41;
/// A style or Blend If on a render kind that carries none (a Pass Through
/// render layer). Unreachable from a document: `render_layer` converts a
/// styled Pass Through group to an isolated one first.
pub(crate) const UNSTYLED_KIND: &str =
    "internal error: a style or Blend If on a render layer kind that carries none";

#[derive(Clone)]
pub(crate) enum Kind {
    Raster {
        image: PrecisionImage,
        offset: (i32, i32),
    },
    Adjustment(PrecisionAdjustmentState),
    Group(Arc<Vec<PrecisionLayer>>),
    PassThrough(Arc<Vec<PrecisionLayer>>),
}

/// Private contents keep malformed/deep trees unrepresentable after creation.
#[derive(Clone)]
pub struct PrecisionLayer {
    pub(crate) kind: Kind,
    pub(crate) opacity: f32,
    pub(crate) blend_mode: BlendMode,
    records: usize,
    group_depth: usize,
    pub(crate) mask: Option<PrecisionMask>,
    /// The CONTRIBUTING clipped entries above this base, bottom-first; empty
    /// for every ordinary layer. A non-empty list renders through the clip
    /// kernel in `render_layers` (`doc.rs::composite_clip_group_into`).
    pub(crate) clipped_members: Arc<Vec<PrecisionLayer>>,
    /// Blend If on a raster or isolated group: `source_weight_16` (at float
    /// `source_weight_float`) on the
    /// layer's colour and the accumulator beneath multiplies the effective
    /// alpha after `checked_alpha` (`precision_blend_if`).
    pub(crate) blend_if: Option<BlendIf>,
    /// Fill opacity and effects on a raster or isolated group: routed to
    /// `precision_style_render::composite_styled_into` (`precision_style`).
    pub(crate) style: Option<Arc<LayerStyle>>,
    /// Marks the ONE record a `KnockoutCapture` render is looking for
    /// (`precision_style_knockout::level_backdrop`). False in all four
    /// constructors, so an ordinary scene never carries it.
    pub(crate) knockout_probe: bool,
}

impl PrecisionLayer {
    /// This layer's own knockout, or `None` when it carries no style — the
    /// typed twin of `style_knockout::knockout_of`.
    pub(crate) fn knockout(&self) -> crate::style::Knockout {
        self.style
            .as_deref()
            .map_or(crate::style::Knockout::None, |s| s.knockout)
    }

    /// Whether this layer is the capture's probe, directly or through one of
    /// its clipped members — the ONE test `render_layers` makes at loop top.
    pub(crate) fn is_probe(&self) -> bool {
        self.knockout_probe || self.clipped_members.iter().any(|m| m.knockout_probe)
    }

    /// Consumes the layer and marks it (or the matching descendant) as the
    /// capture probe.
    pub(crate) fn with_knockout_probe(mut self, probe: bool) -> Self {
        self.knockout_probe = probe;
        self
    }
}

/// D4's clipped-member rule at 16 bits: whether some member of this run wants
/// the base-only snapshot. False for every clip unit in every document without
/// knockout, which is what keeps the clone off the shipped path.
pub(crate) fn members_want_base_snapshot16(members: &[PrecisionLayer]) -> bool {
    members
        .iter()
        .any(|m| m.knockout() != crate::style::Knockout::None)
}

fn opacity_valid(opacity: f32) -> Result<(), String> {
    if opacity.is_finite() && (0.0..=1.0).contains(&opacity) {
        Ok(())
    } else {
        Err("precision layer opacity must be finite and in [0,1]".into())
    }
}

pub(crate) fn checked_alpha(alpha: f32, opacity: f32, coverage: f32) -> Result<f32, String> {
    if alpha == 0.0 || opacity == 0.0 || coverage == 0.0 {
        return Ok(0.0);
    }
    let effective = alpha * opacity * coverage;
    if !effective.is_finite() || effective < MIN_RENDER_ALPHA {
        return Err(
            "precision prototype effective alpha is below the supported 2^-96 arithmetic floor"
                .into(),
        );
    }
    Ok(effective)
}

fn structure(layers: &[PrecisionLayer]) -> Result<(usize, usize), String> {
    if layers.len() > MAX_RECORDS {
        return Err("precision prototype exceeds its record limit".into());
    }
    let mut records = 0_usize;
    let mut depth = 0;
    for layer in layers {
        records = records
            .checked_add(layer.records)
            .ok_or("precision record count overflow")?;
        depth = depth.max(layer.group_depth);
        if records > MAX_RECORDS || depth > MAX_GROUP_DEPTH {
            return Err("precision prototype exceeds its record or group-depth limit".into());
        }
    }
    Ok((records, depth))
}

impl PrecisionLayer {
    /// Bounded encoded-RGB layer modes. Adjustments and pass-through groups
    /// retain their separate contribution semantics. No additional buffers.
    pub fn with_blend_mode(mut self, mode: BlendMode) -> Result<Self, String> {
        use crate::precision_layer_blend::BlendTarget;
        crate::precision_layer_blend::validate(
            mode,
            match self.kind {
                Kind::Raster { .. } => BlendTarget::Raster,
                Kind::Adjustment(_) => BlendTarget::Adjustment,
                Kind::Group(_) => BlendTarget::IsolatedGroup,
                Kind::PassThrough(_) => BlendTarget::PassThroughGroup,
            },
        )?;
        self.blend_mode = mode;
        Ok(self)
    }
    pub fn with_mask(mut self, mask: Option<PrecisionMask>) -> Self {
        self.mask = mask;
        self
    }
    /// Raster layers, adjustments (This Layer reads the adjusted
    /// colour, `precision_style_adjustment`) and isolated groups; `None` is
    /// the identity on any kind. No additional buffers on a raster or group:
    /// the weight is evaluated per pixel against the accumulator the layer is
    /// about to composite onto.
    pub fn with_blend_if(mut self, blend_if: Option<BlendIf>) -> Result<Self, String> {
        if blend_if.is_some()
            && !matches!(
                self.kind,
                Kind::Raster { .. } | Kind::Adjustment(_) | Kind::Group(_)
            )
        {
            return Err(UNSTYLED_KIND.into());
        }
        self.blend_if = blend_if;
        Ok(self)
    }
    /// Raster layers, adjustments and isolated groups; `None` is the
    /// identity on any kind. The styled composite is planned by
    /// `style_scratch` and rendered per composite with no retained planes.
    pub fn with_style(mut self, style: Option<Arc<LayerStyle>>) -> Result<Self, String> {
        if style.is_some()
            && !matches!(
                self.kind,
                Kind::Raster { .. } | Kind::Adjustment(_) | Kind::Group(_)
            )
        {
            return Err(UNSTYLED_KIND.into());
        }
        self.style = style;
        Ok(self)
    }
    /// Whether this layer takes the styled path (`render_styled`): any
    /// style, or an ADJUSTMENT with Blend If, which has no per-pixel
    /// weight site of its own and so composites through the styled interior
    /// (`precision_style_adjustment`).
    pub(crate) fn renders_styled(&self) -> bool {
        self.style.is_some()
            || (matches!(self.kind, Kind::Adjustment(_)) && self.blend_if.is_some())
    }
    pub fn adjustment(state: PrecisionAdjustmentState, opacity: f32) -> Result<Self, String> {
        opacity_valid(opacity)?;
        Ok(Self {
            kind: Kind::Adjustment(state),
            opacity,
            blend_mode: BlendMode::Normal,
            records: 1,
            group_depth: 0,
            mask: None,
            clipped_members: Arc::new(Vec::new()),
            blend_if: None,
            style: None,
            knockout_probe: false,
        })
    }
    /// Children evaluate against the enclosing backdrop; group opacity gates
    /// their combined premultiplied contribution with the shared legacy kernel.
    pub fn pass_through(children: Vec<Self>, opacity: f32) -> Result<Self, String> {
        let mut layer = Self::isolated_normal(children, opacity)?;
        if let Kind::Group(children) = layer.kind {
            layer.kind = Kind::PassThrough(children);
        }
        Ok(layer)
    }
    pub fn raster(image: PrecisionImage, offset: (i32, i32), opacity: f32) -> Result<Self, String> {
        opacity_valid(opacity)?;
        Ok(Self {
            kind: Kind::Raster { image, offset },
            opacity,
            blend_mode: BlendMode::Normal,
            records: 1,
            group_depth: 0,
            mask: None,
            clipped_members: Arc::new(Vec::new()),
            blend_if: None,
            style: None,
            knockout_probe: false,
        })
    }

    /// Children are bottom-first and use canvas coordinates. Empty groups are
    /// valid. No pass-through, clipping, mask or style semantics are implied.
    pub fn isolated_normal(children: Vec<Self>, opacity: f32) -> Result<Self, String> {
        opacity_valid(opacity)?;
        let (records, depth) = structure(&children)?;
        if records >= MAX_RECORDS || depth >= MAX_GROUP_DEPTH {
            return Err("precision prototype exceeds its record or group-depth limit".into());
        }
        Ok(Self {
            kind: Kind::Group(Arc::new(children)),
            opacity,
            blend_mode: BlendMode::Normal,
            records: records + 1,
            group_depth: depth + 1,
            mask: None,
            clipped_members: Arc::new(Vec::new()),
            blend_if: None,
            style: None,
            knockout_probe: false,
        })
    }

    /// `members` are the CONTRIBUTING clipped entries above this base,
    /// bottom-first (any kind may be a base). Their records are added to this
    /// layer's; `depth = max(self.group_depth, members' max group_depth)`,
    /// refused at `>= MAX_GROUP_DEPTH` exactly as `isolated_normal` refuses
    /// (the record count is refused at `> MAX_RECORDS`, `structure`'s own
    /// bound: this unit adds no record of its own, so the limit itself is
    /// admissible),
    /// and the unit's `group_depth` becomes `depth + 1` because the clip buffer
    /// is one full plane above whatever the base or a member allocates. Empty
    /// `members` is the identity, which keeps plain documents on the plain path.
    pub fn with_clipped_members(mut self, members: Vec<Self>) -> Result<Self, String> {
        if members.is_empty() {
            return Ok(self);
        }
        let (added, member_depth) = structure(&members)?;
        let records = self
            .records
            .checked_add(added)
            .ok_or("precision record count overflow")?;
        let depth = self.group_depth.max(member_depth);
        if records > MAX_RECORDS || depth >= MAX_GROUP_DEPTH {
            return Err("precision prototype exceeds its record or group-depth limit".into());
        }
        self.records = records;
        self.group_depth = depth + 1;
        self.clipped_members = Arc::new(members);
        Ok(self)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrototypeOperation {
    NormalComposite,
    IsolatedNormalGroup,
    PassThroughGroup,
    ExplicitStorageConversion,
    ExplicitRgba8Preview,
    Adjustment,
    OtherBlendMode,
    Mask,
    Geometry,
    Paint,
    Retouch,
    NativePersistence,
    ProfileConversion,
    Hdr,
}

/// Rendering/storage operations available on PrecisionScene itself. Document
/// editing, persistence and profile conversion use PrecisionDocument plans;
/// this scene-level query does not advertise those separate owner APIs.
pub fn require_supported(operation: PrototypeOperation) -> Result<(), String> {
    match operation {
        PrototypeOperation::NormalComposite | PrototypeOperation::IsolatedNormalGroup | PrototypeOperation::PassThroughGroup
        | PrototypeOperation::ExplicitStorageConversion | PrototypeOperation::ExplicitRgba8Preview | PrototypeOperation::Adjustment | PrototypeOperation::Mask => Ok(()),
        _ => Err(format!("{operation:?} is unavailable in the high-depth prototype; no implicit RGBA8 conversion is permitted")),
    }
}

#[derive(Clone)]
pub struct PrecisionScene {
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) profile: Arc<IccProfile>,
    pub(crate) layers: Arc<Vec<PrecisionLayer>>,
    group_depth: usize,
    /// The document's EVALUATION POLICY as tables, handed down by
    /// `PrecisionDocument::scene_cut` (which obtained it from
    /// `PrecisionDocument::transfer`, the refusal walk). `None` is `encoded`
    /// and every statement below is then what it was before Phase 11E.
    pub(crate) linear_light: Option<Arc<crate::precision_evaluation::TypedTransfer>>,
    /// True for a 32-bit FLOAT scene (`float_document`): the accumulator's
    /// components are already LINEAR and extended-range, so no decode, no
    /// re-encode and no clamp happens anywhere in this render.
    pub(crate) float: bool,
    /// What the TOP accumulator seeds `StyleEnv::knockout` with
    /// (`precision_style_knockout`). The default —
    /// `Projection { background: false }` — is every scene before Phase
    /// 12E-3b and every scene with no knockout, and takes the shipped path
    /// statement for statement.
    pub(crate) knockout_seed: crate::precision_style_knockout::KnockoutSeed,
    /// The document's GLOBAL LIGHT (`precision_global_light`), handed down
    /// by `PrecisionDocument::scene_for`; every `StyleEnv` reads it. A
    /// scene built by `new` alone has the default.
    pub(crate) light: GlobalLight,
    /// The DOCUMENT canvas, which every `StyleEnv` reads. `width`/`height`
    /// are the FRAME this scene renders: the canvas, unless `with_frame`
    /// replaced it (a merge's union extent, `precision_merge_extent`).
    pub(crate) canvas: (u32, u32),
    /// The document pixel the frame's pixel (0, 0) is: (0, 0) unless
    /// `with_frame` set it. Layers keep their document coordinates.
    pub(crate) origin: (i32, i32),
    /// A Lab Color document's grid, set by `with_lab`: every
    /// `StyleEnv` reads it, so the scene composites through `lab_blend`'s
    /// arms. `None` for every other scene.
    pub(crate) lab: Option<crate::lab_blend::LabGrid>,
}

#[derive(Clone, Copy, Debug)]
pub struct PrecisionRenderPlan {
    /// Unique live heap payload capacity, including profile/record arrays.
    /// Arc headers, allocator rounding and query bookkeeping use the fixed
    /// scratch allowance. This is not process RSS or an OS memory ceiling.
    pub unique_retained_bytes: u64,
    /// Conservative peak render allocation, including the returned pixels.
    pub scratch_bytes: u64,
    pub output_bytes: u64,
    pub work_units: u64,
    pub group_depth: usize,
}

impl PrecisionRenderPlan {
    pub fn total_working_bytes(&self, other_retained_bytes: u64) -> Result<u64, String> {
        self.unique_retained_bytes
            .checked_add(self.scratch_bytes)
            .and_then(|v| v.checked_add(other_retained_bytes))
            .ok_or("precision working-memory calculation overflow".into())
    }
}

impl PrecisionScene {
    pub fn new(
        width: u32,
        height: u32,
        profile: Arc<IccProfile>,
        layers: Vec<PrecisionLayer>,
        linear_light: Option<Arc<crate::precision_evaluation::TypedTransfer>>,
        float: bool,
    ) -> Result<Self, String> {
        pixel_count(width, height)?;
        let (_, group_depth) = structure(&layers)?;
        fn profiles(layers: &[PrecisionLayer], expected: &Arc<IccProfile>) -> bool {
            layers.iter().all(|l| {
                profiles(&l.clipped_members, expected)
                    && match &l.kind {
                        Kind::Raster { image, .. } => Arc::ptr_eq(image.profile(), expected),
                        Kind::Adjustment(state) => Arc::ptr_eq(state.profile(), expected),
                        Kind::Group(children) | Kind::PassThrough(children) => {
                            profiles(children, expected)
                        }
                    }
            })
        }
        if !profiles(&layers, &profile) {
            return Err("precision scene requires one shared captured profile handle; assign or convert explicitly outside the prototype".into());
        }
        Ok(Self {
            width,
            height,
            profile,
            layers: Arc::new(layers),
            group_depth,
            linear_light,
            float,
            knockout_seed: crate::precision_style_knockout::KnockoutSeed::default(),
            light: GlobalLight::default(),
            canvas: (width, height),
            origin: (0, 0),
            lab: None,
        })
    }

    /// Marks the scene as a Lab Color document's on `grid`.
    pub(crate) fn with_lab(mut self, grid: Option<crate::lab_blend::LabGrid>) -> Self {
        self.lab = grid;
        self
    }

    /// Renders the FRAME `dims` whose pixel (0, 0) is document pixel `origin`
    /// instead of the canvas — a merge's union extent
    /// (`precision_merge_extent`). Only the frame changes: the layers stay in
    /// document coordinates and `canvas` stays the document's.
    pub(crate) fn with_frame(
        mut self,
        origin: (i32, i32),
        dims: (u32, u32),
    ) -> Result<Self, String> {
        pixel_count(dims.0, dims.1)?;
        self.width = dims.0;
        self.height = dims.1;
        self.origin = origin;
        Ok(self)
    }

    /// Replaces the scene's global light (`precision_global_light`).
    pub(crate) fn with_light(mut self, light: GlobalLight) -> Self {
        self.light = light;
        self
    }

    /// Replaces the knockout SEED (`precision_style_knockout`). The document's
    /// own projection sets `Projection { background }`; every derived members
    /// document of a merge sets `Handed`, so D5 can never fire in one.
    pub(crate) fn with_knockout_seed(
        mut self,
        seed: crate::precision_style_knockout::KnockoutSeed,
    ) -> Self {
        self.knockout_seed = seed;
        self
    }

    /// Metadata-only query: no pixels are scanned or rendered. All actual
    /// input capacities are counted once even when layers/trees share Arcs.
    pub fn plan(&self, storage: PrecisionStorage) -> Result<PrecisionRenderPlan, String> {
        let count = pixel_count(self.width, self.height)? as u64;
        let mut inventory = Inventory::default();
        inventory.object(&*self.profile, "profile", false);
        self.profile.record_resources(&mut inventory);
        // The linear-light tables are RETAINED bytes, not scratch: they are
        // built once per document and live as long as it does.
        if let Some(transfer) = &self.linear_light {
            inventory.object(&**transfer, "linear_light_tables", false);
            transfer.record_resources(&mut inventory);
        }
        let mut guide_scratch = 0;
        let mut mask_scratch = 0;
        let mut clip_scratch = 0;
        let mut style_scratch_peak = 0;
        let mut work = count * 2; // Initial accumulator fill and final storage.
        #[allow(clippy::too_many_arguments)]
        fn collect(
            layers: &Arc<Vec<PrecisionLayer>>,
            dimensions: (u32, u32),
            origin: (i32, i32),
            inventory: &mut Inventory,
            work: &mut u64,
            guide_scratch: &mut u64,
            mask_scratch: &mut u64,
            clip_levels: usize,
            clip_scratch: &mut u64,
            style_scratch_peak: &mut u64,
            style_held: u64,
        ) -> Result<(), String> {
            inventory.object(&**layers, "precision_structure", false);
            inventory.vec(layers, "precision_structure", false);
            let count = u64::from(dimensions.0) * u64::from(dimensions.1);
            for layer in layers.iter() {
                // What an enclosing styled clip base still holds while this
                // layer renders: `composite_styled_into` drives the members
                // with the base's planes alive, so a nested styled layer
                // allocates on top of them (the way `clip_scratch` nests).
                let mut member_held = style_held;
                // D8 at 16 bits (`precision_style_knockout::knockout_scratch`):
                // the routed unit buffer, the grouped-interior alpha record and
                // a clip unit's base-only snapshot. A conservative admission
                // bound — the plan has no planes yet, so a
                // `blend_interior_as_group` flag is charged whether or not one
                // of its four contributions turns out to be enabled.
                let routed = layer.style.as_deref().is_some_and(|s| {
                    s.knockout != crate::style::Knockout::None || s.blend_interior_as_group
                });
                let grouped = layer
                    .style
                    .as_deref()
                    .is_some_and(|s| s.blend_interior_as_group);
                let knockout_member = members_want_base_snapshot16(&layer.clipped_members);
                let knockout = knockout_scratch(routed, grouped, knockout_member, dimensions);
                if let Some(mask) = &layer.mask {
                    mask.record_resources(inventory);
                    let (scratch, mask_work) = mask.plan()?;
                    *mask_scratch = (*mask_scratch).max(scratch);
                    *work = work.checked_add(mask_work).ok_or("mask work overflow")?;
                }
                // A styled ADJUSTMENT — any style, or a Blend If under
                // an identity one (`render_styled`) — pays one pass over the
                // frame for its interior and is charged the 16 B/px frame a
                // knockout or grouped interior's unit pass holds
                // (`Adjusted16::interior_frame`).
                let adjusted = matches!(layer.kind, Kind::Adjustment(_)) && layer.renders_styled();
                let identity = LayerStyle::default();
                let styled = match layer.style.as_deref() {
                    Some(style) => Some(style),
                    None if adjusted => Some(&identity),
                    None => None,
                };
                if let Some(style) = styled {
                    // What the u8 renderer allocates for this layer's planes
                    // (a HARD gate here, unlike the byte reporting bound),
                    // the Blend If snapshot and the package copy.
                    let plane = match &layer.kind {
                        Kind::Raster { image, .. } => image.dimensions(),
                        _ => dimensions,
                    };
                    let package = (layer.opacity < 1.0
                        && layer.blend_mode != BlendMode::Dissolve
                        && style.has_enabled_effects())
                    .then_some(dimensions);
                    let (scratch, style_work) =
                        style_scratch(style, plane, layer.blend_if.is_some(), package)?;
                    let (frame, frame_work) = if adjusted {
                        (count.saturating_mul(16), count)
                    } else {
                        (0, 0)
                    };
                    let scratch = scratch.saturating_add(frame);
                    *style_scratch_peak = (*style_scratch_peak)
                        .max(style_held.saturating_add(scratch).saturating_add(knockout));
                    member_held = style_held.saturating_add(scratch);
                    *work = work
                        .checked_add(style_work)
                        .and_then(|w| w.checked_add(frame_work))
                        .ok_or("precision work count overflow")?;
                }
                if !layer.clipped_members.is_empty() {
                    // The clip buffer itself is one plane charged through
                    // `group_depth`; each live clip level also holds its
                    // base's recorded alpha. Zero, alpha record and the final
                    // composite are three passes over the canvas.
                    let levels = clip_levels + 1;
                    *clip_scratch =
                        (*clip_scratch).max((levels as u64 * count * 4).saturating_add(knockout));
                    *work = work
                        .checked_add(count * 3)
                        .ok_or("precision work count overflow")?;
                    collect(
                        &layer.clipped_members,
                        dimensions,
                        origin,
                        inventory,
                        work,
                        guide_scratch,
                        mask_scratch,
                        levels,
                        clip_scratch,
                        style_scratch_peak,
                        member_held,
                    )?;
                }
                let added = match &layer.kind {
                    Kind::Raster { image, offset } => {
                        image.record_resources(inventory);
                        let (x0, y0, x1, y1) =
                            intersection_at(image.dimensions(), *offset, origin, dimensions);
                        (x1 - x0) * (y1 - y0)
                    }
                    Kind::Adjustment(state) => {
                        state.record_resources(inventory);
                        *guide_scratch = (*guide_scratch).max(
                            state
                                .kernel
                                .guide_scratch_bytes(dimensions.0, dimensions.1)?,
                        );
                        u64::from(dimensions.0) * u64::from(dimensions.1)
                            + state.kernel.guide_work_units(dimensions.0, dimensions.1)?
                    }
                    Kind::Group(children) | Kind::PassThrough(children) => {
                        collect(
                            children,
                            dimensions,
                            origin,
                            inventory,
                            work,
                            guide_scratch,
                            mask_scratch,
                            clip_levels,
                            clip_scratch,
                            style_scratch_peak,
                            // A styled group's planes are built AFTER its
                            // children have rendered and released theirs.
                            style_held,
                        )?;
                        u64::from(dimensions.0) * u64::from(dimensions.1) * 2
                    }
                };
                *work = work
                    .checked_add(added)
                    .ok_or("precision work count overflow")?;
            }
            Ok(())
        }
        collect(
            &self.layers,
            (self.width, self.height),
            self.origin,
            &mut inventory,
            &mut work,
            &mut guide_scratch,
            &mut mask_scratch,
            0,
            &mut clip_scratch,
            &mut style_scratch_peak,
            0,
        )?;
        let output_bytes = count * storage.bytes_per_pixel();
        // D5's ONE Background snapshot, charged once for the whole render
        // (`precision_style_knockout`); zero for every scene with no knockout.
        let seed_scratch = match self.knockout_seed {
            crate::precision_style_knockout::KnockoutSeed::Projection { background } => {
                if background {
                    count * 16
                } else {
                    0
                }
            }
            // `Handed` planes are RESERVED by the merge that captured them
            // (`precision_merge::merge_scratch_bytes`), not here.
            crate::precision_style_knockout::KnockoutSeed::Handed { .. } => 0,
        };
        let nested = count * 16 * (self.group_depth as u64 + 1);
        // A u16 result overlaps the top accumulator while converting; a float
        // result adopts the accumulator and then tiles it, so both are held
        // (`F32_FINALIZATION_BYTES_PER_PIXEL`). Group buffers have dropped
        // before either.
        let finalization = if storage == PrecisionStorage::Rgba16Unorm {
            count * 16 + output_bytes
        } else {
            count * F32_FINALIZATION_BYTES_PER_PIXEL
        };
        Ok(PrecisionRenderPlan {
            unique_retained_bytes: inventory.unique_bytes(),
            scratch_bytes: (nested
                + guide_scratch
                + mask_scratch
                + clip_scratch
                + style_scratch_peak
                + seed_scratch)
                .max(finalization)
                + BOOKKEEPING_BYTES,
            output_bytes,
            work_units: work,
            group_depth: self.group_depth,
        })
    }

    /// The scene's compositing environment. `capture` is `None` for every
    /// ordinary render and `Some` only for a level-backdrop capture
    /// (`precision_style_knockout`); `float_style` is the render's ONE
    /// [`FloatStyle`] on a float scene (`StyleEnv::float_style_for`).
    pub(crate) fn style_env<'a>(
        &'a self,
        colors: Option<&'a crate::authored_color::AuthoredColors>,
        float_style: Option<&'a FloatStyle>,
        capture: Option<&'a crate::precision_style_knockout::TypedCapture>,
    ) -> StyleEnv<'a> {
        StyleEnv {
            light: self.light,
            canvas: self.canvas,
            colors,
            transfer: self.linear_light.as_deref(),
            float: self.float,
            float_style,
            knockout: crate::style_knockout::KnockoutBackdrop::NONE,
            capture,
            lab: self.lab,
        }
    }

    /// The two `B0` planes the PROBE record's level held, captured by running
    /// this scene's own projection and abandoning it the moment the probe is
    /// reached (`precision_style_knockout`). Nothing is encoded or stored: the
    /// accumulator exists only to reach that level.
    pub(crate) fn capture_level_backdrop(
        &self,
        storage: PrecisionStorage,
        working_limit_bytes: u64,
        other_retained_bytes: u64,
        job: JobContext<'_>,
    ) -> Result<crate::precision_style_knockout::CapturedPlanes, String> {
        job.check()?;
        admit(
            0,
            BOOKKEEPING_BYTES,
            other_retained_bytes,
            working_limit_bytes,
        )?;
        job.reserve(BOOKKEEPING_BYTES)?;
        let plan = self.plan(storage)?;
        admit(
            plan.unique_retained_bytes,
            plan.scratch_bytes,
            other_retained_bytes,
            working_limit_bytes,
        )?;
        job.reserve(plan.scratch_bytes)?;
        let count = pixel_count(self.width, self.height)?;
        let mut progress = Progress {
            job,
            completed: 0,
            total: plan.work_units,
        };
        let mut acc = zero_plane(count, &mut progress)?;
        let colors = StyleEnv::colors_for(&self.profile);
        let float_style = StyleEnv::float_style_for(&self.profile, self.float)?;
        let capture = crate::precision_style_knockout::TypedCapture::new();
        let env = self.style_env(colors.as_ref(), float_style.as_ref(), Some(&capture));
        crate::precision_style_knockout::render_projection(
            &self.layers,
            (self.width, self.height),
            self.origin,
            &mut acc,
            &mut progress,
            &env,
            &self.knockout_seed,
        )?;
        drop(acc);
        Ok(capture.take())
    }

    pub fn render(
        &self,
        storage: PrecisionStorage,
        working_limit_bytes: u64,
        other_retained_bytes: u64,
        control: Option<&DrJobControl>,
    ) -> Result<PrecisionImage, String> {
        let _run = control.map(DrJobControl::begin).transpose()?;
        let job = control.map_or(JobContext::NONE, JobContext::new);
        self.render_with_context(storage, working_limit_bytes, other_retained_bytes, job)
    }

    pub(crate) fn render_with_context(
        &self,
        storage: PrecisionStorage,
        working_limit_bytes: u64,
        other_retained_bytes: u64,
        job: JobContext<'_>,
    ) -> Result<PrecisionImage, String> {
        job.check()?;
        // A metadata-only plan has bounded allocation too, even when final
        // raster admission will refuse. Charge that before creating Inventory.
        admit(
            0,
            BOOKKEEPING_BYTES,
            other_retained_bytes,
            working_limit_bytes,
        )?;
        job.reserve(BOOKKEEPING_BYTES)?;
        let plan = self.plan(storage)?;
        admit(
            plan.unique_retained_bytes,
            plan.scratch_bytes,
            other_retained_bytes,
            working_limit_bytes,
        )?;
        job.reserve(plan.scratch_bytes)?;
        let count = pixel_count(self.width, self.height)?;
        let mut progress = Progress {
            job,
            completed: 0,
            total: plan.work_units,
        };
        let mut acc = zero_plane(count, &mut progress)?;
        // ONE authored-colour conversion per render, however many styled
        // layers the scene holds (`DrDocument::style_colors`) — and on a
        // float scene ONE `FloatStyle` (`style_float`).
        let colors = StyleEnv::colors_for(&self.profile);
        let float_style = StyleEnv::float_style_for(&self.profile, self.float)?;
        let env = self.style_env(colors.as_ref(), float_style.as_ref(), None);
        // The scene's SEED decides whether D5 may establish a Background here
        // (`precision_style_knockout`); with no knockout in the scene this is
        // `render_layers` on the whole list, unchanged.
        crate::precision_style_knockout::render_projection(
            &self.layers,
            (self.width, self.height),
            self.origin,
            &mut acc,
            &mut progress,
            &env,
            &self.knockout_seed,
        )?;
        // The ONE re-encode of a linear-light render: the accumulator holds
        // LINEAR components, so it returns to the document's encoding here,
        // continuously (`TypedTransfer::encode`) — the existing
        // `round(v · 65535)` in the store below is still the only rounding.
        // Alpha is coverage and never passes through a curve.
        if let Some(transfer) = &self.linear_light {
            for pixel in acc.iter_mut() {
                let encoded = transfer.encode([pixel[0], pixel[1], pixel[2]]);
                pixel[..3].copy_from_slice(&encoded);
            }
            progress.job.check()?;
        }
        let output = if storage == PrecisionStorage::Rgba32Float {
            PrecisionImage::from_rendered_float_unseeded(
                self.width,
                self.height,
                self.profile.clone(),
                acc,
                self.float,
                job,
            )?
        } else {
            PrecisionImage::from_float_samples_unseeded(
                self.width,
                self.height,
                self.profile.clone(),
                storage,
                acc.into_iter(),
                job,
            )?
        };
        job.report(1.0, 5)?;
        Ok(output)
    }
}

pub(crate) fn intersection(
    source: (u32, u32),
    offset: (i32, i32),
    canvas: (u32, u32),
) -> (u64, u64, u64, u64) {
    intersection_at(source, offset, (0, 0), canvas)
}

/// Frame-local span of `source` at document `offset` in a frame of `frame`
/// whose pixel (0, 0) is document pixel `origin`; i64 inside, so no offset
/// difference can overflow.
pub(crate) fn intersection_at(
    source: (u32, u32),
    offset: (i32, i32),
    origin: (i32, i32),
    frame: (u32, u32),
) -> (u64, u64, u64, u64) {
    let rx = i64::from(offset.0) - i64::from(origin.0);
    let ry = i64::from(offset.1) - i64::from(origin.1);
    let x0 = rx.clamp(0, i64::from(frame.0));
    let y0 = ry.clamp(0, i64::from(frame.1));
    let x1 = (rx + i64::from(source.0)).clamp(x0, i64::from(frame.0));
    let y1 = (ry + i64::from(source.1)).clamp(y0, i64::from(frame.1));
    (x0 as u64, y0 as u64, x1 as u64, y1 as u64)
}

pub(crate) struct Progress<'a> {
    pub(crate) job: JobContext<'a>,
    pub(crate) completed: u64,
    pub(crate) total: u64,
}
impl Progress<'_> {
    pub(crate) fn advance(&mut self, count: usize) -> Result<(), String> {
        self.completed += count as u64;
        self.job
            .report(self.completed as f64 / self.total as f64, 6)
    }
}

pub(crate) fn zero_plane(
    count: usize,
    progress: &mut Progress<'_>,
) -> Result<Vec<[f32; 4]>, String> {
    let mut out = reserved_vec(count)?;
    while out.len() < count {
        let added = (count - out.len()).min(4096);
        progress.advance(added)?;
        out.extend(std::iter::repeat_n([0.0; 4], added));
    }
    progress.job.check()?;
    Ok(out)
}

/// `acc` is a frame of `dimensions` whose pixel `(x, y)` is document pixel
/// `(x + origin.0, y + origin.1)`: every mask, Dissolve and blend position is
/// read in document coordinates, every buffer and guide is frame-local.
pub(crate) fn render_layers(
    layers: &[PrecisionLayer],
    dimensions: (u32, u32),
    origin: (i32, i32),
    acc: &mut [[f32; 4]],
    progress: &mut Progress<'_>,
    env: &StyleEnv<'_>,
) -> Result<(), String> {
    let (gx, gy) = (i64::from(origin.0), i64::from(origin.1));
    for layer in layers {
        let kind = env.kind(layer.blend_mode);
        progress.job.check()?;
        // A merge's `B0` CAPTURE (`precision_style_knockout`): this LEVEL's
        // env is what the probe record would have read, and it is recorded
        // BEFORE any member substitution. The render is then abandoned —
        // nothing may work on a half-rendered buffer.
        if let Some(capture) = env.capture {
            if capture.is_filled() {
                return Ok(());
            }
            if layer.is_probe() {
                capture.fill(env.knockout);
                return Ok(());
            }
        }
        if layer.renders_styled() {
            // A styled raster, adjustment (or Blend-If adjustment),
            // isolated group or clip base takes the byte
            // `composite_styled_into` order; every unstyled arm below is
            // untouched.
            render_styled(layer, dimensions, origin, acc, progress, env)?;
            continue;
        }
        if !layer.clipped_members.is_empty() {
            // The byte clip-group rule (`doc.rs::composite_clip_group_into`):
            // the base renders into a transparent buffer at full opacity in
            // Normal (its own mask applies there), its alpha footprint is
            // recorded, each member composites through the ordinary kernel
            // onto the base with its alpha frozen (`clip_unit`'s Open /
            // Settle / Close, #578), and the finished buffer composites once
            // with the base's mode and opacity — the `Kind::Group` arm's
            // closing loop without a mask.
            let mut buf = zero_plane(acc.len(), progress)?;
            let full = PrecisionLayer {
                opacity: 1.0,
                blend_mode: BlendMode::Normal,
                clipped_members: Arc::new(Vec::new()),
                blend_if: None,
                style: None,
                // The unit's probe was already matched at the loop top, so the
                // base must not re-offer.
                knockout_probe: false,
                ..layer.clone()
            };
            render_layers(
                std::slice::from_ref(&full),
                dimensions,
                origin,
                &mut buf,
                progress,
                env,
            )?;
            if env.capture.is_some_and(TypedCapture::is_filled) {
                return Ok(());
            }
            // The footprint is the base's UNWEIGHTED alpha × mask (the byte
            // `Shape::of_layer` rule): recorded before any Blend If weight.
            let mut base_alpha: Vec<f32> = reserved_vec(buf.len())?;
            for chunk in buf.chunks(4096) {
                progress.advance(chunk.len())?;
                base_alpha.extend(chunk.iter().map(|pixel| pixel[3]));
            }
            if let Some(blend_if) = &layer.blend_if {
                // The base's Blend If reads its rendered colour against the
                // accumulator beneath the unit and weights only the base's
                // own alpha (the byte `composite_styled_into` clip path).
                for (start, chunk) in buf.chunks_mut(4096).enumerate() {
                    progress.job.check()?;
                    for (j, pixel) in chunk.iter_mut().enumerate() {
                        if pixel[3] > 0.0 {
                            let under = acc[start * 4096 + j];
                            pixel[3] *= env.blend_if_weight(
                                blend_if,
                                [pixel[0], pixel[1], pixel[2]],
                                [under[0], under[1], under[2]],
                            );
                        }
                    }
                }
            }
            // `clip_unit`'s Open: the Blend If weight stays RELATIVE to the
            // footprint. Then D4's clipped-member rule, the UNSTYLED base's
            // loop: a member with a knockout punches to this buffer as it
            // stands after the base alone rendered (opened). The clone
            // happens only when a member asks.
            clip_pass(&mut buf, &base_alpha, ClipStep::Open);
            let base_only =
                members_want_base_snapshot16(&layer.clipped_members).then(|| buf.clone());
            let member_env = StyleEnv {
                knockout: crate::style_knockout::KnockoutBackdrop {
                    shallow: base_only.as_deref(),
                    deep: base_only.as_deref(),
                },
                ..*env
            };
            for member in layer.clipped_members.iter() {
                render_layers(
                    std::slice::from_ref(member),
                    dimensions,
                    origin,
                    &mut buf,
                    progress,
                    &member_env,
                )?;
                if env.capture.is_some_and(TypedCapture::is_filled) {
                    return Ok(());
                }
                clip_pass(&mut buf, &base_alpha, ClipStep::Settle);
            }
            clip_pass(&mut buf, &base_alpha, ClipStep::Close);
            for start in (0..buf.len()).step_by(4096) {
                let end = (start + 4096).min(buf.len());
                for (i, pixel) in buf.iter().enumerate().take(end).skip(start) {
                    let alpha = checked_alpha(pixel[3], layer.opacity, 1.0)?;
                    if alpha > 0.0 {
                        composite_source_into(
                            acc,
                            i,
                            [pixel[0], pixel[1], pixel[2]],
                            alpha,
                            kind,
                            (
                                (i % dimensions.0 as usize) as i64 + gx,
                                (i / dimensions.0 as usize) as i64 + gy,
                            ),
                        );
                    }
                }
                progress.advance(end - start)?;
            }
            continue;
        }
        match &layer.kind {
            Kind::Raster { image, offset } => {
                let (x0, y0, x1, y1) =
                    intersection_at(image.dimensions(), *offset, origin, dimensions);
                if x0 == x1 || y0 == y1 {
                    continue;
                }
                let mask = prepare_mask(layer, progress)?;
                // `x0..x1` is `intersection_at`'s span, so every chunk's
                // layer-space columns lie in `[0, image width)`. `(ox, oy)` is
                // the layer's FRAME-local offset.
                let (ox, oy) = (i64::from(offset.0) - gx, i64::from(offset.1) - gy);
                for y in y0..y1 {
                    let ly = (y as i64 - oy) as u32;
                    for start in (x0..x1).step_by(4096) {
                        let end = (start + 4096).min(x1);
                        let (lx0, lx1) = ((start as i64 - ox) as u32, (end as i64 - ox) as u32);
                        for (lx, run) in image.row_segments(ly, lx0, lx1) {
                            for j in 0..run.len() {
                                let x = (i64::from(lx) + j as i64 + ox) as u64;
                                let pixel = run.pixel(j);
                                let coverage = mask
                                    .as_ref()
                                    .map_or(1.0, |m| m.at(x as i64 + gx, y as i64 + gy));
                                let i = (y * u64::from(dimensions.0) + x) as usize;
                                composite_raster_sample16(
                                    acc,
                                    i,
                                    pixel,
                                    layer,
                                    coverage,
                                    kind,
                                    env,
                                    (x as i64 + gx, y as i64 + gy),
                                )?;
                            }
                        }
                        progress.advance((end - start) as usize)?;
                    }
                }
            }
            Kind::Adjustment(state) => {
                let guide_work = state.kernel.guide_work_units(dimensions.0, dimensions.1)?;
                // An identity kernel is only a no-op under Normal: under any
                // other mode the adjusted colour (= the backdrop) is still
                // pushed through the blend function against that backdrop, so
                // the skip must not swallow it. `opacity == 0.0` stays a no-op
                // on every mode (`k = 0`, Dissolve included).
                if layer.opacity == 0.0
                    || (state.kernel.is_identity() && layer.blend_mode == BlendMode::Normal)
                {
                    progress.advance(acc.len() + guide_work as usize)?;
                    continue;
                }
                let start = progress.completed as f64 / progress.total as f64;
                let end = (progress.completed + guide_work) as f64 / progress.total as f64;
                let guide = state.kernel.guide(
                    dimensions.0,
                    dimensions.1,
                    &|i| acc[i],
                    progress.job.range(start, end),
                )?;
                progress.advance(guide_work as usize)?;
                let mask = prepare_mask(layer, progress)?;
                for (start, chunk) in acc.chunks_mut(4096).enumerate() {
                    for (j, pixel) in chunk.iter_mut().enumerate() {
                        if pixel[3] == 0.0 {
                            continue;
                        }
                        let i = start * 4096 + j;
                        let x = (i % dimensions.0 as usize) as u32;
                        let y = (i / dimensions.0 as usize) as u32;
                        let g = guide.as_ref().map_or(0.0, |guide| guide.at(x, y));
                        let k = layer.opacity
                            * mask
                                .as_ref()
                                .map_or(1.0, |m| m.at(i64::from(x) + gx, i64::from(y) + gy));
                        adjust_backdrop_sample16(
                            pixel,
                            state,
                            g,
                            k,
                            kind,
                            env,
                            (i64::from(x) + gx, i64::from(y) + gy),
                        )?;
                    }
                    progress.advance(chunk.len())?;
                }
            }
            Kind::Group(children) => {
                let mut group = zero_plane(acc.len(), progress)?;
                // D4: an ISOLATED group's buffer is a HARD STOP for both
                // knockout modes. It starts transparent, so a hole inside it
                // shows what lies beneath the GROUP once the group composites.
                let inner = StyleEnv {
                    knockout: crate::style_knockout::KnockoutBackdrop::NONE,
                    ..*env
                };
                render_layers(children, dimensions, origin, &mut group, progress, &inner)?;
                if env.capture.is_some_and(TypedCapture::is_filled) {
                    return Ok(());
                }
                let mask = prepare_mask(layer, progress)?;
                for start in (0..group.len()).step_by(4096) {
                    let end = (start + 4096).min(group.len());
                    for (i, pixel) in group.iter().enumerate().take(end).skip(start) {
                        let coverage = mask.as_ref().map_or(1.0, |m| {
                            m.at(
                                (i % dimensions.0 as usize) as i64 + gx,
                                (i / dimensions.0 as usize) as i64 + gy,
                            )
                        });
                        composite_group_sample16(
                            acc,
                            i,
                            *pixel,
                            layer,
                            coverage,
                            kind,
                            env.float,
                            (
                                (i % dimensions.0 as usize) as i64 + gx,
                                (i / dimensions.0 as usize) as i64 + gy,
                            ),
                        )?;
                    }
                    progress.advance(end - start)?;
                }
            }
            Kind::PassThrough(children) => {
                let mut evaluated = reserved_vec(acc.len())?;
                for chunk in acc.chunks(4096) {
                    progress.advance(chunk.len())?;
                    evaluated.extend_from_slice(chunk);
                }
                // D4: SHALLOW inside a pass-through group punches to the
                // accumulator as it stood when the children began — and the
                // children render into `evaluated`, so `acc` IS that state and
                // nothing is allocated. Deep rides through unchanged: a
                // pass-through group is not a boundary for it.
                let child_env = StyleEnv {
                    knockout: crate::style_knockout::KnockoutBackdrop {
                        shallow: Some(&*acc),
                        deep: env.knockout.deep,
                    },
                    ..*env
                };
                render_layers(
                    children,
                    dimensions,
                    origin,
                    &mut evaluated,
                    progress,
                    &child_env,
                )?;
                if env.capture.is_some_and(TypedCapture::is_filled) {
                    return Ok(());
                }
                let mask = prepare_mask(layer, progress)?;
                for start in (0..acc.len()).step_by(4096) {
                    let end = (start + 4096).min(acc.len());
                    for i in start..end {
                        let coverage = mask.as_ref().map_or(1.0, |m| {
                            m.at(
                                (i % dimensions.0 as usize) as i64 + gx,
                                (i / dimensions.0 as usize) as i64 + gy,
                            )
                        });
                        gate_pass_through_sample16(
                            &mut acc[i],
                            evaluated[i],
                            layer.opacity,
                            coverage,
                        )?;
                    }
                    progress.advance(end - start)?;
                }
            }
        }
    }
    progress.job.check()
}

/// One sample of `render_layers`' `Kind::Raster` arm — the MASTER per-pixel
/// body, moved here unchanged so the regional compositor
/// (`precision_regional`) runs the same f32 statements in the same order:
/// `pixel` is the stored source sample, `coverage` the prepared mask's value
/// (1.0 without one), `i` the accumulator index and `xy` the canvas position.
#[allow(clippy::too_many_arguments)]
#[inline]
pub(crate) fn composite_raster_sample16(
    acc: &mut [[f32; 4]],
    i: usize,
    pixel: [f32; 4],
    layer: &PrecisionLayer,
    coverage: f32,
    kind: BlendKind,
    env: &StyleEnv<'_>,
    xy: (i64, i64),
) -> Result<(), String> {
    let mut alpha = checked_alpha(pixel[3], layer.opacity, coverage)?;
    // Under `linear_light` the accumulator holds LINEAR
    // components, so the stored sample is decoded here
    // — the typed twin of `doc::composite_layer_into`'s
    // one decode site. `None` leaves it exactly as read.
    let cs = env
        .transfer
        .map_or([pixel[0], pixel[1], pixel[2]], |transfer| {
            transfer.decode([pixel[0], pixel[1], pixel[2]])
        });
    if alpha > 0.0 {
        if let Some(blend_if) = &layer.blend_if {
            let under = acc[i];
            alpha *= env.blend_if_weight(
                blend_if,
                [pixel[0], pixel[1], pixel[2]],
                [under[0], under[1], under[2]],
            );
        }
    }
    if alpha > 0.0 {
        composite_source_into(acc, i, cs, alpha, kind, xy);
    }
    Ok(())
}

/// One sample of `render_layers`' `Kind::Group` arm's closing loop — the
/// MASTER per-pixel body, moved here unchanged so the regional compositor
/// (`precision_regional_tree`) runs the same f32 statements in the same
/// order: `pixel` is the group buffer's sample (already composited
/// components, so nothing is decoded), `coverage` the group's prepared mask
/// value (1.0 without one), `i` the accumulator index and `xy` the canvas
/// position; `float` picks the float Blend If weight (`blend_if_weight_at`).
#[allow(clippy::too_many_arguments)]
#[inline]
pub(crate) fn composite_group_sample16(
    acc: &mut [[f32; 4]],
    i: usize,
    pixel: [f32; 4],
    layer: &PrecisionLayer,
    coverage: f32,
    kind: BlendKind,
    float: bool,
    xy: (i64, i64),
) -> Result<(), String> {
    let mut alpha = checked_alpha(pixel[3], layer.opacity, coverage)?;
    if alpha > 0.0 {
        if let Some(blend_if) = &layer.blend_if {
            let under = acc[i];
            alpha *= blend_if_weight_at(
                blend_if,
                [pixel[0], pixel[1], pixel[2]],
                [under[0], under[1], under[2]],
                float,
            );
        }
    }
    if alpha > 0.0 {
        composite_source_into(acc, i, [pixel[0], pixel[1], pixel[2]], alpha, kind, xy);
    }
    Ok(())
}

/// One sample of `render_layers`' `Kind::PassThrough` arm's gate — the
/// MASTER per-pixel body, moved here unchanged so the regional compositor
/// (`precision_regional_tree`) runs the same statements: `evaluated` is the
/// children's result over a copy of `out`, gated back by `opacity ·
/// coverage`.
#[inline]
pub(crate) fn gate_pass_through_sample16(
    out: &mut [f32; 4],
    evaluated: [f32; 4],
    opacity: f32,
    coverage: f32,
) -> Result<(), String> {
    // A positive contribution over transparent backdrop
    // must obey the same alpha floor as isolated groups.
    if out[3] == 0.0 {
        checked_alpha(evaluated[3], opacity, coverage)?;
    }
    gate_source_into(out, evaluated, opacity * coverage);
    Ok(())
}

/// One backdrop sample of `render_layers`' `Kind::Adjustment` arm — the
/// MASTER per-pixel body, moved here unchanged so the regional compositor
/// (`precision_regional`) runs the same f32 statements in the same order. The
/// caller has already skipped a transparent backdrop; `guide` is this pixel's
/// guide value (0.0 for a guide-less op) and `k = opacity * coverage`.
#[inline]
pub(crate) fn adjust_backdrop_sample16(
    pixel: &mut [f32; 4],
    state: &PrecisionAdjustmentState,
    guide: f32,
    k: f32,
    kind: BlendKind,
    env: &StyleEnv<'_>,
    xy: (i64, i64),
) -> Result<(), String> {
    let cb = [pixel[0], pixel[1], pixel[2]];
    // The TRANSCODE rule (`doc_evaluation`): a pointwise
    // kernel is defined on ENCODED numbers, so under
    // `linear_light` the backdrop is re-encoded for it —
    // continuously at this depth, so nothing is quantized —
    // and its answer decoded straight back. The `k` lerp
    // below then happens in linear.
    let adjusted = match env.transfer {
        // A FLOAT scene evaluates the calibrated kernel
        // in LINEAR light directly (`adjust_precision::
        // apply_linear`): there is no encoding to
        // transcode through and no [0,1] to clamp to.
        None if env.float => state.kernel.apply_linear(cb, xy)?,
        None => state.kernel.apply_at(cb, guide, xy),
        Some(transfer) => transfer.decode(state.kernel.apply_at(transfer.encode(cb), guide, xy)),
    };
    // The kernel's own output is checked HERE, before any
    // blend branch: a typed-only contract with no byte twin,
    // so every path to the accumulator (including the
    // Dissolve shown branch) keeps it.
    if adjusted.iter().any(|c| !c.is_finite()) {
        return Err("high-depth adjustment produced nonfinite RGB".into());
    }
    // The byte `doc.rs::composite_adjustment_into` rule: the
    // adjusted colour is pushed through this layer's mode as
    // the SOURCE against the backdrop (Normal's `b_normal`
    // returns `cs`, so Normal is bit-identical to a plain
    // mix), then lerped back by `k = opacity * coverage`.
    // Dissolve has no blend function and keeps its
    // canvas-absolute all-or-nothing dither; alpha never
    // changes on either path.
    if let BlendKind::Dissolve = kind {
        if dissolve_threshold(xy.0, xy.1) < k {
            pixel[..3].copy_from_slice(&adjusted);
        }
        return Ok(());
    }
    let effective = match kind {
        BlendKind::Separable(f) => [
            f(cb[0], adjusted[0]),
            f(cb[1], adjusted[1]),
            f(cb[2], adjusted[2]),
        ],
        BlendKind::NonSeparable(f) => f(cb, adjusted),
        BlendKind::Dissolve => unreachable!("dissolve handled above"),
    };
    if effective.iter().any(|c| !c.is_finite()) {
        return Err("high-depth adjustment produced nonfinite RGB".into());
    }
    mix_adjusted_rgb(pixel, effective, k);
    Ok(())
}

/// A styled raster, adjustment or isolated group (a clip base or not)
/// through `precision_style_render::composite_styled_into`: the pixels, the
/// prepared mask and the clipped members' renderer are handed over, the order
/// and the per-member clamp are its. An ADJUSTMENT hands over its
/// opaque shape and its interior hook (`precision_style_adjustment`), and one
/// with only a Blend If renders under an identity `LayerStyle`, so its
/// clipped members follow the byte styled-base rule.
fn render_styled(
    layer: &PrecisionLayer,
    dimensions: (u32, u32),
    origin: (i32, i32),
    acc: &mut [[f32; 4]],
    progress: &mut Progress<'_>,
    env: &StyleEnv<'_>,
) -> Result<(), String> {
    let identity;
    let style = match (layer.style.as_deref(), &layer.kind) {
        (Some(style), _) => style,
        (None, Kind::Adjustment(_)) if layer.blend_if.is_some() => {
            identity = LayerStyle::default();
            &identity
        }
        _ => return Err("unstyled layer on the styled path".into()),
    };
    let job = progress.job;
    let members = &layer.clipped_members;
    let plane = match &layer.kind {
        Kind::Raster { image, .. } => image.dimensions(),
        _ => dimensions,
    };
    let group = match &layer.kind {
        Kind::Group(children) => {
            let mut group = zero_plane(acc.len(), progress)?;
            // A STYLED group is isolated, so its buffer is D4's hard stop too.
            let inner = StyleEnv {
                knockout: crate::style_knockout::KnockoutBackdrop::NONE,
                ..*env
            };
            render_layers(children, dimensions, origin, &mut group, progress, &inner)?;
            if env.capture.is_some_and(TypedCapture::is_filled) {
                return Ok(());
            }
            Some(group)
        }
        Kind::Raster { .. } | Kind::Adjustment(_) => None,
        Kind::PassThrough(_) => return Err(UNSTYLED_KIND.into()),
    };
    // A styled ADJUSTMENT (`precision_style_adjustment`): its shape is
    // the frame under its mask (S-MASK) and its interior is applied at step 3,
    // after its own Below effects (POST). The Shadows/Highlights guide is
    // built here, from `acc` before the layer draws anything, as the unstyled
    // arm builds it.
    let guide = match &layer.kind {
        Kind::Adjustment(state) => {
            let guide_work = state.kernel.guide_work_units(dimensions.0, dimensions.1)?;
            let start = progress.completed as f64 / progress.total as f64;
            let end = (progress.completed + guide_work) as f64 / progress.total as f64;
            let guide = state.kernel.guide(
                dimensions.0,
                dimensions.1,
                &|i| acc[i],
                progress.job.range(start, end),
            )?;
            progress.advance(guide_work as usize)?;
            guide
        }
        _ => None,
    };
    let adjusted = match &layer.kind {
        Kind::Adjustment(state) => Some(crate::precision_style_adjustment::Adjusted16 {
            state,
            guide: guide.as_ref(),
        }),
        _ => None,
    };
    let mask = prepare_mask(layer, progress)?;
    let pixels = match (&layer.kind, &group) {
        (Kind::Raster { image, offset }, _) => StyledPixels::Image {
            image,
            offset: *offset,
        },
        (_, Some(group)) => StyledPixels::Buffer {
            pixels: group,
            width: dimensions.0,
            height: dimensions.1,
            origin,
        },
        (Kind::Adjustment(_), None) => StyledPixels::Opaque {
            width: dimensions.0,
            height: dimensions.1,
            origin,
        },
        _ => return Err("unstyled layer on the styled path".into()),
    };
    let base_work = match &layer.kind {
        Kind::Raster { image, offset } => {
            let (x0, y0, x1, y1) = intersection_at(image.dimensions(), *offset, origin, dimensions);
            ((x1 - x0) * (y1 - y0)) as usize
        }
        _ => acc.len(),
    };
    let styled = StyledLayer {
        pixels,
        mask: mask.as_ref(),
        style,
        blend_if: layer.blend_if.as_ref(),
        blend: layer.blend_mode,
        opacity: layer.opacity,
        members_want_base: members_want_base_snapshot16(members),
        adjusted: adjusted.as_ref(),
    };
    // The planes report inside the work the plan charged (13A-5b-iii-c-2,
    // `prepare_style`'s rule): `completed/total → (completed + work)/total`,
    // then the advances below. A failing charge leaves the range empty and
    // fails below, where it always did.
    let start = progress.completed as f64 / progress.total as f64;
    let end = style_scratch(style, plane, false, None).map_or(start, |(_, work)| {
        (progress.completed + work) as f64 / progress.total as f64
    });
    let planes_job = job.range(start, end);
    {
        // D4's clipped-member rule, the STYLED base's loop: the unit hands its
        // base-only snapshot down as BOTH planes.
        let mut render_member = |i: usize, buf: &mut [[f32; 4]], base_only: Option<&[[f32; 4]]>| {
            let member_env = StyleEnv {
                knockout: crate::style_knockout::KnockoutBackdrop {
                    shallow: base_only,
                    deep: base_only,
                },
                ..*env
            };
            render_layers(
                std::slice::from_ref(&members[i]),
                dimensions,
                origin,
                buf,
                progress,
                &member_env,
            )
        };
        let handed = (!members.is_empty()).then_some((
            members.len(),
            &mut render_member
                as &mut dyn FnMut(
                    usize,
                    &mut [[f32; 4]],
                    Option<&[[f32; 4]]>,
                ) -> Result<(), String>,
        ));
        composite_styled_into(acc, dimensions, origin, &styled, env, handed, planes_job)?;
    }
    if env.capture.is_some_and(TypedCapture::is_filled) {
        return Ok(());
    }
    let package = (layer.opacity < 1.0
        && layer.blend_mode != BlendMode::Dissolve
        && style.has_enabled_effects())
    .then_some(dimensions);
    let (_, style_work) = style_scratch(style, plane, layer.blend_if.is_some(), package)?;
    progress.advance(base_work)?;
    if adjusted.is_some() {
        // The interior's pass over the frame (the plan's frame work).
        progress.advance(acc.len())?;
    }
    progress.advance(style_work as usize)?;
    if !members.is_empty() {
        progress.advance(acc.len() * 3)?;
    }
    Ok(())
}

pub(crate) fn prepare_mask<'a>(
    layer: &'a PrecisionLayer,
    progress: &mut Progress<'_>,
) -> Result<Option<PreparedMask<'a>>, String> {
    let Some(mask) = &layer.mask else {
        return Ok(None);
    };
    let work = mask.plan()?.1;
    let job = progress.job.range(
        progress.completed as f64 / progress.total as f64,
        (progress.completed + work) as f64 / progress.total as f64,
    );
    let prepared = mask.prepare(job)?;
    progress.advance(work as usize)?;
    Ok(Some(prepared))
}
