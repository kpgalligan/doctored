//! The clipboard's conversions ACROSS colour modes and CMYK profiles (#547
//! phase 4c): a typed payload pasted where its pixels are not at home.
//! Every arm here converts pasted pixels into the destination's mode and
//! profile at relative colorimetric with black-point compensation — always
//! through `cmyk_color`'s legs and `cmyk_convert`'s three per-image
//! functions, never new colour math.
//!
//! * **RGB → CMYK** ([`ClipboardPayload::converted_to_cmyk`]): the payload is
//!   first brought to u16 RGB in its OWN profile exactly as a 16-bit RGB
//!   paste brings it (`ClipboardPayload::converted_with_context`: a float
//!   payload under its own tone map; a u16 one is used as it is), then
//!   through `RgbToCmyk` into the destination policy at its declared depth
//!   (`cmyk_convert::rgb16_to_cmyk_image`).
//! * **CMYK → RGB** ([`CmykClipboardPayload::converted_to_rgb`]): ink through
//!   `CmykToRgb` into the target profile as u16
//!   (`cmyk_convert::cmyk_to_rgb16_image`), then, into a float target, the
//!   existing 16 → float promotion.
//! * **CMYK A → CMYK B** ([`CmykClipboardPayload::converted_with`]): Convert
//!   runs `CmykToCmyk` (`cmyk_convert::cmyk_to_cmyk_image`) at the
//!   destination's declared depth; Preserve (Don't Convert) keeps the ink and
//!   the paste relabels it; the same profile bytes keep the ink bit for bit
//!   at any declared depth. Both answer `None`.
//!
//! Every arm carries the payload's editable section as it is: the host
//! re-inks the record it holds (`ClipboardEditable.swift`), so a layers
//! paste's converted CMYK raster lands without a description
//! (`doc_paste_layers`' `cmyk_converted_clip` builds it fresh) and the host
//! re-renders it.
//!
//! Don't Convert across modes is refused by [`CROSS_MODE_PRESERVE`]: an
//! ink number means nothing as an RGB one. Every conversion admits its
//! charge before it allocates, built from the mode conversions' own terms
//! (`TO_CMYK_BYTES_PER_PIXEL`, `TO_RGB_BYTES_PER_PIXEL` and the memo's
//! `memo_peak_bytes(_for)`).

use std::sync::Arc;

use image::RgbaImage;

use crate::cmyk_clipboard_codec::{cmyk_payload_bytes, CmykClipboardPayload};
use crate::cmyk_color::{CmykToCmyk, CmykToRgb, ConversionOptions, RgbToCmyk};
use crate::cmyk_convert::{
    cmyk_to_cmyk_image, cmyk_to_rgb16_image, rgb16_to_cmyk_image, TO_CMYK_BYTES_PER_PIXEL,
    TO_RGB_BYTES_PER_PIXEL,
};
use crate::cmyk_document::{CmykDepth, CmykPolicy};
use crate::icc::IccProfile;
use crate::job_control::{DrJobControl, JobContext};
use crate::precision_clipboard::{ClipboardPayload, PasteProfile};
use crate::precision_clipboard_codec::payload_bytes;
use crate::precision_document::PrecisionDocument;
use crate::precision_evaluation::TypedTransfer;
use crate::precision_image::{
    admit, pixel_count, rgba8_preview_scratch, PrecisionStorage, BOOKKEEPING_BYTES,
};
use crate::precision_quantize::PrecisionDither;
use crate::soft_proof::{memo_peak_bytes, memo_peak_bytes_for};

/// The refusal of Don't Convert across colour modes.
pub(crate) const CROSS_MODE_PRESERVE: &str =
    "Don't Convert keeps numbers within one colour mode; pixels pasted between RGB and CMYK are always converted";

const OVERFLOW: &str = "clipboard conversion size overflow";

/// The RGB → CMYK leg's charge over `pixels`: the mode conversion's per-pixel
/// term and the `[f64; 4]` memo.
pub(crate) fn to_cmyk_terms(pixels: u64) -> Result<u64, String> {
    pixels
        .checked_mul(TO_CMYK_BYTES_PER_PIXEL)
        .and_then(|n| n.checked_add(memo_peak_bytes_for::<[f64; 4]>(pixels)))
        .and_then(|n| n.checked_add(BOOKKEEPING_BYTES))
        .ok_or_else(|| OVERFLOW.to_string())
}

