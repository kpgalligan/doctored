//! MOVE SELECTED PIXELS: the one request both depths share, plus the
//! byte kernel. The extent rule, the per-pixel body and the walk live in
//! `selected_pixels_walk`, re-exported here.
//!
//! The contract (measured against Photoshop 26.10, `Trnf` on a synthetic
//! 48×16 image), restricted to an integer translation:
//!
//! * the FLOAT is the layer's pixels × the canvas selection coverage `c`;
//! * the HOLE it leaves is alpha `a·(1−c)` with the RGB kept
//!   (`blend::erase_alpha`, the eraser's own rule);
//! * with `duplicate` (⌥-drag, ⌥+arrow, ⌥⌘T) there is no hole;
//! * the float merges back with Normal SOURCE-OVER at the offset, its
//!   effective alpha `a·c` (`blend::composite_source_into`);
//! * floated pixels that would land off the canvas are DISCARDED: the
//!   destination extent is `layer ∪ ((float + offset) ∩ canvas)`
//!   (`selected_pixels_walk::move_extent`), so a layer that already extends
//!   past the canvas keeps its off-canvas pixels bit for bit;
//! * the layer mask, its properties, meta, style and warp stay put;
//! * Lock Transparency and Lock Position are ALLOWED, Lock Pixels and Lock All
//!   refuse. The gate is `lock_block(idx, Pixels)` asked directly, never
//!   `under_locks` — `freezing_alpha` would restore the alpha the hole just
//!   cut (turning the move into a stamp) and refuse a grown extent outright
//!   (`doc_lock`'s module doc records the exception).
//!
//! Every value is read from the ORIGINAL layer, weighted in f32 and quantized
//! ONCE, so an integer move of a hard selection is byte-exact at 8 bits and
//! code-exact at 16. [`SelectedPixelsEdit`] is the sibling of
//! `precision_geometry::GeometryEdit`; the `TransformSelected` arm resamples
//! the float through `selected_pixels_transform` and lands it through the same
//! walk, `move_extent` and `moved_pixel`.
//!
//! The transform extends to a targeted LAYER MASK or ALPHA CHANNEL:
//! `"target":"mask"` / `"target":{"channel":N}` with an optional
//! `"background"` (u16 coverage gray). The plane's selected coverage floats,
//! resamples and lands through THIS walk; the hole is the background colour
//! (`Hole::Fill`). The decisions and the write-back live in `selected_plane`.
//! The MOVE takes the same `target`/`background` fields: the Move tool
//! and `move_selected_pixels` on a targeted mask or channel run the same plane
//! walk with `Landing::Offset`, so a plane move equals the transform's
//! integer translation bit for bit. The colour planes are targets too
//! (`"target":"red"|"green"|"blue"|"alpha"`, with `"swatch"` for R/G/B): one
//! component of the layer through the same plane walk
//! (`selected_color_plane`). #1 adds a CMYK layer's ink plates
//! (`"target":"cyan"|"magenta"|"yellow"|"black"`, with `"swatch"` too, its
//! gray read as ink): `cmyk_selected_plate`.
//!
//! one strict `SelectedPixelsEdit` (`{"op":"move_selected",layer,dx,dy}`, a sibling of
//! `GeometryEdit`, with a `TransformSelected` arm), one extent rule (`move_extent`: layer
//! ∪ ((F + d) ∩ canvas)) and ONE per-pixel body (`moved_pixel`: hole `blend::erase_alpha(a, c)`
//! with RGB kept, landing `blend::composite_source_into` Normal at `sa = a·c > 0`, quantized once)
//! walked by the depth-generic `move_samples` — all three live in `selected_pixels_walk.rs` and
//! `selected_pixels` re-exports what its consumers use. The mask, meta, style and warp
//! stay put; an unchanged extent writes back through the rect writers, a grown one builds a new
//! plane. The byte plan/edit exports are the host's only seam at 8 bits; the typed request rides
//! `dr_precision_doc_edit` (family `selected_pixels_move`; its float row admits the planes
//! and the layer, f32 through the same walk with no quantize).
//! `"duplicate":true` (a move, or a transform for ⌥⌘T; `hole()`) passes c = 0 for the
//! hole, so the original stays and the copy lands through the same `moved_pixel`. Tests:
//! `tests/move_selected_pixels.rs`, `tests/selected_pixels_float.rs`.

