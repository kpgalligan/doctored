//! The PSD side of the document's PATHS and of VECTOR MASKS: image resources
//! 2000–2997 (saved paths, each named by its resource's Pascal name), 1025
//! (the work path) and 2999 (the clipping path) in every PSD, PSB, layered
//! TIFF and flat TIFF (`tiff_paths`), and a layer's `vmsk` in
//! every mode the PSD codecs carry layers in (the typed ones through
//! `psd_typed_exchange`). The records are `psd_path_records`'.
//!
//! **The clipping path.** Import marks the kept saved path resource 2999
//! names as the clipping path (`doc_path::ClippingPath`), keeping the
//! resource's bytes after the name; a 2999 that names no kept saved path, or
//! does not parse, is a sentence and no mark. Export writes 2999 after the
//! paths when the marked path is itself written: its name, then that tail.
//!
//! **Paths.** Paths are document state at every depth and in every mode, so
//! every importer reads them ([`file_paths`]) on both import choices and
//! every writer writes them ([`resources`]). Import is entry by entry: a
//! saved path whose records do not decode, whose shape the model refuses,
//! whose name is empty, the work path's or another path's, or that is past
//! the model's caps is dropped with its own sentence, and every other path
//! is kept. A path that starts from all pixels (an inverted initial fill) or
//! combines its subpaths in a way Doctored's one fill rule cannot hold is
//! kept as its outline with a sentence. Export writes the work path as 1025
//! and each saved path as 2000 + its index, its name in the resource's Pascal
//! string (UTF-8; the model's 255-byte name cap is the Pascal string's own);
//! a path whose coordinates leave the fixed-point range is not written, with
//! a sentence.
//!
//! **Vector masks.** A `vmsk` is that layer's vector mask (`set_vector_mask`),
//! enabled and linked from its flags, when its path is one Doctored holds
//! exactly ([`vmsk_shape`]), it fits the document's anchor cap beside the
//! file's paths and the masks before it ([`MaskRoom`]), and its record's mask
//! channels are a form the import reads (`psd_raster_masks::VectorMaskRole`):
//! none, Photoshop's `-2` rendered from the path (dropped), or the user's own
//! mask (`-3`, or at 16 bits an unrendered `-2`), which becomes the vector
//! mask's raster part. On a raster record, any other `vmsk` becomes a raster
//! mask: the `-2` the file rendered from it, or with none the path drawn
//! under its operations ([`vmsk_coverage`]), each with a sentence. The
//! import draws only a mask within the drawing caps ([`bakes`]: one shape's
//! subpath and anchor caps, and [`MAX_BAKE_SAMPLES`] of canvas passes); one
//! past them is dropped with the dropped-mask sentence. A drawn mask is
//! planned as a canvas plane (`psd_raster_masks::MaskBudget`), drawn under
//! the open's job, and a mask the rasterizer refuses refuses the layered
//! open by the layer's name, never leaving the layer unmasked. A `vmsk`
//! over a fill the mode's open does not import as a fill layer (every fill at
//! Gray and Lab, a Gradient or Pattern fill at 32 bits), any other on a shape
//! layer, and a `vsms` keep the importer's prior answer: the record's mask is
//! dropped with its sentence, and a shape layer keeps the pixels Photoshop
//! stored, already masked. The decision is the record walk's
//! (`psd_group_records::vector_mask_usable` and the role), so the report and
//! every mode's open agree. The writers write a live
//! vector mask with NO raster part as a `vmsk` and no mask channel, the
//! authored form; one with a raster part, or one a PSD path cannot hold,
//! keeps its baked mask and a sentence.

use std::collections::HashSet;

use crate::doc_path::{
    ClippingPath, PathEntry, PathKind, MAX_CLIPPING_TAIL_BYTES, MAX_PATHS, MAX_PATH_NAME_BYTES,
    WORK_PATH_NAME,
};
use crate::path_geometry::PathShape;
use crate::psd_container::{resource_entries, Container};
use crate::psd_export_encode::ResourceBlock;
use crate::psd_path_records::{decode, decode_clipping, encode, encode_clipping};

