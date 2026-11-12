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
//! **Vector masks.** A `vmsk` on a raster record with no raster mask is that
//! layer's vector mask (`set_vector_mask`), enabled and linked from its
//! flags, when its path is one Doctored holds exactly ([`vmsk_shape`]) and
//! fits the document's anchor cap beside the file's paths and the masks
//! before it ([`MaskRoom`]). Every other `vmsk` — beside a raster mask,
//! starting from all pixels, combining subpaths by subtracting or
//! intersecting, not decoding, past the cap, or over a fill the mode's open
//! does not import as a fill layer (every fill at Gray and Lab, a Gradient
//! or Pattern fill at 32 bits) — and a `vsms` keep the importer's prior
//! answer: the record's mask is dropped with its sentence, and a shape layer
//! keeps the pixels Photoshop stored, already masked. The decision is the
//! record walk's (`psd_group_records::vector_mask_usable`), so the report and
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
        tail: tail.to_vec(),
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
        let record = crate::doc_vector_mask::VectorMaskRecord {
            layer_id: layer.id,
            shape,
            enabled,
            linked,
            raster: None,
            baked: None,
            baked_offset: (0, 0),
        };
        self.installing_vector_mask(idx, record, crate::job_control::JobContext::NONE)
            .unwrap_or(self)
    }
}

impl crate::precision_document::PrecisionDocument {
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
        let Some(layer_id) = self.layer_id(index) else {
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
            raster: None,
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
