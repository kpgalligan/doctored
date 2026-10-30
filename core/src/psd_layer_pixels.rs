//! Full-rectangle PSD raster storage at 8, 16 and 32 bits (a 32-bit plane's
//! samples are big-endian f32, decoded by `psd_import_float`). The bounded record walk lends the
//! original channel bytes; Adobe's exclusive signed bounds remain authoritative.
//! Raw samples copy directly, PackBits rows come from `psd_framing::rle_rows` and the one
//! in-crate decoder, and ZIP planes from `psd_zip`. Ordinary masks share its channel validation.
//! A PSB's planes (`LayerPixels::large`) carry u32 row counts and the 300000-pixel side limit.
//!
//! The same walk lends original channel slices/exclusive signed bounds to an admitted
//! full-rectangle raster seam; a tiny metadata-only decoder view preserves names/tree/properties
//! without its channel/geometry bugs. Raw copies and the in-crate PackBits and ZIP decoders
//! retain exact straight RGBA, absent alpha255 and off-canvas storage. Per-layer/aggregate native
//! caps and a conservative decode envelope (`psd_layer_pixels::import_envelope_bytes()`, the
//! process working limit) precede metadata/codec/output allocation (the FFI source read and
//! unrelated host state are outside it). Reversed/one-side-zero rectangles, incomplete raster
//! storage and malformed channel rows refuse; a both-sides-zero rectangle is a pixel-less record
//! handled per record (omitted adjustment/fill or empty layer), and such a record never carries the
//! "pixels irrelevant" marker. The decode envelope's arithmetic is the shared
//! `psd_layer_pixels::peak_forecast` (now scaled by `sample_bytes`), evaluated again by the export
//! preflight on the file it is about to write, so import admission and export disclosure can never
//! drift apart.

use image::{Rgba, RgbaImage};

use crate::cmyk_image::CmykSample;
use crate::doc::MAX_PIXELS;
use crate::drdc::MAX_DRDC_TOTAL_LAYER_PIXELS;
use crate::precision_image::{gib_text, process_working_limit};
use crate::psd_framing::{rle_rows, unpack_packbits, validate_packbits};

pub(crate) struct Channel<'a> {
    pub kind: i16,
    pub bytes: &'a [u8], // compression word, optional row table, encoded samples
}

pub(crate) struct LayerPixels<'a> {
    pub bounds: [i32; 4], // top, left, exclusive bottom, exclusive right
    pub channels: Vec<Channel<'a>>,
    /// The record came from a PSB: RLE row counts are u32 and a side may
    /// reach 300000 pixels (`psd_framing`).
    pub large: bool,
}

/// One decoded 8-, 16- or 32-bit plane's bytes, row-major, for any admitted
/// compression word: Raw is borrowed, RLE and ZIP are decoded here. `bytes`
/// starts at the compression word, exactly as the record walk lent it.
pub(crate) fn plane_bytes<'b>(
    bytes: &'b [u8],
    row_bytes: usize,
    height: usize,
    sample_bytes: usize,
    large: bool,
) -> Result<std::borrow::Cow<'b, [u8]>, String> {
    let word = u16::from_be_bytes(bytes[..2].try_into().unwrap());
    Ok(match word {
        0 => std::borrow::Cow::Borrowed(&bytes[2..]),
        1 => {
            let mut out = Vec::with_capacity(row_bytes * height);
            for row in rle_rows(&bytes[2..], height, large)? {
                out.extend_from_slice(&unpack_packbits(row, row_bytes)?);
            }
            std::borrow::Cow::Owned(out)
        }
        // One zlib stream per channel, with the per-row prediction on word 3
        // (Photoshop's byte-plane form at 32 bits, `psd_zip`).
        _ if sample_bytes == 4 => std::borrow::Cow::Owned(crate::psd_zip::unpack_zip32(
            &bytes[2..],
            row_bytes,
            height,
            word == 3,
        )?),
        _ if sample_bytes == 2 => std::borrow::Cow::Owned(crate::psd_zip::unpack_zip16(
            &bytes[2..],
            row_bytes,
            height,
            word == 3,
        )?),
        _ => std::borrow::Cow::Owned(crate::psd_zip::unpack_zip8(
            &bytes[2..],
            row_bytes,
            height,
            word == 3,
        )?),
    })
}

