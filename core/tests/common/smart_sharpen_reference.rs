//! An independent binary64 reference of Smart Sharpen at Reduce Noise
//! 0, written from the rules the core states (`sampling_smart_sharpen`'s
//! module doc and the three kernels' docs), not from their code:
//!
//! - Gaussian: the normalized `ceil(3σ)` FIR (`sampling_gaussian`'s module
//!   doc), separable, over premultiplied samples with clamped edges.
//! - Lens: the integer discs `D_r`, row `dy` spanning `|dx| ≤ ⌊√((r + ½)² −
//!   dy²)⌋` (`sampling_disc`), clamped edges, premultiplied means; a fractional
//!   radius `ρ` is `mean(D_⌊ρ⌋) + (ρ − ⌊ρ⌋)·(mean(D_⌊ρ⌋+1) − mean(D_⌊ρ⌋))`; a
//!   mean alpha below half a 16-bit code is transparent black.
//! - Motion: the equal mean of `d + 1` bilinear premultiplied samples
//!   (`d = max(1, round(2·radius))`) at `p + (k − d/2)·(cos θ, −sin θ)`, `p`
//!   the pixel centre, the tent over the four nearest pixel centres with
//!   clamped edges (`sampling_motion`), the same floor.
//! - The colour: `clamp(D + a·w·(D − straight(B)), 0, 1)` per channel for a
//!   visible pixel, `a = amount/100`; `w` the fade weight over the
//!   alpha-weighted clamped-edge box mean of the Rec. 709 luma (clamped to
//!   0…1); Lens then holds each channel within 2/255 of the least and greatest
//!   of `D` over the clamped 3×3 window's visible pixels. Alpha is the
//!   source's; an alpha-0 pixel is returned as read.

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Remove {
    Gaussian,
    Lens,
    Motion,
}

/// `(fade, tonal_width, radius)` of one section, in the filter's units.
pub type Section = (f64, f64, u32);

#[derive(Clone, Copy, Debug)]
pub struct Settings {
    pub amount: f64,
    pub radius: f64,
    pub remove: Remove,
    pub angle: f64,
    pub shadows: Section,
    pub highlights: Section,
}

const FLOOR: f64 = 0.5 / 65535.0;
const OVERSHOOT: f64 = 2.0 / 255.0;

fn premultiplied(p: [f64; 4]) -> [f64; 4] {
    [p[0] * p[3], p[1] * p[3], p[2] * p[3], p[3]]
}

fn clamped(v: i64, len: usize) -> usize {
    v.clamp(0, len as i64 - 1) as usize
}

fn gaussian(src: &[[f64; 4]], w: usize, h: usize, sigma: f64) -> Vec<[f64; 4]> {
    let half = (3.0 * sigma).ceil() as i64;
    let raw: Vec<f64> = (-half..=half)
        .map(|k| (-(k * k) as f64 / (2.0 * sigma * sigma)).exp())
        .collect();
    let total: f64 = raw.iter().sum();
    let taps: Vec<f64> = raw.iter().map(|t| t / total).collect();
    let pm: Vec<[f64; 4]> = src.iter().map(|p| premultiplied(*p)).collect();
    let mut across = vec![[0.0; 4]; w * h];
    for y in 0..h {
        for x in 0..w {
            let mut acc = [0.0; 4];
            for (j, t) in taps.iter().enumerate() {
                let sx = clamped(x as i64 + j as i64 - half, w);
                for c in 0..4 {
                    acc[c] += t * pm[y * w + sx][c];
                }
            }
            across[y * w + x] = acc;
        }
    }
    let mut out = vec![[0.0; 4]; w * h];
    for y in 0..h {
        for x in 0..w {
            let mut acc = [0.0; 4];
            for (j, t) in taps.iter().enumerate() {
                let sy = clamped(y as i64 + j as i64 - half, h);
                for c in 0..4 {
                    acc[c] += t * across[sy * w + x][c];
                }
            }
            out[y * w + x] = acc;
        }
    }
    out
}

fn disc_mean(src: &[[f64; 4]], w: usize, h: usize, x: usize, y: usize, r: usize) -> [f64; 4] {
    let mut acc = [0.0; 4];
    let mut count = 0.0;
    let limit = (r as f64 + 0.5) * (r as f64 + 0.5);
    for dy in -(r as i64)..=r as i64 {
        let hw = (limit - (dy * dy) as f64).sqrt().floor() as i64;
        let sy = clamped(y as i64 + dy, h);
        for dx in -hw..=hw {
            let sx = clamped(x as i64 + dx, w);
            let p = premultiplied(src[sy * w + sx]);
            for c in 0..4 {
                acc[c] += p[c];
            }
            count += 1.0;
        }
    }
    acc.map(|v| v / count)
}

fn lens(src: &[[f64; 4]], w: usize, h: usize, radius: f64) -> Vec<[f64; 4]> {
    let lower = radius.floor();
    let t = radius - lower;
    let mut out = Vec::with_capacity(w * h);
    for y in 0..h {
        for x in 0..w {
            let mut mean = disc_mean(src, w, h, x, y, lower as usize);
            if t > 0.0 {
                let upper = disc_mean(src, w, h, x, y, lower as usize + 1);
                for c in 0..4 {
                    mean[c] += t * (upper[c] - mean[c]);
                }
            }
            out.push(if mean[3] < FLOOR { [0.0; 4] } else { mean });
        }
    }
    out
}

