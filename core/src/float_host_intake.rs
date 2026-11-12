//! The host-decoded HDR still as a 32-bit float document: the
//! adapter between the host's pixel buffer (`host_pixels`' declared layouts)
//! and the ONE float document builder, `float_io::document_of`.
//!
//! The host owns stills (ImageIO / Core Image decode them), so what crosses
//! is one of two encodings:
//!
//! - [`HdrEncoding::Linear`] — a gain-map still Core Image expanded to its
//!   full headroom: `rgba32f_*` samples, already linear light with the SDR
//!   base's white at 1.0, in the primaries of the matrix/TRC ICC that comes
//!   with them (the base image's own space, which becomes the document
//!   profile).
//! - [`HdrEncoding::Signal`] — a PQ or HLG still as ImageIO's integer decode:
//!   a 16-bit or `rgb10_packed_le` layout, each code read by the layout's
//!   existing rule (`HostPixelPlan::codes`, so `E′ = code / 65535`), turned
//!   into linear light by `hdr_transfer::signal_to_linear` under the one
//!   reference-white rule (203 cd/m² is 1.0). Its stated primaries take
//!   `float_io::profile_of`'s rule: BT.709 → the built-in sRGB, Display P3 →
//!   the built-in Display P3, BT.2020 → converted once in linear light to
//!   sRGB primaries with that rule's warning; negatives are kept.
//!
//! An f32 sample that is not finite, or whose magnitude is past
//! `FLOAT_RANGE_MAX`, refuses by name: the host rendered it, so a repair
//! would hide a host defect rather than a file's. Alpha is straightened
//! here (`HostLayout::float_straight`), so `document_of` sees straight
//! samples and its own clamp discloses anything a tiny alpha pushed past
//! the range.
//!
//! # Admission
//!
//! Charged before anything is allocated ([`HostPixelPlan::admit_float`]):
//! the host's input, the straight f32 sample vector `document_of` takes
//! (16 B/px), the tile plane it builds while that vector is held (16 B/px),
//! the profile parse and bookkeeping — `float_io::admit_open`'s shape with
//! the sample vector as the seam's scratch.

use std::sync::Arc;

use crate::exr_float::Chromaticities;
use crate::float_document::FLOAT_RANGE_MAX;
use crate::float_io::{self, FloatContainer, FloatSource};
use crate::float_primaries;
use crate::hdr_transfer::{self, HdrTransfer};
use crate::host_pixels::HostPixelPlan;
use crate::icc::IccProfile;
use crate::job_control::JobContext;
use crate::metadata::{Metadata, Resolution};
use crate::precision_document::PrecisionDocument;
use crate::precision_image::{pixel_count, reserved_vec};

/// What the host's pixels are (module doc).
pub(crate) enum HdrEncoding {
    Linear(Arc<IccProfile>),
    Signal {
        transfer: HdrTransfer,
        primaries: Chromaticities,
    },
}

/// The label a refusal names the source by.
const LABEL: &str = "host pixels";

/// The primaries a PQ/HLG intake names: `bt709`, `display_p3` or `bt2020`.
pub(crate) fn primaries_named(name: &str) -> Result<Chromaticities, String> {
    match name {
        "bt709" => Ok(float_primaries::REC709),
        "display_p3" => Ok(float_primaries::DISPLAY_P3),
        "bt2020" => Ok(float_primaries::BT2020),
        other => Err(format!(
            "HDR host primaries \"{other}\" are not one of bt709, display_p3 or bt2020"
        )),
    }
}

/// A linear intake's profile: exact complete RGB matrix/TRC ICC bytes the
/// float policy can encode through.
pub(crate) fn linear_profile(bytes: &[u8]) -> Result<Arc<IccProfile>, String> {
    let profile = IccProfile::parse(bytes)
        .ok_or("HDR host pixel profile is not a supported RGB ICC profile")?;
    if profile.bytes().as_ref() != bytes {
        return Err(
            "HDR host pixel profile requires exact complete RGB matrix/TRC ICC bytes".into(),
        );
    }
    let model = profile
        .model()
        .ok_or("a 32-bit float document requires an RGB matrix/TRC profile")?;
    model.validate_precision()?;
    Ok(Arc::new(profile))
}