/// A mask plane's decoded bytes, through the same seam raster planes use.
pub(crate) fn mask_plane_bytes(
    bytes: &[u8],
    width: u32,
    height: u32,
    sample_bytes: usize,
    large: bool,
) -> Result<std::borrow::Cow<'_, [u8]>, String> {
    plane_bytes(
        bytes,
        width as usize * sample_bytes,
        height as usize,
        sample_bytes,
        large,
    )
}

impl LayerPixels<'_> {
    /// Photoshop's pixel-less record: the rectangle is zero on BOTH sides
    /// (`right == left` and `bottom == top`), which is what an adjustment,
    /// fill or empty layer writes. Any other zero, negative or oversized side
    /// stays malformed and refuses through [`dimensions`](Self::dimensions).
    pub fn is_empty(&self) -> bool {
        let [top, left, bottom, right] = self.bounds;
        right == left && bottom == top
    }

    pub fn dimensions(&self) -> Result<(u32, u32), String> {
        let [top, left, bottom, right] = self.bounds;
        let w = i64::from(right) - i64::from(left);
        let h = i64::from(bottom) - i64::from(top);
        // The format's own side limit, the same for every compression: 30000
        // in a classic file, 300000 in a PSB.
        let side: i64 = if self.large { 300_000 } else { 30_000 };
        if w <= 0 || h <= 0 || w > side || h > side {
            return Err(format!(
                "PSD raster rectangle is empty, reversed or exceeds {side} pixels per side"
            ));
        }
        if w as u64 * h as u64 > MAX_PIXELS {
            return Err("PSD raster rectangle exceeds the 100 MP editing limit".into());
        }
        Ok((w as u32, h as u32))
    }

    pub fn validate(&self, color: u16, sample_bytes: u64) -> Result<(), String> {
        // A pixel-less record has no rectangle to admit; its channels must
        // still frame exactly (a Raw plane is the bare compression word, an
        // RLE one that word and an empty row table), so stray plane bytes
        // still refuse.
        let (width, height) = if self.is_empty() {
            (0, 0)
        } else {
            self.dimensions()?
        };
        let required: &[i16] = match color {
            1 => &[0],
            // CMYK (#547 phase 3): all four ink planes.
            4 => &[0, 1, 2, 3],
            _ => &[0, 1, 2],
        };
        if required
            .iter()
            .any(|kind| !self.channels.iter().any(|c| c.kind == *kind))
        {
            return Err("PSD raster is missing a required color channel".into());
        }
        for channel in &self.channels {
            if channel.kind < -1 {
                continue; // framed, disclosed mask payload; no mask interpretation
            }
            if color == 1 && channel.kind > 0 {
                return Err("PSD grayscale raster has unexpected color channels".into());
            }
            if color == 4 && channel.kind > 3 {
                return Err("PSD CMYK raster has unexpected color channels".into());
            }
            validate_channel(channel.bytes, width, height, sample_bytes, self.large)?;
        }
        Ok(())
    }

    /// All layers/rows and the complete peak envelope have already passed
    /// admission. Store straight samples without alpha multiplication or clipping.
    pub fn decode(&self, color: u16) -> Result<RgbaImage, String> {
        let (width, height) = self.dimensions()?;
        let mut out = RgbaImage::from_pixel(width, height, Rgba([0, 0, 0, 255]));
        for channel in &self.channels {
            if channel.kind < -1 {
                continue;
            }
            let component = if channel.kind == -1 {
                3
            } else {
                channel.kind as usize
            };
            let insert = |pixel: &mut Rgba<u8>, value| {
                pixel[component] = value;
                if color == 1 && component == 0 {
                    pixel[1] = value;
                    pixel[2] = value;
                }
            };
            let plane = plane_bytes(
                channel.bytes,
                width as usize,
                height as usize,
                1,
                self.large,
            )?;
            for (pixel, value) in out.pixels_mut().zip(plane.iter()) {
                insert(pixel, *value);
            }
        }
        Ok(out)
    }
}

