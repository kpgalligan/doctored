//! The Remove tool's model engine (M3a): the pixel math around a local
//! inpainting model that runs on a square crop at its native size. The
//! adapters (`doc_remove_model`) read the plate and the layer; the host runs
//! the model between the two halves:
//!
//! 1. INPUT. The crop around the stroke ([`crop`]), the native mask (a cell
//!    is 1 when any canvas pixel its footprint touches is covered) and the
//!    native image (the exact area-weighted mean of the plate over the cell's
//!    footprint, premultiplied and divided by the mean alpha) — [`native_input`]
//!    over [`native_mean`], which Generative Fill (`gen_fill`) shares at a
//!    non-square native size.
//! 2. APPLY. The fill read back from the model's plane at each covered
//!    canvas pixel ([`fill_at`]) and composited over the layer by the
//!    coverage ([`composite`]).
//!
//! Every value is display-encoded RGB in 0…1, the plate's own encoding: the
//! model was trained on encoded photographs, and the composite is the paint
//! tools' coverage mix, which works in the layer's encoding.

use crate::job_control::JobContext;
use crate::sampling_radial::sample_exact;

/// The model's native side the host may ask for: 64 is the smallest crop a
/// convolutional inpainter still has context in, 1024 twice MI-GAN's 512 so
/// a larger model needs no new export.
pub(crate) const SIDES: std::ops::RangeInclusive<u32> = 64..=1024;
/// The refusal of a hole with no context around it: over
/// a transparent surround the model would paint an opaque blob from nothing.
pub(crate) const NOTHING_TO_SAMPLE: &str =
    "there is nothing to sample from: every pixel around the stroke is transparent.";
/// The refusal of a hole that covers every cell of the crop (a Select All
/// hole, or a stroke wider than the canvas): the model would invent the whole
/// crop from no context, and the transparency sentence would name a cause
/// that is not there.
pub(crate) const NO_CONTEXT: &str =
    "the hole covers the whole crop: there is no picture around it to fill from";
/// The refusal of a coverage with no sample above 0.
pub(crate) const NO_COVERAGE: &str = "the stroke covers no pixel of the picture";
/// The refusal of a model plane holding a NaN or an infinity.
pub(crate) const NOT_FINITE: &str = "remove model output is not finite";

/// `side` inside [`SIDES`], else the refusal.
pub(crate) fn check_side(side: u32) -> Result<(), String> {
    if SIDES.contains(&side) {
        Ok(())
    } else {
        Err(format!(
            "remove model side must be {}…{}",
            SIDES.start(),
            SIDES.end()
        ))
    }
}

/// A canvas rect `(x, y, w, h)`.
pub(crate) type Rect = (u32, u32, u32, u32);

/// The bounding box of the samples of `coverage` (a `width`-wide canvas)
/// above 0; `None` when there is none. Polls `job` every few thousand
/// samples.
pub(crate) fn covered_rect(
    coverage: &[u16],
    width: u32,
    job: JobContext<'_>,
) -> Result<Option<Rect>, String> {
    let w = width as usize;
    if w == 0 {
        return Ok(None);
    }
    let (mut x0, mut y0, mut x1, mut y1) = (usize::MAX, usize::MAX, 0, 0);
    for (i, c) in coverage.iter().enumerate() {
        job.every(i)?;
        if *c == 0 {
            continue;
        }
        let (x, y) = (i % w, i / w);
        x0 = x0.min(x);
        y0 = y0.min(y);
        x1 = x1.max(x + 1);
        y1 = y1.max(y + 1);
    }
    Ok((x0 < x1).then(|| (x0 as u32, y0 as u32, (x1 - x0) as u32, (y1 - y0) as u32)))
}

