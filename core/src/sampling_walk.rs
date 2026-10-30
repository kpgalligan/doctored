//! Shared spatial walks. Storage adapters supply premultiplication and final
//! quantization; coordinates, tap order, weights and edge policy have one home.
use super::{resize_weights, Kernel, MAX_TAPS};
use crate::doc_transform::{covering_pixel, visible, Sampler, SourceMap};
use crate::job_control::JobContext;
use crate::warp::WarpField;

/// A warp keeps the old source-map walk untouched, but shares its tap order,
/// transparent edge policy, and kernel with a binary64 arithmetic adapter.
#[allow(clippy::too_many_arguments)]
pub(crate) fn warp_into<P: Copy, const C: usize>(
    source: (i32, i32, u32, u32),
    destination: (i32, i32, u32, u32),
    field: &WarpField,
    read: impl Fn(usize) -> P,
    accumulate: impl Fn(&mut [f64; C], P, f64),
    finish: impl Fn([f64; C]) -> P,
    write: impl FnMut(usize, P),
    job: JobContext<'_>,
) -> Result<(), String> {
    let (sx, sy, _, _) = source;
    let (dx, dy, _, _) = destination;
    map_into(
        (source.2, source.3),
        (destination.2, destination.3),
        field.filter.kernel(),
        |x, y, job| {
            let p = field.inverse(source, [f64::from(dx) + x, f64::from(dy) + y], job)?;
            Ok([p[0] - f64::from(sx), p[1] - f64::from(sy)])
        },
        read,
        accumulate,
        finish,
        write,
        job,
    )
}