/// The 16-bit twin of [`LayerPixels::decode`]: straight RGBA16 samples, hidden
/// RGB kept, absent alpha defaulting to 65535, through the same plane seam at
/// a doubled row width; samples are big-endian pairs.
impl LayerPixels<'_> {
    pub fn decode16(&self) -> Result<Vec<[u16; 4]>, String> {
        let (width, height) = self.dimensions()?;
        let row_bytes = width as usize * 2;
        let mut out = vec![[0u16, 0, 0, 65535]; width as usize * height as usize];
        for channel in &self.channels {
            if channel.kind < -1 {
                continue;
            }
            let component = if channel.kind == -1 {
                3
            } else {
                channel.kind as usize
            };
            let plane = plane_bytes(channel.bytes, row_bytes, height as usize, 2, self.large)?;
            for (pixel, sample) in out.iter_mut().zip(plane.chunks_exact(2)) {
                pixel[component] = u16::from_be_bytes([sample[0], sample[1]]);
            }
        }
        Ok(out)
    }
}

/// A gray raster's straight samples as RGBA16 with R = G = B (#547 phase
/// 3a): channel 0 is the gray plane, `-1` the transparency, and a record
/// with none is opaque. An 8-bit sample is its code ×257. `validate` has
/// already required channel 0 and refused any other colour channel.
impl LayerPixels<'_> {
    pub fn decode_gray(&self, sample_bytes: usize) -> Result<Vec<[u16; 4]>, String> {
        let (width, height) = self.dimensions()?;
        let row_bytes = width as usize * sample_bytes;
        let mut out = vec![[0u16, 0, 0, 65535]; width as usize * height as usize];
        for channel in &self.channels {
            if !matches!(channel.kind, -1 | 0) {
                continue;
            }
            let plane = plane_bytes(
                channel.bytes,
                row_bytes,
                height as usize,
                sample_bytes,
                self.large,
            )?;
            for (pixel, sample) in out.iter_mut().zip(plane.chunks_exact(sample_bytes)) {
                let value = if sample_bytes == 2 {
                    u16::from_be_bytes([sample[0], sample[1]])
                } else {
                    u16::from(sample[0]) * 257
                };
                if channel.kind == 0 {
                    pixel[..3].fill(value);
                } else {
                    pixel[3] = value;
                }
            }
        }
        Ok(out)
    }
}

/// A CMYK raster's straight CMYKA16 INK (#547 phase 3): a PSD stores each ink
/// plane as `max − ink`, flipped here; channel `-1` is the transparency, and
/// a record with none is opaque. An 8-bit sample is its code ×257. `validate`
/// has already required the four ink planes.
impl LayerPixels<'_> {
    pub fn decode_cmyk(&self, sample_bytes: usize) -> Result<Vec<CmykSample>, String> {
        let (width, height) = self.dimensions()?;
        let row_bytes = width as usize * sample_bytes;
        let mut out = vec![[0u16, 0, 0, 0, 65535]; width as usize * height as usize];
        for channel in &self.channels {
            if channel.kind < -1 {
                continue;
            }
            let (component, inverted) = if channel.kind == -1 {
                (4, false)
            } else {
                (channel.kind as usize, true)
            };
            let plane = plane_bytes(
                channel.bytes,
                row_bytes,
                height as usize,
                sample_bytes,
                self.large,
            )?;
            for (pixel, sample) in out.iter_mut().zip(plane.chunks_exact(sample_bytes)) {
                let value = if sample_bytes == 2 {
                    u16::from_be_bytes([sample[0], sample[1]])
                } else {
                    u16::from(sample[0]) * 257
                };
                pixel[component] = if inverted { 65535 - value } else { value };
            }
        }
        Ok(out)
    }
}

