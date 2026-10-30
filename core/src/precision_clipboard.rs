//! Phase 12G-1: the TYPED clipboard interchange — a Doctored→Doctored copy
//! at 16 bits or 32-bit float that round-trips samples, alpha, profile,
//! offset, selection coverage and PPI bit for bit.
//!
//! # The payload
//!
//! [`ClipboardPayload`] is one window of raw samples in the SOURCE
//! document's storage and profile, the canvas position the window came from,
//! the document's resolution and, when a selection was in force, the
//! window's u16 coverage. The public TIFF/PNG a host writes beside it is a
//! RENDERING; this is the master.
//!
//! # The codec
//!
//! The payload and its RZCB stream live in `precision_clipboard_codec`
//! (RZCB v3, whose module doc is the format), and the convert arm in
//! `precision_clipboard_convert`; both are re-exported here. A payload may
//! carry the host's editable section (v3): Copy, Cut and `from_rgba8` make
//! none, and `converted` and `trim` (`precision_clipboard_trim`) carry it.
//! The core never reads the section: New from Clipboard builds its pixels
//! and the HOST restores the record into the new document, having trimmed
//! the payload to its content box first.
//!
//! # The three operations
//!
//! Copy reads a raster record's RAW samples (latent RGB retained, opacity,
//! blend mode and mask not applied — the byte Copy's rule) or, for a group,
//! the group's RAW projection (its children composited without the group's
//! own opacity, blend, mask, style or Blend If — `precision_clipboard_group`),
//! Copy Merged reads the projection, and every read multiplies alpha
//! by the selection's coverage. Cut refuses a group before any read, and is
//! otherwise Copy followed by exactly Clear's erase (`PaintEdit::Erase` at
//! opacity 1; at float, `blend::erase_alpha` on the f32 alpha with RGB
//! retained), so a transparency lock makes its document half a no-op rather
//! than a refusal. Float reads stamp the document's tone map on the payload.
//! Paste inserts the payload as one raster record. A payload of another
//! storage or profile is never refused for it: it is brought into
//! the document's depth and profile first through
//! `ClipboardPayload::converted_with` under the request's [`PasteProfile`]
//! — Convert (the default) keeps the pixels' appearance, Preserve
//! ("Don't Convert") keeps their numbers and relabels them — and a 16-bit
//! payload is PROMOTED into a 32-bit float document
//! (`precision_clipboard_promote`). New from Clipboard (`from_clipboard`)
//! builds a one-record document at the payload's own depth and profile.
//!
//! # Admission
//!
//! Every operation charges its bookkeeping first, and every read charges the
//! payload AND its encoding (`2 · payload_bytes`, because the FFI handle
//! holds both) before it allocates either. Copy Merged and a group's Copy
//! additionally charge the canvas-sized render before calling the renderer,
//! whose own charge then runs with the payload counted as other retained
//! bytes. Every read also admits the host's public-rendering stage
//! (`precision_clipboard_host`). A paste admits bookkeeping, then the
//! conversion's own charge (`ClipboardPayload::conversion_bytes`), then the
//! insertion with the converted payload counted as other retained bytes and,
//! when a conversion into float ran, [`PASTE_PROMOTE_MARGIN_PER_PIXEL`] ×
//! canvas; [`PrecisionDocument::clipboard_paste_plan`] is the largest of
//! those (the exact threshold) and
//! [`PrecisionDocument::clipboard_paste_allocation`] the new memory.
//!
//! The OPS module: Copy reads a raster record's raw samples through its offset (latent RGB kept;
//! zeros where it does not reach) or, for a group (the GROUP module), the group's RAW
//! projection — `raw_group_document` (the typed twin of the byte `group_projection`: the group as
//! the only root at opacity 1, visible, Normal, no mask/warp/style/Blend If/clip, Pass Through
//! re-expressed as isolated) rendered and windowed by Copy Merged's own `read_rendered` — Copy
//! Merged the projection, all with alpha × coverage (u16 rounded integer product, f32 `a·c/65535`),
//! float reads stamping the document's tone map. Every read admits bookkeeping, then
//! `2·payload_bytes` (payload + the encoding the `DrClipboardPayload` handle holds), Copy Merged
//! and a group's Copy also the render, before allocating. Cut = a group refusal first ("Cut
//! requires a raster layer; layer N is a group"), then lock refusal, Copy, then exactly Clear's
//! `PaintEdit::Erase` at u16 or, at float, `blend::erase_alpha(a, c/65535)` in f32 with RGB
//! retained under `under_locks` (a transparency lock answers `(payload, None)`). Paste
//! never refuses another storage or profile: it runs
//! `ClipboardPayload::converted_with_context` under the request's `PasteProfile` (`profile`:
//! `convert` default / `preserve`, Don't Convert; `convert` is gone from `PasteRequest`) whenever
//! storage or profile bytes differ (the CONVERT module): same storage with Preserve or the same
//! profile is `Ok(None)` (the insert relabels through `with_profile`); the insertion adds
//! `PASTE_PROMOTE_MARGIN_PER_PIXEL` (29, fitted) × canvas when a conversion into float ran
//! (re-exported from `precision_clipboard`; the PROMOTE module's doc is its measurement record).
//! `clipboard_paste_plan` is the exact threshold (the max of the paste's admits with
//! `retained_bytes()`) and `clipboard_paste_allocation` the new memory (the plan without it), over
//! one private `clipboard_paste_terms` (the `group_copy_terms` pattern). It inserts one raster
//! record at `insert_slot` through `inserting_record`, sharing the payload's pixels; placement
//! `into` (Paste Into / Paste Outside) requires a positive-size `frame` `[x, y, w, h]`
//! and centres on it with Centered's floor rule, and a frame with any other placement refuses —
//! both checked before any conversion. `from_clipboard` is New from Clipboard: one "Background"
//! record at the payload's own depth/profile/size/PPI (a float payload's tone map carried). The
//! `.rz` format is untouched.

