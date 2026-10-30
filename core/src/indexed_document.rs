//! The Indexed Color document mode (#547 Indexed phase 1a): the policy, the
//! edit gate and the refusal sentences (`designs/indexed-mode.md`).
//!
//! # What an Indexed document IS
//!
//! A `PrecisionDocument` whose `indexed` field is `Some` (the `gray` and
//! `lab` fields' pattern). The policy holds the palette (2–256 RGB entries,
//! duplicates allowed), the optional transparent index and the canvas-sized
//! index plane. The storage is the RGB precision document of ONE raster
//! layer, "Index", full canvas at offset 0, holding the policy's render:
//! each pixel `palette[i] × 257` with alpha 65535, alpha 0 at the
//! transparent index ([`IndexedPolicy::render`]). The document's profile is
//! an RGB profile, the palette's space, so the canvas, `render`, thumbnails
//! and the RGB proof read the palette colours through it with no Indexed arm
//! of their own. A Grayscale source's twin becomes that profile.
//!
//! # The gate
//!
//! An Indexed Color document holds one layer and edits its pixels only in
//! palette colours. Phases 1a to 2a edited nothing; since phase 2b
//! [`admit_edit`], asked by `ffi_precision_edit::request` and `ffi_retouch`,
//! admits the Brush in Normal mode (`paint_blend`, which `request` routes to
//! `indexed_paint`: the RGB paint, then the re-index of `indexed_edge`).
//! Since phase 2c it also admits Fill, Stroke, the Paint Bucket, the Eraser
//! and Clear ([`IndexedRoute::Paint`], `indexed_paint`; Cut is
//! `indexed_paint::cut`, asked by `ffi_clipboard`) and the whole-canvas
//! geometry ([`IndexedRoute::Geometry`], `indexed_geometry`: exact for a
//! move or a Nearest pick, else re-indexed). Phase 2d, which completed
//! phase 2, added the layer transforms (also `indexed_geometry`), the
//! selected pixels' transform ([`IndexedRoute::Selected`],
//! `indexed_transform`), the layer row's rename, label and lock
//! ([`IndexedRoute::Structure`], the shared structure edit), and outside
//! the request API Warp (`precision_warp`'s arms, `indexed_transform`),
//! Paste and Paste Special (`indexed_clipboard`, asked by
//! `ffi_clipboard::dispatch_paste` and `ffi_indexed`) and Export Layers
//! (`indexed_export`); Duplicate Layer and a layers paste refuse
//! (`precision_document_edit`, `doc_paste_layers`). Phase 3a added the
//! Move tool with a selection (`move_selected`, [`IndexedRoute::Selected`];
//! the layer move is `affine_set`), the selected pixels of an alpha channel,
//! the layer row's visibility, order and disclosure (`layer_visible`,
//! `move_layer_to`, `layer_open`, [`IndexedRoute::Structure`]) and, outside
//! the request API, Assign Profile and Convert to Profile
//! (`indexed_profile`). Phase 3b admitted the paint tools in every blend
//! mode (`indexed_edge::EdgeInk::Blend`), the source selections that sample
//! the palette colours, an alpha channel's set and read
//! ([`IndexedRoute::Shared`]) and the background colour a clear pixel takes
//! with no transparent index (`indexed_paint::BACKGROUND_KEY`); HDR Toning
//! and Auto-Blend Layers refuse for good. Phase 3c admitted Clone Stamp
//! (`indexed_paint`, re-indexed `Own`), the Magic Wand, Quick Selection, the
//! alpha channels' commands and scalar edits ([`IndexedRoute::Shared`]) and,
//! outside the request API, the History Brush (`indexed_paint`, asked by
//! `precision_retouch_brush`); the Gradient, the retouch brushes, Refine
//! Edge, the Global Light, Stack Modes, Arrange and Link refuse for good.
//! That gate, with the host's own gate in front of it, is
//! what keeps the plane and the layer together: `PrecisionDocument::rebuilt`
//! carries the plane unconditionally, so a core op reaching it outside
//! `request` would leave a stale plane, which the `.rz` writer's
//! `indexed_native::check` then refuses rather than writing.
//! [`refused_sentence`] names what is not available in Indexed Color
//! (`DISABLED_OPS`, for good), what needs more than the one layer
//! (`ONE_LAYER`, for good, phase 3a) and, since phase 3c, everything else as
//! offered on RGB documents only (for good). Phase 2a's list was checked
//! against Photoshop's Indexed Color menus
//! (`scripts/photoshop-probe/indexed_mode_take.py`, take1). The host's
//! `IndexedDocument` mirrors these lists by tool name, and
//! `precision_capability::INDEXED_FAMILIES` is the admitted family table
//! (render, sample, convert, since phase 1b export, since phase 2b paint,
//! since phase 2c erase, Fill and Stroke, Copy, Cut and the geometry, since
//! phase 2d Paste, the transforms, Warp and the layer row, since phase 3a
//! the Move tool's selected pixels, the row's visibility and order and the
//! profile commands, since phase 3b the source selections, an alpha
//! channel's set and its plane read, since phase 3c the channel commands,
//! Clone Stamp and the histogram).
//!
//! Phase 1c's Image ▸ Mode ▸ Color Table replaces the palette and the
//! transparent index over the same plane ([`IndexedPolicy::with_table`],
//! `indexed_table`), re-rendering the one layer; it is not an edit request.