/// The work path's resource.
pub(crate) const WORK_PATH_RESOURCE: u16 = 1025;
/// The first and last saved-path resources.
pub(crate) const FIRST_SAVED: u16 = 2000;
pub(crate) const LAST_SAVED: u16 = 2997;
/// The clipping path's resource: the name of the saved path it designates.
pub(crate) const CLIPPING_PATH_RESOURCE: u16 = 2999;

/// The file's paths and the import report's sentences about them.
pub(crate) fn file_paths(container: &Container<'_>) -> (Vec<PathEntry>, Vec<String>) {
    run_paths(container.resources, (container.width, container.height))
}

/// The paths of an 8BIM `run` (a PSD's resource section, a TIFF's tag
/// 34377) whose records are fractions of `canvas`, and the import report's
/// sentences about them (module doc): [`file_paths`]' body, and a flat
/// TIFF's (`tiff_paths`).
pub(crate) fn run_paths(run: &[u8], canvas: (u32, u32)) -> (Vec<PathEntry>, Vec<String>) {
    let mut losses = Vec::new();
    let Ok(resources) = resource_entries(run) else {
        return (Vec::new(), losses);
    };
    let clipping = resources
        .iter()
        .find(|(id, _, _)| *id == CLIPPING_PATH_RESOURCE)
        .map(|(_, _, body)| *body);
    let mut found: Vec<(u16, &[u8], &[u8])> = resources
        .into_iter()
        .filter(|(id, _, _)| *id == WORK_PATH_RESOURCE || (FIRST_SAVED..=LAST_SAVED).contains(id))
        .collect();
    // The work path first, then the saved paths in id order.
    found.sort_by_key(|(id, _, _)| (*id != WORK_PATH_RESOURCE, *id));
    let mut entries: Vec<PathEntry> = Vec::new();
    let mut names = HashSet::new();
    let mut anchors = 0usize;
    for (id, name, body) in found {
        let work = id == WORK_PATH_RESOURCE;
        let name = if work {
            WORK_PATH_NAME.to_string()
        } else {
            String::from_utf8_lossy(name).trim().to_string()
        };
        let label = if work {
            "The work path".to_string()
        } else {
            format!("Path “{name}”")
        };
        let decoded = match decode(body, canvas) {
            Ok(decoded) => decoded,
            Err(reason) => {
                losses.push(format!(
                    "{label} could not be read ({reason}); it is not imported."
                ));
                continue;
            }
        };
        if decoded.inverted {
            losses.push(format!("{label} starts from all pixels; Doctored keeps its outline, which fills the inside instead."));
        }
        if decoded.combined {
            losses.push(format!("{label} combines its subpaths by intersecting or subtracting them; Doctored keeps its outline under one fill rule (nonzero)."));
        }
        // The shape first, so a path dropped for its shape claims no name.
        let shape = match decoded.shape.validated() {
            Ok(shape) => shape,
            Err(reason) => {
                losses.push(format!("{label} is not imported: {reason}."));
                continue;
            }
        };
        let refusal = if !work && name.is_empty() {
            Some("it has no name".to_string())
        } else if !work && name == WORK_PATH_NAME {
            Some(format!("its name is “{WORK_PATH_NAME}”, the work path's"))
        } else if !work && name.len() > MAX_PATH_NAME_BYTES {
            Some(format!(
                "its name is longer than {MAX_PATH_NAME_BYTES} bytes"
            ))
        } else if !work && !names.insert(name.clone()) {
            Some("another path has its name".to_string())
        } else if entries.len() >= MAX_PATHS {
            Some(format!("a document holds at most {MAX_PATHS} paths"))
        } else {
            None
        };
        if let Some(reason) = refusal {
            losses.push(format!("{label} is not imported: {reason}."));
            continue;
        }
        let held = anchors.saturating_add(shape.anchor_count());
        if held > crate::doc_path::MAX_DOCUMENT_PATH_ANCHORS {
            losses.push(format!(
                "{label} is not imported: {}.",
                crate::doc_path::anchor_cap_sentence()
            ));
            continue;
        }
        anchors = held;
        entries.push(PathEntry {
            id: 0,
            name,
            kind: if work {
                PathKind::Work
            } else {
                PathKind::Saved
            },
            shape,
            clipping: None,
        });
    }
    if let Some(body) = clipping {
        if let Some(loss) = mark_clipping(&mut entries, body) {
            losses.push(loss);
        }
    }
    (crate::doc_path::normalized(entries), losses)
}

