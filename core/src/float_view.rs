//! A VIEW of a 32-bit float projection: the display path
//! under an exposure in stops and, optionally, a tone map other than the
//! document's own.
//!
//! A tone-mapped display hides what is above white: under `clip` every value
//! past 1.0 is the same white, and under `reinhard` everything past the white
//! point is. A view exposure is how a host looks at that range without
//! editing the document — `gain = 2^stops` on each RGB sample before the map,
//! alpha untouched — and a candidate map is how a host previews a tone-map
//! choice before committing it. Neither is document state: nothing here
//! returns a document, and the samples, the readouts, the exports and the
//! saved file are the same whatever a window is showing.
//!
//! # One display path
//!
//! [`PrecisionDocument::display_rgba8_view`] is `display_rgba8` with those
//! two inputs: the same admission (`float_document::display_scratch`, so it
//! is not a new memory plan), the same encode loop
//! (`float_document::encoded_u16_gained`) and the same undithered quantizer.
//! At [`FloatView::DOCUMENT`] — the document's own map and 0 stops — the gain
//! is exactly 1.0 and the bytes are `display_rgba8`'s, byte for byte.
//!
//! # The extended-range view (EDR)
//!
//! [`PrecisionDocument::display_extended`] is the one other form: on an EDR
//! display the window shows the samples themselves, values above white
//! brighter than white, so instead of codes it writes `display_encoding`'s
//! rule X — linear RGB times the same `2^stops` gain, premultiplied, as
//! RGBA binary16 — into a buffer the host owns and wraps in the document
//! profile's extended-linearized space. No tone map applies. It is a new
//! memory plan, [`extended_display_scratch`]: [`EXTENDED_DISPLAY_BYTES_PER_PIXEL`]
//! a pixel and bookkeeping — not only the host's buffer (four binary16, 8
//! bytes a pixel, a format fact), but what the window then does with it,
//! measured:
//!
//! # The extended view's admission (measured)
//!
//! The first plan, the buffer alone (8 B/px + 1 MiB), missed by 4.4×: on the
//! Release app (`scripts/verify_v1x_hdr_photos.py --measure 3`: macOS 26.5.1,
//! `DR_WORKING_LIMIT_BYTES` 8 GiB, `MallocLargeCache=0`, one case per fresh
//! instance, `DR_EDR_HEADROOM=4`; the document opened and settled with HDR
//! Preview off, `proc_reset_footprint_interval`, then HDR Preview on and E =
//! `ri_interval_max_phys_footprint` − pre once the extended image is on screen
//! and the footprint has been still for 2 s; a baseline twin per run read 0 /
//! 0 / 0) the EDR view peaks at 33.7–36.6 B/px — the buffer, the CGImage over
//! it and Core Animation's converted copy for the canvas layer. E per run:
//! gain map 12 MP (4032×3024) 428,835,128 / 441,254,200 / 441,221,408; 48 MP
//! (8064×6048) 1,645,839,536 / 1,645,888,736 / 1,645,823,200; PQ 12 MP
//! 434,700,624 / 446,267,728 / 446,300,496; 48 MP 1,644,332,256 / 1,644,332,256
//! / 1,632,732,384. The rule, fixed before the run: `1 MiB + b·px ≥
//! ⌈1.25 × max E⌉` for every case, `b` the least integer: 46 (PQ 12 MP binds,
//! 560,867,328 + 1 MiB = 561,915,904, 1.2591× of 446,300,496; 48 MP 1.3637×
//! and 1.3650×).
//!
//! # Why ±20 stops
//!
//! A stored sample's magnitude is at most 65504 (2^16) and the smallest
//! positive value a float document can hold that still matters on screen is
//! around 2^-16 of white; 20 stops either way brings either end of that range
//! to mid-grey with room to spare, and `2^±20` is far inside f32. A wider
//! range would buy nothing the samples can hold.

use image::RgbaImage;

use crate::display_encoding::extended_display_sample;
use crate::float_document::{display_scratch, encoded_u16_gained, ToneMap};
use crate::job_control::{DrJobControl, JobContext};
use crate::precision_document::PrecisionDocument;
use crate::precision_image::{admit, pixel_count, PrecisionImage, BOOKKEEPING_BYTES};
use crate::precision_quantize::PrecisionDither;

/// The largest view exposure, in stops either way (module doc).
pub const VIEW_EXPOSURE_STOPS_MAX: f64 = 20.0;

/// How one window looks at a float projection: a tone map (`None` = the
/// document's own) and an exposure in stops.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FloatView {
    pub tone_map: Option<ToneMap>,
    pub exposure_stops: f64,
}

impl FloatView {
    /// The document's own map at 0 stops: `display_rgba8` exactly.
    pub const DOCUMENT: FloatView = FloatView {
        tone_map: None,
        exposure_stops: 0.0,
    };

