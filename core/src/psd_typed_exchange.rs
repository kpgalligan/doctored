//! The typed PSD codecs' one seam for what a layered PSD, PSB or layered
//! TIFF carries beside its pixels in every typed mode — RGB at 16 bits,
//! Grayscale (and Duotone, which rides it) at 8, 16 and 32 bits, Lab at 8
//! and 16, CMYK at 8 and 16 and float RGB at 32: the Background mark
//! (`psd_background`), the layer comps (`psd_comps`) and raster-less vector
//! masks (`psd_paths`). Every typed importer and writer calls it, so the
//! rules stay in those three modules and nowhere else.
//!
//! **Import** ([`install`]) runs on the FINISHED layered document of each
//! open — after the mode's own `with_gray` / `with_lab` / warnings — and
//! never on a stored-composite open: file record 0's mark when the open kept
//! that record (the importer's own predicate, since Gray, Lab and 32-bit
//! float drop records the RGB walk keeps), the vector mask each carried
//! `vmsk` describes on the layer built from its record, then the comps bound
//! to those layers. `sources[k]` is the record the k-th built layer came
//! from, in the order the tree was emitted.
//!
//! **Export** ([`stamp`], [`resources`]): on the layered mode's record list,
//! before any mode reshapes it into Photoshop's own form, every record's
//! `lyid`/`shmd`, each raster-less vector mask's `vmsk` in place of its baked
//! mask, and record 0 as Photoshop's Background record when it is one; then
//! the path resources and, layered, resource 1065. `written` is the writer's
//! own "this layer's style writes its `knko`" answer, which decides whether a
//! knockout reaches the bottom (`psd_background`).

use crate::precision_document::{PrecisionDocument, PrecisionDocumentLayer};
use crate::psd_background::BottomExport;
use crate::psd_comps::CompSource;
use crate::psd_container::Container;
use crate::psd_export_encode::{Record, ResourceBlock};
use crate::psd_group_records::GroupRecords;

/// `document` with file record 0's mark, the carried vector masks and the
/// comps installed (module doc). `kept(index)` is the importer's "record
/// `index` of `records.layers` was built" answer. A refused vector-mask
/// admission refuses the open, as every other import charge does.
pub(crate) fn install(
    mut document: PrecisionDocument,
    container: &Container<'_>,
    records: &GroupRecords<'_>,
    sources: &[CompSource],
    kept: impl Fn(usize) -> bool,
    limit: u64,
    other: u64,
) -> Result<PrecisionDocument, String> {
    // FILE record 0, when kept, is root entry 0 (`psd_background`).
    if let Some(role) = crate::psd_background::import_role(records, kept) {
        document.mark_bottom(role);
    }
    // The vector masks the walk carried (`psd_paths`), each on the layer
    // built from its file record.
    for (k, &source) in sources.iter().enumerate() {
        if let Some(body) = crate::psd_paths::source_vmsk(records, source) {
            document = document.importing_vector_mask(k, body, limit, other)?;
        }
    }
    document.comps = crate::psd_comps::imported(container, records, sources, |k| {
        Some((document.layer_id(k)?, document.layer(k)?.name().to_string()))
    });
    Ok(document)
}

/// Adds the comps' `lyid`/`shmd`, the raster-less vector masks' `vmsk` and
/// the Background record to `records`, the layered mode's walk of
/// `document` in FILE order (module doc).
pub(crate) fn stamp(
    document: &PrecisionDocument,
    records: &mut [Record],
    written: &dyn Fn(&PrecisionDocumentLayer) -> bool,
) {
    let mut owners = Vec::new();
    crate::psd_comps::typed_owners(document.root_layers(), &mut owners);
    crate::psd_comps::stamp_records(records, &owners, document.layer_comps());
    // A live vector mask with no raster part is written as its `vmsk`
    // (`psd_paths`), in place of the baked mask.
    let masks: Vec<Option<crate::psd_paths::WrittenMask>> = owners
        .iter()
        .map(|owner| {
            let (id, _) = (*owner)?;
            let index = document.layer_index_by_id(id)?;
            document
                .live_vector_mask(index)
                .filter(|record| record.raster.is_none())
                .map(|record| (record.shape, record.enabled, record.linked))
        })
        .collect();
    crate::psd_paths::stamp_vector_masks(records, &masks, document.dimensions());
    // Root entry 0 as Photoshop's Background record, when it is one: FILE
    // record 0 (`psd_background`).
    if document.bottom_export(written) == BottomExport::Background {
        if let (Some(record), Some(layer)) = (records.first_mut(), document.root_layers().first()) {
            crate::psd_background::shape_record(record, layer.locks());
        }
    }
}

/// The image resources beside the records: the paths' (`psd_paths`) in
/// both modes, and resource 1065 for the comps the `layered` mode writes
/// (`psd_comps`).
pub(crate) fn resources(document: &PrecisionDocument, layered: bool) -> Vec<ResourceBlock> {
    let mut out = crate::psd_paths::resources(document.paths(), document.dimensions());
    if layered {
        out.extend(crate::psd_comps::resource(document.layer_comps()));
    }
    out
}

/// The ids of the layers whose live vector mask the layered mode writes as
/// pixels, for the preflight's sentence: one with a raster part, or one a
/// PSD path cannot hold (`psd_paths::vmsk_body`).
pub(crate) fn baked_vector_masks(document: &PrecisionDocument) -> Vec<u64> {
    document
        .live_vector_masks()
        .into_iter()
        .filter(|(_, record)| {
            record.raster.is_some()
                || crate::psd_paths::vmsk_body(
                    &record.shape,
                    record.enabled,
                    record.linked,
                    document.dimensions(),
                )
                .is_none()
        })
        .map(|(_, record)| record.layer_id)
        .collect()
}