/// Marks the saved path resource 2999's `body` names as the clipping path,
/// its tail kept (`doc_path::ClippingPath`); the report's sentence when it
/// cannot.
fn mark_clipping(entries: &mut [PathEntry], body: &[u8]) -> Option<String> {
    let Some((name, tail)) = decode_clipping(body) else {
        return Some(
            "The clipping path resource could not be read; no clipping path is set.".into(),
        );
    };
    // Decoded as a saved path's name is (above).
    let name = String::from_utf8_lossy(name).trim().to_string();
    if tail.len() > MAX_CLIPPING_TAIL_BYTES {
        return Some(format!("The clipping path “{name}” carries more than {MAX_CLIPPING_TAIL_BYTES} bytes after its name; no clipping path is set."));
    }
    let Some(entry) = entries
        .iter_mut()
        .find(|e| e.kind == PathKind::Saved && e.name == name)
    else {
        return Some(format!("The clipping path names “{name}”, which is not one of the file's imported paths; no clipping path is set."));
    };
    entry.clipping = Some(ClippingPath {
        tail: crate::psd_path_records::kept_clipping_tail(tail),
    });
    None
}

/// The path resources a writer writes for `entries` on a `canvas`
/// (module doc): the clipping path's 2999 after the paths, when its path is
/// written.
pub(crate) fn resources(entries: &[PathEntry], canvas: (u32, u32)) -> Vec<ResourceBlock> {
    let mut saved = FIRST_SAVED;
    let mut out = Vec::new();
    let mut clipping = None;
    for entry in entries {
        let Ok(body) = encode(&entry.shape, canvas, false) else {
            continue;
        };
        match entry.kind {
            PathKind::Work => out.push(ResourceBlock {
                id: WORK_PATH_RESOURCE,
                name: String::new(),
                body,
            }),
            PathKind::Saved => {
                out.push(ResourceBlock {
                    id: saved,
                    name: entry.name.clone(),
                    body,
                });
                saved += 1;
                if let Some(mark) = &entry.clipping {
                    clipping = encode_clipping(entry.name.as_bytes(), &mark.tail);
                }
            }
        }
    }
    if let Some(body) = clipping {
        out.push(ResourceBlock {
            id: CLIPPING_PATH_RESOURCE,
            name: String::new(),
            body,
        });
    }
    out
}

/// The export report's sentences for `entries`: each path [`resources`]
/// does not write, and the clipping path that goes with it. A name always
/// fits its Pascal string (module doc).
pub(crate) fn export_losses(entries: &[PathEntry], canvas: (u32, u32)) -> Vec<String> {
    let mut losses = Vec::new();
    for entry in entries {
        let label = match entry.kind {
            PathKind::Work => "The work path".to_string(),
            PathKind::Saved => format!("Path “{}”", entry.name),
        };
        if encode(&entry.shape, canvas, false).is_err() {
            losses.push(format!("{label} reaches past the range a PSD path can hold (128 times the canvas); it is not written."));
            if entry.clipping.is_some() {
                losses.push(format!(
                    "{label} is the clipping path; the file names no clipping path."
                ));
            }
        }
    }
    losses
}