use std::sync::Arc;

use serde::Deserialize;

use crate::doc_lock::EditKind;
use crate::icc::IccProfile;
use crate::job_control::{DrJobControl, JobContext};
use crate::layer_insert::InsertPosition;
use crate::metadata::Metadata;
use crate::precision_clipboard_codec::payload_bytes;
pub use crate::precision_clipboard_codec::ClipboardPayload;
use crate::precision_clipboard_group::GROUP_COPY_MARGIN_PER_PIXEL;
pub use crate::precision_clipboard_host::host_stage_bytes;
pub use crate::precision_clipboard_promote::PASTE_PROMOTE_MARGIN_PER_PIXEL;
use crate::precision_document::{Content, PrecisionDocument, PrecisionDocumentLayer};
use crate::precision_image::{
    admit, pixel_count, reserved_vec, PrecisionImage, PrecisionRun, PrecisionStorage,
    BOOKKEEPING_BYTES,
};
use crate::precision_paint::{canvas_index, PaintEdit};

/// The admission terms one paste shares between its plan, its allocation
/// and the paste itself.
struct PasteTerms {
    /// The conversion's own admission (0 when nothing converts).
    conversion: u64,
    /// The insertion's scratch.
    insertion: u64,
    /// The promotion margin (0 unless a conversion into float runs).
    margin: u64,
    /// `2 · converted.payload_bytes()` (0 when nothing converts).
    held: u64,
}

/// What a read copies: one raster record's raw samples, or the projection.
/// Windows are canvas coordinates.
#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum ClipboardRead {
    Copy {
        layer: usize,
        x: u32,
        y: u32,
        width: u32,
        height: u32,
    },
    CopyMerged {
        x: u32,
        y: u32,
        width: u32,
        height: u32,
    },
}

/// Where a pasted record sits: centred on the canvas, at the offset the
/// payload was copied from, or (`Into`, Paste Into / Paste Outside) centred
/// on the request's `frame` — the host's selection bounds.
#[derive(Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PastePlacement {
    Centered,
    InPlace,
    Into,
}

/// How a payload in another profile lands: Convert (the default)
/// converts its pixels into the document's profile, keeping their
/// appearance; Preserve ("Don't Convert") keeps their numbers and
/// relabels them with the document's profile. Depth is always converted.
#[derive(Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PasteProfile {
    #[default]
    Convert,
    Preserve,
}

/// A paste: the new record's name and its slot (`layer_insert`'s rule).
/// `profile` (default Convert) is how a payload in another profile lands.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PasteRequest {
    pub name: String,
    pub anchor: usize,
    pub position: InsertPosition,
    pub placement: PastePlacement,
    #[serde(default)]
    pub profile: PasteProfile,
    /// `[x, y, width, height]` in canvas pixels; required by (and only valid
    /// with) `PastePlacement::Into`.
    #[serde(default)]
    pub frame: Option<[i64; 4]>,
}

/// A window inside the canvas, refused unless it is non-empty and contained.
fn validated_window(
    doc: &PrecisionDocument,
    x: u32,
    y: u32,
    width: u32,
    height: u32,
) -> Result<(), String> {
    let (w, h) = doc.dimensions();
    if width == 0
        || height == 0
        || u64::from(x) + u64::from(width) > u64::from(w)
        || u64::from(y) + u64::from(height) > u64::from(h)
    {
        return Err(format!(
            "clipboard window {width}×{height} at ({x}, {y}) is empty or leaves the {w}×{h} canvas"
        ));
    }
    Ok(())
}

