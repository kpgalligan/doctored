//! Byte fallback documents retain original assets at their genuine sample depth.
use super::{validate_binding, Binding, PendingSources, SourceTable};
use crate::doc::{DrDocument, LayerKind};
use crate::icc::IccProfile;
use crate::job_control::JobContext;
use crate::retained_source::RetainedSource8;
use std::sync::Arc;

pub(crate) fn byte_sources(doc: &DrDocument) -> Result<SourceTable, String> {
    let mut table = SourceTable::new(8);
    for (index, layer) in doc.layers.iter().enumerate() {
        if let Some(source) = layer.effective_source(&doc.profile) {
            if layer.kind != LayerKind::Raster || layer.meta.is_some() {
                return Err(
                    "embedded source requires an ordinary raster fallback without layer metadata"
                        .into(),
                );
            }
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
impl PendingSources<'_> {
    pub fn preflight_byte(
        &self,
        layers: &[crate::doc::Layer],
        fallback_dimensions: &[(u32, u32)],
        fallback_pixels: u64,
    ) -> Result<u64, String> {
        self.validate_raw_descriptors()?;
        let mut bytes = fallback_pixels
            .checked_mul(4)
            .ok_or("native fallback byte overflow")?;
        self.inspect_pixels(&mut bytes, crate::drdc::MAX_DRDC_DECODED_BYTES)?;
        for binding in &self.bindings {
            let layer = &layers[binding.layer];
            if layer.kind != LayerKind::Raster || layer.meta.is_some() {
                return Err(
                    "embedded source requires an ordinary raster fallback without layer metadata"
                        .into(),
                );
            }
            validate_binding(
                binding,
                self.assets[binding.asset].dimensions,
                fallback_dimensions[binding.layer],
                layer.offset,
            )?;
        }
        Ok(bytes)
    }
    pub fn materialize_byte(
        self,
        layers: &mut [crate::doc::Layer],
        document_profile: &Arc<IccProfile>,
        profiles: Vec<Arc<IccProfile>>,
    ) -> Result<(), String> {
        let (assets, bindings) = self.decode_assets(profiles, JobContext::NONE)?;
        for binding in bindings {
            let layer = &mut layers[binding.layer];
            // A layer whose bytes are its 8-bit asset's IS that plane, as at
            // place time: adopt the asset's tiles under a fresh identity.
            if let Some(asset) = assets[binding.asset].rgba8() {
                if layer.pixels.same_bytes(asset) {
                    layer.pixels = asset.reidentified();
                }
            }
            layer.retained_source = Some(RetainedSource8 {
                asset: assets[binding.asset].clone(),
                transform: binding.transform,
                filter: binding.filter,
                rendered: layer.pixels.clone(),
                rendered_offset: layer.offset,
                rendered_profile: document_profile.clone(),
                link: binding.link,
            });
        }
        Ok(())
    }
}
