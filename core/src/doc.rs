//! Layered document model: `DrDocument` (canvas + bottom-to-top layer
//! stack), the f32 compositing projection, document ops, and per-layer
//! masks. See include/doctored_core.h for the contract. The blend-mode
//! table and blend math live in `blend`, the DRDC native format in `drdc`,
//! and layered PSD import in `psd`. Layer styles (the effect stack, fill
//! opacity, Blend If, the global light) live in `style`, are rendered by
//! `style_render` and composited by `style_composite`; this module only
//! routes styled layers there. LAYER GROUPS — the nesting marked on each
//! entry, the invariant that keeps it well formed, and the recursive
//! compositor this module's projection wires up — live in `doc_group`, with
//! the per-entry geometry and link queries in `doc_group_query` and the lock
//! bits in `doc_lock`. Layer pixel and mask buffers are `Arc`-shared so
//! document copies are copy-on-write.
//!
//! A styled layer's compositing is routed out to `style_composite.rs`; `doc.rs` only wires it
//! (`CompositeEnv`, `merge_extent`).

use std::sync::Arc;

use image::imageops::{self, FilterType};
use image::{GenericImageView, GrayImage, ImageBuffer, Luma, Pixel, RgbaImage};

use crate::adjust::Adjustment;
use crate::blend::{
    blend_kind, composite_buffer_into, composite_source_into, dissolve_threshold, BlendKind,
};
use crate::clip_unit::{clip_pass, ClipStep};
use crate::comps_geometry::merged_comps;
use crate::doc_align;
use crate::doc_channel::{self, Channel};
use crate::doc_group;
use crate::doc_guide::{self, Guide};
use crate::doc_lock::EditKind;
use crate::doc_plane::channel_lerp;
use crate::doc_sampler;
use crate::icc::IccProfile;
use crate::metadata::{Metadata, Resolution};
use crate::style::{scaled_style, GlobalLight, LayerStyle};
use crate::style_composite::{composite_styled_into, merge_extent, CompositeEnv, Cut};
use crate::DrImage;

pub use crate::blend::BlendMode;
pub use crate::layer_raster::LayerRaster;

/// Largest permitted canvas or merged-extent size, in total pixels (matches
/// the `dr_image_resize` guard). The FFI applies it to caller-declared layer
/// buffer dimensions too, so absurd dimensions are refused rather than read.
pub(crate) const MAX_PIXELS: u64 = 100_000_000;

// ------------------------------------------------------------------ model --

/// What an entry in the stack IS. A GROUP is a container with no pixels of
/// its own; its children are the entries immediately BELOW it at greater
/// depth (see `doc_group`, whose module doc is the layout's specification).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(i32)]
pub enum LayerKind {
    Raster = 0,
    Group = 1,
}

impl LayerKind {
    /// Maps a raw `DrLayerKind` value coming across the FFI or out of a `.rz`
    /// record; `None` for anything else, which the reader refuses rather
    /// than repairs (a kind it cannot name is a structural claim, not a
    /// cosmetic one).
    pub fn from_c(value: i32) -> Option<Self> {
        match value {
            0 => Some(LayerKind::Raster),
            1 => Some(LayerKind::Group),
            _ => None,
        }
    }

    /// The `DrLayerKind` value for the FFI.
    pub fn to_c(self) -> i32 {
        self as i32
    }
}

/// One entry in the stack: either a raster layer — straight-alpha RGBA8
/// pixels of the layer's own size, an integer canvas offset, and display
/// properties — or a GROUP, which carries the same display properties over
/// the children below it and no pixels of its own. The pixel buffer is
/// immutable and shared (`Arc`), so cloning a layer is cheap.
#[derive(Clone)]
pub struct Layer {
    /// Stable identity, unique among every layer this process has minted —
    /// the handle a host hangs per-layer state on, since an INDEX is
    /// renumbered by every insert, delete, group, merge and reorder and a
    /// NAME is neither unique nor stable. Never 0, unique within its
    /// document, and PERSISTED in the `.rz` `LIDS` table since 12E-1
    /// (`identity_native`); the minting rule and the whole argument live in
    /// `doc_identity`.
    pub id: u64,
    /// Embedded ordinary source, guarded against destructive fallback changes.
    pub retained_source: Option<crate::retained_source::RetainedSource8>,
    /// Re-editable displacement and its immutable original planes.
    pub warp: Option<crate::warp::WarpBinding8>,
    /// A Generative Layer's variations, bound to the raster they rendered
    /// (`doc_gen_variations`); absent everywhere once not live.
    pub generated: Option<crate::doc_gen_variations::GeneratedBinding8>,
    /// Straight-alpha RGBA8 pixels, the layer's own size. Identity is
    /// `LayerRaster::key` / `LayerRaster::ptr_eq`, not an `Arc` address.
    pub pixels: LayerRaster,
    /// Canvas position of the layer's top-left pixel. On a GROUP, which has
    /// no pixels of its own, it is instead where its CANVAS-sized mask sits:
    /// a group's mask travels with the group as bookkeeping on this offset
    /// rather than as a resample per move, so a drag out and back restores it
    /// exactly (`doc_align::translate_entry`). It stays (0, 0) on a group
    /// with no mask.
    pub offset: (i32, i32),
    /// Display name.
    pub name: String,
    /// Opacity in [0, 1].
    pub opacity: f32,
    /// Blend mode used when compositing this layer onto the stack below.
    pub blend: BlendMode,
    /// Invisible layers are skipped by the projection.
    pub visible: bool,
    /// Original coverage bytes (0 hides, 255 shows), independently sized and
    /// placed by mask_properties. Density/feather never rewrite this plane.
    /// Shared (`Arc`) for the same copy-on-write reason as the pixels.
    pub mask: Option<Arc<GrayImage>>,
    /// When false the mask is retained but ignored while compositing.
    pub mask_enabled: bool,
    /// Independent raw storage placement, link and nondestructive coverage properties.
    pub mask_properties: crate::doc_mask::MaskProperties,
    /// Opaque per-layer metadata (a minimal "parasite"): the core stores,
    /// copies and serializes this JSON blob but never interprets it — with
    /// ONE exception: meta that parses as an [`Adjustment`] description
    /// (`{"type":"adjust", ...}`, see `adjust`) makes the compositor treat
    /// the layer as an adjustment layer.
    pub meta: Option<String>,
    /// Clipping-mask flag (PSD semantics): a clipped entry is confined
    /// to the alpha footprint of the first UNCLIPPED SIBLING beneath it —
    /// WITHIN ITS LEVEL — which is its base. Structure is purely positional:
    /// [`DrDocument::flattened`] re-derives both the nesting and the base +
    /// consecutive-clipped-run groups on every composite, so reordering or
    /// deleting entries needs no bookkeeping. A clipped entry at the bottom
    /// of its level (no unclipped sibling below) has no base and composites
    /// as if unclipped. Copied on duplicate like every other property.
    pub clipped: bool,
    /// Layer style (effects + blending options); see `style`. Rides along
    /// like meta, dropped by merge/flatten (which bake it), copied by
    /// duplicate, scaled by the transforms. Never an identity style (the
    /// setter clears those), so `Some` means "renders something".
    pub style: Option<Arc<LayerStyle>>,
    /// Raster or group. A GROUP's `pixels` is a private 1x1 fully
    /// transparent buffer that nothing ever reads: it exists so a group is
    /// a `Layer` (every FFI export addresses entries by index) and so its
    /// identity (`LayerRaster::key`) is unique — the layer-style plane cache
    /// keys on that identity (`style_cache::CacheKey::of`), and a shared
    /// dummy would make two styled groups render each other's effects.
    pub kind: LayerKind,
    /// Nesting depth: 0 at the top level, one more inside each enclosing
    /// group. See `doc_group` for the ONE invariant this participates in and
    /// for why that invariant is checked at runtime.
    pub depth: u16,
    /// Lock flags: bit 0 transparency, bit 1 pixels, bit 2 position (the
    /// positions PSD's `lspf` uses; `lspf` spells Lock All as bit 31 —
    /// `psd_locks`). All three set is "Lock All". Bits 3..31 are reserved
    /// and are masked off wherever a value enters. See `doc_lock`.
    pub locks: u32,
    /// Link group: 0 unlinked, otherwise every entry carrying the same value
    /// moves and transforms with this one. Ids are the smallest unused
    /// non-zero value, so a `.rz` round trip is byte-identical. See
    /// `doc_group_query`.
    pub link: u32,
    /// Whether the panel shows a GROUP expanded. Meaningless on a raster
    /// entry (kept `true` there). Document state so it survives a save.
    pub open: bool,
    /// Colour LABEL: 0 none, 1..7 the Photoshop `lclr` palette. Panel
    /// bookkeeping only — nothing composites differently — carried by every
    /// op like `link` and persisted in the `LLBL` table. See `doc_label`.
    pub label: u8,
    /// Photoshop's `fxrp`: (0, 0) at creation; moved by translations and mapped by Image Size,
    /// Canvas Size, Crop, the angled crop, turns, flips, a filtered Rotate Canvas and Free
    /// Transform (`reference_point`); carried by clone/duplicate/conversion; merges start at (0, 0).
    /// A linked Pattern Overlay set without an anchor anchors where the layer was MADE, offset −
    /// reference (`style_pattern::made_at`). Persisted in the `.rz` `LREF` table
    /// (`reference_native`) and the PSD `fxrp` (`psd_pattern_fill::record_reference`).
    pub reference: (i32, i32),
}

impl Layer {
    /// A plain RASTER layer at offset (0, 0), at the top level: fully
    /// opaque, Normal, visible, unclipped, unlocked, unlinked, with no mask
    /// and no meta. `pub(crate)` because `doc_structure`'s merge, stamp and
    /// Via Copy each end with exactly this layer — one constructor, so a new
    /// `Layer` field can never be forgotten at one of them.
    pub(crate) fn new(pixels: RgbaImage, name: &str) -> Self {
        Self::from_raster(LayerRaster::new(pixels), name)
    }

    /// [`Self::new`] over an already built raster (a temp document whose
    /// plane nothing reads whole passes an `unseeded` one).
    pub(crate) fn from_raster(pixels: LayerRaster, name: &str) -> Self {
        Layer {
            // A NEW picture, so a new identity (`doc_identity`). Every
            // insert, Via Copy, Stamp Visible and flatten reaches this line,
            // which is what makes "fresh entry, fresh id" true by
            // construction rather than by a rule each caller remembers.
            id: crate::doc_identity::mint_layer_id(),
            retained_source: None,
            warp: None,
            generated: None,
            pixels,
            offset: (0, 0),
            name: name.to_string(),
            opacity: 1.0,
            blend: BlendMode::Normal,
            visible: true,
            mask: None,
            mask_enabled: true,
            mask_properties: Default::default(),
            meta: None,
            clipped: false,
            style: None,
            kind: LayerKind::Raster,
            depth: 0,
            locks: 0,
            link: 0,
            open: true,
            label: 0,
            reference: (0, 0),
        }
    }

    /// Enabled effective coverage aligned to the raster pixel extent.
    pub(crate) fn active_mask(&self) -> Option<Arc<GrayImage>> {
        if !self.mask_enabled {
            return None;
        }
        self.aligned_mask()
    }
}