/// Whether every path of `entries` is written ([`resources`]) on a
/// `canvas`: what a flat TIFF that carries the paths asks before it drops
/// "paths" from its losses (`tiff_paths`).
pub(crate) fn all_written(entries: &[PathEntry], canvas: (u32, u32)) -> bool {
    entries
        .iter()
        .all(|entry| encode(&entry.shape, canvas, false).is_ok())
}

/// The `vmsk` body of the record built layer `k` came from, when the walk
/// carried one (`psd_group_records::vector_mask_usable`).
pub(crate) fn source_vmsk<'a>(
    records: &crate::psd_group_records::GroupRecords<'a>,
    source: crate::psd_comps::CompSource,
) -> Option<&'a [u8]> {
    match source {
        crate::psd_comps::CompSource::Layer(index) => records.layer_vmsk.get(index).copied()?,
        crate::psd_comps::CompSource::Group(id) => records.groups.get(id as usize - 1)?.vmsk,
    }
}

/// The `vmsk` body the import bakes into the mask of the layer built from
/// `source` (`psd_group_records`, `VectorMaskRole::Baked`): raster records only.
pub(crate) fn source_bake<'a>(
    records: &crate::psd_group_records::GroupRecords<'a>,
    source: crate::psd_comps::CompSource,
) -> Option<&'a [u8]> {
    match source {
        crate::psd_comps::CompSource::Layer(index) => records.layer_bake.get(index).copied()?,
        crate::psd_comps::CompSource::Group(_) => None,
    }
}

/// What the record walk weighs each `vmsk` against
/// (`psd_group_records::vector_mask_usable`): the canvas its records are
/// fractions of, and the anchors the file's kept paths ([`file_paths`])
/// hold, which every carried vector mask shares the document's one anchor
/// cap with (`doc_path::MAX_DOCUMENT_PATH_ANCHORS`).
#[derive(Clone, Copy, Debug)]
pub(crate) struct MaskRoom {
    pub canvas: (u32, u32),
    pub path_anchors: usize,
}

impl MaskRoom {
    /// The room of the file `container` holds.
    pub(crate) fn of(container: &Container<'_>) -> Self {
        Self {
            canvas: (container.width, container.height),
            path_anchors: file_paths(container)
                .0
                .iter()
                .map(|entry| entry.shape.anchor_count())
                .sum(),
        }
    }
}

/// A `vmsk` body's shape and flags when Doctored holds its path exactly as a
/// vector mask: it decodes, the model admits the shape, and the path neither
/// starts from all pixels (flag bit 0, or an initial fill record of 1) nor
/// combines its subpaths in a way one fill rule cannot hold
/// (`Decoded::combined`: carried, a subtracted or intersected subpath would
/// fill its whole outline). `None` for every other body.
pub(crate) fn vmsk_shape(body: &[u8], canvas: (u32, u32)) -> Option<(PathShape, u32)> {
    let (decoded, flags) = crate::psd_path_records::decode_vmsk(body, canvas).ok()?;
    if flags & 1 != 0 || decoded.inverted || decoded.combined {
        return None;
    }
    Some((decoded.shape.validated().ok()?, flags))
}

/// The most samples the import spends drawing one vector mask it does not
/// carry ([`vmsk_coverage`]): each subpath is a pass over the whole canvas
/// (a rasterization and a combine), so the work is subpaths × canvas
/// pixels. 2³⁰ is ten passes over a 100 MP canvas and over 250 over a 4 MP
/// one: far past any mask a person draws, and a bound on what one record of
/// a crafted file can cost the open.
pub(crate) const MAX_BAKE_SAMPLES: u64 = 1 << 30;

