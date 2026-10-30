//! The thumbnail reducers: the equal-area box mean ([`box_reduced`], split
//! by [`area_split`]; its row-run form [`box_reduced_rows`] reads a plane
//! placed on a canvas) that gets a canvas-sized plane down to thumbnail
//! scale without a resampling kernel reading every sample, with
//! [`gray_resized`] the kernel pass after it. [`box_reduced`] is the master
//! copy of the plain box mean.
//!
//! `box_reduced` (the master plain box mean — EQUAL real-valued area boxes tiling the plane
//! exactly, generic over the sample with a byte map, polled per row), `area_split` (the ONE
//! statement of the equal-area split, also read by `layer_thumbnail::box_reduced_rgba`),
//! `box_reduced_rows` (its row-run form, the body `box_reduced` wraps with one run per row, which
//! reads a plane PLACED on a canvas through `doc_channel::placed_row` without the canvas-sized
//! copy) and `gray_resized` (the Triangle-resize tail every plane and mask thumbnail shares).
//! Callers: `ffi_channel::plane_image`, `layer_thumbnail` and `precision_thumbnail`;
//! `doc_plane::gray_image` stays in `doc_plane`.
//!
//! `thumbnail_reduce::box_reduced` (the master plain box mean, generic over the sample with a byte
//! map, polled per row) uses the same equal-area boxes, so the Channels panel's plane
//! thumbnails (`ffi_channel::plane_image`, guard 4, tail `thumbnail_reduce::gray_resized`) keep
//! their box centres on the kernel's grid; its body is `box_reduced_rows` (row
//! runs covering `[0, w)`, `box_reduced_rgba`'s contract) and `box_reduced` the one-run-per-row
//! wrapper, its bytes pinned by SHA-256 against the pre-split pass.

use image::imageops::{self, FilterType};
use image::{GrayImage, RgbaImage};

use crate::doc_plane::gray_image;
use crate::job_control::JobContext;
use crate::precision_image::reserved_vec;

/// Where sample `s` of an axis of `n` samples lands when that axis is cut
/// into `m` equal boxes of `n / m` samples (real-valued; box `i` spans
/// `[i·n/m, (i + 1)·n/m)`): its box, the share of the sample inside it and
/// the share spilling into the next one (0 unless the sample straddles an
/// edge; `n / m >= 2`, so it never reaches a third box). The ONE statement
/// of the equal-area split, read by [`box_reduced`] and
/// `layer_thumbnail::box_reduced_rgba`.
pub(crate) fn area_split(s: u32, n: u32, m: u32) -> (usize, f64, f64) {
    let (s, n, m) = (u64::from(s), u64::from(n), u64::from(m));
    let b = s * m / n;
    let edge = (b + 1) * n;
    if (s + 1) * m <= edge {
        (b as usize, 1.0, 0.0)
    } else {
        let inside = (edge - s * m) as f64 / m as f64;
        (b as usize, inside, 1.0 - inside)
    }
}

/// `plane` reduced by about the whole-number factor `k` in both axes, each
/// sample read as a byte through `value`: the result is `floor(w / k)` x
/// `floor(h / k)`, returned with its dimensions, and every output byte is
/// `round(Σ overlap · value / box_area)` over its box. The master copy of
/// the plain box mean (`layer_thumbnail::box_reduced_rgba` is its RGBA,
/// premultiplied relative; `patchmatch` names it too).
///
/// The boxes are EQUAL real-valued areas that tile the plane exactly —
/// `w / sw` by `h / sh` samples each, so between `k` and `k + 1` — and a
/// sample straddling a box edge is shared between the boxes by the area on
/// either side ([`area_split`]). That keeps every box centre on the uniform
/// grid the resampling kernel after it assumes, so a constant plane still
/// reduces to that constant everywhere. The former whole `k`-sample boxes
/// with a partial last one stretched that grid instead: at a 2400 × 1600
/// hard edge (`k` = 17) they drifted 54 codes from the single kernel pass on
/// an RGBA layer (`layer_thumbnail`'s measurement) and, in a one-off
/// measurement of the former code, 14 on a two-tone plane, where these
/// boxes measure 1 (`tests/layer_thumbnail.rs`).
///
/// It exists for one job: getting a canvas-sized plane down to a panel
/// thumbnail without a resampling kernel reading every sample. Whole spans
/// of a box within a row are summed as plain integer slice sums; f64 is
/// used only for the samples straddling a box edge and once per box per
/// row. Measured end to end, a 6000 × 4000 plane's 34-pixel thumbnail
/// through `dr_image_plane_image` — the plane read out of the image, this
/// pass and the kernel — takes 19.3–20.4 ms (release, medians of 3 over four
/// runs, `a_large_plane_thumbnail_is_box_reduced_within_two_codes_of_the_single_pass`).
///
/// AVERAGING, not sampling every k-th sample: a matte's few-pixel detail has
/// to survive into the thumbnail, and a stride would drop it.
///
/// `Ok(None)` for `k < 2`, a zero dimension, a side smaller than `k` or a
/// buffer that is not `w * h` samples — nothing to reduce, so the caller
/// keeps the plane it has. `Err` for an allocation failure or a cancel;
/// `job` is polled once per row. The body is [`box_reduced_rows`] with ONE
/// run per row, which sums every whole span of a box as one integer slice
/// sum and keeps the f64 order `(v · row_in) · col_in` for a straddling
/// sample, so these bytes are the ones the single-buffer pass gave
/// (`the_box_path_bytes_are_unchanged_by_the_row_run_refactor`).
pub(crate) fn box_reduced<T: Copy>(
    plane: &[T],
    w: u32,
    h: u32,
    k: u32,
    value: impl Fn(T) -> u8,
    job: JobContext<'_>,
) -> Result<Option<(Vec<u8>, u32, u32)>, String> {
    if k < 2 || w == 0 || h == 0 || w < k || h < k {
        return Ok(None);
    }
    if Some(plane.len()) != (w as usize).checked_mul(h as usize) {
        return Ok(None);
    }
    let width = w as usize;
    box_reduced_rows(
        w,
        h,
        k,
        |y| {
            let start = y as usize * width;
            std::iter::once((0, &plane[start..start + width]))
        },
        value,
        job,
    )
}

