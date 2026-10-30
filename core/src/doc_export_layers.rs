//! Per-item deliverables: one top-level layer or group exported on its own,
//! through the SAME `doc_export` chain a whole-document export uses.
//!
//! # What an "item" is
//!
//! A depth-0 entry of the layer stack — a raster or a whole group with its
//! members. Nested entries are not exported individually: a layer inside a
//! group is part of that group's picture, and pulling it out would deliver a
//! composite nobody has ever seen. The items are reported in PANEL order (top
//! first), which is the order the host numbers them in.
//!
//! # The isolated render
//!
//! An item is rendered by building a document that holds ONLY that item's
//! subtree and flattening it. That is the whole rule, and it is deliberately
//! the shipped compositor rather than a second one:
//!
//! * The item's own opacity, blend mode, mask, style and Blend If apply, and
//!   they apply against TRANSPARENCY, because there is nothing else in the
//!   stack. A Multiply layer therefore delivers its own pixels rather than
//!   the black its blend against a white backdrop would produce.
//! * The root's `visible` flag is forced true — the host asked for THIS item,
//!   and a hidden item that exported an empty frame would be a silent
//!   failure. Descendants keep their own visibility, because a hidden member
//!   of a group is part of how that group looks.
//! * A CLIPPED top-level item composites unclipped. Its base is a different
//!   item and is not exported with it, so it is alone at the bottom of its
//!   level — the existing "a clipped entry with no unclipped sibling below
//!   composites as if unclipped" rule, not a new one.
//!
//! # Canvas-framed or trimmed
//!
//! `trimmed: false` delivers the canvas: every item of a document lines up
//! pixel for pixel, which is what a compositing hand-off or a contact-sheet
//! cell wants, and a fully transparent item delivers a fully transparent
//! frame. `trimmed: true` delivers the item's own alpha bounding box, which
//! is what a sprite or a cut-out wants — and there a fully transparent item
//! is a REFUSAL, because there is no rectangle to deliver. A 32-bit float
//! item trims too, and its samples are copied exactly.
//!
//! `top_level_items` reports them in PANEL order (top first), which is where `{index}`'s ordinal
//! comes from. `isolated_root` builds a document holding ONLY that item's subtree (byte:
//! `doc_group::subtree` + `validated`; typed: `rebuilt(w, h, vec![root], vec![])`) with the ROOT's
//! `visible` forced true and descendants' own visibility kept, then renders it through the shipped
//! compositor (`flattened` / `render_with_context`) — so the item's opacity, blend, mask, style and
//! Blend If apply against TRANSPARENCY, and a clipped top-level item composites unclipped because
//! its base is a different item. `trimmed` takes the alpha bounding box (`alpha_bounds8` /
//! `alpha_bounds16` / `alpha_bounds32` → `imageops::crop_imm` / `crop_rect`, the ONE bare-image
//! crop, one `read_rect_rgba16/32` in the render's own storage — so a 32-bit float item trims too
//! and its samples are copied exactly) and is the ONLY place a fully transparent item refuses; the
//! byte `alpha_bounds8` polls the export's own job once per row (`edge_bounds`, H checkpoints), so
//! a cancel lands inside the scan; the typed `alpha_bounds16` / `alpha_bounds32` poll the export's
//! job once per 256-row band (one shared tile-band walk, the float arm included), while the
//! clipboard trim and the content box pass `NONE`. The typed `export_layer_plan` is the isolated
//! document's own `export_plan` widened by its one-root `render_plan().scratch_bytes` and a trim
//! copy (one canvas at `render_storage().bytes_per_pixel()`: 8 at u16, 16 at float). A TRIMMED
//! typed export re-admits and re-reserves after the trim when `export_plan_for` over the cut
//! exceeds the canvas plan (a size mode scales from the cut, so a small item can ask for a large
//! output); `export_layer_plan` and its FFI stay canvas-based, because the cut is unknown before
//! the render. Float Export Layers takes size modes too.

use image::RgbaImage;
use serde_json::Value;

