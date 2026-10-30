//! Genuine decoded source storage is independent of its document's fallback
//! depth. Content revisions are immutable; per-layer links are descriptive only
//! and never authorize filesystem access or an automatic refresh in the core.
//!
//! Since 13B-2c-2c `SourcePixels::Rgba8.pixels` is a tiled `LayerRaster` and `SourceAsset::rgba8()`
//! answers `Option<&LayerRaster>` (compare with `same_bytes` / `equals_image`, identity with
//! `LayerRaster::ptr_eq`). Layer bindings own optional path/fingerprint metadata; the pure core
//! never reads links.
//! `SourceAsset.profile_refusal` keeps the host-relayed core open's profile-refusal sentence
//! (1–4096 bytes, no NUL; `validate_profile_refusal`): only `place`/`replace` accept the key
//! (absent clears on replace; capture/retain pass `None`), it lives on the ASSET so every op
//! reusing the `Arc` keeps it, and `info` always reports `profile_refusal`. See
//! `designs/phase-8-retained-source-status.md`.
//!
//! A master may be 32-bit float (`SourcePixels::Rgba32F`, linear light in its
//! own profile): a float document's one raster becomes one. Its native
//! payload is word 67's raw f32 asset (`native_sources`). An 8-
//! or 16-bit document admits one too, under `convert_fallback`
//! (`DepthConversion::require`), rendered clipped to the document's range,
//! and a byte file stores it under word 72 (`float_native::BYTE_MASTER_VERSION`).
//!
//! Smart-object contents, Stack Mode and live filters were removed on
//! 2026-10-10 (`designs/convert-to-smart-object-status.md`): an asset is a
//! decoded master and nothing more.
use crate::doc::{BlendMode, DrDocument, LayerKind};
use crate::icc::IccProfile;
use crate::layer_raster::LayerRaster;
use crate::precision_document::PrecisionDocument;
use crate::precision_image::PrecisionImage;
use crate::resources::Inventory;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Clone)]
pub enum SourcePixels {
    Rgba8 {
        pixels: LayerRaster,
        profile: Arc<IccProfile>,
    },
    Rgba16(PrecisionImage),
    /// `rgba32_float`, linear light in its own profile.
    /// Boxed so `SourcePixels` keeps its size: two unboxed `PrecisionImage`
    /// variants cannot share its niche, and the 8/16-bit plans count the
    /// asset's size (`Inventory::object`), which must not change.
    Rgba32F(Box<PrecisionImage>),
}
#[derive(Clone)]
pub struct SourceAsset {
    pub id: String,
    pub revision_id: String,
    pub pixels: SourcePixels,
    pub raw: Option<Arc<crate::raw_source::RawSource>>,
    pub attachment: Option<Arc<crate::source_attachment::SourceAttachment>>,
    /// The host-relayed sentence the core's open reported for this file's
    /// colour profile, kept verbatim. It belongs to the immutable
    /// revision, so every op that reuses the asset keeps it and a Replace
    /// sets it from the new file.
    pub profile_refusal: Option<String>,
}
/// The cap on [`SourceAsset::profile_refusal`]. Core open sentences stay
/// under 1 KiB because ICC names are capped at 128 characters; the native
/// `RSPR` reader enforces the same cap.
pub const MAX_PROFILE_REFUSAL_BYTES: usize = 4096;

