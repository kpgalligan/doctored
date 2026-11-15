//! The byte native writer's plane fields (13C-2a): each layer, channel and
//! mask plane written to the sink as it is read, never assembled. A tiled
//! plane is first offered to the sink (`NativeSink::raster`, which only the
//! tile pool's skeleton sink takes, as keys); otherwise a layer or channel is
//! a length-prefixed PNG streamed row by row (`png_rows8`) and a mask its raw
//! rows. Each `put_*_png` and [`put_runs`] is the ONE writer of its field,
//! shared by `DrDocument::write_native` and the tile pool's rebuild
//! (`doc_tile_pool`), so a rebuilt record is the saved record byte for byte.

use super::MAX_DRDC_PNG_LEN;
use crate::gray_raster::GrayRaster;
use crate::job_control::JobContext;
use crate::layer_raster::LayerRaster;
use crate::native_stream::{NativeSink, RasterRef};
use crate::png_rows8;

/// A byte layer's pixel field: offered to the sink as tiles first, else its
/// PNG record ([`put_layer_png`]) over the raster's rows.
pub(super) fn put_layer(
    out: &mut dyn NativeSink,
    pixels: &LayerRaster,
    job: JobContext<'_>,
) -> Result<(), String> {
    if let Some(plane) = pixels.tile_plane() {
        if out.raster(RasterRef::Rgba8Png(plane))? {
            return Ok(());
        }
    }
    put_layer_png(
        out,
        pixels.dimensions(),
        &mut rgba_rows(|y| pixels.row_segments(y, 0, u32::MAX)),
        job,
    )
}

/// A byte layer record's pixel field: the u32 length, then the RGBA8 PNG of
/// `rows`, at most [`MAX_DRDC_PNG_LEN`].
pub(crate) fn put_layer_png(
    out: &mut dyn NativeSink,
    (width, height): (u32, u32),
    rows: &mut png_rows8::Rows<'_>,
    job: JobContext<'_>,
) -> Result<(), String> {
    let record = png_rows8::Record {
        prefix: 4,
        cap: u64::from(MAX_DRDC_PNG_LEN),
        too_large: "layer PNG too large",
        blob: false,
    };
    let png = png_rows8::Png8 {
        width,
        height,
        channels: 4,
        after_ihdr: &[],
        before_iend: &[],
    };
    png_rows8::put_png8_record(out, &record, &png, rows, job).map(|_| ())
}

/// A byte channel's pixel field: offered to the sink as tiles first, else
/// its PNG record ([`put_channel_png`]) over the plane's rows.
pub(super) fn put_channel(
    out: &mut dyn NativeSink,
    plane: &GrayRaster,
    job: JobContext<'_>,
) -> Result<(), String> {
    if let Some(tiles) = plane.tile_plane() {
        if out.raster(RasterRef::Gray8Png(tiles))? {
            return Ok(());
        }
    }
    put_channel_png(
        out,
        plane.dimensions(),
        &mut gray_rows(|y| plane.row_segments(y, 0, u32::MAX)),
        job,
    )
}

/// A byte channel's pixel field: the u32 length, then the L8 PNG of `rows`,
/// at most [`MAX_DRDC_PNG_LEN`], announced as one payload.
pub(crate) fn put_channel_png(
    out: &mut dyn NativeSink,
    (width, height): (u32, u32),
    rows: &mut png_rows8::Rows<'_>,
    job: JobContext<'_>,
) -> Result<(), String> {
    let record = png_rows8::Record {
        prefix: 4,
        cap: u64::from(MAX_DRDC_PNG_LEN),
        too_large: "channel PNG too large",
        blob: true,
    };
    let png = png_rows8::Png8 {
        width,
        height,
        channels: 1,
        after_ihdr: &[],
        before_iend: &[],
    };
    png_rows8::put_png8_record(out, &record, &png, rows, job).map(|_| ())
}

/// A byte mask record's raw rows: offered to the sink as tiles first, else
/// announced as one payload and written run by run ([`put_runs`]).
pub(super) fn put_mask(
    out: &mut dyn NativeSink,
    mask: &GrayRaster,
    job: JobContext<'_>,
) -> Result<(), String> {
    if let Some(tiles) = mask.tile_plane() {
        if out.raster(RasterRef::Gray8Raw(tiles))? {
            return Ok(());
        }
    }
    out.begin_blob(mask.byte_len())?;
    put_runs(out, mask.runs(), job)
}

/// A mask plane's row-major samples, run by run (13B-4: the tiles read in
/// row order write the bytes a contiguous plane wrote), polling `job` once
/// per 64 KiB.
pub(crate) fn put_runs<'a>(
    out: &mut dyn NativeSink,
    runs: impl Iterator<Item = &'a [u8]>,
    job: JobContext<'_>,
) -> Result<(), String> {
    let mut moved = 0;
    for run in runs {
        out.put(run)?;
        moved += run.len();
        if moved >= crate::cancel_io::POLL_BYTES {
            moved -= crate::cancel_io::POLL_BYTES;
            job.check()?;
        }
    }
    Ok(())
}

/// Puts the small fields gathered so far and empties the scratch.
pub(super) fn put_pending(out: &mut dyn NativeSink, buf: &mut Vec<u8>) -> Result<(), String> {
    out.put(buf)?;
    buf.clear();
    Ok(())
}

/// An RGBA8 row source over `segments(y)`, the runs of row `y` left to
/// right (`LayerRaster::row_segments`, `TilePlane::row_segments`).
pub(crate) fn rgba_rows<'a, I>(
    segments: impl Fn(u32) -> I + 'a,
) -> impl FnMut(u32, &mut [u8]) -> Result<(), String> + 'a
where
    I: Iterator<Item = (u32, &'a [[u8; 4]])>,
{
    move |y, row| {
        let mut at = 0;
        for (_, run) in segments(y) {
            let bytes = run.as_flattened();
            fill(row, at, bytes)?;
            at += bytes.len();
        }
        full_row(at, row.len())
    }
}

/// The gray twin of [`rgba_rows`] (`GrayRaster::row_segments`,
/// `TilePlane<u8>::row_segments`).
pub(crate) fn gray_rows<'a, I>(
    segments: impl Fn(u32) -> I + 'a,
) -> impl FnMut(u32, &mut [u8]) -> Result<(), String> + 'a
where
    I: Iterator<Item = (u32, &'a [u8])>,
{
    move |y, row| {
        let mut at = 0;
        for (_, run) in segments(y) {
            fill(row, at, run)?;
            at += run.len();
        }
        full_row(at, row.len())
    }
}

/// Copies `run` into `row` at `at`, refusing a run past the row's end.
fn fill(row: &mut [u8], at: usize, run: &[u8]) -> Result<(), String> {
    row.get_mut(at..at + run.len())
        .ok_or("PNG encoding failed: a row is long")?
        .copy_from_slice(run);
    Ok(())
}

/// A row source's check that its runs filled the row.
fn full_row(filled: usize, len: usize) -> Result<(), String> {
    if filled == len {
        Ok(())
    } else {
        Err("PNG encoding failed: a row is short".into())
    }
}