/// A layered document: canvas size plus an ordered layer stack
/// (index 0 = bottom). Always contains at least one ENTRY; an entry may be a
/// group, so a document may hold no pixels at all (an empty group composites
/// to a transparent canvas).
#[derive(Clone)]
pub struct DrDocument {
    /// Canvas width in pixels.
    pub width: u32,
    /// Canvas height in pixels.
    pub height: u32,
    /// Layer stack, bottom first. A FOREST, flattened: a group's children
    /// are the entries immediately below it at greater depth, and the group
    /// entry is the last record of its own subtree. `doc_group` owns the
    /// layout, its invariant and the walk.
    pub layers: Vec<Layer>,
    /// Shared light direction every "use global light" effect reads.
    pub global_light: GlobalLight,
    /// Named canvas-sized coverage planes — saved selections. See
    /// `doc_channel`; they never composite.
    pub channels: Vec<Channel>,
    /// Alignment guides — each an orientation and a canvas position, sorted
    /// and de-duplicated. See `doc_guide`; like channels they are canvas
    /// space, they never composite, and every geometry op moves them.
    pub guides: Vec<Guide>,
    /// The ruler zero point, in canvas coordinates; (0, 0) — the canvas's
    /// top-left — by default. See `doc_guide`: it moves the ruler's LABELS
    /// and nothing else, but it is a property of the picture's coordinate
    /// frame, so every geometry op carries it.
    pub ruler_origin: (f64, f64),
    /// The document's COLOUR SAMPLERS (persisted): up to ten
    /// pinned canvas pixels, in label order. See `doc_sampler`; like the
    /// guides they are canvas space, never composite, and every geometry op
    /// moves them (dropping one that leaves the canvas).
    pub(crate) color_samplers: Vec<crate::doc_sampler::ColorSampler>,
    /// The document's PATHS: named Bézier paths in canvas space. See
    /// `doc_path`; like the samplers they never composite and every geometry
    /// op moves them, but none is ever dropped.
    pub(crate) paths: crate::doc_path::DocPaths,
    /// The document's working colour space: the ICC profile its pixel
    /// numbers belong to, embedded on export. Defaults to the built-in
    /// sRGB and is never absent — an untagged file is assumed sRGB at open,
    /// which is what every reader does anyway and what gives the display,
    /// the export and the transform exactly one rule. See `doc_color`.
    pub profile: Arc<IccProfile>,
    /// EXIF / XMP / IPTC packets, verbatim and uninterpreted. See
    /// `metadata`.
    pub metadata: Metadata,
    /// Print resolution in ppi; 72 x 72 by default. Pixels never change with
    /// it — only the print size does. See `metadata::Resolution`.
    pub resolution: Resolution,
    /// The document's EVALUATION POLICY, as the transfer tables its
    /// projection composites through: `None` is `encoded` — every document
    /// before Phase 11E and still the default — and `Some` is `linear_light`.
    /// Built once per document and `Arc`-shared by every derived handle, so
    /// an undo stack of handles holds ONE copy. See `doc_evaluation`, which
    /// owns the policy, the walk that keeps it honest and the rebind.
    pub(crate) linear_light: Option<Arc<crate::doc_evaluation::ByteTransfer>>,
    /// The document's LAYER COMPS — named recordings of per-layer
    /// visibility, position and appearance, by stable identity and never
    /// pixels. See `doc_comps`, which owns every rule; `doc.rs` only carries
    /// the list through the ops that rebuild the document.
    pub(crate) comps: Vec<crate::doc_comps::LayerComp>,
    /// The BACKGROUND MARK (`doc_background`): whether the bottom layer is a
    /// Background or an ordinary layer, when the source file said so; `None`
    /// keeps D5's structural rule. It names a layer by id, so every op that
    /// rebuilds the document carries it verbatim.
    pub(crate) background: Option<crate::doc_background::BackgroundMark>,
}

/// Clamps opacity to [0, 1], mapping non-finite values to 1.
pub(crate) fn sane_opacity(opacity: f32) -> f32 {
    if opacity.is_finite() {
        opacity.clamp(0.0, 1.0)
    } else {
        1.0
    }
}

// ------------------------------------------------------------- projection --

/// Composites `layer` onto the straight-alpha f32 accumulator `acc` (size
/// `acc_w` x `acc_h`, whose top-left pixel sits at canvas coordinate
/// `origin`), using the W3C compositing formula with the layer's blend mode
/// and opacity. An enabled layer mask scales the source alpha per pixel
/// (`mask / 255`) on top of the layer opacity. Pixels outside the layer's
/// extent are untouched. The caller is responsible for visibility filtering.
///
/// A GROUP entry never reaches this function as itself: `doc_group` renders
/// an isolated group into a private buffer and hands the result over as a
/// synthetic raster layer, so everything below is written for pixels and
/// needs no group branch.
///
/// A layer whose meta parses as an [`Adjustment`] is routed to
/// [`composite_adjustment_into`] instead — every projection (flatten, render,
/// export, merge-down) goes through this function, so adjustment layers
/// behave identically everywhere. A layer carrying a style is routed to
/// `style_composite` (its effects rendered under `env`: the global light
/// and the document canvas). A STYLED adjustment layer (its style or Blend
/// If, `Layer::renders_adjustment_style`) is routed to `style_adjustment`,
/// which composites it through the same styled path with its mask over the
/// canvas as the shape and its adjustment applied in place after its own
/// Below effects; an unstyled one keeps [`composite_adjustment_into`] byte
/// for byte. (`env` also carries the
/// conversion an authored style colour takes into the document's colour
/// space — see `style_composite`, which applies it where a contribution's
/// colour is read.)
pub(crate) fn composite_layer_into(
    acc: &mut [[f32; 4]],
    acc_w: u32,
    acc_h: u32,
    origin: (i32, i32),
    layer: &Layer,
    env: CompositeEnv<'_>,
) -> Result<(), String> {
    env.job.check()?;
    let opacity = crate::opacity_byte::quantized(layer.opacity);
    if opacity <= 0.0 {
        return Ok(());
    }
    // A GROUP's meta is the host's alone: the core never interprets it, so
    // a group whose blob happened to parse as an adjustment cannot silently
    // become an adjustment layer. (Reached only defensively — the compositor
    // hands this function a group's SYNTHETIC layer, which carries no meta.)
    if layer.kind != LayerKind::Group {
        if let Some(adjustment) = layer
            .meta
            .as_deref()
            .and_then(Adjustment::from_meta)
            .and_then(|op| op.in_profile(env.profile).ok())
        {
            if let Some(style) = layer.renders_adjustment_style() {
                return crate::style_adjustment::composite_styled_adjustment_into(
                    acc,
                    acc_w,
                    acc_h,
                    origin,
                    layer,
                    &[],
                    &adjustment,
                    style,
                    opacity,
                    env,
                );
            }
            composite_adjustment_into(acc, acc_w, acc_h, origin, layer, &adjustment, opacity, env)?;
            return Ok(());
        }
    }
    if let Some(style) = layer.renders_style() {
        return composite_styled_into(
            acc,
            acc_w,
            acc_h,
            origin,
            layer,
            &[],
            style,
            opacity,
            env,
            None,
        );
    }
    let active_mask = layer.active_mask_with_job(env.job.checkpoints_only())?;
    let mask = active_mask.as_deref().map(|m| m.as_raw().as_slice());
    let kind = blend_kind(layer.blend);
    let (lw, lh) = layer.pixels.dimensions();
    let rel_x = i64::from(layer.offset.0) - i64::from(origin.0);
    let rel_y = i64::from(layer.offset.1) - i64::from(origin.1);
    let x0 = rel_x.max(0);
    let y0 = rel_y.max(0);
    let x1 = (rel_x + i64::from(lw)).min(i64::from(acc_w));
    let y1 = (rel_y + i64::from(lh)).min(i64::from(acc_h));
    // The layer-space span, computed once and only for a non-empty canvas
    // span: a layer right of the accumulator has `x1 < x0`, and no negative
    // may reach the `u32` cast. `(0, 0)` reads nothing, as `x0..x1` would.
    let (sx0, sx1) = if x0 < x1 {
        ((x0 - rel_x) as u32, (x1 - rel_x) as u32)
    } else {
        (0, 0)
    };
    for ay in y0..y1 {
        env.job.check()?;
        let ly = (ay - rel_y) as u64;
        for (sx, run) in layer.pixels.row_segments(ly as u32, sx0, sx1) {
            let raw = run.as_flattened();
            for i in 0..run.len() {
                let lx = sx as u64 + i as u64;
                let ax = lx as i64 + rel_x;
                let mi = (ly * u64::from(lw) + lx) as usize;
                let li = i * 4;
                let coverage = mask.map_or(1.0, |m| f32::from(m[mi]) / 255.0);
                let ai = (ay as u64 * u64::from(acc_w) + ax as u64) as usize;
                let canvas_xy = (ax + i64::from(origin.0), ay + i64::from(origin.1));
                composite_layer_sample(
                    acc,
                    ai,
                    raw,
                    li,
                    coverage,
                    opacity,
                    kind,
                    env.transfer,
                    canvas_xy,
                );
            }
        }
    }
    Ok(())
}

/// One sample of [`composite_layer_into`]'s loop — the MASTER per-pixel body,
/// moved here unchanged so the regional compositor (`doc_regional`) runs the
/// same f32 statements in the same order: the stored byte at `raw[li..]`,
/// scaled by `coverage` and `opacity`, decoded under `transfer` and composited
/// onto `acc[ai]` at `canvas_xy`.
#[allow(clippy::too_many_arguments)]
#[inline]
pub(crate) fn composite_layer_sample(
    acc: &mut [[f32; 4]],
    ai: usize,
    raw: &[u8],
    li: usize,
    coverage: f32,
    opacity: f32,
    kind: BlendKind,
    transfer: Option<&crate::doc_evaluation::ByteTransfer>,
    canvas_xy: (i64, i64),
) {
    let sa = f32::from(raw[li + 3]) / 255.0 * coverage * opacity;
    if sa <= 0.0 {
        return;
    }
    let cs = [
        f32::from(raw[li]) / 255.0,
        f32::from(raw[li + 1]) / 255.0,
        f32::from(raw[li + 2]) / 255.0,
    ];
    // Under `linear_light` the accumulator holds LINEAR components,
    // so the source is decoded here — the one place a stored byte
    // enters the projection (`doc_evaluation`). `None` is the encoded
    // policy and leaves the triple exactly as it was read.
    let cs = transfer.map_or(cs, |transfer| transfer.decode(cs));
    composite_source_into(acc, ai, cs, sa, kind, canvas_xy);
}

/// Composites an ADJUSTMENT layer onto the accumulator: the layer's own
/// pixels are ignored; instead the adjustment is applied to the accumulated
/// backdrop color (`adjusted = adjustment(backdrop)`, straight [0, 1] RGB),
/// pushed through the layer's blend mode as the SOURCE against the backdrop
/// (Normal therefore leaves `adjusted` as-is), and lerped back in by
/// `k = opacity * mask coverage`:
///
///   out_rgb = backdrop_rgb + (effective_rgb - backdrop_rgb) * k
///
/// Alpha is NEVER changed, and a fully transparent backdrop pixel is left
/// entirely untouched (a STYLED adjustment's interior, `style_adjustment`,
/// keeps the same rule). Coverage honors the layer's offset exactly as for
/// raster layers, but gates differently: with no mask (or a disabled one)
/// the adjustment reaches the WHOLE accumulator — an unmasked adjustment
/// layer is canvas-wide regardless of its own pixel extent — while an
/// enabled mask confines it (0 outside the mask's extent). Dissolve, which
/// has no blend function, keeps its all-or-nothing dither: each pixel is
/// fully adjusted with probability `k`, otherwise untouched.
#[allow(clippy::too_many_arguments)]
fn composite_adjustment_into(
    acc: &mut [[f32; 4]],
    acc_w: u32,
    acc_h: u32,
    origin: (i32, i32),
    layer: &Layer,
    adjustment: &Adjustment,
    opacity: f32,
    env: CompositeEnv<'_>,
) -> Result<(), String> {
    let transfer = env.transfer;
    let active_mask = layer.active_mask_with_job(env.job.checkpoints_only())?;
    let mask = active_mask.as_deref().map(|m| m.as_raw().as_slice());
    let kind = blend_kind(layer.blend);
    let (lw, lh) = layer.pixels.dimensions();
    let rel_x = i64::from(layer.offset.0) - i64::from(origin.0);
    let rel_y = i64::from(layer.offset.1) - i64::from(origin.1);
    // A SPATIAL adjustment (Shadows/Highlights) weights its lift by a
    // large-radius ALPHA-WEIGHTED blur of the backdrop's luma. The plane is
    // built ONCE here, from the accumulator, and read per pixel below; every
    // other op returns None and pays nothing (`adjust::Adjustment::guide`).
    // Under `linear_light` the guide reads the backdrop re-encoded, as the
    // kernel does (`doc_evaluation::guide_view`).
    let guide = adjustment.guide_with_job(
        acc_w,
        acc_h,
        &|i| crate::doc_evaluation::guide_view(acc[i], transfer),
        env.job.checkpoints_only(),
    )?;
    for ay in 0..i64::from(acc_h) {
        env.job.check()?;
        for ax in 0..i64::from(acc_w) {
            let coverage = match mask {
                None => 1.0,
                Some(m) => {
                    let lx = ax - rel_x;
                    let ly = ay - rel_y;
                    if lx < 0 || ly < 0 || lx >= i64::from(lw) || ly >= i64::from(lh) {
                        continue; // outside the mask's extent: coverage 0
                    }
                    f32::from(m[(ly as u64 * u64::from(lw) + lx as u64) as usize]) / 255.0
                }
            };
            let k = coverage * opacity;
            if k <= 0.0 {
                continue;
            }
            let ai = (ay as u64 * u64::from(acc_w) + ax as u64) as usize;
            let canvas_xy = (ax + i64::from(origin.0), ay + i64::from(origin.1));
            // 0.0 for a guide-less op: `apply_at`'s guide argument is only
            // ever read by an op whose `guide` returned a plane.
            let g = guide
                .as_ref()
                .map_or(0.0, |plane| plane.at(ax as u32, ay as u32));
            adjust_backdrop_sample(&mut acc[ai], adjustment, k, g, kind, transfer, canvas_xy);
        }
    }
    Ok(())
}

