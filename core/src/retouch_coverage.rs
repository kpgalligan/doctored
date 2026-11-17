//! A typed retouch's coverage as a canvas-placed plane (13B-9c): the u16
//! samples of one canvas rect, every canvas pixel outside the rect at 0.
//!
//! A Clone, Heal, Spot Heal or Content-Aware Fill request may name its
//! `coverage_rect` (`precision_retouch::RetouchEdit`); its stroke and its
//! selection then arrive as that rect's samples, and the shared prelude's
//! selection domain is built over the stroke's bbox only. Without the key
//! the rect is the canvas, every earlier request's meaning. Every reader
//! indexes a plane through canvas coordinates ([`CoveragePlane::at`],
//! [`CoveragePlane::get`]) or crops it to a frame ([`CoveragePlane::crop`]),
//! so a plane over a rect answers what the canvas plane holding the same
//! samples (0 elsewhere) answers, to the sample.

use crate::job_control::JobContext;
use crate::precision_image::reserved_vec;
use crate::retouch_transform::CanvasRect;

/// The selection domain over `bbox` (the stroke's bbox), row-major: the
/// stroke where `selection` is above 0, else 0. The retouch's working
/// coverage under a selection, polled once per [`JobContext::every`] index.
pub(crate) fn selection_domain(
    stroke: CoveragePlane<'_>,
    selection: CoveragePlane<'_>,
    (x, y, w, h): Rect,
    job: JobContext<'_>,
) -> Result<Vec<u16>, String> {
    let mut domain = reserved_vec(w as usize * h as usize)?;
    for ly in 0..h {
        let cy = i64::from(y) + i64::from(ly);
        for lx in 0..w {
            job.every(domain.len())?;
            let cx = i64::from(x) + i64::from(lx);
            domain.push(if selection.get(cx, cy) == 0 {
                0
            } else {
                stroke.get(cx, cy)
            });
        }
    }
    Ok(domain)
}

/// A canvas rect `(x, y, w, h)`.
pub(crate) type Rect = (u32, u32, u32, u32);

/// Coverage samples row-major over `rect`, a rect of a `canvas`-sized
/// canvas (module doc).
#[derive(Clone, Copy)]
pub(crate) struct CoveragePlane<'a> {
    samples: &'a [u16],
    rect: Rect,
    canvas: (u32, u32),
}

impl<'a> CoveragePlane<'a> {
    /// `samples` over `rect`; `Err` unless `rect` lies inside `canvas` and
    /// holds exactly `samples.len()` pixels.
    pub(crate) fn new(samples: &'a [u16], rect: Rect, canvas: (u32, u32)) -> Result<Self, String> {
        let (x, y, w, h) = rect;
        if u64::from(x) + u64::from(w) > u64::from(canvas.0)
            || u64::from(y) + u64::from(h) > u64::from(canvas.1)
            || samples.len() as u64 != u64::from(w) * u64::from(h)
        {
            return Err("retouch coverage extent mismatch".into());
        }
        Ok(Self {
            samples,
            rect,
            canvas,
        })
    }

    pub(crate) fn rect(&self) -> Rect {
        self.rect
    }

    /// The rect as a canvas frame, exclusive ends.
    pub(crate) fn frame(&self) -> CanvasRect {
        let (x, y, w, h) = self.rect;
        let (x, y) = (i64::from(x), i64::from(y));
        (x, y, x + i64::from(w), y + i64::from(h))
    }

    pub(crate) fn samples(&self) -> &'a [u16] {
        self.samples
    }

    /// The sample at canvas index `canvas_index` (row-major over the
    /// canvas); 0 outside the rect.
    pub(crate) fn at(&self, canvas_index: usize) -> u16 {
        let width = self.canvas.0 as usize;
        if width == 0 {
            return 0;
        }
        self.get((canvas_index % width) as i64, (canvas_index / width) as i64)
    }

    /// The sample at canvas `(x, y)`; 0 outside the rect.
    pub(crate) fn get(&self, x: i64, y: i64) -> u16 {
        let (rx, ry, rw, rh) = self.rect;
        let (lx, ly) = (x - i64::from(rx), y - i64::from(ry));
        if lx < 0 || ly < 0 || lx >= i64::from(rw) || ly >= i64::from(rh) {
            return 0;
        }
        self.samples[ly as usize * rw as usize + lx as usize]
    }

    /// The canvas bbox of the samples above 0: `layer_rect::coverage_rect16`'s
    /// row scan over the rect's own plane, moved by the rect's origin, so on
    /// a canvas-sized plane it is that function's answer. `None` when no
    /// sample is above 0.
    pub(crate) fn bbox(&self) -> Option<Rect> {
        let (rx, ry, rw, rh) = self.rect;
        let (x, y, w, h) =
            crate::layer_rect::coverage_rect16(self.samples, (rw, rh), (rw, rh), (0, 0))?;
        Some((x + rx, y + ry, w, h))
    }

    /// The samples over canvas `frame` (exclusive ends), row-major over it,
    /// 0 where the frame leaves the rect. An empty frame answers no samples.
    pub(crate) fn crop(&self, frame: CanvasRect) -> Result<Vec<u16>, String> {
        let (x0, y0, x1, y1) = frame;
        let (Ok(fw), Ok(fh)) = (usize::try_from(x1 - x0), usize::try_from(y1 - y0)) else {
            return Ok(Vec::new());
        };
        let count = fw
            .checked_mul(fh)
            .ok_or("retouch coverage frame is too large")?;
        let mut out = reserved_vec(count)?;
        out.resize(count, 0);
        let (rx0, ry0, rx1, ry1) = self.frame();
        let (ix0, ix1) = (x0.max(rx0), x1.min(rx1));
        if ix0 >= ix1 {
            return Ok(out);
        }
        let (run, stride) = ((ix1 - ix0) as usize, self.rect.2 as usize);
        for y in y0.max(ry0)..y1.min(ry1) {
            let from = (y - ry0) as usize * stride + (ix0 - rx0) as usize;
            let to = (y - y0) as usize * fw + (ix0 - x0) as usize;
            out[to..to + run].copy_from_slice(&self.samples[from..from + run]);
        }
        Ok(out)
    }
}