use serde::Deserialize;
use serde_json::json;

use crate::doc::{DrDocument, LayerKind};
use crate::doc_lock::EditKind;
use crate::job_control::{DrJobControl, JobContext};
use crate::layer_raster::LayerRaster;
use crate::precision_image::admit;
use crate::precision_sampling::GeometryFilter;
use crate::puppet_arap::PuppetSpec;
use crate::selected_pixels_float::bounded_scratch;
use crate::selected_pixels_puppet::PuppetLanding;
use crate::selected_pixels_transform::SelectedTransform;
pub(crate) use crate::selected_pixels_walk::{
    area, intersect, move_samples, plan_scratch, union, CanvasRect, Hole, Landing, MovedSamples,
};

/// The request bound, the same 64 KiB every strict geometry request uses.
const REQUEST_LIMIT: usize = 64 * 1024;
/// Fixed bookkeeping on top of the per-pixel scratch.
pub(crate) const PLAN_BOOKKEEPING: u64 = 4 * 1024 * 1024;

/// The selected-pixels request, identical at both depths:
/// `{"op":"move_selected","layer":N,"dx":i32,"dy":i32[,"duplicate":bool]}`, or
/// `{"op":"transform_selected","layer":N,"matrix":[a,b,c,d,tx,ty],"filter":f}` /
/// `{"op":"transform_selected","layer":N,"source_rect":[x,y,w,h],"quad":[x0,y0,…,x3,y3],"filter":f}`.
/// Both take an optional `"duplicate":bool` and `"bounds":[x,y,w,h]`: a canvas rect the caller
/// promises holds every non-zero coverage sample over the layer, which lets
/// the plan charge the selection instead of the layer ([`OUTSIDE_BOUNDS`]
/// when the promise is broken). Both also take `"target"` ("layer", "mask"
/// or `{"channel":N}`; "red", "green", "blue" or "alpha", one colour
/// component of the layer) and, for a mask or channel target only,
/// `"background":u16`; for a red, green or blue target only,
/// `"swatch":[r,g,b]` (the background swatch's 8-bit codes in the document
/// profile's encoding, black when absent). The other two ops are
/// `{"op":"puppet_selected","layer":N,"mode","density","expansion","pins"}`
/// (`doc_puppet`'s fields, the layer target only, an optional `bounds`), and
/// `{"op":"perspective_selected","layer":N,"quads","straighten"}`
/// (`doc_perspective_warp`'s fields, likewise).
#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum SelectedPixelsEdit {
    /// Float the layer's selected pixels (or a targeted mask's or channel's
    /// selected coverage) and move them by `(dx, dy)`. With
    /// `duplicate` (the ⌥-drag) the float leaves no hole: the
    /// original stays and the copy lands source-over.
    MoveSelected {
        layer: usize,
        dx: i32,
        dy: i32,
        #[serde(default)]
        duplicate: bool,
        #[serde(default)]
        bounds: Option<[i64; 4]>,
        #[serde(default)]
        target: SelectedTarget,
        #[serde(default)]
        background: Option<u16>,
        #[serde(default)]
        swatch: Option<[u8; 3]>,
    },
    /// Float the layer's selected pixels and resample them through a canvas
    /// affine `matrix`, or through the map taking `source_rect` onto `quad`
    /// (TL, TR, BR, BL). Exactly one of the two forms.
    TransformSelected {
        layer: usize,
        matrix: Option<[f64; 6]>,
        source_rect: Option<[f64; 4]>,
        quad: Option<[f64; 8]>,
        filter: GeometryFilter,
        #[serde(default)]
        duplicate: bool,
        #[serde(default)]
        bounds: Option<[i64; 4]>,
        #[serde(default)]
        target: SelectedTarget,
        #[serde(default)]
        background: Option<u16>,
        #[serde(default)]
        swatch: Option<[u8; 3]>,
    },
    /// Edit ▸ Puppet Warp with a selection active: float the layer's
    /// selected pixels and warp them through the pins' as-rigid-as-possible
    /// pose (`selected_pixels_puppet`). The layer only, no duplicate; the
    /// mode, density, expansion and pins are `doc_puppet`'s request fields,
    /// validated by [`Self::parse`] with the same sentences.
    PuppetSelected {
        layer: usize,
        #[serde(default)]
        mode: Option<serde_json::Value>,
        #[serde(default)]
        density: Option<serde_json::Value>,
        #[serde(default)]
        expansion: Option<serde_json::Value>,
        #[serde(default)]
        pins: Option<serde_json::Value>,
        #[serde(default)]
        bounds: Option<[i64; 4]>,
    },
    /// Edit ▸ Perspective Warp with a selection active: float the
    /// layer's selected pixels and warp them through the planes' map
    /// (`selected_pixels_perspective`). The layer only, no duplicate; the
    /// quads and straighten entries are `doc_perspective_warp`'s request
    /// fields, validated by [`Self::parse`] with the same sentences.
    PerspectiveSelected {
        layer: usize,
        #[serde(default)]
        quads: Option<serde_json::Value>,
        #[serde(default)]
        straighten: Option<serde_json::Value>,
        #[serde(default)]
        bounds: Option<[i64; 4]>,
    },
}

