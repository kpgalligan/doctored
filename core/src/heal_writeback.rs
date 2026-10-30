//! The healing family's shared membrane source/destination writeback. Byte and
//! u16 storage select only numeric range, coverage threshold, solver accuracy
//! policy and final quantization; region classification and RGB mixing are one
//! implementation. Alpha and unusable/off-canvas storage remain exact.
//!
//! `heal_writeback.rs` is the shared byte/u16 membrane writeback, and `poisson.rs` selects a
//! separately bounded typed residual policy without changing the byte schedule.
//!
//! The f32 sample is a 32-bit float document's retouch in W codes
//! (`retouch_encoding`): the stored range mapped onto ±65535, so it takes the
//! u16 threshold, range and residual policy, and its final "quantize" clamps
//! `source + u` to ±65535 at the same statement the byte and u16 samples
//! clamp to their own range. That is the stored bound, not a clamp to 0…1.
//! The region and write coverage carry their own sample type, so u16 stroke
//! coverage drives an f32 destination.

use crate::job_control::JobContext;
use crate::poisson::Region;

pub(crate) trait HealSample: Copy + PartialEq {
    const MAX: f32;
    const THRESHOLD: f32;
    fn number(self) -> f32;
    fn quantize(value: f32) -> Self;
    fn quantize_precise(value: f64) -> Self {
        Self::quantize(value as f32)
    }
    fn solve(
        region: &Region<'_>,
        b: &[f32],
        diameter: usize,
        u: &mut [f32],
        job: JobContext<'_>,
    ) -> Result<usize, String>;
}
impl HealSample for u8 {
    const MAX: f32 = 255.0;
    const THRESHOLD: f32 = crate::poisson::COVERED_THRESHOLD as f32;
    fn number(self) -> f32 {
        f32::from(self)
    }
    fn quantize(value: f32) -> Self {
        (value.clamp(0.0, <Self as HealSample>::MAX) + 0.5).floor() as u8
    }
    fn solve(
        region: &Region<'_>,
        b: &[f32],
        diameter: usize,
        u: &mut [f32],
        job: JobContext<'_>,
    ) -> Result<usize, String> {
        crate::poisson::solve_membrane_with_context(region, b, diameter, u, job)
    }
}
impl HealSample for u16 {
    const MAX: f32 = 65535.0;
    const THRESHOLD: f32 = 32768.0;
    fn number(self) -> f32 {
        f32::from(self)
    }
    fn quantize(value: f32) -> Self {
        (value.clamp(0.0, <Self as HealSample>::MAX) + 0.5).floor() as u16
    }
    fn quantize_precise(value: f64) -> Self {
        value.clamp(0., 65535.).round() as u16
    }
    fn solve(
        region: &Region<'_>,
        b: &[f32],
        diameter: usize,
        u: &mut [f32],
        job: JobContext<'_>,
    ) -> Result<usize, String> {
        crate::poisson::solve_membrane_precision(region, b, diameter, u, job)
    }
}