impl HostPixelPlan {
    /// The float intake's plan, admitted and reserved (module doc,
    /// "Admission"): `input` and the profile beside `other`, under `limit`.
    pub(crate) fn admit_float(
        &self,
        input: &[u8],
        profile_length: usize,
        limit: u64,
        other: u64,
        job: JobContext<'_>,
    ) -> Result<(), String> {
        let count = pixel_count(self.width, self.height)? as u64;
        let seam = count
            .checked_mul(16)
            .and_then(|v| v.checked_add(profile_length as u64 * 64))
            .ok_or("HDR still working-memory calculation overflow")?;
        float_io::admit_open(
            FloatContainer::HostStill,
            input,
            LABEL,
            (self.width, self.height),
            seam,
            limit,
            other,
            job,
        )
    }

    /// The host's pixels as a one-layer float document at the `clip` map
    /// (module doc). Refuses a layout that does not match the encoding, a
    /// non-finite or out-of-range f32 sample, and a cancel.
    pub(crate) fn float(
        &self,
        input: &[u8],
        encoding: HdrEncoding,
        job: JobContext<'_>,
    ) -> Result<PrecisionDocument, String> {
        if input.len() != self.input_length {
            return Err("HDR host pixel length disagrees with the plan".into());
        }
        let count = pixel_count(self.width, self.height)?;
        let mut warnings = Vec::new();
        let mut samples = reserved_vec(count * 4)?;
        let (profile, matrix) = match encoding {
            HdrEncoding::Linear(profile) => {
                if !self.layout.is_float() {
                    return Err("linear HDR host pixels require an rgba32f layout".into());
                }
                for index in 0..count {
                    job.every(index)?;
                    let raw = self.float_at(input, index);
                    if let Some(problem) = raw.iter().find_map(|v| {
                        if !v.is_finite() {
                            Some("a non-finite sample")
                        } else if v.abs() > FLOAT_RANGE_MAX {
                            Some("a sample outside ±65504")
                        } else {
                            None
                        }
                    }) {
                        let width = self.width as usize;
                        return Err(format!(
                            "HDR host pixels hold {problem} at ({}, {})",
                            index % width,
                            index / width
                        ));
                    }
                    samples.extend(self.layout.float_straight(raw));
                }
                (profile, None)
            }
            HdrEncoding::Signal {
                transfer,
                primaries,
            } => {
                if self.layout.is_float() || self.layout.depth != 16 {
                    return Err(
                        "PQ and HLG host pixels require an integer 16-bit or rgb10_packed_le layout"
                            .into(),
                    );
                }
                let luma = float_primaries::luminance_row(&primaries)
                    .ok_or("HDR host primaries do not describe a colour basis")?;
                let chosen = float_io::profile_of(
                    FloatContainer::HostStill,
                    Some(primaries),
                    "primaries",
                    LABEL,
                    &mut warnings,
                )?;
                for index in 0..count {
                    job.every(index)?;
                    let codes = self.codes(input, index)?;
                    let signal = [codes[0], codes[1], codes[2]].map(|c| f64::from(c) / 65535.0);
                    let linear = hdr_transfer::signal_to_linear(transfer, signal, luma);
                    samples.extend([
                        linear[0] as f32,
                        linear[1] as f32,
                        linear[2] as f32,
                        f32::from(codes[3]) / 65535.0,
                    ]);
                }
                chosen
            }
        };
        job.report(0.6, 1)?;
        float_io::document_of(
            FloatSource {
                container: FloatContainer::HostStill,
                width: self.width,
                height: self.height,
                orientation: 1,
                canvas: (self.width, self.height),
                offset: (0, 0),
                channels: 4,
                alpha: Some(false),
                samples,
                profile,
                matrix,
                resolution: Resolution::default(),
                metadata: Metadata::default(),
                warnings,
            },
            LABEL,
            job,
        )
    }
}