use crate::doc::DrDocument;
use crate::doc_export::{ExportRequest, ExportResult};
use crate::doc_group;
use crate::job_control::{DrJobControl, JobContext};
use crate::precision_document::PrecisionDocument;
use crate::precision_image::{PrecisionImage, PrecisionRun, PrecisionStorage};
use crate::tile_store::TILE_EDGE;

/// What one item's result describes: which entry it was, what it was called,
/// and where in the canvas the delivered rectangle sits.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LayerExportInfo {
    pub index: usize,
    pub name: String,
    /// Canvas position of the delivered rectangle's top-left pixel. `(0, 0)`
    /// for a canvas-framed export.
    pub origin: (u32, u32),
    /// The delivered rectangle BEFORE any resample — the canvas for a
    /// canvas-framed export, the alpha bounding box for a trimmed one.
    pub size: (u32, u32),
}

/// The tightest rectangle containing every pixel with alpha > 0, as
/// `(x, y, width, height)`. `Ok(None)` when the image is fully transparent;
/// `Err` is `job`'s own error (a cancel), never folded into `None`. The
/// scan is `doc_group_query::edge_bounds`, the one the content box uses, so a
/// trimmed export and `dr_doc_layer_bounds` agree by construction.
pub(crate) fn alpha_bounds8(
    image: &RgbaImage,
    job: JobContext<'_>,
) -> Result<Option<(u32, u32, u32, u32)>, String> {
    use crate::doc_group_query::{edge_bounds, runs_extent};
    let (w, h) = image.dimensions();
    let pixels = image.as_raw().as_chunks::<4>().0;
    // Polls the export's own job once per row (`h` checkpoints), so a cancel
    // lands inside the scan.
    let bounds = edge_bounds(w, h, job, |y, x0, x1| {
        let row = y as usize * w as usize;
        let run = &pixels[row + x0 as usize..row + x1 as usize];
        runs_extent(std::iter::once((x0, run)), |_, p| p[3] != 0)
    })?;
    Ok(bounds.map(|(x0, y0, x1, y1)| (x0, y0, x1 - x0, y1 - y0)))
}

/// The typed twin over an RGBA16 image's tiles, walked in TILE-BAND order
/// (each 256-row band, each 256-column span, each row's runs): the extremes
/// are order-independent, so the answer is the forward scan's. `Ok(None)`
/// when nothing is visible, and for f32 storage; `Err` is `job`'s.
pub(crate) fn alpha_bounds16(
    image: &PrecisionImage,
    job: JobContext<'_>,
) -> Result<Option<(u32, u32, u32, u32)>, String> {
    tile_band_bounds(image, job, |run| {
        let PrecisionRun::Rgba16(run) = run else {
            return None;
        };
        let first = run.iter().position(|p| p[3] != 0)?;
        Some((first, run.iter().rposition(|p| p[3] != 0).unwrap_or(first)))
    })
}

/// The f32 twin of [`alpha_bounds16`]: the same tile-band walk over an
/// RGBA32 image, a pixel visible when its alpha is `> 0.0`, with the same
/// poll contract (a float document's trimmed layer export passes its
/// job). `Ok(None)` when nothing is visible, and for u16 storage; `Err` is
/// `job`'s.
pub(crate) fn alpha_bounds32(
    image: &PrecisionImage,
    job: JobContext<'_>,
) -> Result<Option<(u32, u32, u32, u32)>, String> {
    tile_band_bounds(image, job, |run| {
        let PrecisionRun::Rgba32(run) = run else {
            return None;
        };
        let first = run.iter().position(|p| p[3] > 0.0)?;
        Some((first, run.iter().rposition(|p| p[3] > 0.0).unwrap_or(first)))
    })
}

