//! Generative Fill on an 8-bit CMYK document (M3b phase 2): the two
//! colour legs around `gen_fill`. A 16-bit CMYK document refuses by name
//! (`doc_gen_fill::gen_fill_depth`).
//!
//! - INPUT. The plate is [`PrecisionDocument::cmyk_gen_fill_plate`], the
//!   image `cmyk_retouch_plate` builds for `RetouchSampling::All` (the one
//!   CMYK row compositor over the canvas, `cmyk_frame_image`) without its
//!   raster-anchor check, so an adjustment or group anchor works as at the
//!   other depths; Generative Expand reads its alpha. Each crop pixel's ink
//!   goes through `cmyk_color::CmykToRgb` into the built-in sRGB with
//!   `ConversionOptions::default()` (relative colorimetric with black point
//!   compensation), memoized by `ink_key`, so the provider sees sRGB.
//! - APPLY and SELECT. The provider's sRGB read-back (`gen_fill::fill_at`)
//!   goes through `cmyk_color::RgbToCmyk` with the same options, memoized by
//!   its sRGB codes (the `cmyk_artwork` glyph leg), into a `raster_cmyk`
//!   record: paper at α 0 outside the covered rect, the ink opaque inside,
//!   the u16 coverage its mask and the tag its description. The stored
//!   variations stay sRGB, which is why a table's `ink` is part of the live
//!   rule (`doc_gen_variations`).
//!
//! The plan is `cmyk_plate_scratch(All)`, the ink layer twice (`n·10`, its
//! rows and its tiles) and its mask twice (`n·2`), 128 times the profile's
//! bytes for the two legs' parsed tables and memos, and the kernel's own
//! terms (`gen_fill::scratch_bytes` at ten bytes a pixel and two a mask
//! sample).

use crate::cmyk_color::{ink_key, CmykToRgb, ConversionOptions, RgbToCmyk};
use crate::cmyk_image::{CmykImage, PAPER};
use crate::doc_gen_fill::rect;
use crate::gen_fill::{fill_at, scratch_bytes};
use crate::gen_variations::GeneratedVariations;
use crate::icc::IccProfile;
use crate::job_control::JobContext;
use crate::precision_document::PrecisionDocument;
use crate::remove_model::{native_mean, Rect};
use crate::retouch_sampling::RetouchSampling;

/// A unit ink as a u16 code (`cmyk_composite::code`'s rule).
fn code(v: f64) -> u16 {
    (v.clamp(0.0, 1.0) * 65535.0).round() as u16
}

impl PrecisionDocument {
    fn cmyk_profile_term(&self) -> Result<u64, String> {
        let policy = self.require_cmyk_policy("generative fill")?;
        Ok(128 * policy.profile_bytes().len() as u64)
    }

    /// The CMYK job's working set at `native` for `variations` pictures
    /// (module doc).
    pub(crate) fn cmyk_gen_fill_plan(
        &self,
        native: (u32, u32),
        variations: u32,
    ) -> Result<u64, String> {
        let (w, h) = self.dimensions();
        let n = u64::from(w) * u64::from(h);
        Ok(scratch_bytes(w, h, native, 10, 2, u64::from(variations))
            .saturating_add(self.cmyk_plate_scratch(RetouchSampling::All)?)
            .saturating_add(n * 10)
            .saturating_add(n * 2)
            .saturating_add(self.cmyk_profile_term()?))
    }

    /// The apply's own working set for `count` pictures: the kernel's terms,
    /// the ink layer's second copy and the profile term.
    pub(crate) fn cmyk_gen_fill_apply_plan(
        &self,
        native: (u32, u32),
        count: u64,
    ) -> Result<u64, String> {
        let (w, h) = self.dimensions();
        let n = u64::from(w) * u64::from(h);
        Ok(scratch_bytes(w, h, native, 10, 2, count)
            .saturating_add(n * 10)
            .saturating_add(self.cmyk_profile_term()?))
    }

    /// The full composite every CMYK generative input reads (module doc):
    /// the canvas through `cmyk_frame_image`, the same image
    /// `cmyk_retouch_plate(_, All, false, _)` builds (its cut excludes
    /// nothing for All), with no anchor-kind check. Its plan term is
    /// `cmyk_plate_scratch(All)`.
    pub(crate) fn cmyk_gen_fill_plate(&self, job: JobContext<'_>) -> Result<CmykImage, String> {
        let (w, h) = self.dimensions();
        self.cmyk_frame_image(((0, 0), (w, h)), None, job)
    }

    /// `native_mean`'s planes over `crop` of the CMYK plate, each ink
    /// converted to sRGB (module doc).
    pub(crate) fn cmyk_gen_fill_planes(
        &self,
        coverage: &[u16],
        crop: Rect,
        native: (u32, u32),
        image: &mut [f32],
        mask: &mut [f32],
        job: JobContext<'_>,
    ) -> Result<(), String> {
        let policy = self.require_cmyk_policy("generative fill")?.clone();
        let plate = self.cmyk_gen_fill_plate(job.range(0.0, 0.5))?;
        let srgb = IccProfile::srgb();
        let mut leg = CmykToRgb::new(&policy.profile, &srgb, ConversionOptions::default())?;
        let (cx, cy, cw, _) = crop;
        let row = |r: u32| {
            let mut out = Vec::with_capacity(cw as usize);
            for (_, run) in plate.row_segments(cy + r, cx, cx + cw) {
                for s in run {
                    let ink = [s[0], s[1], s[2], s[3]];
                    let rgb = leg.rgb(ink_key(ink), ink.map(|v| f64::from(v) / 65535.0));
                    let [r, g, b] = rgb.map(|v| v.clamp(0.0, 1.0));
                    out.push([r, g, b, f32::from(s[4]) / 65535.0]);
                }
            }
            Ok(out)
        };
        native_mean(
            coverage,
            self.dimensions().0,
            crop,
            native,
            row,
            image,
            mask,
            job.range(0.5, 1.0),
        )?;
        Ok(())
    }

    /// The ink raster of `table` from one sRGB picture (module doc).
    pub(crate) fn cmyk_gen_fill_render(
        &self,
        table: &GeneratedVariations,
        result: &[u8],
        size: (u32, u32),
        job: JobContext<'_>,
    ) -> Result<CmykImage, String> {
        let policy = self.require_cmyk_policy("generative fill")?.clone();
        let mut leg = RgbToCmyk::new(
            IccProfile::srgb(),
            &policy.profile,
            ConversionOptions::default(),
        )?;
        let (x0, y0, w, h) = rect(table.rect);
        let crop = rect(table.crop);
        CmykImage::from_rows_with_job(
            table.raster.0,
            table.raster.1,
            |y, row| {
                row.fill(PAPER);
                if (y0..y0 + h).contains(&y) {
                    for x in x0..x0 + w {
                        let rgb = fill_at(result, size, crop, x, y);
                        let key = ink_key([code(rgb[0]), code(rgb[1]), code(rgb[2]), 0]);
                        let ink = leg.ink(key, rgb);
                        row[x as usize] = [
                            code(ink[0]),
                            code(ink[1]),
                            code(ink[2]),
                            code(ink[3]),
                            65535,
                        ];
                    }
                }
                job.check()
            },
            job,
        )
    }
}