/// Which plane a transform or a move floats: the layer's
/// pixels, its mask, an alpha channel (whose index replaces the layer), one
/// colour component of the layer (Red, Green, Blue, or its Alpha), or
/// one ink plate of a CMYK layer (#1: Cyan, Magenta, Yellow, Black,
/// `cmyk_selected_plate`).
#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SelectedTarget {
    #[default]
    Layer,
    Mask,
    Channel(usize),
    Red,
    Green,
    Blue,
    Alpha,
    Cyan,
    Magenta,
    Yellow,
    Black,
}

impl SelectedTarget {
    /// The colour component a colour-plane target names (R 0, G 1, B 2,
    /// A 3); `None` for the layer, a mask, a channel or an ink plate.
    pub(crate) fn component(self) -> Option<usize> {
        match self {
            Self::Red => Some(0),
            Self::Green => Some(1),
            Self::Blue => Some(2),
            Self::Alpha => Some(3),
            Self::Layer
            | Self::Mask
            | Self::Channel(_)
            | Self::Cyan
            | Self::Magenta
            | Self::Yellow
            | Self::Black => None,
        }
    }

    /// The ink plate an ink target names (C 0, M 1, Y 2, K 3); `None` for
    /// every other target.
    pub(crate) fn ink_plate(self) -> Option<usize> {
        match self {
            Self::Cyan => Some(0),
            Self::Magenta => Some(1),
            Self::Yellow => Some(2),
            Self::Black => Some(3),
            Self::Layer
            | Self::Mask
            | Self::Channel(_)
            | Self::Red
            | Self::Green
            | Self::Blue
            | Self::Alpha => None,
        }
    }

    /// The wire name of a colour or ink target, for refusals.
    pub(crate) fn plane_name(self) -> &'static str {
        match self {
            Self::Red => "red",
            Self::Green => "green",
            Self::Blue => "blue",
            Self::Alpha => "alpha",
            Self::Cyan => "cyan",
            Self::Magenta => "magenta",
            Self::Yellow => "yellow",
            Self::Black => "black",
            Self::Layer => "layer",
            Self::Mask => "mask",
            Self::Channel(_) => "channel",
        }
    }
}

/// An ink-plate target's refusal on an 8-bit document, which is never CMYK
/// (#1: the typed twin is `precision_selected_pixels`').
fn ink_plate_refusal(edit: &SelectedPixelsEdit) -> Option<String> {
    edit.target().ink_plate().map(|_| {
        format!(
            "The {} channel requires a CMYK document",
            edit.target().plane_name()
        )
    })
}

/// A request whose selection coverage reaches past its declared `bounds`.
pub(crate) const OUTSIDE_BOUNDS: &str =
    "selected pixels coverage lies outside the request's bounds";
const BAD_BOUNDS: &str = "selected pixels bounds must be [x, y, w, h] with w and h at least 1";