    /// The wire form: an optional tone map description (the `ToneMap::parse`
    /// grammar) and the stops. Refuses a malformed map and a non-finite or
    /// out-of-range exposure by name.
    pub fn parse(tone_map_json: Option<&str>, exposure_stops: f64) -> Result<Self, String> {
        if !exposure_stops.is_finite() || exposure_stops.abs() > VIEW_EXPOSURE_STOPS_MAX {
            return Err(format!(
                "view exposure must be a finite number of stops in [-{VIEW_EXPOSURE_STOPS_MAX}, {VIEW_EXPOSURE_STOPS_MAX}]"
            ));
        }
        Ok(Self {
            tone_map: tone_map_json.map(ToneMap::parse).transpose()?,
            exposure_stops,
        })
    }

    /// The linear gain the exposure applies to RGB: `2^stops`.
    pub fn gain(self) -> f32 {
        2f64.powf(self.exposure_stops) as f32
    }
}

/// The extended view's plan a pixel: the host's buffer and what the window
/// does with it, measured (module doc, "The extended view's admission"; the
/// first plan of 8, the buffer alone, missed and was raised here).
pub const EXTENDED_DISPLAY_BYTES_PER_PIXEL: u64 = 46;

/// The working memory of [`PrecisionDocument::display_extended`] over a
/// `width` × `height` image (module doc): bookkeeping plus
/// [`EXTENDED_DISPLAY_BYTES_PER_PIXEL`] a pixel. The ONE statement of that
/// plan, admitted before anything is written.
pub(crate) fn extended_display_scratch(width: u32, height: u32) -> Result<u64, String> {
    let count = pixel_count(width, height)? as u64;
    count
        .checked_mul(EXTENDED_DISPLAY_BYTES_PER_PIXEL)
        .and_then(|bytes| bytes.checked_add(BOOKKEEPING_BYTES))
        .ok_or_else(|| "extended display working-memory calculation overflow".to_string())
}

impl PrecisionDocument {
    /// The float projection `image` as the extended-range view (module doc)
    /// at `exposure_stops`, written into `out` (exactly width·height·8 bytes,
    /// RGBA binary16 little-endian, premultiplied, linear in the document
    /// primaries). Returns the largest RGB component written. Refuses a
    /// document that is not a float document, an image that is not
    /// rgba32_float, a wrong buffer length, a bad exposure, scratch past
    /// `limit` and a cancel.
    pub fn display_extended(
        &self,
        image: &PrecisionImage,
        exposure_stops: f64,
        out: &mut [u8],
        limit: u64,
        other: u64,
        control: Option<&DrJobControl>,
    ) -> Result<f32, String> {
        let _run = control.map(DrJobControl::begin).transpose()?;
        let job = control.map_or(JobContext::NONE, JobContext::new);
        job.check()?;
        let gain = FloatView::parse(None, exposure_stops)?.gain();
        if self.float.is_none() {
            return Err("an extended-range display requires a 32-bit float document".into());
        }
        let (width, height) = image.dimensions();
        let scratch = extended_display_scratch(width, height)?;
        // The buffer's stride is its format: RGBA, four binary16 (4 × 2 bytes).
        let expected = pixel_count(width, height)? as u64 * (4 * 2);
        if out.len() as u64 != expected {
            return Err(
                "the extended-range display buffer must hold width × height × 8 bytes".into(),
            );
        }
        admit(image.retained_bytes(), scratch, other, limit)?;
        job.reserve(scratch)?;
        let runs = image
            .runs_rgba32()
            .ok_or("an extended-range display requires rgba32_float storage")?;
        let mut peak = f32::NEG_INFINITY;
        let mut at = 0;
        for (i, pixel) in runs.flatten().enumerate() {
            job.every(i)?;
            let sample = extended_display_sample(*pixel, gain);
            for (c, bits) in sample.iter().enumerate() {
                if c < 3 {
                    peak = peak.max(half::f16::from_bits(*bits).to_f32());
                }
                out[at..at + 2].copy_from_slice(&bits.to_le_bytes());
                at += 2;
            }
        }
        job.report(1.0, 5)?;
        Ok(peak)
    }

    /// The float projection `image` as display RGBA8 under `view` (module
    /// doc). Refuses a document that is not a float document, an image that
    /// is not rgba32_float, scratch past `limit` and a cancel — `display_rgba8`'s
    /// refusals.
    pub fn display_rgba8_view(
        &self,
        image: &PrecisionImage,
        view: FloatView,
        limit: u64,
        other: u64,
        control: Option<&DrJobControl>,
    ) -> Result<RgbaImage, String> {
        let _run = control.map(DrJobControl::begin).transpose()?;
        let job = control.map_or(JobContext::NONE, JobContext::new);
        job.check()?;
        let (width, height) = image.dimensions();
        let scratch = display_scratch(width, height)?;
        admit(image.retained_bytes(), scratch, other, limit)?;
        job.reserve(scratch)?;
        let policy = self
            .float
            .as_ref()
            .ok_or("tone mapping requires a 32-bit float document")?;
        let mapped = match view.tone_map {
            Some(map) => encoded_u16_gained(&policy.with_tone_map(map), image, view.gain(), job)?,
            None => encoded_u16_gained(policy, image, view.gain(), job)?,
        };
        let out = mapped.quantize_with_context(PrecisionDither::None, job)?;
        job.report(1.0, 5)?;
        Ok(out)
    }
}