use std::sync::Arc;

use serde_json::Value;

use crate::icc::IccProfile;
use crate::indexed_palette::{Rgb, MAX_COLORS, MIN_COLORS};
use crate::job_control::JobContext;
use crate::precision_document::{Content, PrecisionDocument};
use crate::precision_image::{PrecisionImage, PrecisionRun};
use crate::tile_store::TilePlane;

/// The storage-free name this mode reports in descriptors.
pub(crate) const COLOR_MODE_INDEXED: &str = "indexed";

/// The name of an Indexed document's one layer.
pub(crate) const LAYER_NAME: &str = "Index";

/// The edits Indexed Color refuses for good: since phase 2a new layers,
/// Layer via Copy and Cut, Duplicate Layer, New Fill Layer, Perspective
/// Warp, Content-Aware Fill and Liquify, the merges and stamps, masks, styles,
/// blend modes, clipping, adjustments, filters and artwork, since phase 3a a
/// layers paste (new layers are refused, so a
/// paste cannot add layers), Puppet Warp with or without a selection and a
/// mask transform (no layer mask), since phase 3b HDR Toning (with the
/// other adjustments), and since phase 3c the Gradient tool, the retouch
/// tools Doctored does not run on indexed colour (Healing Brush, Spot
/// Healing, Content-Aware Move, Red Eye, Dodge, Burn, Sponge, Blur, Sharpen,
/// Smudge), Refine Edge, the Global Light (no layer style) and Stack Modes
/// (no Smart Object).
const DISABLED_OPS: &[&str] = &[
    "add_layer",
    "adjustment",
    "adjustment_layer",
    "apply_mask",
    "artwork",
    "content_aware_fill",
    "content_aware_move",
    "dodge_burn",
    "duplicate_layer",
    "duplicate_to_new_document",
    "filter",
    "global_light",
    "gradient",
    "group_layers",
    "hdr_toning",
    "heal",
    "layer_blend",
    "layer_blend_if",
    "layer_clipped",
    "layer_style",
    "layer_via",
    "liquify",
    "mask_properties",
    "merge_down",
    "merge_group",
    "merge_layers",
    "merge_visible",
    "paste_layers",
    "perspective_selected",
    "puppet_selected",
    "puppet_warp",
    "rasterize",
    "red_eye",
    "refine_edge",
    "remove_mask",
    "retouch_brush",
    "retouch_heal",
    "set_mask",
    "spot_heal",
    "stack_mode",
    "stamp_selected",
    "stamp_visible",
    "transform_mask",
    "ungroup_layer",
];

/// The edits that need more than one layer (phase 3a): Align and Distribute,
/// Layer ▸ Delete and Flatten Image, since phase 3b Edit ▸ Auto-Blend Layers
/// (Auto-Align Layers is the host's, which names the same sentence), and
/// since phase 3c Arrange and Link Layers. Each needs a second layer, and
/// an Indexed document holds one, so each refuses for good with the
/// one-layer sentence.
const ONE_LAYER: &[&str] = &[
    "align_set",
    "arrange_layer",
    "auto_blend_layers",
    "distribute_set",
    "flatten",
    "link_layers",
    "remove_layers",
    "unlink_layers",
];

/// The one refusal sentence of the mode (module doc).
pub(crate) fn refused_sentence(operation: &str) -> String {
    if DISABLED_OPS.contains(&operation) {
        return format!("{operation} is not available in Indexed Color.");
    }
    if ONE_LAYER.contains(&operation) {
        return format!(
            "{operation} needs more than one layer, and an Indexed Color document holds one."
        );
    }
    format!(
        "{operation} is unavailable in an Indexed Color document: Doctored offers it on RGB \
         documents only."
    )
}

