//! Typed storage adapters for the shared transform and separable resize walks.
//! Nearest and exact permutations copy u16 codes or f32 samples bit for bit,
//! including hidden RGB. Other filters accumulate premultiplied f32 and
//! quantize once at the u16 boundary; at 32-bit float they finish
//! through `straight_extended`, which drops alpha below 2^-24
//! (`EXTENDED_ALPHA_FLOOR`) and clamps RGB to the layer's own visible range (`visible_range`).
//! `precision_warp`'s float arm reads the same `visible_range`.
//!
//! A TILED u16 source whose samples are all bitwise equal (File
//! ▸ New's white background) resamples with a constant read, one destination
//! row at a time, into a band of tiles that shares one `Arc` per uniform
//! `((w, h), value)` (`TilePlane::from_rows_shared_with_job`), so it binds no
//! contiguous source copy, contiguous output or adoption copy. Every output
//! sample comes from the same walk arithmetic (`resample_rows_into`, the
//! walk's one body); only the blank pass's checkpoints go. `uniform_source`
//! is the ONE predicate `transform_image` and the geometry plan both ask,
//! and `uniform_band_bytes` is the band the plan charges in place of the
//! two copies.
//!
//! A bitwise-uniform TILED u16 raster (File ▸ New's background) resamples with a
//! constant read into a band of shared tiles (`precision_sampling::uniform_source`, the ONE
//! predicate the plan and `transform_image` ask; `resample_rows_into`, the walk's one body;
//! `TilePlane::from_rows_shared_with_job`), bit-identical to the former path, and the plan charges
//! its band (`uniform_band_bytes`) in place of the source and adoption copies, so a white 6000×4000
//! background plus a placed 6000×4000 photo turns 17° (record in `precision_geometry_budget.rs`).
use serde::Deserialize;

use crate::cmyk_image::{CmykImage, CmykSample};
use crate::doc_transform::{Sampler, SourceMap};
use crate::job_control::JobContext;
use crate::precision_image::{pixel_count, reserved_vec, PrecisionImage, PrecisionStorage};
use crate::precision_mask::PrecisionPlane;
use crate::sampling::{
    accumulate_extended, accumulate_planes, accumulate_unorm, resample_into, resample_rows_into,
    resize_into, straight_extended, straight_planes, straight_unorm, ColorRange, Kernel, Unorm,
};
use crate::tile_store::{TilePlane, TILE_EDGE};

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GeometryFilter {
    Nearest,
    Bilinear,
    Bicubic,
    Lanczos3,
}
impl GeometryFilter {
    pub(crate) fn sampler(self) -> Sampler {
        match self {
            Self::Nearest => Sampler::Nearest,
            Self::Bilinear => Sampler::Kernel(Kernel::Bilinear),
            Self::Bicubic => Sampler::Kernel(Kernel::Bicubic),
            Self::Lanczos3 => Sampler::Kernel(Kernel::Lanczos3),
        }
    }
    pub(crate) fn taps(self) -> u64 {
        match self.sampler() {
            Sampler::Nearest => 1,
            Sampler::Kernel(k) => k.taps() as u64,
        }
    }
}
fn blank<P: Copy + Default>(w: u32, h: u32, job: JobContext<'_>) -> Result<Vec<P>, String> {
    blank_value(w, h, P::default(), job)
}
fn blank_value<P: Copy>(w: u32, h: u32, value: P, job: JobContext<'_>) -> Result<Vec<P>, String> {
    let count = pixel_count(w, h)?;
    let mut out = reserved_vec(count)?;
    for i in 0..count {
        job.every(i)?;
        out.push(value);
    }
    Ok(out)
}