impl ClipboardPayload {
    /// This payload as CMYK ink in `policy` at `depth` (module doc). Admits
    /// [`Self::cmyk_conversion_bytes`] first; Preserve refuses.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn converted_to_cmyk(
        &self,
        policy: &Arc<CmykPolicy>,
        depth: CmykDepth,
        mode: PasteProfile,
        limit: u64,
        other: u64,
        job: JobContext<'_>,
    ) -> Result<CmykClipboardPayload, String> {
        job.check()?;
        if mode == PasteProfile::Preserve {
            return Err(CROSS_MODE_PRESERVE.into());
        }
        let scratch = self.cmyk_conversion_bytes(policy)?;
        admit(0, scratch, other, limit)?;
        job.reserve(scratch)?;
        let own = self.image().profile().clone();
        let staged = self.converted_with_context(
            PrecisionStorage::Rgba16Unorm,
            &own,
            PasteProfile::Convert,
            limit,
            other,
            job.range(0.0, 0.3),
        )?;
        let rgb16 = staged.as_ref().unwrap_or(self);
        let mut leg = RgbToCmyk::new(own, &policy.profile, ConversionOptions::default())?;
        let image = rgb16_to_cmyk_image(rgb16.image(), &mut leg, depth, job.range(0.3, 1.0))?;
        Ok(CmykClipboardPayload {
            image,
            policy: policy.clone(),
            depth,
            offset: self.offset(),
            resolution: self.resolution(),
            coverage: self.coverage.clone(),
            editable: self.editable.clone(),
        })
    }

    /// [`Self::converted_to_cmyk`]'s charge: the 16-bit staging
    /// (`conversion_bytes` into u16 in the payload's own profile, 0 for a u16
    /// payload) and the RGB → CMYK leg over the window.
    pub(crate) fn cmyk_conversion_bytes(&self, _policy: &CmykPolicy) -> Result<u64, String> {
        let (w, h) = self.dimensions();
        let staged = self.conversion_bytes(
            PrecisionStorage::Rgba16Unorm,
            self.image().profile(),
            PasteProfile::Convert,
        )?;
        to_cmyk_terms(pixel_count(w, h)? as u64)?
            .checked_add(staged)
            .ok_or_else(|| OVERFLOW.to_string())
    }
}

