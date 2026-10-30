//! Sky Replacement's "New Layers" output (M2b): the pixel math of the
//! editable Sky Replacement Group. The adapters (`doc_sky_group`) build, in
//! a Pass Through group above the source, bottom-first:
//!
//! 0. with `decontaminate` on, [`EDGE_NAME`], a canvas-sized raster at
//!    (0, 0), Normal, opacity 1, holding `Prepared::foreground`'s
//!    decontaminated colour with the source's alpha inside the edge band and
//!    transparent outside it, masked (binary) to that band ([`edge_planes`]);
//! 1. [`COLOR_NAME`], a Curves adjustment at opacity `color_adjustment/100`
//!    whose per-channel curve adds the colour step ([`foreground_curves`]);
//! 2. [`LIGHTING_NAME`], a Curves adjustment in the Lighting Mode (Multiply
//!    or Screen) at opacity `lighting/100` whose flat curve is the sky's mean
//!    `K̄`;
//! 3. [`SKY_NAME`], the placed adjusted sky `K`, opaque and canvas-sized at
//!    (0, 0), Normal, masked by [`sky_mask`].
//!
//! Over an opaque foreground `F` the children composite to `sky_harmonize`'s
//! Duplicate Layer pixel exactly (up to quantization of the curves and the
//! mask):
//!
//! * the edge layer, opaque wherever its mask is on, replaces `F` with the
//!   decontaminated colour there and leaves it elsewhere — the kernel's `F`
//!   is `Prepared::foreground`, the same value, so the steps below start
//!   from the same colour;
//! * the colour curve is `F + s` with `s = 255·COLOR_TRANSFER·d` (held
//!   within ±254 codes so the curve keeps two distinct inputs), clipped at
//!   the ends by the curve's flat extension, so at opacity `c` it is
//!   `F + c·COLOR_TRANSFER·d`, clamped — the kernel's `F1`, which is why the
//!   kernel clamps `F1` too;
//! * a flat curve at `K̄` under Multiply or Screen at opacity `L` is
//!   `F1 + L·(B(F1, K̄) − F1)` — the kernel's `F2`;
//! * the sky at mask `m + (1 − m)·edge·band` over `F2` is
//!   `F2 + (m + (1 − m)·edge·band)·(K − F2)`, which is the kernel's
//!   `m·K + (1 − m)·F3` with `F3 = F2 + edge·band·(K − F2)`.
//!
//! Every value is display-encoded RGB in 0…1, like `sky_harmonize`'s; the
//! planes come from `sky_harmonize::prepare`, never from a second refine,
//! placement or harmony.

use crate::job_control::JobContext;
use crate::sky_harmonize::{Harmony, Prepared, SkyParams, COLOR_TRANSFER};

/// The group's name when `params.name` is absent.
pub(crate) const GROUP_NAME: &str = "Sky Replacement Group";
/// The masked sky layer's name.
pub(crate) const SKY_NAME: &str = "Sky";
/// The lighting adjustment layer's name.
pub(crate) const LIGHTING_NAME: &str = "Foreground Lighting";
/// The colour adjustment layer's name.
pub(crate) const COLOR_NAME: &str = "Foreground Color";
/// The retained output per canvas pixel at 8 bits: the RGBA8 sky layer (4),
/// its mask (1) and the two adjustment layers' white masks (1 each).
pub(crate) const NEW_LAYERS_BYTES_8: u64 = 7;
/// The same at 16 bits: the RGBA16 sky (8), its u16 mask (2) and two u16
/// white masks (2 each).
pub(crate) const NEW_LAYERS_BYTES_16: u64 = 14;
/// The "Foreground Edge" layer's name.
pub(crate) const EDGE_NAME: &str = "Foreground Edge";
/// The edge layer's retained output per canvas pixel at 8 bits: its RGBA8
/// raster (4) and u8 mask (1).
pub(crate) const EDGE_LAYER_BYTES_8: u64 = 5;
/// The same at 16 bits: the RGBA16 raster (8) and u16 mask (2).
pub(crate) const EDGE_LAYER_BYTES_16: u64 = 10;

