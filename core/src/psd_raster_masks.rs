//! Ordinary PSD masks over the shared channel codec, separate from record framing.
//! Adobe's mask body describes coordinate relativity, not native content links:
//! https://www.adobe.com/devnet-apps/photoshop/fileformatashtml/
//! Only a 20-byte body, a canvas-coordinate rectangle, default0/255, disabled flag and one
//! -2 Raw/RLE channel are qualified. Other forms remain explicit per-file losses.
//! A 20-byte body whose rectangle is empty is Photoshop's own "no mask" form on
//! an adjustment record: Absent at outside255, a reported drop at outside0.
//! An admitted ordinary mask with malformed geometry or samples refuses layers.
//!
//! Ordinary20-byte -2 Raw/PackBits/ZIP masks retain independent bounds, exact samples, outside0/255 and
//! enabled state on rasters/folders, with explicit unlinked import; a 20-byte body whose rectangle
//! is EMPTY is Photoshop's own "no mask" form (Absent at outside255, a reported drop at outside0),
//! never a refusal — and that empty-rectangle rule is tested BEFORE the plane's compression,
//! because such a record's `-2` channel is never read at all (at 8 bits Photoshop leaves it a bare
//! compression word, at 16 a canvas-sized ZIP plane). Mask planes decode through the same
//! in-crate seam raster planes use (`psd_layer_pixels::mask_plane_bytes`), at the container's own
//! row-count width (`large` for a PSB). The rectangle is read as canvas coordinates
//! on rasters and folders alike: the position-relative flag (bit 0) is recorded for the report and
//! never applied, which is how Photoshop itself reads a file that sets it. Mask admission
//! includes native caps and codec scratch. Beside a vector mask, Photoshop's rendered `-2` and the
//! 36-byte body's real `-3` are read by [`VectorMaskRole`]. Other extended and parameter forms
//! remain explicit losses; malformed admitted mask data refuses layers.

use std::sync::Arc;

use image::GrayImage;

use crate::doc::{Layer, LayerKind};
use crate::doc_mask::MaskProperties;
use crate::drdc::MAX_DRDC_TOTAL_MASK_PIXELS;
use crate::precision_mask::{PrecisionMask, PrecisionPlane};
use crate::psd_layer_pixels::{mask_plane_bytes, validate_channel, Channel, LayerPixels};

/// What a record's `vmsk` makes of its mask channels (`psd_group_records`).
/// Photoshop writes, beside an ENABLED vector mask on a raster record, a `-2`
/// channel it RENDERED from the path (mask flag bit 3), and the user's own
/// raster mask, when the layer has one, as `-3` with the 36-byte body's
/// "real" flags, background and rectangle (the `authored_exchange` family).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum VectorMaskRole {
    /// No vector mask is weighed: the ordinary rules.
    None,
    /// The vector mask is carried live: the rendered `-2` is dropped and the
    /// `-3` user mask, when present, is the layer's mask (the raster part).
    Carried,
    /// The vector mask is not carried but Photoshop's rendering of it is the
    /// layer's mask: the rendered `-2` as an ordinary mask.
    Rendered,
    /// The vector mask is not carried and the file holds no rendering of it
    /// (Photoshop's 16-bit files): the import bakes the path
    /// (`psd_paths::vmsk_coverage`) into the layer's mask.
    Baked,
}

#[derive(Clone, Copy)]
pub(crate) enum MaskRecord<'a> {
    Absent,
    Dropped(&'static str),
    Ordinary(OrdinaryMask<'a>),
    /// The mask the import bakes from the record's vector mask
    /// ([`VectorMaskRole::Baked`]); nothing here attaches it.
    FromPath,
}

#[derive(Clone, Copy)]
pub(crate) struct OrdinaryMask<'a> {
    bounds: [i32; 4],
    position_relative_flag: bool,
    outside: u8,
    enabled: bool,
    bytes: &'a [u8],
    /// The record came from a PSB (`LayerPixels::large`).
    large: bool,
    /// The plane is Photoshop's rendering of a vector mask the import does
    /// not carry ([`VectorMaskRole::Rendered`]).
    rendered: bool,
}

