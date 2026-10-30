//! Embedded ordinary images retain their genuine decoded samples and original
//! ICC profile. The rendered image is a fallback, never the master. A fallback
//! allocation/profile/description guard invalidates the binding after edits;
//! integer placement changes are folded into the accumulated source transform.
//! Evaluation order is source ICC conversion, accumulated affine sampling,
//! independent layer mask, then the established style/blend/opacity compositor.
//!
//! Explicit cross-depth evaluation retains original precision and quantizes only the fallback.
//! Replacement preserves layer properties; rasterization preserves fallback pixels. Guards prevent
//! source resurrection after destructive edits. A mixed resource inventory counts shared masters
//! across both handle families once.
use std::sync::Arc;

use serde::Deserialize;
use serde_json::{json, Value};

use crate::doc::{Layer, LayerKind, LayerRaster};
use crate::doc_transform::{transformed_plane_bounds, Affine};
use crate::icc::IccProfile;
use crate::precision_document::PrecisionDocumentLayer;
use crate::precision_image::PrecisionImage;
use crate::precision_sampling::GeometryFilter;
pub use crate::source_asset::{SourceAsset, SourceInput, SourceLink, SourcePixels};
use crate::source_transform::SourceTransform;
#[derive(Clone)]
pub struct RetainedSource8 {
    pub asset: Arc<SourceAsset>,
    pub link: Option<Arc<SourceLink>>,
    pub transform: SourceTransform,
    pub filter: GeometryFilter,
    pub rendered: LayerRaster,
    pub rendered_offset: (i32, i32),
    pub rendered_profile: Arc<IccProfile>,
}
#[derive(Clone)]
pub struct RetainedSource16 {
    pub asset: Arc<SourceAsset>,
    pub link: Option<Arc<SourceLink>>,
    pub transform: SourceTransform,
    pub filter: GeometryFilter,
    pub rendered: PrecisionImage,
    pub rendered_offset: (i32, i32),
}