/// Per-channel RGB min/max over the pixels with alpha > 0: the clamp range
/// of a filtered float result (D3). Hidden RGB under alpha 0 never widens
/// it. No visible pixel gives an all-zero range.
pub(crate) fn visible_range(
    pixels: impl IntoIterator<Item = [f32; 4]>,
    job: JobContext<'_>,
) -> Result<ColorRange, String> {
    let mut range: Option<ColorRange> = None;
    for (i, pixel) in pixels.into_iter().enumerate() {
        job.every(i)?;
        if pixel[3] <= 0.0 {
            continue;
        }
        let r = range.get_or_insert(ColorRange {
            min: [pixel[0], pixel[1], pixel[2]],
            max: [pixel[0], pixel[1], pixel[2]],
        });
        for ((min, max), &value) in r.min.iter_mut().zip(&mut r.max).zip(&pixel[..3]) {
            *min = min.min(value);
            *max = max.max(value);
        }
    }
    Ok(range.unwrap_or(ColorRange {
        min: [0.0; 3],
        max: [0.0; 3],
    }))
}

/// The clamp range a float filter needs; `Nearest` copies samples and
/// never finishes an accumulator, so it skips the pass.
fn float_range(
    pixels: &[[f32; 4]],
    filter: GeometryFilter,
    job: JobContext<'_>,
) -> Result<ColorRange, String> {
    match filter.sampler() {
        Sampler::Nearest => Ok(ColorRange {
            min: [0.0; 3],
            max: [0.0; 3],
        }),
        Sampler::Kernel(_) => visible_range(pixels.iter().copied(), job),
    }
}

/// The uniform sample `transform_image` reads with a constant (module
/// doc), or `None`: `Some(value)` iff `source` is RGBA16, its plane
/// is tiled and bitwise uniform (`PrecisionImage::uniform_rgba16`), and the
/// `destination` is a valid plane. The ONE predicate the geometry plan and
/// `transform_image` share.
pub(crate) fn uniform_source(source: &PrecisionImage, destination: (u32, u32)) -> Option<[u16; 4]> {
    if source.storage() != PrecisionStorage::Rgba16Unorm {
        return None;
    }
    pixel_count(destination.0, destination.1).ok()?;
    source.uniform_rgba16()
}

/// The uniform resample's transient: one tile band's buffers
/// plus the reused row, `8·dw·(TILE_EDGE + 1)`, and 64 B per destination
/// tile for its `Arc` and index slot (`adoption_copy_bytes`' figure).
pub(crate) fn uniform_band_bytes(destination: (u32, u32)) -> Result<u64, String> {
    let (dw, dh) = (u64::from(destination.0), u64::from(destination.1));
    let tiles = dw
        .div_ceil(u64::from(TILE_EDGE))
        .checked_mul(dh.div_ceil(u64::from(TILE_EDGE)))
        .and_then(|tiles| tiles.checked_mul(64));
    dw.checked_mul(8 * (u64::from(TILE_EDGE) + 1))
        .zip(tiles)
        .and_then(|(band, tiles)| band.checked_add(tiles))
        .ok_or_else(|| "typed geometry uniform band overflow".into())
}