/// The square crop the model runs on, around the covered rect `covered` on a
/// `canvas`: `s = max(side, ⌈2·L⌉)` with `L` the rect's longer side, so the
/// hole fills at most half the crop's width and the model sees at least as
/// much context as hole; clipped to the canvas and centred on the rect,
/// slid inside the canvas. Because `s ≥ L` and the slide is clamped, the
/// crop always contains the covered rect. A canvas smaller than `side` is
/// the whole canvas, resampled UP to the native size.
pub(crate) fn crop(covered: Rect, canvas: (u32, u32), side: u32) -> Rect {
    let (x0, y0, w, h) = covered;
    let longer = u64::from(w.max(h));
    let s = u64::from(side).max(2 * longer);
    let cw = s.min(u64::from(canvas.0)) as i64;
    let ch = s.min(u64::from(canvas.1)) as i64;
    let place = |start: u32, extent: u32, size: i64, total: u32| -> u32 {
        let centred = i64::from(start) + i64::from(extent) / 2 - size / 2;
        centred.clamp(0, i64::from(total) - size) as u32
    };
    (
        place(x0, w, cw, canvas.0),
        place(y0, h, ch, canvas.1),
        cw as u32,
        ch as u32,
    )
}

/// The kernel's own working set on a `width` × `height` canvas for a model
/// of native `side`, writing `bytes_per_pixel` bytes a layer pixel (4 byte,
/// 8 u16), beside the plate (the adapters add it):
///
/// - the three native planes the host keeps live across the job: image
///   `3·side²` f32, mask `side²` f32 and the model's plane `3·side²` f32 —
///   28 bytes a cell;
/// - [`native_input`]'s accumulator, four f64 per cell, and its mask flags —
///   33 bytes a cell;
/// - the write-back: the covered rect read off the layer and the tiles
///   rewritten from it, each at most the canvas at `bytes_per_pixel` since
///   the plan sees no coverage;
/// - `new_layer`: the canvas-sized layer the fill lands on, one more canvas
///   at `bytes_per_pixel`.
///
/// The crop's own rows (one row of the plate at a time) and the per-axis
/// footprint tables are O(width + height + side) and ride in the 1 KiB
/// per-side slack below.
pub(crate) fn scratch_bytes(
    width: u32,
    height: u32,
    side: u32,
    bytes_per_pixel: u64,
    new_layer: bool,
) -> u64 {
    let cells = u64::from(side) * u64::from(side);
    let n = u64::from(width) * u64::from(height);
    let bpp = bytes_per_pixel;
    let planes = 28 * cells;
    let accumulate = 33 * cells;
    let write_back = 2 * n * bpp;
    let layer = if new_layer { n * bpp } else { 0 };
    let slack = 1024 * (u64::from(side) + u64::from(width) + u64::from(height));
    planes + accumulate + write_back + layer + slack
}

/// The canvas pixels `[floor(i·extent/side), ceil((i+1)·extent/side))` of
/// cell `i` along one axis (crop-relative), the footprint the mask reads.
fn footprint(i: u32, extent: u32, side: u32) -> (u32, u32) {
    let (i, e, s) = (u64::from(i), u64::from(extent), u64::from(side));
    ((i * e / s) as u32, ((i + 1) * e).div_ceil(s) as u32)
}

/// Per crop-relative row or column `p` along an axis of `extent` pixels: the
/// first and one-past-last cell whose footprint holds it (a contiguous run).
fn cells_of(extent: u32, side: u32) -> Vec<(u32, u32)> {
    let mut runs = vec![(u32::MAX, 0u32); extent as usize];
    for cell in 0..side {
        let (a, b) = footprint(cell, extent, side);
        for p in a..b.min(extent) {
            let run = &mut runs[p as usize];
            run.0 = run.0.min(cell);
            run.1 = run.1.max(cell + 1);
        }
    }
    runs
}

/// The length of pixel `p`'s unit interval `[p, p+1)` inside cell `cell`'s
/// exact interval `[cell·extent/side, (cell+1)·extent/side)`: the area weight
/// of the native image along one axis. 0 for a pixel the footprint only
/// grazes.
fn weight(p: u32, cell: u32, extent: u32, side: u32) -> f64 {
    let scale = f64::from(extent) / f64::from(side);
    let a = f64::from(cell) * scale;
    let b = f64::from(cell + 1) * scale;
    (b.min(f64::from(p) + 1.0) - a.max(f64::from(p))).max(0.0)
}

/// What [`native_mean`] found around the hole: `unmasked` when some cell is
/// left at 0 in the mask, and `context_alpha`, the plate's summed alpha
/// weight over those cells.
pub(crate) struct NativeContext {
    pub unmasked: bool,
    pub context_alpha: f64,
}

