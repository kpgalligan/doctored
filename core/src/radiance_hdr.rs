//! Radiance RGBE (`.hdr`): the pure inspect / decode / encode SEAM for the
//! 32-bit float document, over `image::codecs::hdr`.
//!
//! # The boundary
//!
//! As with `tiff_float` and `exr_float`, this module reads and writes the
//! container and decides nothing about what a sample means: `float_io` owns
//! the profile a `PRIMARIES=` line becomes and what `EXPOSURE=` and
//! `COLORCORR=` are told to the user. Both are REPORTED in the plan and
//! applied nowhere.
//!
//! # What is read
//!
//! A file starting `#?RADIANCE` or `#?RGBE`, `FORMAT=32-bit_rle_rgbe` (or no
//! FORMAT line, which is the same format), the standard `-Y H +X W`
//! orientation, flat or run-length encoded scanlines in either the new or the
//! old scheme. Each pixel is four bytes — three mantissas and a shared
//! exponent — and decodes to `m · 2^(e − 136)` per channel, zero when `e` is
//! 0. There is no alpha and no negative value in the format.
//!
//! Refused by name, before the codec is asked: any other FORMAT (`xyze` is
//! CIE XYZ, which would need a conversion this slice does not make) and any
//! other orientation (the codec has no reader for the seven others).
//!
//! # What is written
//!
//! `HdrEncoder`'s bytes — new-scheme RLE for widths 8…32768, flat otherwise —
//! with ONE header line spliced in before the blank line that ends the
//! header: `PRIMARIES=` and the eight xy numbers, red green blue white, which
//! is how the format states the space its RGB is in. The encoder takes the
//! largest channel's exponent and truncates each mantissa, so a round trip is
//! within one mantissa step (1/256 of the largest channel's power of two).

use std::io::Cursor;

use image::codecs::hdr::{HdrDecoder, HdrEncoder, HdrMetadata};
use image::{ImageDecoder, Rgb};

use crate::cancel_io::{settle, Polled};
use crate::exr_float::Chromaticities;
use crate::job_control::JobContext;
use crate::precision_image::{pixel_count, reserved_vec};

/// The one pixel format this seam reads and writes.
const FORMAT_RGBE: &str = "32-bit_rle_rgbe";

/// True when `bytes` start with either Radiance signature.
pub fn is_radiance(bytes: &[u8]) -> bool {
    bytes.starts_with(b"#?RADIANCE") || bytes.starts_with(b"#?RGBE")
}

/// What a Radiance file's header says, before a scanline is read.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RadiancePlan {
    pub width: u32,
    pub height: u32,
    /// The `PRIMARIES=` line; `None` when the file has none.
    pub primaries: Option<Chromaticities>,
    /// The product of the file's `EXPOSURE=` lines. Reported, not applied.
    pub exposure: Option<f32>,
    /// The product of the file's `COLORCORR=` lines. Reported, not applied.
    pub color_correction: Option<[f32; 3]>,
    /// The codec's byte plane plus the f32 plane a decode returns.
    pub scratch_bytes: u64,
}

fn problem(category: &str, label: &str, error: impl std::fmt::Display) -> String {
    format!("Radiance HDR {category} for {label}: {error}")
}

/// The header's text lines up to the blank line that ends it, then the
/// resolution line. `None` when the header never ends.
fn header_lines(bytes: &[u8]) -> Option<(Vec<&[u8]>, &[u8])> {
    let mut lines = Vec::new();
    let mut rest = bytes;
    loop {
        let end = rest.iter().position(|byte| *byte == b'\n')?;
        let line = &rest[..end];
        rest = &rest[end + 1..];
        if line.is_empty() {
            let end = rest.iter().position(|byte| *byte == b'\n')?;
            return Some((lines, &rest[..end]));
        }
        lines.push(line);
    }
}

/// The value of the LAST `key=` line, trimmed.
fn header_value(lines: &[&[u8]], key: &str) -> Option<String> {
    lines.iter().rev().find_map(|line| {
        let text = String::from_utf8_lossy(line);
        let (name, value) = text.split_once('=')?;
        (name.trim() == key).then(|| value.trim().to_string())
    })
}

/// The refusals the codec would word without naming the element, and the
/// `PRIMARIES=` line it does not parse.
fn header_checks(bytes: &[u8], label: &str) -> Result<Option<Chromaticities>, String> {
    let (lines, resolution) =
        header_lines(bytes).ok_or_else(|| problem("decode", label, "the header never ends"))?;
    if let Some(format) = header_value(&lines, "FORMAT") {
        if format != FORMAT_RGBE {
            return Err(problem(
                "format",
                label,
                format!("the pixel format {format} is unavailable; {FORMAT_RGBE} is supported"),
            ));
        }
    }
    let resolution = String::from_utf8_lossy(resolution);
    let mut parts = resolution.split_whitespace();
    let tags = (parts.next(), parts.nth(1));
    if tags != (Some("-Y"), Some("+X")) {
        return Err(problem(
            "format",
            label,
            format!(
                "the orientation \"{}\" is unavailable; -Y height +X width is supported",
                resolution.trim()
            ),
        ));
    }
    let Some(text) = header_value(&lines, "PRIMARIES") else {
        return Ok(None);
    };
    let numbers: Vec<f32> = text
        .split_whitespace()
        .filter_map(|number| number.parse::<f32>().ok())
        .filter(|number| number.is_finite())
        .collect();
    if numbers.len() != 8 || text.split_whitespace().count() != 8 {
        return Err(problem(
            "format",
            label,
            "PRIMARIES must state eight numbers: red, green, blue and white as x y pairs",
        ));
    }
    Ok(Some(std::array::from_fn(|i| {
        [numbers[i * 2], numbers[i * 2 + 1]]
    })))
}