impl SelectedPixelsEdit {
    /// Strict parse, bounded before serde sees the text.
    pub fn parse(request: &str) -> Result<Self, String> {
        if request.len() > REQUEST_LIMIT {
            return Err("selected pixels request exceeds 64 KiB".into());
        }
        let edit: Self = serde_json::from_str(request).map_err(|e| {
            // Only a failed parse asks the shape of `bounds`, so a wrong
            // shape refuses by name while serde stays THE parse.
            let value = serde_json::from_str::<serde_json::Value>(request).ok();
            let bounds = value.as_ref().and_then(|v| v.get("bounds"));
            let shaped = |b: &serde_json::Value| {
                b.as_array()
                    .is_some_and(|a| a.len() == 4 && a.iter().all(|v| v.as_i64().is_some()))
            };
            match bounds {
                Some(b) if !b.is_null() && !shaped(b) => BAD_BOUNDS.to_string(),
                _ => format!("invalid selected pixels request: {e}"),
            }
        })?;
        if let Self::TransformSelected {
            matrix,
            source_rect,
            quad,
            ..
        } = &edit
        {
            crate::selected_pixels_transform::check_shape(
                matrix.as_ref(),
                source_rect.as_ref(),
                quad.as_ref(),
            )?;
        }
        edit.puppet_spec()?;
        edit.perspective_spec()?;
        let plane = matches!(
            edit.target(),
            SelectedTarget::Mask | SelectedTarget::Channel(_)
        );
        if !plane && edit.background().is_some() {
            return Err("background applies to a mask or channel target only".into());
        }
        let colour =
            matches!(edit.target().component(), Some(0..=2)) || edit.target().ink_plate().is_some();
        if !colour && edit.swatch().is_some() {
            return Err(
                "swatch applies to a red, green, blue, cyan, magenta, yellow or black target only"
                    .into(),
            );
        }
        if let Some(bounds) = edit.raw_bounds() {
            let fits = bounds.iter().all(|v| i32::try_from(*v).is_ok());
            if !fits || bounds[2] < 1 || bounds[3] < 1 {
                return Err(BAD_BOUNDS.into());
            }
        }
        Ok(edit)
    }

    fn raw_bounds(&self) -> Option<[i64; 4]> {
        match self {
            Self::MoveSelected { bounds, .. }
            | Self::TransformSelected { bounds, .. }
            | Self::PuppetSelected { bounds, .. }
            | Self::PerspectiveSelected { bounds, .. } => *bounds,
        }
    }

    /// A Puppet Warp's validated mode, mesh options and pins; `None` for the
    /// other ops.
    pub(crate) fn puppet_spec(&self) -> Result<Option<PuppetSpec>, String> {
        match self {
            Self::PuppetSelected {
                mode,
                density,
                expansion,
                pins,
                ..
            } => PuppetSpec::parse(
                mode.as_ref(),
                density.as_ref(),
                expansion.as_ref(),
                pins.as_ref(),
            )
            .map(Some),
            Self::MoveSelected { .. }
            | Self::TransformSelected { .. }
            | Self::PerspectiveSelected { .. } => Ok(None),
        }
    }

    /// The request's `bounds` as a canvas rect (checked by [`Self::parse`]).
    pub(crate) fn bounds(&self) -> Option<CanvasRect> {
        self.raw_bounds()
            .map(|[x, y, w, h]| (x, y, w as u32, h as u32))
    }

    /// The plane the request floats.
    pub fn target(&self) -> SelectedTarget {
        match self {
            Self::MoveSelected { target, .. } | Self::TransformSelected { target, .. } => *target,
            Self::PuppetSelected { .. } | Self::PerspectiveSelected { .. } => SelectedTarget::Layer,
        }
    }

    /// A plane target's background coverage gray, as the request names it.
    fn background(&self) -> Option<u16> {
        match self {
            Self::MoveSelected { background, .. } | Self::TransformSelected { background, .. } => {
                *background
            }
            Self::PuppetSelected { .. } | Self::PerspectiveSelected { .. } => None,
        }
    }

    /// A colour target's background swatch, as the request names it.
    pub(crate) fn swatch(&self) -> Option<[u8; 3]> {
        match self {
            Self::MoveSelected { swatch, .. } | Self::TransformSelected { swatch, .. } => *swatch,
            Self::PuppetSelected { .. } | Self::PerspectiveSelected { .. } => None,
        }
    }

    /// Whether the float leaves no hole (⌥-drag, ⌥+arrow, ⌥⌘T).
    pub(crate) fn duplicate(&self) -> bool {
        match self {
            Self::MoveSelected { duplicate, .. } | Self::TransformSelected { duplicate, .. } => {
                *duplicate
            }
            Self::PuppetSelected { .. } | Self::PerspectiveSelected { .. } => false,
        }
    }