/// Shared binary64 tap walk for persisted warps and RAW source maps.
#[allow(clippy::too_many_arguments)]
pub(crate) fn map_into<P: Copy, Q, const C: usize>(
    source: (u32, u32),
    destination: (u32, u32),
    kernel: Kernel,
    map: impl Fn(f64, f64, JobContext<'_>) -> Result<[f64; 2], String>,
    read: impl Fn(usize) -> P,
    accumulate: impl Fn(&mut [f64; C], P, f64),
    finish: impl Fn([f64; C]) -> Q,
    mut write: impl FnMut(usize, Q),
    job: JobContext<'_>,
) -> Result<(), String> {
    let taps = kernel.taps();
    let (sw, sh) = source;
    let (dw, dh) = destination;
    let mut wx = [0.0; MAX_TAPS];
    let mut wy = [0.0; MAX_TAPS];
    for y in 0..dh {
        job.check()?;
        for x in 0..dw {
            job.every(x as usize)?;
            let [u, v] = map(f64::from(x) + 0.5, f64::from(y) + 0.5, job)?;
            let x0 = kernel.weights_f64(u, &mut wx);
            let y0 = kernel.weights_f64(v, &mut wy);
            let mut acc = [0.0; C];
            for (ty, &wyi) in wy.iter().enumerate().take(taps) {
                let iy = y0 + ty as i64;
                if wyi == 0.0 || iy < 0 || iy >= i64::from(sh) {
                    continue;
                }
                for (tx, &wxi) in wx.iter().enumerate().take(taps) {
                    let ix = x0 + tx as i64;
                    if wxi == 0.0 || ix < 0 || ix >= i64::from(sw) {
                        continue;
                    }
                    accumulate(
                        &mut acc,
                        read(iy as usize * sw as usize + ix as usize),
                        wyi * wxi,
                    );
                }
            }
            write(y as usize * dw as usize + x as usize, finish(acc));
        }
        job.report(f64::from(y + 1) / f64::from(dh), 6)?;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn resample_into<P: Copy, const C: usize>(
    source: (u32, u32),
    destination: (u32, u32),
    map: &SourceMap,
    sampler: Sampler,
    read: impl Fn(usize) -> P,
    accumulate: impl Fn(&mut [f32; C], P, f32),
    finish: impl Fn([f32; C]) -> P,
    write: impl FnMut(usize, P),
    report_progress: bool,
    precise_weights: bool,
    job: JobContext<'_>,
) -> Result<(), String> {
    resample_rows_into(
        source,
        destination,
        0..destination.1,
        map,
        sampler,
        read,
        accumulate,
        finish,
        write,
        report_progress,
        precise_weights,
        job,
    )
}

/// Rows `rows` of [`resample_into`]'s walk, its ONE body: the same per-row
/// checkpoint, `map.row_start(j)` (absolute, so a row computes identically
/// whatever range it is walked in), per-pixel `job.every(i)`, the absolute
/// destination index `di = j·dw + i` handed to `write`, and the report
/// `(j + 1)/dh` at stage 6 when `report_progress`. The uniform resample
/// (`precision_sampling::transform_image`'s uniform-source branch) walks one
/// row at a time into a tile band.
#[allow(clippy::too_many_arguments)]
pub(crate) fn resample_rows_into<P: Copy, const C: usize>(
    source: (u32, u32),
    destination: (u32, u32),
    rows: std::ops::Range<u32>,
    map: &SourceMap,
    sampler: Sampler,
    read: impl Fn(usize) -> P,
    accumulate: impl Fn(&mut [f32; C], P, f32),
    finish: impl Fn([f32; C]) -> P,
    mut write: impl FnMut(usize, P),
    report_progress: bool,
    precise_weights: bool,
    job: JobContext<'_>,
) -> Result<(), String> {
    let (sw, sh) = source;
    let (dw, dh) = destination;
    let mut wx = [0.0; MAX_TAPS];
    let mut wy = [0.0; MAX_TAPS];
    for j in rows {
        job.check()?;
        let (mut nu, mut nv, mut nw) = map.row_start(j);
        for i in 0..dw {
            job.every(i as usize)?;
            let (u, v) = (nu / nw, nv / nw);
            if visible(u, v, nw) {
                let di = j as usize * dw as usize + i as usize;
                match sampler {
                    Sampler::Nearest => {
                        if let Some(si) = covering_pixel(u, v, sw, sh) {
                            write(di, read(si));
                        }
                    }
                    Sampler::Kernel(kernel) => {
                        let taps = kernel.taps();
                        let x0 = if precise_weights {
                            kernel.weights_precise(u, &mut wx)
                        } else {
                            kernel.weights(u, &mut wx)
                        };
                        let y0 = if precise_weights {
                            kernel.weights_precise(v, &mut wy)
                        } else {
                            kernel.weights(v, &mut wy)
                        };
                        let mut acc = [0.0; C];
                        for (ty, &wyt) in wy.iter().enumerate().take(taps) {
                            let sy = y0 + ty as i64;
                            if wyt == 0.0 || sy < 0 || sy >= i64::from(sh) {
                                continue;
                            }
                            let row = sy as usize * sw as usize;
                            for (tx, &wxt) in wx.iter().enumerate().take(taps) {
                                let sx = x0 + tx as i64;
                                if wxt == 0.0 || sx < 0 || sx >= i64::from(sw) {
                                    continue;
                                }
                                accumulate(&mut acc, read(row + sx as usize), wyt * wxt);
                            }
                        }
                        write(di, finish(acc));
                    }
                }
            }
            nu += map.step.0;
            nv += map.step.1;
            nw += map.step.2;
        }
        if report_progress {
            job.report(f64::from(j + 1) / f64::from(dh), 6)?;
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn resize_into<P: Copy, const C: usize>(
    source: (u32, u32),
    destination: (u32, u32),
    kernel: Kernel,
    read: impl Fn(usize) -> P,
    accumulate: impl Fn(&mut [f32; C], P, f32),
    finish: impl Fn([f32; C]) -> P,
    mut write: impl FnMut(usize, P),
    report_progress: bool,
    job: JobContext<'_>,
) -> Result<(), String> {
    let (sw, sh) = source;
    let (w, h) = destination;
    let mut intermediate = vec![[0.0; C]; sw as usize * h as usize];
    let mut weights = Vec::new();
    for y in 0..h {
        job.check()?;
        let first = resize_weights(kernel, sh, h, y, &mut weights, job)?;
        for x in 0..sw {
            job.every(x as usize)?;
            let acc = &mut intermediate[y as usize * sw as usize + x as usize];
            for (tap, &weight) in weights.iter().enumerate() {
                job.every(tap)?;
                accumulate(
                    acc,
                    read((first as usize + tap) * sw as usize + x as usize),
                    weight,
                );
            }
        }
        if report_progress {
            job.report(0.5 * f64::from(y + 1) / f64::from(h), 6)?;
        }
    }
    for x in 0..w {
        job.check()?;
        let first = resize_weights(kernel, sw, w, x, &mut weights, job)?;
        for y in 0..h {
            job.every(y as usize)?;
            let mut acc = [0.0; C];
            for (tap, &weight) in weights.iter().enumerate() {
                job.every(tap)?;
                let sample = intermediate[y as usize * sw as usize + first as usize + tap];
                for c in 0..C {
                    acc[c] += sample[c] * weight;
                }
            }
            write(y as usize * w as usize + x as usize, finish(acc));
        }
        if report_progress {
            job.report(0.5 + 0.5 * f64::from(x + 1) / f64::from(w), 6)?;
        }
    }
    Ok(())
}
