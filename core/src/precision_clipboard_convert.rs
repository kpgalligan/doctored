//! The clipboard's CONVERT arm (Phase 12G-2, later widened): a typed
//! payload of another depth or profile brought into a document's storage
//! and profile under a [`PasteProfile`] mode.
//!
//! The resolution, in order:
//!
//! - The same storage, and either Preserve or the same profile bytes →
//!   `Ok(None)`: nothing converts, and a paste relabels the samples through
//!   `with_profile`.
//! - A u16 target, depth first, then profile:
//!   1. f32 → u16, in the PAYLOAD's own profile under the PAYLOAD's carried
//!      tone map: `rgb = FloatPolicy::encode_sdr(rgb)` — exactly
//!      `tone_mapped_u16`'s rule under the document's map — and
//!      `a = round(clamp(a, 0, 1) · 65535)`, rounded once to u16.
//!   2. Under Convert, when the profile bytes differ,
//!      `precision_color::convert_u16_pixels` (the kernel `convert_layers`
//!      runs) on those u16 samples, in place. A u16 payload starts here.
//!      Preserve skips this step: the numbers are kept and relabelled.
//! - An f32 target → `precision_clipboard_promote` (u16 → f32 promotion, or
//!   an f32 payload's linear matrix into another profile under Convert).
//!
//! Any conversion clears the byte-origin flag. Every conversion admits its
//! charge ([`ClipboardPayload::conversion_bytes`]) before allocating.
//!
//! `conversion_bytes` is the conversion's own admit (bookkeeping + `2·payload_bytes` of the
//! target + the tone encoder / transform preparation into u16, or `TypedTransfer::table_bytes()` into f32;
//! 0 when nothing converts).

use std::sync::Arc;

use crate::float_document::{FloatPolicy, ToneMap};
use crate::icc::IccProfile;
use crate::icc_transform::{PrecisionToneEncoder, PrecisionTransform};
use crate::job_control::{DrJobControl, JobContext};
use crate::precision_clipboard::PasteProfile;
use crate::precision_clipboard_codec::{payload_bytes, ClipboardPayload};
use crate::precision_color::{convert_u16_pixels, U16Conversion};
use crate::precision_evaluation::TypedTransfer;
use crate::precision_image::{
    admit, pixel_count, reserved_vec, PrecisionImage, PrecisionStorage, BOOKKEEPING_BYTES,
};

impl ClipboardPayload {
    /// The payload in `storage` and `profile` under Convert
    /// ([`Self::converted_with`]).
    pub fn converted(
        &self,
        storage: PrecisionStorage,
        profile: &Arc<IccProfile>,
        limit: u64,
        other: u64,
        control: Option<&DrJobControl>,
    ) -> Result<Option<Self>, String> {
        self.converted_with(
            storage,
            profile,
            PasteProfile::Convert,
            limit,
            other,
            control,
        )
    }

    /// The payload in `storage` and `profile` under `mode` (module doc), or
    /// `Ok(None)` when nothing converts. Admits
    /// [`Self::conversion_bytes`] before allocating.
    pub fn converted_with(
        &self,
        storage: PrecisionStorage,
        profile: &Arc<IccProfile>,
        mode: PasteProfile,
        limit: u64,
        other: u64,
        control: Option<&DrJobControl>,
    ) -> Result<Option<Self>, String> {
        let _run = control.map(DrJobControl::begin).transpose()?;
        let job = control.map_or(JobContext::NONE, JobContext::new);
        self.converted_with_context(storage, profile, mode, limit, other, job)
    }

    /// True when `storage`, `profile` and `mode` resolve to no conversion.
    fn unchanged_into(
        &self,
        storage: PrecisionStorage,
        profile: &Arc<IccProfile>,
        mode: PasteProfile,
    ) -> bool {
        storage == self.storage()
            && (mode == PasteProfile::Preserve || self.image.profile().bytes() == profile.bytes())
    }