/// Strict, bounded requests shared by byte/typed document owners.
#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum SourceEdit {
    Retain {
        source_id: String,
        #[serde(default)]
        revision_id: Option<String>,
    },
    Place {
        source_id: String,
        #[serde(default)]
        revision_id: Option<String>,
        #[serde(default)]
        link: Option<SourceLink>,
        #[serde(default)]
        depth_conversion: DepthConversion,
        name: String,
        x: i32,
        y: i32,
        #[serde(default)]
        source_transform: Option<[f64; 6]>,
        /// The placement ANCHOR and SLOT (`layer_insert`). Both absent is
        /// today's behaviour byte-for-byte — the placed layer is PUSHED onto
        /// the root level — and both present routes through the one slot
        /// rule. One without the other is refused by `parse`, because an
        /// anchor with no position would have to invent a default the caller
        /// never named.
        #[serde(default)]
        anchor: Option<usize>,
        #[serde(default)]
        position: Option<crate::layer_insert::InsertPosition>,
        /// The core open's profile-refusal sentence for the placed
        /// file, relayed verbatim by the host and kept on the asset.
        #[serde(default)]
        profile_refusal: Option<String>,
    },
    Transform {
        layer: usize,
        matrix: [f64; 6],
        filter: GeometryFilter,
        /// The 16-bit ⌘T PREVIEW of a placed image (forwarded from its
        /// `affine`) keeps `MAX_WORK_UNITS`; a commit omits it and takes its
        /// caller's cap (none under a job control, `MAX_DESTRUCTIVE_WORK`
        /// without one). Honoured at both depths.
        #[serde(default)]
        interactive: bool,
    },
    CaptureRaster {
        layer: usize,
        source_id: String,
        #[serde(default)]
        revision_id: Option<String>,
        source_transform: [f64; 6],
        #[serde(default)]
        depth_conversion: DepthConversion,
        #[serde(default)]
        link: Option<SourceLink>,
    },
    Rasterize {
        layer: usize,
    },
    Replace {
        layer: usize,
        revision_id: String,
        #[serde(default)]
        link: Option<SourceLink>,
        #[serde(default)]
        depth_conversion: DepthConversion,
        #[serde(default)]
        source_transform: Option<[f64; 6]>,
        /// New master coordinates into the old master; precomposed with the
        /// existing affine or projective retained binding.
        #[serde(default)]
        source_coordinate_map: Option<[f64; 6]>,
        /// The new file's profile-refusal sentence; absent clears it.
        #[serde(default)]
        profile_refusal: Option<String>,
    },
}
#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DepthConversion {
    #[default]
    Reject,
    ConvertFallback,
}
impl DepthConversion {
    /// At `destination` 32 (a float document's fallback) every master is
    /// admitted without the authorization: a master is kept at its own
    /// precision, and the fallback is always a linear-light render, placed
    /// into a 32-bit document without asking (`retained_source_float`). Below
    /// 32 a 32-bit float master takes the same rule as any other master of a
    /// different depth: it needs `convert_fallback`.
    pub(crate) fn require(self, asset: &SourceAsset, destination: u32) -> Result<(), String> {
        if self == Self::Reject && destination != 32 && asset.depth() != destination {
            return Err("source and document depths differ; explicit convert_fallback authorization is required".into());
        }
        Ok(())
    }
}
impl SourceEdit {
    pub fn parse(text: &str) -> Result<Self, String> {
        if text.len() > 64 * 1024 {
            return Err("source request exceeds 64 KiB".into());
        }
        let edit: Self =
            serde_json::from_str(text).map_err(|e| format!("invalid source request: {e}"))?;
        match &edit {
            Self::Retain {
                source_id,
                revision_id,
            } => {
                validate_id(source_id)?;
                if let Some(revision) = revision_id {
                    validate_id(revision)?;
                }
            }
            Self::Place {
                source_id,
                revision_id,
                name,
                link,
                source_transform,
                anchor,
                position,
                profile_refusal,
                ..
            } => {
                validate_id(source_id)?;
                if let Some(text) = profile_refusal {
                    crate::source_asset::validate_profile_refusal(text)?;
                }
                if let Some(revision) = revision_id {
                    validate_id(revision)?;
                }
                if let Some(link) = link {
                    link.validate()?;
                }
                if let Some(matrix) = source_transform {
                    validate_matrix(*matrix)?;
                }
                if anchor.is_some() != position.is_some() {
                    return Err(
                        "placement position requires both an anchor layer and a position".into(),
                    );
                }
                crate::precision_document::scalar_and_name(name, 1.0)?;
            }
            Self::Transform { matrix, .. } => {
                validate_matrix(*matrix)?;
            }
            Self::CaptureRaster {
                source_id,
                revision_id,
                source_transform,
                link,
                ..
            } => {
                validate_id(source_id)?;
                if let Some(revision) = revision_id {
                    validate_id(revision)?;
                }
                validate_matrix(*source_transform)?;
                if let Some(link) = link {
                    link.validate()?;
                }
            }
            Self::Replace {
                revision_id,
                link,
                source_transform,
                source_coordinate_map,
                profile_refusal,
                ..
            } => {
                validate_id(revision_id)?;
                if let Some(text) = profile_refusal {
                    crate::source_asset::validate_profile_refusal(text)?;
                }
                if let Some(link) = link {
                    link.validate()?;
                }
                if let Some(matrix) = source_transform {
                    validate_matrix(*matrix)?;
                }
                if source_transform.is_some() && source_coordinate_map.is_some() {
                    return Err(
                        "source transform and source coordinate map are mutually exclusive".into(),
                    );
                }
                if let Some(matrix) = source_coordinate_map {
                    validate_matrix(*matrix)?;
                }
            }
            Self::Rasterize { .. } => {}
        }
        Ok(edit)
    }
}
pub(crate) fn validate_id(id: &str) -> Result<(), String> {
    if id.is_empty()
        || id.len() > 128
        || !id
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
    {
        return Err(
            "source identity must contain 1–128 ASCII letters, digits, hyphens or underscores"
                .into(),
        );
    }
    Ok(())
}
pub(crate) fn validate_matrix(matrix: [f64; 6]) -> Result<Affine, String> {
    let m = Affine::from_array(matrix);
    if !m.is_finite() || m.inverse().is_none() {
        return Err("source transform is nonfinite or singular".into());
    }
    Ok(m)
}
pub(crate) fn compose(a: [f64; 6], b: [f64; 6]) -> [f64; 6] {
    [
        a[0] * b[0] + a[2] * b[1],
        a[1] * b[0] + a[3] * b[1],
        a[0] * b[2] + a[2] * b[3],
        a[1] * b[2] + a[3] * b[3],
        a[0] * b[4] + a[2] * b[5] + a[4],
        a[1] * b[4] + a[3] * b[5] + a[5],
    ]
}
pub(crate) fn identity(matrix: [f64; 6]) -> bool {
    matrix
        .iter()
        .zip([1., 0., 0., 1., 0., 0.])
        .all(|(a, b)| (*a - b).abs() <= crate::doc_transform::EXACT_EPSILON)
}
pub(crate) fn source_bounds(
    dim: (u32, u32),
    matrix: [f64; 6],
) -> Result<(i32, i32, u32, u32), String> {
    transformed_plane_bounds(dim, (0, 0), &validate_matrix(matrix)?)
        .ok_or_else(|| "source transform exceeds supported pixel or coordinate bounds".into())
}
impl Layer {
    pub fn effective_source(&self, profile: &Arc<IccProfile>) -> Option<RetainedSource8> {
        let source = self.retained_source.as_ref()?;
        if self.kind != LayerKind::Raster
            // The ordinary-source owner and a layer-description owner must
            // never compete to rerender the same fallback. The placement ABI
            // requires an absent meta slot too. Unknown future descriptions
            // detach without parsing or rewriting this opaque host payload.
            || self.meta.is_some()
            || !LayerRaster::ptr_eq(&self.pixels, &source.rendered)
            || profile.bytes() != source.rendered_profile.bytes()
        {
            return None;
        }
        let mut source = source.clone();
        source.transform = crate::source_transform::translated(
            source.transform,
            f64::from(self.offset.0) - f64::from(source.rendered_offset.0),
            f64::from(self.offset.1) - f64::from(source.rendered_offset.1),
        )
        .ok()?;
        source.rendered_offset = self.offset;
        Some(source)
    }
}
impl crate::doc::DrDocument {
    pub(crate) fn detach_invalid_sources(&mut self) {
        for layer in &mut self.layers {
            layer.retained_source = layer.effective_source(&self.profile);
            layer.warp = layer.effective_warp(&self.profile);
            layer.generated = layer.effective_generated(&self.profile);
        }
    }
}
pub(crate) fn clean_typed_layers(
    layers: &mut [PrecisionDocumentLayer],
    profile: &Arc<IccProfile>,
    cmyk: bool,
) {
    fn invalid(layers: &[PrecisionDocumentLayer], profile: &Arc<IccProfile>, cmyk: bool) -> bool {
        layers.iter().any(|layer| {
            (layer.retained_source.is_some() && layer.effective_source().is_none())
                || (layer.warp.is_some() && layer.effective_warp(profile).is_none())
                || (layer.generated.is_some() && layer.effective_generated(profile, cmyk).is_none())
                || layer
                    .children()
                    .is_some_and(|children| invalid(children, profile, cmyk))
        })
    }
    for layer in layers {
        layer.retained_source = layer.effective_source();
        layer.warp = layer.effective_warp(profile);
        layer.generated = layer.effective_generated(profile, cmyk);
        match &mut layer.content {
            crate::precision_document::Content::Group(children)
            | crate::precision_document::Content::PassThrough(children) => {
                if invalid(children, profile, cmyk) {
                    clean_typed_layers(Arc::make_mut(children).as_mut_slice(), profile, cmyk);
                }
            }
            _ => {}
        }
    }
}
impl PrecisionDocumentLayer {
    pub fn effective_source(&self) -> Option<RetainedSource16> {
        if self.description.is_some() {
            return None;
        }
        let source = self.retained_source.as_ref()?;
        let image = self.source()?;
        // The fallback's storage is the record's own: u16, or f32 in a float
        // document (`retained_source_float`).
        if image.storage() != source.rendered.storage()
            || !PrecisionImage::ptr_eq(image, &source.rendered)
            || image.profile().bytes() != source.rendered.profile().bytes()
        {
            return None;
        }
        let mut source = source.clone();
        source.transform = crate::source_transform::translated(
            source.transform,
            f64::from(self.offset().0) - f64::from(source.rendered_offset.0),
            f64::from(self.offset().1) - f64::from(source.rendered_offset.1),
        )
        .ok()?;
        source.rendered_offset = self.offset();
        Some(source)
    }
}
pub(crate) fn info(
    asset: &SourceAsset,
    link: Option<&SourceLink>,
    matrix: SourceTransform,
    filter: GeometryFilter,
    fallback_depth: u32,
) -> Value {
    let (dimensions, profile, depth) = (asset.dimensions(), asset.profile(), asset.depth());
    let (affine, projective, kind) = match matrix {
        SourceTransform::Affine(m) => (Some(m), None, "affine"),
        SourceTransform::Projective(m) => (None, Some(m), "projective"),
    };
    json!({"valid":true,"source_id":asset.id,"revision_id":asset.revision_id,"link":link,"width":dimensions.0,"height":dimensions.1,
        "depth":depth,"storage":match depth {32=>"rgba32_float",16=>"rgba16_unorm",_=>"rgba8_unorm"},
        "master_depth":depth,"fallback_depth":fallback_depth,"depth_conversion":if depth==fallback_depth{"none"}else{"convert_fallback"},
        "profile":profile.name(),"profile_bytes":profile.bytes().len(),"transform":affine,
        "projective_transform":projective,"transform_kind":kind,
        "filter":match filter {GeometryFilter::Nearest=>"nearest",GeometryFilter::Bilinear=>"bilinear",GeometryFilter::Bicubic=>"bicubic",GeometryFilter::Lanczos3=>"lanczos3"},
        "embedded":link.is_none(),"cached_original":true,"raw":asset.raw.as_ref().map(|r|r.info()),
        "profile_refusal":asset.profile_refusal,
        "source_attachment":asset.attachment.as_ref().map(|a|json!({"kind":a.descriptor.kind,
            "descriptor_bytes":a.descriptor.encoded_len().unwrap_or(usize::MAX),
            "resource_count":a.resources.len(),"resource_bytes":a.resources.iter().map(|r|r.len()).collect::<Vec<_>>()})),
        "evaluation":"source_profile_then_affine_then_mask_style_blend_v1"})
}
pub(crate) fn info_scratch(asset: &SourceAsset, link: Option<&SourceLink>) -> u64 {
    let strings = asset.id.len()
        + asset.revision_id.len()
        + asset.profile().name().len()
        + asset.profile_refusal.as_ref().map_or(0, String::len)
        + link.map_or(0, |l| l.path.len() + l.sha256.len());
    let raw = asset
        .raw
        .as_ref()
        .map_or(Ok(0), |r| r.descriptor.encoded_len());
    raw.map_or(u64::MAX, |length| {
        // Core Value/string serialization and the host's parsed dictionaries
        // overlap. Three descriptor expansion bounds cover that publication.
        (64 * 1024_u64)
            .saturating_add(strings as u64 * 64)
            .saturating_add(length as u64 * 3 * crate::raw_source::RAW_DESCRIPTOR_EXPANSION)
    })
}
impl RetainedSource8 {
    pub(crate) fn record_resources(&self, inventory: &mut crate::resources::Inventory) {
        self.asset.record_resources(inventory);
        if let Some(link) = &self.link {
            link.record_resources(inventory);
        }
        self.rendered
            .record_resources(inventory, "layer_pixels", false);
        inventory.object(&*self.rendered_profile, "profile", false);
        self.rendered_profile.record_resources(inventory);
    }
}
impl RetainedSource16 {
    pub(crate) fn record_resources(&self, inventory: &mut crate::resources::Inventory) {
        self.asset.record_resources(inventory);
        if let Some(link) = &self.link {
            link.record_resources(inventory);
        }
        self.rendered.record_resources(inventory);
    }
}

#[derive(Clone, Copy)]
pub struct SourcePlan {
    pub retained_bytes: u64,
    pub scratch_bytes: u64,
    pub work_units: u64,
}
impl SourcePlan {
    pub fn json(&self) -> Value {
        json!({"retained_bytes":self.retained_bytes,"scratch_bytes":self.scratch_bytes,"work_units":self.work_units})
    }
}
