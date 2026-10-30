//! The pixel half of the CMYK document's merges (#1 phase 4a and the
//! styles batch): Merge Layers, Merge Down, Merge Visible and Stamp Visible
//! (`precision_merge`, `precision_stamp_flatten`, whose structural rules are
//! the RGB ones verbatim) take their pixels from the ONE row compositor
//! (`cmyk_stack`) over a FRAME — the members' union extent for Merge Layers
//! and Merge Down (`precision_merge_extent`), the canvas for Merge Visible
//! and Stamp Visible — written as STRAIGHT ink and alpha, so a merged leaf
//! composites as its members did (groups, clip units, blend modes, Fill
//! Opacity, masks, adjustment records and layer styles baked exactly as they
//! project) and pixels a member holds outside the canvas survive.
//!
//! # Knockout in a merge (D7)
//!
//! Merge Visible and Stamp Visible render the document's own projection
//! ([`PrecisionDocument::cmyk_projection_image`]), D5's Background included,
//! so a knockout bakes exactly. Merge Layers and Merge Down render a MEMBERS
//! document, which is never the projection: its root is HANDED its backdrop
//! ([`PrecisionDocument::cmyk_frame_image`], `CmykStack::row_seeded`), never
//! D5's. When a visible direct member punches (`precision_merge`'s
//! `merge_wants_backdrop16`) that backdrop is the SOURCE document's own,
//! captured row by row at the lowest member (`CmykStack::capture_at`) and
//! transparent outside the canvas — `precision_style_knockout::
//! render_projection` and `precision_merge_extent::windowed_seed` (the RGB
//! masters) on the row; otherwise it is transparent (D4's private
//! accumulator).
//!
//! Every merge's plan is [`PrecisionDocument::cmyk_merge_scratch_bytes`]:
//! `cmyk_convert::FLATTEN_BYTES_PER_PIXEL` over the larger of the frame and
//! the canvas (the new plane and the result's display preparation, as the
//! CMYK Flatten Image measures them), one frame-wide ink row, the members
//! stack's scratch and bookkeeping; a seeded merge adds the source stack's
//! scratch and its two backdrop row copies and source row
//! ([`PrecisionDocument::cmyk_seed_scratch_bytes`]); a miss in
//! `scripts/cmyk-admission` raises that per-pixel term.

use crate::cmyk_composite::{codes, InkPixel};
use crate::cmyk_image::CmykImage;
use crate::cmyk_stack::{CmykStack, KnockoutSeed};
use crate::job_control::JobContext;
use crate::precision_document::PrecisionDocument;
use crate::precision_image::BOOKKEEPING_BYTES;
use crate::precision_merge_extent::Frame;

/// `frame`'s canvas rect `(x0, y0, x1, y1)`.
fn bounds_of(frame: Frame) -> (i64, i64, i64, i64) {
    let ((fx, fy), (w, h)) = frame;
    (
        i64::from(fx),
        i64::from(fy),
        i64::from(fx) + i64::from(w),
        i64::from(fy) + i64::from(h),
    )
}

/// `rows(y, acc)` fills each frame row; the image is its straight codes.
fn frame_rows(
    frame: Frame,
    job: JobContext<'_>,
    mut rows: impl FnMut(i64, &mut [InkPixel]),
) -> Result<CmykImage, String> {
    let ((_, fy), (w, h)) = frame;
    let mut acc: Vec<InkPixel> = vec![[0f32; 5]; w as usize];
    CmykImage::from_rows_with_job(
        w,
        h,
        |y, row| {
            rows(i64::from(fy) + i64::from(y), &mut acc);
            for (dst, ink) in row.iter_mut().zip(acc.iter()) {
                *dst = codes(*ink);
            }
            job.every(y as usize * w as usize)
        },
        job,
    )
}

/// `seed`'s row windowed onto the canvas (module doc): transparent outside
/// columns `0..canvas_w` and on a row outside `0..canvas_h`.
fn windowed(
    plane: Option<&[InkPixel]>,
    into: &mut Vec<InkPixel>,
    y: i64,
    x0: i64,
    canvas: (u32, u32),
) -> bool {
    let Some(plane) = plane else {
        return false;
    };
    into.clear();
    into.extend_from_slice(plane);
    let inside_row = y >= 0 && y < i64::from(canvas.1);
    for (k, pixel) in into.iter_mut().enumerate() {
        let x = x0 + k as i64;
        if !inside_row || x < 0 || x >= i64::from(canvas.0) {
            *pixel = [0.0; 5];
        }
    }
    true
}