/// Resamples `source` into `destination` through `map`. A typed source binds
/// its transient contiguous copy (`source_copy_bytes`, or at float
/// `float_source_copy_bytes`) for the whole walk, and that copy is still live
/// when the contiguous output is tiled into the returned image
/// (`adoption_copy_bytes` / `float_adoption_copy_bytes`); the geometry plan
/// charges both. A uniform u16 source (`uniform_source`) binds neither: each
/// destination row starts at the former blank value `[0; 4]` (the walk leaves
/// non-visible and non-covering pixels unwritten), is walked by
/// `resample_rows_into` with a constant read and lands in a band of shared
/// tiles (`uniform_band_bytes`), adopted by `from_rgba16_tiles_unseeded`.
pub(crate) fn transform_image(
    source: &PrecisionImage,
    destination: (u32, u32),
    map: &SourceMap,
    filter: GeometryFilter,
    job: JobContext<'_>,
) -> Result<PrecisionImage, String> {
    // 32-bit float storage. `Nearest` — the exact canvas ops' quarter
    // turns and flips included — takes `resample_into`'s `write(di, read(si))`
    // branch: a PERMUTATION, so an extended-range component is copied bit for
    // bit. A kernel accumulates premultiplied f32 and finishes through
    // `straight_extended` against the source's visible range.
    if source.storage() == PrecisionStorage::Rgba32Float {
        let samples = source.transient_rgba32()?;
        let range = float_range(&samples, filter, job)?;
        let (w, h) = destination;
        let mut output: Vec<[f32; 4]> = blank(w, h, job)?;
        resample_into(
            source.dimensions(),
            destination,
            map,
            filter.sampler(),
            |i| samples[i],
            |acc, pixel, weight| accumulate_extended(acc, &pixel, weight),
            |acc| straight_extended(acc, &range),
            |i, pixel| output[i] = pixel,
            true,
            true,
            job,
        )?;
        return PrecisionImage::from_rgba32_extended(w, h, output, source.profile().clone(), None);
    }
    if source.storage() != PrecisionStorage::Rgba16Unorm {
        return Err("typed geometry requires RGBA16 storage".into());
    }
    let (w, h) = destination;
    if let Some(value) = uniform_source(source, destination) {
        let plane = TilePlane::from_rows_shared_with_job(
            w,
            h,
            |y, row| {
                row.fill([0; 4]);
                let start = y as usize * w as usize;
                resample_rows_into(
                    source.dimensions(),
                    destination,
                    y..y + 1,
                    map,
                    filter.sampler(),
                    |_| value,
                    |acc, pixel, weight| accumulate_unorm(acc, &pixel, weight),
                    straight_unorm,
                    |i, pixel| row[i - start] = pixel,
                    true,
                    true,
                    job,
                )
            },
            job,
        )?;
        return PrecisionImage::from_rgba16_tiles_unseeded(plane, source.profile().clone());
    }
    let samples = source.transient_rgba16()?;
    let mut output: Vec<[u16; 4]> = blank(w, h, job)?;
    resample_into(
        source.dimensions(),
        destination,
        map,
        filter.sampler(),
        |i| samples[i],
        |acc, pixel, weight| accumulate_unorm(acc, &pixel, weight),
        straight_unorm,
        |i, pixel| output[i] = pixel,
        true,
        true,
        job,
    )?;
    PrecisionImage::from_rgba16_unseeded(w, h, output, source.profile().clone(), None)
}

/// [`transform_image`]'s CMYK twin (#547 phase 4b): the same walk
/// (`resample_into`, its master) over straight CMYKA16 with the five-plane
/// premultiplied adapter (`accumulate_planes` / `straight_planes`, C = 5,
/// alpha last), so each ink plane is exactly what the RGBA16 walk makes of a
/// colour plane under the same alpha. `Nearest` is `write(di, read(si))`, a
/// bit-for-bit permutation. The source's contiguous copy
/// (`cmyk_source_copy_bytes`) is bound for the walk and the adoption
/// (`cmyk_adoption_copy_bytes`); there is no uniform-source fast path.
pub(crate) fn transform_cmyk(
    source: &CmykImage,
    destination: (u32, u32),
    map: &SourceMap,
    filter: GeometryFilter,
    job: JobContext<'_>,
) -> Result<CmykImage, String> {
    let samples = source.to_contiguous()?;
    let (w, h) = destination;
    let mut output: Vec<CmykSample> = blank(w, h, job)?;
    resample_into(
        source.dimensions(),
        destination,
        map,
        filter.sampler(),
        |i| samples[i],
        |acc: &mut [f32; 5], pixel: CmykSample, weight| accumulate_planes(acc, &pixel, weight),
        straight_planes,
        |i, pixel| output[i] = pixel,
        true,
        true,
        job,
    )?;
    CmykImage::from_contiguous(w, h, &output, job)
}

