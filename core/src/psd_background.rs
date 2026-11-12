//! The PSD side of the Background mark (`doc_background`) in every mode the
//! PSD codecs carry layers in — RGB at 8 and 16 bits, and through
//! `psd_typed_exchange` Grayscale (Duotone included) at 8, 16 and 32, Lab,
//! CMYK and 32-bit float RGB: what layered import marks, and when the writer
//! stores entry 0 as Photoshop's Background record.
//!
//! **Import.** File record 0 is the document's entry 0 whenever it is an
//! ordinary layer record (divider 0) the layered import keeps — the
//! importer's own answer, since the Gray, Lab and 32-bit opens drop records
//! the RGB walk keeps: a group's closing divider precedes its contents, so
//! record 0 is never inside one.
//! Its mark is `Background` when the record is Photoshop's Background (a
//! rectangle and no channel id `-1`, `GroupRecords::bottom`) and `Ordinary`
//! otherwise, so a Deep knockout over that layer punches where the file's
//! author saw it punch. The stored-composite choice sets no mark.
//!
//! **Export.** Entry 0 is written as a Background record ([`BottomExport`])
//! when D5 treats it as the Background (`style_knockout::background_entry`,
//! the mark included), it can be one — an opaque, unmasked, undescribed
//! raster, RGB or CMYK — and either its mark says Background or it is
//! unmarked and a WRITTEN knockout reaches it (a baked style writes no
//! `knko`; each writer answers that for its own styles). The record
//! takes the authored form of `tests/fixtures/psd_photoshop/knockout/
//! source.psd`'s record 0: no channel `-1`, header flag bit 0, an `lnsr`
//! of `bgnd` and `lspf` with the Background's implicit `0x0d` OR'd in
//! (`psd_locks::background_body`). Every other document's records are the
//! prior writer's bytes. A qualifying bottom a written knockout reaches that
//! cannot be a Background record (a transparent pixel, a mask, a
//! description) is written as an ordinary layer with the preflight's
//! sentence, because Photoshop then punches through to transparency.

use crate::doc::{DrDocument, LayerKind};
use crate::doc_background::BackgroundRole;
use crate::precision_document::{Content, PrecisionDocument, PrecisionDocumentLayer};
use crate::psd_export_encode::Record;
use crate::psd_group_records::GroupRecords;
use crate::style_composite::Cut;

/// The mark layered import gives entry 0, or `None` when file record 0 is
/// not an ordinary layer record the import kept (module doc). `kept(index)`
/// is the importer's own "record `index` of `records.layers` was built".
pub(crate) fn import_role(
    records: &GroupRecords<'_>,
    kept: impl Fn(usize) -> bool,
) -> Option<BackgroundRole> {
    let bottom = records.bottom.as_ref()?;
    // The reversed walk pushes file record 0 LAST.
    let index = records.layers.len().checked_sub(1)?;
    if !kept(index) {
        return None;
    }
    Some(if bottom.background {
        BackgroundRole::Background
    } else {
        BackgroundRole::Ordinary
    })
}

/// What the writer does with entry 0 (module doc).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BottomExport {
    /// An ordinary record, with nothing to say.
    Ordinary,
    /// Photoshop's Background record.
    Background,
    /// An ordinary record that D5 treats as the Background while a written
    /// knockout reaches it: the preflight says so.
    Unwritable,
}

/// The one decision, at both depths. `qualifies` is D5 with the mark,
/// `role` the applicable mark, `reached` whether a WRITTEN knockout reaches
/// the bottom and `writable` whether entry 0 can be a Background record.
fn decide(
    qualifies: bool,
    role: Option<BackgroundRole>,
    reached: bool,
    writable: bool,
) -> BottomExport {
    if !qualifies || role == Some(BackgroundRole::Ordinary) {
        return BottomExport::Ordinary;
    }
    let wanted = role == Some(BackgroundRole::Background) || reached;
    match (wanted && writable, reached) {
        (true, _) => BottomExport::Background,
        (false, true) => BottomExport::Unwritable,
        (false, false) => BottomExport::Ordinary,
    }
}

/// Reshapes `record` (entry 0's, as the ordinary walk wrote it) into
/// Photoshop's Background record (module doc). `locks` are the layer's own.
pub(crate) fn shape_record(record: &mut Record, locks: u32) {
    record.channels.retain(|(id, _)| *id != -1);
    record.flags |= 0x01;
    let lspf = (*b"lspf", crate::psd_locks::background_body(locks).to_vec());
    match record.blocks.iter().position(|(key, _)| key == b"lspf") {
        Some(at) => record.blocks[at] = lspf,
        None => {
            // Photoshop's order: `lspf` immediately before `lclr`.
            let at = record
                .blocks
                .iter()
                .position(|(key, _)| key == b"lclr")
                .unwrap_or(record.blocks.len());
            record.blocks.insert(at, lspf);
        }
    }
    // Photoshop's name source, right after `luni`.
    record.blocks.insert(0, (*b"lnsr", b"bgnd".to_vec()));
}

impl DrDocument {
    /// The byte writer's [`BottomExport`] for entry 0.
    pub(crate) fn bottom_export(&self) -> BottomExport {
        let qualifies = crate::style_knockout::background_entry(
            &self.layers,
            (self.width, self.height),
            Cut::NONE,
            self.background,
        )
        .is_some();
        if !qualifies {
            return BottomExport::Ordinary;
        }
        let reached = crate::style_knockout::document_reaches_background(&self.layers, |layer| {
            crate::psd_export_rasterize::export_style(layer, false)
                .is_some_and(|style| style.bake.is_none())
        });
        let writable = self.layers.first().is_some_and(|layer| {
            layer.kind == LayerKind::Raster
                && layer.mask.is_none()
                && layer.meta.is_none()
                && layer
                    .pixels
                    .runs()
                    .all(|run| run.iter().all(|p| p[3] == 255))
        });
        decide(qualifies, self.background_role(), reached, writable)
    }
}

impl PrecisionDocument {
    /// A typed writer's [`BottomExport`] for root entry 0. `written` is that
    /// writer's "this layer's style writes its `knko`" (a baked one does not).
    pub(crate) fn bottom_export(
        &self,
        written: &dyn Fn(&PrecisionDocumentLayer) -> bool,
    ) -> BottomExport {
        let roots = self.root_layers();
        let qualifies = crate::precision_style_knockout::background_record(
            roots,
            self.dimensions(),
            Cut::NONE,
            self.float.is_some(),
            self.background,
        );
        if !qualifies {
            return BottomExport::Ordinary;
        }
        let reached = crate::precision_style_knockout::document_reaches_background16(
            roots,
            0,
            Cut::NONE,
            written,
        );
        let writable = roots.first().is_some_and(|layer| {
            layer.mask().is_none()
                && layer.description.is_none()
                && match &layer.content {
                    Content::Raster { image, .. } => crate::doc_crop_extend::opaque(image),
                    Content::CmykRaster { image, .. } => crate::doc_crop_extend::opaque_cmyk(image),
                    _ => false,
                }
        });
        decide(qualifies, self.background_role(), reached, writable)
    }
}