pub(crate) fn validate_profile_refusal(text: &str) -> Result<(), String> {
    if text.is_empty() || text.len() > MAX_PROFILE_REFUSAL_BYTES || text.contains('\0') {
        return Err("source profile refusal requires 1–4096 bytes of UTF-8 without NUL".into());
    }
    Ok(())
}
impl SourceAsset {
    pub fn dimensions(&self) -> (u32, u32) {
        match &self.pixels {
            SourcePixels::Rgba8 { pixels, .. } => pixels.dimensions(),
            SourcePixels::Rgba16(image) => image.dimensions(),
            SourcePixels::Rgba32F(image) => image.dimensions(),
        }
    }
    pub fn profile(&self) -> &Arc<IccProfile> {
        match &self.pixels {
            SourcePixels::Rgba8 { profile, .. } => profile,
            SourcePixels::Rgba16(image) => image.profile(),
            SourcePixels::Rgba32F(image) => image.profile(),
        }
    }
    pub fn depth(&self) -> u32 {
        match self.pixels {
            SourcePixels::Rgba8 { .. } => 8,
            SourcePixels::Rgba16(_) => 16,
            SourcePixels::Rgba32F(_) => 32,
        }
    }
    pub fn rgba8(&self) -> Option<&LayerRaster> {
        match &self.pixels {
            SourcePixels::Rgba8 { pixels, .. } => Some(pixels),
            _ => None,
        }
    }
    pub fn rgba16(&self) -> Option<&PrecisionImage> {
        match &self.pixels {
            SourcePixels::Rgba16(image) => Some(image),
            _ => None,
        }
    }
    /// The 32-bit float master, `None` for an 8/16-bit one.
    pub fn rgba32(&self) -> Option<&PrecisionImage> {
        match &self.pixels {
            SourcePixels::Rgba32F(image) => Some(image),
            _ => None,
        }
    }
    pub(crate) fn record_resources(&self, inventory: &mut Inventory) {
        inventory.object(self, "embedded_source_structure", false);
        inventory.string(&self.id, "embedded_source_identity", false);
        inventory.string(&self.revision_id, "embedded_source_identity", false);
        if let Some(text) = &self.profile_refusal {
            inventory.string(text, "embedded_source_identity", false);
        }
        if let Some(raw) = &self.raw {
            raw.record_resources(inventory);
        }
        if let Some(attachment) = &self.attachment {
            attachment.record_resources(inventory);
        }
        match &self.pixels {
            SourcePixels::Rgba8 { pixels, profile } => {
                pixels.record_resources(inventory, "embedded_source_pixels", false);
                inventory.object(&**profile, "profile", false);
                profile.record_resources(inventory);
            }
            SourcePixels::Rgba16(image) => image.record_resources(inventory),
            SourcePixels::Rgba32F(image) => image.record_resources(inventory),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SourceLink {
    pub path: String,
    pub sha256: String,
}
impl SourceLink {
    pub fn validate_parts(path: &str, sha256: &str) -> Result<(), String> {
        if !path.starts_with('/') || path.len() > 16 * 1024 || path.contains('\0') {
            return Err(
                "source link requires an absolute path of at most 16 KiB without NUL".into(),
            );
        }
        if sha256.len() != 64
            || !sha256
                .bytes()
                .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
        {
            return Err(
                "source fingerprint requires exactly 64 lowercase hexadecimal SHA256 characters"
                    .into(),
            );
        }
        Ok(())
    }
    pub fn validate(&self) -> Result<(), String> {
        Self::validate_parts(&self.path, &self.sha256)
    }
    pub(crate) fn record_resources(&self, inventory: &mut Inventory) {
        inventory.object(self, "source_link", false);
        inventory.string(&self.path, "source_link", false);
        inventory.string(&self.sha256, "source_link", false);
    }
}

/// Borrowed already-decoded ordinary input. Both C owners remain alive until the
/// plan/edit returns; creating an asset shares their genuine samples and ICC.
#[derive(Clone, Copy)]
pub enum SourceInput<'a> {
    Byte(&'a DrDocument),
    Typed(&'a PrecisionDocument),
}
impl<'a> SourceInput<'a> {
    pub fn from_handles(
        byte: Option<&'a DrDocument>,
        typed: Option<&'a PrecisionDocument>,
    ) -> Result<Option<Self>, String> {
        match (byte, typed) {
            (Some(_), Some(_)) => Err("supply exactly one source document depth".into()),
            (Some(d), None) => Ok(Some(Self::Byte(d))),
            (None, Some(d)) => Ok(Some(Self::Typed(d))),
            (None, None) => Ok(None),
        }
    }
    pub(crate) fn record_resources(self, inventory: &mut Inventory) {
        match self {
            Self::Byte(d) => inventory.document(d),
            Self::Typed(d) => d.record_resources(inventory),
        }
    }
    pub(crate) fn asset(
        self,
        id: &str,
        revision_id: &str,
        profile_refusal: Option<&str>,
    ) -> Result<Arc<SourceAsset>, String> {
        crate::retained_source::validate_id(id)?;
        crate::retained_source::validate_id(revision_id)?;
        if let Some(text) = profile_refusal {
            validate_profile_refusal(text)?;
        }
        let pixels = match self {
            Self::Byte(d) => {
                let l = d.layers.first().ok_or("source document has no raster")?;
                if d.layers.len() != 1
                    || l.kind != LayerKind::Raster
                    || l.mask.is_some()
                    || l.meta.is_some()
                    || l.style.is_some()
                    || l.opacity != 1.
                    || l.clipped
                    || !l.visible
                    || l.blend != BlendMode::Normal
                    || l.effective_source(&d.profile).is_some()
                {
                    return Err("source input requires one decoded ordinary raster with its original profile".into());
                }
                SourcePixels::Rgba8 {
                    pixels: l.pixels.clone(),
                    profile: d.profile.clone(),
                }
            }
            Self::Typed(d) => {
                let l = d.layer(0).ok_or("source document has no raster")?;
                if d.layer_count() != 1
                    || l.description.is_some()
                    || l.mask().is_some()
                    || l.opacity() != 1.
                    || !l.visible()
                    || l.blend_if().is_some()
                    || l.style().is_some()
                    || l.effective_source().is_some()
                {
                    return Err("source input requires one decoded ordinary raster with its original profile".into());
                }
                let image = l
                    .source()
                    .ok_or("source input requires an ordinary raster")?
                    .clone();
                // A float document's raster is an f32 master.
                if d.is_float() {
                    SourcePixels::Rgba32F(Box::new(image))
                } else {
                    SourcePixels::Rgba16(image)
                }
            }
        };
        Ok(Arc::new(SourceAsset {
            id: id.into(),
            revision_id: revision_id.into(),
            pixels,
            raw: None,
            attachment: None,
            profile_refusal: profile_refusal.map(Into::into),
        }))
    }
}
