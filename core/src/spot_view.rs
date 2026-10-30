//! The spot channels' DISPLAY overprint on a byte composite (#547 phase
//! 1a-2, `designs/spot-channels.md`): the adapter between a
//! document's spot channels and the `spot_overprint` kernel. It converts each
//! ink's D50 Lab into the composite's encoding and runs the kernel once per
//! spot channel, in the channel list's order. Display only: a document's
//! layers and its projection never include the overprint; a host draws it
//! over the composite view and composites it into a flattened byte export.
//! A `region` / `preview_divisor` render (1c-1) overprints the same samples
//! the full render's crop does: its output (i, j) reads canvas
//! (x + i·d, y + j·d), `regional`'s rule, and the full overprint is that
//! rule at (0, 0), divisor 1.
//!
//! The ink's encoding is the document profile's, through the same
//! `ColorPipeline` the Lab document's colour adjustments read (the Lab twin
//! as the source, relative colorimetric, no black point compensation). A
//! profile the pipeline cannot target falls back to the built-in sRGB, the
//! core twin of the host's `ColorProfile.drawingSpace` fallback, so a
//! composite always shows its inks rather than failing.

use crate::doc::DrDocument;
use crate::icc::IccProfile;
use crate::icc_intent::{ColorPipeline, RenderingIntent};
use crate::lab_document::lab_twin;
use crate::spot_channel::SpotInk;
use crate::spot_overprint::overprint_rgba8;
use crate::DrImage;

/// The ink's colour as the composite's encoded RGB in `[0, 1]` under
/// `profile` (`None`, or a profile `ColorPipeline` refuses as a
/// destination, reads as the built-in sRGB). `Err` only when even the sRGB
/// pipeline cannot be prepared, which is a build defect, not an input.
/// Public only as the black-box tests' seam (their oracle runs the kernel
/// on this encoding).
#[doc(hidden)]
pub fn ink_encoded(ink: &SpotInk, profile: Option<&IccProfile>) -> Result<[f32; 3], String> {
    let twin = lab_twin();
    let intent = RenderingIntent::RelativeColorimetric;
    let pipeline = match profile.map(|p| ColorPipeline::new(&twin, p, intent, false)) {
        Some(Ok(pipeline)) => pipeline,
        _ => ColorPipeline::new(&twin, &IccProfile::srgb(), intent, false)?,
    };
    // The Lab twin's codes: L* / 100 and (a*, b* + 128) / 255
    // (`lab_document`'s encoding).
    let [l, a, b] = ink.lab();
    let codes = [
        (l / 100.0) as f32,
        ((a + 128.0) / 255.0) as f32,
        ((b + 128.0) / 255.0) as f32,
    ];
    Ok(pipeline.color(codes).map(|v| v.clamp(0.0, 1.0)))
}

/// `image` with one ink overprinted at `coverage` (exactly width × height
/// bytes, 255 = full ink), the ink encoded under `profile` as
/// [`ink_encoded`] says. `Err` for a coverage of the wrong length. Pure: the
/// input image is never changed. [`image_spot_overprint_region`] at origin
/// (0, 0), divisor 1, over the image's own canvas.
pub fn image_spot_overprint(
    image: &DrImage,
    coverage: &[u8],
    ink: &SpotInk,
    profile: Option<&IccProfile>,
) -> Result<DrImage, String> {
    let canvas = image.pixels.dimensions();
    image_spot_overprint_region(image, coverage, canvas, (0, 0), 1, ink, profile)
}

/// `image`, a `region` / `preview_divisor` render of a canvas of `canvas`
/// size, with one ink overprinted: output pixel (i, j) takes the coverage
/// at canvas (x + i·d, y + j·d), `regional`'s top-left SAMPLING rule, so a
/// region render overprints exactly the samples the full render's crop
/// does. `coverage` is the CANVAS's plane (canvas width × height bytes).
/// `Err` for a coverage of the wrong length, a zero divisor, or a region
/// whose last sample falls outside the canvas. Pure.
pub fn image_spot_overprint_region(
    image: &DrImage,
    coverage: &[u8],
    canvas: (u32, u32),
    origin: (u32, u32),
    divisor: u32,
    ink: &SpotInk,
    profile: Option<&IccProfile>,
) -> Result<DrImage, String> {
    let sampled = sampled_coverage(coverage, canvas, origin, divisor, image.pixels.dimensions())?;
    let encoded = ink_encoded(ink, profile)?;
    let mut pixels = image.pixels.clone();
    if !overprint_rgba8(
        &mut pixels,
        &sampled,
        encoded,
        f32::from(ink.solidity()) / 100.0,
    ) {
        return Err("the spot coverage does not match the image".into());
    }
    Ok(DrImage { pixels })
}