/// `alpha · coverage` in u16: the rounded integer product, exact at 0 and 65535.
pub(crate) fn cover_u16(alpha: u16, coverage: u16) -> u16 {
    ((u32::from(alpha) * u32::from(coverage) + 32767) / 65535) as u16
}

/// `alpha · coverage` in f32: exact at coverage 65535.
pub(crate) fn cover_f32(alpha: f32, coverage: u16) -> f32 {
    alpha * (f32::from(coverage) / 65535.0)
}

/// The window's samples, read by rows out of the source placed at its
/// offset (zeros where the source does not reach), with alpha multiplied by
/// the canvas coverage when one is in force.
struct Window<'a> {
    x: u32,
    y: u32,
    width: u32,
    height: u32,
    canvas_width: u32,
    coverage: Option<&'a [u16]>,
}

impl Window<'_> {
    fn coverage_at(&self, px: u32, py: u32) -> u16 {
        self.coverage.map_or(u16::MAX, |c| {
            c[(self.y + py) as usize * self.canvas_width as usize + (self.x + px) as usize]
        })
    }

    fn cropped_coverage(&self, job: JobContext<'_>) -> Result<Option<Arc<Vec<u16>>>, String> {
        let Some(_) = self.coverage else {
            return Ok(None);
        };
        let mut out = reserved_vec(pixel_count(self.width, self.height)?)?;
        for py in 0..self.height {
            job.check()?;
            for px in 0..self.width {
                out.push(self.coverage_at(px, py));
            }
        }
        Ok(Some(Arc::new(out)))
    }

    /// The window's layer-space columns `[a, b)` over an image placed at
    /// `offset`, and their clamp `[lx0, lx1)` to the image's `w` columns.
    fn columns(&self, w: u32, offset: (i32, i32)) -> (i64, i64, i64) {
        let a = i64::from(self.x) - i64::from(offset.0);
        let b = a + i64::from(self.width);
        (a, a.clamp(0, i64::from(w)), b.clamp(0, i64::from(w)))
    }

    /// Row `py`'s layer row, `None` when it misses the image's `h` rows.
    fn layer_row(&self, py: u32, h: u32, offset: (i32, i32)) -> Option<u32> {
        let ly = i64::from(self.y) + i64::from(py) - i64::from(offset.1);
        (ly >= 0 && ly < i64::from(h)).then_some(ly as u32)
    }

    /// Every window row read through `image.row_segments` (zeros where the
    /// image placed at `offset` does not reach), alpha covered.
    fn u16(
        &self,
        profile: Arc<IccProfile>,
        image: &PrecisionImage,
        offset: (i32, i32),
        job: JobContext<'_>,
    ) -> Result<PrecisionImage, String> {
        let (w, h) = image.dimensions();
        let (a, lx0, lx1) = self.columns(w, offset);
        let width = self.width as usize;
        let mut out = reserved_vec(pixel_count(self.width, self.height)?)?;
        for py in 0..self.height {
            job.check()?;
            let start = out.len();
            match self.layer_row(py, h, offset) {
                Some(ly) if lx0 < lx1 => {
                    out.resize(start + (lx0 - a) as usize, [0; 4]);
                    for (_, run) in image.row_segments(ly, lx0 as u32, lx1 as u32) {
                        let PrecisionRun::Rgba16(run) = run else {
                            continue;
                        };
                        out.extend_from_slice(run);
                    }
                    out.resize(start + width, [0; 4]);
                }
                _ => out.resize(start + width, [0; 4]),
            }
            debug_assert_eq!(out.len() - start, width);
            for (px, pixel) in out[start..].iter_mut().enumerate() {
                pixel[3] = cover_u16(pixel[3], self.coverage_at(px as u32, py));
            }
        }
        PrecisionImage::from_rgba16_unseeded(self.width, self.height, out, profile, None)
    }

    /// The f32 twin of [`Self::u16`].
    fn f32(
        &self,
        profile: Arc<IccProfile>,
        image: &PrecisionImage,
        offset: (i32, i32),
        job: JobContext<'_>,
    ) -> Result<PrecisionImage, String> {
        let (w, h) = image.dimensions();
        let (a, lx0, lx1) = self.columns(w, offset);
        let width = self.width as usize;
        let mut out = reserved_vec(pixel_count(self.width, self.height)?)?;
        for py in 0..self.height {
            job.check()?;
            let start = out.len();
            match self.layer_row(py, h, offset) {
                Some(ly) if lx0 < lx1 => {
                    out.resize(start + (lx0 - a) as usize, [0.0; 4]);
                    for (_, run) in image.row_segments(ly, lx0 as u32, lx1 as u32) {
                        let PrecisionRun::Rgba32(run) = run else {
                            continue;
                        };
                        out.extend_from_slice(run);
                    }
                    out.resize(start + width, [0.0; 4]);
                }
                _ => out.resize(start + width, [0.0; 4]),
            }
            debug_assert_eq!(out.len() - start, width);
            for (px, pixel) in out[start..].iter_mut().enumerate() {
                pixel[3] = cover_f32(pixel[3], self.coverage_at(px as u32, py));
            }
        }
        PrecisionImage::from_rgba32_extended(self.width, self.height, out, profile, None)
    }

    /// Reads every window pixel out of `image` placed at `offset`.
    fn read(
        &self,
        image: &PrecisionImage,
        offset: (i32, i32),
        profile: Arc<IccProfile>,
        job: JobContext<'_>,
    ) -> Result<PrecisionImage, String> {
        match image.storage() {
            PrecisionStorage::Rgba16Unorm => self.u16(profile, image, offset, job),
            PrecisionStorage::Rgba32Float => self.f32(profile, image, offset, job),
        }
    }
}