/// Shared exact Raw/PackBits/ZIP channel framing for raster and ordinary mask
/// planes. The caller supplies the appropriate independent rectangle
/// dimensions, the container's bytes per sample, which scale both a Raw
/// plane's length and an RLE row's decoded width, and whether the container
/// is a PSB, whose RLE row counts are u32.
pub(crate) fn validate_channel(
    bytes: &[u8],
    width: u32,
    height: u32,
    sample_bytes: u64,
    large: bool,
) -> Result<(), String> {
    if bytes.len() < 2 {
        return Err("PSD channel omits compression bytes".into());
    }
    let row_bytes = width as usize * sample_bytes as usize;
    match u16::from_be_bytes(bytes[..2].try_into().unwrap()) {
        0 if bytes.len() == 2 + row_bytes * height as usize => {}
        0 => return Err("PSD raw layer channel length mismatch".into()),
        1 => {
            for row in rle_rows(&bytes[2..], height as usize, large)? {
                validate_packbits(row, row_bytes)?;
            }
        }
        // ZIP (2) and ZIP-with-prediction (3): Photoshop writes them for a
        // 16-bit file's layer and mask planes, and other writers at 8 bits.
        // Only the zlib header is framed here; the inflated length is checked
        // by the one decoder (`psd_zip`), which allocates no more than this
        // plane.
        2 | 3 => crate::psd_zip::validate_zip_header(&bytes[2..])?,
        _ => return Err(
            "PSD layer channel uses unsupported compression; only Raw, RLE and ZIP are supported"
                .into(),
        ),
    }
    Ok(())
}

