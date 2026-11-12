//! OpenEXR: the pure inspect / decode / encode SEAM for the 32-bit float
//! document, over the `exr` crate.
//!
//! # The boundary
//!
//! Like `tiff_float`, this module answers only "can this crate read and
//! write the container exactly". What a sample MEANS — the finite rule, the
//! alpha convention, which primaries become which profile — is `float_io`'s,
//! so samples leave here as the file stores them: RGBA in the file's own
//! primaries, alpha ASSOCIATED (the OpenEXR convention), NaN and infinity
//! counted in [`ExrImage::non_finite`] and passed through unchanged.
//!
//! # What is read
//!
//! The first part that is not deep and has channels named exactly `R`, `G`
//! and `B` (plus `A` when it has one), at its largest resolution level,
//! scanline or tiled, any compression the `exr` crate decodes, HALF, FLOAT or
//! UINT samples widened to f32 (a half widens exactly). The part's DATA
//! window is the image; the DISPLAY window and the data window's position in
//! it are reported in the plan and applied by the caller.
//!
//! Refused by name: a file whose parts are all deep, a file with no part
//! holding R, G and B (grayscale and luminance-chroma files), and a file that
//! mixes a deep part with flat ones — the `exr` crate validates every header
//! before it reads a block and has no deep reader, so the flat part beside a
//! deep one cannot be reached through it. Subsampled channels are refused by
//! the crate itself.
//!
//! # What is written
//!
//! One part, RGBA, ZIP in 16-line scanline blocks, increasing Y, FLOAT or
//! HALF samples, with the `chromaticities` attribute. Increasing line order
//! makes the crate write its blocks in file order, so the bytes are a pure
//! function of the samples.
//!
//! # Why the plan is separate from the decode
//!
//! [`inspect`] reads the headers only and reports what a decode costs
//! ([`ExrPlan::scratch_bytes`]: the RGBA f32 plane this returns plus the
//! crate's block buffers), so a caller admits the allocation before it
//! happens — `tiff_float::inspect` / `decode`'s split.

use std::cell::Cell;
use std::io::Cursor;

use exr::image::{Blocks, Encoding, Image, SpecificChannels};
use exr::math::Vec2;
use exr::meta::attribute::{Compression, LineOrder};
use exr::meta::header::Header;
use exr::meta::MetaData;
use exr::prelude::{ReadChannels, ReadLayers, WritableImage};

use crate::cancel_io::{settle, Polled};
use crate::job_control::JobContext;
use crate::precision_image::{pixel_count, reserved_vec};
use crate::tiff_float::FloatSampleType;

/// xy chromaticities, in this order everywhere: red, green, blue, white.
pub type Chromaticities = [[f32; 2]; 4];

/// The four bytes every OpenEXR file starts with.
const MAGIC: [u8; 4] = [0x76, 0x2f, 0x31, 0x01];

/// True when `bytes` start with the OpenEXR magic number.
pub fn is_exr(bytes: &[u8]) -> bool {
    bytes.starts_with(&MAGIC)
}

/// What an OpenEXR file's headers say about the part a decode would open.
#[derive(Clone, Debug, PartialEq)]
pub struct ExrPlan {
    /// The opened part's DATA window.
    pub width: u32,
    pub height: u32,
    /// The DISPLAY window's size.
    pub canvas: (u32, u32),
    /// The data window's origin minus the display window's.
    pub offset: (i32, i32),
    /// True when the part has an `A` channel.
    pub alpha: bool,
    /// The opened part's index, and how many parts the file holds.
    pub part: usize,
    pub parts: usize,
    pub part_name: Option<String>,
    /// The `chromaticities` attribute; `None` when the file has none, which
    /// OpenEXR defines as Rec.709 primaries.
    pub chromaticities: Option<Chromaticities>,
    pub pixel_aspect: f32,
    /// The RGBA f32 plane a decode returns plus the codec's working copies.
    pub scratch_bytes: u64,
}

/// One decoded part: `samples` is RGBA as stored (associated alpha), row 0 =
/// top, `A = 1` when the part has no alpha channel.
#[derive(Clone, Debug)]
pub struct ExrImage {
    pub plan: ExrPlan,
    pub samples: Vec<f32>,
    /// How many samples were NaN or infinite. They are in `samples`
    /// unchanged; this is the disclosure, not a repair.
    pub non_finite: u64,
}

