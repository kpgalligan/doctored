//! Whether a flat save can preserve the document's editable state. This is
//! separate from explicit export, which deliberately bakes the projection.
//! Keep this inventory in step with the native schema: its editable-state
//! lines are `flat_state`'s, shared with typed documents
//! (`precision_preservation`). Default Background
//! layer properties and inferred sRGB/72 ppi need no native representation.
//! JPEG compression and GIF palette quantization remain explicit format
//! choices; this query protects editing state, alpha and sidecar data.

use crate::doc::{BlendMode, DrDocument, LayerKind};
use crate::doc_color::{CARRIES_EXIF, CARRIES_IPTC, CARRIES_PROFILE, CARRIES_XMP};
use crate::dr_image::clamp_jfif_density;
use crate::flat_state::{self, EditableState};
use crate::metadata::{self, Resolution};
use crate::metadata_write::{png_phys, EncodeSidecar};
use crate::style::GlobalLight;
use crate::tiff_view::resolution_from_exif;
use crate::Format;

/// A PNG density is an integer number of pixels/metre: half its 0.0254 ppi
/// step is 0.0127. Allow 0.013 ppi for that quantization and float rounding,
/// also when evaluating JPEG's coarser integer-ppi fallback. Larger shifts
/// require native preservation unless normalized EXIF retains the precision.
const DENSITY_TOLERANCE: f32 = 0.013;

/// TIFF writes four-decimal rational PPI directly. JPEG/PNG ask the same
/// reader that will reopen this output through tiny header-only streams,
/// using the actual writer's density conversion
/// and existing scanner, including its "1 x 1 means unstated" rule, without
/// encoding image pixels or implementing another metadata parser.
fn reopened_resolution(format: Format, sidecar: &EncodeSidecar<'_>) -> Resolution {
    if format == Format::Tiff {
        return sidecar.dpi.unwrap_or_default().sane();
    }
    if !matches!(format, Format::Png | Format::Jpeg) {
        return Resolution::default();
    }
    if let Some(exif) = sidecar.exif.as_deref().and_then(resolution_from_exif) {
        return exif.sane();
    }
    let Some(dpi) = sidecar.dpi else {
        return Resolution::default();
    };
    let mut header = match format {
        Format::Png => crate::metadata::PNG_SIGNATURE.to_vec(),
        // SOI, APP0 (16 bytes), JFIF identifier, version 1.2, inches.
        Format::Jpeg => b"\xff\xd8\xff\xe0\x00\x10JFIF\0\x01\x02\x01".to_vec(),
        _ => unreachable!(),
    };
    if format == Format::Png {
        header.extend_from_slice(&png_phys(dpi));
    } else {
        header.extend_from_slice(&clamp_jfif_density(dpi.x).to_be_bytes());
        header.extend_from_slice(&clamp_jfif_density(dpi.y).to_be_bytes());
        header.extend_from_slice(&[0, 0]); // no JFIF thumbnail
    }
    metadata::scan(&header).resolution.unwrap_or_default()
}

impl DrDocument {
    /// Reasons that Save should use native preservation instead. Packet
    /// normalization matches export (orientation/density and stale previews
    /// are intentionally normalized); an entirely dropped packet is a loss.
    pub(crate) fn flat_save_losses(
        &self,
        format: Format,
        embed_profile: bool,
        strip_metadata: bool,
    ) -> Vec<&'static str> {
        let layers = &self.layers;
        let mut losses = flat_state::losses(&EditableState {
            sources: layers
                .iter()
                .any(|layer| layer.effective_source(&self.profile).is_some()),
            warps: layers
                .iter()
                .any(|layer| layer.effective_warp(&self.profile).is_some()),
            layers: layers.len() != 1 || layers.iter().any(|l| l.kind == LayerKind::Group),
            masks: layers.iter().any(|l| l.mask.is_some() || !l.mask_enabled),
            descriptions: layers.iter().any(|l| l.meta.is_some()),
            styles: layers.iter().any(|l| l.style.is_some()),
            properties: layers.iter().any(|l| {
                l.name != "Background"
                    || !l.visible
                    || l.opacity != 1.0
                    || l.blend != BlendMode::Normal
                    || l.clipped
                    || l.locks != 0
                    || l.link != 0
                    || l.label != 0
                    || l.depth != 0
                    || !l.open
            }),
            bounds: layers
                .iter()
                .any(|l| l.offset != (0, 0) || l.pixels.dimensions() != (self.width, self.height)),
            channels: self.channels.iter().any(|c| c.spot.is_none()),
            // A flat TIFF keeps them as extra samples (#1 1c-2a, `tiff_spot`).
            spot_channels: self.has_spot_channel() && format != Format::Tiff,
            guides: !self.guides.is_empty() || self.ruler_origin != (0.0, 0.0),
            samplers: !self.color_samplers.is_empty(),
            paths: !self.paths.entries.is_empty(),
            comps: !self.comps.is_empty(),
            global_light: self.global_light != GlobalLight::default(),
        });

        let (sidecar, carried) = self.image_sidecar(
            format,
            embed_profile,
            strip_metadata,
            self.width,
            self.height,
        );
        // WebP capture packets do not round-trip through this build's
        // opener. TIFF now reads its profile and standard capture tags.
        // Only the built-in default may be inferred on reopen. Another sRGB
        // profile still has its own authored bytes/name that Save must retain.
        if !self.profile.is_builtin_srgb() && carried & CARRIES_PROFILE == 0 {
            losses.push("color profile");
        }
        let density = reopened_resolution(format, &sidecar);
        let tolerance = if matches!(format, Format::Png | Format::Jpeg | Format::Tiff) {
            DENSITY_TOLERANCE
        } else {
            0.0 // No density slot is read: only the exact default is preserved.
        };
        if (self.resolution.x - density.x).abs() > tolerance
            || (self.resolution.y - density.y).abs() > tolerance
        {
            losses.push("print resolution");
        }
        for (packet, bit, label) in [
            (&self.metadata.exif, CARRIES_EXIF, "EXIF metadata"),
            (&self.metadata.xmp, CARRIES_XMP, "XMP metadata"),
            (&self.metadata.iptc, CARRIES_IPTC, "IPTC metadata"),
        ] {
            if packet.is_some()
                && (carried & bit == 0
                    || !matches!(format, Format::Png | Format::Jpeg | Format::Tiff))
            {
                losses.push(label);
            }
        }
        // This check needs no projection: a plain layer is already the
        // canvas, and rich documents have been caught above. JPEG discards
        // alpha; GIF only carries a binary transparent palette index.
        if matches!(format, Format::Jpeg | Format::Gif)
            && self.layers.iter().any(|l| {
                l.pixels.runs().flatten().any(|p| {
                    if format == Format::Jpeg {
                        p[3] != 255
                    } else {
                        p[3] != 0 && p[3] != 255
                    }
                })
            })
        {
            losses.push("pixel transparency");
        }
        // Flat writes encode the projection, whose fully transparent pixels
        // have zero RGB. Even a plain PNG layer can contain hidden color that
        // an alpha edit could reveal; native preserves those stored bytes.
        if self.layers.iter().any(|l| {
            l.pixels
                .runs()
                .flatten()
                .any(|p| p[3] == 0 && p[..3].iter().any(|&c| c != 0))
        }) {
            losses.push("hidden pixel colors");
        }
        losses
    }
}
