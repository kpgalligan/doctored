//! The Bitmap document mode (#547 Bitmap phase 1a): the policy, the edit
//! gate and the refusal sentences (`designs/bitmap-mode.md`).
//!
//! # What a Bitmap document IS
//!
//! An 8-bit Grayscale `PrecisionDocument` (its `gray` policy `Some`, depth
//! 8, the source's Gray ICC and twin) whose `bitmap` field is `Some` too.
//! The policy holds the packed 1-bit plane (`bitmap_method`: `ceil(w/8)`
//! bytes a row, MSB first, 1 white, 0 black, pad bits 0), which is the
//! AUTHORITY; the storage is ONE raster layer, "Bitmap", canvas-sized at
//! the origin, opaque, holding the plane's render: each pixel
//! `[65535; 4]` white or `[0, 0, 0, 65535]` black ([`BitmapPolicy::render`],
//! Indexed's rule). So the canvas, `render`, thumbnails and the Gray proof
//! read a Bitmap document with no Bitmap arm of their own. It holds no alpha
//! channel ([`PrecisionDocument::with_bitmap`] refuses one); guides,
//! samplers, paths and metadata carry. The packed plane costs `ceil(w/8)·h`
//! bytes (12.5 MB at `MAX_PIXELS`, 10⁸) beside the layer's 8 B/px, which any
//! 8-bit Gray document already holds.
//!
//! Because a Bitmap document is a Gray document underneath, the core's
//! `is_gray()` is true on it: every gate asks [`PrecisionDocument::is_bitmap`]
//! BEFORE `is_gray()`.
//!
//! # The gate
//!
//! [`admit_edit`], asked by `ffi_precision_edit::request` and `ffi_retouch`
//! before the Gray gate, admits since phase 2a-i the edits `bitmap_edit`
//! runs on the Gray storage and puts back on the plane: the Brush, Fill,
//! Stroke and the Paint Bucket in Normal, Darken or Lighten mode (phase
//! 2b, [`BLEND_MODES`]; another `blend_mode` refuses by
//! [`BLEND_REFUSAL`]), the scalar paint, the Eraser and Clear, and the
//! bit-exact geometry (`crop`, `canvas_size`, the quarter turns, the flips
//! and a zero-angle `straighten`, the Crop tool's Delete Cropped and
//! past-canvas crop); since phase 2a-ii the resampled geometry (`resize`, a
//! `straighten` at any matrix, the layer's `affine`, `perspective`,
//! `affine_set` and `perspective_set`: Image Size, the Crop tool's
//! straighten and Move) and the selected pixels' `move_selected`. Since
//! phase 2b `transform_selected` refuses for good and the host refuses Free
//! Transform, Distort and Transform Again; the layer's four transform ops
//! stay for Move. By the controller's ruling arbitrary rotation
//! (`rotate_canvas`, the one tool Image ▸ Image Rotation ▸ Arbitrary
//! serves) refuses for good too. Phase 3 admits the Gradient (thresholded,
//! no ink snap), Perspective Crop and the Clone Stamp through `bitmap_edit`
//! ([`BitmapRoute::Edit`]; the Gradient and the Clone Stamp in
//! [`BLEND_MODES`] too), and routes the layer row's name, label and lock
//! and the source selections to the shared parser ([`BitmapRoute::Shared`]:
//! no pixel and no bit moves). A `selection` of the Magic Wand, Color
//! Range, Similar/Grow or Select and Mask's edge refuses for good. Liquify
//! and the History Brush run through `bitmap_tools`; [`admit_selected`]
//! refuses every other selected edit. Everything else refuses by
//! [`refused_sentence`] of its `op`, in three tiers, none naming a phase:
//! what is not available in Bitmap mode (`DISABLED_OPS`: Indexed Color's
//! list, `duplicate_layer` and the Magic Wand, revised in phase 2b), what
//! needs a second layer (`ONE_LAYER`), and what Doctored offers on
//! Grayscale and colour documents only. That
//! gate, with the host's `BitmapDocument` in front of it, keeps the plane
//! and the layer together: `PrecisionDocument::rebuilt` carries the plane,
//! so a core op reaching it
//! outside the gate would leave a stale plane, which the `.rz` writer's
//! `bitmap_native::check` then refuses rather than writing.
//! `precision_capability::BITMAP_FAMILIES` is the admitted family table
//! (render, sample, convert, since phase 1c export: the 1-bit PNG, TIFF,
//! BMP and PSD files of `bitmap_io`, `bitmap_export` and `bitmap_psd`,
//! which are no edit requests, since phase 2a-i the paint and geometry
//! families above, since phase 2a-ii Cut, the resampled geometry and the
//! selected pixels' moves, and since phase 3 Copy, the layer row's name,
//! label and lock, the source selections, the Clone Stamp and the
//! histogram).

