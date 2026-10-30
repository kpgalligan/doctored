//! Layer Via Copy / Layer Via Cut at 16 bits and 32-bit float:
//! the typed twin of the byte `doc_structure::layer_via`, whose doc comment is
//! the contract.
//!
//! The request is `{"op":"layer_via","layer":N,"cut":bool,"name":S,
//! "selection":bool}`, every key required and unknown keys refused.
//!
//! - `selection: true` reads ONE canvas-sized u16 coverage plane. A layer
//!   pixel at canvas position `(cx, cy)` keeps its RGB exactly and takes
//!   alpha `cover_u16(alpha, coverage[cy · W + cx])`; a layer pixel off the
//!   canvas has coverage 0, because a selection never reaches past the
//!   canvas.
//! - `selection: false` reads no input and copies EVERY layer pixel at full
//!   coverage, on or off the canvas (the byte contract's `mask == None`).
//!
//! The new record has the source's dimensions and offset, no mask, style,
//! Blend If or description, Normal, opacity 1, visible and unclipped, and
//! lands directly above the source at the source's level
//! (`insert_slot(layer, Above)` with `clipped` forced off, which is
//! `precision_stamp_flatten::stamp_visible`'s sequence).
//!
//! `cut` erases the source BEFORE the insert, through `PaintEdit::Erase` at
//! opacity 1 under the same coverage (exactly Clear). With `selection:
//! false` the erase coverage is a canvas plane of 65535 this module builds
//! (`precision_clipboard::clipboard_cut`'s shape), so the source keeps its
//! off-canvas pixels, as the byte `clear_selection` leaves them.
//!
//! Refusals by name: an index out of range, a group, an adjustment layer and
//! storage that is not the document's own. `Ok(None)`: every copied
//! alpha is 0, or the cut's erase changed nothing (a transparency-locked
//! source). The copy is lock-exempt (it adds a record and writes none); the
//! cut takes the erase's own lock refusal.
//!
//! # 32-bit float
//!
//! A float document takes the same request on its RGBA32F records. A copied
//! pixel keeps its RGB bit for bit (above white and below zero included) and
//! takes alpha `cover_f32(alpha, coverage)`, `a · c / 65535` in f32, exact at
//! 65535. The cut is `precision_clipboard::erased_float`, the float Cut's own
//! erase (`blend::erase_alpha(a, c / 65535)`, RGB retained, on-canvas pixels
//! only), under `under_locks`, so a transparency lock answers `Ok(None)` as
//! at 16 bits. The new record is built by `merged_leaf`, the float leaf
//! constructor's caller.
//!
//! # Admission
//!
//! The plan's scratch is `COPIED_PLANE_COPIES · 8 B/px · source extent`,
//! plus the erase's own plan for a cut, plus `2 B/px · canvas` for a cut
//! with `selection: false` (the full plane), plus the structure term of
//! `precision_structure::StructureEdit::scratch_bytes`.
//!
//! Measured with `MallocLargeCache=0 DR_LV_CASE=<case> make t
//! F=precision_layer_via T=admission_measure A='--ignored --nocapture'`
//! (Release, one case per process, three runs each, working limit
//! 8,589,934,592): a 4000×3000 document with one opaque canvas-sized RGBA16
//! layer, coverage rect (200, 150, 3600, 2700) at 65535 for the two selection
//! cases. `E` is `ri_interval_max_phys_footprint − ri_phys_footprint` after
//! `proc_reset_footprint_interval`, with the result held until it is read.
//!
//! | case       | scratch (plan) | max E of 3  | ⌈1.25 · max E⌉ |
//! |------------|----------------|-------------|----------------|
//! | baseline   | —              | 0           | —              |
//! | copy       | 292,198,414    | 195,002,512 | 243,753,140    |
//! | cut        | 416,392,718    | 287,588,568 | 359,485,710    |
//! | copy_whole | 292,198,414    | 192,594,064 | 240,742,580    |
//! | cut_whole  | 440,392,718    | 312,721,648 | 390,902,060    |
//!
//! `baseline` is the twin: the same fixture and coverage plane with no edit
//! run, so its E is the harness's own. A first round at a factor of 2 measured
//! the same peaks (max E 194,969,744 / 287,588,592 / 192,577,704 /
//! 312,705,288) against plans 96,000,000 smaller, which missed every floor;
//! `COPIED_PLANE_COPIES` was raised to 3 and the table above re-measured. Two
//! rounds at 3 agree within 33 KB a case; the table holds the larger.
//!
//! `core/tests/precision_layer_via.rs`'s `plans_cover_the_measured_peaks`
//! holds the same `max E` values and fails when a plan drops below its floor.
//!
//! # Admission, 32-bit float
//!
//! The same plan with 16 B/px for the copied plane and
//! `PrecisionDocument::erased_float_scratch` (bookkeeping plus one 16 B/px
//! copy of the record) as the cut's erase term.
//!
//! Measured with `MallocLargeCache=0 DR_FLOAT_CASE=<case> make t
//! F=float_layer_edits T=admission_measure A='--ignored --nocapture'`
//! (Release, one case per process, three runs each, working limit
//! 8,589,934,592): a 4000×3000 float document with one opaque non-uniform
//! canvas-sized RGBA32F layer and the coverage rect above; `E` as above.
//!
//! | case           | scratch (plan) | max E of 3  | ⌈1.25 · max E⌉ |
//! |----------------|----------------|-------------|----------------|
//! | baseline       | —              | 0           | —              |
//! | via_copy       | 580,198,426    | 387,399,992 | 484,249,990    |
//! | via_cut        | 776,392,730    | 572,555,672 | 715,694,590    |
//! | via_copy_whole | 580,198,426    | 385,007,928 | 481,259,910    |
//! | via_cut_whole  | 800,392,730    | 601,129,392 | 751,411,740    |
//!
//! Every plan passed its floor in the first round, so `COPIED_PLANE_COPIES`
//! stays 3. Two rounds agree within 17 KB a case; the table holds the larger.
//!
//! `core/tests/float_layer_edits.rs`'s `plans_cover_the_measured_peaks` holds
//! the same `max E` values.
//!
//! # CMYK (#1 phase 4c)
//!
//! A CMYK document takes the same request on its CMYK raster records. A
//! copied pixel keeps its ink exactly and takes alpha `cover_u16(alpha,
//! coverage)`; the cut is `CmykPaintEdit`'s erase at opacity 1 under the
//! same coverage (exactly Clear in ink, under the source's locks, so a
//! transparency lock answers `Ok(None)`); the new record is a `raster_cmyk`
//! leaf with the same placement. The plan is the same shape with 10 B/px for
//! the copied plane and the erase's own plan.

