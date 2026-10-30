//! GUIDES and the RULER ORIGIN at high depth (Phase 12E-1c) — the typed twin
//! of `doc_guide`'s `impl DrDocument` block, and nothing more.
//!
//! **Every rule is `doc_guide`'s**, and this module deliberately states none
//! of its own: the coordinate convention (a grid-line position in the
//! continuous canvas space, both edges legal), the model invariant (sorted by
//! `(orientation, position)`, no duplicate after four-decimal quantization,
//! every position inside `[0, extent]`), the four-decimal quantization, the
//! `MAX_GUIDES` cap, the DROP-versus-CLAMP split of the canvas policies and
//! the refusal each mutator answers all live there. The five mutators below
//! are wrappers over the five list-level free functions `doc_guide` exposes
//! for exactly this reason, so a typed document and a byte document cannot
//! decide what a guide means two different ways.
//!
//! What is genuinely typed is [`PrecisionDocument::geometry_chrome`]: the
//! byte depth builds its moved guides inside each geometry op's own struct
//! literal (`doc.rs`'s `geometry`, `crop`, `canvas_resize` and `resize`),
//! while the typed depth has ONE canvas-geometry entry point
//! (`precision_geometry::geometry_unlocked`) taking a `GeometryEdit`, so the
//! per-op choice is made once here and `precision_geometry` only folds the
//! answer into its result.
//!
//! **The typed `padded_*` caller.** `doc_guide::padded_guides` /
//! `padded_origin` are Canvas Size's helpers, and since Phase 12F-1 the
//! `CanvasSize` arm of [`PrecisionDocument::geometry_chrome`] is their typed
//! caller: `RasterDocument.canvasResized` routes to `GeometryEdit::CanvasSize`
//! at depth and `canvas_size` is admitted at 16 bits and at float.
//!
//! **Why the ids are not persisted here either.** `Guide::id` is a
//! session-scoped host handle at both depths (`doc_guide`'s own note), so the
//! typed `LGDE` table (`precision_guide_native`) stores orientation and
//! position only and the reader mints, exactly as `drdc_read` does.
//!
//! **Every rule is `doc_guide.rs`'s**, and `precision_guide` states none of its own: the five byte
//! mutator BODIES were hoisted out of `impl DrDocument` into list-level free functions (`added`,
//! `moved`, `removed`, `cleared`, `origin_set`) and both depths are one-line wrappers, so the
//! refusals, the four-decimal quantization, the duplicate test, `MAX_GUIDES` and the `[0, extent]`
//! range are each written once; `guide_tests.rs` is the byte regression. The ONE genuinely typed
//! piece is `geometry_chrome`: the byte depth decides per op inside four struct literals
//! (`doc.rs`'s `geometry`/`crop`/`canvas_resize`/`resize`) while the typed depth has one entry
//! point taking a `GeometryEdit`, so Resize → `resized_*` (`fx = new_w / width`), Crop →
//! `cropped_*`, the five turns/flips → `geometry_*` with `lw = lh = 0`, and a per-layer
//! Affine/Perspective → unchanged; the moved chrome counts toward `precision_geometry`'s `changed`,
//! so a crop that shifted only a guide is still an edit, and `Guide` derives `PartialEq` (id
//! included) for exactly that test. `padded_guides`/`padded_origin`'s typed caller is the
//! `CanvasSize` arm added in 12F-1 (`precision_geometry_canvas.rs`), so both helpers are now
//! reached at either depth and no rule is written twice. The chrome also crosses
//! `precision_conversion` (both directions), `float_to_u16` and the three profile-edit sites.

use crate::doc_guide::{self, Guide, GuideOrientation};
use crate::precision_document::PrecisionDocument;
use crate::precision_geometry::GeometryEdit;

impl PrecisionDocument {
    /// The document's guides, in the model's sorted order.
    pub fn guides(&self) -> &[Guide] {
        &self.guides
    }

    /// The canvas coordinate of the ruler's zero point, `(0, 0)` by default.
    pub fn ruler_origin(&self) -> (f64, f64) {
        self.ruler_origin
    }

    /// Adds a guide (`doc_guide::added`, whose doc comment owns the four
    /// refusals this `None` carries).
    pub fn add_guide(&self, orientation: GuideOrientation, position: f64) -> Option<Self> {
        let guides =
            doc_guide::added(&self.guides, orientation, position, self.width, self.height)?;
        Some(Self {
            guides,
            ..self.clone()
        })
    }

    /// Moves guide `i` along its own axis (`doc_guide::moved`).
    pub fn move_guide(&self, i: usize, position: f64) -> Option<Self> {
        let guides = doc_guide::moved(&self.guides, i, position, self.width, self.height)?;
        Some(Self {
            guides,
            ..self.clone()
        })
    }

    /// Drops guide `i` (`doc_guide::removed`).
    pub fn remove_guide(&self, i: usize) -> Option<Self> {
        let guides = doc_guide::removed(&self.guides, i)?;
        Some(Self {
            guides,
            ..self.clone()
        })
    }

    /// Drops every guide (`doc_guide::cleared`); `None` on an empty list.
    pub fn clear_guides(&self) -> Option<Self> {
        let guides = doc_guide::cleared(&self.guides)?;
        Some(Self {
            guides,
            ..self.clone()
        })
    }

