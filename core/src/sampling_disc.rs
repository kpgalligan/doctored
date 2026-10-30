//! Lens Blur and Tilt-Shift: a weighted premultiplied mean over a
//! pixel-centred disc whose radius may vary per pixel, two `SpatialKernel` arms
//! on the one spatial route (`SelectedFilter::Spatial` at 16 bits, `filter_op`
//! at 8; no new export).
//!
//! The disc of integer radius `r` is `D_r = {(dx, dy) : dx² + dy² ≤ (r + ½)²}`
//! (so `D_0` is the pixel itself), read with clamped plane edges. It is summed
//! as ROW SPANS: row `dy` of the disc is the columns `−hw..=hw` with
//! `hw = ⌊√((r + ½)² − dy²)⌋`, and a span's sum is one subtraction of two
//! entries of that source row's prefix sums. The prefix sums live in a ring of
//! `2R + 1` rows (`R` the largest radius), each extended `R` columns past both
//! plane edges with the edge pixel, so a span never needs a clamp of its own.
//!
//! Each pixel `q` carries a weight `w(q)`, and the mean is `Σ w·P / Σ w` with
//! `P` the premultiplied sample. Without a highlight `w = 1`: a plain disc
//! mean, which is all Tilt-Shift uses. Lens Blur's specular highlights are
//! `w = 1 + (brightness/100) · LENS_HIGHLIGHT_GAIN · clamp((Y − t) / max(1 − t,
//! 1/255), 0, 1)`, `t = threshold/255`, `Y` the luma (`blend::LUMA_*`) of the
//! PREMULTIPLIED colour: for an opaque pixel that is its own luma, and a
//! transparent pixel, whose stored colour is latent, weighs exactly 1.
//!
//! A fractional radius `ρ` is `lerp(mean(D_⌊ρ⌋), mean(D_⌈ρ⌉), frac ρ)`, so a
//! radius field that varies smoothly (Tilt-Shift's transition) gives a result
//! that does. Alpha is filtered, as in Box Blur, whose combine and transparent
//! floor this shares. Neither filter takes a frame.
use crate::blend::{LUMA_B, LUMA_G, LUMA_R};
use crate::job_control::JobContext;
use crate::precision_image::{pixel_count, reserved_vec, BOOKKEEPING_BYTES};
use crate::sampling_radial::{premultiplied, RADIAL_BILINEAR_WORK_PER_SAMPLE};

/// How far above 1 a pixel at full highlight weighs at Brightness 100: nine
/// times an ordinary pixel, so a specular point a few pixels wide still shows
/// as a disc after it is spread over a radius-15 disc of ~700 pixels, while
/// Brightness 0 is exactly the unweighted mean. Our own shaping, not a fit.
pub(crate) const LENS_HIGHLIGHT_GAIN: f64 = 8.0;

/// The disc kernel's per-pixel plan term: it holds the n×16 output plane; the
/// ring and the span table are charged separately (`ring_bytes`).
///
/// Record (`sampling_motion`'s `MOTION_BLUR_BYTES_PER_PIXEL` harness and
/// document, radius 100, where the kernel's plan is 436,034,396, below the
/// combine stage's 24n at 16 bits and 20n at 8, so the term does not bind):
/// - Lens Blur (brightness 50, threshold 200), 16-bit: E 583,778,712 /
///   583,451,032 / 583,664,024 against `selected_edit_plan` 822,291,456
///   (1.409×). 8-bit: E 871,563,888 / 871,187,008 / 871,432,816 against the
///   host's commit stage 1,020,000,000 + 96,003,794 retained (1.280×).
/// - Tilt-Shift, 16-bit: E 579,862,936 / 579,666,328 / 579,781,016 (1.418×).
///   8-bit: E 852,771,440 / 843,301,464 / 843,203,160 against 1,020,000,000 +
///   71,427,794 retained (1.280×; the focus band's tiles stay shared).
const DISC_BYTES_PER_PIXEL: u64 = 16;

/// One prefix-sum entry: the four weighted premultiplied channels, then the
/// weight.
type Sums = [f64; 5];

/// The ring of `2R + 1` extended prefix rows plus the span table.
fn ring_bytes(width: u64, radius: u64) -> u64 {
    let rows = 2 * radius + 1;
    rows * (width + 2 * radius + 1) * std::mem::size_of::<Sums>() as u64 + (radius + 1) * rows * 4
}

/// `(scratch, work)`, refused before any allocation: `max_radius` outside
/// `1..=100`, or `n·(2R + 1)` spans of two prefix reads above `cap`.
pub(crate) fn disc_plan(
    dimensions: (u32, u32),
    max_radius: u32,
    cap: u64,
) -> Result<(u64, u64), String> {
    let n = pixel_count(dimensions.0, dimensions.1)? as u64;
    if !(1..=100).contains(&max_radius) {
        return Err("disc blur radius must be an integer in [1,100]".into());
    }
    let radius = u64::from(max_radius);
    let work = n
        .saturating_mul(2 * radius + 1)
        .saturating_mul(2 * RADIAL_BILINEAR_WORK_PER_SAMPLE);
    if work > cap {
        return Err("disc blur exceeds the bounded pixel-work limit".into());
    }
    Ok((
        n * DISC_BYTES_PER_PIXEL
            + ring_bytes(u64::from(dimensions.0), radius)
            + 2 * BOOKKEEPING_BYTES,
        work,
    ))
}

