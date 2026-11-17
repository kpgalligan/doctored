//! A transparency lock on a CMYK raster: the CMYK arm of
//! `PrecisionDocument::restoring_alpha` (precision_lock.rs), which
//! `freezing_alpha` runs after every `under_locks(…, Pixels, …)` writer whose
//! record is transparency-locked, by its own bit or an enclosing group's.
//!
//! **The rules are RGB's verbatim** (precision_lock.rs's module doc): the
//! same owner (`lock_owner(index, LOCK_TRANSPARENCY)`) and sentence for a
//! moved or resized raster, which `freezing_alpha` answers before this arm
//! runs; `cmyk_composite::frozen_sample` as the per-sample rule
//! (`restoring_alpha`'s, on CMYKA16); and the same latch: restored samples
//! equal to the source with the same mask are `Ok(None)`, never an identical
//! copy.
//!
//! The arm differs only in expression. It READS before it writes: one scan
//! of the bbox of the tiles `dst` does not share with `src`
//! (`CmykImage::unshared_rect`, through both images' `row_segments`,
//! allocating nothing) answers whether the frozen samples are the source's
//! and where they differ from `dst`. Most CMYK writers already freeze inline
//! (`cmyk_paint`, `cmyk_stroke`, `cmyk_warp`, `cmyk_passes::join_sample`), so
//! for them the scan finds nothing to write and `out` goes through untouched.
//! A difference is written onto `dst` one band at a time, each at most
//! `TILE_EDGE` rows and inside one tile row (`CmykImage::with_rect`), so the
//! restore holds the rewritten tiles and one band's samples, never a whole
//! plane copy. Those bytes are in no writer's plan (a re-render's, which
//! `cmyk_artwork`'s Admission section discloses, nor Fade's, the plate
//! writes' or Equalize's, which reach the same write when they restore the
//! ink at α-0 pixels) and were not measured.

use crate::cmyk_composite::frozen_sample;
use crate::job_control::JobContext;
use crate::precision_document::{Content, PrecisionDocument};
use crate::precision_image::reserved_vec;
use crate::precision_lock::same_mask;
use crate::tile_store::TILE_EDGE;

/// A bbox as `(x0, y0, x1, y1)`, the ends exclusive.
type Bounds = (u32, u32, u32, u32);

impl PrecisionDocument {
    /// `out` with CMYK raster `index`'s original alpha restored (module
    /// doc). `Ok(Some(out))` untouched when either record is not a CMYK
    /// raster or no sample needs restoring; `Ok(None)` when the restore
    /// leaves the record as it was; `Err` when the edit resized or moved the
    /// raster.
    pub(crate) fn restoring_cmyk_alpha(
        &self,
        index: usize,
        out: Self,
    ) -> Result<Option<Self>, String> {
        let before = self
            .layer(index)
            .ok_or("high-depth layer index is out of range")?;
        let after = out
            .layer(index)
            .ok_or("high-depth layer index is out of range")?;
        let (Some(src), Some(dst)) = (before.cmyk_source(), after.cmyk_source()) else {
            return Ok(Some(out));
        };
        if src.dimensions() != dst.dimensions() || before.offset() != after.offset() {
            return Err("the alpha restore needs the layer's own buffer, unmoved".into());
        }
        let Some((x, y, w, h)) = src.unshared_rect(dst) else {
            // Every tile shared: `dst` is `src`, sample for sample.
            return Ok((!same_mask(after, before)).then_some(out));
        };
        // The read (module doc). The two images have one size, so one tile
        // grid, and their runs pair one to one.
        let mut same = true;
        let mut changes: Option<Bounds> = None;
        for row in y..y + h {
            for ((at, s), (_, d)) in
                src.row_segments(row, x, x + w)
                    .zip(dst.row_segments(row, x, x + w))
            {
                for (column, (s, d)) in (at..).zip(s.iter().zip(d)) {
                    let frozen = frozen_sample(*s, *d);
                    same &= frozen == *s;
                    if frozen != *d {
                        changes = Some(match changes {
                            None => (column, row, column + 1, row + 1),
                            Some((x0, y0, x1, y1)) => {
                                (x0.min(column), y0, x1.max(column + 1), y1.max(row + 1))
                            }
                        });
                    }
                }
            }
        }
        if same && same_mask(after, before) {
            return Ok(None);
        }
        let Some((x0, y0, x1, y1)) = changes else {
            return Ok(Some(out));
        };
        // The write: one band per tile row the bbox crosses.
        let mut image = dst.clone();
        let mut top = y0;
        while top < y1 {
            let bottom = (top / TILE_EDGE + 1).saturating_mul(TILE_EDGE).min(y1);
            let mut band = reserved_vec((x1 - x0) as usize * (bottom - top) as usize)?;
            for row in top..bottom {
                for ((_, s), (_, d)) in src
                    .row_segments(row, x0, x1)
                    .zip(dst.row_segments(row, x0, x1))
                {
                    band.extend(s.iter().zip(d).map(|(s, d)| frozen_sample(*s, *d)));
                }
            }
            let rect = (x0, top, x1 - x0, bottom - top);
            if let Some(next) = image.with_rect(rect, &band, JobContext::NONE)? {
                image = next;
            }
            top = bottom;
        }
        let mut layer = after.clone();
        layer.content = Content::CmykRaster {
            image,
            offset: after.offset(),
        };
        out.replacing_layer(index, layer)
    }
}