impl<'a> MaskRecord<'a> {
    /// The record walker has already bounded the body and every channel slice.
    /// Unsupported mask data is never fed to a pixel decoder, preserving the
    /// established loss-reporting path for arbitrary/ZIP mask payloads.
    pub fn classify(
        body: &[u8],
        channels: &[Channel<'a>],
        advanced: bool,
        large: bool,
        role: VectorMaskRole,
    ) -> Result<Self, String> {
        match role {
            VectorMaskRole::None => {}
            VectorMaskRole::Baked => return Ok(Self::FromPath),
            VectorMaskRole::Carried => {
                // The `-3` user mask under the body's "real" fields, as an
                // ordinary 20-byte body; with no `-3`, a `-2` Photoshop did
                // not render (flag bit 3 clear, its 16-bit files) is the
                // user's own; else no raster part.
                let Some(real) = channels.iter().find(|c| c.kind == -3) else {
                    let own = body.len() == 20 && body[17] & 8 == 0;
                    if own && channels.iter().any(|c| c.kind == -2) {
                        return Self::classify(body, channels, false, large, VectorMaskRole::None);
                    }
                    return Ok(Self::Absent);
                };
                let Some(fields) = body.get(18..36) else {
                    return Ok(Self::Dropped(
                        "a real user mask (-3) without its extended mask metadata is unsupported",
                    ));
                };
                let mut ordinary = [0u8; 20];
                ordinary[..16].copy_from_slice(&fields[2..18]);
                ordinary[16] = fields[1];
                ordinary[17] = fields[0];
                let user = Channel {
                    kind: -2,
                    bytes: real.bytes,
                };
                return Self::classify(&ordinary, &[user], false, large, VectorMaskRole::None);
            }
            VectorMaskRole::Rendered => {
                // The rendered `-2` under the body's first 20 bytes, its
                // "rendered" flag (bit 3) cleared; a `-3` is already in it.
                let Some(head) = body.get(..20) else {
                    return Ok(Self::Dropped(
                        "mask metadata and ordinary -2 channel are not both present",
                    ));
                };
                let mut ordinary = [0u8; 20];
                ordinary.copy_from_slice(head);
                ordinary[17] &= !8;
                let rendered: Vec<Channel<'a>> = channels
                    .iter()
                    .filter(|c| c.kind == -2)
                    .map(|c| Channel {
                        kind: c.kind,
                        bytes: c.bytes,
                    })
                    .collect();
                return Self::classify(&ordinary, &rendered, advanced, large, VectorMaskRole::None)
                    .map(|record| match record {
                        Self::Ordinary(mask) => Self::Ordinary(OrdinaryMask {
                            rendered: true,
                            ..mask
                        }),
                        other => other,
                    });
            }
        }
        let user = channels.iter().find(|c| c.kind == -2);
        let real = channels.iter().any(|c| c.kind == -3);
        if advanced || real {
            return Ok(Self::Dropped(
                "vector, real-user (-3), or advanced mask behavior is unsupported",
            ));
        }
        if body.is_empty() && user.is_none() {
            return Ok(Self::Absent);
        }
        let Some(user) = user.filter(|_| !body.is_empty()) else {
            return Ok(Self::Dropped(
                "mask metadata and ordinary -2 channel are not both present",
            ));
        };
        if body.len() != 20 {
            return Ok(Self::Dropped(
                "extended or unrecognized mask metadata length is unsupported",
            ));
        }
        let flags = body[17];
        // Bit 0 (position relative to layer) stays inside the `flags & !3`
        // whitelist below and only chooses the report phrase: Photoshop reads
        // the rectangle as canvas coordinates whether or not it is set.
        let position_relative_flag = flags & 1 != 0;
        if flags & 16 != 0 {
            return Ok(Self::Dropped(
                "mask density/feather parameters are unsupported",
            ));
        }
        if flags & !3 != 0 {
            return Ok(Self::Dropped(
                "rendered, inverted, or unknown mask flags are unsupported",
            ));
        }
        if !matches!(body[16], 0 | 255) {
            return Ok(Self::Dropped("mask outside default is not 0 or 255"));
        }
        let mut bounds = [0; 4];
        for (value, bytes) in bounds.iter_mut().zip(body[..16].chunks_exact(4)) {
            *value = i32::from_be_bytes(bytes.try_into().unwrap());
        }
        if bounds[2] == bounds[0] && bounds[3] == bounds[1] {
            // Photoshop writes a full 20-byte mask body with a ZERO rectangle
            // and a bare -2 channel on every adjustment record, whether or not
            // the layer has a mask. With outside 255 that describes no mask at
            // all, so it is Absent rather than a refusal on an empty
            // rectangle; with outside 0 it would hide the whole layer, which
            // is a real difference in appearance and is reported as a drop.
            return Ok(if body[16] == 255 {
                Self::Absent
            } else {
                Self::Dropped(
                    "empty mask rectangle with outside default 0 would hide the whole layer",
                )
            });
        }
        // Only now, with a real rectangle, does the plane's compression
        // matter: the bare comp-0 or canvas-sized comp-3 `-2` channel of a
        // zero-rectangle body is never read at all. Raw, RLE, ZIP (2) and
        // ZIP-with-prediction (3) are admitted at both depths.
        if !matches!(user.bytes.get(..2), Some([0, 0..=3])) {
            return Ok(Self::Dropped(
                "mask compression is unsupported; only Raw, RLE and ZIP are imported",
            ));
        }
        // Bytes18..20 are padding, with no semantic value assigned to them.
        Ok(Self::Ordinary(OrdinaryMask {
            bounds,
            position_relative_flag,
            outside: body[16],
            enabled: flags & 2 == 0,
            bytes: user.bytes,
            large,
            rendered: false,
        }))
    }

    pub fn present(self) -> bool {
        !matches!(self, Self::Absent)
    }

    /// A Gradient Fill layer's Align with Layer box `[x, y, w, h]` over this
    /// record (`fill_align_box::align_box`): an Ordinary mask's plane
    /// decoded as [`MaskRecord::attach`] (`sample_bytes` 1) or
    /// [`MaskRecord::attach_typed`] decodes it, at its rectangle's top-left,
    /// with its outside default and enabled state; Absent and Dropped give
    /// the canvas. `Err` is the plane's decode error.
    pub fn align_box(self, canvas: (u32, u32), sample_bytes: usize) -> Result<[i32; 4], String> {
        use crate::fill_align_box::{align_box, MaskCoverage};
        let Self::Ordinary(mask) = self else {
            return Ok(align_box::<u8>(canvas, None));
        };
        let (w, h) = mask.dimensions()?;
        let plane = mask_plane_bytes(mask.bytes, w, h, sample_bytes, mask.large)?;
        let offset = (mask.bounds[1], mask.bounds[0]);
        if sample_bytes == 2 {
            let samples = be_u16_samples(&plane);
            return Ok(align_box(
                canvas,
                Some(MaskCoverage {
                    samples: &samples,
                    dimensions: (w, h),
                    offset,
                    outside: mask.outside,
                    enabled: mask.enabled,
                }),
            ));
        }
        Ok(align_box(
            canvas,
            Some(MaskCoverage {
                samples: &plane[..],
                dimensions: (w, h),
                offset,
                outside: mask.outside,
                enabled: mask.enabled,
            }),
        ))
    }

    pub fn validate(self, sample_bytes: u64) -> Result<(), String> {
        if let Self::Ordinary(mask) = self {
            let (w, h) = mask.dimensions()?;
            validate_channel(mask.bytes, w, h, sample_bytes, mask.large)
                .map_err(|e| format!("PSD mask: {e}"))?;
        }
        Ok(())
    }

    /// The typed twin of [`MaskRecord::attach`], at the container's own
    /// sample width: the same ordinary rectangle, samples, outside default
    /// and enabled state as an independent (unlinked) `PrecisionMask`, whose
    /// density and feather are the neutral 1/0 this import never infers. A
    /// 16-bit plane's big-endian samples, or an 8-bit plane's codes ×257 —
    /// the typed mask of an 8-bit CMYK file (#1 phase 3), whose document is
    /// typed at either depth — or a 32-bit plane's big-endian f32 coverage,
    /// stored as `round(clamp(v, 0, 1) · 65535)` with NaN as 0: a
    /// float document's masks are u16 coverage, and `psd_export_float`
    /// writes `v / 65535`, which this returns exactly.
    pub fn attach_typed(self, sample_bytes: usize) -> Result<Option<PrecisionMask>, String> {
        let Self::Ordinary(mask) = self else {
            return Ok(None);
        };
        let (w, h) = mask.dimensions()?;
        let plane = mask_plane_bytes(mask.bytes, w, h, sample_bytes, mask.large)?;
        let samples: Vec<u16> = if sample_bytes == 4 {
            plane
                .chunks_exact(4)
                .map(|s| {
                    let v = f32::from_be_bytes([s[0], s[1], s[2], s[3]]);
                    if v.is_nan() {
                        0
                    } else {
                        (f64::from(v.clamp(0.0, 1.0)) * 65535.0).round() as u16
                    }
                })
                .collect()
        } else if sample_bytes == 2 {
            be_u16_samples(&plane)
        } else {
            plane.iter().map(|v| u16::from(*v) * 257).collect()
        };
        let mut properties = MaskProperties::at((mask.bounds[1], mask.bounds[0]));
        // Typed raw outside coverage is the same explicit 0/255 endpoint pair
        // the byte masks use (`doc_mask`), not a u16 code.
        properties.outside_coverage = mask.outside;
        // As in `attach`: the PSD position flag is not a native link state.
        properties.linked = false;
        Ok(Some(PrecisionMask::new(
            PrecisionPlane::adopt(w, h, samples)?,
            properties,
            mask.enabled,
        )?))
    }

    pub fn attach(self, layer: &mut Layer) -> Result<(), String> {
        let Self::Ordinary(mask) = self else {
            return Ok(());
        };
        let (w, h) = mask.dimensions()?;
        // The same in-crate plane seam as raster channels, not a second
        // PackBits or ZIP implementation.
        let pixels = GrayImage::from_raw(
            w,
            h,
            mask_plane_bytes(mask.bytes, w, h, 1, mask.large)?.into_owned(),
        )
        .ok_or("PSD raw mask length mismatch")?;
        let mut properties = MaskProperties::at((mask.bounds[1], mask.bounds[0]));
        properties.outside_coverage = mask.outside;
        // The PSD position flag is not a documented native link-state mapping.
        // Preserve independent placement on later raster moves, disclosed by report.
        properties.linked = false;
        if layer.kind == LayerKind::Group {
            // Native group offset is its mask anchor (drdc's group contract;
            // doc_mask's geometry methods keep these fields together). Group
            // property moves derive their delta from the descendant union,
            // never this raw record offset, and respect the independent mask.
            layer.offset = properties.offset;
        }
        layer.mask = Some(Arc::new(pixels));
        layer.mask_enabled = mask.enabled;
        layer.mask_properties = properties;
        Ok(())
    }

    pub fn report(
        self,
        target: &str,
        kind: &str,
        losses: &mut Vec<String>,
        observations: &mut Vec<String>,
    ) {
        match self {
            Self::Absent => {}
            Self::FromPath => losses.push(format!(
                "{target} has a vector mask Doctored does not keep as a path (its subpaths combine in a way one fill rule cannot hold, it starts from all pixels, or the document's paths reach their anchor cap); it is imported as a raster mask drawn from its path."
            )),
            Self::Dropped(reason) => losses.push(format!(
                "{target} has a {kind} or advanced mask metadata; layered import drops it: {reason}."
            )),
            Self::Ordinary(mask) => {
                if mask.rendered {
                    losses.push(format!(
                        "{target} has a vector mask Doctored does not keep as a path (its subpaths combine in a way one fill rule cannot hold, it starts from all pixels, or the document's paths reach their anchor cap); the mask the file rendered from it is imported as a raster mask."
                    ));
                }
                let placement = if mask.position_relative_flag {
                    "placement at its stored rectangle (the PSD position-relative flag, bit 0, is set and not applied, as Photoshop reads it)"
                } else { "absolute placement" };
                observations.push(format!(
                    "{target}: ordinary raster mask samples, {placement}, enabled state and outside default are retained. The imported mask is independent (unlinked); PSD linkage is not inferred or preserved."
                ));
            }
        }
    }
}

impl OrdinaryMask<'_> {
    fn dimensions(self) -> Result<(u32, u32), String> {
        // The raster rectangle's signed bounds, per-container side limit and
        // 100 MP restriction, without introducing a second geometry policy.
        LayerPixels {
            bounds: self.bounds,
            channels: Vec::new(),
            large: self.large,
        }
        .dimensions()
        .map_err(|e| format!("PSD mask rectangle: {e}"))
    }
}