    /// The hole the float leaves: none for a `duplicate` (a move or, with
    /// ⌥⌘T, a transform), the layer's `a·(1−c)`, a plane's background
    /// coverage (0 when the request names none), or a colour
    /// plane's 0 — Alpha's rule; `selected_color_plane` builds the Red,
    /// Green and Blue hole from the swatch itself.
    pub(crate) fn hole(&self) -> Hole {
        if self.duplicate() {
            return Hole::Keep;
        }
        match self.target() {
            SelectedTarget::Layer => Hole::Erase,
            SelectedTarget::Mask | SelectedTarget::Channel(_) => {
                Hole::Fill(f32::from(self.background().unwrap_or(0)) / 65535.0)
            }
            SelectedTarget::Red
            | SelectedTarget::Green
            | SelectedTarget::Blue
            | SelectedTarget::Alpha
            | SelectedTarget::Cyan
            | SelectedTarget::Magenta
            | SelectedTarget::Yellow
            | SelectedTarget::Black => Hole::Fill(0.0),
        }
    }

    /// The one layer the request edits.
    pub fn layer(&self) -> usize {
        match self {
            Self::MoveSelected { layer, .. }
            | Self::TransformSelected { layer, .. }
            | Self::PuppetSelected { layer, .. }
            | Self::PerspectiveSelected { layer, .. } => *layer,
        }
    }

    /// The verb's gerund for refusal sentences: "moving" | "transforming" |
    /// "warping".
    pub(crate) fn gerund(&self) -> &'static str {
        match self {
            Self::MoveSelected { .. } => "moving",
            Self::TransformSelected { .. } => "transforming",
            Self::PuppetSelected { .. } | Self::PerspectiveSelected { .. } => "warping",
        }
    }

    /// The empty-float refusal: the move's or the transform's sentence, or
    /// for a colour plane `selected_color_plane::COLOR_EMPTY_AREA`.
    pub(crate) fn empty_area(&self) -> &'static str {
        if self.target().component().is_some() || self.target().ink_plate().is_some() {
            return crate::selected_color_plane::COLOR_EMPTY_AREA;
        }
        match self {
            Self::MoveSelected { .. } => EMPTY_AREA,
            Self::TransformSelected { .. } => crate::selected_pixels_transform::EMPTY_AREA,
            Self::PuppetSelected { .. } => crate::selected_pixels_puppet::EMPTY_AREA,
            Self::PerspectiveSelected { .. } => crate::selected_pixels_perspective::EMPTY_AREA,
        }
    }

    /// "Moving selected pixels needs a pixel layer, and layer N is `what`".
    pub(crate) fn target_refusal(&self, layer: usize, what: &str) -> String {
        let gerund = self.gerund();
        format!(
            "{}{} selected pixels needs a pixel layer, and layer {layer} is {what}",
            gerund[..1].to_uppercase(),
            &gerund[1..]
        )
    }

    pub(crate) fn retained_refusal(&self) -> String {
        format!(
            "Rasterize the retained source before {} its selected pixels",
            self.gerund()
        )
    }

    /// `None` = identity. An integer translation (as a matrix or a
    /// parallelogram quad) IS the move. `resolve` refuses bad geometry (the
    /// companion selection op's own validation included), so the plan calls
    /// this too. Nothing here refuses for work.
    pub(crate) fn landing(&self, canvas: (u32, u32)) -> Result<Option<Landing>, String> {
        match self {
            Self::MoveSelected { dx, dy, .. } => {
                Ok(((*dx, *dy) != (0, 0))
                    .then(|| Landing::Offset((i64::from(*dx), i64::from(*dy)))))
            }
            Self::TransformSelected {
                matrix,
                source_rect,
                quad,
                filter,
                ..
            } => {
                let Some(transform) =
                    SelectedTransform::resolve(*matrix, *source_rect, *quad, *filter, canvas)?
                else {
                    return Ok(None);
                };
                Ok(Some(match transform.translation() {
                    Some((dx, dy)) => Landing::Offset((i64::from(dx), i64::from(dy))),
                    None => Landing::Transform(transform),
                }))
            }
            Self::PuppetSelected { .. } => Ok(self
                .puppet_spec()?
                .filter(|spec| !spec.is_identity())
                .map(|spec| Landing::Puppet(PuppetLanding { spec }))),
            Self::PerspectiveSelected { .. } => {
                Ok(self.perspective_landing()?.map(Landing::Perspective))
            }
        }
    }

    /// Scratch on top of the move's: the resampled landing at 16 bytes per
    /// pixel. Resolves through [`Self::landing`], so bad geometry
    /// refuses as there. 0 for a move, an identity, an
    /// integer translation or a layer that misses the canvas; otherwise the
    /// landing box of the layer's on-canvas rect, and the whole canvas when
    /// that box cannot be formed. It never refuses on its own.
    ///
    /// Why the layer's on-canvas rect bounds the kernel's landing without
    /// reading a sample: the float F is where the layer has alpha AND the
    /// coverage is non-zero, and the coverage is 0 off the canvas, so
    /// F ⊆ layer ∩ canvas. Growing a rect by the reach, projecting its
    /// corners (an affine image, or the projective image of a rect whose four
    /// corners all project, is convex and contains the image of any sub-rect),
    /// `snap_to_integer`, the outward floor/ceil and the canvas clip are each
    /// monotone, so landing(F) ⊆ landing_box(layer ∩ canvas). Every fallback
    /// is the whole canvas, which bounds any landing.
    pub(crate) fn landing_scratch(
        &self,
        canvas: (u32, u32),
        layer: CanvasRect,
    ) -> Result<u64, String> {
        let whole = 16u64.saturating_mul(u64::from(canvas.0) * u64::from(canvas.1));
        let transform = match self.landing(canvas)? {
            Some(Landing::Transform(transform)) => transform,
            // The deformed box is not known before the solve.
            Some(Landing::Puppet(puppet)) => {
                return puppet.scratch(canvas, intersect(layer, (0, 0, canvas.0, canvas.1)))
            }
            Some(Landing::Perspective(perspective)) => return Ok(perspective.scratch(canvas)),
            Some(Landing::Offset(_)) | None => return Ok(0),
        };
        let Some(bound) = intersect(layer, (0, 0, canvas.0, canvas.1)) else {
            return Ok(0);
        };
        Ok(match transform.landing_box(bound, canvas) {
            Ok(Some(dest)) => 16u64.saturating_mul(area(dest)),
            Ok(None) => 0,
            Err(_) => whole,
        })
    }
}