/// `image` placed at `offset` on a transparent `canvas`-sized image in its
/// own storage: the whole-canvas [`Window`] read with no coverage, so every
/// sample copies through exactly (`cover_u16`/`cover_f32` at 65535 are the
/// identity) and zeros fill where the image does not reach.
pub(crate) fn placed_on_canvas(
    image: &PrecisionImage,
    offset: (i32, i32),
    canvas: (u32, u32),
    profile: Arc<IccProfile>,
    job: JobContext<'_>,
) -> Result<PrecisionImage, String> {
    Window {
        x: 0,
        y: 0,
        width: canvas.0,
        height: canvas.1,
        canvas_width: canvas.0,
        coverage: None,
    }
    .read(image, offset, profile, job)
}

impl PrecisionDocument {
    /// Copy (a raster record's raw samples) or Copy Merged (the projection)
    /// of one canvas window. `coverage` is canvas-sized u16 or `None` for a
    /// fully covered window.
    pub fn clipboard_copy(
        &self,
        read: &ClipboardRead,
        coverage: Option<&[u16]>,
        limit: u64,
        other: u64,
        control: Option<&DrJobControl>,
    ) -> Result<ClipboardPayload, String> {
        let _run = control.map(DrJobControl::begin).transpose()?;
        let job = control.map_or(JobContext::NONE, JobContext::new);
        self.clipboard_copy_with_context(read, coverage, limit, other, job)
    }

    fn clipboard_copy_with_context(
        &self,
        read: &ClipboardRead,
        coverage: Option<&[u16]>,
        limit: u64,
        other: u64,
        job: JobContext<'_>,
    ) -> Result<ClipboardPayload, String> {
        job.check()?;
        admit(0, BOOKKEEPING_BYTES, other, limit)?;
        job.reserve(BOOKKEEPING_BYTES)?;
        let (x, y, width, height) = match *read {
            ClipboardRead::Copy {
                x,
                y,
                width,
                height,
                ..
            }
            | ClipboardRead::CopyMerged {
                x,
                y,
                width,
                height,
            } => (x, y, width, height),
        };
        validated_window(self, x, y, width, height)?;
        let canvas = pixel_count(self.width, self.height)?;
        if coverage.is_some_and(|c| c.len() != canvas) {
            return Err("clipboard coverage must be canvas-sized".into());
        }
        admit(
            0,
            host_stage_bytes(width, height, self.render_storage(), coverage.is_some())?,
            other,
            limit,
        )?;
        let window = Window {
            x,
            y,
            width,
            height,
            canvas_width: self.width,
            coverage,
        };
        let image = match *read {
            ClipboardRead::Copy { layer, .. } => {
                let record = self
                    .layer(layer)
                    .ok_or("clipboard layer index is out of range")?;
                match &record.content {
                    Content::CmykRaster { .. } | Content::CmykAdjustment(_) => return Err(crate::cmyk_document::refused_sentence("Copy")),
                    Content::Raster { image, offset } => {
                        let bytes =
                            2 * payload_bytes(width, height, image.storage(), coverage.is_some());
                        admit(0, bytes, other, limit)?;
                        job.reserve(bytes)?;
                        window.read(image, *offset, self.profile.clone(), job)?
                    }
                    // A group copies its RAW projection
                    // (`precision_clipboard_group`).
                    Content::Group(_) | Content::PassThrough(_) => {
                        let source = self.raw_group_document(layer)?;
                        let margin = GROUP_COPY_MARGIN_PER_PIXEL
                            .checked_mul(canvas as u64)
                            .ok_or("clipboard size overflow")?;
                        self.read_rendered(&source, &window, canvas, margin, limit, other, job)?
                    }
                    Content::Adjustment(_) => {
                        return Err(format!(
                            "Copy requires a raster layer; layer {layer} is an adjustment layer (use Copy Merged)"
                        ))
                    }
                }
            }
            ClipboardRead::CopyMerged { .. } => {
                self.read_rendered(self, &window, canvas, 0, limit, other, job)?
            }
        };
        let coverage = window.cropped_coverage(job)?;
        job.report(1.0, 5)?;
        Ok(ClipboardPayload {
            image,
            offset: (x as i32, y as i32),
            resolution: self.resolution(),
            coverage,
            byte_origin: false,
            tone_map: self.tone_map(),
            editable: None,
        })
    }