impl HealSample for f32 {
    const MAX: f32 = 65535.0;
    const THRESHOLD: f32 = 32768.0;
    fn number(self) -> f32 {
        self
    }
    fn quantize(value: f32) -> Self {
        value.clamp(-<Self as HealSample>::MAX, <Self as HealSample>::MAX)
    }
    fn solve(
        region: &Region<'_>,
        b: &[f32],
        diameter: usize,
        u: &mut [f32],
        job: JobContext<'_>,
    ) -> Result<usize, String> {
        crate::poisson::solve_membrane_precision(region, b, diameter, u, job)
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn heal<T: HealSample, C: HealSample, S: Copy + Into<f32>>(
    canvas: (u32, u32),
    dimensions: (u32, u32),
    raw: &mut [T],
    offset: (i32, i32),
    win: (i64, i64, usize, usize),
    source: &[S],
    region: &[C],
    write: &[C],
    strength: f32,
    control: JobContext<'_>,
) -> Result<bool, String> {
    heal_with_selection(
        canvas, dimensions, raw, offset, win, source, region, write, strength, None, control,
    )
}

/// Native selected retouch keeps canvas coverage separate through the final
/// scalar round. Legacy entry above takes the original arithmetic branch.
#[allow(clippy::too_many_arguments)]
pub(crate) fn heal_with_selection<T: HealSample, C: HealSample, S: Copy + Into<f32>>(
    canvas: (u32, u32),
    dimensions: (u32, u32),
    raw: &mut [T],
    offset: (i32, i32),
    win: (i64, i64, usize, usize),
    source: &[S],
    region: &[C],
    write: &[C],
    strength: f32,
    selection: Option<&[u16]>,
    control: JobContext<'_>,
) -> Result<bool, String> {
    heal_with_selection_channels::<T, C, S, 3>(
        canvas, dimensions, raw, offset, win, source, region, write, strength, selection, control,
    )
}

/// [`heal_with_selection`] over `N` colour channels: `raw` holds `N + 1`
/// samples a pixel with alpha last, `source` `N` a pixel. `N = 3` is the RGB
/// writeback above, statement for statement; `N = 4` heals a CMYK document's
/// four ink codes (#547, `cmyk_retouch`), each plate its own membrane solve.
#[allow(clippy::too_many_arguments)]
pub(crate) fn heal_with_selection_channels<
    T: HealSample,
    C: HealSample,
    S: Copy + Into<f32>,
    const N: usize,
>(
    canvas: (u32, u32),
    dimensions: (u32, u32),
    raw: &mut [T],
    offset: (i32, i32),
    win: (i64, i64, usize, usize),
    source: &[S],
    region: &[C],
    write: &[C],
    strength: f32,
    selection: Option<&[u16]>,
    control: JobContext<'_>,
) -> Result<bool, String> {
    if selection.is_some_and(|s| s.len() != canvas.0 as usize * canvas.1 as usize) {
        return Err("native healing selection extent mismatch".into());
    }
    control.report(0.0, 4)?;
    let (wx, wy, ww, wh) = win;
    let Some(n) = ww.checked_mul(wh) else {
        return Ok(false);
    };
    if n == 0 || !strength.is_finite() {
        return Ok(false);
    }
    // The memory bound first: it is a statement about the WINDOW, and it
    // also keeps `n * N` below from having to be a checked multiply.
    if let Some(message) = crate::doc_heal::over_window_limit(ww, wh) {
        return Err(message);
    }
    if source.len() < n * N || region.len() < n || write.len() < n {
        return Ok(false);
    }
    let strength = strength.clamp(0.0, 1.0);
    let (lw, lh) = dimensions;
    let (off_x, off_y) = (i64::from(offset.0), i64::from(offset.1));
    // Window pixel -> byte index of the layer pixel under it, or None when
    // that pixel is off-canvas or off-layer. Same canvas->layer mapping as
    // `DrDocument::painting_layer` (`doc.rs`), in i64 so an extreme offset
    // cannot wrap.
    let layer_byte = |wxi: usize, wyi: usize| -> Option<usize> {
        let cx = wx + wxi as i64;
        let cy = wy + wyi as i64;
        if cx < 0 || cy < 0 || cx >= i64::from(canvas.0) || cy >= i64::from(canvas.1) {
            return None;
        }
        let lx = cx - off_x;
        let ly = cy - off_y;
        if lx < 0 || ly < 0 || lx >= i64::from(lw) || ly >= i64::from(lh) {
            return None;
        }
        Some(((ly as u64 * u64::from(lw) + lx as u64) * (N as u64 + 1)) as usize)
    };

    let mut covered = vec![false; n];
    let mut usable = vec![false; n];
    for wyi in 0..wh {
        control.check()?;
        for wxi in 0..ww {
            let i = wyi * ww + wxi;
            control.every(i)?;
            covered[i] = region[i].number() >= C::THRESHOLD;
            // Usable = the solve may read and write this pixel: inside the
            // canvas, inside the layer, and not fully transparent.
            usable[i] = layer_byte(wxi, wyi).is_some_and(|di| raw[di + N].number() > 0.0);
        }
    }
    let solve = Region {
        w: ww,
        h: wh,
        covered: &covered,
        usable: &usable,
    };
    let max_diameter = ww.max(wh);
    let mut b = vec![0f32; n];
    let mut u = vec![0f32; n];
    let mut changed = false;
    for channel in 0..N {
        control.report(channel as f64 / N as f64, 4)?;
        for wyi in 0..wh {
            control.check()?;
            for wxi in 0..ww {
                let i = wyi * ww + wxi;
                control.every(i)?;
                b[i] = 0.0;
                if !(covered[i] && usable[i]) {
                    continue;
                }
                if let Some(di) = layer_byte(wxi, wyi) {
                    b[i] = raw[di + channel].number() - source[i * N + channel].into();
                }
            }
        }
        u.fill(0.0);
        T::solve(&solve, &b, max_diameter, &mut u, control)?;
        for wyi in 0..wh {
            control.check()?;
            for wxi in 0..ww {
                let i = wyi * ww + wxi;
                control.every(i)?;
                if !(covered[i] && usable[i]) {
                    continue;
                }
                let weight = write[i].number() / C::MAX * strength;
                if weight <= 0.0 {
                    continue;
                }
                let Some(di) = layer_byte(wxi, wyi) else {
                    continue;
                };
                let old = raw[di + channel];
                let healed = source[i * N + channel].into() + u[i];
                let out = old.number() + weight * (healed - old.number());
                // Round half-up, clamping only here: `source + u` may leave
                // [0, 255] where the data genuinely does.
                let new = if let Some(selection) = selection {
                    let cx = wx + wxi as i64;
                    let cy = wy + wyi as i64;
                    let coverage =
                        f64::from(selection[cy as usize * canvas.0 as usize + cx as usize])
                            / 65535.;
                    T::quantize_precise(
                        f64::from(old.number())
                            + f64::from(weight) * coverage * f64::from(healed - old.number()),
                    )
                } else {
                    T::quantize(out)
                };
                if new != old {
                    raw[di + channel] = new;
                    changed = true;
                }
            }
        }
    }
    control.report(1.0, 4)?;
    Ok(changed)
}