/// The coverage `output`-sized region reads (module doc of
/// [`image_spot_overprint_region`]); the canvas's own plane, borrowed, at
/// origin (0, 0) and divisor 1 over the whole canvas.
fn sampled_coverage<'a>(
    coverage: &'a [u8],
    canvas: (u32, u32),
    origin: (u32, u32),
    divisor: u32,
    output: (u32, u32),
) -> Result<std::borrow::Cow<'a, [u8]>, String> {
    let (cw, ch) = canvas;
    if coverage.len() as u64 != u64::from(cw) * u64::from(ch) {
        return Err("the spot coverage does not match the image".into());
    }
    if divisor == 0 {
        return Err("the spot overprint's divisor must be at least 1".into());
    }
    let (w, h) = output;
    if w == 0 || h == 0 {
        return Ok(std::borrow::Cow::Borrowed(&coverage[..0]));
    }
    // The last sample each axis reads, in u64 so no input overflows.
    let last = |o: u32, n: u32| u64::from(o) + u64::from(n - 1) * u64::from(divisor);
    if last(origin.0, w) >= u64::from(cw) || last(origin.1, h) >= u64::from(ch) {
        return Err("the spot overprint's region falls outside the canvas".into());
    }
    if origin == (0, 0) && divisor == 1 && output == canvas {
        return Ok(std::borrow::Cow::Borrowed(coverage));
    }
    let mut out = crate::precision_image::reserved_vec(w as usize * h as usize)?;
    for j in 0..h {
        let row = (origin.1 + j * divisor) as usize * cw as usize;
        for i in 0..w {
            out.push(coverage[row + (origin.0 + i * divisor) as usize]);
        }
    }
    Ok(std::borrow::Cow::Owned(out))
}

impl DrDocument {
    /// True when some channel is a spot channel.
    pub fn has_spot_channel(&self) -> bool {
        self.channels.iter().any(|c| c.spot.is_some())
    }

    /// Channel `i`'s ink; `None` for an alpha channel or an index out of
    /// range.
    pub fn channel_spot(&self, i: usize) -> Option<SpotInk> {
        self.channels.get(i)?.spot
    }

    /// `composite` (a canvas-sized byte rendering of this document) with
    /// every spot channel overprinted in list order, each ink encoded under
    /// the document's profile. `Ok(None)` when the document has no spot
    /// channel; `Err` when the composite is not canvas-sized.
    /// [`Self::spot_overprint_region`] at origin (0, 0), divisor 1.
    pub fn spot_overprint(&self, composite: &DrImage) -> Result<Option<DrImage>, String> {
        if self.has_spot_channel() && composite.pixels.dimensions() != (self.width, self.height) {
            return Err("the composite does not match this document's canvas".into());
        }
        self.spot_overprint_region(composite, (0, 0), 1)
    }

    /// `composite`, a `region` / `preview_divisor` render of this document
    /// whose output (i, j) is canvas (x + i·d, y + j·d), with every spot
    /// channel overprinted in list order at the same samples
    /// ([`image_spot_overprint_region`]). `Ok(None)` without a spot
    /// channel; `Err` for a zero divisor or a region past the canvas.
    pub fn spot_overprint_region(
        &self,
        composite: &DrImage,
        origin: (u32, u32),
        divisor: u32,
    ) -> Result<Option<DrImage>, String> {
        if !self.has_spot_channel() {
            return Ok(None);
        }
        let canvas = (self.width, self.height);
        let output = composite.pixels.dimensions();
        let mut pixels = composite.pixels.clone();
        for channel in &self.channels {
            let Some(ink) = channel.spot else {
                continue;
            };
            let coverage =
                sampled_coverage(channel.data.as_raw(), canvas, origin, divisor, output)?;
            let encoded = ink_encoded(&ink, Some(&self.profile))?;
            let solidity = f32::from(ink.solidity()) / 100.0;
            if !overprint_rgba8(&mut pixels, &coverage, encoded, solidity) {
                return Err("a spot channel does not match this document's canvas".into());
            }
        }
        Ok(Some(DrImage { pixels }))
    }
}