/// The sample-free admission plan both depths answer: a move's plan is
/// delta-independent, a transform's landing term follows its geometry.
pub struct SelectedPixelsPlan {
    pub retained_bytes: u64,
    pub scratch_bytes: u64,
    pub input_samples: u64,
}

impl SelectedPixelsPlan {
    /// Exactly the three keys.
    pub fn json(&self) -> serde_json::Value {
        json!({
            "retained_bytes": self.retained_bytes,
            "scratch_bytes": self.scratch_bytes,
            "input_samples": self.input_samples,
        })
    }
}

pub(crate) const EMPTY_AREA: &str = "The selection holds no pixels to move.";

impl DrDocument {
    /// Index and raster-target validation shared by the plan, the kernel and
    /// the float-bounds query.
    pub(crate) fn selected_pixels_target(
        &self,
        edit: &SelectedPixelsEdit,
    ) -> Result<&crate::doc::Layer, String> {
        let layer = edit.layer();
        let entry = self
            .layers
            .get(layer)
            .ok_or("selected pixels layer index is out of range")?;
        if entry.kind == LayerKind::Group {
            return Err(edit.target_refusal(layer, "a group"));
        }
        if crate::retouch_sampling::is_adjustment(entry) {
            return Err(edit.target_refusal(layer, "an adjustment layer"));
        }
        Ok(entry)
    }

