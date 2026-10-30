//! Typed fallback documents retain byte and u16 source masters independently.
use super::{Binding, SourceTable};
use crate::precision_document::PrecisionDocument;

pub(crate) fn precision_sources(doc: &PrecisionDocument) -> Result<SourceTable, String> {
    let mut table = SourceTable::new(16);
    for index in 0..doc.records {
        let layer = doc
            .layer(index)
            .ok_or("invalid embedded source layer index")?;
        if let Some(source) = layer.effective_source() {
            table.push(
                source.asset,
                Binding {
                    layer: index,
                    asset: 0,
                    transform: source.transform,
                    filter: source.filter,
                    link: source.link,
                    dropped: false,
                },
            )?;
        }
    }
    Ok(table)
}