/// The codec's decoder over `bytes`: strict for `#?RADIANCE`, and the
/// codec's signature-free mode for `#?RGBE`, whose first line it then reads
/// as the comment it is.
fn decoder<'a>(bytes: &'a [u8], label: &str) -> Result<HdrDecoder<Cursor<&'a [u8]>>, String> {
    let reader = Cursor::new(bytes);
    if bytes.starts_with(b"#?RADIANCE") {
        HdrDecoder::new(reader)
    } else {
        HdrDecoder::new_nonstrict(reader)
    }
    .map_err(|e| problem("decode", label, e))
}

fn plan_of(
    primaries: Option<Chromaticities>,
    meta: &HdrMetadata,
    label: &str,
) -> Result<RadiancePlan, String> {
    let count =
        pixel_count(meta.width, meta.height).map_err(|e| problem("capacity", label, e))? as u64;
    Ok(RadiancePlan {
        width: meta.width,
        height: meta.height,
        primaries,
        exposure: meta.exposure,
        color_correction: meta.color_correction.map(|(r, g, b)| [r, g, b]),
        scratch_bytes: count * 24 + 1024 * 1024,
    })
}

/// Header-only inspection. Allocates no pixels.
pub fn inspect(bytes: &[u8], label: &str) -> Result<RadiancePlan, String> {
    if !is_radiance(bytes) {
        return Err(problem("format", label, "input is not a Radiance HDR file"));
    }
    let primaries = header_checks(bytes, label)?;
    plan_of(primaries, &decoder(bytes, label)?.metadata(), label)
}

/// Decodes a Radiance file whose plan costs at most `limit` scratch bytes:
/// the plan and `width * height * 3` RGB samples, row 0 = top.
pub fn decode(bytes: &[u8], label: &str, limit: u64) -> Result<(RadiancePlan, Vec<f32>), String> {
    if !is_radiance(bytes) {
        return Err(problem("format", label, "input is not a Radiance HDR file"));
    }
    let primaries = header_checks(bytes, label)?;
    let decoder = decoder(bytes, label)?;
    let plan = plan_of(primaries, &decoder.metadata(), label)?;
    if plan.scratch_bytes > limit {
        return Err(problem(
            "capacity",
            label,
            "decoding this Radiance HDR file needs more working memory than the caller allowed",
        ));
    }
    let count = plan.width as usize * plan.height as usize;
    let mut raw: Vec<u8> = reserved_vec(count * 12)?;
    raw.resize(count * 12, 0);
    decoder
        .read_image(&mut raw)
        .map_err(|e| problem("decode", label, e))?;
    let mut samples: Vec<f32> = reserved_vec(count * 3)?;
    for bytes in raw.as_chunks::<4>().0 {
        samples.push(f32::from_ne_bytes(*bytes));
    }
    Ok((plan, samples))
}

/// Writes `rgb` (`width * height * 3` samples, row 0 = top) as a Radiance
/// RGBE file declaring `primaries`: `HdrEncoder`'s bytes with one
/// `PRIMARIES=` header line spliced in (module doc).
pub fn encode(
    width: u32,
    height: u32,
    rgb: &[f32],
    primaries: Chromaticities,
) -> Result<Vec<u8>, String> {
    encode_polled(width, height, rgb, primaries, JobContext::NONE)
}

/// [`encode`] through `cancel_io::Polled`: `HdrEncoder` writes each scanline
/// as it is encoded, so `job` is polled throughout; a cancel answers exactly
/// `operation cancelled`.
pub(crate) fn encode_polled(
    width: u32,
    height: u32,
    rgb: &[f32],
    primaries: Chromaticities,
    job: JobContext<'_>,
) -> Result<Vec<u8>, String> {
    let expected = u64::from(width) * u64::from(height) * 3;
    if width == 0 || height == 0 || rgb.len() as u64 != expected {
        return Err(
            "Radiance HDR encode: sample count differs from the declared dimensions".into(),
        );
    }
    let mut pixels: Vec<Rgb<f32>> = reserved_vec(rgb.len() / 3)?;
    for pixel in rgb.as_chunks::<3>().0 {
        pixels.push(Rgb(*pixel));
    }
    let mut out = Vec::new();
    let encoded = HdrEncoder::new(Polled::new(&mut out, job))
        .encode(&pixels, width as usize, height as usize)
        .map_err(|e| format!("Radiance HDR encode failed: {e}"));
    settle(job, encoded)?;
    drop(pixels);
    // The header ends at its first blank line; the new line goes before it.
    let blank = out
        .windows(2)
        .position(|pair| pair == b"\n\n")
        .ok_or("Radiance HDR encode failed: the encoder wrote no header")?;
    let numbers: Vec<String> = primaries
        .iter()
        .flatten()
        .map(|value| value.to_string())
        .collect();
    let line = format!("PRIMARIES= {}\n", numbers.join(" "));
    out.splice(blank + 1..blank + 1, line.bytes());
    Ok(out)
}