/// The ONE typed tile-band walk behind [`alpha_bounds16`] and
/// [`alpha_bounds32`]. `span` answers a run's first and last visible index —
/// a run's extremes are its first and last visible pixels — or `None` for
/// the other storage or a run with nothing visible. Poll contract: exactly
/// `h.div_ceil(TILE_EDGE)` calls to `job.check()`, one at the top of each
/// band before its reads; the walk is full and never exits early, and `Err`
/// is the job's own error, never folded into `None`.
fn tile_band_bounds(
    image: &PrecisionImage,
    job: JobContext<'_>,
    span: impl Fn(&PrecisionRun<'_>) -> Option<(usize, usize)>,
) -> Result<Option<(u32, u32, u32, u32)>, String> {
    let (w, h) = image.dimensions();
    let mut extent = Extent::new();
    for band in (0..h).step_by(TILE_EDGE as usize) {
        job.check()?;
        for x0 in (0..w).step_by(TILE_EDGE as usize) {
            let x1 = x0.saturating_add(TILE_EDGE).min(w);
            for y in band..band.saturating_add(TILE_EDGE).min(h) {
                for (rx, run) in image.row_segments(y, x0, x1) {
                    if let Some((first, last)) = span(&run) {
                        extent.include(rx + first as u32, y);
                        extent.include(rx + last as u32, y);
                    }
                }
            }
        }
    }
    Ok(extent.rect())
}

/// The extremes of the visible pixels seen so far, in any visiting order.
struct Extent {
    min_x: u32,
    min_y: u32,
    max_x: u32,
    max_y: u32,
    any: bool,
}

impl Extent {
    fn new() -> Self {
        Self {
            min_x: u32::MAX,
            min_y: u32::MAX,
            max_x: 0,
            max_y: 0,
            any: false,
        }
    }

    fn include(&mut self, x: u32, y: u32) {
        self.any = true;
        self.min_x = self.min_x.min(x);
        self.min_y = self.min_y.min(y);
        self.max_x = self.max_x.max(x);
        self.max_y = self.max_y.max(y);
    }

    /// `(x, y, width, height)`; `None` when nothing was included.
    fn rect(&self) -> Option<(u32, u32, u32, u32)> {
        self.any.then(|| {
            (
                self.min_x,
                self.min_y,
                self.max_x - self.min_x + 1,
                self.max_y - self.min_y + 1,
            )
        })
    }
}

/// A typed image cropped to `rect`, by ONE `read_rect_rgba16` /
/// `read_rect_rgba32` in its own storage — the ONE bare-image crop in this
/// crate. `precision_geometry` crops DOCUMENTS (masks, channels, placement
/// and all); there is nothing here but samples, and routing a single
/// rectangle through the document machinery would mean rebuilding a
/// document to take it apart again.
pub(crate) fn crop_rect(
    image: &PrecisionImage,
    rect: (u32, u32, u32, u32),
) -> Result<PrecisionImage, String> {
    let (x, y, w, h) = rect;
    let (sw, sh) = image.dimensions();
    if w == 0
        || h == 0
        || x.checked_add(w).is_none_or(|e| e > sw)
        || y.checked_add(h).is_none_or(|e| e > sh)
    {
        return Err("a trimmed layer export rectangle must lie inside the canvas".into());
    }
    match image.storage() {
        PrecisionStorage::Rgba16Unorm => {
            let out = image
                .read_rect_rgba16(x, y, w, h)
                .ok_or("could not allocate precision output")?;
            PrecisionImage::from_rgba16_unseeded(w, h, out, image.profile().clone(), None)
        }
        PrecisionStorage::Rgba32Float => {
            let out = image
                .read_rect_rgba32(x, y, w, h)
                .ok_or("could not allocate precision output")?;
            PrecisionImage::from_rgba32_extended(w, h, out, image.profile().clone(), None)
        }
    }
}

/// The refusal a fully transparent item earns when it is asked for TRIMMED.
fn empty_refusal(name: &str) -> String {
    format!("layer \"{name}\" renders no visible pixels; nothing to trim")
}

// ----------------------------------------------------------- the byte path --

impl DrDocument {
    /// The stack indices of every top-level entry, in PANEL order (top
    /// first). The host numbers items from this list, so `{index}` and the
    /// per-item report agree with what the Layers panel shows.
    pub fn top_level_items(&self) -> Vec<usize> {
        let mut out: Vec<usize> = doc_group::top_level(&self.layers)
            .into_iter()
            .map(|unit| unit.idx)
            .collect();
        out.reverse();
        out
    }

    /// A document holding ONLY `idx`'s subtree, with the root visible.
    /// `None` when `idx` is not a top-level entry or the extracted subtree is
    /// somehow malformed — a no-op rather than a corrupt document.
    pub(crate) fn isolated_root(&self, idx: usize) -> Option<DrDocument> {
        if self.layers.get(idx)?.depth != 0 {
            return None;
        }
        let range = doc_group::subtree(&self.layers, idx);
        let mut out = self.clone();
        out.layers = self.layers[range].to_vec();
        out.layers.last_mut()?.visible = true;
        doc_group::validated(out)
    }

    /// Encode one top-level item as a deliverable, at 8 bits.
    pub fn export_layer_encoded(
        &self,
        idx: usize,
        trimmed: bool,
        request: &ExportRequest,
        control: Option<&DrJobControl>,
    ) -> Result<ExportResult, String> {
        if request.bit_depth != 8 {
            return Err("an 8-bit document exports at 8 bits; convert it to 16-bit first".into());
        }
        if let Some(refusal) = request.destination_refusal(&self.profile) {
            return Err(refusal);
        }
        let name = self
            .layers
            .get(idx)
            .ok_or("layer index out of range")?
            .name
            .clone();
        let isolated = self
            .isolated_root(idx)
            .ok_or("a layer export needs a top-level layer or group")?;

        let _run = control.map(DrJobControl::begin).transpose()?;
        let job = control.map_or(JobContext::NONE, JobContext::new);
        job.check()?;

        let rendered = isolated.flattened()?;
        job.check()?;
        let (source, origin, size) = if trimmed {
            let rect = alpha_bounds8(&rendered, job)?.ok_or_else(|| empty_refusal(&name))?;
            let cropped =
                image::imageops::crop_imm(&rendered, rect.0, rect.1, rect.2, rect.3).to_image();
            (cropped, (rect.0, rect.1), (rect.2, rect.3))
        } else {
            let dims = rendered.dimensions();
            (rendered, (0, 0), dims)
        };
        job.check()?;
        Ok(self
            .export_stages(&source, request, job)?
            .with_layer(LayerExportInfo {
                index: idx,
                name,
                origin,
                size,
            }))
    }
}

// ---------------------------------------------------------- the typed path --

impl PrecisionDocument {
    /// The postorder indices of every top-level record, in PANEL order (top
    /// first) — the twin of the byte list, over the typed forest.
    pub fn top_level_items(&self) -> Vec<usize> {
        let mut out = Vec::new();
        let mut base = 0usize;
        for layer in self.root_layers() {
            base += layer.records;
            out.push(base - 1);
        }
        out.reverse();
        out
    }

    /// A typed document holding ONLY the root whose postorder index is `idx`,
    /// with that root visible.
    pub(crate) fn isolated_root(&self, idx: usize) -> Result<PrecisionDocument, String> {
        let mut base = 0usize;
        for layer in self.root_layers() {
            base += layer.records;
            if base - 1 == idx {
                let mut root = layer.clone();
                root.visible = true;
                let (w, h) = self.dimensions();
                return self.rebuilt(w, h, vec![root], Vec::new());
            }
            if base > idx {
                break;
            }
        }
        Err("a layer export needs a top-level layer or group".into())
    }

    /// Admission for one typed item export: the isolated document's own
    /// export plan, widened by the scratch its ONE-ROOT render spends.
    pub fn export_layer_plan(
        &self,
        idx: usize,
        trimmed: bool,
        request: &ExportRequest,
    ) -> Result<Value, String> {
        // #547 Indexed phase 2d: the Index layer (`indexed_export`).
        if self.is_indexed() {
            return self.indexed_layer_export_plan(idx, trimmed, request);
        }
        let isolated = self.isolated_root(idx)?;
        let mut plan = isolated.export_plan(None, request)?;
        let render = isolated.render_plan()?.scratch_bytes;
        let scratch = plan["scratch_bytes"].as_u64().unwrap_or(0);
        // A trim copies the cropped rectangle out of the rendered plane, and
        // the rectangle can be the whole canvas; charge a second full plane
        // in the render's storage (8 B/px at u16, 16 at float) rather than a
        // rectangle nobody has measured yet.
        let trim = if trimmed {
            crate::precision_image::pixel_count(self.dimensions().0, self.dimensions().1)? as u64
                * self.render_storage().bytes_per_pixel()
        } else {
            0
        };
        plan["scratch_bytes"] = Value::from(
            scratch
                .checked_add(render)
                .and_then(|v| v.checked_add(trim))
                .ok_or("high-depth layer export scratch calculation overflow")?,
        );
        Ok(plan)
    }

    /// Encode one top-level item as a deliverable, at 32, 16 or 8 bits.
    /// Admission is the canvas-based [`Self::export_layer_plan`]; a TRIMMED
    /// export then re-admits and re-reserves with the plan over its cut
    /// (`export_plan_for`) when that one is larger — a size mode scales from
    /// the trimmed rectangle, so a small cut can ask for a larger output
    /// than the canvas would.
    pub fn export_layer_encoded(
        &self,
        idx: usize,
        trimmed: bool,
        request: &ExportRequest,
        limit: u64,
        other: u64,
        control: Option<&DrJobControl>,
    ) -> Result<ExportResult, String> {
        // #547 Indexed phase 2d: the Index layer (`indexed_export`).
        if self.is_indexed() {
            return self.indexed_layer_export_encoded(idx, trimmed, request, limit, other, control);
        }
        let plan = self.export_layer_plan(idx, trimmed, request)?;
        let retained = plan["retained_bytes"].as_u64().unwrap_or(0);
        let scratch = plan["scratch_bytes"].as_u64().unwrap_or(0);
        crate::precision_image::admit(retained, scratch, other, limit)?;

        let isolated = self.isolated_root(idx)?;
        let name = isolated
            .root_layers()
            .first()
            .map(|layer| (*layer.name).clone())
            .unwrap_or_default();

        let _run = control.map(DrJobControl::begin).transpose()?;
        let job = control.map_or(JobContext::NONE, JobContext::new);
        job.check()?;
        job.reserve(scratch)?;

        let rendered = isolated.render_with_context(limit, other, job)?;
        job.check()?;
        let (source, origin, size) = if trimmed {
            let rect = match rendered.storage() {
                PrecisionStorage::Rgba16Unorm => alpha_bounds16(&rendered, job)?,
                PrecisionStorage::Rgba32Float => alpha_bounds32(&rendered, job)?,
            }
            .ok_or_else(|| empty_refusal(&name))?;
            let size = (rect.2, rect.3);
            let scratch_of = |plan: Value| plan["scratch_bytes"].as_u64().unwrap_or(0);
            let canvas = scratch_of(isolated.export_plan(None, request)?);
            let cut = scratch_of(isolated.export_plan_for(size, None, request)?);
            if cut > canvas {
                let needed = scratch
                    .checked_sub(canvas)
                    .and_then(|v| v.checked_add(cut))
                    .ok_or("high-depth layer export scratch calculation overflow")?;
                // The host preflight is canvas-based, so this refusal names
                // what it could not see: the cut and the size mode's output.
                crate::precision_image::admit(retained, needed, other, limit).map_err(|_| {
                    let (ow, oh) =
                        crate::doc_export::output_dimensions(size, request).unwrap_or(size);
                    let total = retained.saturating_add(needed).saturating_add(other);
                    let mib = |v: u64| v.div_ceil(1024 * 1024);
                    format!(
                        "Exporting layer \"{name}\" trimmed to {} × {} and resized to {ow} × {oh} needs {} MiB of working memory, more than the {} MiB working limit. Export it untrimmed or at a smaller size.",
                        size.0,
                        size.1,
                        mib(total),
                        limit / (1024 * 1024)
                    )
                })?;
                job.reserve(needed)?;
            }
            (crop_rect(&rendered, rect)?, (rect.0, rect.1), size)
        } else {
            let dims = rendered.dimensions();
            (rendered, (0, 0), dims)
        };
        job.check()?;
        Ok(isolated
            .export_stages(&source, request, job)?
            .with_layer(LayerExportInfo {
                index: idx,
                name,
                origin,
                size,
            }))
    }
}