/// Whether a `vmsk` body bakes ([`vmsk_coverage`]): it decodes, is enabled
/// (flag bit 2 clear), names only known operations and is within the
/// drawing caps — one shape's own (`path_geometry::MAX_SUBPATHS`,
/// `MAX_PATH_ANCHORS`) and [`MAX_BAKE_SAMPLES`] of passes over `canvas`.
pub(crate) fn bakes(body: &[u8], canvas: (u32, u32)) -> bool {
    use crate::path_geometry::{MAX_PATH_ANCHORS, MAX_SUBPATHS};
    crate::psd_path_records::decode_vmsk(body, canvas).is_ok_and(|(decoded, flags)| {
        let subpaths = decoded.shape.subpaths.len();
        let passes = (subpaths as u64)
            .saturating_mul(u64::from(canvas.0))
            .saturating_mul(u64::from(canvas.1));
        flags & 4 == 0
            && subpaths <= MAX_SUBPATHS
            && decoded.shape.anchor_count() <= MAX_PATH_ANCHORS
            && passes <= MAX_BAKE_SAMPLES
            && decoded
                .operations
                .iter()
                .all(|&word| crate::vector_mask_combine::SubpathOp::from_word(word).is_some())
    })
}

/// The refusal of a layered open whose vector mask on layer `name` the
/// rasterizer does not draw: the report said the mask is drawn, so the
/// layer does not open unmasked instead.
fn undrawn(name: &str, reason: &str) -> String {
    format!("PSD vector mask on layer “{name}” could not be drawn: {reason}")
}

/// [`bakes`]: the seam a black-box test reads the drawing caps through.
#[doc(hidden)]
pub fn vmsk_bakes(body: &[u8], canvas: (u32, u32)) -> bool {
    bakes(body, canvas)
}

/// The coverage over the whole `canvas` of a `vmsk` body Doctored does not
/// hold as a live vector mask, under its per-subpath operations, initial
/// fill and inverted flag (`vector_mask_combine`), for an import that keeps
/// the picture as a raster mask. `None` for a body that does not decode,
/// names an unknown operation or is disabled (flag bit 2: it masks
/// nothing); `Some(Err)` is the rasterizer's refusal or a cancellation.
pub(crate) fn vmsk_coverage(
    body: &[u8],
    canvas: (u32, u32),
    job: crate::job_control::JobContext<'_>,
) -> Option<Result<Vec<u16>, String>> {
    use crate::vector_mask_combine::{apply, finish, start, SubpathOp};
    let (decoded, flags) = crate::psd_path_records::decode_vmsk(body, canvas).ok()?;
    if flags & 4 != 0 {
        return None;
    }
    let operations: Vec<SubpathOp> = decoded
        .operations
        .iter()
        .map(|&word| SubpathOp::from_word(word))
        .collect::<Option<_>>()?;
    let (w, h) = canvas;
    // A first subpath that subtracts starts from all pixels (`vector_mask_combine`).
    let full = decoded.inverted || operations.first() == Some(&SubpathOp::Subtract);
    let mut acc = start(w as usize * h as usize, full);
    for (subpath, op) in decoded.shape.subpaths.into_iter().zip(operations) {
        let one = PathShape {
            subpaths: vec![subpath],
            fill_rule: crate::path_geometry::FillRule::NonZero,
        };
        if one.subpaths[0].anchors.len() < 2 {
            continue;
        }
        match one.coverage((0, 0), w, h, true, job) {
            Ok(coverage) => apply(&mut acc, op, &coverage),
            Err(reason) => return Some(Err(reason)),
        }
    }
    finish(&mut acc, flags & 1 != 0);
    Some(Ok(acc))
}

/// The record a vector mask imported from `body` installs on a `canvas`
/// already holding `held` path anchors, as `(shape, enabled, linked)`. The
/// walk carried `body` only when [`vmsk_shape`] holds it within the cap
/// ([`MaskRoom`]), so `None` here is a guard that leaves the layer unmasked.
fn imported_shape(body: &[u8], canvas: (u32, u32), held: usize) -> Option<(PathShape, bool, bool)> {
    let (shape, flags) = vmsk_shape(body, canvas)?;
    if crate::vector_mask_common::anchor_refusal(held, &shape).is_some() {
        return None;
    }
    Some((shape, flags & 4 == 0, flags & 2 == 0))
}

