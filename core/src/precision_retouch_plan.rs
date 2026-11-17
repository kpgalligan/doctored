//! The IN-PLACE terms of `retouch_plan` (13B-9c): a Clone, Heal, Spot Heal
//! or Content-Aware Fill that writes its target layer at its own extent
//! (`output: layer`, no `expand_destination`), outside the full-frame seam.
//! Every other arm (a new layer, a grown extent, `full_frame`, Red Eye) keeps
//! the canvas formula in `precision_retouch`.
//!
//! Each multiplier is the bytes the allocation it names holds at 16 bits;
//! a 32-bit float document doubles them (its samples are f32) and adds the
//! W-code copy of a heal, spot heal or fill's destination rect
//! (`float_retouch`). `c` is the request's coverage area (`coverage_rect`, or
//! the canvas), `n` the canvas:
//!
//! - **2·c with a selection**: the selection domain, built over the stroke's
//!   bbox, which lies inside the coverage area (the raw stroke may reach past
//!   the bounds where the selection is zero, so not inside the bounds).
//! - **Clone / Heal**: the planned frame `g` (the coverage area inside the
//!   request's bounds when it has them, grown by 1 for a Heal, clamped to
//!   the canvas) holds the kernel's bound, so `8g` for the transformed
//!   samples, `8g` for the destination rect read and the 256 × 256 tiles that
//!   rect can rewrite (`with_rect_rgba16`'s new tiles), a Heal's frame-cropped
//!   planes (`2g`, `4g` with a selection) and `precision_heal::plan(g)`; the
//!   taps `retouch_taps_plan` over `retouch_transform::tap_rect(g)`, which
//!   holds the taps of every frame inside `g`.
//! - **Spot Heal / Content-Aware Fill**: the destination rect over
//!   `precision_inpaint::estimate_window_size` and its tiles (every component
//!   window lies inside that window), `precision_inpaint::estimate` with its
//!   hole/walk term over the bounds (the domain it plans), the compiled
//!   `source_mask` (`n`, one byte a canvas pixel) when there is one, and the
//!   plate unless the synthesis reads its source layer in place.
//!
//! The walk's own inventory (the domain, samples, destination, tiles, W copy
//! and `Current` taps) is charged at 5/4 of its bytes, the 1.25 × margin
//! every admitted plan keeps over the footprint it was counted or measured
//! at: these terms are exact counts, so without it a whole-canvas stroke
//! would plan at its counted peak. `precision_heal::plan` and
//! `precision_inpaint::estimate` carry their own measured margins, a
//! rendered plate its own plan.

use crate::doc_inpaint::Caller;
use crate::precision_document::PrecisionDocument;
use crate::precision_retouch::{RetouchEdit, RetouchOp};
use crate::retouch_sampling::RetouchSampling;
use crate::retouch_transform::CanvasRect;
use crate::tile_store::TILE_EDGE;

/// One 256 × 256 tile at 16 bits.
const TILE_BYTES: u64 = TILE_EDGE as u64 * TILE_EDGE as u64 * 8;

/// The pixels of one 256 × 256 tile (a CMYK tile holds 10 bytes a pixel).
pub(crate) const TILE_PIXELS: u64 = TILE_EDGE as u64 * TILE_EDGE as u64;

/// The admission margin over the walk's own inventory (module doc):
/// `⌈5/4 ×⌉`, the 1.25 × a plan keeps over what it was counted to hold.
pub(crate) fn margin(bytes: u64) -> u64 {
    bytes.saturating_mul(5).div_ceil(4)
}

/// The most tiles a `w × h` rect can touch at any position: a span of `v ≥ 1`
/// pixels crosses at most `(v + 254) / 256 + 1` tile columns.
pub(crate) fn rect_tiles(w: u32, h: u32) -> u64 {
    if w == 0 || h == 0 {
        return 0;
    }
    let edge = u64::from(TILE_EDGE);
    let span = |v: u32| (u64::from(v) + edge - 2) / edge + 1;
    span(w) * span(h)
}

/// The synthesis caller of a Spot Heal or Content-Aware Fill request.
pub(crate) fn synthesis_caller(edit: &RetouchEdit) -> Caller {
    if edit.op == RetouchOp::ContentAwareFill {
        Caller::Fill
    } else {
        Caller::SpotHeal
    }
}

/// The 2·c selection domain term (module doc); 0 without a selection.
pub(crate) fn domain_bytes(doc: &PrecisionDocument, edit: &RetouchEdit) -> Result<u64, String> {
    let (_, _, w, h) = edit.coverage_area(doc)?;
    Ok(if edit.selection {
        2 * u64::from(w) * u64::from(h)
    } else {
        0
    })
}