/// A decoded 16-bit mask plane's big-endian samples: the one decode both
/// [`MaskRecord::attach_typed`] and [`MaskRecord::align_box`] read.
fn be_u16_samples(plane: &[u8]) -> Vec<u16> {
    plane
        .chunks_exact(2)
        .map(|s| u16::from_be_bytes([s[0], s[1]]))
        .collect()
}

#[derive(Default)]
pub(crate) struct MaskBudget {
    pub pixels: u64,
    pub largest: u64,
    pub encoded: u64,
}

impl MaskBudget {
    /// Before any row validation, wrapper copy or output allocation. Count only
    /// retained masks; dropped payloads stay borrowed and are never decoded.
    /// A mask the import draws from a path ([`MaskRecord::FromPath`]) is a
    /// plane of `canvas_pixels` with no encoded channel.
    pub fn add(&mut self, record: MaskRecord<'_>, canvas_pixels: u64) -> Result<(), String> {
        let (pixels, encoded) = match record {
            MaskRecord::Ordinary(mask) => {
                let (w, h) = mask.dimensions()?;
                (u64::from(w) * u64::from(h), mask.bytes.len() as u64)
            }
            MaskRecord::FromPath => (canvas_pixels, 0),
            MaskRecord::Absent | MaskRecord::Dropped(_) => return Ok(()),
        };
        self.pixels = self.pixels.saturating_add(pixels);
        self.largest = self.largest.max(pixels);
        self.encoded = self.encoded.max(encoded);
        if self.pixels > MAX_DRDC_TOTAL_MASK_PIXELS {
            return Err("PSD aggregate mask pixels exceed the native mask limit".into());
        }
        Ok(())
    }
}