/// A selected-pixel move or transform of a colour plane (phase 3a): an
/// Indexed document's Channels panel shows one channel, Index, so there is
/// no red, green, blue or alpha plane to float.
pub(crate) const INDEX_CHANNEL_REFUSAL: &str = "An Indexed Color document has one channel, Index: it has no red, green, blue or alpha plane to edit; target the layer.";

/// Where [`admit_edit`] sends an admitted request.
pub(crate) enum IndexedRoute {
    /// `indexed_paint`: the paint family, re-indexed by `indexed_edge`.
    Paint,
    /// `indexed_geometry`: the whole-canvas geometry and, since phase 2d,
    /// the layer transforms.
    Geometry,
    /// `indexed_transform::IndexedSelectedEdit`: the selected pixels'
    /// transform (phase 2d) and move (phase 3a).
    Selected,
    /// The shared structure edit, which keeps the pixels and the plane: the
    /// layer row's rename, label and lock (phase 2d), visibility, order and
    /// disclosure (phase 3a; the order and disclosure change nothing on the
    /// one raster).
    Structure,
    /// The shared parser, whose edit leaves the layer and the plane
    /// untouched (phase 3b): the source selections that sample the palette
    /// colours (`color_range`, `similar`, `alpha`), an alpha channel's set
    /// (Save Selection, Quick Mask) and its plane read (Load Selection);
    /// since phase 3c the Magic Wand, Quick Selection, the alpha channels'
    /// commands and an alpha channel's scalar edit and statistics.
    Shared,
}

/// The gate `ffi_precision_edit::request` and `ffi_retouch` ask on an
/// Indexed document. Since phase 2b it admits the Brush (`paint_blend`);
/// since phase 2c also the Paint Bucket (`bucket_fill`) and Stroke… with
/// nothing selected (`stroke_layer_edge`), since phase 3b in every blend
/// mode (the kernels refuse an invalid one), the scalar
/// paint and erase (`paint`, `erase`) and the Eraser (`erase_native`), all
/// [`IndexedRoute::Paint`]; and the whole-canvas geometry and Perspective
/// Crop, [`IndexedRoute::Geometry`]. Since phase 2d also the layer
/// transforms (`affine`, `perspective`, `affine_set`, `perspective_set`,
/// [`IndexedRoute::Geometry`]), `transform_selected`
/// ([`IndexedRoute::Selected`]) and the layer row's `rename_layer`,
/// `layer_label` and `layer_lock` ([`IndexedRoute::Structure`]). Since phase
/// 3a also `move_selected` ([`IndexedRoute::Selected`]) and `layer_visible`,
/// `move_layer_to` and `layer_open` ([`IndexedRoute::Structure`]). Since
/// phase 3b also the source selections `color_range`, `similar` and `alpha`
/// (the palette colours sampled), `set_channel` and `read_plane` of an
/// alpha channel ([`IndexedRoute::Shared`]); a selection or read of a
/// colour channel, or
/// of the composite's luminosity, refuses with [`INDEX_CHANNEL_REFUSAL`].
/// Since phase 3c also Clone Stamp (`retouch_clone`, [`IndexedRoute::Paint`]),
/// the Magic Wand and Quick Selection (`magic_wand`, `quick`), the alpha
/// channels' commands (`remove_channel`, `rename_channel`,
/// `duplicate_channel`, `channel_properties`, `import_channel`) and an alpha
/// channel's `scalar_edit` and `scalar_statistics` ([`IndexedRoute::Shared`]);
/// Refine Edge refuses for good. Every other request refuses by
/// [`refused_sentence`] of its `op`.
pub(crate) fn admit_edit(text: &str) -> Result<IndexedRoute, String> {
    let value: Value =
        serde_json::from_str(text).map_err(|e| format!("invalid high-depth edit request: {e}"))?;
    let op = value["op"].as_str().unwrap_or("edit");
    match op {
        "paint_blend" | "bucket_fill" | "stroke_layer_edge" => Ok(IndexedRoute::Paint),
        "paint" | "erase" | "erase_native" | "retouch_clone" => Ok(IndexedRoute::Paint),
        "selection" => match value["operation"]["kind"].as_str() {
            Some("color_range" | "similar" | "alpha" | "magic_wand" | "quick") => {
                Ok(IndexedRoute::Shared)
            }
            Some("refine_edge") => Err(refused_sentence("refine_edge")),
            Some("color" | "luminance") => Err(INDEX_CHANNEL_REFUSAL.into()),
            _ => Err(refused_sentence(op)),
        },
        "set_channel" | "remove_channel" | "rename_channel" | "duplicate_channel"
        | "channel_properties" | "import_channel" => Ok(IndexedRoute::Shared),
        "read_plane" | "scalar_edit" | "scalar_statistics" => match value["target"].as_str() {
            Some("channel") => Ok(IndexedRoute::Shared),
            Some("red" | "green" | "blue" | "alpha") => Err(INDEX_CHANNEL_REFUSAL.into()),
            _ => Err(refused_sentence(op)),
        },
        "resize" | "crop" | "canvas_size" | "straighten" | "rotate_canvas" | "rotate90"
        | "rotate180" | "rotate270" | "flip_horizontal" | "flip_vertical" | "perspective_crop"
        | "affine" | "perspective" | "affine_set" | "perspective_set" => Ok(IndexedRoute::Geometry),
        "transform_selected" | "move_selected" => Ok(IndexedRoute::Selected),
        "rename_layer" | "layer_label" | "layer_lock" | "layer_visible" | "move_layer_to"
        | "layer_open" => Ok(IndexedRoute::Structure),
        _ => Err(refused_sentence(op)),
    }
}