/// One backdrop sample of [`composite_adjustment_into`]'s loop — the MASTER
/// per-pixel body, moved here unchanged so the regional compositor
/// (`doc_regional`) runs the same f32 statements in the same order. `k` is
/// `coverage * opacity` (already > 0) and `guide` this pixel's guide value
/// (0.0 for a guide-less op).
#[inline]
pub(crate) fn adjust_backdrop_sample(
    px: &mut [f32; 4],
    adjustment: &Adjustment,
    k: f32,
    guide: f32,
    kind: BlendKind,
    transfer: Option<&crate::doc_evaluation::ByteTransfer>,
    canvas_xy: (i64, i64),
) {
    let bg = *px;
    if bg[3] <= 0.0 {
        return; // nothing to adjust; alpha (and color) stay exact
    }
    let cb = [bg[0], bg[1], bg[2]];
    // The TRANSCODE rule (`doc_evaluation`): a pointwise kernel is
    // defined on ENCODED numbers, so under `linear_light` the
    // backdrop is re-encoded for it — to a LATTICE value, which is
    // exactly what the encoded path shows the same kernel — and its
    // answer decoded straight back. The `k` lerp below then happens
    // in linear, which is the part the policy is for.
    let adjusted = match transfer {
        None => adjustment.apply_at(cb, guide, canvas_xy),
        Some(transfer) => {
            transfer.decode(adjustment.apply_at(transfer.encode_f32(cb), guide, canvas_xy))
        }
    };
    if let BlendKind::Dissolve = kind {
        let (cx, cy) = canvas_xy;
        if dissolve_threshold(cx, cy) < k {
            *px = [adjusted[0], adjusted[1], adjusted[2], bg[3]];
        }
        return;
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
    crate::blend::mix_adjusted_rgb(px, effective, k);
}

/// Composites a CLIP GROUP — `base` plus `group`, the consecutive run of
/// clipped layers stacked immediately above it — onto the accumulator. The
/// caller has already filtered the base's visibility (an invisible base hides
/// its whole group). With an EMPTY group this is exactly
/// [`composite_layer_into`] on the base: the group machinery never engages,
/// keeping the plain path byte-identical.
///
/// A non-empty group blends as one unit: the base renders into a private
/// transparent buffer at FULL opacity in Normal mode, its layer mask applied as usual, and the buffer's
/// alpha after that render — the base's footprint — is recorded. Each
/// visible clipped layer then composites into the buffer through the normal
/// kernel (its own opacity, blend mode, mask and offset; adjustment meta
/// routes through [`composite_adjustment_into`] with the buffer as
/// backdrop) onto the base with the base's alpha FROZEN — `clip_unit`'s
/// Open / Settle / Close around the members (#2, measured against
/// Photoshop: a member over a semi-transparent base mixes in at its own
/// strength), so
/// clipped layers never extend or shrink the footprint. The finished buffer
/// finally composites onto `acc` with the BASE layer's blend mode and
/// opacity (Dissolve keeps its canvas-absolute dither). Invisible clipped
/// layers are skipped.
///
/// One consequence of the base rendering into a TRANSPARENT buffer: an
/// UNSTYLED adjustment-meta base has no pixel footprint there (adjustments
/// never touch alpha, and a transparent backdrop is left untouched), so a
/// non-empty group over it contributes nothing. A STYLED base with a
/// non-empty group is routed to `style_composite`, which renders its below
/// effects once, the group as the interior unit, then its interior effects;
/// a STYLED adjustment base goes there with its shape (`style_adjustment`),
/// so its members are frozen to its mask over the canvas.
pub(crate) fn composite_clip_group_into(
    acc: &mut [[f32; 4]],
    acc_w: u32,
    acc_h: u32,
    origin: (i32, i32),
    base: &Layer,
    group: &[Layer],
    env: CompositeEnv<'_>,
) -> Result<(), String> {
    if group.is_empty() {
        return composite_layer_into(acc, acc_w, acc_h, origin, base, env);
    }
    let opacity = crate::opacity_byte::quantized(base.opacity);
    if opacity <= 0.0 {
        return Ok(());
    }
    if let Some(style) = base.renders_style() {
        return composite_styled_into(
            acc, acc_w, acc_h, origin, base, group, style, opacity, env, None,
        );
    }
    if let Some(style) = base.renders_adjustment_style() {
        if let Some(adjustment) = base
            .meta
            .as_deref()
            .and_then(Adjustment::from_meta)
            .and_then(|op| op.in_profile(env.profile).ok())
        {
            return crate::style_adjustment::composite_styled_adjustment_into(
                acc,
                acc_w,
                acc_h,
                origin,
                base,
                group,
                &adjustment,
                style,
                opacity,
                env,
            );
        }
    }
    env.job.check()?;
    let mut buf = vec![[0.0f32; 4]; acc.len()];
    let full = Layer {
        opacity: 1.0,
        blend: BlendMode::Normal,
        style: None,
        ..base.clone()
    };
    composite_layer_into(&mut buf, acc_w, acc_h, origin, &full, env)?;
    let base_alpha: Vec<f32> = buf.iter().map(|px| px[3]).collect();
    clip_pass(&mut buf, &base_alpha, ClipStep::Open);
    // D4's clipped-member rule (`style_knockout`), the UNSTYLED base's loop:
    // a member with a knockout punches to this buffer as it stands after the
    // base alone rendered (opened). The clone happens only when a visible
    // member asks.
    let base_only = crate::style_knockout::members_want_base_snapshot(group).then(|| buf.clone());
    let members = crate::style_knockout::member_env(env, base_only.as_deref());
    for layer in group.iter().filter(|l| l.visible) {
        env.job.check()?;
        composite_layer_into(&mut buf, acc_w, acc_h, origin, layer, members)?;
        clip_pass(&mut buf, &base_alpha, ClipStep::Settle);
    }
    clip_pass(&mut buf, &base_alpha, ClipStep::Close);
    env.job.check()?;
    // The group buffer is accumulator-aligned (same size, same origin), so
    // this is composite_layer_into's kernel with an f32 source and the
    // base's mode and opacity (blend::composite_buffer_into). Pixels a
    // clipped layer touched outside the footprint carry color at forced
    // alpha 0 and are skipped there.
    composite_buffer_into(
        acc,
        &buf,
        acc_w,
        acc_h,
        origin,
        blend_kind(base.blend),
        opacity,
        env.job,
    )?;
    Ok(())
}

/// The five exact (lossless, axis-aligned) whole-document transforms. Naming
/// one lets `geometry` apply the SAME transform to a layer's pixels and to its
/// mask, which is what keeps the two the same size. `doc_transform` reuses
/// them as the exact fast paths of the arbitrary-affine layer transform.
#[derive(Clone, Copy)]
pub(crate) enum Geometry {
    Rotate90,
    Rotate180,
    Rotate270,
    FlipH,
    FlipV,
}

impl Geometry {
    pub(crate) fn apply<I>(
        self,
        img: &I,
    ) -> ImageBuffer<I::Pixel, Vec<<I::Pixel as Pixel>::Subpixel>>
    where
        I: GenericImageView,
        I::Pixel: 'static,
    {
        match self {
            Geometry::Rotate90 => imageops::rotate90(img),
            Geometry::Rotate180 => imageops::rotate180(img),
            Geometry::Rotate270 => imageops::rotate270(img),
            Geometry::FlipH => imageops::flip_horizontal(img),
            Geometry::FlipV => imageops::flip_vertical(img),
        }
    }

    /// True for the two quarter turns, which exchange the canvas's width and
    /// height — and with them the document's two print resolutions.
    pub(crate) fn swaps_axes(self) -> bool {
        matches!(self, Geometry::Rotate90 | Geometry::Rotate270)
    }
}

/// Quantizes a straight-alpha f32 accumulator to RGBA8. The ONE quantizer:
/// the document's projection and every isolated group's private projection
/// (`doc_group::rendered_group`) both end here, so a group boundary rounds
/// exactly the way the canvas does. The `NONE` wrapper of
/// [`quantize_with_job`].
pub(crate) fn quantize(acc: &[[f32; 4]], w: u32, h: u32) -> RgbaImage {
    quantize_with_job(acc, w, h, crate::job_control::JobContext::NONE)
        .expect("uncancellable context")
}

/// [`quantize`] polling `job` at the head of every row (Phase 13A-5b-ii-b);
/// the same row-major samples through the same [`quantize_sample`].
pub(crate) fn quantize_with_job(
    acc: &[[f32; 4]],
    w: u32,
    h: u32,
    job: crate::job_control::JobContext<'_>,
) -> Result<RgbaImage, String> {
    let mut out = RgbaImage::new(w, h);
    if w == 0 || h == 0 {
        return Ok(out);
    }
    let row = w as usize;
    for (dst_row, src_row) in out.chunks_exact_mut(row * 4).zip(acc.chunks(row)) {
        job.check()?;
        for (dst, src) in dst_row.chunks_exact_mut(4).zip(src_row) {
            dst.copy_from_slice(&quantize_sample(*src));
        }
    }
    Ok(out)
}

/// One pixel of [`quantize`]'s loop — the MASTER per-pixel body, moved here
/// unchanged so the regional compositor (`doc_regional_tree`) quantizes an
/// isolated group's sample with the same statement.
#[inline]
pub(crate) fn quantize_sample(px: [f32; 4]) -> [u8; 4] {
    let mut out = [0u8; 4];
    for (d, &v) in out.iter_mut().zip(px.iter()) {
        *d = (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    }
    out
}

// ------------------------------------------------------------- operations --

impl DrDocument {
    /// Wraps straight-alpha RGBA8 pixels as a single-layer document.
    pub fn from_pixels(pixels: RgbaImage) -> Self {
        Self::from_raster(LayerRaster::new(pixels))
    }

    /// [`Self::from_pixels`] over an already built raster.
    pub(crate) fn from_raster(pixels: LayerRaster) -> Self {
        let (width, height) = pixels.dimensions();
        DrDocument {
            width,
            height,
            layers: vec![Layer::from_raster(pixels, "Background")],
            global_light: GlobalLight::default(),
            channels: Vec::new(),
            guides: Vec::new(),
            ruler_origin: (0.0, 0.0),
            color_samplers: Vec::new(),
            paths: crate::doc_path::DocPaths::default(),
            // Bare pixels are assumed sRGB with no metadata and a 72 ppi
            // print size; `DrDocument::open` overwrites all three from
            // whatever the file said.
            profile: IccProfile::srgb(),
            metadata: Metadata::default(),
            resolution: Resolution::default(),
            // Bare pixels are an ENCODED document; a policy is only ever
            // chosen deliberately (`doc_evaluation`).
            linear_light: None,
            // Bare pixels carry no comp; `doc_comps` adds them deliberately.
            comps: Vec::new(),
            // …and no Background mark: D5 is structural (`doc_background`).
            background: None,
        }
    }

    fn layer(&self, idx: usize) -> Option<&Layer> {
        self.layers.get(idx)
    }

    /// Entry `idx` when it is a RASTER layer — the accessor every
    /// pixel-writing op uses in place of [`Self::layer`], so "a group has no
    /// pixels of its own, refuse" is written once instead of at each of the
    /// nineteen sites that need it. `None` for an out-of-range index and for
    /// a group.
    pub(crate) fn raster_layer(&self, idx: usize) -> Option<&Layer> {
        let layer = self.layers.get(idx)?;
        (layer.kind == LayerKind::Raster).then_some(layer)
    }

    /// Canvas-sized straight-alpha projection of all visible layers,
    /// composited bottom-to-top in f32 and quantized once at the end.
    ///
    /// The walk re-derives the whole structure positionally on every call —
    /// the NESTING and, within each level, the CLIP GROUPS — and lives in
    /// [`doc_group::composite_level_into`], which this function only wires
    /// up: build the accumulator, the style-colour transform, the group
    /// allocation budget and the top level, composite, quantize once. Within
    /// a level each unclipped entry is a BASE and the consecutive run of
    /// clipped entries immediately above it composites with it as one unit
    /// through [`composite_clip_group_into`], confined to the base's alpha
    /// footprint. An invisible base hides its whole group; clipped entries at
    /// the BOTTOM of their level have no base and composite as if unclipped.
    /// A base with no clipped entries above composites exactly as it always
    /// did — a document with no groups in it takes byte-for-byte the path it
    /// took before groups existed.
    pub fn flattened(&self) -> Result<RgbaImage, String> {
        self.flattened_with_group_budget(doc_group::MAX_GROUP_BUFFER_PIXELS)
    }

    /// Same render with a reduced group-buffer pixel budget, for capacity
    /// qualification without gigabyte fixtures. Cannot raise the production cap.
    #[doc(hidden)]
    pub fn flattened_with_group_budget(&self, group_pixels: u64) -> Result<RgbaImage, String> {
        self.flattened_cut(group_pixels, Cut::NONE)
    }

    /// The same projection with some entries contributing nothing — the
    /// retouch sampling plate's full-frame form ([`Cut`]). `Cut::NONE` is the
    /// projection above, statement for statement.
    pub(crate) fn flattened_cut(
        &self,
        group_pixels: u64,
        cut: Cut<'_>,
    ) -> Result<RgbaImage, String> {
        self.flattened_cut_with_job(group_pixels, cut, crate::job_control::JobContext::NONE)
    }

    /// [`Self::flattened_cut`] under `job`: the context parse, every
    /// compositor checkpoint and the encode's rows poll it. The same bytes —
    /// every `_with_job` twin it reaches is.
    pub(crate) fn flattened_cut_with_job(
        &self,
        group_pixels: u64,
        cut: Cut<'_>,
        job: crate::job_control::JobContext<'_>,
    ) -> Result<RgbaImage, String> {
        crate::adjust_calibrated::validate_context_with_job(&self.layers, &self.profile, job)?;
        // The evaluation policy's transfer, and with it the refusal walk: a
        // `linear_light` document carrying an operator this phase does not
        // evaluate cannot project at all, and says which layer and which
        // operator (`doc_evaluation`). An encoded document gets `None` here
        // and takes byte-for-byte the path it always took.
        let transfer = self.transfer()?;
        let px = self.width as usize * self.height as usize;
        let mut acc = vec![[0.0f32; 4]; px];
        // Built once for the whole projection and borrowed by every styled
        // layer (`style_composite`): the conversion an authored layer-style
        // colour takes into this document's space.
        let colors = self.style_colors();
        let env = CompositeEnv {
            cut,
            job,
            ..self.composite_env(colors.as_ref(), transfer)
        };
        // The allocation budget every isolated group charges before it makes
        // a buffer (`doc_group::MAX_GROUP_BUFFER_PIXELS`): one projection,
        // one budget, so a nest of groups cannot ask for tens of gigabytes.
        let budget = doc_group::RenderBudget::new(group_pixels);
        let level = doc_group::top_level(&self.layers);
        // The document's OWN projection is the one composite where a knockout
        // has a Background to punch to (`style_knockout`, D5); every other
        // accumulator in the crate starts transparent, which is D4's rule for
        // merges, bakes and isolated group buffers. Without a knockout in the
        // stack this is `composite_level_into` on the whole level, unchanged.
        crate::style_knockout::composite_projection_into(
            &mut acc,
            (self.width, self.height),
            &self.layers,
            self.background,
            &level,
            env,
            &budget,
        )?;
        crate::doc_evaluation::encode_projection_with_job(
            &acc,
            self.width,
            self.height,
            transfer,
            job,
        )
    }

    /// Layer `idx`'s OWN pixels on a transparent canvas-sized buffer, placed
    /// at its offset and clipped to the canvas — the single-layer counterpart
    /// of [`Self::flattened`], and what a layer-scoped copy puts on the
    /// clipboard.
    ///
    /// Deliberately raw: opacity, blend mode, visibility and the layer mask
    /// are COMPOSITING properties — they describe how the layer meets the
    /// stack, not what its pixels are — so they play no part here and a
    /// hidden layer still yields its pixels. Straight alpha throughout, so
    /// nothing is un/premultiplied and the bytes copy across verbatim.
    ///
    /// For a GROUP the answer is its RAW projection on that same buffer —
    /// its children composited among themselves, WITHOUT the group's own
    /// opacity, blend mode, mask or style, which keeps this export's
    /// "deliberately raw" contract exactly.
    ///
    /// None for an out-of-range `idx`; an empty canvas or a layer entirely
    /// off-canvas simply yields a fully transparent buffer.
    pub fn layer_canvas_image(&self, idx: usize) -> Option<RgbaImage> {
        let layer = self.layers.get(idx)?;
        if layer.kind == LayerKind::Group {
            return self.group_projection(idx);
        }
        let mut out = RgbaImage::new(self.width, self.height);
        let Some((x, y, w, h)) = crate::layer_rect::canvas_clip((self.width, self.height), layer)
        else {
            return Some(out);
        };
        // Layer space -> canvas space in i64: an offset near i32::MIN/MAX
        // must not wrap; the clip keeps every run inside the canvas.
        let (off_x, off_y) = (i64::from(layer.offset.0), i64::from(layer.offset.1));
        let stride = self.width as usize * 4;
        let target: &mut [u8] = &mut out;
        for ly in y..y + h {
            let cy = (off_y + i64::from(ly)) as usize;
            for (rx, run) in layer.pixels.row_segments(ly, x, x + w) {
                let at = cy * stride + (off_x + i64::from(rx)) as usize * 4;
                target[at..at + run.len() * 4].copy_from_slice(run.as_flattened());
            }
        }
        Some(out)
    }

    /// Pure setter: replaces layer `idx`'s name. `None` when it already
    /// carries that name — see [`Self::with_layer`]'s note on the latch.
    pub fn with_layer_name(&self, idx: usize, name: &str) -> Option<Self> {
        if self.layer(idx)?.name == name {
            return None;
        }
        self.with_layer(idx, |l| l.name = name.to_string())
    }

    /// Pure setter: replaces layer `idx`'s opacity (clamped to [0, 1]).
    /// `None` when the clamped value is the one already stored.
    pub fn with_layer_opacity(&self, idx: usize, opacity: f32) -> Option<Self> {
        let opacity = sane_opacity(opacity);
        if self.layer(idx)?.opacity == opacity {
            return None;
        }
        self.with_layer(idx, |l| l.opacity = opacity)
    }

    /// Pure setter: replaces entry `idx`'s blend mode. `None` — a domain
    /// refusal — for `BlendMode::PassThrough` on a RASTER layer: pass-through
    /// is a group's declaration that it has no footprint of its own, and a
    /// leaf carrying it would have no defined meaning. `None` too when the
    /// entry is already in that mode.
    pub fn with_layer_blend_mode(&self, idx: usize, mode: BlendMode) -> Option<Self> {
        let layer = self.layer(idx)?;
        if (mode.is_group_only() && layer.kind != LayerKind::Group) || layer.blend == mode {
            return None;
        }
        self.with_layer(idx, |l| l.blend = mode)
    }

    /// Pure setter: replaces layer `idx`'s visibility flag. `None` when the
    /// flag already has that value.
    pub fn with_layer_visible(&self, idx: usize, visible: bool) -> Option<Self> {
        if self.layer(idx)?.visible == visible {
            return None;
        }
        self.with_layer(idx, |l| l.visible = visible)
    }

    /// Pure setter: replaces layer `idx`'s clipped flag (see [`Layer::clipped`]).
    pub fn with_layer_clipped(&self, idx: usize, clipped: bool) -> Option<Self> {
        self.with_layer(idx, |l| l.clipped = clipped)
    }

    /// Pure setter: replaces layer `idx`'s canvas offset.
    ///
    /// A GROUP has no pixel BUFFER of its own, so this SHIFTS its whole
    /// subtree instead: every raster descendant moves by the delta that puts
    /// the group's rect origin at `(x, y)`. That rect is
    /// [`Self::layer_pixel_rect`] — the union of its descendants' buffer
    /// rects, which is exactly what `dr_doc_layer_offset_x/y/width/height`
    /// report for a group, so reading an entry's offset and writing it back
    /// is the identity on both kinds of entry. (`layer_bounds`, the CONTENT
    /// box, is a different rectangle and belongs to align and distribute; it
    /// also costs a per-pixel scan, which a property setter has no business
    /// paying.) `None` for a group holding no raster descendant — there is no
    /// origin to move.
    ///
    /// A POSITION edit under `doc_lock`, so a position-locked entry (or a
    /// group holding one) refuses it. It deliberately does NOT follow LINKS:
    /// it is a property write, not a move — [`Self::move_layers`] is the op
    /// that means "move this".
    ///
    /// `None` for a move of zero — writing back the offset just read — on
    /// either kind of entry, like every other property setter here.
    pub fn with_layer_offset(&self, idx: usize, x: i32, y: i32) -> Option<Self> {
        self.under_locks(idx, EditKind::Position, |doc| {
            let entry = doc.layer(idx)?;
            if entry.kind == LayerKind::Group {
                let (bx, by, _, _) = doc.layer_pixel_rect(idx)?;
                let dx = i64::from(x) - i64::from(bx);
                let dy = i64::from(y) - i64::from(by);
                if dx == 0 && dy == 0 {
                    return None;
                }
                let mut out = doc.clone();
                let (mdx, mdy) = (saturating_i32(dx), saturating_i32(dy));
                // Every entry of the subtree through the ONE translate the
                // Move tool's drag also uses, so a nested group's canvas-sized
                // MASK travels with it exactly as a layer's buffer does — as
                // bookkeeping on its offset, never as a resample.
                for i in doc_group::subtree(&doc.layers, idx) {
                    doc_align::translate_entry(&mut out.layers, i, mdx, mdy);
                }
                return Some(out);
            }
            if entry.offset == (x, y) {
                return None;
            }
            doc.with_layer(idx, |l| {
                let (dx, dy) = (
                    i64::from(x) - i64::from(l.offset.0),
                    i64::from(y) - i64::from(l.offset.1),
                );
                if l.mask_properties.linked {
                    l.translate_mask(dx, dy);
                }
                l.offset = (x, y);
                // A move: the reference point accumulates it (`Layer::reference`).
                l.reference = translated_offset(l.reference, dx, dy);
            })
        })
    }

    /// [`Self::with_layer_offset`]'s offset write, lock gate and `None` rules, but every
    /// entry's [`Layer::reference`] stays as in `self`: a PLACEMENT, not a move (a layer comp's
    /// restore), as the typed twin assigns the offset (`precision_comps`).
    pub(crate) fn with_layer_placement(&self, idx: usize, x: i32, y: i32) -> Option<Self> {
        let mut out = self.with_layer_offset(idx, x, y)?;
        for (placed, before) in out.layers.iter_mut().zip(&self.layers) {
            placed.reference = before.reference;
        }
        Some(out)
    }

    /// Pure setter: replaces layer `idx`'s pixels (any size; offset and
    /// properties kept). Independent raw masks and their properties survive
    /// replacements of every size, in the same canvas location.
    /// `None` on a GROUP index: a group has no pixels of its own.
    ///
    /// A PIXELS edit under `doc_lock`. It is the funnel almost every pixel op
    /// ends in, so those ops pay the lock gate twice — once at their own entry
    /// point and once here. That is deliberate: the gate is idempotent (a
    /// second alpha restore changes nothing), it costs one extra buffer clone
    /// only on a transparency-locked layer, and it means this export honours
    /// locks on its own, which it must — the host calls it directly.
    /// `None` also when replacing the pixels would grow beyond native capacity;
    /// equal-sized edits and reductions remain possible in an oversized import.
    pub fn with_layer_pixels(&self, idx: usize, pixels: RgbaImage) -> Option<Self> {
        let raster = self.raster_layer(idx)?.pixels.rebuilt(pixels);
        self.with_layer_raster(idx, raster)
    }

    /// [`Self::with_layer_pixels`] with the raster already built (the paint
    /// writers hand in a `LayerRaster::with_rect` result): the same group
    /// refusal, capacity check and lock gate.
    pub(crate) fn with_layer_raster(&self, idx: usize, pixels: LayerRaster) -> Option<Self> {
        self.raster_layer(idx)?;
        if !self.native_layers_fit_dimensions(self.layers.iter().enumerate().map(|(i, l)| {
            if i == idx {
                pixels.dimensions()
            } else {
                l.pixels.dimensions()
            }
        })) {
            return None;
        }
        self.under_locks(idx, EditKind::Pixels, move |doc| {
            doc.with_layer(idx, |l| {
                l.pixels = pixels;
            })
        })
    }

    /// Clone-then-mutate for one entry. It cannot tell whether `edit` changed
    /// anything, so the LATCH — the purity rule's "an op that would change
    /// nothing returns None, never an identical copy", without which a host's
    /// read-modify-write registers a phantom undo step and dirties the
    /// document — lives in each property setter above, which compares the one
    /// field it writes before it gets here.
    fn with_layer(&self, idx: usize, edit: impl FnOnce(&mut Layer)) -> Option<Self> {
        self.layer(idx)?;
        let mut doc = self.clone();
        edit(&mut doc.layers[idx]);
        Some(doc)
    }

    /// Inserts a transparent canvas-sized layer above `idx`. `None` for an
    /// invalid index or growth past native capacity, checked before allocation.
    /// The layer is one repeated sample over `LayerRaster::filled`: at
    /// most 4 tile allocations and no view seed, the same bytes as a zeroed
    /// canvas image.
    pub fn adding_layer(&self, idx: usize, name: &str) -> Option<Self> {
        self.layer(idx)?;
        if !self.native_layers_fit_dimensions(
            self.layers
                .iter()
                .map(|l| l.pixels.dimensions())
                .chain(std::iter::once((self.width, self.height))),
        ) {
            return None;
        }
        self.inserting_above(idx, self.transparent_canvas_layer(name), None)
            .ok()
            .flatten()
    }

    /// A transparent canvas-sized raster layer over `LayerRaster::filled`,
    /// shared by [`Self::adding_layer`] and `adding_layer_at`.
    pub(crate) fn transparent_canvas_layer(&self, name: &str) -> Layer {
        Layer::from_raster(LayerRaster::filled(self.width, self.height, [0; 4]), name)
    }

    /// Inserts a layer with the given pixels (offset 0) above `idx`: at
    /// `idx`'s own depth, immediately above its whole SUBTREE, so "above a
    /// group" means above the group and not inside it. Identical to
    /// `insert(idx + 1)` on a document with no groups. `None` for an invalid
    /// index or growth past native layer-count/decoded-pixel capacity.
    pub fn adding_image_layer(&self, idx: usize, pixels: RgbaImage, name: &str) -> Option<Self> {
        self.adding_image_layer_with_job(idx, pixels, name, None)
            .ok()
            .flatten()
    }

    /// [`Self::adding_image_layer`]'s insertion and validation with the new layer at
    /// `offset`: a paste is MADE there, so its reference point (`Layer::reference`) is (0, 0).
    pub fn add_image_layer_at(
        &self,
        idx: usize,
        pixels: RgbaImage,
        name: &str,
        offset: (i32, i32),
    ) -> Option<Self> {
        let mut out = self.adding_image_layer(idx, pixels, name)?;
        let at = doc_group::subtree(&self.layers, idx).end;
        let layer = out.layers.get_mut(at)?;
        layer.offset = offset;
        Some(out)
    }

    /// Same insertion and validation; optional job bounds owned record copying.
    pub(crate) fn adding_image_layer_with_job(
        &self,
        idx: usize,
        pixels: RgbaImage,
        name: &str,
        job: Option<crate::job_control::JobContext<'_>>,
    ) -> Result<Option<Self>, String> {
        if self.layer(idx).is_none() {
            return Ok(None);
        }
        if !self.native_layers_fit_dimensions(
            self.layers
                .iter()
                .map(|l| l.pixels.dimensions())
                .chain(std::iter::once(pixels.dimensions())),
        ) {
            return Ok(None);
        }
        self.inserting_above(idx, Layer::new(pixels, name), job)
    }

    /// The one placement of [`Self::adding_layer`] and
    /// [`Self::adding_image_layer_with_job`]: `layer` at `idx`'s depth,
    /// immediately above its whole subtree, unclipped. The callers have
    /// checked capacity. `Ok(None)` for an invalid index or a malformed
    /// result; `Err` only from the optional job's copy.
    fn inserting_above(
        &self,
        idx: usize,
        mut layer: Layer,
        job: Option<crate::job_control::JobContext<'_>>,
    ) -> Result<Option<Self>, String> {
        let Some(source) = self.layer(idx) else {
            return Ok(None);
        };
        layer.depth = source.depth;
        let at = doc_group::subtree(&self.layers, idx).end;
        let mut doc = match job {
            Some(job) => self.clone_with_job(job)?,
            None => self.clone(),
        };
        doc.layers.insert(at, layer);
        Ok(doc_group::validated(doc))
    }

    /// Duplicates entry `idx` (pixels shared, " copy" appended to the name),
    /// inserting the duplicate immediately above it. A GROUP duplicates its
    /// whole SUBTREE, depths preserved and " copy" appended to the group's
    /// name only. Meta and style are copied like every other property; LINKS
    /// are not — a copy is a new object, not a second member of the
    /// original's link group. `None` for an invalid index or duplication that
    /// would exceed native layer-count/decoded-pixel capacity.
    pub fn duplicating_layer(&self, idx: usize) -> Option<Self> {
        self.layer(idx)?;
        let sub = doc_group::subtree(&self.layers, idx);
        if !self.native_layers_fit_dimensions(
            self.layers.iter().map(|l| l.pixels.dimensions()).chain(
                self.layers[sub.clone()]
                    .iter()
                    .map(|l| l.pixels.dimensions()),
            ),
        ) {
            return None;
        }
        let at = sub.end;
        let mut copies: Vec<Layer> = self.layers[sub]
            .iter()
            .map(|l| Layer {
                link: 0,
                // A copy is a NEW object, not a second reference to the
                // original — the same rule `Channel::duplicated` follows, and
                // per COPIED ENTRY so a duplicated group's whole subtree
                // comes back with fresh ids (`doc_identity`).
                id: crate::doc_identity::mint_layer_id(),
                ..l.clone()
            })
            .collect();
        copies
            .last_mut()
            .expect("a subtree always contains its own entry")
            .name
            .push_str(" copy");
        let mut doc = self.clone();
        doc.layers.splice(at..at, copies);
        doc_group::validated(doc).filter(|out| self.native_edit_fits(out))
    }

    /// Removes entry `idx` and, for a GROUP, its whole subtree; `None` when
    /// that would leave the document with no entries at all.
    pub fn removing_layer(&self, idx: usize) -> Option<Self> {
        self.layer(idx)?;
        let sub = doc_group::subtree(&self.layers, idx);
        if sub.len() == self.layers.len() {
            return None;
        }
        let mut doc = self.clone();
        doc.layers.drain(sub);
        doc_group::validated(doc)
    }

    /// Merges entry `idx` into the previous SIBLING — the entry immediately
    /// below it within its own level, which on a document with no groups is
    /// simply `idx - 1`. The merged layer covers the union of both extents;
    /// BOTH entries' modes and opacities are baked into its pixels via the
    /// same kernel as [`DrDocument::flattened`] (the lower composites onto a
    /// transparent backdrop, where every blend function degenerates to Normal
    /// and Dissolve keeps exactly its dithered pixels), so the result is
    /// Normal at opacity 1 and keeps only the lower entry's name,
    /// visibility, clipped flag, depth, locks and link. An invisible upper
    /// entry contributes nothing (it is simply removed); a hidden LOWER entry
    /// refuses the merge (`None`) so the upper entry's content cannot
    /// silently vanish. `None` too when `idx` is at the BOTTOM of its level
    /// (generalising the old `idx == 0` rule) or out of range, and when the
    /// LOWER entry's locks refuse a merge (`doc_lock::EditKind::Merge`) —
    /// merging replaces that entry's whole picture, which is exactly what
    /// Lock Pixels forbids.
    ///
    /// Either operand may be a GROUP: it is materialized through
    /// [`doc_group::rendered_group`] first — the ONE definition of what a
    /// group's pixels are — so a group merged down (or merged into) becomes
    /// a plain raster entry at the level's depth and its subtree is removed.
    /// A group with nothing visible in it materializes to nothing and simply
    /// contributes nothing, exactly like an invisible layer.
    ///
    /// The merge is destructive, so the merged layer carries neither entry's
    /// mask, meta or layer style: an ENABLED mask is baked into the pixels by
    /// the kernel (like opacity and blend), a disabled one is simply dropped,
    /// meta no longer describes the pixels it is attached to, and both
    /// entries' style effects are baked by the same kernel (the union extent
    /// grows by each one's `style_composite::merge_extent`: the style's
    /// pad, plus the drop shadow's shifted rect clipped to the canvas).
    /// Because the kernel
    /// is shared, merging an ADJUSTMENT layer down bakes the adjustment into
    /// the layer below, gated by its blend mode, opacity and mask.
    ///
    /// A CLIPPED upper entry is baked through its clipping: the pair runs the
    /// same group kernel as the projection ([`composite_clip_group_into`]
    /// with the lower entry as base), so the upper entry's contribution is
    /// alpha-limited to the lower entry's footprint. The merged layer keeps
    /// the LOWER entry's clipped flag (like its name and visibility), so a
    /// merge inside a clip group stays in the group.
    ///
    /// The merged entry keeps the lower entry's id, so a LAYER COMP bound to
    /// it follows the merge (`comps_geometry`'s module doc): its
    /// recorded offsets shift by the merged origin − the lower entry's offset
    /// and its recorded appearance becomes the merged layer's own (Normal,
    /// opacity 1, no style), through `merged_comps`.
    pub fn merging_down(&self, idx: usize) -> Option<Self> {
        crate::adjust_calibrated::validate_context(&self.layers, &self.profile).ok()?;
        let upper_entry = self.layer(idx)?;
        let upper_visible = upper_entry.visible;
        let upper_clipped = upper_entry.clipped;
        let upper_sub = doc_group::subtree(&self.layers, idx);
        let level_start = doc_group::level_range(&self.layers, idx).start;
        if upper_sub.start == level_start {
            return None; // nothing below it inside its own level
        }
        // A sibling's subtree ENDS with its own record, so the entry
        // immediately below this subtree is exactly the previous sibling.
        let lower_idx = upper_sub.start - 1;
        let lower_entry = &self.layers[lower_idx];
        if !lower_entry.visible {
            return None;
        }
        // The DESTINATION's locks, asked before anything is built: the merge
        // drains both subtrees, so this is the last point at which the index
        // still names the entry the lock belongs to (see `doc_lock`).
        if self.lock_block(lower_idx, EditKind::Merge) != 0 {
            return None;
        }
        let lower_sub = doc_group::subtree(&self.layers, lower_idx);
        let merged_name = lower_entry.name.clone();
        let merged_depth = lower_entry.depth;
        let merged_clipped = lower_entry.clipped;
        let merged_locks = lower_entry.locks;
        let merged_link = lower_entry.link;
        let merged_id = lower_entry.id;
        let merged_label = lower_entry.label;

        let colors = self.style_colors();
        // The projection's own refusal walk applies to a MERGE too: it is a
        // projection over two subtrees (`doc_evaluation`).
        let transfer = self.transfer().ok()?;
        let env = self.composite_env(colors.as_ref(), transfer);
        let budget = doc_group::RenderBudget::default();
        let upper = upper_visible
            .then(|| doc_group::materialized(&self.layers, idx, env, &budget))
            .transpose()
            .ok()?
            .flatten();
        let lower = doc_group::materialized(&self.layers, lower_idx, env, &budget).ok()?;

        let mut doc = self.clone();
        doc.layers.drain(upper_sub);
        let Some(upper) = upper else {
            // Nothing to merge in: the upper entry (or the whole group) was
            // invisible or empty, so it is simply removed.
            return doc_group::validated(doc);
        };
        let (mut x0, mut y0, mut x1, mut y1) = merge_extent(&upper, env);
        if let Some(lower) = lower.as_ref() {
            let (lx0, ly0, lx1, ly1) = merge_extent(lower, env);
            x0 = x0.min(lx0);
            y0 = y0.min(ly0);
            x1 = x1.max(lx1);
            y1 = y1.max(ly1);
        }
        let (uw, uh) = ((x1 - x0) as u64, (y1 - y0) as u64);
        if uw == 0 || uh == 0 || uw * uh > MAX_PIXELS {
            return None;
        }
        if x0 < i64::from(i32::MIN) || y0 < i64::from(i32::MIN) {
            return None;
        }
        let origin = (x0 as i32, y0 as i32);
        // The REAL backdrop a knockout member would have read from the
        // document's own projection, windowed onto this private accumulator
        // (`style_knockout_merge`). Both planes are absent — today's statements
        // exactly — for a merge with no knockout member and for the one
        // excluded set (a lowest member clipped to a base outside the pair).
        let backdrop = self
            .merge_backdrop(&[lower_idx, idx], origin, (uw as u32, uh as u32), &budget)
            .ok()?;
        let env = CompositeEnv {
            knockout: backdrop.backdrop(),
            ..env
        };
        let mut acc = vec![[0.0f32; 4]; (uw * uh) as usize];
        match lower.as_ref() {
            Some(lower) if upper_clipped => composite_clip_group_into(
                &mut acc,
                uw as u32,
                uh as u32,
                origin,
                lower,
                std::slice::from_ref(&upper),
                env,
            )
            .ok()?,
            Some(lower) => {
                composite_layer_into(&mut acc, uw as u32, uh as u32, origin, lower, env).ok()?;
                composite_layer_into(&mut acc, uw as u32, uh as u32, origin, &upper, env).ok()?;
            }
            // A clipped upper entry whose base materialized to nothing is
            // baseless, which composites as if unclipped — the same rule the
            // projection applies at the bottom of a level.
            None => {
                composite_layer_into(&mut acc, uw as u32, uh as u32, origin, &upper, env).ok()?
            }
        }
        let mut merged = Layer::new(
            crate::doc_evaluation::encode_projection(&acc, uw as u32, uh as u32, transfer),
            &merged_name,
        );
        merged.offset = origin;
        merged.visible = true;
        merged.clipped = merged_clipped;
        merged.depth = merged_depth;
        merged.locks = merged_locks;
        merged.link = merged_link;
        // The merged entry IS the lower one with new pixels — it takes its
        // slot, name, flags, locks and link — so it keeps that entry's
        // IDENTITY and colour label too (`doc_identity`'s one merge rule,
        // which `merge_layers` and `merge_visible` already followed).
        merged.id = merged_id;
        merged.label = merged_label;
        if let Some(comps) = merged_comps(
            &self.comps,
            self.comps_after_growth(lower_idx, origin),
            merged_id,
        ) {
            doc.comps = comps;
        }
        doc.layers.splice(lower_sub, std::iter::once(merged));
        doc_group::validated(doc)
    }

    /// Single-layer document containing the projection, named "Background".
    ///
    /// Like [`DrDocument::merging_down`], this is destructive: the projection
    /// bakes every ENABLED mask and every layer style into the composited
    /// pixels (a disabled mask is simply dropped with its layer), so the
    /// resulting layer carries neither a mask, meta nor style — nothing is
    /// left that could describe those pixels. The document's global light
    /// is a preference, not layer state, and is kept; so are the alpha
    /// channels, which are canvas-sized document state and have nothing to
    /// do with the layer stack being collapsed; so are the GUIDES and the
    /// RULER ORIGIN, canvas-space state for exactly the same reason (the
    /// canvas does not move, so they are carried verbatim); and so, again for
    /// that reason, are the colour profile (the flattened pixels are still in
    /// that space), the metadata packets and the print resolution.
    ///
    /// Every one of those seven is listed EXPLICITLY: this builds on
    /// `..from_pixels`, so anything not named here is silently replaced by a
    /// default. That applies to the LAYER-level state too, deliberately: the
    /// one surviving entry is a raster layer at depth 0 with no locks, no
    /// link and `open` true, because the whole tree — groups included — has
    /// been collapsed into it.
    pub fn flattening(&self) -> Result<Self, String> {
        Ok(DrDocument {
            global_light: self.global_light,
            channels: self.channels.clone(),
            guides: self.guides.clone(),
            ruler_origin: self.ruler_origin,
            color_samplers: self.color_samplers.clone(),
            paths: self.paths.clone(),
            profile: Arc::clone(&self.profile),
            metadata: self.metadata.clone(),
            resolution: self.resolution,
            // The projection is already in this document's space under its
            // own policy, so the collapsed document keeps that policy (the
            // tables are `Arc`-shared, not rebuilt).
            linear_light: self.linear_light.clone(),
            // A FLATTEN collapses every layer into ONE new picture, so every
            // comp entry's identity is gone and the whole list goes with it
            // (`from_pixels` supplies the empty one).
            ..DrDocument::from_pixels(self.flattened()?)
        })
    }

    /// Rotates the whole document 90 degrees clockwise.
    pub fn rotate90(&self) -> Option<Self> {
        let ch = i64::from(self.height);
        self.geometry(self.height, self.width, Geometry::Rotate90, |o, _lw, lh| {
            (saturating_i32(ch - i64::from(o.1) - lh), o.0)
        })
    }

    /// Rotates the whole document 180 degrees.
    pub fn rotate180(&self) -> Option<Self> {
        let (cw, ch) = (i64::from(self.width), i64::from(self.height));
        self.geometry(self.width, self.height, Geometry::Rotate180, |o, lw, lh| {
            (
                saturating_i32(cw - i64::from(o.0) - lw),
                saturating_i32(ch - i64::from(o.1) - lh),
            )
        })
    }

    /// Rotates the whole document 90 degrees counter-clockwise.
    pub fn rotate270(&self) -> Option<Self> {
        let cw = i64::from(self.width);
        self.geometry(
            self.height,
            self.width,
            Geometry::Rotate270,
            |o, lw, _lh| (o.1, saturating_i32(cw - i64::from(o.0) - lw)),
        )
    }

    /// Mirrors the whole document left-right.
    pub fn flip_horizontal(&self) -> Option<Self> {
        let cw = i64::from(self.width);
        self.geometry(self.width, self.height, Geometry::FlipH, |o, lw, _lh| {
            (saturating_i32(cw - i64::from(o.0) - lw), o.1)
        })
    }

    /// Mirrors the whole document top-bottom.
    pub fn flip_vertical(&self) -> Option<Self> {
        let ch = i64::from(self.height);
        self.geometry(self.width, self.height, Geometry::FlipV, |o, _lw, lh| {
            (o.0, saturating_i32(ch - i64::from(o.1) - lh))
        })
    }

    /// Applies one exact geometric transform to every layer's pixels AND its
    /// independently placed mask, regardless of the content-link flag. `f`
    /// supplies a raster's new offset from an offset and its own dimensions —
    /// the layer's own, and each layer comp's recorded one (`comps_geometry`).
    fn geometry(
        &self,
        new_w: u32,
        new_h: u32,
        geom: Geometry,
        f: impl Fn((i32, i32), i64, i64) -> (i32, i32),
    ) -> Option<Self> {
        // The follow hook (`doc_vector_mask`): a raster-less vector mask is
        // rebuilt from its shape through the canvas's own point map, the one
        // `doc_path::geometry_paths` gives the path list.
        let (fw, fh) = (f64::from(self.width), f64::from(self.height));
        let following = |out: Self| {
            let out = self.following_vector_masks(out, |x, y| match geom {
                Geometry::Rotate90 => (fh - y, x),
                Geometry::Rotate180 => (fw - x, fh - y),
                Geometry::Rotate270 => (y, fw - x),
                Geometry::FlipH => (fw - x, y),
                Geometry::FlipV => (x, fh - y),
            });
            self.following_geometry(out, geom)
        };
        if self.has_any_effective_source() {
            return self.exact_source_geometry(geom).map(following);
        }
        let layers = self
            .layers
            .iter()
            .map(|l| {
                let (lw, lh) = l.pixels.dimensions();
                let mut out = l.clone();
                out.pixels = {
                    let image = l.pixels.to_contiguous().ok()?;
                    LayerRaster::unseeded(geom.apply(&image))
                };
                out.offset =
                    group_offset(l).unwrap_or_else(|| f(l.offset, i64::from(lw), i64::from(lh)));
                out.geometry_mask(geom, self.width, self.height);
                Some(out)
            })
            .collect::<Option<Vec<_>>>()?;
        let result = DrDocument {
            width: new_w,
            height: new_h,
            layers,
            global_light: self.global_light,
            channels: doc_channel::geometry_channels(&self.channels, geom),
            guides: doc_guide::geometry_guides(&self.guides, geom, self.width, self.height),
            ruler_origin: doc_guide::geometry_origin(
                self.ruler_origin,
                geom,
                self.width,
                self.height,
            ),
            color_samplers: doc_sampler::geometry_samplers(
                &self.color_samplers,
                geom,
                self.width,
                self.height,
            ),
            paths: self
                .paths
                .moved(|p| crate::doc_path::geometry_paths(p, geom, self.width, self.height)),
            profile: Arc::clone(&self.profile),
            metadata: self.metadata.clone(),
            // A 300 x 150 ppi document turned 90 degrees is 150 x 300; the
            // other three geometries leave the axes where they were.
            resolution: self.resolution.transposed_if(geom.swaps_axes()),
            linear_light: self.linear_light.clone(),
            comps: self.remapped_comps(
                |l, rec| {
                    let (lw, lh) = l.pixels.dimensions();
                    Some(f(rec, lw.into(), lh.into()))
                },
                None,
                |_| 1.0,
            ),
            background: self.background,
        };
        crate::doc_rotate::exact_geometry_changed(self, &result).then(|| following(result))
    }

    /// Moves the canvas window: canvas becomes `w` x `h`, offsets shift by
    /// (-x, -y), layer pixels untouched. Bounds-checked against the canvas
    /// like `dr_image_crop`.
    ///
    /// Nothing changes in layer space — pixels outside the new canvas are
    /// retained rather than trimmed — so masks and meta ride along untouched:
    /// a mask is layer-space, and moving the window past it keeps it hiding
    /// exactly the same pixels.
    ///
    /// `None` for an empty or out-of-bounds rect, and for the whole canvas
    /// (0, 0, width, height) — that window IS the current one, and returning
    /// an identical copy would register a phantom undo step in the host.
    pub fn crop(&self, x: u32, y: u32, w: u32, h: u32) -> Option<Self> {
        if w == 0 || h == 0 {
            return None;
        }
        let x_end = x.checked_add(w)?;
        let y_end = y.checked_add(h)?;
        if x_end > self.width || y_end > self.height {
            return None;
        }
        if x == 0 && y == 0 && w == self.width && h == self.height {
            return None;
        }
        let (dx, dy) = (-i64::from(x), -i64::from(y));
        let layers = self
            .layers
            .iter()
            .map(|l| {
                let mut out = l.clone();
                if out.kind == LayerKind::Raster {
                    out.offset = translated_offset(out.offset, dx, dy);
                }
                out.translate_mask(dx, dy);
                out
            })
            .collect();
        let out = DrDocument {
            width: w,
            height: h,
            layers,
            global_light: self.global_light,
            channels: doc_channel::cropped_channels(&self.channels, x, y, w, h),
            guides: doc_guide::cropped_guides(&self.guides, x, y, w, h),
            ruler_origin: doc_guide::cropped_origin(self.ruler_origin, x, y, w, h),
            color_samplers: doc_sampler::cropped_samplers(&self.color_samplers, x, y, w, h),
            paths: self
                .paths
                .moved(|p| crate::doc_path::cropped_paths(p, x, y)),
            profile: Arc::clone(&self.profile),
            metadata: self.metadata.clone(),
            resolution: self.resolution,
            linear_light: self.linear_light.clone(),
            comps: self.remapped_comps(
                |_, rec| Some(translated_offset(rec, dx, dy)),
                None,
                |_| 1.0,
            ),
            background: self.background,
        };
        Some(self.following_translation(out, crate::reference_point::ReferenceOp::Crop, dx, dy))
    }

    /// Changes the canvas size without scaling anything: the canvas becomes
    /// `w` x `h` and every layer's offset shifts by `origin` — where the old
    /// canvas's top-left corner lands in the new canvas. Layer pixels are
    /// untouched; content outside the new canvas is retained, as with crop.
    ///
    /// Growing or shrinking the canvas is a pure offset change, so — exactly
    /// as in [`DrDocument::crop`] — masks and meta ride along untouched.
    ///
    /// Channels are canvas-sized, so growing the canvas grows every one of
    /// them: `None` when that would push the list past the DRDC total-pixel
    /// budget (`doc_channel::channels_fit_at`), rather than building a
    /// document `dr_doc_save_native` could not write.
    ///
    /// An EXACT no-op — the same size at origin `(0, 0)` — answers `None`
    /// too (Phase 12F-1b), like the typed twin and like every other pure op:
    /// an identical copy would register a phantom undo step in the app. The
    /// host answers that request as a SUCCESSFUL no-op at every depth before
    /// it ever calls here.
    pub fn canvas_resize(&self, w: u32, h: u32, origin: (i32, i32)) -> Option<Self> {
        if w == 0 || h == 0 || u64::from(w) * u64::from(h) > MAX_PIXELS {
            return None;
        }
        if !doc_channel::channels_fit_at(w, h, self.channels.len()) {
            return None;
        }
        if (w, h) == (self.width, self.height) && origin == (0, 0) {
            return None;
        }
        let (dx, dy) = (i64::from(origin.0), i64::from(origin.1));
        let layers = self
            .layers
            .iter()
            .map(|l| {
                let mut out = l.clone();
                if out.kind == LayerKind::Raster {
                    out.offset = translated_offset(out.offset, dx, dy);
                }
                out.translate_mask(dx, dy);
                out
            })
            .collect();
        let out = DrDocument {
            width: w,
            height: h,
            layers,
            global_light: self.global_light,
            channels: doc_channel::padded_channels(&self.channels, w, h, origin),
            guides: doc_guide::padded_guides(&self.guides, w, h, origin),
            ruler_origin: doc_guide::padded_origin(self.ruler_origin, w, h, origin),
            color_samplers: doc_sampler::padded_samplers(&self.color_samplers, w, h, origin),
            paths: self
                .paths
                .moved(|p| crate::doc_path::padded_paths(p, origin)),
            profile: Arc::clone(&self.profile),
            metadata: self.metadata.clone(),
            resolution: self.resolution,
            linear_light: self.linear_light.clone(),
            comps: self.remapped_comps(
                |_, rec| Some(translated_offset(rec, dx, dy)),
                None,
                |_| 1.0,
            ),
            background: self.background,
        };
        let op = crate::reference_point::ReferenceOp::CanvasSize;
        Some(self.following_translation(out, op, dx, dy))
    }

    /// Scales the canvas and every layer (sizes and offsets) proportionally.
    /// The total-pixel guard applies to the canvas, as in `dr_image_resize`.
    /// Layer styles are scaled with the layer ("Scale Effects", by the mean
    /// factor `sqrt(fx * fy)`).
    ///
    /// Channels resample to the new canvas, so — as in
    /// [`DrDocument::canvas_resize`] — `None` when the enlarged list would
    /// break the DRDC total-pixel budget.
    pub fn resize(&self, w: u32, h: u32, filter: FilterType) -> Option<Self> {
        if self.has_any_effective_source() {
            // The follow hook, as below.
            let (sx, sy) = (
                f64::from(w) / f64::from(self.width),
                f64::from(h) / f64::from(self.height),
            );
            return self
                .resize_source_geometry(w, h, filter)
                .map(|out| self.following_vector_masks(out, |x, y| (x * sx, y * sy)))
                .map(|out| self.following_resize(out));
        }
        if w == 0 || h == 0 || u64::from(w) * u64::from(h) > MAX_PIXELS {
            return None;
        }
        if !doc_channel::channels_fit_at(w, h, self.channels.len()) {
            return None;
        }
        let fx = f64::from(w) / f64::from(self.width);
        let fy = f64::from(h) / f64::from(self.height);
        let dimensions: Vec<_> = self
            .layers
            .iter()
            .map(|l| {
                let (lw, lh) = l.pixels.dimensions();
                (
                    ((f64::from(lw) * fx).round() as u32).max(1),
                    ((f64::from(lh) * fy).round() as u32).max(1),
                )
            })
            .collect();
        if !self.native_layers_fit_dimensions(dimensions.iter().copied()) {
            return None;
        }
        if !self.native_masks_fit_dimensions(
            self.layers.iter().filter_map(|l| l.mask.as_ref()).map(|m| {
                (
                    ((f64::from(m.width()) * fx).round() as u32).max(1),
                    ((f64::from(m.height()) * fy).round() as u32).max(1),
                )
            }),
        ) {
            return None;
        }
        let layers = self
            .layers
            .iter()
            .zip(dimensions)
            .map(|(l, (nw, nh))| {
                let mut out = l.clone();
                out.pixels = {
                    let image = l.pixels.to_contiguous().ok()?;
                    LayerRaster::unseeded(crate::sampling::resize(&image, nw, nh, filter))
                };
                out.offset = group_offset(l).unwrap_or_else(|| scaled_offset(l.offset, fx, fy));
                out.resize_mask(fx, fy, filter)?;
                out.style = scaled_style(&l.style, (fx * fy).sqrt());
                Some(out)
            })
            .collect::<Option<Vec<_>>>()?;

        let result = DrDocument {
            width: w,
            height: h,
            layers,
            global_light: self.global_light,
            channels: doc_channel::resized_channels(&self.channels, w, h, filter),
            // Unlike the layer offsets above, a guide's scaled position is
            // deliberately NOT rounded: an offset is an integer pixel
            // address while a guide position is a continuous coordinate, and
            // rounding at every Image Size accumulates — 1000 -> 333 -> 1000
            // would walk a guide off the feature it marks.
            guides: doc_guide::resized_guides(&self.guides, fx, fy, w, h),
            ruler_origin: doc_guide::resized_origin(self.ruler_origin, fx, fy, w, h),
            color_samplers: doc_sampler::resized_samplers(&self.color_samplers, fx, fy, w, h),
            paths: self
                .paths
                .moved(|p| crate::doc_path::resized_paths(p, fx, fy)),
            profile: Arc::clone(&self.profile),
            metadata: self.metadata.clone(),
            // The "Resample: on" case — the pixels change and so
            // does the print size, but the ppi holds. `set_resolution` is
            // the op that moves ppi, and it never touches a pixel.
            resolution: self.resolution,
            linear_light: self.linear_light.clone(),
            comps: self.remapped_comps(
                |_, rec| Some(scaled_offset(rec, fx, fy)),
                None,
                |_| (fx * fy).sqrt(),
            ),
            background: self.background,
        };
        // The follow hook (`doc_vector_mask`): a raster-less vector mask is
        // rebuilt from its shape scaled by the canvas's own factors, as
        // `doc_path::resized_paths` scales the path list.
        let result = self.following_vector_masks(result, |x, y| (x * fx, y * fy));
        Some(self.following_resize(result))
    }
}

pub(crate) fn saturating_i32(v: i64) -> i32 {
    v.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32
}

/// `offset` moved by `(dx, dy)`, saturating — Crop's and Canvas Size's rule
/// for a layer and for a layer comp's recorded offset alike.
pub(crate) fn translated_offset(offset: (i32, i32), dx: i64, dy: i64) -> (i32, i32) {
    (
        saturating_i32(i64::from(offset.0) + dx),
        saturating_i32(i64::from(offset.1) + dy),
    )
}

/// `offset` scaled by `(fx, fy)` and rounded, saturating — Image Size's rule
/// for a layer and for a layer comp's recorded offset alike.
pub(crate) fn scaled_offset(offset: (i32, i32), fx: f64, fy: f64) -> (i32, i32) {
    (
        saturating_i32((f64::from(offset.0) * fx).round() as i64),
        saturating_i32((f64::from(offset.1) * fy).round() as i64),
    )
}

/// `Some((0, 0))` for a GROUP entry, `None` for a raster one — the offset the
/// whole-document maps must give a group. A group has no pixel rect, so its
/// `offset` is a phantom: letting the maps derive one from the 1x1 dummy
/// would drift it away from (0, 0) on every rotate and crop, and every reader
/// that is later taught about groups would inherit the drift.
fn group_offset(layer: &Layer) -> Option<(i32, i32)> {
    (layer.kind == LayerKind::Group).then_some((0, 0))
}

// ------------------------------------------------------------ layer masks --

/// What a freshly added layer mask is filled with.
pub enum MaskKind<'a> {
    /// Fully visible everywhere (255).
    RevealAll,
    /// Fully hidden everywhere (0).
    HideAll,
    /// A CANVAS-sized coverage buffer (`width * height` bytes, row 0 top —
    /// the selection convention shared with `bucket_fill` and `gradient`),
    /// cropped to the layer's rect. Layer pixels lying outside the canvas get
    /// 0, since a selection never extends past the canvas.
    FromSelection(&'a [u8]),
}

impl DrDocument {
    /// Gives entry `idx` a mask (replacing any existing one) and enables it.
    /// The mask is created at exactly the layer's pixel dimensions — or, for
    /// a GROUP, at the CANVAS's, which is the size a group mask always has
    /// (`doc_group`): a group has no pixel buffer to be the size of. All three
    /// [`MaskKind`]s honour that; `FromSelection` is canvas-sized already, so
    /// on a group it is taken verbatim rather than cropped to anything.
    /// `None` on an out-of-range index or a `FromSelection` buffer that is
    /// not canvas-sized. A MASK edit under `doc_lock`: only "Lock All"
    /// refuses it, so a pixel-locked layer's mask stays editable.
    pub fn add_mask(&self, idx: usize, kind: MaskKind) -> Option<Self> {
        self.under_locks(idx, EditKind::Mask, |doc| {
            let layer = doc.layer(idx)?;
            let (lw, lh) = if layer.kind == LayerKind::Group {
                (doc.width, doc.height)
            } else {
                layer.pixels.dimensions()
            };
            let (off_x, off_y) = match group_offset(layer) {
                Some((x, y)) => (i64::from(x), i64::from(y)),
                None => (i64::from(layer.offset.0), i64::from(layer.offset.1)),
            };
            let properties = crate::doc_mask::MaskProperties::at((off_x as i32, off_y as i32));
            if !properties.valid((lw, lh))
                || !doc.native_masks_fit_dimensions(doc.layers.iter().enumerate().filter_map(
                    |(i, l)| {
                        if i == idx {
                            Some((lw, lh))
                        } else {
                            l.mask.as_ref().map(|m| m.dimensions())
                        }
                    },
                ))
            {
                return None;
            }
            let mask = match kind {
                MaskKind::RevealAll => GrayImage::from_pixel(lw, lh, Luma([255])),
                MaskKind::HideAll => GrayImage::new(lw, lh),
                MaskKind::FromSelection(sel) => {
                    let canvas_px = (doc.width as usize).checked_mul(doc.height as usize)?;
                    if sel.len() != canvas_px {
                        return None;
                    }
                    GrayImage::from_fn(lw, lh, |x, y| {
                        let cx = i64::from(x) + off_x;
                        let cy = i64::from(y) + off_y;
                        if cx < 0
                            || cy < 0
                            || cx >= i64::from(doc.width)
                            || cy >= i64::from(doc.height)
                        {
                            return Luma([0]);
                        }
                        Luma([sel[cy as usize * doc.width as usize + cx as usize]])
                    })
                }
            };
            doc.with_layer(idx, |l| {
                l.mask = Some(Arc::new(mask));
                l.mask_enabled = true;
                l.mask_properties = properties;
                // A fresh group mask is canvas-ALIGNED, so the offset the old
                // one had travelled to goes with it (`doc_align`).
                if l.kind == LayerKind::Group {
                    l.offset = (0, 0);
                }
            })
        })
    }

    /// Drops layer `idx`'s mask. With `apply`, the mask is first baked into
    /// the layer's alpha (`alpha' = alpha * mask / 255`, straight alpha)
    /// REGARDLESS of `mask_enabled` — "apply" always means what it says — so
    /// the projection is unchanged for an enabled mask. `None` if the layer
    /// has no mask, and `None` for `apply` on a GROUP — a group has no pixels
    /// to bake a mask into, so "apply" has nothing to mean there. A MASK edit
    /// under `doc_lock`: applying a mask is a mask operation, and treating
    /// it as a pixel edit would make "Lock Pixels" mean something different
    /// here than everywhere else. With `apply` it is
    /// the STRONGER `EditKind::MaskApply`, because that variant multiplies the
    /// coverage into the layer's alpha and Lock Transparency exists precisely
    /// to make that impossible.
    pub fn remove_mask(&self, idx: usize, apply: bool) -> Option<Self> {
        let kind = if apply {
            EditKind::MaskApply
        } else {
            EditKind::Mask
        };
        self.under_locks(idx, kind, |doc| {
            let layer = doc.layer(idx)?;
            layer.mask.as_deref()?;
            if apply && layer.kind == LayerKind::Group {
                return None;
            }
            let baked = apply.then(|| {
                let mask = layer
                    .effective_mask_rect(layer.offset, layer.pixels.width(), layer.pixels.height())
                    .expect("existing mask");
                let mut pixels = layer.pixels.to_contiguous()?;
                for (px, cov) in pixels.pixels_mut().zip(mask.pixels()) {
                    px[3] = (f32::from(px[3]) * f32::from(cov[0]) / 255.0).round() as u8;
                }
                Ok::<_, String>(pixels)
            });
            let baked = baked.transpose().ok()?;
            doc.with_layer(idx, |l| {
                if let Some(pixels) = baked {
                    l.pixels = l.pixels.rebuilt(pixels);
                }
                l.mask = None;
                l.mask_enabled = true;
                l.mask_properties = Default::default();
                // A group's offset positions its mask and means nothing
                // without one (`doc_align`).
                if l.kind == LayerKind::Group {
                    l.offset = (0, 0);
                }
            })
        })
    }

    /// Enables or disables layer `idx`'s mask (a disabled mask is retained but
    /// ignored while compositing). `None` if the layer has no mask. A MASK
    /// edit under `doc_lock`.
    pub fn set_mask_enabled(&self, idx: usize, enabled: bool) -> Option<Self> {
        self.under_locks(idx, EditKind::Mask, |doc| {
            doc.layer(idx)?.mask.as_ref()?;
            doc.with_layer(idx, |l| l.mask_enabled = enabled)
        })
    }

    /// Paints layer `idx`'s mask with a canvas-frame PREMULTIPLIED RGBA8
    /// overlay (`overlay`, exactly canvas w*h*4 bytes — the same buffer
    /// `painting_layer` takes), mapped through the layer's offset: each mask
    /// pixel samples the overlay at its canvas position, and
    /// `mask' = round(lerp(mask, luma(straight colour), overlay alpha))`.
    /// Painting white therefore reveals and black hides, with the stroke's
    /// own anti-aliasing (and any selection clipping the caller already
    /// applied to the overlay) carried by the alpha. Overlay pixels outside
    /// the layer are ignored; `None` if the layer has no mask, the buffer is
    /// the wrong size, or the layer's extent misses the canvas entirely (no
    /// mask pixel could change), matching `painting_layer`. A GROUP's mask is
    /// CANVAS-sized and sits at the group's own offset — which is exactly what
    /// that offset positions (`doc_align`) — so the same walk covers it with
    /// the canvas as its frame. A MASK edit under `doc_lock`.
    pub fn paint_mask(&self, idx: usize, overlay: &[u8]) -> Option<Self> {
        self.under_locks(idx, EditKind::Mask, |doc| {
            let layer = doc.layer(idx)?;
            let mask = layer.mask.as_deref()?;
            let expected = (doc.width as usize)
                .checked_mul(doc.height as usize)?
                .checked_mul(4)?;
            if overlay.len() != expected {
                return None;
            }
            let (lw, lh) = mask.dimensions();
            let (off_x, off_y) = (
                i64::from(layer.mask_properties.offset.0),
                i64::from(layer.mask_properties.offset.1),
            );
            let lx0 = (-off_x).max(0);
            let ly0 = (-off_y).max(0);
            let lx1 = (i64::from(doc.width) - off_x).min(i64::from(lw));
            let ly1 = (i64::from(doc.height) - off_y).min(i64::from(lh));
            if lx0 >= lx1 || ly0 >= ly1 {
                return None;
            }
            let mut painted = mask.clone();
            let raw: &mut [u8] = &mut painted;
            for ly in ly0..ly1 {
                for lx in lx0..lx1 {
                    let cx = (lx + off_x) as u64;
                    let cy = (ly + off_y) as u64;
                    let si = ((cy * u64::from(doc.width) + cx) * 4) as usize;
                    let sp = [
                        overlay[si],
                        overlay[si + 1],
                        overlay[si + 2],
                        overlay[si + 3],
                    ];
                    let di = (ly as u64 * u64::from(lw) + lx as u64) as usize;
                    // `doc_plane::channel_lerp` IS this rule (it was lifted
                    // from here), now shared with channel and colour-plane
                    // painting; None means the overlay pixel is transparent
                    // and the mask byte is left alone. This loop deliberately
                    // keeps `paint_mask`'s historic lack of a "nothing
                    // changed" latch — the channel paints have one — so the
                    // refactor changes no behaviour here.
                    if let Some(v) = channel_lerp(raw[di], sp) {
                        raw[di] = v;
                    }
                }
            }
            doc.with_layer(idx, |l| l.mask = Some(Arc::new(painted)))
        })
    }

    /// Layer `idx`'s mask expanded to an opaque grayscale RGBA image (for the
    /// layers-panel thumbnail); `None` if the layer has no mask.
    pub fn mask_image(&self, idx: usize) -> Option<RgbaImage> {
        let layer = self.layer(idx)?;
        let raw = layer.mask.as_deref()?;
        let canvas_mask = (layer.kind == LayerKind::Group).then(|| {
            doc_channel::padded_plane_with_coverage(
                raw,
                self.width,
                self.height,
                layer.mask_properties.offset,
                layer.mask_properties.outside_coverage,
            )
        });
        let mask = canvas_mask.as_ref().unwrap_or(raw);
        let (mw, mh) = mask.dimensions();
        Some(RgbaImage::from_fn(mw, mh, |x, y| {
            let v = mask.get_pixel(x, y)[0];
            image::Rgba([v, v, v, 255])
        }))
    }
}

// ------------------------------------------------------------------- open --

impl DrDocument {
    /// Opens a document, sniffing the container: "DRDC" (written; legacy
    /// "RZDC" read) is the native format,
    /// "8BPS" a Photoshop document (layered import), anything else decodes
    /// via the `DrImage::open_bytes` rules to a single "Background" layer —
    /// except a multipage TIFF, whose pages become layers "Page 1"… with only
    /// page 1 visible (`doc_tiff_pages`).
    ///
    /// A FLAT open also lifts the file's sidecar onto the document: its
    /// embedded ICC profile (from the decoder, which reassembles JPEG APP2
    /// chunks and inflates PNG `iCCP` for every format it reads), its EXIF,
    /// XMP and IPTC packets and its print resolution (from `metadata`'s
    /// JPEG/PNG container walk). A `.rz` carries all of that itself; a PSD
    /// captures none of it — `psd` 0.3.5 exposes only the Slices image
    /// resource, so the resolution (id 1005) and the ICC profile (id 1039)
    /// are unreachable.
    ///
    /// This REPORTS what the file said and adopts no working space: an
    /// embedded profile that differs from the host's preference is converted
    /// by `adopt_working_space`, once, by the caller. The one exception is a
    /// profile a document cannot carry but the open can convert from — a
    /// Gray profile on a grayscale image, a CMYK profile on a CMYK TIFF —
    /// whose pixels `open_bytes` has already converted to sRGB.
    pub fn open(path: &str) -> Result<Self, String> {
        let mut bytes = std::fs::read(path).map_err(|e| format!("failed to read {path}: {e}"))?;
        if crate::native_magic::is_native_magic(&bytes) {
            crate::drdc::parse_native(&bytes)
                .map_err(|e| format!("failed to read document {path}: {e}"))
        } else if bytes.len() >= 4 && &bytes[..4] == b"8BPS" {
            crate::psd::open_psd(&bytes, path)
        } else {
            // A TIFF holding more than one image opens each page as a layer
            // (`tiff_pages`, `doc_tiff_pages`).
            if crate::metadata_tiff::is_tiff(&bytes) {
                if let Some(pages) = crate::tiff_pages::read(&bytes) {
                    return Self::open_tiff_pages(&mut bytes, &pages, path);
                }
            }
            Self::open_flat(&bytes, path)
        }
    }

    /// The flat branch of [`Self::open`] over bytes already read: a one-image
    /// file, or page 1 of a multipage TIFF made the file's only page
    /// (`tiff_pages::with_page`).
    pub(crate) fn open_flat(bytes: &[u8], path: &str) -> Result<Self, String> {
        let (image, sidecar) = DrImage::open_bytes(bytes, path)?;
        let mut doc = DrDocument::from_pixels(image.pixels);
        // The sidecar carries only a profile the open KEPT: one it
        // converted from (Gray, CMYK) or ignored (any other non-RGB
        // profile, or one past the blob cap, which a zlib-compressed PNG
        // `iCCP` can inflate to) is absent, the pixels are sRGB's
        // numbers, and the import report names it (`open_profile`).
        // Parsing again here is the backstop: `IccProfile::parse`
        // is where every refusal lives, so this path can never build a
        // document the native writer would refuse to save.
        if let Some(profile) = sidecar.icc.as_deref().and_then(IccProfile::parse) {
            doc.profile = Arc::new(profile);
        }
        doc.metadata = sidecar.metadata;
        if let Some(resolution) = sidecar.resolution {
            doc.resolution = resolution;
        }
        // A TIFF's spot channels (#1 1c-2a, `tiff_spot`).
        if crate::metadata_tiff::is_tiff(bytes) {
            return doc.with_tiff_spots(bytes);
        }
        Ok(doc)
    }
}