use serde::Deserialize;
use serde_json::{json, Value};

use crate::cmyk_image::{CmykImage, CmykSample, CMYK_BYTES_PER_PIXEL};
use crate::cmyk_paint::CmykPaintEdit;
use crate::job_control::JobContext;
use crate::layer_insert::{InsertPosition, InsertSlot};
use crate::precision_clipboard::{cover_f32, cover_u16};
use crate::precision_document::{Content, PrecisionDocument, PrecisionDocumentLayer};
use crate::precision_image::{
    admit, pixel_count, reserved_vec, PrecisionImage, PrecisionRun, PrecisionStorage,
    BOOKKEEPING_BYTES,
};
use crate::precision_paint::{canvas_index, PaintEdit};

/// The plan's one factor, in copies of the source extent (8 B/px at 16 bits,
/// 16 B/px at float). The edit
/// holds two at its peak — the weighted contiguous copy and the leaf's tiles
/// built from it — and the plan must be at least 1.25 × the measured peak,
/// which two copies miss (the first measurement: E 194,969,744 against a plan
/// of 196,198,414 for the copy case), so the factor is 3. The
/// measured-admission tables in the module doc are what it is checked
/// against; a miss raises this constant.
const COPIED_PLANE_COPIES: u64 = 3;

/// One stored sample of either storage, as the copy loop reads it.
trait ViaSample: Copy {
    const TRANSPARENT: Self;
    /// The run's samples when it holds this storage.
    fn run(run: PrecisionRun<'_>) -> Option<&[Self]>;
    /// Alpha weighted by one coverage code, RGB untouched.
    fn cover(&mut self, coverage: u16);
    /// True when alpha is not 0.
    fn visible(&self) -> bool;
}

impl ViaSample for [u16; 4] {
    const TRANSPARENT: Self = [0; 4];
    fn run(run: PrecisionRun<'_>) -> Option<&[Self]> {
        match run {
            PrecisionRun::Rgba16(run) => Some(run),
            PrecisionRun::Rgba32(_) => None,
        }
    }
    fn cover(&mut self, coverage: u16) {
        self[3] = cover_u16(self[3], coverage);
    }
    fn visible(&self) -> bool {
        self[3] != 0
    }
}

impl ViaSample for [f32; 4] {
    const TRANSPARENT: Self = [0.0; 4];
    fn run(run: PrecisionRun<'_>) -> Option<&[Self]> {
        match run {
            PrecisionRun::Rgba32(run) => Some(run),
            PrecisionRun::Rgba16(_) => None,
        }
    }
    fn cover(&mut self, coverage: u16) {
        self[3] = cover_f32(self[3], coverage);
    }
    fn visible(&self) -> bool {
        self[3] != 0.0
    }
}

impl ViaSample for CmykSample {
    const TRANSPARENT: Self = [0; 5];
    /// A CMYK plane's rows are not `PrecisionRun`s; [`copied_rows`] reads
    /// them through `CmykImage::row_segments` instead.
    fn run(_: PrecisionRun<'_>) -> Option<&[Self]> {
        None
    }
    fn cover(&mut self, coverage: u16) {
        self[4] = cover_u16(self[4], coverage);
    }
    fn visible(&self) -> bool {
        self[4] != 0
    }
}

/// The copied plane in its own storage, held until the leaf is built.
enum Copied {
    U16(Vec<[u16; 4]>),
    F32(Vec<[f32; 4]>),
    Cmyk(Vec<CmykSample>),
}

/// The source record's pixels: a typed RGB image or a CMYK one.
#[derive(Clone, Copy)]
enum Source<'a> {
    Rgb(&'a PrecisionImage),
    Cmyk(&'a CmykImage),
}

impl Source<'_> {
    fn dimensions(&self) -> (u32, u32) {
        match self {
            Source::Rgb(image) => image.dimensions(),
            Source::Cmyk(image) => image.dimensions(),
        }
    }
}

/// Every row of a `width × height` source (`row(ly, out)` appends layer row
/// `ly`) as one contiguous copy, alpha weighted by the canvas `coverage`
/// when the request has a selection, and whether any copied alpha is not 0.
/// Progress runs over the first half.
fn copied_rows<P: ViaSample>(
    (width, height): (u32, u32),
    offset: (i32, i32),
    canvas: (u32, u32),
    coverage: Option<&[u16]>,
    job: JobContext<'_>,
    mut row: impl FnMut(u32, &mut Vec<P>),
) -> Result<(Vec<P>, bool), String> {
    let mut copied = reserved_vec(pixel_count(width, height)?)?;
    let mut any = false;
    for ly in 0..height {
        job.check()?;
        let start = copied.len();
        row(ly, &mut copied);
        copied.resize(start + width as usize, P::TRANSPARENT);
        if let Some(coverage) = coverage {
            let cy = i64::from(ly) + i64::from(offset.1);
            for (lx, pixel) in copied[start..].iter_mut().enumerate() {
                let cx = lx as i64 + i64::from(offset.0);
                // A selection never reaches past the canvas, so a layer
                // pixel hanging off it is never selected.
                let covered = canvas_index(cx, cy, canvas.0, canvas.1).map_or(0, |ci| coverage[ci]);
                pixel.cover(covered);
            }
        }
        any |= copied[start..].iter().any(P::visible);
        job.report(0.5 * f64::from(ly + 1) / f64::from(height), 3)?;
    }
    Ok((copied, any))
}

/// [`copied_rows`] over a typed RGB image: its runs of storage `P`.
fn copied_image_rows<P: ViaSample>(
    image: &PrecisionImage,
    offset: (i32, i32),
    canvas: (u32, u32),
    coverage: Option<&[u16]>,
    job: JobContext<'_>,
) -> Result<(Vec<P>, bool), String> {
    let (width, height) = image.dimensions();
    copied_rows((width, height), offset, canvas, coverage, job, |ly, out| {
        for (_, run) in image.row_segments(ly, 0, width) {
            if let Some(run) = P::run(run) {
                out.extend_from_slice(run);
            }
        }
    })
}

/// [`copied_rows`] over a CMYK image's ink.
fn copied_cmyk_rows(
    image: &CmykImage,
    offset: (i32, i32),
    canvas: (u32, u32),
    coverage: Option<&[u16]>,
    job: JobContext<'_>,
) -> Result<(Vec<CmykSample>, bool), String> {
    let (width, height) = image.dimensions();
    copied_rows((width, height), offset, canvas, coverage, job, |ly, out| {
        for (_, run) in image.row_segments(ly, 0, width) {
            out.extend_from_slice(run);
        }
    })
}

/// The CMYK cut's erase: `CmykPaintEdit`'s at opacity 1 over the canvas
/// coverage plane (module doc).
fn cmyk_erase(layer: usize) -> CmykPaintEdit {
    CmykPaintEdit::EraseNative {
        layer,
        opacity: 1.0,
        selection: false,
    }
}

/// `{op:"layer_via",layer,cut,name,selection}` — a one-variant tagged enum
/// (`precision_style::LayerStyleEdit`'s shape), so a missing or unknown key
/// refuses.
#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum LayerViaEdit {
    LayerVia {
        layer: usize,
        cut: bool,
        name: String,
        selection: bool,
    },
}