/// The Indexed mode's policy (module doc).
#[derive(Clone)]
pub struct IndexedPolicy {
    palette: Vec<Rgb>,
    transparent: Option<u8>,
    indices: Arc<Vec<u8>>,
    width: u32,
    height: u32,
}

impl IndexedPolicy {
    /// A policy of `palette` (2..=256 entries), `transparent` (an index of
    /// it) and a `width`×`height` plane of indices into it.
    pub(crate) fn new(
        palette: Vec<Rgb>,
        transparent: Option<u8>,
        indices: Vec<u8>,
        width: u32,
        height: u32,
    ) -> Result<Self, String> {
        if !(MIN_COLORS..=MAX_COLORS).contains(&palette.len()) {
            return Err("an Indexed Color palette holds 2 to 256 colours".into());
        }
        if transparent.is_some_and(|t| usize::from(t) >= palette.len()) {
            return Err("the Indexed Color transparent index is outside the palette".into());
        }
        if indices.len() as u64 != u64::from(width) * u64::from(height) {
            return Err("the Indexed Color index plane does not match the canvas".into());
        }
        if indices.iter().any(|&i| usize::from(i) >= palette.len()) {
            return Err("an Indexed Color index is outside the palette".into());
        }
        Ok(Self {
            palette,
            transparent,
            indices: Arc::new(indices),
            width,
            height,
        })
    }

    /// This policy with `palette` and `transparent` in place of its own, the
    /// plane shared (Image ▸ Mode ▸ Color Table, `indexed_table`). A palette
    /// shorter than the plane's largest index refuses, naming both.
    pub(crate) fn with_table(
        &self,
        palette: Vec<Rgb>,
        transparent: Option<u8>,
    ) -> Result<Self, String> {
        if !(MIN_COLORS..=MAX_COLORS).contains(&palette.len()) {
            return Err(crate::indexed_act::TABLE_SIZE_SENTENCE.into());
        }
        if transparent.is_some_and(|t| usize::from(t) >= palette.len()) {
            return Err("the Indexed Color transparent index is outside the palette".into());
        }
        let used = self.max_index();
        if usize::from(used) >= palette.len() {
            return Err(format!(
                "The colour table holds {} colours, but the image uses index {used}; it needs at \
                 least {}.",
                palette.len(),
                u16::from(used) + 1
            ));
        }
        Ok(Self {
            palette,
            transparent,
            indices: self.indices.clone(),
            width: self.width,
            height: self.height,
        })
    }

    /// The largest index the plane uses (0 for an empty plane).
    pub(crate) fn max_index(&self) -> u8 {
        self.indices.iter().copied().max().unwrap_or(0)
    }

    pub(crate) fn palette(&self) -> &[Rgb] {
        &self.palette
    }

    pub(crate) fn transparent(&self) -> Option<u8> {
        self.transparent
    }

    pub(crate) fn indices(&self) -> &[u8] {
        &self.indices
    }