/// The native input of a `side`² model over `crop` on a canvas `width`
/// wide: [`native_mean`] at `(side, side)`, refusing a hole with no context.
/// `Err(NO_CONTEXT)` when the mask leaves no cell at 0, and
/// `Err(NOTHING_TO_SAMPLE)` when the plate's alpha sums to 0 over every cell
/// it does leave at 0: either way the model would invent the whole crop from
/// no context.
#[allow(clippy::too_many_arguments)]
pub(crate) fn native_input(
    coverage: &[u16],
    width: u32,
    crop: Rect,
    side: u32,
    row: impl FnMut(u32) -> Result<Vec<[f32; 4]>, String>,
    image_out: &mut [f32],
    mask_out: &mut [f32],
    job: JobContext<'_>,
) -> Result<(), String> {
    let cells = side as usize * side as usize;
    if image_out.len() != 3 * cells || mask_out.len() != cells || crop.2 == 0 || crop.3 == 0 {
        return Err("invalid remove model input buffers".into());
    }
    let context = native_mean(
        coverage,
        width,
        crop,
        (side, side),
        row,
        image_out,
        mask_out,
        job,
    )
    .map_err(|e| {
        if e == ROW_LENGTH {
            "remove model plate row has the wrong length".to_string()
        } else {
            e
        }
    })?;
    if !context.unmasked {
        return Err(NO_CONTEXT.into());
    }
    if context.context_alpha <= 0.0 {
        return Err(NOTHING_TO_SAMPLE.into());
    }
    Ok(())
}

/// [`native_mean`]'s refusal of a plate row that is not `crop.2` samples.
const ROW_LENGTH: &str = "native plate row has the wrong length";

/// The native mean of the plate over `crop` on a canvas `width` wide, at a
/// `native = (nw, nh)` frame: `mask_out` (`nw·nh`, row-major, 1.0 where any
/// canvas pixel the cell's footprint touches has coverage above 0) and
/// `image_out` (`3·nw·nh`, channel-major R, G, B, row-major each: the exact
/// area-weighted mean of the plate over the cell's fractional footprint,
/// premultiplied and divided by the mean alpha, 0 where the alpha sums to 0).
///
/// `row(r)` answers the plate's straight unit samples of crop row `r` (canvas
/// row `crop.1 + r`), `crop.2` of them, so the plate is read one row at a
/// time whatever its depth.
///
/// Separable: each row is reduced across into one value per cell column,
/// then added down into the cells whose footprint holds the row with the
/// row's vertical weight. The caller decides what a hole with no context
/// means from the answered [`NativeContext`].
#[allow(clippy::too_many_arguments)]
pub(crate) fn native_mean(
    coverage: &[u16],
    width: u32,
    crop: Rect,
    native: (u32, u32),
    mut row: impl FnMut(u32) -> Result<Vec<[f32; 4]>, String>,
    image_out: &mut [f32],
    mask_out: &mut [f32],
    job: JobContext<'_>,
) -> Result<NativeContext, String> {
    let (cx, cy, cw, ch) = crop;
    let (nw, nh) = native;
    let s = nw as usize;
    let cells = s * nh as usize;
    if image_out.len() != 3 * cells || mask_out.len() != cells || cw == 0 || ch == 0 {
        return Err("invalid native input buffers".into());
    }
    let rows = cells_of(ch, nh);
    // Per cell column: the crop columns it reads, with their area weights.
    let mut across: Vec<Vec<(u32, f64)>> = Vec::with_capacity(s);
    for cell in 0..nw {
        let (a, b) = footprint(cell, cw, nw);
        across.push(
            (a..b.min(cw))
                .map(|p| (p, weight(p, cell, cw, nw)))
                .collect(),
        );
    }
    let mut acc = vec![[0f64; 4]; cells];
    let mut covered = vec![false; cells];
    let mut line = vec![[0f64; 4]; s];
    let mut line_covered = vec![false; s];
    for r in 0..ch {
        job.check()?;
        let samples = row(r)?;
        if samples.len() != cw as usize {
            return Err(ROW_LENGTH.into());
        }
        let canvas_row = (cy + r) as usize * width as usize + cx as usize;
        let hits = &coverage[canvas_row..canvas_row + cw as usize];
        for (cell, taps) in across.iter().enumerate() {
            let mut sum = [0f64; 4];
            let mut any = false;
            for &(p, w) in taps {
                let [red, green, blue, alpha] = samples[p as usize].map(f64::from);
                let wa = w * alpha;
                sum[0] += wa * red;
                sum[1] += wa * green;
                sum[2] += wa * blue;
                sum[3] += wa;
                any |= hits[p as usize] > 0;
            }
            line[cell] = sum;
            line_covered[cell] = any;
        }
        let (first, last) = rows[r as usize];
        for cell_row in first..last {
            let wy = weight(r, cell_row, ch, nh);
            let base = cell_row as usize * s;
            let targets = acc[base..base + s]
                .iter_mut()
                .zip(&mut covered[base..base + s]);
            for ((target, flag), (sum, any)) in targets.zip(line.iter().zip(&line_covered)) {
                for (t, v) in target.iter_mut().zip(sum) {
                    *t += wy * v;
                }
                *flag |= *any;
            }
        }
    }
    let mut context_alpha = 0f64;
    let mut unmasked = false;
    for i in 0..cells {
        job.every(i)?;
        let [r, g, b, a] = acc[i];
        mask_out[i] = if covered[i] { 1.0 } else { 0.0 };
        if !covered[i] {
            unmasked = true;
            context_alpha += a;
        }
        let rgb = if a > 0.0 {
            [r / a, g / a, b / a]
        } else {
            [0.0; 3]
        };
        for c in 0..3 {
            image_out[c * cells + i] = rgb[c].clamp(0.0, 1.0) as f32;
        }
    }
    Ok(NativeContext {
        unmasked,
        context_alpha,
    })
}