pub(crate) fn transform_plane(
    source: &PrecisionPlane,
    destination: (u32, u32),
    map: &SourceMap,
    filter: GeometryFilter,
    job: JobContext<'_>,
) -> Result<PrecisionPlane, String> {
    transform_plane_outside(source, destination, map, filter, 0, job)
}

pub(crate) fn transform_plane_outside(
    source: &PrecisionPlane,
    destination: (u32, u32),
    map: &SourceMap,
    filter: GeometryFilter,
    outside: u16,
    job: JobContext<'_>,
) -> Result<PrecisionPlane, String> {
    let (w, h) = destination;
    let mut output: Vec<u16> = blank_value(w, h, outside, job)?;
    resample_into(
        source.dimensions(),
        destination,
        map,
        filter.sampler(),
        |i| source.samples()[i],
        |acc: &mut [f32; 1], pixel, weight| {
            acc[0] += weight * (f32::from(pixel) - f32::from(outside))
        },
        |acc| u16::quantize(acc[0] + f32::from(outside)),
        |i, pixel| output[i] = pixel,
        true,
        true,
        job,
    )?;
    PrecisionPlane::from_u16(w, h, output, None)
}

fn nearest_into<P: Copy>(
    source: &[P],
    size: (u32, u32),
    destination: (u32, u32),
    out: &mut [P],
    job: JobContext<'_>,
) -> Result<(), String> {
    let (sw, sh) = size;
    let (w, h) = destination;
    // Exact rational pixel-centre mapping avoids f32 index rounding on long
    // typed planes. This is point selection only, with no color arithmetic.
    for y in 0..h {
        job.check()?;
        let sy = ((2 * u64::from(y) + 1) * u64::from(sh) / (2 * u64::from(h))) as usize;
        for x in 0..w {
            job.every(x as usize)?;
            let sx = ((2 * u64::from(x) + 1) * u64::from(sw) / (2 * u64::from(w))) as usize;
            out[y as usize * w as usize + x as usize] = source[sy * sw as usize + sx];
        }
        job.report(f64::from(y + 1) / f64::from(h), 6)?;
    }
    Ok(())
}
/// Resizes `source` to `w`×`h` (the same image for its own size). Like
/// `transform_image`, a typed resize holds its source copy (`source_copy_bytes`
/// / `float_source_copy_bytes`) while the output is tiled
/// (`adoption_copy_bytes` / `float_adoption_copy_bytes`); the separable
/// temporaries are dropped before that adoption.
pub(crate) fn resize_image(
    source: &PrecisionImage,
    w: u32,
    h: u32,
    filter: GeometryFilter,
    job: JobContext<'_>,
) -> Result<PrecisionImage, String> {
    if source.dimensions() == (w, h) {
        job.check()?;
        return Ok(source.clone());
    }
    if source.storage() == PrecisionStorage::Rgba32Float {
        let pixels = source.transient_rgba32()?;
        let range = float_range(&pixels, filter, job)?;
        let mut out: Vec<[f32; 4]> = blank(w, h, job)?;
        match filter.sampler() {
            Sampler::Nearest => nearest_into(&pixels, source.dimensions(), (w, h), &mut out, job)?,
            Sampler::Kernel(kernel) => resize_into(
                source.dimensions(),
                (w, h),
                kernel,
                |i| pixels[i],
                |acc, pixel, weight| accumulate_extended(acc, &pixel, weight),
                |acc| straight_extended(acc, &range),
                |i, pixel| out[i] = pixel,
                true,
                job,
            )?,
        }
        return PrecisionImage::from_rgba32_extended(w, h, out, source.profile().clone(), None);
    }
    if source.storage() != PrecisionStorage::Rgba16Unorm {
        return Err("typed geometry requires RGBA16 storage".into());
    }
    let pixels = source.transient_rgba16()?;
    let mut out: Vec<[u16; 4]> = blank(w, h, job)?;
    match filter.sampler() {
        Sampler::Nearest => nearest_into(&pixels, source.dimensions(), (w, h), &mut out, job)?,
        Sampler::Kernel(kernel) => resize_into(
            source.dimensions(),
            (w, h),
            kernel,
            |i| pixels[i],
            |acc, pixel, weight| accumulate_unorm(acc, &pixel, weight),
            straight_unorm,
            |i, pixel| out[i] = pixel,
            true,
            job,
        )?,
    }
    PrecisionImage::from_rgba16_unseeded(w, h, out, source.profile().clone(), None)
}
/// [`resize_image`]'s CMYK twin (#547 phase 4b): `nearest_into` (a
/// permutation) or the separable `resize_into` with the five-plane adapter
/// of [`transform_cmyk`]; the same image for its own size.
pub(crate) fn resize_cmyk(
    source: &CmykImage,
    w: u32,
    h: u32,
    filter: GeometryFilter,
    job: JobContext<'_>,
) -> Result<CmykImage, String> {
    if source.dimensions() == (w, h) {
        job.check()?;
        return Ok(source.clone());
    }
    let pixels = source.to_contiguous()?;
    let mut out: Vec<CmykSample> = blank(w, h, job)?;
    match filter.sampler() {
        Sampler::Nearest => nearest_into(&pixels, source.dimensions(), (w, h), &mut out, job)?,
        Sampler::Kernel(kernel) => resize_into(
            source.dimensions(),
            (w, h),
            kernel,
            |i| pixels[i],
            |acc: &mut [f32; 5], pixel: CmykSample, weight| accumulate_planes(acc, &pixel, weight),
            straight_planes,
            |i, pixel| out[i] = pixel,
            true,
            job,
        )?,
    }
    CmykImage::from_contiguous(w, h, &out, job)
}