fn problem(category: &str, label: &str, error: impl std::fmt::Display) -> String {
    format!("OpenEXR {category} for {label}: {error}")
}

/// True when `header` has a channel named exactly `name`.
fn has_channel(header: &Header, name: &str) -> bool {
    header
        .channels
        .list
        .iter()
        .any(|channel| channel.name.eq(name))
}

/// Header-only inspection. Allocates no pixels.
pub fn inspect(bytes: &[u8], label: &str) -> Result<ExrPlan, String> {
    if !is_exr(bytes) {
        return Err(problem("format", label, "input is not an OpenEXR file"));
    }
    let meta = MetaData::read_from_buffered(Cursor::new(bytes), false)
        .map_err(|e| problem("decode", label, e))?;
    let headers = &meta.headers;
    if headers.is_empty() {
        return Err(problem("format", label, "the file has no part"));
    }
    let deep = headers.iter().filter(|header| header.deep).count();
    if deep == headers.len() {
        return Err(problem(
            "format",
            label,
            "the file holds only deep data, which is unavailable",
        ));
    }
    if deep > 0 {
        return Err(problem(
            "format",
            label,
            "the file mixes deep parts with flat ones, which is unavailable",
        ));
    }
    MetaData::validate(headers, false).map_err(|e| problem("decode", label, e))?;
    let (part, header) = headers
        .iter()
        .enumerate()
        .find(|(_, header)| ["R", "G", "B"].iter().all(|name| has_channel(header, name)))
        .ok_or_else(|| {
            problem(
                "format",
                label,
                "no part has R, G and B channels; grayscale and luminance-chroma files are unavailable",
            )
        })?;
    let side = |value: usize| {
        u32::try_from(value)
            .map_err(|_| problem("capacity", label, "a window side exceeds 32 bits"))
    };
    let (width, height) = (
        side(header.layer_size.width())?,
        side(header.layer_size.height())?,
    );
    let display = header.shared_attributes.display_window;
    let canvas = (side(display.size.width())?, side(display.size.height())?);
    let count = pixel_count(width, height).map_err(|e| problem("capacity", label, e))? as u64;
    pixel_count(canvas.0, canvas.1).map_err(|e| problem("capacity", label, e))?;
    let position = header.own_attributes.layer_position;
    let offset = (
        position
            .x()
            .checked_sub(display.position.x())
            .ok_or_else(|| problem("format", label, "the data window lies outside 32 bits"))?,
        position
            .y()
            .checked_sub(display.position.y())
            .ok_or_else(|| problem("format", label, "the data window lies outside 32 bits"))?,
    );
    let chromaticities = header.shared_attributes.chromaticities.map(|c| {
        [
            [c.red.x(), c.red.y()],
            [c.green.x(), c.green.y()],
            [c.blue.x(), c.blue.y()],
            [c.white.x(), c.white.y()],
        ]
    });
    Ok(ExrPlan {
        width,
        height,
        canvas,
        offset,
        alpha: has_channel(header, "A"),
        part,
        parts: headers.len(),
        part_name: header
            .own_attributes
            .layer_name
            .as_ref()
            .map(ToString::to_string),
        chromaticities,
        pixel_aspect: header.shared_attributes.pixel_aspect,
        scratch_bytes: count * 32 + 1024 * 1024,
    })
}