    /// Copy Merged's render-then-window read, over `source` (this document
    /// for Copy Merged, the raw group document for a group's Copy): admit
    /// the canvas-sized render plus `2 · payload` plus `margin`, reserve the
    /// payload, render `source` with the payload counted as other retained
    /// bytes, then read the window out of the render.
    #[allow(clippy::too_many_arguments)]
    fn read_rendered(
        &self,
        source: &PrecisionDocument,
        window: &Window<'_>,
        canvas: usize,
        margin: u64,
        limit: u64,
        other: u64,
        job: JobContext<'_>,
    ) -> Result<PrecisionImage, String> {
        let storage = source.render_storage();
        let bytes = 2 * payload_bytes(
            window.width,
            window.height,
            storage,
            window.coverage.is_some(),
        );
        let render = canvas as u64 * storage.bytes_per_pixel();
        admit(
            0,
            render
                .checked_add(bytes)
                .and_then(|n| n.checked_add(margin))
                .ok_or("clipboard size overflow")?,
            other,
            limit,
        )?;
        job.reserve(bytes)?;
        let other = other.checked_add(bytes).ok_or("clipboard size overflow")?;
        let rendered = source.render_with_context(limit, other, job.range(0.0, 0.9))?;
        window.read(&rendered, (0, 0), self.profile.clone(), job)
    }

    /// Cut: [`Self::clipboard_copy`] of record `layer`'s window, then exactly
    /// Clear's erase under the same coverage. Answers `(payload, None)` when
    /// the erase changed nothing — a transparency-locked record, or a window
    /// that is already transparent — and refuses a pixels lock before any
    /// payload exists. A float document erases through
    /// [`Self::erased_float`] under the same locks.
    #[allow(clippy::too_many_arguments)]
    pub fn clipboard_cut(
        &self,
        layer: usize,
        x: u32,
        y: u32,
        width: u32,
        height: u32,
        coverage: Option<&[u16]>,
        limit: u64,
        other: u64,
        control: Option<&DrJobControl>,
    ) -> Result<(ClipboardPayload, Option<Self>), String> {
        let _run = control.map(DrJobControl::begin).transpose()?;
        let job = control.map_or(JobContext::NONE, JobContext::new);
        if self
            .layer(layer)
            .is_some_and(|record| record.children().is_some())
        {
            return Err(format!(
                "Cut requires a raster layer; layer {layer} is a group"
            ));
        }
        self.lock_refusal(layer, EditKind::Pixels)?;
        let read = ClipboardRead::Copy {
            layer,
            x,
            y,
            width,
            height,
        };
        let payload =
            self.clipboard_copy_with_context(&read, coverage, limit, other, job.range(0.0, 0.4))?;
        let held = 2 * payload.payload_bytes();
        admit(0, held, other, limit)?;
        let canvas = pixel_count(self.width, self.height)?;
        let full;
        let erase_coverage = match coverage {
            Some(c) => c,
            None => {
                admit(
                    0,
                    canvas as u64 * 2,
                    other.checked_add(held).ok_or("clipboard size overflow")?,
                    limit,
                )?;
                let mut plane = reserved_vec(canvas)?;
                plane.resize(canvas, u16::MAX);
                full = plane;
                &full
            }
        };
        // The erase's other retained bytes are the caller's, the payload with
        // its encoding, and the full-coverage plane this call built.
        let erase_other = other
            .checked_add(held)
            .and_then(|n| {
                n.checked_add(if coverage.is_none() {
                    canvas as u64 * 2
                } else {
                    0
                })
            })
            .ok_or("clipboard size overflow")?;
        let document = if self.is_float() {
            let job = job.range(0.4, 1.0);
            self.under_locks(layer, EditKind::Pixels, |doc| {
                doc.erased_float(layer, erase_coverage, limit, erase_other, job)
            })?
        } else {
            self.paint_with_context(
                &PaintEdit::Erase {
                    layer,
                    opacity: 1.0,
                },
                erase_coverage,
                limit,
                erase_other,
                job.range(0.4, 1.0),
            )?
        };
        Ok((payload, document))
    }