/// `Err(NOT_FINITE)` when any sample of `plane` is a NaN or an infinity.
pub(crate) fn check_plane(plane: &[f32], job: JobContext<'_>) -> Result<(), String> {
    for (i, v) in plane.iter().enumerate() {
        job.every(i)?;
        if !v.is_finite() {
            return Err(NOT_FINITE.into());
        }
    }
    Ok(())
}

/// The model's fill at canvas pixel `(px, py)`: its `side`² channel-major
/// `plane` read as opaque `[r, g, b, 1]` with `sampling_radial::sample_exact`
/// (bilinear, the pixel centre mapped into the crop's native frame),
/// clamped to 0…1. Bilinear, not the cell the pixel falls in: when the crop
/// is resampled down to the native size a nearest read would print the
/// cells as blocks across the hole.
pub(crate) fn fill_at(plane: &[f32], side: u32, crop: Rect, px: u32, py: u32) -> [f64; 3] {
    let s = side as usize;
    let cells = s * s;
    let read = |i: usize| [plane[i], plane[cells + i], plane[2 * cells + i], 1.0];
    let (cx, cy, cw, ch) = crop;
    let qx = (f64::from(px) + 0.5 - f64::from(cx)) * f64::from(side) / f64::from(cw);
    let qy = (f64::from(py) + 0.5 - f64::from(cy)) * f64::from(side) / f64::from(ch);
    let v = sample_exact(&read, (s, s), (qx, qy), true);
    [v[0], v[1], v[2]].map(|c| c.clamp(0.0, 1.0))
}

/// The fill `fill` laid over the straight unit pixel `under` at coverage
/// `c` (0 < c ≤ 1): `A = c + (1 − c)·a0`, `RGB = (c·f + (1 − c)·a0·rgb0)/A` —
/// source-over of the fill at alpha `c`, so a fully covered pixel becomes the
/// opaque fill and a soft edge mixes into what was there. On a new layer
/// (`under` transparent) it is the fill at alpha `c`.
pub(crate) fn composite(c: f64, fill: [f64; 3], under: [f64; 4]) -> [f64; 4] {
    let a0 = under[3];
    let alpha = c + (1.0 - c) * a0;
    if alpha <= 0.0 {
        return [0.0; 4];
    }
    let mix = |k: usize| ((c * fill[k] + (1.0 - c) * a0 * under[k]) / alpha).clamp(0.0, 1.0);
    [mix(0), mix(1), mix(2), alpha.clamp(0.0, 1.0)]
}