use std::sync::Arc;

use serde_json::Value;

use crate::bitmap_method::{is_white, stride};
use crate::icc::IccProfile;
use crate::job_control::JobContext;
use crate::precision_document::{Content, PrecisionDocument};
use crate::precision_image::{PrecisionImage, PrecisionRun};
use crate::selected_adjustment::SelectedEdit;
use crate::tile_store::TilePlane;

/// The storage-free name this mode reports in descriptors.
pub(crate) const COLOR_MODE_BITMAP: &str = "bitmap";

/// The name of a Bitmap document's one layer.
pub(crate) const LAYER_NAME: &str = "Bitmap";

/// Where an admitted request runs on a Bitmap document ([`admit_edit`]).
pub(crate) enum BitmapRoute {
    /// `bitmap_edit`: the Gray storage's op, then `republish`.
    Edit,
    /// The shared parser (`Edit::parse`), whose edits leave the layer's
    /// pixels and the plane untouched (the layer row's name, label and lock,
    /// a source selection's coverage), or answer as on every document.
    Shared,
}

/// The edits phases 2a-i and 2a-ii admit (module doc), as phase 2b left
/// them: `transform_selected` went to [`DISABLED_OPS`] with Edit ▸
/// Free Transform and Transform and `rotate_canvas` with Image ▸ Image
/// Rotation ▸ Arbitrary, while the layer's `affine`, `perspective`,
/// `affine_set` and `perspective_set` stay for the Move tool and `crop` for
/// the Crop tool; the blend-mode paints only in [`BLEND_MODES`]. Phase 3
/// adds the Gradient, Perspective Crop and the Clone Stamp.
const ADMITTED: &[&str] = &[
    "paint_blend",
    "bucket_fill",
    "stroke_layer_edge",
    "paint",
    "erase",
    "erase_native",
    "crop",
    "canvas_size",
    "rotate90",
    "rotate180",
    "rotate270",
    "flip_horizontal",
    "flip_vertical",
    "straighten",
    "resize",
    "affine",
    "perspective",
    "affine_set",
    "perspective_set",
    "move_selected",
    "gradient",
    "perspective_crop",
    "retouch_clone",
];

/// The edits phase 3 routes to the shared parser ([`BitmapRoute::Shared`]):
/// the layer row's name, label and lock, which change no pixel, and
/// `liquify`, which is no edit-request op on any document (the shared
/// parser answers it; Liquify runs through the selected edit, [`admit_selected`]).
const SHARED: &[&str] = &["rename_layer", "layer_label", "layer_lock", "liquify"];

/// The edits that need a second layer, which a Bitmap document never holds
/// (Indexed's `ONE_LAYER` shape): they refuse for good by the one-layer
/// reason.
const ONE_LAYER: &[&str] = &[
    "align_set",
    "arrange_layer",
    "auto_blend_layers",
    "distribute_set",
    "flatten",
    "link_layers",
    "move_layer_to",
    "remove_layers",
    "unlink_layers",
];

/// The core blend codes (`blend::BlendMode`) the Brush, Fill, Stroke, the
/// Paint Bucket, the Gradient and the Clone Stamp take on a Bitmap
/// document: Normal, Darken and Lighten.
/// Phase 2b's walk (`bitmap_take.py`'s walk2): in Photoshop a black fill
/// over a binary selection on a Bitmap document gave the W3C formula's
/// black count in each of the three. Dissolve has no prediction (its count
/// equalled Normal's at 100 %), so it refuses.
const BLEND_MODES: &[i64] = &[0, 6, 7];

/// The blend-mode paints' refusal of any mode but [`BLEND_MODES`].
pub(crate) const BLEND_REFUSAL: &str = "The Brush, Fill, Stroke, the Paint Bucket, the Gradient \
                                        and the Clone Stamp paint in Normal, Darken or Lighten \
                                        mode in a Bitmap document.";