    /// The working memory the conversion admits, 0 when nothing converts:
    /// bookkeeping plus the converted payload and its encoding (`2 ·
    /// payload_bytes` of the target), plus, into u16, the tone encoder for
    /// an f32 source and the profile transform's preparation under Convert
    /// with differing profiles, or, into f32, `TypedTransfer::table_bytes`.
    /// Refuses, under Convert with differing profiles, a profile with no
    /// matrix/TRC model.
    pub fn conversion_bytes(
        &self,
        storage: PrecisionStorage,
        profile: &Arc<IccProfile>,
        mode: PasteProfile,
    ) -> Result<u64, String> {
        if self.unchanged_into(storage, profile, mode) {
            return Ok(0);
        }
        let (w, h) = self.dimensions();
        let target = 2 * payload_bytes(w, h, storage, self.coverage.is_some());
        let mut scratch = BOOKKEEPING_BYTES + target;
        if storage == PrecisionStorage::Rgba32Float {
            if mode == PasteProfile::Convert {
                self.profile_models(profile)?;
            }
            return Ok(scratch + TypedTransfer::table_bytes());
        }
        if self.storage() == PrecisionStorage::Rgba32Float {
            scratch += PrecisionToneEncoder::scratch_bytes();
        }
        if mode == PasteProfile::Convert {
            if let Some((src, dst)) = self.profile_models(profile)? {
                scratch += PrecisionTransform::prepare_scratch_bytes(src, dst);
            }
        }
        Ok(scratch)
    }

    /// The source and destination matrix/TRC models when the profile bytes
    /// differ, `None` when they agree; refuses a profile with no model
    /// (`precision_clipboard_promote::models`, the one statement of the rule).
    pub(crate) fn profile_models<'a>(
        &'a self,
        profile: &'a Arc<IccProfile>,
    ) -> Result<
        Option<(
            &'a crate::icc_transform::MatrixTrc,
            &'a crate::icc_transform::MatrixTrc,
        )>,
        String,
    > {
        crate::precision_clipboard_promote::models(self.image.profile(), profile)
    }

    pub(crate) fn converted_with_context(
        &self,
        storage: PrecisionStorage,
        profile: &Arc<IccProfile>,
        mode: PasteProfile,
        limit: u64,
        other: u64,
        job: JobContext<'_>,
    ) -> Result<Option<Self>, String> {
        job.check()?;
        let scratch = self.conversion_bytes(storage, profile, mode)?;
        if scratch == 0 {
            return Ok(None);
        }
        admit(0, scratch, other, limit)?;
        job.reserve(scratch)?;
        if storage == PrecisionStorage::Rgba32Float {
            return self.promoted(profile, mode, job).map(Some);
        }
        let source_profile = self.image.profile();
        let (w, h) = self.dimensions();
        let count = pixel_count(w, h)?;
        let models = match mode {
            PasteProfile::Convert => self.profile_models(profile)?,
            PasteProfile::Preserve => None,
        };
        let mut pixels: Vec<[u16; 4]> = reserved_vec(count)?;
        if let Some(runs) = self.image.runs_rgba16() {
            for run in runs {
                pixels.extend_from_slice(run);
            }
        } else if let Some(runs) = self.image.runs_rgba32() {
            let policy =
                FloatPolicy::for_profile(source_profile, self.tone_map.unwrap_or(ToneMap::Clip))?;
            let mut i = 0;
            for run in runs {
                for pixel in run {
                    job.every(i)?;
                    i += 1;
                    let rgb = policy.encode_sdr([pixel[0], pixel[1], pixel[2]]);
                    let mapped = [rgb[0], rgb[1], rgb[2], pixel[3]];
                    if mapped.iter().any(|v| !v.is_finite()) {
                        return Err("clipboard conversion produced a nonfinite component".into());
                    }
                    pixels.push(mapped.map(|v| (v.clamp(0.0, 1.0) * 65535.0).round() as u16));
                }
            }
        } else {
            return Err("clipboard payload has no typed samples".into());
        }
        if let Some((src, dst)) = models {
            let transform = PrecisionTransform::new(src, dst)
                .map_err(|e| crate::icc_encode_refusal::own_profile_error(profile, e))?;
            convert_u16_pixels(
                &mut pixels,
                U16Conversion::Matrix(&transform),
                job,
                0,
                count as u64,
            )?;
        }
        let image = PrecisionImage::from_rgba16_unseeded(w, h, pixels, profile.clone(), None)?;
        job.report(1.0, 5)?;
        Ok(Some(Self {
            image,
            offset: self.offset,
            resolution: self.resolution,
            coverage: self.coverage.clone(),
            byte_origin: false,
            tone_map: None,
            editable: self.editable.clone(),
        }))
    }
}