impl PrecisionDocument {
    /// A Clone or Heal's planned frame `g` (module doc), a canvas rect with
    /// exclusive ends: every bound its walk can take lies inside it. The
    /// empty rect when the coverage area misses the bounds.
    pub(crate) fn retouch_planned_frame(&self, edit: &RetouchEdit) -> Result<CanvasRect, String> {
        let (cw, ch) = self.dimensions();
        let heal = edit.op == RetouchOp::RetouchHeal;
        let (ax, ay, aw, ah) = edit.coverage_area(self)?;
        let (mut x0, mut y0) = (i64::from(ax), i64::from(ay));
        let (mut x1, mut y1) = (x0 + i64::from(aw), y0 + i64::from(ah));
        // A Heal's bounds are validated here as before; a Clone's only
        // narrow the frame when they are valid (its walk refuses them).
        let bounds = if heal && edit.width > 0 && edit.height > 0 {
            Some(edit.bounds(self)?)
        } else {
            (edit.width > 0 && edit.height > 0)
                .then(|| edit.bounds(self).ok())
                .flatten()
        };
        if let Some((bx, by, bw, bh)) = bounds {
            x0 = x0.max(i64::from(bx));
            y0 = y0.max(i64::from(by));
            x1 = x1.min(i64::from(bx) + i64::from(bw));
            y1 = y1.min(i64::from(by) + i64::from(bh));
        }
        let grow = i64::from(heal);
        Ok(if x0 < x1 && y0 < y1 {
            (
                (x0 - grow).max(0),
                (y0 - grow).max(0),
                (x1 + grow).min(i64::from(cw)),
                (y1 + grow).min(i64::from(ch)),
            )
        } else {
            (0, 0, 0, 0)
        })
    }

    /// A Spot Heal or Content-Aware Fill's bounds and the size of the
    /// window every component window lies inside (module doc).
    #[allow(clippy::type_complexity)]
    pub(crate) fn synthesis_window(
        &self,
        edit: &RetouchEdit,
    ) -> Result<((u32, u32, u32, u32), (u32, u32)), String> {
        let bounds = edit.bounds(self)?;
        let area = u64::from(bounds.2) * u64::from(bounds.3);
        let size = crate::precision_inpaint::estimate_window_size(
            self.dimensions(),
            bounds,
            area,
            edit.ring,
            synthesis_caller(edit),
            edit.spot_type,
        )?;
        Ok((bounds, size))
    }

    /// `retouch_plan`'s in-place terms (module doc), beside its fixed
    /// bookkeeping: `plate` is the source plan `retouch_plan` validated,
    /// charged here where the walk renders it.
    pub(crate) fn in_place_retouch_scratch(
        &self,
        edit: &RetouchEdit,
        mode: RetouchSampling,
        plate: u64,
        float: bool,
    ) -> Result<u64, String> {
        let (cw, ch) = self.dimensions();
        let n = u64::from(cw) * u64::from(ch);
        let domain = domain_bytes(self, edit)?;
        if matches!(edit.op, RetouchOp::RetouchClone | RetouchOp::RetouchHeal) {
            let heal = edit.op == RetouchOp::RetouchHeal;
            let frame = self.retouch_planned_frame(edit)?;
            let (gw, gh) = ((frame.2 - frame.0) as u32, (frame.3 - frame.1) as u32);
            let g = u64::from(gw) * u64::from(gh);
            let mut buffers = domain + g * 8 + g * 8 + rect_tiles(gw, gh) * TILE_BYTES;
            if heal {
                buffers += g * if edit.selection { 4 } else { 2 };
            }
            let mut scratch = buffers;
            if float {
                scratch += buffers + if heal { g * 16 } else { 0 };
            }
            let taps = crate::retouch_transform::tap_rect((cw, ch), &edit.matrix, frame)?;
            let source = edit.source_layer.unwrap_or(edit.layer);
            scratch = margin(scratch + self.retouch_taps_plan(source, mode, taps)?);
            if heal {
                scratch += crate::precision_heal::plan(g)?.0;
            }
            return Ok(scratch);
        }
        let (bounds, (ww, wh)) = self.synthesis_window(edit)?;
        let area = u64::from(bounds.2) * u64::from(bounds.3);
        let window = u64::from(ww) * u64::from(wh);
        let buffers = domain + window * 8 + rect_tiles(ww, wh) * TILE_BYTES;
        let mut scratch = buffers;
        if float {
            scratch += buffers + window * 16;
        }
        scratch = margin(scratch);
        if !edit.reads_plate_in_place() {
            scratch += plate;
        }
        scratch += crate::precision_inpaint::estimate(
            (cw, ch),
            bounds,
            area,
            edit.ring,
            synthesis_caller(edit),
            edit.spot_type,
            3,
            area,
        )?
        .0;
        if edit.source_mask.is_some() {
            scratch += n;
        }
        Ok(scratch)
    }
}
