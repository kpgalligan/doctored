//! New from Clipboard's TRIM: a typed clipboard payload cut down to
//! its content box — the tightest rectangle holding every pixel whose alpha
//! is above zero (`doc_export_layers::alpha_bounds16` / `alpha_bounds32`, the
//! one alpha-bounds scan), so a document made from it is sized to what was
//! copied rather than to the window it was copied from.
//!
//! The rule: the trimmed payload's samples are EXACTLY the box window of the
//! stored samples (latent RGB under zero alpha inside the box kept), its
//! coverage is windowed the same way, its offset moves by the box's origin
//! (so it still names where its top-left pixel sat on the source canvas),
//! and its resolution, byte-origin flag, tone map and editable section are
//! carried unchanged. A fully transparent payload has no box and keeps its
//! whole window, and a payload whose box already IS its window answers
//! `Ok(None)` — purity's "no identical copy" rule. The rect reads are
//! `read_rect_rgba16` / `read_rect_rgba32`, the plane's own rect read, so no
//! full-image copy is made.
//!
//! The TRIM module (`ClipboardPayload::trim`, `dr_clipboard_payload_trim`) cuts a payload to
//! its content box (`doc_export_layers::alpha_bounds16` / `alpha_bounds32`, alpha > 0): samples
//! exactly the box window (latent RGB kept, read through `read_rect_rgba16/32`), coverage windowed,
//! offset moved by the box origin (checked i32), every other field carried; `Ok(None)` for a fully
//! transparent or already-tight payload; admits bookkeeping, then `2·(samples + coverage +
//! section) + profile` before allocating.

use std::sync::Arc;

use crate::doc_export_layers::{alpha_bounds16, alpha_bounds32};
use crate::job_control::JobContext;
use crate::precision_clipboard_codec::{payload_bytes, ClipboardPayload};
use crate::precision_image::{
    admit, pixel_count, reserved_vec, PrecisionImage, PrecisionStorage, BOOKKEEPING_BYTES,
};

impl ClipboardPayload {
    /// This payload trimmed to its content box (module doc), or `Ok(None)`
    /// when it is fully transparent or already tight. Admits the bookkeeping
    /// first and, after the scan and before allocating, the trimmed payload
    /// and its encoding (`2 · (samples + coverage + section)`) plus the
    /// profile bytes the encoding repeats. An offset the box's origin would
    /// carry outside `i32` is refused by name.
    pub fn trim(&self, limit: u64, other: u64) -> Result<Option<ClipboardPayload>, String> {
        admit(0, BOOKKEEPING_BYTES, other, limit)?;
        let (sw, sh) = self.dimensions();
        let storage = self.storage();
        let bounds = match storage {
            PrecisionStorage::Rgba16Unorm => alpha_bounds16(&self.image, JobContext::NONE)?,
            PrecisionStorage::Rgba32Float => alpha_bounds32(&self.image, JobContext::NONE)?,
        };
        let Some((x, y, w, h)) = bounds else {
            return Ok(None);
        };
        if (x, y, w, h) == (0, 0, sw, sh) {
            return Ok(None);
        }
        let offset = i32::try_from(x)
            .ok()
            .and_then(|x| self.offset.0.checked_add(x))
            .zip(
                i32::try_from(y)
                    .ok()
                    .and_then(|y| self.offset.1.checked_add(y)),
            )
            .ok_or("the trimmed clipboard payload's offset overflows")?;
        let section = self.editable.as_ref().map_or(0, |s| s.len() as u64);
        let retained = payload_bytes(w, h, storage, self.coverage.is_some())
            .checked_add(section)
            .and_then(|n| n.checked_mul(2))
            .and_then(|n| n.checked_add(self.image.profile().bytes().len() as u64))
            .ok_or("clipboard payload size overflow")?;
        admit(retained, BOOKKEEPING_BYTES, other, limit)?;
        let profile = self.image.profile().clone();
        let image = match storage {
            PrecisionStorage::Rgba16Unorm => {
                let pixels = self
                    .image
                    .read_rect_rgba16(x, y, w, h)
                    .ok_or("could not allocate precision output")?;
                PrecisionImage::from_rgba16_decoded(w, h, pixels, profile, None)?
            }
            PrecisionStorage::Rgba32Float => {
                let pixels = self
                    .image
                    .read_rect_rgba32(x, y, w, h)
                    .ok_or("could not allocate precision output")?;
                PrecisionImage::from_rgba32_extended_decoded(w, h, pixels, profile, None)?
            }
        };
        let coverage = match &self.coverage {
            None => None,
            Some(plane) => {
                let mut out = reserved_vec(pixel_count(w, h)?)?;
                for row in y..y + h {
                    let start = row as usize * sw as usize + x as usize;
                    out.extend_from_slice(&plane[start..start + w as usize]);
                }
                Some(Arc::new(out))
            }
        };
        Ok(Some(ClipboardPayload {
            image,
            offset,
            resolution: self.resolution,
            coverage,
            byte_origin: self.byte_origin,
            tone_map: self.tone_map,
            editable: self.editable.clone(),
        }))
    }
}