impl PrecisionDocument {
    /// This (members) document's composite over `frame` (canvas origin,
    /// size) as one straight CMYKA16 image, its root HANDED the backdrop of
    /// `seed_from`'s record `(document, record id)` row by row, or a
    /// transparent one without (module doc, D7).
    pub(crate) fn cmyk_frame_image(
        &self,
        frame: Frame,
        seed_from: Option<(&PrecisionDocument, u64)>,
        job: JobContext<'_>,
    ) -> Result<CmykImage, String> {
        self.require_cmyk_policy("a CMYK merge")?;
        let ((fx, _), (w, _)) = frame;
        let x0 = i64::from(fx);
        let mut stack = self.cmyk_stack_over(bounds_of(frame), job)?;
        let mut source: Option<(CmykStack<'_>, (u32, u32))> = match seed_from {
            Some((document, record)) => {
                let mut stack = document.cmyk_stack_over(bounds_of(frame), job)?;
                stack.capture_at(record);
                Some((stack, document.dimensions()))
            }
            None => None,
        };
        let mut source_row: Vec<InkPixel> = vec![[0f32; 5]; w as usize];
        let (mut shallow, mut deep) = (Vec::new(), Vec::new());
        frame_rows(frame, job, |y, acc| match source.as_mut() {
            Some((source, canvas)) => {
                source.row(y, x0, &mut source_row);
                let captured = source.captured();
                let has_shallow = windowed(captured.shallow, &mut shallow, y, x0, *canvas);
                let has_deep = windowed(captured.deep, &mut deep, y, x0, *canvas);
                let seed = KnockoutSeed {
                    shallow: has_shallow.then_some(&shallow[..]),
                    deep: has_deep.then_some(&deep[..]),
                };
                stack.row_seeded(y, x0, acc, seed);
            }
            None => stack.row_seeded(y, x0, acc, KnockoutSeed::NONE),
        })
    }

    /// This document's own PROJECTION over `frame` (D5's Background
    /// included): Merge Visible's, Stamp Visible's and Trim's composite.
    pub(crate) fn cmyk_projection_image(
        &self,
        frame: Frame,
        job: JobContext<'_>,
    ) -> Result<CmykImage, String> {
        self.require_cmyk_policy("a CMYK merge")?;
        let x0 = i64::from(frame.0 .0);
        let mut stack = self.cmyk_stack_over(bounds_of(frame), job)?;
        frame_rows(frame, job, |y, acc| stack.row(y, x0, acc))
    }

    /// The working memory of rendering this (members) document over
    /// `frame` into a merged leaf (module doc).
    pub(crate) fn cmyk_merge_scratch_bytes(&self, frame: Frame) -> Result<u64, String> {
        let overflow = || "CMYK merge byte calculation overflow".to_string();
        let (w, h) = self.dimensions();
        let ((_, _), (fw, fh)) = frame;
        let pixels = (u64::from(fw) * u64::from(fh)).max(u64::from(w) * u64::from(h));
        let stack = self.cmyk_stack_scratch_bytes_over(fw.max(w))?;
        pixels
            .checked_mul(crate::cmyk_convert::FLATTEN_BYTES_PER_PIXEL)
            .and_then(|v| v.checked_add(u64::from(fw) * 20))
            .and_then(|v| v.checked_add(stack))
            .and_then(|v| v.checked_add(BOOKKEEPING_BYTES))
            .ok_or_else(overflow)
    }

    /// What a SEEDED merge adds (module doc): this (source) document's stack
    /// over `frame`'s width, its row, and the two windowed backdrop rows.
    pub(crate) fn cmyk_seed_scratch_bytes(&self, frame: Frame) -> Result<u64, String> {
        let ((_, _), (fw, _)) = frame;
        let width = fw.max(self.dimensions().0);
        self.cmyk_stack_scratch_bytes_over(width)?
            .checked_add(u64::from(width) * 20 * 3)
            .ok_or_else(|| "CMYK merge byte calculation overflow".to_string())
    }
}