    /// Sample-free: a move's plan is delta-independent, a transform's landing
    /// term follows its geometry. Validates the index, the raster target and
    /// the storage, and a transform's geometry. No lock check.
    pub fn selected_pixels_plan(
        &self,
        edit: &SelectedPixelsEdit,
    ) -> Result<SelectedPixelsPlan, String> {
        if let Some(refusal) = ink_plate_refusal(edit) {
            return Err(refusal);
        }
        if edit.target().component().is_some() {
            return self.selected_color_plane_plan(edit);
        }
        if edit.target() != SelectedTarget::Layer {
            return self.selected_plane_plan(edit);
        }
        let entry = self.selected_pixels_target(edit)?;
        let canvas = (self.width, self.height);
        let (w, h) = entry.pixels.dimensions();
        let rect = (i64::from(entry.offset.0), i64::from(entry.offset.1), w, h);
        let mut inventory = crate::resources::Inventory::default();
        inventory.document(self);
        let scratch_bytes = match edit.bounds() {
            Some(bounds) => bounded_scratch(edit, canvas, rect, bounds, 4)?,
            None => {
                plan_scratch(canvas, rect, 4).saturating_add(edit.landing_scratch(canvas, rect)?)
            }
        };
        Ok(SelectedPixelsPlan {
            retained_bytes: inventory.unique_bytes(),
            scratch_bytes,
            input_samples: u64::from(self.width) * u64::from(self.height),
        })
    }

    /// Float the layer's pixels × `coverage` (canvas-sized u16), move them by
    /// the request's delta (or resample them through its transform) and
    /// composite them back. `Ok(None)` for the identity or when no stored
    /// byte changed. Refusals are `Err` sentences: bad length, index or
    /// target, a retained source, a Pixels lock, bad geometry and an empty
    /// selected area.
    pub fn selected_pixels_edit(
        &self,
        edit: &SelectedPixelsEdit,
        coverage: &[u16],
        limit: u64,
        other_retained: u64,
        control: Option<&DrJobControl>,
    ) -> Result<Option<Self>, String> {
        let _run = control.map(DrJobControl::begin).transpose()?;
        let job = control.map_or(JobContext::NONE, JobContext::new);
        if let Some(refusal) = ink_plate_refusal(edit) {
            return Err(refusal);
        }
        if edit.target().component().is_some() {
            return self.selected_color_plane_edit(edit, coverage, limit, other_retained, job);
        }
        if edit.target() != SelectedTarget::Layer {
            return self.selected_plane_edit(edit, coverage, limit, other_retained, job);
        }
        let canvas = (self.width, self.height);
        if coverage.len() as u64 != u64::from(canvas.0) * u64::from(canvas.1) {
            return Err("selected pixels coverage must be canvas-sized u16".into());
        }
        let index = edit.layer();
        let entry = self.selected_pixels_target(edit)?;
        if self.has_retained_source(index) {
            return Err(edit.retained_refusal());
        }
        let blocking = self.lock_block(index, EditKind::Pixels);
        if blocking != 0 {
            return Err(match self.lock_owner(index, blocking) {
                Some(owner) if owner != index => {
                    format!("layer {index} is inside group {owner}, which is locked (pixels)")
                }
                _ => format!("layer {index} is locked (pixels)"),
            });
        }
        let Some(landing) = edit.landing(canvas)? else {
            return Ok(None);
        };
        let plan = self.selected_pixels_plan(edit)?;
        admit(
            plan.retained_bytes,
            plan.scratch_bytes,
            other_retained,
            limit,
        )?;
        job.reserve(plan.scratch_bytes)?;
        job.check()?;
        let (w, h) = entry.pixels.dimensions();
        let rect = (i64::from(entry.offset.0), i64::from(entry.offset.1), w, h);
        let pixels = &entry.pixels;
        let moved = move_samples::<[u8; 4]>(
            |x, y, w, h| pixels.read_rect(x, y, w, h),
            rect,
            canvas,
            coverage,
            &landing,
            edit.empty_area(),
            edit.hole(),
            edit.bounds(),
            job,
        )?;
        let grown_comps = match &moved {
            MovedSamples::Plane(origin, ..) => self.comps_after_growth(index, *origin),
            MovedSamples::Rect(..) => None,
        };
        let mut next = self.clone();
        if let Some(comps) = grown_comps {
            next.comps = comps;
        }
        let layer = &mut next.layers[index];
        match moved {
            MovedSamples::Rect((x, y, w, h), samples) => {
                let Some(raster) = pixels.with_rect(x, y, w, h, &samples)? else {
                    return Ok(None);
                };
                layer.pixels = raster;
            }
            MovedSamples::Plane(origin, (w, h), samples) => {
                let image = image::RgbaImage::from_raw(w, h, samples.into_flattened())
                    .ok_or("could not build the moved layer")?;
                layer.pixels = LayerRaster::new(image);
                layer.offset = origin;
            }
        }
        job.check()?;
        Ok(Some(next))
    }
}