impl crate::doc::DrDocument {
    /// Entry `idx` with the vector mask of `body` installed, its flags the
    /// body's; the document unchanged when the body cannot be one. Installed
    /// past the layer's locks, which describe the file's layer, not an edit.
    pub(crate) fn importing_vector_mask(self, idx: usize, body: &[u8]) -> Self {
        let held = self.vector_mask_anchor_count()
            + self
                .paths
                .entries
                .iter()
                .map(|e| e.shape.anchor_count())
                .sum::<usize>();
        let Some(layer) = self.layers.get(idx) else {
            return self;
        };
        let Some((shape, enabled, linked)) = imported_shape(body, (self.width, self.height), held)
        else {
            return self;
        };
        // The user's raster mask the file keeps beside the path (its `-3`
        // channel, `psd_raster_masks`), already attached, is the raster part.
        let record = crate::doc_vector_mask::VectorMaskRecord {
            layer_id: layer.id,
            shape,
            enabled,
            linked,
            raster: layer
                .mask
                .as_ref()
                .map(|plane| crate::doc_vector_mask::RasterPart {
                    plane: std::sync::Arc::clone(plane),
                    properties: layer.mask_properties.clone(),
                    enabled: layer.mask_enabled,
                }),
            baked: None,
            baked_offset: (0, 0),
        };
        self.installing_vector_mask(idx, record, crate::job_control::JobContext::NONE)
            .unwrap_or(self)
    }
}

impl crate::doc::DrDocument {
    /// Entry `idx` with the raster mask a `vmsk` Doctored cannot hold as a
    /// vector mask draws ([`vmsk_coverage`]) over the canvas, independent
    /// (unlinked) and enabled; the document unchanged when the body does not
    /// bake. Installed past the layer's locks, as the vector masks are.
    /// `Err` is a cancellation, or the rasterizer's refusal by the layer's
    /// name (the open is refused, never left unmasked).
    pub(crate) fn importing_baked_vector_mask(
        mut self,
        idx: usize,
        body: &[u8],
        job: crate::job_control::JobContext<'_>,
    ) -> Result<Self, String> {
        let (w, h) = (self.width, self.height);
        let coverage = match vmsk_coverage(body, (w, h), job) {
            None => return Ok(self),
            Some(Ok(coverage)) => coverage,
            Some(Err(reason)) => {
                job.check()?;
                let name = self.layers.get(idx).map_or("", |layer| layer.name.as_str());
                return Err(undrawn(name, &reason));
            }
        };
        // A u16 sample as a byte, rounded to nearest (`doc_vector_mask`'s).
        let bytes: Vec<u8> = coverage
            .iter()
            .map(|&c| ((u32::from(c) * 255 + 32767) / 65535) as u8)
            .collect();
        let (Some(plane), Some(layer)) = (
            image::GrayImage::from_raw(w, h, bytes),
            self.layers.get_mut(idx),
        ) else {
            return Ok(self);
        };
        let mut properties = crate::doc_mask::MaskProperties::at((0, 0));
        properties.linked = false;
        layer.mask = Some(std::sync::Arc::new(plane));
        layer.mask_enabled = true;
        layer.mask_properties = properties;
        Ok(self)
    }
}