/// The edits Bitmap mode, which holds one layer as Indexed Color does,
/// refuses for good: Indexed's list (`indexed_document`) less its retouch
/// ops, the Gradient and `paste_layers`, plus since
/// phase 2a-ii `duplicate_layer` (a second layer, which the one-layer rule
/// forbids: the Move tool's ⌥ copy and a transform copy reach it) and the
/// Magic Wand. Phase 2b added `transform_selected` (Edit ▸ Free Transform
/// and Transform) and `color_assign`, `color_convert` and `profile` (Assign
/// and Convert to Profile), and took out `liquify`, which now names
/// phase 3. The controller's ruling added `rotate_canvas` (Image Rotation ▸
/// Arbitrary, the one leaf it serves) and kept `filter` (Blur ▸ Lens Blur
/// is not built for a Bitmap document; the host names that gap). Phase 3
/// adds Select ▸ Color Range (`color_range`), Similar and Grow (`similar`)
/// and Select and Mask (`refine_edge`), every Image ▸ Adjustments row
/// (`hdr_toning`), Edit ▸ Transform (`puppet_selected`, `puppet_warp`,
/// `transform_mask`), Paste (`paste`), and the layer style's and Smart
/// Objects' rows (`global_light`, `stack_mode`). They refuse for good.
const DISABLED_OPS: &[&str] = &[
    "add_layer",
    "adjustment",
    "adjustment_layer",
    "apply_mask",
    "artwork",
    "color_assign",
    "color_convert",
    "color_range",
    "content_aware_fill",
    "duplicate_layer",
    "duplicate_to_new_document",
    "filter",
    "global_light",
    "group_layers",
    "hdr_toning",
    "layer_blend",
    "layer_blend_if",
    "layer_clipped",
    "layer_style",
    "layer_via",
    "magic_wand",
    "mask_properties",
    "merge_down",
    "merge_group",
    "merge_layers",
    "merge_visible",
    "paste",
    "perspective_selected",
    "profile",
    "puppet_selected",
    "puppet_warp",
    "rasterize",
    "refine_edge",
    "remove_mask",
    "rotate_canvas",
    "set_mask",
    "similar",
    "stack_mode",
    "stamp_selected",
    "stamp_visible",
    "transform_mask",
    "transform_selected",
    "ungroup_layer",
];

/// The mode's refusal sentence (module doc), in three tiers, none naming a
/// phase: what is not available in Bitmap mode ([`DISABLED_OPS`]),
/// what needs a second layer ([`ONE_LAYER`]), and everything else, which
/// Doctored offers on Grayscale and colour documents only.
pub(crate) fn refused_sentence(operation: &str) -> String {
    if DISABLED_OPS.contains(&operation) {
        return format!("{operation} is not available in Bitmap mode.");
    }
    if ONE_LAYER.contains(&operation) {
        return format!("{operation} needs more than one layer, and a Bitmap document holds one.");
    }
    format!(
        "{operation} is unavailable in a Bitmap document: Doctored offers it on Grayscale and \
         colour documents only; convert to Grayscale first."
    )
}

/// The gate `ffi_precision_edit::request` and `ffi_retouch` ask on a Bitmap
/// document, before the Gray gate: a request that is not JSON is refused as
/// Indexed's gate refuses it; a `selection` of kind `magic_wand`,
/// `color_range`, `similar` or `refine_edge` refuses for good by its kind,
/// and any other selection (its kind absent or unknown included) routes
/// [`BitmapRoute::Shared`]; so does [`SHARED`]; [`ADMITTED`] routes
/// [`BitmapRoute::Edit`] (`bitmap_edit` runs it), a `blend_mode` outside
/// [`BLEND_MODES`] refusing by [`BLEND_REFUSAL`]; every other request
/// refuses by [`refused_sentence`] of its `op`.
pub(crate) fn admit_edit(text: &str) -> Result<BitmapRoute, String> {
    let value: Value =
        serde_json::from_str(text).map_err(|e| format!("invalid high-depth edit request: {e}"))?;
    let op = value["op"].as_str().unwrap_or("edit");
    if op == "selection" {
        return match value["operation"]["kind"].as_str() {
            Some(kind @ ("magic_wand" | "color_range" | "similar" | "refine_edge")) => {
                Err(refused_sentence(kind))
            }
            _ => Ok(BitmapRoute::Shared),
        };
    }
    if SHARED.contains(&op) {
        return Ok(BitmapRoute::Shared);
    }
    if !ADMITTED.contains(&op) {
        return Err(refused_sentence(op));
    }
    if let "paint_blend" | "bucket_fill" | "stroke_layer_edge" | "gradient" | "retouch_clone" = op {
        let mode = &value["blend_mode"];
        if !mode.is_null()
            && !mode
                .as_i64()
                .is_some_and(|code| BLEND_MODES.contains(&code))
        {
            return Err(BLEND_REFUSAL.into());
        }
    }
    Ok(BitmapRoute::Edit)
}

