//! Stack Modes at 16 bits and 32-bit float: the typed adapter over
//! `stack_stat`, whose module doc is the rule and the admission record, and
//! the twin of the byte `doc_stack_mode`.
//!
//! The request reaches here as `ffi_precision_edit`'s `stack_mode` op. A
//! 16-bit document's statistic is of the stored RGBA16 codes; a float
//! document's of its linear RGBA32F values, computed in f64 and stored
//! unclamped, so values above white and below zero survive. One bound
//! applies: a float Range can reach twice the storage bound
//! (`float_document::FLOAT_RANGE_MAX`), and is stored as the bound, because
//! a record past it is not a float record.
//!
//! The members are read row by row through `PrecisionImage::row_segments`
//! into one extent-wide row per member; no member is made contiguous. The
//! result is ONE new record built by `merged_leaf` at the extent's origin,
//! spliced at `insert_slot(above, Above)` with `clipped` forced off
//! (`precision_stamp_flatten::stamp_visible`'s sequence), and every member
//! that was visible is hidden in the same rebuild. Lock-exempt: it adds a
//! record and changes visibility only.
//!
//! A CMYK document takes `cmyk_stack_mode`, which shares
//! [`PrecisionDocument::stack_mode_frame`] and
//! [`PrecisionDocument::stacked_document`] with this arm.
//!
//! Refusals, each by a sentence: `StackModeRequest::members`' own; a member
//! out of range, a group, an adjustment layer or storage that is not the
//! document's own (named by index); `above` out of range; an extent that is
//! empty, past the pixel cap or past the native capacity; the working limit;
//! a cancel; and members with no pixel anywhere. Never `Ok(None)`.

use serde_json::{json, Value};

use crate::job_control::JobContext;
use crate::layer_insert::{InsertPosition, InsertSlot};
use crate::precision_document::{Content, PrecisionDocument, PrecisionDocumentLayer};
use crate::precision_image::{
    admit, pixel_count, reserved_vec, PrecisionImage, PrecisionRun, PrecisionStorage,
    BOOKKEEPING_BYTES,
};
use crate::precision_structure_ops::{flatten, rebuild};
use crate::stack_stat::{
    fill_row, plan_scratch, stack_row, union_extent, StackMode, StackModeRequest, StackSample,
    StackScratch, NO_PIXELS,
};

/// A pixel rectangle `(x, y, width, height)`.
type Extent = (i32, i32, u32, u32);