    /// The float Cut's erase of record `layer` under canvas `coverage`:
    /// `alpha' = blend::erase_alpha(a, c / 65535)` — the erase at opacity 1
    /// — in f32, RGB retained, the coverage's rect written back the way
    /// `painting_unlocked` writes its rect. `Ok(None)` when no sample changed.
    /// Layer Via Cut in a float document runs it too (`precision_layer_via`).
    pub(crate) fn erased_float(
        &self,
        layer: usize,
        coverage: &[u16],
        limit: u64,
        other: u64,
        job: JobContext<'_>,
    ) -> Result<Option<Self>, String> {
        let scratch = self.erased_float_scratch(layer)?;
        let record = self
            .layer(layer)
            .ok_or("high-depth layer index is out of range")?;
        let image = record.source().ok_or("Cut requires a raster layer")?;
        let (w, h) = image.dimensions();
        admit(self.retained_bytes(), scratch, other, limit)?;
        job.reserve(scratch)?;
        let offset = record.offset();
        // 13B-3d-1: only the coverage's rect on the layer is read and written.
        let rect = crate::layer_rect::coverage_rect16(coverage, self.dimensions(), (w, h), offset);
        let Some((rx, ry, rw, rh)) = rect else {
            job.report(1.0, 5)?;
            return Ok(None);
        };
        let read = image.read_rect_rgba32(rx, ry, rw, rh);
        let mut pixels = read.ok_or("could not allocate precision output")?;
        let mut changed = false;
        for (row, line) in pixels.chunks_exact_mut(rw as usize).enumerate() {
            job.check()?;
            let cy = i64::from(ry) + row as i64 + i64::from(offset.1);
            for (col, pixel) in line.iter_mut().enumerate() {
                let cx = i64::from(rx) + col as i64 + i64::from(offset.0);
                let ci = canvas_index(cx, cy, self.width, self.height);
                let Some(ci) = ci.filter(|&ci| coverage[ci] != 0) else {
                    continue;
                };
                let alpha = crate::blend::erase_alpha(pixel[3], f32::from(coverage[ci]) / 65535.0);
                changed |= alpha.to_bits() != pixel[3].to_bits();
                pixel[3] = alpha;
            }
            job.report((row + 1) as f64 / f64::from(rh), 3)?;
        }
        let written = changed.then(|| image.with_rect_rgba32(rx, ry, rw, rh, &pixels));
        let Some(image) = written.transpose()?.flatten() else {
            job.report(1.0, 5)?;
            return Ok(None);
        };
        let result = self.replacing_content(layer, Content::Raster { image, offset })?;
        job.report(1.0, 5)?;
        Ok(result)
    }

    /// [`Self::erased_float`]'s scratch for record `layer`, and its
    /// refusals of a missing record, a record with no raster and storage that
    /// is not f32: `4·BOOKKEEPING_BYTES` plus 16 B/px of the record (the
    /// rect it reads is at most the whole record). The ONE statement of that
    /// charge: the erase admits it and a float Layer Via Cut plans it.
    pub(crate) fn erased_float_scratch(&self, layer: usize) -> Result<u64, String> {
        let record = self
            .layer(layer)
            .ok_or("high-depth layer index is out of range")?;
        let image = record.source().ok_or("Cut requires a raster layer")?;
        if image.storage() != PrecisionStorage::Rgba32Float {
            return Err("a float Cut requires rgba32_float storage".into());
        }
        let (w, h) = image.dimensions();
        Ok(4 * BOOKKEEPING_BYTES + pixel_count(w, h)? as u64 * 16)
    }