/// The New Layers output's retained bytes per canvas pixel at 16 bits
/// (`sixteen`) or 8, with the edge layer when `decontaminate` is on.
pub(crate) fn new_layers_bytes(sixteen: bool, decontaminate: bool) -> u64 {
    let (group, edge) = if sixteen {
        (NEW_LAYERS_BYTES_16, EDGE_LAYER_BYTES_16)
    } else {
        (NEW_LAYERS_BYTES_8, EDGE_LAYER_BYTES_8)
    };
    group + if decontaminate { edge } else { 0 }
}

/// The colour step's largest curve offset in codes: a curve needs two
/// distinct inputs, so `s` stops one code short of 255.
const MAX_COLOR_SHIFT: f64 = 254.0;

/// The sky layer's mask at a pixel: `m + (1 − m)·edge_lighting/100·band`,
/// in 0…1 (module doc).
pub(crate) fn sky_mask(m: f32, band: f32, p: &SkyParams) -> f32 {
    (m + (1.0 - m) * p.edge_lighting / 100.0 * band).clamp(0.0, 1.0)
}

/// The sky layer's planes over the `dimensions` canvas: each pixel's placed
/// sky `K` and its [`sky_mask`], handed to `write` once, in order.
pub(crate) fn sky_planes(
    prepared: &Prepared,
    dimensions: (u32, u32),
    p: &SkyParams,
    job: JobContext<'_>,
    mut write: impl FnMut(usize, [f32; 3], f32),
) -> Result<(), String> {
    let w = dimensions.0 as usize;
    let rows = dimensions.1 as usize;
    for y in 0..rows {
        for x in 0..w {
            let i = y * w + x;
            job.every(i)?;
            let (m, band) = prepared.mask_at(i);
            let k = prepared.placement.sample(x as u32, y as u32);
            write(i, k, sky_mask(m, band, p));
        }
        job.report((y + 1) as f64 / rows as f64, 6)?;
    }
    Ok(())
}

/// The edge layer's planes over the `dimensions` canvas (module doc, child
/// 0): each pixel's `Prepared::foreground` of the source sample `read(i)`
/// with model coverage `m0[i]`, and whether it is in the band
/// (`Prepared::in_edge`), handed to `write` once, in order.
pub(crate) fn edge_planes(
    read: impl Fn(usize) -> [f32; 4],
    m0: &[u16],
    prepared: &Prepared,
    dimensions: (u32, u32),
    job: JobContext<'_>,
    mut write: impl FnMut(usize, [f32; 4], bool),
) -> Result<(), String> {
    let w = dimensions.0 as usize;
    let rows = dimensions.1 as usize;
    for y in 0..rows {
        for x in 0..w {
            let i = y * w + x;
            job.every(i)?;
            let fg = read(i);
            write(
                i,
                prepared.foreground(i, fg, m0[i]),
                prepared.in_edge(i, fg, m0[i]),
            );
        }
        job.report((y + 1) as f64 / rows as f64, 6)?;
    }
    Ok(())
}

/// The two adjustment layers' per-channel (R, G, B) curves, each two
/// `[in, out]` points on 0…255.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ForegroundCurves {
    pub color: [[[f64; 2]; 2]; 3],
    pub lighting: [[[f64; 2]; 2]; 3],
}

/// The colour and lighting curves for `h` (module doc): the colour step
/// `s = 255·COLOR_TRANSFER·d` per channel as `[[0, s], [255 − s, 255]]` (or
/// `[[−s, 0], [255, 255 + s]]` for a negative `s`), and the lighting curve
/// flat at `255·K̄`.
pub(crate) fn foreground_curves(h: &Harmony) -> ForegroundCurves {
    let color = std::array::from_fn(|c| {
        let s = (255.0 * f64::from(COLOR_TRANSFER) * f64::from(h.cast_shift[c]))
            .clamp(-MAX_COLOR_SHIFT, MAX_COLOR_SHIFT);
        if s >= 0.0 {
            [[0.0, s], [255.0 - s, 255.0]]
        } else {
            [[-s, 0.0], [255.0, 255.0 + s]]
        }
    });
    let lighting = std::array::from_fn(|c| {
        let k = 255.0 * f64::from(h.sky_mean[c]).clamp(0.0, 1.0);
        [[0.0, k], [255.0, k]]
    });
    ForegroundCurves { color, lighting }
}