/// The plane: at each pixel the weighted premultiplied mean over the disc of
/// radius `radius_at(x, y)` (clamped to `0..=max_radius`; a non-finite radius
/// is 0), below `alpha_floor` of alpha transparent black (`box_plane`'s
/// floor: half a 16-bit code at 8 and 16 bits, 2^-24 in a float layer;
/// `filter_kernels::SampleRange::alpha_floor`). `highlight` is Lens Blur's
/// `(brightness 0..=100, threshold in 8-bit
/// levels 0..=255)`, or `None` for the unweighted mean. `ops_spatial`'s Box
/// Blur combine turns the result back to straight samples.
///
/// In a float layer the highlight weight keeps its formula on the stored
/// LINEAR luma with `t = threshold / 255`: the ramp is clamped to 0…1, so a
/// pixel above white takes the full weight at any threshold, which is the
/// point of the control (the brightest lights bloom into discs), and a
/// threshold of 255 still means "white and above".
pub(crate) fn disc_plane(
    read: impl Fn(usize) -> [f32; 4],
    dimensions: (u32, u32),
    radius_at: impl Fn(usize, usize) -> f64,
    max_radius: u32,
    highlight: Option<(f64, f64)>,
    alpha_floor: f64,
    job: JobContext<'_>,
) -> Result<Vec<[f32; 4]>, String> {
    let (w, h) = (dimensions.0 as usize, dimensions.1 as usize);
    let reach = max_radius as usize;
    let rows = 2 * reach + 1;
    let stride = w + 2 * reach + 1;
    // spans[r·rows + (dy + R)] is the half-width of row dy of D_r (unused, 0,
    // where |dy| > r). The argument of the root is an integer plus ¼, never a
    // perfect square, so the correctly rounded root floors exactly.
    let mut spans: Vec<u32> = reserved_vec((reach + 1) * rows)?;
    for r in 0..=reach {
        let limit = (r as f64 + 0.5) * (r as f64 + 0.5);
        for dy in -(reach as i64)..=reach as i64 {
            spans.push(if dy.unsigned_abs() as usize > r {
                0
            } else {
                (limit - (dy * dy) as f64).sqrt().floor() as u32
            });
        }
    }
    let weight = |p: [f32; 4]| -> f64 {
        let Some((brightness, threshold)) = highlight else {
            return 1.0;
        };
        let y = f64::from(LUMA_R) * f64::from(p[0])
            + f64::from(LUMA_G) * f64::from(p[1])
            + f64::from(LUMA_B) * f64::from(p[2]);
        let t = threshold / 255.0;
        1.0 + brightness / 100.0
            * LENS_HIGHLIGHT_GAIN
            * ((y - t) / (1.0 - t).max(1.0 / 255.0)).clamp(0.0, 1.0)
    };
    let mut ring: Vec<Sums> = reserved_vec(rows * stride)?;
    ring.resize(rows * stride, [0.0; 5]);
    let mut loaded = 0_usize;
    let mut out = reserved_vec(w * h)?;
    for y in 0..h {
        job.check()?;
        // Source rows up to y + R, each built once into its ring slot.
        while loaded <= (y + reach).min(h - 1) {
            let row = &mut ring[(loaded % rows) * stride..(loaded % rows + 1) * stride];
            let mut running = [0.0_f64; 5];
            row[0] = running;
            for j in 0..stride - 1 {
                let x = (j as i64 - reach as i64).clamp(0, w as i64 - 1) as usize;
                let p = premultiplied(read(loaded * w + x));
                let k = weight(p);
                for c in 0..4 {
                    running[c] += k * f64::from(p[c]);
                }
                running[4] += k;
                row[j + 1] = running;
            }
            loaded += 1;
        }
        let disc = |x: usize, r: usize| -> [f64; 4] {
            if r == 0 {
                return premultiplied(read(y * w + x)).map(f64::from);
            }
            let mut acc = [0.0_f64; 5];
            for dy in -(r as i64)..=r as i64 {
                let source = (y as i64 + dy).clamp(0, h as i64 - 1) as usize;
                let hw = spans[r * rows + (dy + reach as i64) as usize] as usize;
                let at = (source % rows) * stride + x + reach;
                let (low, high) = (ring[at - hw], ring[at + hw + 1]);
                for c in 0..5 {
                    acc[c] += high[c] - low[c];
                }
            }
            std::array::from_fn(|c| acc[c] / acc[4])
        };
        for x in 0..w {
            job.every(y * w + x)?;
            let rho = radius_at(x, y);
            let rho = if rho.is_finite() {
                rho.clamp(0.0, reach as f64)
            } else {
                0.0
            };
            let lower = rho.floor();
            let t = rho - lower;
            let mut mean = disc(x, lower as usize);
            if t > 0.0 {
                let upper = disc(x, lower as usize + 1);
                for c in 0..4 {
                    mean[c] += t * (upper[c] - mean[c]);
                }
            }
            out.push(if mean[3] < alpha_floor {
                [0.0; 4]
            } else {
                mean.map(|v| v as f32)
            });
        }
        job.report((y + 1) as f64 / h as f64, 6)?;
    }
    Ok(out)
}