    /// Paste: the payload as one raster record at the slot `request` names.
    /// A payload whose storage or profile differs from the document's is
    /// first brought into the document's storage and profile under
    /// `request.profile` ([`ClipboardPayload::converted_with`]; Preserve on
    /// the same storage relabels and converts nothing), never refused for
    /// the mismatch. A 16-bit payload is promoted into a float
    /// document. The only remaining refusals are the conversion's own (a
    /// profile with no matrix/TRC model under Convert, a float value outside
    /// the stored range) and the insertion's.
    pub fn clipboard_paste(
        &self,
        payload: &ClipboardPayload,
        request: &PasteRequest,
        limit: u64,
        other: u64,
        control: Option<&DrJobControl>,
    ) -> Result<Self, String> {
        let _run = control.map(DrJobControl::begin).transpose()?;
        let job = control.map_or(JobContext::NONE, JobContext::new);
        job.check()?;
        match (request.placement, request.frame) {
            (PastePlacement::Into, Some([_, _, fw, fh])) if fw > 0 && fh > 0 => {}
            (PastePlacement::Into, _) => {
                return Err(
                    "placement into requires a frame [x, y, width, height] with a positive size"
                        .into(),
                )
            }
            (_, Some(_)) => return Err("a paste frame is only valid with placement into".into()),
            (_, None) => {}
        }
        admit(0, BOOKKEEPING_BYTES, other, limit)?;
        job.reserve(BOOKKEEPING_BYTES)?;
        let storage = self.render_storage();
        let converted;
        let mut other = other;
        let mut margin = 0;
        let payload = if payload.storage() == storage
            && payload.image().profile().bytes() == self.profile.bytes()
        {
            payload
        } else {
            match payload.converted_with_context(
                storage,
                &self.profile,
                request.profile,
                limit,
                other,
                job.range(0.0, 0.8),
            )? {
                Some(next) => {
                    other = other
                        .checked_add(2 * next.payload_bytes())
                        .ok_or("clipboard size overflow")?;
                    if self.is_float() {
                        margin = self.paste_promote_margin()?;
                    }
                    converted = next;
                    &converted
                }
                // Preserve on the same storage: the numbers are kept and the
                // insertion below relabels them through `with_profile`.
                None => payload,
            }
        };
        let (w, h) = payload.dimensions();
        let offset = match request.placement {
            PastePlacement::InPlace => payload.offset(),
            PastePlacement::Centered => {
                let dx = i64::from(self.width) - i64::from(w);
                let dy = i64::from(self.height) - i64::from(h);
                (
                    i32::try_from(dx.div_euclid(2)).map_err(|_| "paste offset overflow")?,
                    i32::try_from(dy.div_euclid(2)).map_err(|_| "paste offset overflow")?,
                )
            }
            PastePlacement::Into => {
                let [fx, fy, fw, fh] = request.frame.ok_or("placement into requires a frame")?;
                // The Centered rule over the frame instead of the canvas.
                let x = fw
                    .checked_sub(i64::from(w))
                    .map(|d| d.div_euclid(2))
                    .and_then(|d| fx.checked_add(d));
                let y = fh
                    .checked_sub(i64::from(h))
                    .map(|d| d.div_euclid(2))
                    .and_then(|d| fy.checked_add(d));
                (
                    x.and_then(|v| i32::try_from(v).ok())
                        .ok_or("paste offset overflow")?,
                    y.and_then(|v| i32::try_from(v).ok())
                        .ok_or("paste offset overflow")?,
                )
            }
        };
        let slot = self.insert_slot(request.anchor, request.position)?;
        self.preflight_raster_addition((w, h), &request.name)?;
        // The pixels are the payload's own allocation (shared, never copied);
        // what the insertion builds is the rebuilt tree, bounded the way the
        // structure ops bound theirs.
        let scratch = self
            .paste_insertion_scratch(&request.name)
            .checked_add(margin)
            .ok_or("clipboard size overflow")?;
        admit(self.retained_bytes(), scratch, other, limit)?;
        job.reserve(scratch)?;
        let image = payload.image().with_profile(self.profile.clone());
        let record = if self.is_float() {
            PrecisionDocumentLayer::raster_float(&request.name, image, offset, 1.0, true)?
        } else {
            PrecisionDocumentLayer::raster(&request.name, image, offset, 1.0, true)?
        };
        let next = self.inserting_record(record, slot)?;
        job.report(1.0, 5)?;
        Ok(next)
    }

    /// The largest single admission [`Self::clipboard_paste`] makes for
    /// `payload` and `request` with no other retained bytes, i.e. the exact
    /// threshold: `max(BOOKKEEPING_BYTES, conversion, retained_bytes() +
    /// insertion + margin + 2·converted)`, where `conversion` is
    /// [`ClipboardPayload::conversion_bytes`] (0 when nothing converts),
    /// `converted` the converted payload's `payload_bytes` (0 when nothing
    /// converts) and `margin` [`PASTE_PROMOTE_MARGIN_PER_PIXEL`] × canvas
    /// when a conversion into float runs.
    pub fn clipboard_paste_plan(
        &self,
        payload: &ClipboardPayload,
        request: &PasteRequest,
    ) -> Result<u64, String> {
        let terms = self.clipboard_paste_terms(payload, request)?;
        let insertion = self
            .retained_bytes()
            .checked_add(terms.insertion)
            .and_then(|n| n.checked_add(terms.margin))
            .and_then(|n| n.checked_add(terms.held))
            .ok_or("clipboard size overflow")?;
        Ok(BOOKKEEPING_BYTES.max(terms.conversion).max(insertion))
    }