/// Bound retained storage before any wrapper, codec channel copy or RGBA plane
/// is allocated. Source bytes have already been read by the FFI; this is not an
/// admission of that read or of unrelated documents retained by the host.
#[allow(clippy::too_many_arguments)]
pub(crate) fn admit(
    layers: &[LayerPixels<'_>],
    // Parallel to `layers`: a record Import Layers drops entirely
    // (`GroupRecords::omitted`) retains nothing at all.
    omitted: &[bool],
    // Parallel to `layers`: a retained record that imports as a canvas-sized
    // raster (`GroupRecords::canvas_raster`), charged as the canvas whatever
    // its rectangle.
    canvas_rasters: &[bool],
    groups: usize,
    source_bytes: usize,
    metadata_bytes: usize,
    canvas_pixels: u64,
    masks: crate::psd_raster_masks::MaskBudget,
    sample_bytes: u64,
    // The planes a raster decodes to: 4 (RGBA or gray), 5 for CMYK.
    planes: u64,
    // Bytes beside the forecast a carried fill's render prepares (a solid's
    // encoders, `GroupRecords::fill_scratch_bytes`).
    extra_scratch: u64,
) -> Result<(), String> {
    let mut pixels = groups as u64; // native group placeholders are also stored
    let (mut largest, mut encoded) = (masks.largest, masks.encoded);
    for (index, layer) in layers.iter().enumerate() {
        // An omitted record becomes nothing; a canvas raster (a retained
        // pixel-less record, a carried fill) is the canvas, and its own
        // channels are never decoded.
        if omitted.get(index).copied().unwrap_or(false) {
            continue;
        }
        if layer.is_empty() || canvas_rasters.get(index).copied().unwrap_or(false) {
            pixels += canvas_pixels;
            largest = largest.max(canvas_pixels);
            continue;
        }
        let (w, h) = layer.dimensions()?;
        let count = u64::from(w) * u64::from(h);
        pixels += count;
        largest = largest.max(count);
        encoded = encoded.max(
            layer
                .channels
                .iter()
                .map(|c| c.bytes.len() as u64)
                .max()
                .unwrap_or(0),
        );
    }
    if pixels > MAX_DRDC_TOTAL_LAYER_PIXELS {
        return Err("PSD aggregate layer pixels exceed the 400 MP native limit".into());
    }
    // Source + conservative metadata wrapper/name/parser copies (8x the exact
    // framed metadata) + all output planes + one channel's wrapper/codec copy
    // and RGBA scratch (largest raster OR mask), plus retained raw masks.
    // Report also decodes an independent saved composite: reserve three
    // entire input-sized encoded copies and its RGBA, even though that stage
    // does not overlap retained layer output in this code. The composite's
    // borrowed row list (`psd_framing::rle_rows`, 16 bytes a row) is bounded
    // twice over: a classic file has at most 56 channels * 30000 rows of it
    // (27 MiB), inside the fixed 64 MiB beside the bounded
    // record/name/container overhead, and a PSB's rows each cost 4 table
    // bytes in the file, so the list is at most the source bytes times four,
    // which the first term already charges. This envelope applies to admitted
    // layered import, not the
    // independent composite choice after layered admission itself refuses.
    // Saturation makes arithmetic overflow a refusal, never a smaller forecast.
    let peak = peak_forecast(PeakInputs {
        source_bytes: source_bytes as u64,
        metadata_bytes: metadata_bytes as u64,
        pixels,
        mask_pixels: masks.pixels,
        largest,
        encoded,
        canvas_pixels,
        sample_bytes,
        planes,
    })
    .saturating_add(extra_scratch);
    let envelope = import_envelope_bytes();
    if peak > envelope {
        return Err(format!(
            "PSD layered import exceeds the {} GiB decode memory envelope",
            gib_text(envelope)
        ));
    }
    Ok(())
}

/// The layered-import decode envelope [`admit`] refuses above: the process
/// working limit. The export preflight evaluates the same forecast on the
/// file it is ABOUT to write, so a document Doctored could not reopen
/// layered is reported rather than refused (`psd_export`).
pub(crate) fn import_envelope_bytes() -> u64 {
    process_working_limit()
}

/// The measured inputs of one layered decode: the file's own byte count, the
/// exact framed record metadata, retained layer/mask/group plane pixels, the
/// largest single plane, the largest encoded channel and the canvas.
pub(crate) struct PeakInputs {
    pub source_bytes: u64,
    pub metadata_bytes: u64,
    pub pixels: u64,
    pub mask_pixels: u64,
    pub largest: u64,
    pub encoded: u64,
    pub canvas_pixels: u64,
    /// 1 for an 8-bit container, 2 for a 16-bit one: every retained plane and
    /// decode copy is that many times larger.
    pub sample_bytes: u64,
    /// The planes one decoded pixel holds: 4 for RGB or gray (RGBA), 5 for
    /// CMYK (four inks and alpha, #547 phase 3).
    pub planes: u64,
}

/// The ONE peak-memory forecast, shared by import admission and the export
/// preflight so neither can drift from the other. Saturation makes arithmetic
/// overflow a larger forecast, never a smaller one.
pub(crate) fn peak_forecast(inputs: PeakInputs) -> u64 {
    let sample = inputs.sample_bytes.max(1);
    let planes = inputs.planes.max(1);
    inputs
        .source_bytes
        .saturating_mul(4)
        .saturating_add(inputs.metadata_bytes.saturating_mul(8))
        .saturating_add(inputs.pixels.saturating_mul(planes).saturating_mul(sample))
        .saturating_add(inputs.mask_pixels.saturating_mul(sample))
        .saturating_add(inputs.largest.saturating_mul(planes).saturating_mul(sample))
        .saturating_add(inputs.encoded.saturating_mul(2))
        .saturating_add(inputs.canvas_pixels.saturating_mul(4))
        .saturating_add(64 * 1024 * 1024)
}
