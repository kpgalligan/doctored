//! Captured typed retouch plates. The cut gates CONTRIBUTION only, through the
//! ordinary typed compositor; isolation is the document's own (a typed cut is
//! a `style_composite::Cut` too). Raw current-layer reads ignore gates.
//!
//! A Clone or Heal reads only its TAPS (13B-9a): `retouch_taps16` /
//! `retouch_taps32` answer one canvas rect of the plate. `Current` reads that
//! rect straight from the source layer's rows (no canvas plate is built);
//! `all` and `current_and_below` still render the whole plate and keep its
//! rect. Both are admitted and reserved as the whole plate is
//! (`retouch_source_plan`), so the request's admission does not change.
use crate::job_control::JobContext;
use crate::precision_document::{Content, PrecisionDocument, PrecisionDocumentLayer};
use crate::precision_image::{
    admit, pixel_count, reserved_vec, PrecisionImage, PrecisionRun, PrecisionStorage,
    BOOKKEEPING_BYTES,
};
use crate::retouch_sampling::RetouchSampling;
use crate::retouch_transform::CanvasRect;
use crate::style_composite::Cut;

impl PrecisionDocument {
    pub fn retouch_source_plan(&self, index: usize, mode: RetouchSampling) -> Result<u64, String> {
        self.layer_source(index)
            .ok_or("retouch requires a raster source layer")?;
        let n = pixel_count(self.width, self.height)? as u64;
        // The canvas plate in the document's storage: 8 bytes a pixel at 16
        // bits, 16 at 32-bit float.
        Ok(n * self.render_storage().bytes_per_pixel()
            + 4 * BOOKKEEPING_BYTES
            + if mode == RetouchSampling::Current {
                0
            } else {
                self.render_plan()?.scratch_bytes
            })
    }
    /// The plate's admission and reservation (`retouch_source_plan`): the
    /// whole plate's own, read by the plate and by the rect taps alike.
    fn admit_retouch_source(
        &self,
        index: usize,
        mode: RetouchSampling,
        limit: u64,
        other: u64,
        job: JobContext<'_>,
    ) -> Result<(), String> {
        let scratch = self.retouch_source_plan(index, mode)?;
        admit(self.retained_bytes(), scratch, other, limit)?;
        job.reserve(scratch)
    }

    /// The `Current` source layer and its image.
    fn current_source(&self, index: usize) -> Result<(&PrecisionImage, (i32, i32)), String> {
        let layer = self
            .layer(index)
            .ok_or("retouch source layer is out of range")?;
        let image = layer
            .source()
            .ok_or("retouch requires a raster source layer")?;
        Ok((image, layer.offset()))
    }

    pub(crate) fn retouch_source_with_context(
        &self,
        index: usize,
        mode: RetouchSampling,
        ignore: bool,
        limit: u64,
        other: u64,
        job: JobContext<'_>,
    ) -> Result<PrecisionImage, String> {
        self.admit_retouch_source(index, mode, limit, other, job)?;
        if mode == RetouchSampling::Current {
            let (image, offset) = self.current_source(index)?;
            let canvas = self.dimensions();
            let whole = (0, 0, i64::from(canvas.0), i64::from(canvas.1));
            let profile = self.profile.clone();
            return match image.storage() {
                PrecisionStorage::Rgba16Unorm => {
                    let pixels = canvas_plate(whole, image, offset, [0u16; 4], runs16, job)?;
                    PrecisionImage::from_rgba16_unseeded(canvas.0, canvas.1, pixels, profile, None)
                }
                PrecisionStorage::Rgba32Float => {
                    let pixels = canvas_plate(whole, image, offset, [0f32; 4], runs32, job)?;
                    PrecisionImage::from_rgba32_extended(canvas.0, canvas.1, pixels, profile, None)
                }
            };
        }
        let excluded = retouch_cut16(&self.layers, index, mode, ignore, self.records);
        self.render_cut_with_context(Cut::new(&excluded), limit, other, job)
    }

    /// The u16 plate's samples over the canvas `rect` only (row-major over
    /// it; module doc): a Clone or Heal's taps. "missing native retouch
    /// source" when the source is not u16.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn retouch_taps16(
        &self,
        index: usize,
        mode: RetouchSampling,
        ignore: bool,
        rect: CanvasRect,
        limit: u64,
        other: u64,
        job: JobContext<'_>,
    ) -> Result<Vec<[u16; 4]>, String> {
        let source = (index, mode, ignore);
        let storage = PrecisionStorage::Rgba16Unorm;
        self.retouch_taps(
            source,
            rect,
            (limit, other),
            storage,
            [0u16; 4],
            runs16,
            job,
        )
    }

    /// [`Self::retouch_taps16`] over a 32-bit float document's f32 plate.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn retouch_taps32(
        &self,
        index: usize,
        mode: RetouchSampling,
        ignore: bool,
        rect: CanvasRect,
        limit: u64,
        other: u64,
        job: JobContext<'_>,
    ) -> Result<Vec<[f32; 4]>, String> {
        let source = (index, mode, ignore);
        let storage = PrecisionStorage::Rgba32Float;
        self.retouch_taps(
            source,
            rect,
            (limit, other),
            storage,
            [0f32; 4],
            runs32,
            job,
        )
    }

    /// The one body of [`Self::retouch_taps16`] / [`Self::retouch_taps32`]:
    /// `Current` admits and reserves the plate's plan and copies `rect` from
    /// the layer's rows; another mode renders the plate as
    /// `retouch_source_with_context` does and copies its `rect`, the plate
    /// dropped on return.
    #[allow(clippy::too_many_arguments)]
    fn retouch_taps<P: Copy>(
        &self,
        (index, mode, ignore): (usize, RetouchSampling, bool),
        rect: CanvasRect,
        (limit, other): (u64, u64),
        storage: PrecisionStorage,
        blank: P,
        runs: for<'a> fn(PrecisionRun<'a>) -> Option<&'a [P]>,
        job: JobContext<'_>,
    ) -> Result<Vec<P>, String> {
        const MISSING: &str = "missing native retouch source";
        if mode == RetouchSampling::Current {
            self.admit_retouch_source(index, mode, limit, other, job)?;
            let (image, offset) = self.current_source(index)?;
            if image.storage() != storage {
                return Err(MISSING.into());
            }
            return canvas_plate(rect, image, offset, blank, runs, job);
        }
        let plate = self.retouch_source_with_context(index, mode, ignore, limit, other, job)?;
        if plate.storage() != storage {
            return Err(MISSING.into());
        }
        canvas_plate(rect, &plate, (0, 0), blank, runs, job)
    }
}