pub(crate) fn resize_plane(
    source: &PrecisionPlane,
    w: u32,
    h: u32,
    filter: GeometryFilter,
    job: JobContext<'_>,
) -> Result<PrecisionPlane, String> {
    if source.dimensions() == (w, h) {
        job.check()?;
        return Ok(source.clone());
    }
    let mut out: Vec<u16> = blank(w, h, job)?;
    match filter.sampler() {
        Sampler::Nearest => {
            nearest_into(source.samples(), source.dimensions(), (w, h), &mut out, job)?
        }
        Sampler::Kernel(kernel) => resize_into(
            source.dimensions(),
            (w, h),
            kernel,
            |i| source.samples()[i],
            |acc: &mut [f32; 1], pixel, weight| acc[0] += f32::from(pixel) * weight,
            |acc| u16::quantize(acc[0]),
            |i, pixel| out[i] = pixel,
            true,
            job,
        )?,
    }
    PrecisionPlane::from_u16(w, h, out, None)
}

/// Output bytes, maximum live separable temporary bytes, and conservative
/// scalar tap work. The caller sums all outputs and takes the maximum temp.
pub(crate) fn resize_cost(
    source: (u32, u32),
    dest: (u32, u32),
    filter: GeometryFilter,
    rgba: bool,
) -> Result<(u64, u64, u64), String> {
    resize_cost_channels(source, dest, filter, if rgba { 4 } else { 1 })
}