impl CmykClipboardPayload {
    /// This payload as RGB in `profile` and `storage` (module doc). Admits
    /// [`Self::rgb_conversion_bytes`] first; Preserve refuses.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn converted_to_rgb(
        &self,
        profile: &Arc<IccProfile>,
        storage: PrecisionStorage,
        mode: PasteProfile,
        limit: u64,
        other: u64,
        job: JobContext<'_>,
    ) -> Result<ClipboardPayload, String> {
        job.check()?;
        if mode == PasteProfile::Preserve {
            return Err(CROSS_MODE_PRESERVE.into());
        }
        let scratch = self.rgb_conversion_bytes(storage)?;
        admit(0, scratch, other, limit)?;
        job.reserve(scratch)?;
        let mut leg = CmykToRgb::new(&self.policy.profile, profile, ConversionOptions::default())?;
        let image = cmyk_to_rgb16_image(&self.image, &mut leg, profile, job.range(0.0, 0.7))?;
        let rgb16 = ClipboardPayload {
            image,
            offset: self.offset,
            resolution: self.resolution,
            coverage: self.coverage.clone(),
            byte_origin: false,
            tone_map: None,
            editable: self.editable.clone(),
        };
        if storage == PrecisionStorage::Rgba16Unorm {
            return Ok(rgb16);
        }
        let held = other
            .checked_add(rgb16.payload_bytes())
            .ok_or_else(|| OVERFLOW.to_string())?;
        Ok(rgb16
            .converted_with_context(
                storage,
                profile,
                PasteProfile::Convert,
                limit,
                held,
                job.range(0.7, 1.0),
            )?
            .unwrap_or(rgb16))
    }

    /// [`Self::converted_to_rgb`]'s charge: the CMYK → RGB leg's per-pixel
    /// term and memo, and into float the promoted payload with its encoding
    /// and the typed transfer's tables.
    pub(crate) fn rgb_conversion_bytes(&self, storage: PrecisionStorage) -> Result<u64, String> {
        let (w, h) = self.dimensions();
        let pixels = pixel_count(w, h)? as u64;
        let promotion = match storage {
            PrecisionStorage::Rgba16Unorm => 0,
            PrecisionStorage::Rgba32Float => {
                2 * payload_bytes(w, h, storage, self.coverage.is_some())
                    + TypedTransfer::table_bytes()
            }
        };
        pixels
            .checked_mul(TO_RGB_BYTES_PER_PIXEL)
            .and_then(|n| n.checked_add(memo_peak_bytes(pixels)))
            .and_then(|n| n.checked_add(promotion + BOOKKEEPING_BYTES))
            .ok_or_else(|| OVERFLOW.to_string())
    }

    /// This payload's ink in `policy` at `depth` under `mode` (module doc), or
    /// `None` when the ink is kept: the same profile bytes, or Preserve.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn converted_with(
        &self,
        policy: &Arc<CmykPolicy>,
        depth: CmykDepth,
        mode: PasteProfile,
        limit: u64,
        other: u64,
        job: JobContext<'_>,
    ) -> Result<Option<Self>, String> {
        job.check()?;
        let scratch = self.conversion_bytes(policy, mode)?;
        if scratch == 0 {
            return Ok(None);
        }
        admit(0, scratch, other, limit)?;
        job.reserve(scratch)?;
        let mut leg = CmykToCmyk::new(
            &self.policy.profile,
            &policy.profile,
            ConversionOptions::default(),
        )?;
        let image = cmyk_to_cmyk_image(&self.image, &mut leg, depth, job)?;
        Ok(Some(Self {
            image,
            policy: policy.clone(),
            depth,
            offset: self.offset,
            resolution: self.resolution,
            coverage: self.coverage.clone(),
            editable: self.editable.clone(),
        }))
    }

    /// [`Self::converted_with`]'s charge, 0 when the ink is kept: the
    /// CMYK → CMYK leg's per-pixel term (the mode conversion's) and its memo.
    pub(crate) fn conversion_bytes(
        &self,
        policy: &CmykPolicy,
        mode: PasteProfile,
    ) -> Result<u64, String> {
        if mode == PasteProfile::Preserve || self.policy.profile_bytes() == policy.profile_bytes() {
            return Ok(0);
        }
        let (w, h) = self.dimensions();
        to_cmyk_terms(pixel_count(w, h)? as u64)
    }

    /// The payload's 8-bit pixels in `profile` (`None` = the CMYK document's
    /// view profile, Display P3): [`Self::converted_to_rgb`] into u16, then
    /// `PrecisionImage::to_rgba8`'s rule with the intermediate counted as
    /// other retained bytes — `ClipboardPayload::to_rgba8_in`'s shape.
    /// Preserve refuses.
    pub fn to_rgba8_in(
        &self,
        profile: Option<&Arc<IccProfile>>,
        mode: PasteProfile,
        limit: u64,
        other: u64,
        control: Option<&DrJobControl>,
    ) -> Result<RgbaImage, String> {
        let _run = control.map(DrJobControl::begin).transpose()?;
        let job = control.map_or(JobContext::NONE, JobContext::new);
        let target = profile.cloned().unwrap_or_else(IccProfile::display_p3);
        let rgb = self.converted_to_rgb(
            &target,
            PrecisionStorage::Rgba16Unorm,
            mode,
            limit,
            other,
            job.range(0.0, 0.5),
        )?;
        let other = other
            .checked_add(rgb.payload_bytes())
            .ok_or_else(|| OVERFLOW.to_string())?;
        let (w, h) = rgb.dimensions();
        let scratch = rgba8_preview_scratch(w, h)?;
        admit(rgb.image().retained_bytes(), scratch, other, limit)?;
        job.reserve(scratch)?;
        rgb.image()
            .quantize_with_context(PrecisionDither::None, job.range(0.5, 1.0))
    }

    /// A pasteboard image that is a CMYK TIFF or JPEG `cmyk_source` answers
    /// for, as a payload at (0, 0) holding its ink, its declared depth and
    /// its resolution; `Ok(None)` for any other bytes. Its own profile is
    /// kept when it embeds a usable one; otherwise `fallback_icc` (when it is
    /// a usable CMYK profile), else the working CMYK profile, is ASSIGNED and
    /// the second value is the untagged sentence naming it. The open is
    /// admitted by `PrecisionDocument::open_cmyk_file`.
    pub fn from_cmyk_image(
        bytes: &[u8],
        fallback_icc: Option<&[u8]>,
        limit: u64,
        other: u64,
    ) -> Result<Option<(Self, Option<String>)>, String> {
        if crate::cmyk_source::cmyk_source(bytes).is_none() {
            return Ok(None);
        }
        let document = PrecisionDocument::open_cmyk_file(
            bytes,
            "the clipboard image",
            limit,
            other,
            JobContext::NONE,
        )?;
        let opened = document
            .cmyk_policy()
            .ok_or("the clipboard image did not open as CMYK")?
            .clone();
        let depth = opened.depth();
        let embedded = crate::dr_image::embedded_icc(bytes)
            .is_some_and(|icc| CmykPolicy::new(&icc, depth).is_ok());
        let fallback = fallback_icc.and_then(|icc| CmykPolicy::new(icc, depth).ok());
        let (policy, assigned) = match (embedded, fallback) {
            (true, _) => (opened, None),
            (false, Some(fallback)) => {
                let sentence = crate::cmyk_io::untagged_sentence(fallback.name());
                (Arc::new(fallback), Some(sentence))
            }
            (false, None) => {
                let sentence = crate::cmyk_io::untagged_sentence(opened.name());
                (opened, Some(sentence))
            }
        };
        let image = document
            .layer(0)
            .and_then(|layer| layer.cmyk_source())
            .ok_or("the clipboard image did not open as one CMYK layer")?
            .clone();
        let (w, h) = image.dimensions();
        let held = cmyk_payload_bytes(w, h, false);
        admit(document.retained_bytes(), 2 * held, other, limit)?;
        Ok(Some((
            Self {
                image,
                policy,
                depth,
                offset: (0, 0),
                resolution: document.resolution(),
                coverage: None,
                editable: None,
            },
            assigned,
        )))
    }
}