/// [`box_reduced`] over a `w` x `h` plane read row by row through `row(y)`:
/// the row's runs `(x, samples)`, left to right and adjoining, covering
/// exactly `[0, w)` (`layer_thumbnail::box_reduced_rgba`'s contract) — so a
/// plane PLACED on a canvas (`doc_channel::placed_row`: a fill run, the
/// plane's row, a fill run) reduces without the canvas-sized copy.
///
/// A run is cut at box edges as `box_reduced_rgba` cuts it: the whole
/// samples of box `bx` up to its edge (or the run's end) as one integer
/// slice sum, else the one sample straddling the edge, shared by area. A
/// box span split across two runs adds two partial sums, so only the
/// one-run-per-row form is byte for byte [`box_reduced`]'s former pass.
///
/// `Ok(None)` for `k < 2`, a zero dimension or a side smaller than `k`.
/// `Err` for an allocation failure or a cancel; `job` is polled once per
/// row.
pub(crate) fn box_reduced_rows<'a, T: Copy + 'a, R: Iterator<Item = (u32, &'a [T])>>(
    w: u32,
    h: u32,
    k: u32,
    row: impl Fn(u32) -> R,
    value: impl Fn(T) -> u8,
    job: JobContext<'_>,
) -> Result<Option<(Vec<u8>, u32, u32)>, String> {
    if k < 2 || w == 0 || h == 0 || w < k || h < k {
        return Ok(None);
    }
    let (sw, sh) = (w / k, h / k);
    let mut out = reserved_vec((sw as usize) * (sh as usize))?;
    // The current band's accumulators and the next band's (which a row
    // straddling the band edge already feeds), area-weighted, f64 because
    // one box may cover most of the plane.
    let mut current: Vec<f64> = reserved_vec(sw as usize)?;
    current.resize(sw as usize, 0.0);
    let mut next: Vec<f64> = reserved_vec(sw as usize)?;
    next.resize(sw as usize, 0.0);
    let area = f64::from(w) * f64::from(h) / (f64::from(sw) * f64::from(sh));
    let emit = |sums: &mut Vec<f64>, out: &mut Vec<u8>| {
        for acc in sums.iter_mut() {
            out.push((*acc / area).round().clamp(0.0, 255.0) as u8);
            *acc = 0.0;
        }
    };
    let mut band = 0usize;
    for y in 0..h {
        job.check()?;
        let (by, row_in, row_out) = area_split(y, h, sh);
        while band < by {
            emit(&mut current, &mut out);
            std::mem::swap(&mut current, &mut next);
            band += 1;
        }
        for (x0, run) in row(y) {
            let mut x = x0;
            let mut rest = run;
            while !rest.is_empty() {
                let (bx, col_in, col_out) = area_split(x, w, sw);
                if col_out == 0.0 {
                    // Every whole sample of box `bx` from `x` to its edge
                    // (or the run's end), as one integer slice sum.
                    let end = (u64::from(bx as u32 + 1) * u64::from(w) / u64::from(sw)) as u32;
                    let take = ((end - x) as usize).min(rest.len()).max(1);
                    let (cell, tail) = rest.split_at(take);
                    let sum: u64 = cell.iter().map(|&v| u64::from(value(v))).sum();
                    let sum = sum as f64;
                    current[bx] += sum * row_in;
                    if row_out > 0.0 {
                        next[bx] += sum * row_out;
                    }
                    x += take as u32;
                    rest = tail;
                } else {
                    // The one sample straddling the edge, shared by area.
                    let v = f64::from(value(rest[0]));
                    current[bx] += v * row_in * col_in;
                    current[bx + 1] += v * row_in * col_out;
                    if row_out > 0.0 {
                        next[bx] += v * row_out * col_in;
                        next[bx + 1] += v * row_out * col_out;
                    }
                    x += 1;
                    rest = &rest[1..];
                }
            }
        }
    }
    emit(&mut current, &mut out);
    Ok(Some((out, sw, sh)))
}

/// A gray `w` x `h` buffer resized to `tw` x `th` with the Triangle filter
/// (always, even at equal size) and expanded to an opaque grayscale image
/// ([`gray_image`]). `None` when `buffer` is not `w * h` bytes or a
/// dimension is zero. The tail every plane and mask thumbnail shares.
pub(crate) fn gray_resized(buffer: Vec<u8>, w: u32, h: u32, tw: u32, th: u32) -> Option<RgbaImage> {
    let source = GrayImage::from_raw(w, h, buffer)?;
    let small = imageops::resize(&source, tw, th, FilterType::Triangle);
    gray_image(small.as_raw(), tw, th)
}