/// What one request resolves to against a document: the source and every
/// admission term, shared by the plan and the edit.
struct Terms<'a> {
    source: Source<'a>,
    offset: (i32, i32),
    /// Canvas pixels.
    canvas: usize,
    /// The copied plane in one copy, bytes.
    copy: u64,
    /// The core-built full coverage plane (cut with `selection: false`), bytes.
    full_plane: u64,
    /// The whole plan.
    scratch: u64,
}

impl LayerViaEdit {
    pub(crate) fn parse(text: &str) -> Result<Self, String> {
        serde_json::from_str(text).map_err(|e| format!("invalid high-depth layer via request: {e}"))
    }

    fn terms<'a>(&self, document: &'a PrecisionDocument) -> Result<Terms<'a>, String> {
        let Self::LayerVia {
            layer,
            cut,
            name,
            selection,
        } = self;
        let record = document
            .layer(*layer)
            .ok_or("high-depth layer via index is out of range")?;
        let (source, offset) = match &record.content {
            Content::CmykRaster { image, offset } => (Source::Cmyk(image), *offset),
            Content::Raster { image, offset } => (Source::Rgb(image), *offset),
            Content::Group(_) | Content::PassThrough(_) => {
                return Err(format!(
                    "Layer Via requires a raster layer; layer {layer} is a group"
                ))
            }
            Content::Adjustment(_) | Content::CmykAdjustment(_) => {
                return Err(format!(
                    "Layer Via requires a raster layer; layer {layer} is an adjustment layer"
                ))
            }
        };
        // A float document's records are f32 and every other RGB document's
        // u16 (`PrecisionDocument::build`'s storage gate); a CMYK document's
        // are CMYKA16 (`build_cmyk`'s).
        let float = document.is_float();
        let bytes_per_pixel = match source {
            Source::Cmyk(_) => CMYK_BYTES_PER_PIXEL,
            Source::Rgb(image) => {
                if float && image.storage() != PrecisionStorage::Rgba32Float {
                    return Err(
                        "layer via in a 32-bit float document requires rgba32_float storage".into(),
                    );
                }
                if !float && image.storage() != PrecisionStorage::Rgba16Unorm {
                    return Err("high-depth layer via requires u16 storage".into());
                }
                image.storage().bytes_per_pixel()
            }
        };
        crate::precision_document::scalar_and_name(name, 1.0)?;
        let canvas = pixel_count(document.width, document.height)?;
        let (width, height) = source.dimensions();
        let overflow = || "high-depth layer via byte calculation overflow".to_string();
        let copy = (pixel_count(width, height)? as u64)
            .checked_mul(bytes_per_pixel)
            .ok_or_else(overflow)?;
        let erase = match (*cut, float, source) {
            (false, _, _) => 0,
            (true, _, Source::Cmyk(_)) => cmyk_erase(*layer).plan(document)?["scratch_bytes"]
                .as_u64()
                .ok_or("invalid CMYK erase plan")?,
            (true, true, _) => document.erased_float_scratch(*layer)?,
            (true, false, _) => {
                document
                    .paint_plan(&PaintEdit::Erase {
                        layer: *layer,
                        opacity: 1.0,
                    })?
                    .scratch_bytes
            }
        };
        let full_plane = if *cut && !*selection {
            canvas as u64 * 2
        } else {
            0
        };
        // The structure term is `StructureEdit::scratch_bytes`' own (the
        // master copy, `precision_structure`): every name twice, a record's
        // bookkeeping per layer and per channel.
        let names: u64 = (0..document.layer_count())
            .filter_map(|i| document.layer(i))
            .map(|l| l.name().len() as u64)
            .sum();
        let structure = 4 * BOOKKEEPING_BYTES
            + (name.len() as u64 + names) * 2
            + document.layer_count() as u64 * 4096
            + document.channels().len() as u64 * 1024;
        let scratch = COPIED_PLANE_COPIES
            .checked_mul(copy)
            .and_then(|n| n.checked_add(erase))
            .and_then(|n| n.checked_add(full_plane))
            .and_then(|n| n.checked_add(structure))
            .ok_or_else(overflow)?;
        Ok(Terms {
            source,
            offset,
            canvas,
            copy,
            full_plane,
            scratch,
        })
    }

    /// `retained_bytes`, `scratch_bytes` and `input_samples` (the canvas for
    /// `selection: true`, else 0).
    pub(crate) fn plan(&self, document: &PrecisionDocument) -> Result<Value, String> {
        let terms = self.terms(document)?;
        let Self::LayerVia { selection, .. } = self;
        let input = if *selection { terms.canvas } else { 0 };
        Ok(json!({"retained_bytes": document.retained_bytes(),
                  "scratch_bytes": terms.scratch, "input_samples": input}))
    }

    /// The edit. `coverage` is the canvas plane for `selection: true` and
    /// empty for `selection: false`.
    pub(crate) fn apply(
        &self,
        document: &PrecisionDocument,
        coverage: &[u16],
        limit: u64,
        other: u64,
        job: JobContext<'_>,
    ) -> Result<Option<PrecisionDocument>, String> {
        let Self::LayerVia {
            layer,
            cut,
            name,
            selection,
        } = self;
        let terms = self.terms(document)?;
        admit(document.retained_bytes(), terms.scratch, other, limit)?;
        job.reserve(terms.scratch)?;
        job.check()?;
        if coverage.len() != if *selection { terms.canvas } else { 0 } {
            return Err("high-depth coverage length does not match its declared extent".into());
        }
        // The erase's own refusal, asked before the copy is made
        // (`clipboard_cut`'s order).
        if *cut {
            document.lock_refusal(*layer, crate::doc_lock::EditKind::Pixels)?;
        }
        // The slot and the addition are resolved before any pixel is copied,
        // so a refused insert costs no plane (`stamp_visible`'s order).
        let slot = document.insert_slot(*layer, InsertPosition::Above)?;
        let (width, height) = terms.source.dimensions();
        document.preflight_raster_addition((width, height), name)?;
        let offset = terms.offset;
        let float = document.is_float();
        let canvas = (document.width, document.height);
        let weights = selection.then_some(coverage);
        let (copied, any) = match terms.source {
            Source::Cmyk(image) => {
                let (copied, any) = copied_cmyk_rows(image, offset, canvas, weights, job)?;
                (Copied::Cmyk(copied), any)
            }
            Source::Rgb(image) if float => {
                let (copied, any) = copied_image_rows(image, offset, canvas, weights, job)?;
                (Copied::F32(copied), any)
            }
            Source::Rgb(image) => {
                let (copied, any) = copied_image_rows(image, offset, canvas, weights, job)?;
                (Copied::U16(copied), any)
            }
        };
        if !any {
            job.report(1.0, 5)?;
            return Ok(None);
        }
        // The cut runs on the SOURCE first, under the source's locks, with
        // the copied plane (and the full plane, when this call builds it)
        // counted as other retained bytes.
        let cut_document;
        let base = if *cut {
            let held = other
                .checked_add(terms.copy)
                .and_then(|n| n.checked_add(terms.full_plane))
                .ok_or("high-depth layer via byte calculation overflow")?;
            let full;
            let erase_coverage = if *selection {
                coverage
            } else {
                let mut plane = reserved_vec(terms.canvas)?;
                plane.resize(terms.canvas, u16::MAX);
                full = plane;
                &full
            };
            let erased = if let Source::Cmyk(_) = terms.source {
                // `CmykPaintEdit`'s erase at opacity 1, under the source's
                // locks (`cmyk_clipboard::cmyk_clipboard_cut`'s call).
                cmyk_erase(*layer).apply(
                    document,
                    erase_coverage,
                    limit,
                    held,
                    job.range(0.5, 0.9),
                )?
            } else if float {
                // The float Cut's own erase, under the source's locks
                // (`precision_clipboard::clipboard_cut`'s call).
                let job = job.range(0.5, 0.9);
                document.under_locks(*layer, crate::doc_lock::EditKind::Pixels, |doc| {
                    doc.erased_float(*layer, erase_coverage, limit, held, job)
                })?
            } else {
                document.paint_with_context(
                    &PaintEdit::Erase {
                        layer: *layer,
                        opacity: 1.0,
                    },
                    erase_coverage,
                    limit,
                    held,
                    job.range(0.5, 0.9),
                )?
            };
            let Some(erased) = erased else {
                job.report(1.0, 5)?;
                return Ok(None);
            };
            cut_document = erased;
            &cut_document
        } else {
            document
        };
        let profile = document.profile.clone();
        let leaf = match copied {
            Copied::U16(copied) => base.merged_leaf(
                name,
                PrecisionImage::from_rgba16_unseeded(width, height, copied, profile, None)?,
                offset,
            )?,
            Copied::F32(copied) => base.merged_leaf(
                name,
                PrecisionImage::from_rgba32_extended(width, height, copied, profile, None)?,
                offset,
            )?,
            Copied::Cmyk(copied) => {
                let row = width as usize;
                let image = CmykImage::from_rows_with_job(
                    width,
                    height,
                    |y, out| {
                        let at = y as usize * row;
                        out.copy_from_slice(&copied[at..at + row]);
                        Ok(())
                    },
                    job,
                )?;
                PrecisionDocumentLayer::raster_cmyk(name, image, offset, 1.0, true)?
            }
        };
        let next = base.inserting_record(
            leaf,
            InsertSlot {
                clipped: false,
                ..slot
            },
        )?;
        job.report(1.0, 5)?;
        Ok(Some(next))
    }
}