fn bilinear(src: &[[f64; 4]], w: usize, h: usize, qx: f64, qy: f64) -> [f64; 4] {
    let (sx, sy) = (qx - 0.5, qy - 0.5);
    let (x0, y0) = (sx.floor(), sy.floor());
    let (fx, fy) = (sx - x0, sy - y0);
    let (x0, y0) = (x0 as i64, y0 as i64);
    let at = |x: i64, y: i64| premultiplied(src[clamped(y, h) * w + clamped(x, w)]);
    let (p00, p01, p10, p11) = (
        at(x0, y0),
        at(x0 + 1, y0),
        at(x0, y0 + 1),
        at(x0 + 1, y0 + 1),
    );
    std::array::from_fn(|c| {
        p00[c] * (1.0 - fx) * (1.0 - fy)
            + p01[c] * fx * (1.0 - fy)
            + p10[c] * (1.0 - fx) * fy
            + p11[c] * fx * fy
    })
}

fn motion(src: &[[f64; 4]], w: usize, h: usize, radius: f64, angle: f64) -> Vec<[f64; 4]> {
    let d = ((2.0 * radius).round() as u32).max(1);
    let (sin, cos) = angle.to_radians().sin_cos();
    let mut out = Vec::with_capacity(w * h);
    for y in 0..h {
        for x in 0..w {
            let mut acc = [0.0; 4];
            for k in 0..=d {
                let t = f64::from(k) - f64::from(d) / 2.0;
                let p = bilinear(
                    src,
                    w,
                    h,
                    x as f64 + 0.5 + t * cos,
                    y as f64 + 0.5 - t * sin,
                );
                for c in 0..4 {
                    acc[c] += p[c];
                }
            }
            let mean = acc.map(|v| v / f64::from(d + 1));
            out.push(if mean[3] < FLOOR { [0.0; 4] } else { mean });
        }
    }
    out
}

/// The Rec. 709 luma, clamped to 0…1.
pub fn luma(p: [f64; 4]) -> f64 {
    (0.2126 * p[0] + 0.7152 * p[1] + 0.0722 * p[2]).clamp(0.0, 1.0)
}

/// The alpha-weighted clamped-edge box mean of the luma at half-width `r`
/// (0 where the window holds no alpha).
pub fn tone(src: &[[f64; 4]], w: usize, h: usize, r: u32) -> Vec<f64> {
    let r = i64::from(r);
    let mut out = Vec::with_capacity(w * h);
    for y in 0..h {
        for x in 0..w {
            let (mut num, mut den) = (0.0, 0.0);
            for dy in -r..=r {
                for dx in -r..=r {
                    let p = src[clamped(y as i64 + dy, h) * w + clamped(x as i64 + dx, w)];
                    num += p[3] * luma(p);
                    den += p[3];
                }
            }
            out.push(if den > 0.0 {
                (num / den).clamp(0.0, 1.0)
            } else {
                0.0
            });
        }
    }
    out
}

/// One section's factor: 1 at fade 0 or width 0.
fn factor(section: Section, ramp: f64) -> f64 {
    let (f, t) = (section.0 / 100.0, section.1 / 100.0);
    if f == 0.0 || t == 0.0 {
        1.0
    } else {
        1.0 - f * ramp.clamp(0.0, 1.0)
    }
}

/// Smart Sharpen at Reduce Noise 0 over straight unit samples.
pub fn smart_sharpen(src: &[[f64; 4]], w: usize, h: usize, s: &Settings) -> Vec<[f64; 4]> {
    let low = match s.remove {
        Remove::Gaussian => gaussian(src, w, h, s.radius),
        Remove::Lens => lens(src, w, h, s.radius),
        Remove::Motion => motion(src, w, h, s.radius, s.angle),
    };
    let active = |section: Section| section.0 > 0.0 && section.1 > 0.0;
    let shadow = active(s.shadows).then(|| tone(src, w, h, s.shadows.2));
    let highlight = active(s.highlights).then(|| tone(src, w, h, s.highlights.2));
    let mut out = Vec::with_capacity(w * h);
    for y in 0..h {
        for x in 0..w {
            let i = y * w + x;
            let d = src[i];
            if d[3] <= 0.0 {
                out.push(d);
                continue;
            }
            let ls = shadow.as_ref().map_or(0.0, |l| l[i]);
            let lh = highlight.as_ref().map_or(0.0, |l| l[i]);
            let ts = s.shadows.1 / 100.0;
            let th = s.highlights.1 / 100.0;
            let weight = factor(s.shadows, if ts > 0.0 { 1.0 - ls / ts } else { 0.0 })
                * factor(
                    s.highlights,
                    if th > 0.0 { 1.0 - (1.0 - lh) / th } else { 0.0 },
                );
            let b = low[i];
            let straight = if b[3] > 0.0 {
                [b[0] / b[3], b[1] / b[3], b[2] / b[3]]
            } else {
                [0.0; 3]
            };
            let a = s.amount / 100.0 * weight;
            let mut p = d;
            for c in 0..3 {
                p[c] = (d[c] + a * (d[c] - straight[c])).clamp(0.0, 1.0);
            }
            if s.remove == Remove::Lens {
                let (mut lo, mut hi) = ([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]);
                for dy in -1..=1_i64 {
                    for dx in -1..=1_i64 {
                        let q = src[clamped(y as i64 + dy, h) * w + clamped(x as i64 + dx, w)];
                        if q[3] <= 0.0 {
                            continue;
                        }
                        for c in 0..3 {
                            lo[c] = lo[c].min(q[c]);
                            hi[c] = hi[c].max(q[c]);
                        }
                    }
                }
                for c in 0..3 {
                    p[c] = p[c].clamp(lo[c] - OVERSHOOT, hi[c] + OVERSHOOT);
                }
            }
            out.push(p);
        }
    }
    out
}
