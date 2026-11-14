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
/// admission refuses the open, as every other import charge does, and so
/// does a vector mask the import draws under `job` and the rasterizer
/// refuses (`psd_paths`).
#[allow(clippy::too_many_arguments)]
pub(crate) fn install(
    mut document: PrecisionDocument,
    container: &Container<'_>,
    records: &GroupRecords<'_>,
    sources: &[CompSource],
    kept: impl Fn(usize) -> bool,
    limit: u64,
    other: u64,
    job: crate::job_control::JobContext<'_>,
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
        // A vector mask the file holds no rendering of, baked (`psd_paths`).
        if let Some(body) = crate::psd_paths::source_bake(records, source) {
            document = document.importing_baked_vector_mask(k, body, limit, other, job)?;
        }
    }
    document.comps = crate::psd_comps::imported(container, records, sources, |k| {
        let layer = document.layer(k)?;
        Some(crate::psd_comps::BuiltLayer {
            id: document.layer_id(k)?,
            name: layer.name().to_string(),
            style: layer.reported_style(),
        })
    });
    Ok(document)
}

/// The layered walk's owners in FILE order (`psd_comps::typed_owners`). A
/// record's style is rasterized into its pixels when the layer has one and
/// `written`, the writer's "this style is written, not baked", says no; and
/// an adjustment layer is written as its stamp when its style is baked, it
/// has no PSD record, or the document is Lab (`psd_export16::records`; a
/// CMYK document's adjustment layers are `CmykAdjustment`s, never stamped).
fn owners(
    document: &PrecisionDocument,
    written: &dyn Fn(&PrecisionDocumentLayer) -> bool,
) -> Vec<Option<crate::psd_comps::Owner>> {
    // Only a document with a comp to write reads a style or a decision.
    let comps = !document.layer_comps().is_empty();
    let mut owners = Vec::new();
    crate::psd_comps::typed_owners(
        document.root_layers(),
        &|layer| {
            let baked =
                comps && crate::psd_export16::merged_style(layer).is_some() && !written(layer);
            let stamped = match &layer.content {
                crate::precision_document::Content::Adjustment(state) if comps => {
                    baked
                        || document.is_lab()
                        || crate::psd_adjustment::encode(state.description()).is_none()
                }
                _ => false,
            };
            (baked, stamped)
        },
        comps,
        &mut owners,
    );
    owners
}

/// The comps' recorded styles whose patterns the layered file's pattern
/// table holds beside the layers' own (`psd_comp_limits::recorded_styles`).
pub(crate) fn comp_styles(
    document: &PrecisionDocument,
    written: &dyn Fn(&PrecisionDocumentLayer) -> bool,
) -> Vec<crate::style::LayerStyle> {
    crate::psd_comp_limits::recorded_styles(document.layer_comps(), &owners(document, written))
}

/// What the layered writer does not write of the comps' recorded styles
/// (`psd_comp_limits::narrowed`); `refuses` is its effects codec's refusal.
pub(crate) fn comps_narrowed(
    document: &PrecisionDocument,
    written: &dyn Fn(&PrecisionDocumentLayer) -> bool,
    refuses: &dyn Fn(&crate::style::LayerStyle) -> bool,
) -> crate::psd_comp_limits::Narrowed {
    crate::psd_comp_limits::narrowed(document.layer_comps(), &owners(document, written), refuses)
}

/// Adds the comps' `lyid`/`shmd`, the raster-less vector masks' `vmsk` and
/// the Background record to `records`, the layered mode's walk of
/// `document` in FILE order (module doc). `styles` writes a comp's style
/// (`psd_comps::CompStyles`).
pub(crate) fn stamp(
    document: &PrecisionDocument,
    records: &mut [Record],
    written: &dyn Fn(&PrecisionDocumentLayer) -> bool,
    styles: crate::psd_comps::CompStyles<'_>,
) {
    let owners = owners(document, written);
    crate::psd_comps::stamp_records(records, &owners, document.layer_comps(), styles);
    // A live vector mask with no raster part is written as its `vmsk`
    // (`psd_paths`), in place of the baked mask.
    let masks: Vec<Option<crate::psd_paths::WrittenMask>> = owners
        .iter()
        .map(|owner| {
            let index = document.layer_index_by_id(owner.as_ref()?.id)?;
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