/// The selected edit's gate (`ffi_selected_adjustment::request`) on a
/// Bitmap document, before the Gray gate: Liquify passes (`bitmap_tools`
/// runs it on the Gray storage, then thresholds it); the adjustments, Auto,
/// Equalize and Levels by channel refuse as `adjustment`, every filter as
/// `filter`, both for good. Any other document passes.
pub(crate) fn admit_selected(doc: &PrecisionDocument, edit: &SelectedEdit) -> Result<(), String> {
    if !doc.is_bitmap() || edit.op == "liquify" {
        return Ok(());
    }
    let adjustment = matches!(
        edit.op.as_str(),
        "adjustment" | "levels_channels" | "equalize"
    ) || edit.is_auto();
    Err(refused_sentence(if adjustment {
        "adjustment"
    } else {
        "filter"
    }))
}

/// One render sample: white or black, opaque.
fn sample(white: bool) -> [u16; 4] {
    if white {
        [65535; 4]
    } else {
        [0, 0, 0, 65535]
    }
}

/// The Bitmap mode's policy (module doc).
#[derive(Clone)]
pub struct BitmapPolicy {
    packed: Arc<Vec<u8>>,
    width: u32,
    height: u32,
}

impl BitmapPolicy {
    /// A policy of a `width × height` packed plane: `stride(width) · height`
    /// bytes, every pad bit 0.
    pub(crate) fn new(packed: Vec<u8>, width: u32, height: u32) -> Result<Self, String> {
        let line = stride(width);
        if packed.len() as u64 != line as u64 * u64::from(height) {
            return Err("the Bitmap plane does not match the canvas".into());
        }
        let used = width as usize % 8;
        if used != 0 && line > 0 {
            let pad = 0xFFu8 >> used;
            if packed
                .chunks_exact(line)
                .any(|row| row[line - 1] & pad != 0)
            {
                return Err("the Bitmap plane sets a bit past the canvas".into());
            }
        }
        Ok(Self {
            packed: Arc::new(packed),
            width,
            height,
        })
    }

    pub(crate) fn packed(&self) -> &[u8] {
        &self.packed
    }

    pub(crate) fn dimensions(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    /// Packed row `y`.
    fn packed_row(&self, y: u32) -> &[u8] {
        let line = stride(self.width);
        &self.packed[y as usize * line..(y as usize + 1) * line]
    }

    /// The "Bitmap" layer's pixels in `profile` (module doc).
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
                let packed = self.packed_row(y);
                for (x, pixel) in row.iter_mut().enumerate() {
                    *pixel = sample(is_white(packed, x));
                }
                job.every(y as usize * width)
            },
            job,
        )?;
        PrecisionImage::from_rgba16_tiles_unseeded(plane, profile.clone())
    }

    /// Whether `image` is exactly this policy's render (the native check,
    /// `bitmap_native`).
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
            let packed = self.packed_row(y);
            let mut x = 0usize;
            for (_, run) in image.row_segments(y, 0, self.width) {
                let PrecisionRun::Rgba16(run) = run else {
                    return Ok(false);
                };
                for pixel in run {
                    if *pixel != sample(is_white(packed, x)) {
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
        inventory.object(&*self.packed, "bitmap_plane", false);
        inventory.vec(&self.packed, "bitmap_plane", false);
    }
}

impl PrecisionDocument {
    /// True for a Bitmap document (which is also `is_gray()`, module doc).
    pub fn is_bitmap(&self) -> bool {
        self.bitmap.is_some()
    }

    /// The Bitmap policy; `None` for every other document.
    pub(crate) fn bitmap_policy(&self) -> Option<&Arc<BitmapPolicy>> {
        self.bitmap.as_ref()
    }

    /// This document as a Bitmap document under `policy`. Refuses anything
    /// but an 8-bit Gray document (u16 storage), an alpha channel, and
    /// anything but one canvas-sized raster at the origin of the policy's
    /// dimensions. Guides, samplers, paths, metadata and resolution carry
    /// as they are.
    pub(crate) fn with_bitmap(mut self, policy: Arc<BitmapPolicy>) -> Result<Self, String> {
        let gray8 = self
            .gray_policy()
            .is_some_and(|p| p.depth() == crate::gray_document::GrayDepth::Eight);
        if !gray8 || self.is_float() || self.is_cmyk() || self.is_lab() || self.is_indexed() {
            return Err("a Bitmap document is made from an 8-bit Grayscale document".into());
        }
        if !self.channels().is_empty() {
            return Err("a Bitmap document holds no alpha channel".into());
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
            return Err("a Bitmap document holds one canvas-sized layer at the origin".into());
        }
        self.bitmap = Some(policy);
        Ok(self)
    }

    /// This document with no Bitmap policy: the 8-bit Gray document its
    /// storage is, every sample shared.
    pub(crate) fn without_bitmap(&self) -> Self {
        let mut out = self.clone();
        out.bitmap = None;
        out
    }
}