    /// Moves the ruler zero point (`doc_guide::origin_set`).
    pub fn set_ruler_origin(&self, x: f64, y: f64) -> Option<Self> {
        let ruler_origin = doc_guide::origin_set(self.ruler_origin, x, y, self.width, self.height)?;
        Some(Self {
            ruler_origin,
            ..self.clone()
        })
    }

    /// The chrome after `edit`, as the byte depth would have moved it.
    ///
    /// `Resize` is `resized_*` with `fx = new_w / self.width` and `fy`
    /// likewise (`doc.rs`'s `resize`); `Crop` — and `Straighten`, whose byte
    /// twin ends in that very `crop` and whose rotation moves no guide,
    /// exactly as the byte chain's `straighten_layers`/`transform_channels`
    /// pair leaves the chrome alone — is `cropped_*`; `CanvasSize` is
    /// `padded_*` by the origin (`doc.rs`'s `canvas_resize`); the five
    /// whole-document turns and flips are `geometry_*` with `doc::Geometry`
    /// and `lw = lh = 0` (the whole reason those maps are free functions);
    /// and a per-layer `Affine` or `Perspective` leaves both alone, because
    /// moving ONE layer moves nothing about the canvas the guides cross (the
    /// byte precedent is `guide_tests.rs`'s transform case).
    pub(crate) fn geometry_chrome(
        &self,
        edit: &GeometryEdit,
    ) -> Result<(Vec<Guide>, (f64, f64)), String> {
        let (w, h) = (self.width, self.height);
        let geometry = match edit {
            GeometryEdit::Rotate90 => Some(crate::doc::Geometry::Rotate90),
            GeometryEdit::Rotate180 => Some(crate::doc::Geometry::Rotate180),
            GeometryEdit::Rotate270 => Some(crate::doc::Geometry::Rotate270),
            GeometryEdit::FlipHorizontal => Some(crate::doc::Geometry::FlipH),
            GeometryEdit::FlipVertical => Some(crate::doc::Geometry::FlipV),
            _ => None,
        };
        if let Some(geometry) = geometry {
            return Ok((
                doc_guide::geometry_guides(&self.guides, geometry, w, h),
                doc_guide::geometry_origin(self.ruler_origin, geometry, w, h),
            ));
        }
        let chrome = match edit {
            GeometryEdit::Resize { width, height, .. } => {
                let fx = f64::from(*width) / f64::from(w);
                let fy = f64::from(*height) / f64::from(h);
                (
                    doc_guide::resized_guides(&self.guides, fx, fy, *width, *height),
                    doc_guide::resized_origin(self.ruler_origin, fx, fy, *width, *height),
                )
            }
            GeometryEdit::Crop {
                x,
                y,
                width,
                height,
            } => (
                doc_guide::cropped_guides(&self.guides, *x, *y, *width, *height),
                doc_guide::cropped_origin(self.ruler_origin, *x, *y, *width, *height),
            ),
            // A signed window, so `padded_*` by `(−x, −y)` — equal to
            // `cropped_*` for a window inside the canvas.
            GeometryEdit::Straighten {
                x,
                y,
                width,
                height,
                ..
            } => {
                let origin = (x.saturating_neg(), y.saturating_neg());
                (
                    doc_guide::padded_guides(&self.guides, *width, *height, origin),
                    doc_guide::padded_origin(self.ruler_origin, *width, *height, origin),
                )
            }
            GeometryEdit::CanvasSize {
                width,
                height,
                x,
                y,
            } => (
                doc_guide::padded_guides(&self.guides, *width, *height, (*x, *y)),
                doc_guide::padded_origin(self.ruler_origin, *width, *height, (*x, *y)),
            ),
            GeometryEdit::RotateCanvas { degrees, .. } => {
                use crate::doc_rotate::{rotated_canvas, rotation_kind, RotationKind};
                let (kind, normalized) =
                    rotation_kind(*degrees).ok_or("rotation degrees must be finite")?;
                let exact = match kind {
                    RotationKind::Rotate90 => Some(crate::doc::Geometry::Rotate90),
                    RotationKind::Rotate180 => Some(crate::doc::Geometry::Rotate180),
                    RotationKind::Rotate270 => Some(crate::doc::Geometry::Rotate270),
                    RotationKind::Identity => None,
                    RotationKind::Filtered => {
                        let plan = rotated_canvas(w, h, normalized)
                            .ok_or("the rotated canvas would exceed the supported canvas size")?;
                        return Ok((
                            doc_guide::padded_guides(
                                &self.guides,
                                plan.width,
                                plan.height,
                                plan.origin(),
                            ),
                            doc_guide::padded_origin(
                                self.ruler_origin,
                                plan.width,
                                plan.height,
                                plan.origin(),
                            ),
                        ));
                    }
                };
                if let Some(geometry) = exact {
                    (
                        doc_guide::geometry_guides(&self.guides, geometry, w, h),
                        doc_guide::geometry_origin(self.ruler_origin, geometry, w, h),
                    )
                } else {
                    (self.guides.clone(), self.ruler_origin)
                }
            }
            _ => (self.guides.clone(), self.ruler_origin),
        };
        Ok(chrome)
    }
}