fn runs16(run: PrecisionRun<'_>) -> Option<&[[u16; 4]]> {
    match run {
        PrecisionRun::Rgba16(run) => Some(run),
        PrecisionRun::Rgba32(_) => None,
    }
}

fn runs32(run: PrecisionRun<'_>) -> Option<&[[f32; 4]]> {
    match run {
        PrecisionRun::Rgba32(run) => Some(run),
        PrecisionRun::Rgba16(_) => None,
    }
}

/// The Current-mode plate over the canvas `rect` (exclusive ends): `image`
/// (standing at `offset`) copied row by row onto a rect-sized plane of
/// `blank`, row-major over the rect. One copy for both depths and every
/// rect (the whole canvas for the plate); `runs` picks the storage's own run
/// type (`runs16` / `runs32`). `job` is checked once per row and polled once
/// per pixel at the pixel's index in the rect, so the whole-canvas plate
/// keeps its checkpoints. An empty rect answers an empty plane.
fn canvas_plate<P: Copy>(
    (x0, y0, x1, y1): CanvasRect,
    image: &PrecisionImage,
    (ox, oy): (i32, i32),
    blank: P,
    runs: for<'a> fn(PrecisionRun<'a>) -> Option<&'a [P]>,
    job: JobContext<'_>,
) -> Result<Vec<P>, String> {
    let (Ok(rw), Ok(rh)) = (u32::try_from(x1 - x0), u32::try_from(y1 - y0)) else {
        return Ok(Vec::new());
    };
    if rw == 0 || rh == 0 {
        return Ok(Vec::new());
    }
    let (w, h) = image.dimensions();
    let mut pixels = reserved_vec(pixel_count(rw, rh)?)?;
    pixels.resize(rw as usize * rh as usize, blank);
    // The layer's columns under the rect (`Window::u16`'s column rule with
    // the window the rect): rect column = lx - a.
    let a = x0 - i64::from(ox);
    let lx0 = a.clamp(0, i64::from(w));
    let lx1 = (a + i64::from(rw)).clamp(0, i64::from(w));
    for y in 0..rh {
        job.check()?;
        let row = y as usize * rw as usize;
        for x in 0..rw {
            job.every(row + x as usize)?;
        }
        let ly = y0 + i64::from(y) - i64::from(oy);
        if ly < 0 || ly >= i64::from(h) || lx0 >= lx1 {
            continue;
        }
        for (lx, run) in image.row_segments(ly as u32, lx0 as u32, lx1 as u32) {
            let Some(run) = runs(run) else {
                continue;
            };
            let at = row + (i64::from(lx) - a) as usize;
            pixels[at..at + run.len()].copy_from_slice(run);
        }
    }
    Ok(pixels)
}

/// The typed retouch cut: which records contribute nothing to `mode`'s plate
/// for the record at postorder index `target`. A group record follows its
/// children. The cut marks a postorder exclusion and gates contribution only;
/// it never touches `visible`, so a clip base's isolation stays the
/// document's own decision. `out` is `records` long, postorder — the
/// document's own record count — so a [`Cut`] built from it addresses every
/// record. The PSD export's adjustment stamp (`psd_export_bake`) takes the
/// same cut, which is why this is not nested in its first caller.
pub(crate) fn retouch_cut16(
    nodes: &[PrecisionDocumentLayer],
    target: usize,
    mode: RetouchSampling,
    ignore: bool,
    records: usize,
) -> Vec<bool> {
    fn walk(
        nodes: &[PrecisionDocumentLayer],
        cursor: &mut usize,
        target: usize,
        mode: RetouchSampling,
        ignore: bool,
        out: &mut [bool],
    ) {
        for node in nodes {
            let start = *cursor;
            if let Some(children) = node.children() {
                walk(children, cursor, target, mode, ignore, out);
            }
            let index = *cursor;
            *cursor += 1;
            let above = index > target && start > target;
            if above
                && (mode == RetouchSampling::CurrentAndBelow
                    || (ignore
                        && matches!(
                            node.content,
                            Content::Adjustment(_) | Content::CmykAdjustment(_)
                        )))
            {
                out[index] = true;
            }
        }
    }
    let mut excluded = vec![false; records];
    walk(nodes, &mut 0, target, mode, ignore, &mut excluded);
    excluded
}