/// One stored sample of either typed storage, as the row loop reads it.
trait Stored: StackSample {
    /// The run's samples when it holds this storage.
    fn run(run: PrecisionRun<'_>) -> Option<&[Self]>;
    /// The sample held to its storage's own range (module doc).
    fn bounded(self) -> Self;
}

impl Stored for [u16; 4] {
    fn run(run: PrecisionRun<'_>) -> Option<&[Self]> {
        match run {
            PrecisionRun::Rgba16(run) => Some(run),
            PrecisionRun::Rgba32(_) => None,
        }
    }
    fn bounded(self) -> Self {
        self
    }
}

impl Stored for [f32; 4] {
    fn run(run: PrecisionRun<'_>) -> Option<&[Self]> {
        match run {
            PrecisionRun::Rgba32(run) => Some(run),
            PrecisionRun::Rgba16(_) => None,
        }
    }
    fn bounded(self) -> Self {
        let max = crate::float_document::FLOAT_RANGE_MAX;
        [
            self[0].clamp(-max, max),
            self[1].clamp(-max, max),
            self[2].clamp(-max, max),
            self[3],
        ]
    }
}

/// The output plane in its own storage, held until the leaf is built.
enum Stacked {
    U16(Vec<[u16; 4]>),
    F32(Vec<[f32; 4]>),
}

/// What one request resolves to against a document.
struct Terms<'a> {
    members: Vec<usize>,
    /// Each member's pixels and canvas offset, in `members`' order.
    sources: Vec<(&'a PrecisionImage, (i32, i32))>,
    /// The union of the members' pixel rectangles.
    extent: (i32, i32, u32, u32),
    scratch: u64,
}

/// The statistic over the whole extent, one checkpoint and one progress
/// report per row, and whether any output alpha is not 0.
fn stacked_plane<P: Stored>(
    sources: &[(&PrecisionImage, (i32, i32))],
    extent: (i32, i32, u32, u32),
    mode: StackMode,
    job: JobContext<'_>,
) -> Result<(Vec<P>, bool), String> {
    let (ex, ey, w, h) = extent;
    let width = w as usize;
    let mut plane: Vec<P> = reserved_vec(pixel_count(w, h)?)?;
    plane.resize(width * h as usize, P::TRANSPARENT);
    let mut rows: Vec<Vec<P>> = Vec::new();
    for _ in sources {
        let mut row = reserved_vec(width)?;
        row.resize(width, P::TRANSPARENT);
        rows.push(row);
    }
    let mut scratch = StackScratch::new(sources.len())?;
    let mut any = false;
    for y in 0..h {
        job.check()?;
        for (row, (image, offset)) in rows.iter_mut().zip(sources) {
            let (lw, lh) = image.dimensions();
            let ly = i64::from(ey) + i64::from(y) - i64::from(offset.1);
            let at = usize::try_from(i64::from(offset.0) - i64::from(ex));
            match at {
                Ok(at) if lw > 0 && (0..i64::from(lh)).contains(&ly) => fill_row(
                    row,
                    at,
                    image
                        .row_segments(ly as u32, 0, lw)
                        .filter_map(|(x, run)| P::run(run).map(|run| (x, run))),
                ),
                _ => row.fill(P::TRANSPARENT),
            }
        }
        let views: Vec<&[P]> = rows.iter().map(Vec::as_slice).collect();
        let out = &mut plane[y as usize * width..(y as usize + 1) * width];
        any |= stack_row(mode, &views, out, &mut scratch);
        for pixel in out.iter_mut() {
            *pixel = pixel.bounded();
        }
        job.report(f64::from(y + 1) / f64::from(h) * 0.9, 6)?;
    }
    Ok((plane, any))
}

impl PrecisionDocument {
    /// The checks every arm shares once its members are resolved (`rects`
    /// their pixel rectangles): `above` in range, the extent (their union)
    /// non-empty, within the pixel cap and the native capacity; and the plan's
    /// structure term. The CMYK arm (`cmyk_stack_mode`) calls it too.
    pub(crate) fn stack_mode_frame(
        &self,
        request: &StackModeRequest,
        rects: impl Iterator<Item = (i32, i32, u32, u32)>,
    ) -> Result<(Extent, u64), String> {
        if request.above >= self.layer_count() {
            return Err(format!(
                "stack mode cannot insert above layer {}: it is out of range",
                request.above
            ));
        }
        let extent = union_extent(rects).ok_or("the stacked layers have no pixel extent")?;
        let pixels = u64::from(extent.2) * u64::from(extent.3);
        if pixels > crate::doc::MAX_PIXELS {
            return Err("the stacked layers' extent exceeds 100,000,000 pixels".into());
        }
        self.preflight_raster_addition((extent.2, extent.3), &request.name)?;
        // The structure term is `StructureEdit::scratch_bytes`' own (the
        // master copy, `precision_structure`): every name twice, a record's
        // bookkeeping per layer and per channel.
        let names: u64 = (0..self.layer_count())
            .filter_map(|i| self.layer(i))
            .map(|l| l.name().len() as u64)
            .sum();
        let structure = 4 * BOOKKEEPING_BYTES
            + (request.name.len() as u64 + names) * 2
            + self.layer_count() as u64 * 4096
            + self.channels().len() as u64 * 1024;
        Ok((extent, structure))
    }

    /// The document with every member that was visible hidden in ONE
    /// rebuild, then `leaf` spliced at `slot` (resolved before any pixel was
    /// read; hiding renumbers nothing) with `clipped` forced off — every
    /// arm's tail.
    pub(crate) fn stacked_document(
        &self,
        leaf: PrecisionDocumentLayer,
        members: &[usize],
        slot: InsertSlot,
        job: JobContext<'_>,
    ) -> Result<Self, String> {
        let mut flat = flatten(&self.layers);
        for &index in members {
            if let Some(record) = flat.get_mut(index) {
                record.layer.visible = false;
            }
        }
        let hidden = self.rebuilt(
            self.width,
            self.height,
            rebuild(flat)?,
            self.channels.as_ref().clone(),
        )?;
        job.check()?;
        hidden.inserting_record(
            leaf,
            InsertSlot {
                clipped: false,
                ..slot
            },
        )
    }