    /// The NEW memory the same paste allocates — the plan without the
    /// document's resident bytes (`group_copy_allocation`'s rule):
    /// `max(conversion, insertion + margin + 2·converted)`, so it never
    /// exceeds the plan. The measurement gate compares it with the measured
    /// footprint (`precision_clipboard_promote`'s record).
    pub fn clipboard_paste_allocation(
        &self,
        payload: &ClipboardPayload,
        request: &PasteRequest,
    ) -> Result<u64, String> {
        let terms = self.clipboard_paste_terms(payload, request)?;
        let insertion = terms
            .insertion
            .checked_add(terms.margin)
            .and_then(|n| n.checked_add(terms.held))
            .ok_or("clipboard size overflow")?;
        Ok(terms.conversion.max(insertion))
    }

    /// The terms the plan, the allocation and the paste share (the
    /// `group_copy_terms` pattern).
    fn clipboard_paste_terms(
        &self,
        payload: &ClipboardPayload,
        request: &PasteRequest,
    ) -> Result<PasteTerms, String> {
        let storage = self.render_storage();
        let mismatch = payload.storage() != storage
            || payload.image().profile().bytes() != self.profile.bytes();
        let conversion = if mismatch {
            payload.conversion_bytes(storage, &self.profile, request.profile)?
        } else {
            0
        };
        let ran = conversion > 0;
        let held = if ran {
            let (w, h) = payload.dimensions();
            payload_bytes(w, h, storage, payload.coverage().is_some())
                .checked_add(payload.editable().map_or(0, |s| s.len() as u64))
                .and_then(|n| n.checked_mul(2))
                .ok_or("clipboard size overflow")?
        } else {
            0
        };
        let margin = if ran && self.is_float() {
            self.paste_promote_margin()?
        } else {
            0
        };
        Ok(PasteTerms {
            conversion,
            insertion: self.paste_insertion_scratch(&request.name),
            margin,
            held,
        })
    }

    /// The insertion's scratch: the rebuilt tree, bounded the way the
    /// structure ops bound theirs.
    fn paste_insertion_scratch(&self, name: &str) -> u64 {
        let names: u64 = (0..self.layer_count())
            .filter_map(|i| self.layer(i))
            .map(|layer| layer.name().len() as u64)
            .sum();
        4 * BOOKKEEPING_BYTES + (names + name.len() as u64) * 2 + self.layer_count() as u64 * 4096
    }

    /// [`PASTE_PROMOTE_MARGIN_PER_PIXEL`] × this document's canvas.
    fn paste_promote_margin(&self) -> Result<u64, String> {
        let (w, h) = self.dimensions();
        PASTE_PROMOTE_MARGIN_PER_PIXEL
            .checked_mul(pixel_count(w, h)? as u64)
            .ok_or_else(|| "clipboard size overflow".to_string())
    }

    /// New from Clipboard: a document of the payload's own dimensions,
    /// profile and resolution holding ONE "Background" raster record at
    /// (0, 0) and `Metadata::default()` — u16 through `from_parts`, f32
    /// through `from_parts_float` under the payload's carried tone map. The
    /// payload's offset is dropped and its coverage is already in alpha. The
    /// editable section is never read here: the host trims the payload first
    /// (`ClipboardPayload::trim`) and restores the section's record into the
    /// document this returns.
    pub fn from_clipboard(
        payload: &ClipboardPayload,
        limit: u64,
        other: u64,
        control: Option<&DrJobControl>,
    ) -> Result<Self, String> {
        let _run = control.map(DrJobControl::begin).transpose()?;
        let job = control.map_or(JobContext::NONE, JobContext::new);
        job.check()?;
        admit(0, 4 * BOOKKEEPING_BYTES, other, limit)?;
        job.reserve(4 * BOOKKEEPING_BYTES)?;
        let (w, h) = payload.dimensions();
        let profile = payload.image().profile().clone();
        let image = payload.image().clone();
        let document = match payload.storage() {
            PrecisionStorage::Rgba16Unorm => Self::from_parts(
                w,
                h,
                profile,
                Metadata::default(),
                payload.resolution(),
                vec![PrecisionDocumentLayer::raster(
                    "Background",
                    image,
                    (0, 0),
                    1.0,
                    true,
                )?],
            )?,
            PrecisionStorage::Rgba32Float => Self::from_parts_float(
                w,
                h,
                profile,
                Metadata::default(),
                payload.resolution(),
                vec![PrecisionDocumentLayer::raster_float(
                    "Background",
                    image,
                    (0, 0),
                    1.0,
                    true,
                )?],
                Vec::new(),
                payload
                    .tone_map()
                    .unwrap_or(crate::float_document::ToneMap::Clip),
            )?,
        };
        job.report(1.0, 5)?;
        Ok(document)
    }
}
