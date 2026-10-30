//! Captured typed retouch plates. The cut gates CONTRIBUTION only, through the
//! ordinary typed compositor; isolation is the document's own (a typed cut is
//! a `style_composite::Cut` too). Raw current-layer reads ignore gates.
use crate::job_control::JobContext;
use crate::precision_document::{Content, PrecisionDocument, PrecisionDocumentLayer};
use crate::precision_image::{
    admit, pixel_count, reserved_vec, PrecisionImage, PrecisionRun, PrecisionStorage,
    BOOKKEEPING_BYTES,
};
use crate::retouch_sampling::RetouchSampling;
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
    pub(crate) fn retouch_source_with_context(
        &self,
        index: usize,
        mode: RetouchSampling,
        ignore: bool,
        limit: u64,
        other: u64,
        job: JobContext<'_>,
    ) -> Result<PrecisionImage, String> {
        let scratch = self.retouch_source_plan(index, mode)?;
        admit(self.retained_bytes(), scratch, other, limit)?;
        job.reserve(scratch)?;
        if mode == RetouchSampling::Current {
            let layer = self
                .layer(index)
                .ok_or("retouch source layer is out of range")?;
            let image = layer
                .source()
                .ok_or("retouch requires a raster source layer")?;
            let (canvas, offset) = (self.dimensions(), layer.offset());
            let profile = self.profile.clone();
            return match image.storage() {
                PrecisionStorage::Rgba16Unorm => {
                    let pixels = canvas_plate(canvas, image, offset, [0u16; 4], runs16, job)?;
                    PrecisionImage::from_rgba16_unseeded(canvas.0, canvas.1, pixels, profile, None)
                }
                PrecisionStorage::Rgba32Float => {
                    let pixels = canvas_plate(canvas, image, offset, [0f32; 4], runs32, job)?;
                    PrecisionImage::from_rgba32_extended(canvas.0, canvas.1, pixels, profile, None)
                }
            };
        }
        let excluded = retouch_cut16(&self.layers, index, mode, ignore, self.records);
        self.render_cut_with_context(Cut::new(&excluded), limit, other, job)
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

/// The Current-mode plate: `image` (standing at `offset`) copied row by row
/// onto a `canvas`-sized plane of `blank`. One copy for both depths; `runs`
/// picks the storage's own run type (`runs16` / `runs32`).
fn canvas_plate<P: Copy>(
    canvas: (u32, u32),
    image: &PrecisionImage,
    (ox, oy): (i32, i32),
    blank: P,
    runs: for<'a> fn(PrecisionRun<'a>) -> Option<&'a [P]>,
    job: JobContext<'_>,
) -> Result<Vec<P>, String> {
    let (w, h) = image.dimensions();
    let mut pixels = reserved_vec(pixel_count(canvas.0, canvas.1)?)?;
    pixels.resize(canvas.0 as usize * canvas.1 as usize, blank);
    // The layer's columns under the canvas (`Window::u16`'s column
    // rule with the window the whole canvas): canvas x = lx - a.
    let a = -i64::from(ox);
    let lx0 = a.clamp(0, i64::from(w));
    let lx1 = (a + i64::from(canvas.0)).clamp(0, i64::from(w));
    for y in 0..canvas.1 {
        job.check()?;
        let row = y as usize * canvas.0 as usize;
        for x in 0..canvas.0 {
            job.every(row + x as usize)?;
        }
        let ly = i64::from(y) - i64::from(oy);
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