    pub(crate) fn dimensions(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    /// The index at `(x, y)`; `None` off the plane.
    pub(crate) fn index_at(&self, x: u32, y: u32) -> Option<u8> {
        if x >= self.width || y >= self.height {
            return None;
        }
        self.indices
            .get(y as usize * self.width as usize + x as usize)
            .copied()
    }

    /// One index's RGBA16 sample: `palette[i] × 257`, alpha 65535, alpha 0
    /// at the transparent index.
    pub(crate) fn sample(&self, index: u8) -> [u16; 4] {
        let rgb = self.palette[usize::from(index)];
        let alpha = if Some(index) == self.transparent {
            0
        } else {
            65535
        };
        [
            u16::from(rgb[0]) * 257,
            u16::from(rgb[1]) * 257,
            u16::from(rgb[2]) * 257,
            alpha,
        ]
    }

    /// The "Index" layer's pixels in `profile` (module doc).
    pub(crate) fn render(
        &self,
        profile: &Arc<IccProfile>,
        job: JobContext<'_>,
    ) -> Result<PrecisionImage, String> {
        let width = self.width as usize;
        let plane = TilePlane::<[u16; 4]>::from_rows_with_job(
            self.width,
            self.height,
            |y, row| {
                let start = y as usize * width;
                for (pixel, &index) in row.iter_mut().zip(&self.indices[start..start + width]) {
                    *pixel = self.sample(index);
                }
                job.every(start)
            },
            job,
        )?;
        PrecisionImage::from_rgba16_tiles_unseeded(plane, profile.clone())
    }

    /// Whether `image` is exactly this policy's render (the native reader's
    /// check, `indexed_native`).
    pub(crate) fn renders(
        &self,
        image: &PrecisionImage,
        job: JobContext<'_>,
    ) -> Result<bool, String> {
        if image.dimensions() != (self.width, self.height) {
            return Ok(false);
        }
        let width = self.width as usize;
        for y in 0..self.height {
            let mut x = y as usize * width;
            for (_, run) in image.row_segments(y, 0, self.width) {
                let PrecisionRun::Rgba16(run) = run else {
                    return Ok(false);
                };
                for pixel in run {
                    if *pixel != self.sample(self.indices[x]) {
                        return Ok(false);
                    }
                    x += 1;
                }
            }
            job.every(y as usize * width)?;
        }
        Ok(true)
    }

    pub(crate) fn record_resources(&self, inventory: &mut crate::resources::Inventory) {
        inventory.object(&*self.indices, "indexed_plane", false);
        inventory.vec(&self.indices, "indexed_plane", false);
        inventory.vec(&self.palette, "indexed_palette", false);
    }
}

/// Whether `profile` is an RGB profile an Indexed document can be in: RGB
/// data and not the Lab twin (which is RGB data holding L\*a\*b\* codes).
pub(crate) fn is_rgb_profile(profile: &IccProfile) -> bool {
    crate::icc::IccBytes::new(profile.bytes()).is_some_and(|bytes| bytes.data_space() == b"RGB ")
        && !crate::lab_document::is_lab_twin(profile)
}

impl PrecisionDocument {
    /// True for an Indexed Color document.
    pub fn is_indexed(&self) -> bool {
        self.indexed.is_some()
    }

    /// The Indexed policy; `None` for every other document.
    pub(crate) fn indexed_policy(&self) -> Option<&Arc<IndexedPolicy>> {
        self.indexed.as_ref()
    }

    /// The index at `(x, y)`; `None` off the canvas or on any other document.
    pub fn indexed_index_at(&self, x: u32, y: u32) -> Option<u8> {
        self.indexed.as_ref()?.index_at(x, y)
    }

    /// This document as an Indexed document under `policy`. Refuses a CMYK,
    /// Gray or Lab document, float storage, anything but one canvas-sized
    /// raster at offset 0 of the policy's dimensions, and a profile that is
    /// not RGB ([`is_rgb_profile`]). Alpha channels, guides, samplers, paths,
    /// metadata and resolution carry as they are.
    pub(crate) fn with_indexed(mut self, policy: Arc<IndexedPolicy>) -> Result<Self, String> {
        if self.is_cmyk() || self.is_gray() || self.is_lab() {
            return Err("an Indexed Color document is made from an RGB document".into());
        }
        if self.is_float() {
            return Err("an Indexed Color document is 8 bits per channel".into());
        }
        if self.has_spot_channel() {
            return Err(crate::spot_channel::TO_INDEXED_REFUSAL.into());
        }
        if !is_rgb_profile(&self.profile) {
            return Err("an Indexed Color document's palette must be in an RGB profile".into());
        }
        let canvas = (self.width, self.height);
        let one_raster = match self.root_layers() {
            [layer] => matches!(
                &layer.content,
                Content::Raster { image, offset: (0, 0) } if image.dimensions() == canvas
            ),
            _ => false,
        };
        if !one_raster || policy.dimensions() != canvas {
            return Err(
                "an Indexed Color document holds one canvas-sized layer at the origin".into(),
            );
        }
        self.indexed = Some(policy);
        Ok(self)
    }

    /// This document with no Indexed policy: the RGB document its storage
    /// is, every sample shared.
    pub(crate) fn without_indexed(&self) -> Self {
        let mut out = self.clone();
        out.indexed = None;
        out
    }
}
