//! Export an already captured authoritative u16 projection without rendering a
//! second copy. The caller pairs the immutable document/revision with its
//! projection; dimensions and exact profile bytes are checked here, but pixel
//! provenance cannot be inferred without repeating the render.
//!
//! The shared codec retains its existing conservative allocation bound. Its
//! additive packet accounting can exceed unique allocation accounting when
//! packets alias, so the plan explicitly carries that excess into host scratch.
//!
//! Storage, canvas dimensions and exact ICC bytes are required; the host preserves source/revision
//! pairing. Unique document/projection retained accounting reconciles additive codec packet
//! charges, including aliases, with the host plan. Codec estimates are unchanged; the save
//! takes an optional single-use job control that the shared writer polls as the encoded bytes
//! move (`precision_codec::save_with_paths`).

use crate::job_control::DrJobControl;
use crate::precision_codec::{self, PrecisionFormat};
use crate::precision_document::PrecisionDocument;
use crate::precision_image::{admit, PrecisionImage, PrecisionStorage, BOOKKEEPING_BYTES};
use crate::resources::Inventory;
use serde_json::{json, Value};
use std::sync::Arc;

#[derive(Clone, Copy, Debug)]
pub struct ProjectionExportPlan {
    pub retained_bytes: u64,
    pub host_scratch_bytes: u64,
    pub codec_scratch_bytes: u64,
    pub codec_additive_overhead_bytes: u64,
    pub width: u32,
    pub height: u32,
    codec_retained_bytes: u64,
}

impl ProjectionExportPlan {
    pub fn json(self) -> Value {
        json!({
            "retained_bytes": self.retained_bytes,
            "host_scratch_bytes": self.host_scratch_bytes,
            "codec_scratch_bytes": self.codec_scratch_bytes,
            "codec_additive_overhead_bytes": self.codec_additive_overhead_bytes,
            "width": self.width, "height": self.height,
            "storage": "rgba16_unorm",
        })
    }
}

impl PrecisionDocument {
    /// Admission for writing a captured projection. `retained_bytes` is the
    /// unique union of both inputs, so caller-retained bytes must exclude both.
    /// This query allocates only bounded bookkeeping, never a pixel plane.
    pub fn projection_export_plan(
        &self,
        image: &PrecisionImage,
    ) -> Result<ProjectionExportPlan, String> {
        if image.storage() != PrecisionStorage::Rgba16Unorm {
            return Err("high-depth projection export requires RGBA16 UNORM storage".into());
        }
        if image.dimensions() != self.dimensions() {
            return Err("high-depth projection dimensions do not match the document canvas".into());
        }
        if !Arc::ptr_eq(image.profile(), self.profile())
            && image.profile().bytes() != self.profile().bytes()
        {
            return Err("high-depth projection profile bytes do not match the document".into());
        }
        let mut packet_bytes = 0_u64;
        for packet in [
            &self.metadata().exif,
            &self.metadata().xmp,
            &self.metadata().iptc,
        ]
        .into_iter()
        .flatten()
        {
            if packet.len() > crate::drdc::MAX_DRDC_BLOB_LEN as usize {
                return Err("high-depth metadata packet exceeds the native packet limit".into());
            }
            packet_bytes = packet_bytes
                .checked_add(packet.len() as u64)
                .ok_or("precision projection export memory overflow")?;
        }
        let codec_retained_bytes = image
            .retained_bytes()
            .checked_add(packet_bytes)
            .ok_or("precision projection export memory overflow")?;
        let mut inventory = Inventory::default();
        self.record_resources(&mut inventory);
        image.record_resources(&mut inventory);
        let retained_bytes = inventory.unique_bytes();
        let codec_scratch_bytes = precision_codec::encode_scratch_bytes(image)?;
        let codec_additive_overhead_bytes = codec_retained_bytes.saturating_sub(retained_bytes);
        let host_scratch_bytes = codec_scratch_bytes
            .checked_add(codec_additive_overhead_bytes)
            .and_then(|v| v.checked_add(BOOKKEEPING_BYTES))
            .ok_or("precision projection export memory overflow")?;
        let (width, height) = self.dimensions();
        Ok(ProjectionExportPlan {
            retained_bytes,
            host_scratch_bytes,
            codec_scratch_bytes,
            codec_additive_overhead_bytes,
            width,
            height,
            codec_retained_bytes,
        })
    }

    /// Save the supplied authoritative projection with this captured document's
    /// metadata and resolution. No display image, render or cache is involved.
    /// Publication and packet reports use the shared writer, which begins the
    /// optional single-use `control` and polls it as the encoded bytes move; a
    /// cancel answers exactly `operation cancelled` and keeps the previous
    /// file.
    #[allow(clippy::too_many_arguments)]
    pub fn save_projection16(
        &self,
        image: &PrecisionImage,
        path: &str,
        format: PrecisionFormat,
        embed_profile: bool,
        strip_metadata: bool,
        limit: u64,
        other: u64,
        control: Option<&DrJobControl>,
    ) -> Result<u32, String> {
        let plan = self.projection_export_plan(image)?;
        admit(plan.retained_bytes, plan.host_scratch_bytes, other, limit)?;
        // Codec input accounting is C = image retained + each packet length.
        // Charge max(U,C), where U is the union, without ever subtracting C
        // from a smaller U. The codec total then equals the host plan total.
        let codec_other = other
            .checked_add(
                plan.retained_bytes
                    .saturating_sub(plan.codec_retained_bytes),
            )
            .and_then(|v| v.checked_add(BOOKKEEPING_BYTES))
            .ok_or("precision projection export memory overflow")?;
        // A TIFF carries the paths (`tiff_paths`).
        precision_codec::save_with_paths(
            image,
            self.metadata(),
            self.resolution(),
            path,
            format,
            embed_profile,
            strip_metadata,
            (limit, codec_other),
            control,
            self.tiff_path_run(),
        )
    }
}