/// [`resize_cost`] over `channels` u16 planes: 4 RGBA16, 1 a mask, 5 a CMYK
/// document's CMYKA16 (#547 phase 4b), whose separable temporary is five f32
/// planes.
pub(crate) fn resize_cost_channels(
    source: (u32, u32),
    dest: (u32, u32),
    filter: GeometryFilter,
    channels: u64,
) -> Result<(u64, u64, u64), String> {
    let n = pixel_count(dest.0, dest.1)? as u64;
    if source == dest {
        return Ok((0, 0, 0));
    }
    let output = n * channels * 2;
    if matches!(filter, GeometryFilter::Nearest) {
        return Ok((output, 0, n));
    }
    let (sw, sh) = (u64::from(source.0), u64::from(source.1));
    let (dw, dh) = (u64::from(dest.0), u64::from(dest.1));
    let taps = filter.taps();
    let vertical = taps * sh.div_ceil(dh).max(1) + 2;
    let horizontal = taps * sw.div_ceil(dw).max(1) + 2;
    let work = (sw * dh)
        .checked_mul(vertical)
        .and_then(|v| v.checked_add(dw * dh * horizontal))
        .and_then(|v| v.checked_mul(channels))
        .ok_or("typed resize work overflow")?;
    let temporary = sw * dh * channels * 4 + sw.max(sh) * 16;
    Ok((output, temporary, work))
}

/// Bytes of the transient contiguous RGBA16 copy (`PrecisionImage::transient_rgba16`)
/// that `transform_image` / `resize_image` bind of a tiled source: 8 B/px of
/// `source`.
pub(crate) fn source_copy_bytes(source: (u32, u32)) -> Result<u64, String> {
    copy_bytes(source, 8)
}

/// The float twin of [`source_copy_bytes`]: the transient contiguous RGBA32
/// copy (`PrecisionImage::transient_rgba32`), 16 B/px of `source`.
pub(crate) fn float_source_copy_bytes(source: (u32, u32)) -> Result<u64, String> {
    copy_bytes(source, 16)
}

/// Bytes of the destination's contiguous→tiled adoption copy that
/// `from_rgba16_unseeded` makes of a resample's output while the source copy
/// is still bound: 8 B/px of `dest` plus 64 B per tile (the tile's
/// `Arc` and its index slot while `index_from` builds the plane).
pub(crate) fn adoption_copy_bytes(dest: (u32, u32)) -> Result<u64, String> {
    adoption_bytes(dest, 8)
}

/// The float twin of [`adoption_copy_bytes`] (`from_rgba32_extended`):
/// 16 B/px of `dest` plus 64 B per tile.
pub(crate) fn float_adoption_copy_bytes(dest: (u32, u32)) -> Result<u64, String> {
    adoption_bytes(dest, 16)
}

/// The CMYK twin of [`source_copy_bytes`]: [`transform_cmyk`] /
/// [`resize_cmyk`]'s contiguous CMYKA16 copy (`CmykImage::to_contiguous`),
/// 10 B/px of `source`.
pub(crate) fn cmyk_source_copy_bytes(source: (u32, u32)) -> Result<u64, String> {
    copy_bytes(source, crate::cmyk_image::CMYK_BYTES_PER_PIXEL)
}

/// The CMYK twin of [`adoption_copy_bytes`] (`CmykImage::from_contiguous`):
/// 10 B/px of `dest` plus 64 B per tile.
pub(crate) fn cmyk_adoption_copy_bytes(dest: (u32, u32)) -> Result<u64, String> {
    adoption_bytes(dest, crate::cmyk_image::CMYK_BYTES_PER_PIXEL)
}

fn copy_bytes(source: (u32, u32), bytes_per_pixel: u64) -> Result<u64, String> {
    (pixel_count(source.0, source.1)? as u64)
        .checked_mul(bytes_per_pixel)
        .ok_or_else(|| "typed geometry source copy overflow".into())
}

fn adoption_bytes(dest: (u32, u32), bytes_per_pixel: u64) -> Result<u64, String> {
    use crate::tile_store::TILE_EDGE;
    let tiles = u64::from(dest.0.div_ceil(TILE_EDGE)) * u64::from(dest.1.div_ceil(TILE_EDGE));
    (pixel_count(dest.0, dest.1)? as u64)
        .checked_mul(bytes_per_pixel)
        .and_then(|bytes| bytes.checked_add(tiles.checked_mul(64)?))
        .ok_or_else(|| "typed geometry adoption copy overflow".into())
}