    fn stack_mode_terms(&self, request: &StackModeRequest) -> Result<Terms<'_>, String> {
        let members = request.members()?;
        let float = self.is_float();
        let mut sources = Vec::with_capacity(members.len());
        for &index in &members {
            let record = self
                .layer(index)
                .ok_or_else(|| format!("stack mode layer {index} is out of range"))?;
            let (image, offset) = match &record.content {
                // A CMYK document takes `cmyk_stack_mode`; a CMYK record
                // here cannot occur, and is refused by name if it does.
                Content::CmykRaster { .. } | Content::CmykAdjustment(_) => {
                    return Err(crate::cmyk_document::refused_sentence("Stack Mode"))
                }
                Content::Raster { image, offset } => (image, *offset),
                Content::Group(_) | Content::PassThrough(_) => {
                    return Err(format!(
                        "Stack Mode requires raster layers; layer {index} is a group"
                    ))
                }
                Content::Adjustment(_) => {
                    return Err(format!(
                        "Stack Mode requires raster layers; layer {index} is an adjustment layer"
                    ))
                }
            };
            // A float document's records are f32 and every other document's
            // u16 (`PrecisionDocument::build`'s storage gate).
            let own = if float {
                PrecisionStorage::Rgba32Float
            } else {
                PrecisionStorage::Rgba16Unorm
            };
            if image.storage() != own {
                return Err(format!(
                    "stack mode layer {index} does not hold the document's own storage"
                ));
            }
            sources.push((image, offset));
        }
        let (extent, structure) = self.stack_mode_frame(
            request,
            sources.iter().map(|(image, offset)| {
                let (w, h) = image.dimensions();
                (offset.0, offset.1, w, h)
            }),
        )?;
        let scratch = plan_scratch(
            u64::from(extent.2) * u64::from(extent.3),
            u64::from(extent.2),
            if float { 16 } else { 8 },
            members.len() as u64,
            structure,
        )
        .ok_or("stack mode byte calculation overflow")?;
        Ok(Terms {
            members,
            sources,
            extent,
            scratch,
        })
    }

    /// `retained_bytes`, `scratch_bytes` and `input_samples` (always 0: the
    /// request reads no host plane). `Err` with the same sentence for every
    /// refusal the edit gives before it reads a pixel.
    pub(crate) fn stack_mode_plan(&self, request: &StackModeRequest) -> Result<Value, String> {
        if self.is_cmyk() {
            return self.cmyk_stack_mode_plan(request);
        }
        let terms = self.stack_mode_terms(request)?;
        Ok(json!({"retained_bytes": self.retained_bytes(),
                  "scratch_bytes": terms.scratch, "input_samples": 0}))
    }

    /// A Stack Mode (module doc): plan, admit against `limit` beside `other`,
    /// reserve, check, then one checkpoint and one progress report per
    /// extent row. `Err` for every refusal and for a cancel; never
    /// `Ok(None)`.
    pub(crate) fn stack_mode(
        &self,
        request: &StackModeRequest,
        limit: u64,
        other: u64,
        job: JobContext<'_>,
    ) -> Result<Option<Self>, String> {
        if self.is_cmyk() {
            return self.cmyk_stack_mode(request, limit, other, job);
        }
        let terms = self.stack_mode_terms(request)?;
        admit(self.retained_bytes(), terms.scratch, other, limit)?;
        job.reserve(terms.scratch)?;
        job.check()?;
        // The slot is resolved before any pixel is read, so a refused insert
        // costs no plane (`stamp_visible`'s order).
        let slot = self.insert_slot(request.above, InsertPosition::Above)?;
        let (ex, ey, w, h) = terms.extent;
        let (stacked, any) = if self.is_float() {
            let (plane, any) = stacked_plane(&terms.sources, terms.extent, request.mode, job)?;
            (Stacked::F32(plane), any)
        } else {
            let (plane, any) = stacked_plane(&terms.sources, terms.extent, request.mode, job)?;
            (Stacked::U16(plane), any)
        };
        if !any {
            return Err(NO_PIXELS.into());
        }
        let profile = self.profile.clone();
        let image = match stacked {
            Stacked::U16(plane) => {
                PrecisionImage::from_rgba16_unseeded(w, h, plane, profile, None)?
            }
            Stacked::F32(plane) => {
                PrecisionImage::from_rgba32_extended(w, h, plane, profile, None)?
            }
        };
        let leaf = self.merged_leaf(&request.name, image, (ex, ey))?;
        let next = self.stacked_document(leaf, &terms.members, slot, job)?;
        job.report(1.0, 5)?;
        Ok(Some(next))
    }
}