/// Decodes the part [`inspect`] plans, when it costs at most `limit` scratch
/// bytes.
pub fn decode(bytes: &[u8], label: &str, limit: u64) -> Result<ExrImage, String> {
    let plan = inspect(bytes, label)?;
    if plan.scratch_bytes > limit {
        return Err(problem(
            "capacity",
            label,
            "decoding this OpenEXR file needs more working memory than the caller allowed",
        ));
    }
    let (width, height) = (plan.width as usize, plan.height as usize);
    let length = width * height * 4;
    let mut plane: Vec<f32> = reserved_vec(length)?;
    plane.resize(length, 0.0);
    // The crate's `create` callback is `Fn`, so the plane it hands out is
    // taken from a cell; a part of another size than the planned one gets an
    // empty plane, every write misses, and the size check below refuses.
    let prepared = Cell::new(Some(plane));
    let decoded = exr::image::read::read()
        .no_deep_data()
        .largest_resolution_level()
        .rgba_channels(
            |size: Vec2<usize>, _| {
                if (size.width(), size.height()) == (width, height) {
                    prepared.take().unwrap_or_default()
                } else {
                    Vec::new()
                }
            },
            move |pixels: &mut Vec<f32>,
                  position: Vec2<usize>,
                  (r, g, b, a): (f32, f32, f32, f32)| {
                let at = (position.y() * width + position.x()) * 4;
                if let Some(slot) = pixels.get_mut(at..at + 4) {
                    slot.copy_from_slice(&[r, g, b, a]);
                }
            },
        )
        .first_valid_layer()
        .all_attributes()
        .from_buffered(Cursor::new(bytes))
        .map_err(|e| problem("decode", label, e))?;
    let part = decoded.layer_data;
    let samples = part.channel_data.pixels;
    if (part.size.width(), part.size.height()) != (width, height) || samples.len() != length {
        return Err(problem(
            "decode",
            label,
            "sample count differs from the declared dimensions",
        ));
    }
    let non_finite = samples.iter().filter(|v| !v.is_finite()).count() as u64;
    Ok(ExrImage {
        plan,
        samples,
        non_finite,
    })
}

/// Writes `rgba` (`width * height * 4` samples, row 0 = top, stored as they
/// are — the caller has already multiplied by alpha) as a one-part RGBA
/// OpenEXR file: ZIP scanlines, `samples` wide, declaring `chromaticities`.
/// A half sample is the nearest half to its f32 (ties to even).
pub fn encode(
    width: u32,
    height: u32,
    rgba: &[f32],
    samples: FloatSampleType,
    chromaticities: Chromaticities,
) -> Result<Vec<u8>, String> {
    encode_polled(
        width,
        height,
        rgba,
        samples,
        chromaticities,
        JobContext::NONE,
    )
}

/// [`encode`] through `cancel_io::Polled`: the `exr` writer hands each
/// compressed block to the sink as it is finished, so `job` is polled as the
/// file grows; a cancel answers exactly `operation cancelled`.
pub(crate) fn encode_polled(
    width: u32,
    height: u32,
    rgba: &[f32],
    samples: FloatSampleType,
    chromaticities: Chromaticities,
    job: JobContext<'_>,
) -> Result<Vec<u8>, String> {
    let expected = u64::from(width) * u64::from(height) * 4;
    if width == 0 || height == 0 || rgba.len() as u64 != expected {
        return Err("OpenEXR encode: sample count differs from the declared dimensions".into());
    }
    let size = (width as usize, height as usize);
    let stride = width as usize;
    let pixel = |position: Vec2<usize>| {
        let at = (position.y() * stride + position.x()) * 4;
        (rgba[at], rgba[at + 1], rgba[at + 2], rgba[at + 3])
    };
    let encoding = Encoding {
        compression: Compression::ZIP16,
        blocks: Blocks::ScanLines,
        line_order: LineOrder::Increasing,
    };
    let point = |xy: [f32; 2]| Vec2(xy[0], xy[1]);
    let declared = exr::meta::attribute::Chromaticities {
        red: point(chromaticities[0]),
        green: point(chromaticities[1]),
        blue: point(chromaticities[2]),
        white: point(chromaticities[3]),
    };
    let mut buffer = Cursor::new(Vec::new());
    let mut out = Polled::new(&mut buffer, job);
    let written = match samples {
        FloatSampleType::F32 => {
            let mut image =
                Image::from_encoded_channels(size, encoding, SpecificChannels::rgba(pixel));
            image.attributes.chromaticities = Some(declared);
            image.write().to_buffered(&mut out)
        }
        FloatSampleType::F16 => {
            let half = |position: Vec2<usize>| {
                let (r, g, b, a) = pixel(position);
                (
                    half::f16::from_f32(r),
                    half::f16::from_f32(g),
                    half::f16::from_f32(b),
                    half::f16::from_f32(a),
                )
            };
            let mut image =
                Image::from_encoded_channels(size, encoding, SpecificChannels::rgba(half));
            image.attributes.chromaticities = Some(declared);
            image.write().to_buffered(&mut out)
        }
    };
    settle(
        job,
        written.map_err(|e| format!("OpenEXR encode failed: {e}")),
    )?;
    Ok(buffer.into_inner())
}