impl crate::precision_document::PrecisionDocument {
    /// The typed twin of `DrDocument::importing_baked_vector_mask`: a
    /// canvas-sized u16 mask, its planes admitted against `limit` / `other`
    /// (a refused admission refuses the open, as every other import charge
    /// does). `Err` is also a cancellation, or the rasterizer's refusal by
    /// the layer's name.
    pub(crate) fn importing_baked_vector_mask(
        self,
        index: usize,
        body: &[u8],
        limit: u64,
        other: u64,
        job: crate::job_control::JobContext<'_>,
    ) -> Result<Self, String> {
        let (w, h) = self.dimensions();
        // One subpath's coverage (the rasterizer's charge), then the
        // accumulator and the mask the layer keeps, two u16 planes.
        let scratch = crate::path_geometry::coverage_scratch(w, h)
            .saturating_add(4 * u64::from(w) * u64::from(h));
        crate::precision_image::admit(self.retained_bytes(), scratch, other, limit)?;
        let coverage = match vmsk_coverage(body, (w, h), job) {
            None => return Ok(self),
            Some(Ok(coverage)) => coverage,
            Some(Err(reason)) => {
                job.check()?;
                let name = self.layer(index).map_or("", |layer| layer.name());
                return Err(undrawn(name, &reason));
            }
        };
        let Some(mut layer) = self.layer(index).cloned() else {
            return Ok(self);
        };
        let mut properties = crate::doc_mask::MaskProperties::at((0, 0));
        properties.linked = false;
        layer.mask = Some(crate::precision_mask::PrecisionMask::new(
            crate::precision_mask::PrecisionPlane::adopt(w, h, coverage)?,
            properties,
            true,
        )?);
        Ok(self.replacing_layer(index, layer)?.unwrap_or(self))
    }

    /// The typed twin of `DrDocument::importing_vector_mask`, its bake
    /// admitted against `limit` / `other` (a refused admission refuses the
    /// open, as every other import charge does).
    pub(crate) fn importing_vector_mask(
        self,
        index: usize,
        body: &[u8],
        limit: u64,
        other: u64,
    ) -> Result<Self, String> {
        let held = self.vector_mask_anchor_count()
            + self
                .paths()
                .iter()
                .map(|e| e.shape.anchor_count())
                .sum::<usize>();
        let (Some(layer_id), Some(layer)) = (self.layer_id(index), self.layer(index)) else {
            return Ok(self);
        };
        let Some((shape, enabled, linked)) = imported_shape(body, self.dimensions(), held) else {
            return Ok(self);
        };
        let record = crate::precision_vector_mask::TypedVectorMaskRecord {
            layer_id,
            shape,
            enabled,
            linked,
            // The `-3` user mask the import attached (`DrDocument`'s twin).
            raster: layer.mask.clone(),
            baked: None,
            baked_offset: (0, 0),
        };
        let job = crate::job_control::JobContext::NONE;
        self.admit_bake(index, &record, limit, other, job)?;
        self.installing_vector_mask(index, record, job)
    }
}

/// One record's vector mask as the writers hand it over: a live record with
/// no raster part, as `(shape, enabled, linked)`.
pub(crate) type WrittenMask = (PathShape, bool, bool);

/// Writes each `masks[r]` as record `r`'s `vmsk` in place of the baked mask
/// the walk wrote (its `-2` channel and mask section) — only where the walk
/// wrote one, so a mask a style bake swallowed stays inside the bake.
pub(crate) fn stamp_vector_masks(
    records: &mut [crate::psd_export_encode::Record],
    masks: &[Option<WrittenMask>],
    canvas: (u32, u32),
) {
    for (record, mask) in records.iter_mut().zip(masks) {
        let Some((shape, enabled, linked)) = mask else {
            continue;
        };
        if record.mask.is_none() {
            continue;
        }
        let Some(body) = vmsk_body(shape, *enabled, *linked, canvas) else {
            continue;
        };
        record.channels.retain(|(id, _)| *id != -2);
        record.mask = None;
        record.blocks.push((*b"vmsk", body));
    }
}

/// A layer's `vmsk` body for a live vector mask with no raster part, or
/// `None` when it cannot be written as one (module doc).
pub(crate) fn vmsk_body(
    shape: &PathShape,
    enabled: bool,
    linked: bool,
    canvas: (u32, u32),
) -> Option<Vec<u8>> {
    let flags = u32::from(!linked) << 1 | u32::from(!enabled) << 2;
    crate::psd_path_records::encode_vmsk(shape, canvas, flags).ok()
}
